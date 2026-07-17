use nota::Document;

use crate::{ImportResolver, SchemaSource, TrueSchema, macros::MacroContext, schema::Name};

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
        "optional enum-variant payload {enum_name}::{variant_name}; a variant payload must always appear in the text form, so Optional.T is forbidden here — model the optional case as an explicit member carrying a required payload (for example a leaf enum with an explicit All member)"
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
    #[error("empty type reference")]
    EmptyTypeReference,
    #[error("unknown type reference form {head} with {argument_count} arguments")]
    UnknownTypeReferenceForm { head: String, argument_count: usize },
    #[error("reserved scalar type name {name}")]
    ReservedScalarTypeName { name: String },
    #[error("malformed import source: {found}")]
    MalformedImportSource { found: String },
    #[error(
        "malformed import target {target}; an import target must be a simple capitalized type \
         name, not a dotted path — write the path segments before the bracket, as in \
         crate.module.[Type]"
    )]
    MalformedImportTarget { target: String },
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
    #[error(
        "expected a capitalized type reference at a reference leaf, found the \
         lowercase-led name {found}"
    )]
    ExpectedTypeReferenceLeaf { found: String },
    #[error("expected {form} to hold {expected}, found {found} objects")]
    ExpectedSyntaxReferenceArity {
        form: &'static str,
        expected: &'static str,
        found: usize,
    },
    #[error("expected a syntax enum variant, found {found}")]
    ExpectedSyntaxEnumVariant { found: String },
    #[error(
        "ungrouped multi-argument variant payload for variant {variant}: the application head \
         {head} carries multiple arguments, which the dot rule requires be grouped — write the \
         grouped payload {variant}.({head}.(…)), for example Projected.(Map.(Key Value)), never \
         the left-associative {variant}.{head}.(…)"
    )]
    UngroupedVariantPayloadApplication { variant: String, head: String },
    #[error("duplicate source declaration {name}")]
    DuplicateSourceDeclaration { name: String },
    #[error(
        "duplicate declaration {name} in the loaded whole: a schema is one namespace, but {name} \
         is declared as both {first_site} and {second_site} — rename one, the local declaration or \
         the imported one at its source"
    )]
    DuplicateDeclaration {
        name: String,
        first_site: &'static str,
        second_site: &'static str,
    },
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
    /// A root Input/Output position did not decode to an enum body or dotted
    /// application form (`Head.(Arg …)`). A bare declared-name root, built-in
    /// collection, or any other non-application body is not legal.
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
    /// An impls-block entry `TypeName.[ … ]` names a target type that is not
    /// declared anywhere in the schema. An impl block must attach its catalog
    /// to a type declared in the types (or generics) block; an unresolved
    /// target is not an accepted free-standing impl over an arbitrary name.
    #[error("impl block targets type `{name}`, which is not declared in this schema")]
    UnresolvedImplTarget { name: String },
    /// A target type carries the same impl entry twice — the same trait
    /// marker repeated, or the same method signature repeated — across one or
    /// more impls-block entries. Distinct entries for one target compose; an
    /// identical entry is a true duplicate and a typed error.
    #[error("impl catalog for type `{target}` carries duplicate entry `{entry}`")]
    DuplicateImplEntry { target: String, entry: String },
    /// A trait atom inside an impl catalog is not a PascalCase
    /// type-name. Trait references obey the same naming gate as every other
    /// type reference: a lowercase or otherwise non-type-name atom in a trait
    /// position is a typed error, not a silently-accepted trait.
    #[error("impl block trait `{found}` is not a PascalCase type name")]
    NonTypeNameTrait { found: String },
    /// A rename through the `NameTable` named an identifier the table does not
    /// hold. Renaming preserves an existing identifier, so the identifier must
    /// already be bound before it can be renamed.
    #[error("name table has no identifier {identifier} to rename")]
    NameTableIdentifierAbsent { identifier: String },
    /// A rename through the `NameTable` tried to assign a name already held by a
    /// different identifier of the same kind. The table's identifier-to-name
    /// mapping is injective per kind, so a collision is a typed error rather
    /// than a silent double-binding of one name.
    #[error(
        "name {name} is already held by a different {kind:?} identifier {holder}, cannot rename {requested} to it"
    )]
    NameTableNameConflict {
        kind: crate::DeclarationKind,
        name: String,
        holder: String,
        requested: String,
    },
    /// A `CoreSchema` projection met an identifier the `NameTable` does not
    /// hold. The substrate carries only identifiers, so every one of them must
    /// resolve through the table to produce the human-facing view; a miss means
    /// the substrate and the table have diverged.
    #[error("name table has no entry for identifier {identifier}; cannot project its name")]
    CoreProjectionNameAbsent { identifier: String },
    /// A source atom read as a local declaration or reference name was not a
    /// well-formed local name: the `Name` namespace machinery (`local_part`,
    /// `qualified_under`) assumes a source-derived local name carries no `:`
    /// namespace separator and no empty segment, and this atom violated that.
    #[error(
        "source name `{name}` is not a well-formed local name (no `:` or empty segment allowed)"
    )]
    MalformedLocalName { name: String },
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

/// The schema lowering engine. Lowering has exactly one path: the parsed
/// next-gen document builds a [`SchemaSource`] archive directly, and native
/// kind dispatch drives the lowering. The retired macro-node registry that once
/// mediated this is gone; the engine carries no lowering state, but remains the
/// public noun the lowering methods hang on and the constructor consumers reach
/// through `SchemaEngine::default()`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SchemaEngine {}

impl SchemaEngine {
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

    /// Lower authored `.schema` source into the split target model: the
    /// stringless [`crate::CoreSchema`] substrate plus its [`crate::NameTable`].
    /// The name-bearing tree exists only transiently inside this lowering; the
    /// durable model is the returned pair, and the human-facing view is
    /// projected from it on demand through `crate::CoreSchema::project`.
    /// Identifiers re-associate against `prior` — pass
    /// [`crate::NameTable::empty`] when no prior table applies.
    pub fn lower_core_source(
        &self,
        source: &str,
        identity: SchemaIdentity,
        prior: &crate::NameTable,
    ) -> Result<(crate::CoreSchema, crate::NameTable), SchemaError> {
        let schema = self.lower_true_schema_source(source, identity)?;
        schema.tree().decompose(prior)
    }

    /// The resolver-carrying twin of [`SchemaEngine::lower_core_source`], for
    /// sources with cross-crate imports.
    pub fn lower_core_source_with_resolver(
        &self,
        source: &str,
        identity: SchemaIdentity,
        resolver: &ImportResolver,
        prior: &crate::NameTable,
    ) -> Result<(crate::CoreSchema, crate::NameTable), SchemaError> {
        let schema = self.lower_true_schema_source_with_resolver(source, identity, resolver)?;
        schema.tree().decompose(prior)
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
    /// The document entry point has the same strict entry contract as the
    /// source archive: exactly six root slots, in order: imports, input,
    /// output, types, generics, impls. Empty optional sections are typed empty
    /// roots (`{}` / `[]`), not omitted roots.
    pub fn lower_document_with_resolver(
        &self,
        document: &Document,
        identity: SchemaIdentity,
        context: &mut MacroContext,
        resolver: &ImportResolver,
    ) -> Result<TrueSchema, SchemaError> {
        context.remember_structure_header(document.structure_header());

        if document.holds_root_objects() < 6 {
            return Err(SchemaError::ExpectedRootObjectCount {
                expected: "6 root slots (imports input output types generics impls)",
                found: document.holds_root_objects(),
            });
        }

        // The typed source archive is the sole lowering semantics: the parsed
        // next-gen document builds a `SchemaSource` directly. Native kind
        // dispatch (Vector/Optional/Map as core-schema kinds) supersedes the
        // retired macro-node expansion pass, which no longer runs on this path.
        let source = SchemaSource::from_document(document)?;
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
}
