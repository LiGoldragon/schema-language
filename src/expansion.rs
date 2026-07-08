use nota::{Block, Delimiter, Document, StructuralMacroNode};

use crate::{
    MacroContext, MacroObject, MacroOutput, MacroPair, MacroPosition, MacroRegistry, SchemaError,
    macros::SchemaBlockExt,
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
///    registered `Bag` macro becomes its expanded body `(Vector Topic)`,
///    recursively — so the source path sees only built-in heads it already
///    understands. The expansion reuses the registered macro's own
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
    /// bindings. The entry contract (3 roots, or 4 with leading imports) is the
    /// document path's, and is checked by the caller; this pass tolerates any
    /// well-formed positional shape and records what it recognises.
    pub(crate) fn expand(
        &self,
        document: &Document,
        context: &mut MacroContext,
    ) -> Result<Document, SchemaError> {
        let layout = DocumentLayout::of(document);
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
        layout: &DocumentLayout,
        context: &mut MacroContext,
    ) -> Result<(), SchemaError> {
        if let Some(index) = layout.imports_index
            && let Some(block) = document.root_object_at(index)
        {
            self.record_block_firing(block, MacroPosition::RootImports, context);
        }
        if let Some(block) = document.root_object_at(layout.input_index) {
            self.record_block_firing(block, MacroPosition::RootInput, context);
        }
        if let Some(block) = document.root_object_at(layout.output_index) {
            self.record_block_firing(block, MacroPosition::RootOutput, context);
        }
        if let Some(block) = document.root_object_at(layout.namespace_index) {
            self.record_block_firing(block, MacroPosition::RootNamespace, context);
            self.record_namespace_declaration_firings(block, context)?;
        }
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

    /// Record one `KeyValueDeclaration` firing per namespace key/value pair the
    /// registry dispatches at the namespace-declaration position. The pair
    /// segmentation mirrors the source path's namespace walk (a head plus an
    /// optional inline body, with a trailing `{| … |}` impl block skipped) so
    /// the recorded firings match the declarations the source path lowers.
    fn record_namespace_declaration_firings(
        &self,
        namespace: &Block,
        context: &mut MacroContext,
    ) -> Result<(), SchemaError> {
        let Block::Delimited {
            delimiter: Delimiter::Brace,
            root_objects,
            ..
        } = namespace
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

/// The positional slot map of a schema document: an optional leading imports
/// brace, then the generics section, input / output / namespace, then an
/// optional trailing relations block. The pass reads slots from this map so
/// its recording is aligned with `SchemaSource::from_document`'s own
/// positional read.
#[derive(Clone, Copy, Debug)]
struct DocumentLayout {
    imports_index: Option<usize>,
    input_index: usize,
    output_index: usize,
    namespace_index: usize,
}

impl DocumentLayout {
    fn of(document: &Document) -> Self {
        let first_two_are_braces = document
            .root_object_at(0)
            .is_some_and(|block| block.is_brace())
            && document
                .root_object_at(1)
                .is_some_and(|block| block.is_brace());
        let first_is_imports = first_two_are_braces && document.holds_root_objects() >= 5;
        let offset = if first_is_imports { 2 } else { 1 };
        Self {
            imports_index: first_is_imports.then_some(0),
            input_index: offset,
            output_index: offset + 1,
            namespace_index: offset + 2,
        }
    }
}

/// A cursor over a namespace body that segments it into key/value pairs using
/// the head / optional-body / optional-pipe-brace grammar the source-path
/// namespace walk uses. Used only to count the `KeyValueDeclaration` firings
/// for the macro context; segmentation errors are left for the source path to
/// report, so the walk yields what it can and stops.
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
        loop {
            let head = self.objects.get(self.cursor)?;
            if head.is_pipe_brace() {
                return None;
            }
            self.cursor += 1;
            let definition = match self.objects.get(self.cursor) {
                Some(next) if !next.is_pipe_brace() => {
                    self.cursor += 1;
                    Some(next)
                }
                _ => None,
            };
            if let Some(next) = self.objects.get(self.cursor)
                && next.is_pipe_brace()
            {
                self.cursor += 1;
            }
            match definition {
                Some(definition) => {
                    return Some(MacroPair {
                        name: head,
                        definition,
                    });
                }
                // A body-optional `TypeName {| … |}` entry mints no
                // declaration on the macro path; skip it and keep walking.
                None => continue,
            }
        }
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
            Delimiter::PipeParenthesis => "(|",
            Delimiter::PipeBrace => "{|",
        }
    }

    fn closing(&self) -> &'static str {
        match self.delimiter {
            Delimiter::Parenthesis => ")",
            Delimiter::SquareBracket => "]",
            Delimiter::Brace => "}",
            Delimiter::PipeParenthesis => "|)",
            Delimiter::PipeBrace => "|}",
        }
    }
}
