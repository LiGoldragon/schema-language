use nota::{
    AtomShape, Block, CaptureName, DelimitedShape, Delimiter, MacroCandidate, MacroDelimiter,
    MacroNodeDefinition as NotaMacroNodeDefinition, MacroObjectCount as NotaMacroObjectCount,
    MacroRegistry as NotaMacroRegistry, NotaBody, Pattern, PatternElement, PositionPredicate,
    StructureHeader,
};

use crate::{
    Declaration, EnumDeclaration, FieldDeclaration, ImportDeclaration, Name, RootApplication,
    SchemaError, TrueSchema, TypeDeclaration, TypeReference,
};

/// Each position is a keyword structural variant, so a bootstrap macro
/// definition decodes its position atom through the typed macro-node codec
/// instead of a hand-written name match.
#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    nota::NotaDecode,
    nota::NotaEncode,
    nota::StructuralMacroNode,
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
)]
pub enum MacroPosition {
    #[shape(keyword = "RootImports")]
    RootImports,
    #[shape(keyword = "RootInput")]
    RootInput,
    #[shape(keyword = "RootOutput")]
    RootOutput,
    #[shape(keyword = "RootNamespace")]
    RootNamespace,
    #[shape(keyword = "NamespaceDeclaration")]
    NamespaceDeclaration,
    #[shape(keyword = "StructFields")]
    StructFields,
    #[shape(keyword = "EnumVariants")]
    EnumVariants,
    #[shape(keyword = "TypeReference")]
    TypeReference,
}

impl MacroPosition {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RootImports => "RootImports",
            Self::RootInput => "RootInput",
            Self::RootOutput => "RootOutput",
            Self::RootNamespace => "RootNamespace",
            Self::NamespaceDeclaration => "NamespaceDeclaration",
            Self::StructFields => "StructFields",
            Self::EnumVariants => "EnumVariants",
            Self::TypeReference => "TypeReference",
        }
    }

    pub fn position_predicate(&self) -> PositionPredicate {
        PositionPredicate::named(self.as_str())
    }
}

#[derive(Clone, Copy, Debug)]
pub enum MacroObject<'object> {
    Block(&'object Block),
    Pair(MacroPair<'object>),
}

impl<'object> MacroObject<'object> {
    pub fn block(self) -> Option<&'object Block> {
        match self {
            Self::Block(block) => Some(block),
            Self::Pair(_) => None,
        }
    }

    pub fn pair(self) -> Option<MacroPair<'object>> {
        match self {
            Self::Block(_) => None,
            Self::Pair(pair) => Some(pair),
        }
    }

    pub(crate) fn delimited_body(
        self,
        delimiter: Delimiter,
        expected: &'static str,
    ) -> Result<NotaBody<'object>, SchemaError> {
        let block = self
            .block()
            .ok_or(SchemaError::ExpectedDelimiter { expected })?;
        NotaBody::from_delimited(block, delimiter, expected).map_err(SchemaError::from)
    }

    pub fn describe(self) -> String {
        match self {
            Self::Block(block) => format!("block({})", block.reemit_fallback()),
            Self::Pair(pair) => format!(
                "pair({} {})",
                pair.name.reemit_fallback(),
                pair.definition.reemit_fallback()
            ),
        }
    }

    pub fn macro_candidate(self, position: MacroPosition) -> MacroCandidate<'object> {
        match self {
            Self::Block(block) => MacroCandidate::from_block(position.position_predicate(), block),
            Self::Pair(pair) => {
                MacroCandidate::from_pair(position.position_predicate(), pair.name, pair.definition)
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MacroPair<'object> {
    pub name: &'object Block,
    pub definition: &'object Block,
}

pub trait SchemaMacroHandler {
    fn name(&self) -> &str;

    fn matches(&self, object: MacroObject<'_>, position: MacroPosition) -> bool;

    fn lower(
        &self,
        object: MacroObject<'_>,
        position: MacroPosition,
        context: &mut MacroContext,
        registry: &MacroRegistry,
    ) -> Result<MacroOutput, SchemaError>;
}

#[derive(Clone, Debug, Default)]
pub struct MacroContext {
    positions_seen: Vec<MacroPosition>,
    macros_applied: Vec<String>,
    bindings_seen: Vec<String>,
    expanded_templates: Vec<String>,
    structure_headers: Vec<StructureHeader>,
    inline_declarations: Vec<Declaration>,
}

impl MacroContext {
    pub fn remember_position(&mut self, position: MacroPosition) {
        self.positions_seen.push(position);
    }

    pub fn remember_macro(&mut self, macro_name: impl Into<String>) {
        self.macros_applied.push(macro_name.into());
    }

    pub fn remember_binding(&mut self, macro_name: impl AsRef<str>, binding_name: impl AsRef<str>) {
        self.bindings_seen.push(format!(
            "{}::{}",
            macro_name.as_ref(),
            binding_name.as_ref()
        ));
    }

    pub fn remember_expanded_template(
        &mut self,
        macro_name: impl AsRef<str>,
        template: impl AsRef<str>,
    ) {
        self.expanded_templates
            .push(format!("{} -> {}", macro_name.as_ref(), template.as_ref()));
    }

    pub fn remember_structure_header(&mut self, header: StructureHeader) {
        self.structure_headers.push(header);
    }

    pub(crate) fn remember_inline_declaration(&mut self, declaration: Declaration) {
        self.inline_declarations.push(declaration);
    }

    pub(crate) fn inline_declaration_count(&self) -> usize {
        self.inline_declarations.len()
    }

    pub(crate) fn drain_inline_declarations_from(&mut self, index: usize) -> Vec<Declaration> {
        self.inline_declarations.drain(index..).collect()
    }

    pub fn positions_seen(&self) -> &[MacroPosition] {
        &self.positions_seen
    }

    pub fn macros_applied(&self) -> &[String] {
        &self.macros_applied
    }

    pub fn bindings_seen(&self) -> &[String] {
        &self.bindings_seen
    }

    pub fn expanded_templates(&self) -> &[String] {
        &self.expanded_templates
    }

    pub fn structure_headers(&self) -> &[StructureHeader] {
        &self.structure_headers
    }

    pub fn inline_declarations(&self) -> &[Declaration] {
        &self.inline_declarations
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MacroOutput {
    TrueSchema(TrueSchema),
    Imports(Vec<ImportDeclaration>),
    RootEnum(EnumDeclaration),
    /// A root in the application form `(Head Arg …)` — the typed-sum
    /// alternative to `RootEnum` at an Input/Output position.
    RootApplication(RootApplication),
    Types(Vec<Declaration>),
    Type(TypeDeclaration),
    /// A fully-formed namespace declaration carrying its visibility and
    /// declared type parameters. The parameterized declaration head
    /// `(| Name Param … |)` lowers to this so the binders stay attached to
    /// their declaration rather than being recovered from the bare value.
    Declaration(Declaration),
    Fields(Vec<FieldDeclaration>),
    Variants(Vec<crate::EnumVariant>),
    Reference(TypeReference),
    References(Vec<TypeReference>),
}

pub struct MacroRegistry {
    macros: Vec<Box<dyn SchemaMacroHandler>>,
    node_definitions: Vec<MacroNodeDefinition>,
}

impl Default for MacroRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl MacroRegistry {
    pub fn new() -> Self {
        Self {
            macros: Vec::new(),
            node_definitions: Vec::new(),
        }
    }

    pub fn register(&mut self, schema_macro: impl SchemaMacroHandler + 'static) {
        self.macros.push(Box::new(schema_macro));
    }

    pub fn register_box(&mut self, schema_macro: Box<dyn SchemaMacroHandler>) {
        self.macros.push(schema_macro);
    }

    pub fn register_node_definition(&mut self, definition: MacroNodeDefinition) {
        self.node_definitions.push(definition);
    }

    pub fn node_definition(&self, position: MacroPosition) -> Option<&MacroNodeDefinition> {
        self.node_definitions
            .iter()
            .find(|definition| definition.position == position)
    }

    pub fn node_definitions(&self) -> &[MacroNodeDefinition] {
        &self.node_definitions
    }

    pub fn lower(
        &self,
        object: MacroObject<'_>,
        position: MacroPosition,
        context: &mut MacroContext,
    ) -> Result<MacroOutput, SchemaError> {
        for schema_macro in &self.macros {
            if schema_macro.matches(object, position) {
                return schema_macro.lower(object, position, context, self);
            }
        }
        if position != MacroPosition::TypeReference
            && let Some(definition) = self.node_definition(position)
            && definition.has_cases()
        {
            return Err(definition.unsupported_structure_error(object));
        }
        Err(SchemaError::MacroDidNotMatch {
            macro_name: "registered macro".to_owned(),
        })
    }

    /// The c2dc dispatch over the ordered macro list: return the name of the
    /// first registered macro that matches `object` at `position`, first-match
    /// wins. This is the front-end pre-expansion pass's window into the same
    /// ordered-list dispatch [`Self::lower`] uses, but it answers *which* macro
    /// fires without running its lowering — the pass records the firing and
    /// (for type-reference macros) expands the captured body separately.
    pub fn matching_macro_name(
        &self,
        object: MacroObject<'_>,
        position: MacroPosition,
    ) -> Option<&str> {
        self.macros
            .iter()
            .find(|schema_macro| schema_macro.matches(object, position))
            .map(|schema_macro| schema_macro.name())
    }

    pub fn macro_names(&self) -> Vec<String> {
        self.macros
            .iter()
            .map(|schema_macro| schema_macro.name().to_owned())
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MacroNodeDefinition {
    position: MacroPosition,
    dispatch: MacroDispatch,
    cases: Vec<NotaMacroNodeDefinition>,
}

impl MacroNodeDefinition {
    pub fn new(position: MacroPosition, dispatch: MacroDispatch) -> Self {
        Self {
            position,
            dispatch,
            cases: Vec::new(),
        }
    }

    pub fn with_cases(
        position: MacroPosition,
        dispatch: MacroDispatch,
        cases: Vec<NotaMacroNodeDefinition>,
    ) -> Self {
        Self {
            position,
            dispatch,
            cases,
        }
    }

    pub fn root_imports() -> Self {
        Self::with_cases(
            MacroPosition::RootImports,
            MacroDispatch::RootPositional,
            vec![Self::block_case(
                MacroPosition::RootImports,
                "imports map",
                MacroDelimiter::Brace,
                NotaMacroObjectCount::Even,
            )],
        )
    }

    pub fn root_input() -> Self {
        Self::root_enum(MacroPosition::RootInput)
    }

    pub fn root_output() -> Self {
        Self::root_enum(MacroPosition::RootOutput)
    }

    pub fn root_namespace() -> Self {
        Self::with_cases(
            MacroPosition::RootNamespace,
            MacroDispatch::RootPositional,
            vec![Self::block_case(
                MacroPosition::RootNamespace,
                "namespace map",
                MacroDelimiter::Brace,
                NotaMacroObjectCount::Even,
            )],
        )
    }

    pub fn namespace_declaration() -> Self {
        Self::with_cases(
            MacroPosition::NamespaceDeclaration,
            MacroDispatch::Structural,
            vec![
                Self::pair_case(
                    MacroPosition::NamespaceDeclaration,
                    "struct declaration",
                    PatternElement::atom(AtomShape::symbol(Some(CaptureName::new("type_name")))),
                    PatternElement::delimited(DelimitedShape::new(
                        MacroDelimiter::Brace,
                        NotaMacroObjectCount::Any,
                        Some(CaptureName::new("body")),
                    )),
                    "symbol key followed by brace value",
                ),
                Self::pair_case(
                    MacroPosition::NamespaceDeclaration,
                    "enum declaration",
                    PatternElement::atom(AtomShape::symbol(Some(CaptureName::new("type_name")))),
                    PatternElement::delimited(DelimitedShape::new(
                        MacroDelimiter::SquareBracket,
                        NotaMacroObjectCount::Any,
                        Some(CaptureName::new("body")),
                    )),
                    "symbol key followed by square bracket value",
                ),
                Self::pair_case(
                    MacroPosition::NamespaceDeclaration,
                    "newtype declaration",
                    PatternElement::atom(AtomShape::symbol(Some(CaptureName::new("type_name")))),
                    PatternElement::any(Some(CaptureName::new("reference"))),
                    "symbol key followed by type reference value",
                ),
                // Parameterized declaration heads `(| Name Param … |)` carry
                // the binders in the key position. The key is the same
                // captured-head + variable-arity-tail shape the application
                // form uses, gated by the pipe-parenthesis delimiter so
                // declarations and use-site applications stay distinct. Each
                // body shape gets its own parameterized case so the dispatch
                // stays exhaustive over struct / enum / newtype bodies.
                Self::pair_case(
                    MacroPosition::NamespaceDeclaration,
                    "parameterized struct declaration",
                    PatternElement::delimited(DelimitedShape::new(
                        MacroDelimiter::PipeParenthesis,
                        NotaMacroObjectCount::Any,
                        Some(CaptureName::new("type_head")),
                    )),
                    PatternElement::delimited(DelimitedShape::new(
                        MacroDelimiter::Brace,
                        NotaMacroObjectCount::Any,
                        Some(CaptureName::new("body")),
                    )),
                    "parameterized pipe head followed by brace value",
                ),
                Self::pair_case(
                    MacroPosition::NamespaceDeclaration,
                    "parameterized enum declaration",
                    PatternElement::delimited(DelimitedShape::new(
                        MacroDelimiter::PipeParenthesis,
                        NotaMacroObjectCount::Any,
                        Some(CaptureName::new("type_head")),
                    )),
                    PatternElement::delimited(DelimitedShape::new(
                        MacroDelimiter::SquareBracket,
                        NotaMacroObjectCount::Any,
                        Some(CaptureName::new("body")),
                    )),
                    "parameterized pipe head followed by square bracket value",
                ),
                Self::pair_case(
                    MacroPosition::NamespaceDeclaration,
                    "parameterized newtype declaration",
                    PatternElement::delimited(DelimitedShape::new(
                        MacroDelimiter::PipeParenthesis,
                        NotaMacroObjectCount::Any,
                        Some(CaptureName::new("type_head")),
                    )),
                    PatternElement::any(Some(CaptureName::new("reference"))),
                    "parameterized pipe head followed by type reference value",
                ),
            ],
        )
    }

    pub fn struct_fields() -> Self {
        Self::with_cases(
            MacroPosition::StructFields,
            MacroDispatch::Structural,
            vec![
                Self::pair_case(
                    MacroPosition::StructFields,
                    "explicit field",
                    PatternElement::atom(AtomShape::camel_case(Some(CaptureName::new(
                        "field_name",
                    )))),
                    PatternElement::any(Some(CaptureName::new("reference"))),
                    "camelCase field key followed by type reference value",
                ),
                Self::pair_case(
                    MacroPosition::StructFields,
                    "derived field",
                    PatternElement::atom(AtomShape::pascal_case(Some(CaptureName::new(
                        "type_name",
                    )))),
                    PatternElement::literal("*"),
                    "PascalCase type key followed by * marker",
                ),
            ],
        )
    }

    pub fn enum_variants() -> Self {
        Self::with_cases(
            MacroPosition::EnumVariants,
            MacroDispatch::Structural,
            vec![
                NotaMacroNodeDefinition::new(
                    "unit variant",
                    MacroPosition::EnumVariants.position_predicate(),
                    Pattern::new(vec![PatternElement::atom(AtomShape::pascal_case(Some(
                        CaptureName::new("variant_name"),
                    )))]),
                    "PascalCase variant atom",
                ),
                NotaMacroNodeDefinition::new(
                    "data variant",
                    MacroPosition::EnumVariants.position_predicate(),
                    Pattern::new(vec![PatternElement::delimited(
                        DelimitedShape::new(
                            MacroDelimiter::Parenthesis,
                            NotaMacroObjectCount::Exact(2),
                            Some(CaptureName::new("variant_signature")),
                        )
                        .with_children(Pattern::new(vec![
                            PatternElement::atom(AtomShape::pascal_case(Some(CaptureName::new(
                                "variant_name",
                            )))),
                            PatternElement::any(Some(CaptureName::new("payload"))),
                        ])),
                    )]),
                    "parenthesized variant signature carrying variant name and payload type",
                ),
                NotaMacroNodeDefinition::new(
                    "opens variant",
                    MacroPosition::EnumVariants.position_predicate(),
                    Pattern::new(vec![PatternElement::delimited(
                        DelimitedShape::new(
                            MacroDelimiter::Parenthesis,
                            NotaMacroObjectCount::Exact(4),
                            Some(CaptureName::new("variant_signature")),
                        )
                        .with_children(Pattern::new(vec![
                            PatternElement::atom(AtomShape::pascal_case(Some(CaptureName::new(
                                "variant_name",
                            )))),
                            PatternElement::any(Some(CaptureName::new("payload"))),
                            PatternElement::literal("opens"),
                            PatternElement::atom(AtomShape::pascal_case(Some(CaptureName::new(
                                "stream_name",
                            )))),
                        ])),
                    )]),
                    "parenthesized variant signature carrying variant name, payload type, opens keyword, and stream name",
                ),
                NotaMacroNodeDefinition::new(
                    "belongs variant",
                    MacroPosition::EnumVariants.position_predicate(),
                    Pattern::new(vec![PatternElement::delimited(
                        DelimitedShape::new(
                            MacroDelimiter::Parenthesis,
                            NotaMacroObjectCount::Exact(4),
                            Some(CaptureName::new("variant_signature")),
                        )
                        .with_children(Pattern::new(vec![
                            PatternElement::atom(AtomShape::pascal_case(Some(CaptureName::new(
                                "variant_name",
                            )))),
                            PatternElement::any(Some(CaptureName::new("payload"))),
                            PatternElement::literal("belongs"),
                            PatternElement::atom(AtomShape::pascal_case(Some(CaptureName::new(
                                "stream_name",
                            )))),
                        ])),
                    )]),
                    "parenthesized variant signature carrying variant name, payload type, belongs keyword, and stream name",
                ),
            ],
        )
    }

    pub fn type_reference() -> Self {
        Self::with_cases(
            MacroPosition::TypeReference,
            MacroDispatch::StructuralOrTaggedInvocation,
            vec![
                NotaMacroNodeDefinition::new(
                    "plain or scalar reference",
                    MacroPosition::TypeReference.position_predicate(),
                    Pattern::new(vec![PatternElement::atom(AtomShape::symbol(Some(
                        CaptureName::new("reference"),
                    )))]),
                    "symbol reference atom",
                ),
                Self::block_case(
                    MacroPosition::TypeReference,
                    "composite or tagged invocation",
                    MacroDelimiter::Parenthesis,
                    NotaMacroObjectCount::Any,
                ),
            ],
        )
    }

    fn root_enum(position: MacroPosition) -> Self {
        // A root position accepts two structural forms: the enum body
        // `[Variant …]` and the application form `(Head Arg …)`. Both are
        // RootPositional; the handler dispatches on the delimiter.
        Self::with_cases(
            position,
            MacroDispatch::RootPositional,
            vec![
                Self::block_case(
                    position,
                    "root enum body",
                    MacroDelimiter::SquareBracket,
                    NotaMacroObjectCount::Any,
                ),
                Self::block_case(
                    position,
                    "root application body",
                    MacroDelimiter::Parenthesis,
                    NotaMacroObjectCount::Any,
                ),
            ],
        )
    }

    pub fn position(&self) -> MacroPosition {
        self.position
    }

    pub fn dispatch(&self) -> MacroDispatch {
        self.dispatch
    }

    pub fn cases(&self) -> &[NotaMacroNodeDefinition] {
        &self.cases
    }

    pub fn has_cases(&self) -> bool {
        !self.cases.is_empty()
    }

    pub fn matches(&self, object: MacroObject<'_>) -> bool {
        NotaMacroRegistry::unchecked(self.cases.clone())
            .dispatch(&object.macro_candidate(self.position))
            .is_ok()
    }

    pub fn unsupported_structure_error(&self, object: MacroObject<'_>) -> SchemaError {
        let error = NotaMacroRegistry::unchecked(self.cases.clone())
            .dispatch(&object.macro_candidate(self.position))
            .expect_err("unsupported structure checked after no schema macro matched");
        match error {
            nota::MacroError::NoMatch {
                expected, found, ..
            } => SchemaError::UnsupportedMacroNodeStructure {
                position: self.position.as_str().to_owned(),
                expected,
                found,
            },
            nota::MacroError::Conflict(conflict) => SchemaError::UnsupportedMacroNodeStructure {
                position: self.position.as_str().to_owned(),
                expected: vec![format!(
                    "non-conflicting macro cases, found conflict between {} and {}",
                    conflict.first(),
                    conflict.second()
                )],
                found: object.describe(),
            },
        }
    }

    pub fn accepts_tagged_invocation(&self) -> bool {
        matches!(
            self.dispatch,
            MacroDispatch::TaggedInvocation | MacroDispatch::StructuralOrTaggedInvocation
        )
    }

    fn block_case(
        position: MacroPosition,
        name: impl Into<String>,
        delimiter: MacroDelimiter,
        object_count: NotaMacroObjectCount,
    ) -> NotaMacroNodeDefinition {
        let delimiter_name = delimiter.as_str();
        NotaMacroNodeDefinition::new(
            name,
            position.position_predicate(),
            Pattern::new(vec![PatternElement::delimited(DelimitedShape::new(
                delimiter,
                object_count,
                Some(CaptureName::new("body")),
            ))]),
            format!("{delimiter_name} block"),
        )
    }

    fn pair_case(
        position: MacroPosition,
        name: impl Into<String>,
        key: PatternElement,
        value: PatternElement,
        expected: impl Into<String>,
    ) -> NotaMacroNodeDefinition {
        NotaMacroNodeDefinition::new(
            name,
            position.position_predicate(),
            Pattern::new(vec![key, value]),
            expected,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MacroDispatch {
    RootPositional,
    Structural,
    TaggedInvocation,
    StructuralOrTaggedInvocation,
}

pub(crate) trait BlockDebug {
    fn reemit_fallback(&self) -> String;
}

pub(crate) trait SchemaBlockExt {
    fn schema_name(&self) -> Result<Name, SchemaError>;
}

impl BlockDebug for Block {
    fn reemit_fallback(&self) -> String {
        self.demote_to_string()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{self:?}"))
    }
}

impl SchemaBlockExt for Block {
    fn schema_name(&self) -> Result<Name, SchemaError> {
        self.atom()
            .filter(|atom| atom.qualifies_as_symbol())
            .map(|atom| Name::new(atom.text()))
            .ok_or_else(|| SchemaError::ExpectedSymbol {
                found: self.reemit_fallback(),
            })
    }
}
