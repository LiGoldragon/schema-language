use nota::{Block, Delimiter, Document, StructuralMacroNode};

use crate::{
    MacroContext, MacroObject, MacroOutput, MacroPair, MacroPosition, MacroRegistry, SchemaError,
    macros::SchemaBlockExt, source::SchemaDocumentLayout,
};

/// The c2dc front-end pass: the NOTA decoder, once parsed into a document, is
/// extended by the registry of registered macros dispatched as an ordered
/// list (first match wins). This pass runs that dispatch over the parsed
/// document BEFORE [`crate::SchemaSource::from_document`] builds the rkyv
/// archive, so the archive the single source-path lowering reads is already
/// macro-expanded.
///
/// Two jobs, both feeding the one downstream lowering:
///
/// 1. It rewrites user type-reference macro invocations — `(Bag Topic)` with a
///    registered `Bag` macro becomes its expanded source reference
///    `Vector.Topic`, recursively — so the source path sees only built-in heads
///    it already understands. The expansion reuses the registered macro's own
///    capture/substitution machinery (the `DeclarativeSchemaMacro` handler),
///    so `$`-sigil captures bind and substitute exactly as the macro declares.
/// 2. It records every structural root macro firing (`RootImports`,
///    `RootInput`, `RootOutput`, `RootNamespace`, `KeyValueDeclaration`) and
///    every capture binding into the [`MacroContext`]. The structural macros
///    do not rewrite the tree — the source path re-derives roots and namespace
///    declarations — but the registry IS the dispatch layer, so a fired macro
///    is recorded as fired.
///
/// The pass does not re-home the retired rival lowering: it never builds a
/// `TrueSchema`. It produces an expanded [`Document`] string and hands it back to
/// the single source path, preserving the collapse's single-semantics
/// property.
#[derive(Clone, Copy)]
pub(crate) struct MacroExpansionPass<'registry> {
    registry: &'registry MacroRegistry,
}

impl<'registry> MacroExpansionPass<'registry> {
    pub(crate) fn new(registry: &'registry MacroRegistry) -> Self {
        Self { registry }
    }

    /// Run the pass over a parsed document. The returned document is the
    /// macro-expanded re-parse; `context` accumulates the recorded firings and
    /// bindings. The entry contract is the strict six-slot document layout
    /// shared with the source path; grouped dotted root applications occupy one
    /// typed slot even when raw NOTA currently parses them as two blocks.
    pub(crate) fn expand(
        &self,
        document: &Document,
        context: &mut MacroContext,
    ) -> Result<Document, SchemaError> {
        let layout = SchemaDocumentLayout::from_document(document)?;
        self.record_root_firings(document, &layout, context)?;
        let expanded_roots = document
            .root_objects()
            .iter()
            .map(|root| self.expand_block(root, context))
            .collect::<Result<Vec<_>, _>>()?;
        Document::parse(expanded_roots.join("\n")).map_err(SchemaError::from)
    }

    /// Record the structural root macros that the registry dispatches at the
    /// document's positional slots. Recording asks the ordered macro list which
    /// macro fires at each slot (first match wins) and remembers it, without
    /// running the macro's lowering — the source path owns the actual lowering.
    fn record_root_firings(
        &self,
        document: &Document,
        layout: &SchemaDocumentLayout,
        context: &mut MacroContext,
    ) -> Result<(), SchemaError> {
        self.record_block_firing(
            layout.imports().block(document),
            MacroPosition::RootImports,
            context,
        );
        self.record_block_firing(
            layout.input().block(document),
            MacroPosition::RootInput,
            context,
        );
        self.record_block_firing(
            layout.output().block(document),
            MacroPosition::RootOutput,
            context,
        );
        // The former single namespace slot is now three per-kind declaration
        // blocks (types, generics, impls). The `RootNamespace` macro position
        // still names the declaration region; it is recorded on the `types`
        // block — the type namespace the position always described — and the
        // per-declaration `NamespaceDeclaration` walk covers the type and
        // generic declaration blocks, whose entries mint declarations.
        let types = layout.types().block(document);
        self.record_block_firing(types, MacroPosition::RootNamespace, context);
        self.record_namespace_declaration_firings(types, context)?;
        self.record_namespace_declaration_firings(layout.generics().block(document), context)?;
        Ok(())
    }

    fn record_block_firing(
        &self,
        block: &Block,
        position: MacroPosition,
        context: &mut MacroContext,
    ) {
        if let Some(name) = self
            .registry
            .matching_macro_name(MacroObject::Block(block), position)
        {
            context.remember_macro(name.to_owned());
            context.remember_position(position);
        }
    }

    /// Record one `NamespaceDeclaration` firing per per-kind declaration entry
    /// the registry dispatches. The entry segmentation mirrors the source
    /// path's per-kind block reader (a capitalized dotted key split off the
    /// leading atom, then the value block sequence walked by width) so the
    /// recorded firings match the declarations the source path lowers.
    fn record_namespace_declaration_firings(
        &self,
        block: &Block,
        context: &mut MacroContext,
    ) -> Result<(), SchemaError> {
        let Block::Delimited {
            delimiter: Delimiter::Brace,
            root_objects,
            ..
        } = block
        else {
            return Ok(());
        };
        let mut walk = NamespacePairWalk::new(root_objects);
        while let Some(pair) = walk.next_pair() {
            if let Some(name) = self
                .registry
                .matching_macro_name(MacroObject::Pair(pair), MacroPosition::NamespaceDeclaration)
            {
                context.remember_macro(name.to_owned());
                context.remember_position(MacroPosition::NamespaceDeclaration);
            }
        }
        Ok(())
    }

    /// Re-emit one block as NOTA, expanding any user type-reference macro
    /// invocation it (or a descendant) is. A parenthesis whose head matches a
    /// registered type-reference macro lowers through that macro's own
    /// capture/substitution and re-emits as its expanded body; every other
    /// block re-emits faithfully, recursing into its children so a nested
    /// invocation still expands.
    fn expand_block(
        &self,
        block: &Block,
        context: &mut MacroContext,
    ) -> Result<String, SchemaError> {
        if block.is_parenthesis()
            && self
                .registry
                .matching_macro_name(MacroObject::Block(block), MacroPosition::TypeReference)
                .is_some()
        {
            return self.expand_type_reference_macro(block, context);
        }
        match block {
            Block::Delimited {
                delimiter,
                root_objects,
                ..
            } => {
                let children = root_objects
                    .iter()
                    .map(|child| self.expand_block(child, context))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(DelimiterText::new(*delimiter).wrap(&children))
            }
            Block::PipeText(pipe_text) => Ok(format!("[|{}|]", pipe_text.text)),
            Block::Application { head, payload, .. } => Ok(format!(
                "{}.{}",
                self.expand_block(head, context)?,
                self.expand_block(payload, context)?
            )),
            Block::Atom(atom) => Ok(atom.text().to_owned()),
        }
    }

    fn expand_type_reference_macro(
        &self,
        block: &Block,
        context: &mut MacroContext,
    ) -> Result<String, SchemaError> {
        match self.registry.lower(
            MacroObject::Block(block),
            MacroPosition::TypeReference,
            context,
        )? {
            MacroOutput::Reference(reference) => Ok(reference.to_structural_nota()),
            _ => Err(SchemaError::UnexpectedMacroOutput {
                macro_name: block
                    .root_object_at(0)
                    .and_then(|head| head.schema_name().ok())
                    .map(|name| name.as_str().to_owned())
                    .unwrap_or_else(|| "type reference macro".to_owned()),
                expected: "type reference",
            }),
        }
    }
}

/// A cursor over a per-kind declaration block body that segments it into one
/// key/value pair per dotted entry, mirroring the source-path per-kind reader:
/// a capitalized dotted key is split off the leading atom, and the value is the
/// following block when the key atom ends at its dot, or the inline remainder
/// carried in the key atom itself. Used only to dispatch the
/// `NamespaceDeclaration` firings for the macro context; segmentation errors
/// are left for the source path to report, so the walk yields what it can and
/// stops. When the value stays inline in the key atom the pair's `name` and
/// `definition` are the same atom, so the registry sees one dispatch per entry.
#[derive(Clone, Copy, Debug)]
struct NamespacePairWalk<'schema> {
    objects: &'schema [Block],
    cursor: usize,
}

impl<'schema> NamespacePairWalk<'schema> {
    fn new(objects: &'schema [Block]) -> Self {
        Self { objects, cursor: 0 }
    }

    fn next_pair(&mut self) -> Option<MacroPair<'schema>> {
        let entry = self.objects.get(self.cursor)?;
        let (name, definition) = entry.as_application()?;
        self.cursor += 1;
        Some(MacroPair { name, definition })
    }
}

/// The NOTA delimiter pair text, used to re-emit an expanded block tree as a
/// source string the document re-parser reads back.
#[derive(Clone, Copy, Debug)]
struct DelimiterText {
    delimiter: Delimiter,
}

impl DelimiterText {
    fn new(delimiter: Delimiter) -> Self {
        Self { delimiter }
    }

    fn wrap(&self, children: &[String]) -> String {
        format!("{}{}{}", self.opening(), children.join(" "), self.closing())
    }

    fn opening(&self) -> &'static str {
        match self.delimiter {
            Delimiter::Parenthesis => "(",
            Delimiter::SquareBracket => "[",
            Delimiter::Brace => "{",
        }
    }

    fn closing(&self) -> &'static str {
        match self.delimiter {
            Delimiter::Parenthesis => ")",
            Delimiter::SquareBracket => "]",
            Delimiter::Brace => "}",
        }
    }
}
