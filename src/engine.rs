use nota::{Block, Delimiter, Document, NotaBody, NotaEncode};

use crate::{
    ImportResolver, SchemaSource,
    declarative::{MacroExpansionStructBody, MacroExpansionVariants},
    expansion::MacroExpansionPass,
    macros::{
        MacroContext, MacroNodeDefinition, MacroObject, MacroOutput, MacroPair, MacroPosition,
        MacroRegistry, SchemaBlockExt, SchemaMacroHandler,
    },
    schema::{
        Declaration, DeclarationHead, EnumDeclaration, EnumVariant, ImportDeclaration, Name,
        NewtypeDeclaration, RootApplication, TrueSchema, TypeDeclaration, TypeReference,
    },
};

#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    nota::NotaDecode,
    nota::NotaEncode,
    Clone,
    Debug,
    Eq,
    PartialEq,
)]
pub struct SchemaIdentity {
    component: Name,
    version: String,
}

impl SchemaIdentity {
    pub fn new(component: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            component: Name::new(component),
            version: version.into(),
        }
    }

    pub fn component(&self) -> &Name {
        &self.component
    }

    pub fn version(&self) -> &str {
        &self.version
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SchemaError {
    #[error("NOTA parse error: {0}")]
    Nota(#[from] nota::NotaError),
    #[error("NOTA decode error: {0}")]
    NotaDecode(#[from] nota::NotaDecodeError),
    #[error("structural macro parse error: {0}")]
    StructuralMacroParse(String),
    #[error("rkyv archive encoding failed")]
    ArchiveEncode,
    #[error("rkyv archive decoding failed")]
    ArchiveDecode,
    #[error("expected {expected}, found {found} root objects")]
    ExpectedRootObjectCount {
        expected: &'static str,
        found: usize,
    },
    #[error("expected {expected} delimiter")]
    ExpectedDelimiter { expected: &'static str },
    #[error("expected an even number of map entries, found {found}")]
    ExpectedEvenMapEntries { found: usize },
    #[error(
        "retired struct field syntax {found}; struct bodies are positional field types, use TypeName or field_name.TypeName"
    )]
    RetiredStructFieldSyntax { found: String },
    #[error("redundant explicit field role {found}; just use {type_name}")]
    RedundantExplicitFieldRole { found: String, type_name: String },
    #[error(
        "explicit product component {field}.{type_name} is invalid because {type_name} appears only once"
    )]
    ExplicitFieldOnUniqueProductComponent { field: String, type_name: String },
    #[error(
        "product component type {type_name} appears more than once and each occurrence must use an explicit field.Type identity"
    )]
    DuplicateImplicitProductComponent { type_name: String },
    #[error("duplicate explicit product component identity {field} for repeated type {type_name}")]
    DuplicateExplicitProductComponentIdentity { field: String, type_name: String },
    #[error(
        "optional enum-variant payload {enum_name}::{variant_name}; a variant payload must always appear in the text form, so (Optional T) is forbidden here — model the optional case as an explicit member carrying a required payload (for example a leaf enum with an explicit All member)"
    )]
    OptionalVariantPayload {
        enum_name: String,
        variant_name: String,
    },
    #[error(
        "same-named enum-variant payload {enum_name}::{variant_name}({payload_type}); direct variant payload type names must differ from their variant names"
    )]
    SameNamedVariantPayload {
        enum_name: String,
        variant_name: String,
        payload_type: String,
    },
    #[error("io error at {path}: {reason}")]
    Io { path: String, reason: String },
    #[error("malformed schema path: {path}")]
    MalformedSchemaPath { path: String },
    #[error("expected a symbol, found {found}")]
    ExpectedSymbol { found: String },
    #[error("expected an enum variant")]
    ExpectedEnumVariant,
    #[error("malformed schema node: {found}")]
    MalformedSchemaNode { found: String },
    #[error("unsupported macro node structure at {position}: expected {expected:?}, found {found}")]
    UnsupportedMacroNodeStructure {
        position: String,
        expected: Vec<String>,
        found: String,
    },
    #[error("macro {macro_name} did not match")]
    MacroDidNotMatch { macro_name: String },
    #[error("macro {macro_name} produced unexpected output, expected {expected}")]
    UnexpectedMacroOutput {
        macro_name: String,
        expected: &'static str,
    },
    #[error("expected a macro definition, found {found}")]
    ExpectedMacroDefinition { found: String },
    #[error("invalid macro capture: {found}")]
    InvalidMacroCapture { found: String },
    #[error("missing macro binding {name}")]
    MissingMacroBinding { name: String },
    #[error("conflicting macro binding {name}")]
    ConflictingMacroBinding { name: String },
    #[error("expected {expected} template objects at {position}, found {found}")]
    ExpectedTemplateObjectCount {
        position: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("empty type reference")]
    EmptyTypeReference,
    #[error("unknown type reference form {head} with {argument_count} arguments")]
    UnknownTypeReferenceForm { head: String, argument_count: usize },
    #[error("reserved scalar type name {name}")]
    ReservedScalarTypeName { name: String },
    #[error("malformed import source: {found}")]
    MalformedImportSource { found: String },
    #[error("unresolved import crate {crate_name}")]
    UnresolvedImportCrate { crate_name: String },
    #[error("imported type {type_name} not found in {crate_name}:{module}")]
    ImportedTypeNotFound {
        crate_name: String,
        module: String,
        type_name: String,
    },
    #[error("expected a raw declaration name, found {found}")]
    ExpectedRawDeclarationName { found: String },
    #[error("raw declaration name mismatch: key {key} declared {declared}")]
    RawDeclarationNameMismatch { key: String, declared: String },
    #[error("expected an even field-pair count for {declaration}, found {found}")]
    ExpectedRawFieldPairCount { declaration: String, found: usize },
    #[error("expected a syntax declaration, found {found}")]
    ExpectedSyntaxDeclaration { found: String },
    #[error("expected a syntax reference, found {found}")]
    ExpectedSyntaxReference { found: String },
    #[error("expected {form} to hold {expected}, found {found} objects")]
    ExpectedSyntaxReferenceArity {
        form: &'static str,
        expected: &'static str,
        found: usize,
    },
    #[error("expected a syntax enum variant, found {found}")]
    ExpectedSyntaxEnumVariant { found: String },
    #[error("duplicate source declaration {name}")]
    DuplicateSourceDeclaration { name: String },
    #[error("schema edit target {type_name} not found")]
    SchemaEditTargetNotFound { type_name: String },
    #[error("schema edit expected {type_name} to be a struct")]
    SchemaEditExpectedStruct { type_name: String },
    #[error("schema edit expected {type_name} to be an enum")]
    SchemaEditExpectedEnum { type_name: String },
    #[error("schema edit duplicate field {field_name} on {type_name}")]
    SchemaEditDuplicateField {
        type_name: String,
        field_name: String,
    },
    #[error("schema edit duplicate variant {variant_name} on {type_name}")]
    SchemaEditDuplicateVariant {
        type_name: String,
        variant_name: String,
    },
    #[error("schema edit field {field_name} not found on {type_name}")]
    SchemaEditFieldNotFound {
        type_name: String,
        field_name: String,
    },
    #[error("schema edit identity mismatch: expected {expected}, found {found}")]
    SchemaEditIdentityMismatch { expected: String, found: String },
    #[error("family root {name} not found")]
    FamilyRootNotFound { name: String },
    #[error("family reference {name} not found in family {family}")]
    FamilyReferenceNotFound { family: String, name: String },
    #[error("family record {record} not found in family {family}")]
    FamilyRecordNotFound { family: String, record: String },
    #[error("duplicate family name {name}")]
    DuplicateFamilyName { name: String },
    #[error("duplicate family table {table}")]
    DuplicateFamilyTable { table: String },
    #[error("duplicate type parameter {parameter} on {declaration}")]
    DuplicateTypeParameter {
        declaration: String,
        parameter: String,
    },
    #[error("expected a type parameter name on {declaration}, found {found}")]
    ExpectedTypeParameterName { declaration: String, found: String },
    #[error("generic arity mismatch for {head}: expected {expected}, found {found}")]
    GenericArityMismatch {
        head: String,
        expected: usize,
        found: usize,
    },
    /// A parenthesis at a root Input/Output position did not decode to the
    /// application form `(Head Arg …)` — a built-in head (`(Vector T)`), a
    /// collection form, or any other non-application parenthesis is not a
    /// legal root body.
    #[error("expected a root application at {position}, found {found}")]
    ExpectedRootApplication {
        position: &'static str,
        found: String,
    },
    #[error(
        "impl catalog references {kind} `{signature}` on type `{target}`, which is absent from the available Rust surface"
    )]
    UnverifiedImplReference {
        target: String,
        kind: &'static str,
        signature: String,
    },
    /// A body-optional `TypeName {| … |}` impl block names a target type that
    /// is not declared anywhere in the schema. A standalone impl block must
    /// attach its catalog to a type declared by a separate entry; an
    /// unresolved target is not an accepted free-standing impl over an
    /// arbitrary name.
    #[error("impl block targets type `{name}`, which is not declared in this schema")]
    UnresolvedImplTarget { name: String },
    /// A target type carries the same impl entry twice — the same trait
    /// marker repeated, or the same method signature repeated — across one or
    /// more `{| … |}` blocks. Distinct entries for one target compose; an
    /// identical entry is a true duplicate and a typed error.
    #[error("impl catalog for type `{target}` carries duplicate entry `{entry}`")]
    DuplicateImplEntry { target: String, entry: String },
    /// A trait atom inside a `{| … |}` impl block is not a PascalCase
    /// type-name. Trait references obey the same naming gate as every other
    /// type reference: a lowercase or otherwise non-type-name atom in a trait
    /// position is a typed error, not a silently-accepted trait.
    #[error("impl block trait `{found}` is not a PascalCase type name")]
    NonTypeNameTrait { found: String },
}

impl From<nota::MacroError> for SchemaError {
    fn from(value: nota::MacroError) -> Self {
        match value {
            nota::MacroError::NoMatch {
                position,
                expected,
                found,
                ..
            } => Self::UnsupportedMacroNodeStructure {
                position,
                expected,
                found,
            },
            nota::MacroError::Conflict(conflict) => Self::UnsupportedMacroNodeStructure {
                position: "structural macro registry".to_owned(),
                expected: vec![format!(
                    "non-conflicting macro cases, found conflict between {} and {}",
                    conflict.first(),
                    conflict.second()
                )],
                found: "conflicting structural macro definitions".to_owned(),
            },
        }
    }
}

impl From<nota::StructuralVariantError> for SchemaError {
    fn from(value: nota::StructuralVariantError) -> Self {
        match value {
            nota::StructuralVariantError::NoMatch {
                position,
                expected,
                found,
                ..
            } => Self::UnsupportedMacroNodeStructure {
                position,
                expected,
                found,
            },
            nota::StructuralVariantError::Conflict(conflict) => {
                Self::UnsupportedMacroNodeStructure {
                    position: "structural macro node enum".to_owned(),
                    expected: vec![format!(
                        "non-conflicting structural variants, found conflict between {} and {}",
                        conflict.first(),
                        conflict.second()
                    )],
                    found: "conflicting structural macro node variants".to_owned(),
                }
            }
        }
    }
}

impl From<nota::StructuralMacroError<SchemaError>> for SchemaError {
    fn from(value: nota::StructuralMacroError<SchemaError>) -> Self {
        match value {
            nota::StructuralMacroError::Parse { error } => Self::StructuralMacroParse(error),
            nota::StructuralMacroError::ExpectedSingleRoot { found } => {
                Self::ExpectedRootObjectCount {
                    expected: "one structural macro node root object",
                    found,
                }
            }
            nota::StructuralMacroError::Dispatch(error) => Self::from(error),
            nota::StructuralMacroError::MatchedNode(error) => error,
        }
    }
}

impl From<nota::StructuralMacroNodeError> for SchemaError {
    fn from(value: nota::StructuralMacroNodeError) -> Self {
        Self::MalformedSchemaNode {
            found: value.to_string(),
        }
    }
}

impl From<nota::StructuralMacroError<nota::StructuralMacroNodeError>> for SchemaError {
    fn from(value: nota::StructuralMacroError<nota::StructuralMacroNodeError>) -> Self {
        match value {
            nota::StructuralMacroError::Parse { error } => Self::StructuralMacroParse(error),
            nota::StructuralMacroError::ExpectedSingleRoot { found } => {
                Self::ExpectedRootObjectCount {
                    expected: "one structural macro node root object",
                    found,
                }
            }
            nota::StructuralMacroError::Dispatch(error) => Self::from(error),
            nota::StructuralMacroError::MatchedNode(error) => Self::from(error),
        }
    }
}

pub struct SchemaEngine {
    registry: MacroRegistry,
}

impl Default for SchemaEngine {
    fn default() -> Self {
        Self {
            registry: MacroRegistry::with_schema_defaults(),
        }
    }
}

impl SchemaEngine {
    pub fn with_registry(registry: MacroRegistry) -> Self {
        Self { registry }
    }

    pub fn lower_source(
        &self,
        source: &str,
        identity: SchemaIdentity,
    ) -> Result<TrueSchema, SchemaError> {
        let document = Document::parse(source)?;
        self.lower_document(&document, identity)
    }

    pub fn lower_true_schema_source(
        &self,
        source: &str,
        identity: SchemaIdentity,
    ) -> Result<TrueSchema, SchemaError> {
        self.lower_source(source, identity)
    }

    pub fn lower_true_schema_source_with_resolver(
        &self,
        source: &str,
        identity: SchemaIdentity,
        resolver: &ImportResolver,
    ) -> Result<TrueSchema, SchemaError> {
        let mut context = MacroContext::default();
        self.lower_source_with_resolver(source, identity, &mut context, resolver)
    }

    pub fn lower_schema_source(
        &self,
        source: &SchemaSource,
        identity: SchemaIdentity,
    ) -> Result<TrueSchema, SchemaError> {
        self.lower_schema_source_with_resolver(source, identity, &ImportResolver::new())
    }

    pub fn lower_schema_source_with_resolver(
        &self,
        source: &SchemaSource,
        identity: SchemaIdentity,
        resolver: &ImportResolver,
    ) -> Result<TrueSchema, SchemaError> {
        let imports = source.imports().to_schema_imports()?;
        let resolved_imports = resolver.resolve_all(&imports, self)?;
        source.to_true_schema(identity, imports, resolved_imports)
    }

    pub fn lower_source_with_context(
        &self,
        source: &str,
        identity: SchemaIdentity,
        context: &mut MacroContext,
    ) -> Result<TrueSchema, SchemaError> {
        let document = Document::parse(source)?;
        self.lower_document_with_context(&document, identity, context)
    }

    pub fn lower_document(
        &self,
        document: &Document,
        identity: SchemaIdentity,
    ) -> Result<TrueSchema, SchemaError> {
        self.lower_document_with_context(document, identity, &mut MacroContext::default())
    }

    pub fn lower_document_with_context(
        &self,
        document: &Document,
        identity: SchemaIdentity,
        context: &mut MacroContext,
    ) -> Result<TrueSchema, SchemaError> {
        self.lower_document_with_resolver(document, identity, context, &ImportResolver::new())
    }

    /// Lower a document, resolving its imports against `resolver`.
    ///
    /// This is the cross-crate boundary: the consumer build script
    /// registers dependency crate schema directories on the resolver,
    /// and the resolver turns each collected import declaration into a
    /// resolved import that the Rust emitter can use as a `pub use`
    /// alias instead of re-declaring the dependency's type.
    ///
    /// There is exactly one set of lowering semantics: the typed-source
    /// path. The document entry point reparses the document into a
    /// [`SchemaSource`] and lowers *that* — it does not carry a second
    /// hand-mirrored lowerer. Report 702 confirmed a latent divergence
    /// when two engines were kept in lockstep (the document path had no
    /// nested-namespace case and rejected trailing relations, while the
    /// source path handled both); delegating here collapses the two so a
    /// document and its `SchemaSource` cannot lower to different schemas.
    ///
    /// The document entry point keeps its own *entry contract*, narrower
    /// than the source archive's: it accepts 4 roots (generics input output
    /// namespace) or 5 with leading imports, and rejects the trailing
    /// relations form. Within that contract the `SchemaSource` it builds
    /// carries no relations, so the single lowering is well-defined.
    pub fn lower_document_with_resolver(
        &self,
        document: &Document,
        identity: SchemaIdentity,
        context: &mut MacroContext,
        resolver: &ImportResolver,
    ) -> Result<TrueSchema, SchemaError> {
        context.remember_structure_header(document.structure_header());

        if !matches!(document.holds_root_objects(), 4 | 5) {
            return Err(SchemaError::ExpectedRootObjectCount {
                expected: "4 root values (generics input output namespace) or 5 with leading imports",
                found: document.holds_root_objects(),
            });
        }

        // The c2dc seam: run the macro-registry dispatch as a pre-expansion
        // pass over the parsed document BEFORE the typed source archive is
        // built. The pass records every structural-macro firing and capture
        // binding into the context and rewrites user type-reference macro
        // invocations into their expanded built-in bodies, so the archive the
        // single source path lowers is already macro-expanded. The source path
        // stays the sole lowering semantics — the registry is the front-end,
        // not a rival lowerer.
        let expanded = MacroExpansionPass::new(&self.registry).expand(document, context)?;
        let source = SchemaSource::from_document(&expanded)?;
        // The document entry path admits imports + generics + input/output/namespace
        // only; a trailing relations block is a source-archive-only form.
        // `from_document` would read a non-brace 4th-or-later root as
        // relations, so reject any relations the reparse produced rather
        // than silently widening the document contract.
        if !source.relations().is_empty() {
            return Err(SchemaError::ExpectedRootObjectCount {
                expected: "4 root values (generics input output namespace) or 5 with leading imports",
                found: document.holds_root_objects(),
            });
        }
        self.lower_schema_source_with_resolver(&source, identity, resolver)
    }

    pub fn lower_source_with_resolver(
        &self,
        source: &str,
        identity: SchemaIdentity,
        context: &mut MacroContext,
        resolver: &ImportResolver,
    ) -> Result<TrueSchema, SchemaError> {
        let document = Document::parse(source)?;
        self.lower_document_with_resolver(&document, identity, context, resolver)
    }

    /// The engine's macro vocabulary. With the single-engine collapse (report
    /// 702) the registry no longer drives root/namespace lowering — that comes
    /// from the typed-source archive on every entry path — but it remains the
    /// public macro set an engine is built from (`with_registry`,
    /// `with_schema_defaults`) and the seam a future archive-time
    /// type-reference-macro expansion would consult.
    pub fn registry(&self) -> &MacroRegistry {
        &self.registry
    }
}

impl MacroRegistry {
    pub fn with_schema_defaults() -> Self {
        let mut registry = Self::new();
        registry.register_node_definition(MacroNodeDefinition::root_imports());
        registry.register_node_definition(MacroNodeDefinition::root_input());
        registry.register_node_definition(MacroNodeDefinition::root_output());
        registry.register_node_definition(MacroNodeDefinition::root_namespace());
        registry.register_node_definition(MacroNodeDefinition::namespace_declaration());
        registry.register_node_definition(MacroNodeDefinition::struct_fields());
        registry.register_node_definition(MacroNodeDefinition::enum_variants());
        registry.register_node_definition(MacroNodeDefinition::type_reference());
        registry.register(RootImportsMacro::new());
        registry.register(RootEnumMacro::new(
            "RootInput",
            MacroPosition::RootInput,
            "Input",
        ));
        registry.register(RootEnumMacro::new(
            "RootOutput",
            MacroPosition::RootOutput,
            "Output",
        ));
        registry.register(RootNamespaceMacro::new());
        registry.register(KeyValueDeclarationMacro::new());
        registry
    }
}

#[derive(Clone, Debug)]
struct KeyValueDeclarationMacro {
    signature: MacroSignature,
    node: MacroNodeDefinition,
}

impl KeyValueDeclarationMacro {
    fn new() -> Self {
        Self {
            signature: MacroSignature::new(
                "KeyValueDeclaration",
                MacroPosition::NamespaceDeclaration,
                "Name value",
            ),
            node: MacroNodeDefinition::namespace_declaration(),
        }
    }
}

impl SchemaMacroHandler for KeyValueDeclarationMacro {
    fn name(&self) -> &str {
        self.signature.name()
    }

    fn matches(&self, object: MacroObject<'_>, position: MacroPosition) -> bool {
        self.signature.accepts_position(position) && self.node.matches(object)
    }

    fn lower(
        &self,
        object: MacroObject<'_>,
        position: MacroPosition,
        context: &mut MacroContext,
        registry: &MacroRegistry,
    ) -> Result<MacroOutput, SchemaError> {
        self.signature.remember(position, context);
        let pair = object.pair().ok_or(SchemaError::ExpectedDelimiter {
            expected: self.signature.expected_delimiter(),
        })?;
        KeyValueDeclaration::new(pair)
            .lower(registry, context)
            .map(MacroOutput::Declaration)
    }
}

#[derive(Clone, Copy, Debug)]
struct KeyValueDeclaration<'schema> {
    pair: MacroPair<'schema>,
}

impl<'schema> KeyValueDeclaration<'schema> {
    fn new(pair: MacroPair<'schema>) -> Self {
        Self { pair }
    }

    /// Lower a namespace key/value pair into a public declaration. The
    /// key position is a [`DeclarationHead`]: a bare name, or a
    /// parameterized head `(| Name Param … |)` whose binders become the
    /// declaration's type parameters. The body lowers the same way for either
    /// head — the binders only change what the closure walk and arity
    /// validation later see — so the parameters are attached to the finished
    /// `Declaration` here.
    fn lower(
        &self,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<Declaration, SchemaError> {
        let (name, parameters) = DeclarationHead::from_block(self.pair.name)?.into_parts();
        let value = match self.pair.definition {
            Block::Delimited {
                delimiter: nota::Delimiter::Brace,
                root_objects,
                ..
            } => self.lower_struct(name, root_objects, registry, context)?,
            Block::Delimited {
                delimiter: nota::Delimiter::SquareBracket,
                root_objects,
                ..
            } => self.lower_enum(name, root_objects, registry, context)?,
            definition => self.lower_newtype(name, definition, registry, context)?,
        };
        Ok(Declaration::public(value).with_parameters(parameters))
    }

    fn lower_struct(
        &self,
        name: Name,
        root_objects: &'schema [Block],
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<TypeDeclaration, SchemaError> {
        MacroExpansionStructBody::from_blocks(name, root_objects).lower_type(registry, context)
    }

    fn lower_enum(
        &self,
        name: Name,
        root_objects: &'schema [Block],
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<TypeDeclaration, SchemaError> {
        let variants = MacroExpansionVariants::new(root_objects).lower(registry, context)?;
        Ok(TypeDeclaration::Enum(EnumDeclaration::new(name, variants)))
    }

    fn lower_newtype(
        &self,
        name: Name,
        definition: &'schema Block,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<TypeDeclaration, SchemaError> {
        // A `{| … |}` impl block is segmented off as a trailing block by the
        // namespace entry walk, so it never arrives as a newtype definition.
        // A `(| … |)` pipe-parenthesis is a head-position binder list and is
        // still illegal at a value position.
        if matches!(
            definition,
            Block::Delimited {
                delimiter: nota::Delimiter::PipeBrace | nota::Delimiter::PipeParenthesis,
                ..
            }
        ) {
            return Err(SchemaError::ExpectedDelimiter {
                expected: "namespace value reference, not pipe declaration block",
            });
        }
        let reference = TypeReference::from_block_with_registry(definition, registry, context)?;
        Ok(TypeDeclaration::Newtype(NewtypeDeclaration::new(
            name, reference,
        )))
    }
}

#[derive(Clone, Copy, Debug)]
struct MacroSignature {
    name: &'static str,
    position: MacroPosition,
    expected_delimiter: &'static str,
}

impl MacroSignature {
    fn new(name: &'static str, position: MacroPosition, expected_delimiter: &'static str) -> Self {
        Self {
            name,
            position,
            expected_delimiter,
        }
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn expected_delimiter(&self) -> &'static str {
        self.expected_delimiter
    }

    fn accepts_position(&self, position: MacroPosition) -> bool {
        position == self.position
    }

    fn remember(&self, position: MacroPosition, context: &mut MacroContext) {
        context.remember_macro(self.name);
        context.remember_position(position);
    }
}

#[derive(Clone, Debug)]
struct RootImportsMacro {
    signature: MacroSignature,
}

impl RootImportsMacro {
    fn new() -> Self {
        Self {
            signature: MacroSignature::new("RootImports", MacroPosition::RootImports, "{ }"),
        }
    }
}

impl SchemaMacroHandler for RootImportsMacro {
    fn name(&self) -> &str {
        self.signature.name()
    }

    fn matches(&self, object: MacroObject<'_>, position: MacroPosition) -> bool {
        self.signature.accepts_position(position)
            && object.block().is_some_and(|block| block.is_brace())
    }

    fn lower(
        &self,
        object: MacroObject<'_>,
        position: MacroPosition,
        context: &mut MacroContext,
        _registry: &MacroRegistry,
    ) -> Result<MacroOutput, SchemaError> {
        self.signature.remember(position, context);
        let body = object.delimited_body(Delimiter::Brace, self.signature.expected_delimiter())?;
        if body.root_objects().len() % 2 != 0 {
            return Err(SchemaError::ExpectedEvenMapEntries {
                found: body.root_objects().len(),
            });
        }

        let mut imports = Vec::new();
        for chunk in body.root_objects().chunks_exact(2) {
            let local_name = chunk[0].schema_name()?;
            let source = chunk[1].schema_name()?;
            imports.push(ImportDeclaration {
                local_name,
                source: TypeReference::from_name(source),
            });
        }
        Ok(MacroOutput::Imports(imports))
    }
}

#[derive(Clone, Debug)]
struct RootNamespaceMacro {
    signature: MacroSignature,
}

impl RootNamespaceMacro {
    fn new() -> Self {
        Self {
            signature: MacroSignature::new("RootNamespace", MacroPosition::RootNamespace, "{ }"),
        }
    }
}

impl SchemaMacroHandler for RootNamespaceMacro {
    fn name(&self) -> &str {
        self.signature.name()
    }

    fn matches(&self, object: MacroObject<'_>, position: MacroPosition) -> bool {
        self.signature.accepts_position(position)
            && object.block().is_some_and(|block| block.is_brace())
    }

    fn lower(
        &self,
        object: MacroObject<'_>,
        position: MacroPosition,
        context: &mut MacroContext,
        registry: &MacroRegistry,
    ) -> Result<MacroOutput, SchemaError> {
        self.signature.remember(position, context);
        let body = object.delimited_body(Delimiter::Brace, self.signature.expected_delimiter())?;
        Ok(MacroOutput::Types(
            NamespaceBlock::new(body).lower_declarations(registry, context)?,
        ))
    }
}

#[derive(Clone, Copy, Debug)]
struct NamespaceBlock<'schema> {
    body: NotaBody<'schema>,
}

impl<'schema> NamespaceBlock<'schema> {
    fn new(body: NotaBody<'schema>) -> Self {
        Self { body }
    }

    fn lower_declarations(
        &self,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<Vec<Declaration>, SchemaError> {
        self.lower_key_value_declarations(registry, context)
    }

    fn lower_key_value_declarations(
        &self,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<Vec<Declaration>, SchemaError> {
        let mut declarations = Vec::new();
        for pair in self.key_value_pairs()? {
            let name = DeclarationHead::from_block(pair.name)?.into_parts().0;
            if TypeReference::is_reserved_scalar_name(&name) {
                return Err(SchemaError::ReservedScalarTypeName {
                    name: name.as_str().to_owned(),
                });
            }
            self.push_declaration(
                MacroObject::Pair(pair),
                registry,
                context,
                &mut declarations,
            )?;
        }
        Ok(declarations)
    }

    /// Segment the namespace body into key/value pairs the macro lowerer
    /// understands. The body is no longer a flat `chunks_exact(2)` map: an
    /// entry is a head, an optional inline body, and an optional trailing
    /// `{| … |}` impl block (a separate root object). Body-bearing entries
    /// become a [`MacroPair`]; body-optional entries (`TypeName {| … |}`)
    /// mint no type declaration on the macro path and are dropped here — the
    /// typed source archive carries their impls. This walk mirrors
    /// `SourceNamespaceWalk` in `source.rs`; the two must stay in lockstep.
    fn key_value_pairs(&self) -> Result<Vec<MacroPair<'schema>>, SchemaError> {
        let mut pairs = Vec::new();
        let mut walk = NamespaceEntryWalk::new(self.body.root_objects());
        while let Some(entry) = walk.next_entry()? {
            if let Some(definition) = entry.definition {
                pairs.push(MacroPair {
                    name: entry.name,
                    definition,
                });
            }
        }
        Ok(pairs)
    }

    fn push_declaration(
        &self,
        object: MacroObject<'schema>,
        registry: &MacroRegistry,
        context: &mut MacroContext,
        declarations: &mut Vec<Declaration>,
    ) -> Result<(), SchemaError> {
        if object
            .pair()
            .is_some_and(|pair| MetadataDefinitionProbe::new(pair.definition).matches())
        {
            return Ok(());
        }
        let inline_start = context.inline_declaration_count();
        match registry.lower(object, MacroPosition::NamespaceDeclaration, context)? {
            MacroOutput::Declaration(declaration) => {
                declarations.extend(context.drain_inline_declarations_from(inline_start));
                declarations.push(declaration);
            }
            MacroOutput::Type(declaration) => {
                declarations.extend(context.drain_inline_declarations_from(inline_start));
                declarations.push(Declaration::public(declaration));
            }
            _ => {
                return Err(SchemaError::UnexpectedMacroOutput {
                    macro_name: "TypeDeclaration".to_owned(),
                    expected: "type",
                });
            }
        }
        Ok(())
    }
}

/// One segmented namespace entry on the macro path: a head block and an
/// optional inline body (the `definition`). The trailing `{| … |}` impl
/// block is recognised and skipped by the walk — the macro lowerer consumes
/// only the body; the typed source archive (`SourceNamespaceEntry`) is what
/// carries the impl catalog.
#[derive(Clone, Copy, Debug)]
struct NamespaceEntry<'schema> {
    name: &'schema Block,
    definition: Option<&'schema Block>,
}

/// A cursor over a namespace body that segments it into [`NamespaceEntry`]s
/// using the same head / optional-body / optional-pipe-brace grammar as
/// `source.rs`'s `SourceNamespaceWalk`. Keeping the two walks identical is
/// what stops the macro lowering and the typed-source lowering from
/// diverging.
#[derive(Clone, Copy, Debug)]
struct NamespaceEntryWalk<'schema> {
    objects: &'schema [Block],
    cursor: usize,
}

impl<'schema> NamespaceEntryWalk<'schema> {
    fn new(objects: &'schema [Block]) -> Self {
        Self { objects, cursor: 0 }
    }

    fn next_entry(&mut self) -> Result<Option<NamespaceEntry<'schema>>, SchemaError> {
        let Some(head) = self.objects.get(self.cursor) else {
            return Ok(None);
        };
        if head.is_pipe_brace() {
            return Err(SchemaError::ExpectedDelimiter {
                expected: "a type name before a {| … |} impl block, not a leading impl block",
            });
        }
        self.cursor += 1;

        let definition = match self.objects.get(self.cursor) {
            Some(next) if !next.is_pipe_brace() => {
                self.cursor += 1;
                Some(next)
            }
            _ => None,
        };

        let has_impls = match self.objects.get(self.cursor) {
            Some(next) if next.is_pipe_brace() => {
                self.cursor += 1;
                true
            }
            _ => false,
        };

        if definition.is_none() && !has_impls {
            return Err(SchemaError::ExpectedDelimiter {
                expected: "a namespace entry body or a {| … |} impl block",
            });
        }

        Ok(Some(NamespaceEntry {
            name: head,
            definition,
        }))
    }
}

/// Whether a namespace entry's value is a schema-metadata definition —
/// a stream or family declaration — rather than a type declaration.
/// Metadata entries are excluded from namespace type lowering and are
/// collected through the typed `SchemaSource` reading instead.
#[derive(Clone, Copy, Debug)]
struct MetadataDefinitionProbe<'schema> {
    definition: &'schema Block,
}

impl<'schema> MetadataDefinitionProbe<'schema> {
    fn new(definition: &'schema Block) -> Self {
        Self { definition }
    }

    fn matches(&self) -> bool {
        let Block::Delimited {
            delimiter: Delimiter::Parenthesis,
            root_objects,
            ..
        } = self.definition
        else {
            return false;
        };
        root_objects
            .first()
            .and_then(Block::demote_to_string)
            .is_some_and(|head| matches!(head, "Stream" | "Family"))
    }
}

#[derive(Clone, Debug)]
struct RootEnumMacro {
    signature: MacroSignature,
    enum_name: &'static str,
}

impl RootEnumMacro {
    fn new(name: &'static str, position: MacroPosition, enum_name: &'static str) -> Self {
        Self {
            signature: MacroSignature::new(name, position, "[ ]"),
            enum_name,
        }
    }
}

impl SchemaMacroHandler for RootEnumMacro {
    fn name(&self) -> &str {
        self.signature.name()
    }

    fn matches(&self, object: MacroObject<'_>, position: MacroPosition) -> bool {
        // A root position accepts either the enum-body form `[Variant …]`
        // or the application form `(Head Arg …)` — both lower through this
        // handler, dispatched on the delimiter at `lower`.
        self.signature.accepts_position(position)
            && object
                .block()
                .is_some_and(|block| block.is_square_bracket() || block.is_parenthesis())
    }

    fn lower(
        &self,
        object: MacroObject<'_>,
        position: MacroPosition,
        context: &mut MacroContext,
        registry: &MacroRegistry,
    ) -> Result<MacroOutput, SchemaError> {
        self.signature.remember(position, context);
        let object = object.block().ok_or(SchemaError::ExpectedDelimiter {
            expected: self.signature.expected_delimiter(),
        })?;
        if object.is_parenthesis() {
            return RootApplicationBlock::new(object, self.enum_name)
                .lower(registry, context)
                .map(MacroOutput::RootApplication);
        }
        let root_enum = RootEnumBlock::from_block(object, self.enum_name)?;
        let name = root_enum.name();
        let variants = root_enum.variants(registry, context)?;
        Ok(MacroOutput::RootEnum(EnumDeclaration::new(name, variants)))
    }
}

/// The application-form root `(Head Arg …)` at an Input/Output position. It
/// lowers through the *same* `TypeReference::from_block_with_registry`
/// parenthesis decode a field-position application takes, so the head and
/// arguments resolve identically; the only root-specific addition is the
/// position name (`Input` / `Output`) the root is identified by, since an
/// application carries no declaration name of its own. A parenthesis at a
/// root position that does not decode to an application (a built-in head
/// like `(Vector T)`, or a collection form) is rejected as a non-root form.
#[derive(Clone, Copy, Debug)]
struct RootApplicationBlock<'schema> {
    block: &'schema Block,
    position_name: &'static str,
}

impl<'schema> RootApplicationBlock<'schema> {
    fn new(block: &'schema Block, position_name: &'static str) -> Self {
        Self {
            block,
            position_name,
        }
    }

    fn lower(
        &self,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<RootApplication, SchemaError> {
        let reference = TypeReference::from_block_with_registry(self.block, registry, context)?;
        let TypeReference::Application { head, arguments } = reference else {
            return Err(SchemaError::ExpectedRootApplication {
                position: self.position_name,
                found: reference.to_nota(),
            });
        };
        Ok(RootApplication::new(
            Name::new(self.position_name),
            head,
            arguments,
        ))
    }
}

#[derive(Clone, Copy, Debug)]
struct RootEnumBlock<'schema> {
    variants: &'schema [Block],
    enum_name: &'static str,
}

impl<'schema> RootEnumBlock<'schema> {
    fn from_block(object: &'schema Block, enum_name: &'static str) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(object, Delimiter::SquareBracket, "root enum body")?;
        Ok(Self {
            variants: body.root_objects(),
            enum_name,
        })
    }

    fn name(&self) -> Name {
        Name::new(self.enum_name)
    }

    fn variants(
        &self,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<Vec<EnumVariant>, SchemaError> {
        MacroExpansionVariants::new(self.variants).lower(registry, context)
    }
}
