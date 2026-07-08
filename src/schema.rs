use std::fmt;

use nota::{
    AtomClassification, Block, Delimiter, NotaBlock, NotaBody, NotaDecode, NotaDecodeError,
    NotaEncode, NotaString, StructuralMacroNode,
};

use crate::{
    MacroContext, MacroObject, MacroOutput, MacroPosition, MacroRegistry, SchemaError,
    declarative::{MacroExpansionFields, MacroExpansionVariants},
    macros::{BlockDebug, SchemaBlockExt},
};

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, Hash, PartialEq)]
pub struct Name(String);

impl Name {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn namespace_segments(&self) -> Vec<&str> {
        self.0.split(':').collect()
    }

    pub fn local_part(&self) -> &str {
        self.namespace_segments()
            .into_iter()
            .last()
            .expect("split always yields at least one segment")
    }

    pub fn has_namespace(&self) -> bool {
        self.0.contains(':')
    }

    pub fn qualified_under(&self, namespace: Option<&Name>) -> Self {
        match namespace {
            Some(namespace) if !self.has_namespace() => {
                Self::new(format!("{}:{}", namespace.as_str(), self.as_str()))
            }
            Some(_) | None => self.clone(),
        }
    }

    pub fn field_name(&self) -> String {
        let mut output = String::new();
        for (index, character) in self.local_part().chars().enumerate() {
            if character.is_ascii_uppercase() {
                if index > 0 {
                    output.push('_');
                }
                output.push(character.to_ascii_lowercase());
            } else if character == '-' {
                output.push('_');
            } else {
                output.push(character);
            }
        }
        output
    }

    pub fn qualifies_as_symbol_name(&self) -> bool {
        AtomClassification::classify(self.as_str()) == AtomClassification::SymbolCandidate
    }

    /// Whether this name is a PascalCase symbol — a symbol-shaped atom whose
    /// local part begins with an ASCII uppercase letter. This is the head
    /// gate for the generic-application form: only a PascalCase head can name
    /// a parameterized type at a reference position.
    pub fn qualifies_as_pascal_case(&self) -> bool {
        self.qualifies_as_symbol_name()
            && self
                .local_part()
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_uppercase())
    }
}

impl NotaDecode for Name {
    fn from_nota_block(block: &Block) -> Result<Self, NotaDecodeError> {
        NotaBlock::new(block).parse_string().map(Self::new)
    }
}

impl NotaEncode for Name {
    fn to_nota(&self) -> String {
        if self.qualifies_as_symbol_name() {
            self.as_str().to_owned()
        } else {
            NotaString::new(self.as_str()).format()
        }
    }
}

/// A `Name` decodes from a bare symbol atom and re-emits through its NOTA
/// codec, so a structural-macro node can carry it as a head or leaf capture.
/// In the reference grammar the application form's `pascal_head` gate runs
/// first, so only a PascalCase atom reaches this decode there; the
/// symbol-case acceptance keeps the node usable wherever a qualified name is
/// already known to sit at the position.
impl nota::StructuralMacroNode for Name {
    type Error = SchemaError;

    fn structural_position() -> nota::PositionPredicate {
        nota::PositionPredicate::named("type name")
    }

    fn structural_variants() -> Vec<nota::StructuralVariant> {
        vec![
            nota::BlockShape::symbol(Some(nota::CaptureName::new("name")))
                .into_structural_variant("Name", "symbol atom"),
        ]
    }

    fn from_structural_block(
        block: &Block,
    ) -> Result<Self, nota::StructuralMacroError<Self::Error>> {
        block
            .schema_name()
            .map_err(nota::StructuralMacroError::MatchedNode)
    }

    fn from_structural_candidate(
        candidate: nota::MacroCandidate<'_>,
    ) -> Result<Self, nota::StructuralMacroError<Self::Error>> {
        match candidate.blocks() {
            [block] => Self::from_structural_block(block),
            blocks => Err(nota::StructuralMacroError::ExpectedSingleRoot {
                found: blocks.len(),
            }),
        }
    }

    fn to_structural_nota(&self) -> String {
        self.to_nota()
    }
}

impl fmt::Display for Name {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, Hash, PartialEq)]
pub struct SymbolPath(Vec<Name>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymbolPathPosition<'path> {
    Type {
        type_name: &'path Name,
    },
    RootVariant {
        root_name: &'path Name,
        variant_name: &'path Name,
    },
    Field {
        type_name: &'path Name,
        field_name: &'path Name,
    },
    EnumVariant {
        enum_name: &'path Name,
        variant_name: &'path Name,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchemaDeclaredType<'schema> {
    Root(&'schema EnumDeclaration),
    Namespace(&'schema TypeDeclaration),
}

impl SymbolPath {
    pub fn new(segments: impl IntoIterator<Item = Name>) -> Self {
        Self(segments.into_iter().collect())
    }

    pub fn from_identity_and_segments(
        identity: &super::SchemaIdentity,
        segments: impl IntoIterator<Item = Name>,
    ) -> Self {
        let mut path_segments = vec![identity.component().clone()];
        path_segments.extend(segments);
        Self::new(path_segments)
    }

    pub fn segments(&self) -> &[Name] {
        &self.0
    }

    pub fn component(&self) -> Option<&Name> {
        self.0.first()
    }

    pub fn local_segments(&self) -> &[Name] {
        self.0.get(1..).unwrap_or(&[])
    }

    pub fn belongs_to(&self, identity: &super::SchemaIdentity) -> bool {
        self.component()
            .is_some_and(|component| component == identity.component())
    }

    pub fn type_path(identity: &super::SchemaIdentity, type_name: &Name) -> Self {
        Self::from_identity_and_segments(identity, [type_name.clone()])
    }

    pub fn root_variant_path(
        identity: &super::SchemaIdentity,
        root_name: &Name,
        variant_name: &Name,
    ) -> Self {
        Self::from_identity_and_segments(identity, [root_name.clone(), variant_name.clone()])
    }

    pub fn field_path(
        identity: &super::SchemaIdentity,
        type_name: &Name,
        field_name: &Name,
    ) -> Self {
        Self::from_identity_and_segments(identity, [type_name.clone(), field_name.clone()])
    }

    pub fn enum_variant_path(
        identity: &super::SchemaIdentity,
        enum_name: &Name,
        variant_name: &Name,
    ) -> Self {
        Self::from_identity_and_segments(identity, [enum_name.clone(), variant_name.clone()])
    }
}

impl NotaDecode for SymbolPath {
    fn from_nota_block(block: &Block) -> Result<Self, NotaDecodeError> {
        let children =
            NotaBlock::new(block).expect_children(Delimiter::Parenthesis, "SymbolPath", 2)?;
        let variant = children[0]
            .demote_to_string()
            .ok_or(NotaDecodeError::ExpectedAtom {
                type_name: "SymbolPath variant",
            })?;
        if variant != "SymbolPath" {
            return Err(NotaDecodeError::UnknownVariant {
                enum_name: "SymbolPath",
                variant: variant.to_owned(),
            });
        }
        Ok(Self(Vec::<Name>::from_nota_block(&children[1])?))
    }
}

impl NotaEncode for SymbolPath {
    fn to_nota(&self) -> String {
        format!("(SymbolPath {})", self.0.to_nota())
    }
}

impl fmt::Display for SymbolPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let joined = self
            .segments()
            .iter()
            .map(Name::as_str)
            .collect::<Vec<_>>()
            .join("/");
        formatter.write_str(&joined)
    }
}

/// A component-root Input/Output position. Today the position forces an
/// enum body `[Variant …]`, but a root may also be a typed sum applied
/// at the position directly — `(Work SignalInput SemaWriteOutput …)` — an
/// application of an imported or locally-declared parameterized head. The
/// closed sum names the two shapes a root can take; nothing else is a
/// legal root.
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
pub enum Root {
    /// The enum-body root `[Variant …]` — the position lowers to a public
    /// enum declaration whose variants are the root's signatures.
    Enum(EnumDeclaration),
    /// The application-form root `(Head Arg …)` — the position is a typed
    /// sum produced by applying a parameterized head to its arguments. The
    /// application is boxed: an imported head carries a `ResolvedImport`, so
    /// an unboxed `RootApplication` would make `Root` (and every `TrueSchema`
    /// holding two roots) carry that weight even for the common enum root.
    Application(Box<RootApplication>),
}

impl Root {
    /// Build an application root from its parts, boxing the application.
    pub fn application(application: RootApplication) -> Self {
        Self::Application(Box::new(application))
    }

    /// The root's identity name: an enum root carries its declaration name,
    /// an application root carries its position name (`Input` / `Output`).
    pub fn name(&self) -> &Name {
        match self {
            Self::Enum(declaration) => &declaration.name,
            Self::Application(application) => application.name(),
        }
    }

    /// The enum declaration when this root is the enum-body form; `None`
    /// for an application root. Callers that genuinely need the variant
    /// list (symbol-path resolution, variant lookup) read through this.
    pub fn as_enum(&self) -> Option<&EnumDeclaration> {
        match self {
            Self::Enum(declaration) => Some(declaration),
            Self::Application(_) => None,
        }
    }

    /// The application when this root is the application form; `None` for
    /// an enum root.
    pub fn as_application(&self) -> Option<&RootApplication> {
        match self {
            Self::Application(application) => Some(application.as_ref()),
            Self::Enum(_) => None,
        }
    }
}

/// A root in the application form `(Head Arg …)`: a parameterized head
/// applied to a tail of type-reference arguments, standing at a root
/// Input/Output position. It mirrors [`TypeReference::Application`]'s shape
/// but carries the position name the root is identified by, since an
/// application has no declaration name of its own. The content-address
/// closure reuses the field-position `Application` walk by projecting this
/// root back into a [`TypeReference::Application`] (see [`TypeReference`]'s
/// `From<&RootApplication>`).
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
pub struct RootApplication {
    name: Name,
    head: ApplicationHead,
    arguments: Vec<TypeReference>,
}

impl RootApplication {
    pub fn new(name: Name, head: ApplicationHead, arguments: Vec<TypeReference>) -> Self {
        Self {
            name,
            head,
            arguments,
        }
    }

    /// The position name this application root is identified by
    /// (`Input` / `Output`).
    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn head(&self) -> &ApplicationHead {
        &self.head
    }

    pub fn arguments(&self) -> &[TypeReference] {
        &self.arguments
    }

    /// Monomorphize a parameterized frame at this application's root position:
    /// substitute each declared frame binder with the corresponding application
    /// argument throughout the frame's variants, yielding the concrete
    /// `EnumVariant` list this applied root denotes. The frame's variant
    /// *names* (`SignalArrived`, `CommandSemaWrite`, …) are fixed by the frame;
    /// only the payload references are substituted. `frame_parameters` are the
    /// binders the frame head introduced and `frame_variants` is the frame's
    /// declared variant list; their pairing is the same expansion the
    /// equivalence tests assert leg-for-leg against the concrete baseline.
    ///
    /// The argument count must equal the binder count — the arity the frame
    /// fixed (validated at lowering by `arities_verified`); a mismatch is a
    /// caller bug.
    pub fn expand_with(
        &self,
        frame_parameters: &[Name],
        frame_variants: &[EnumVariant],
    ) -> Vec<EnumVariant> {
        frame_variants
            .iter()
            .map(|variant| EnumVariant {
                name: variant.name.clone(),
                payload: variant
                    .payload
                    .as_ref()
                    .map(|payload| self.substitute_binder(frame_parameters, payload)),
                stream_relation: variant.stream_relation.clone(),
            })
            .collect()
    }

    /// Replace a frame binder reference with the argument bound to it; leave
    /// any other reference untouched. The frame's payloads are bare binder
    /// references (`Event`, `WriteDone`, …), so a single-level substitution
    /// covers every leg. A nested application argument (the recursive
    /// Continuation leg) is carried through unchanged — the applied root that
    /// owns it lowers it by sibling reference, not by re-expansion.
    fn substitute_binder(
        &self,
        frame_parameters: &[Name],
        payload: &TypeReference,
    ) -> TypeReference {
        let TypeReference::Plain(name) = payload else {
            return payload.clone();
        };
        frame_parameters
            .iter()
            .position(|parameter| parameter == name)
            .map(|index| self.arguments[index].clone())
            .unwrap_or_else(|| payload.clone())
    }
}

impl From<&RootApplication> for TypeReference {
    /// Project the application root back into a field-position application
    /// reference, so the existing `Application` closure walk and arity
    /// validation cover it without a second code path.
    fn from(application: &RootApplication) -> Self {
        Self::Application {
            head: application.head.clone(),
            arguments: application.arguments.clone(),
        }
    }
}

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
pub struct GenericDefinition {
    name: Name,
    builtin: GenericBuiltin,
}

impl GenericDefinition {
    pub fn new(name: Name, builtin: GenericBuiltin) -> Self {
        Self { name, builtin }
    }

    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn builtin(&self) -> &GenericBuiltin {
        &self.builtin
    }

    pub fn parameter_count(&self) -> usize {
        self.builtin.parameter_count()
    }

    pub fn frame_body(&self) -> Option<(&[Name], &[EnumVariant])> {
        self.builtin.frame_body()
    }

    pub fn to_schema_text(&self) -> String {
        format!("{} {}", self.name.to_nota(), self.builtin.to_schema_text())
    }
}

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
pub enum GenericBuiltin {
    Vector,
    Optional,
    ScopeOf,
    Map,
    FixedBytes,
    Frame(GenericFrame),
}

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
pub struct GenericFrame {
    parameters: Vec<Name>,
    variants: Vec<EnumVariant>,
}

impl GenericFrame {
    pub fn new(parameters: Vec<Name>, variants: Vec<EnumVariant>) -> Self {
        Self {
            parameters,
            variants,
        }
    }

    pub fn parameters(&self) -> &[Name] {
        &self.parameters
    }

    pub fn variants(&self) -> &[EnumVariant] {
        &self.variants
    }
}

impl GenericBuiltin {
    pub fn parameter_count(&self) -> usize {
        match self {
            Self::Vector | Self::Optional | Self::ScopeOf | Self::FixedBytes => 1,
            Self::Map => 2,
            Self::Frame(frame) => frame.parameters().len(),
        }
    }

    pub fn frame_body(&self) -> Option<(&[Name], &[EnumVariant])> {
        match self {
            Self::Frame(frame) => Some((frame.parameters(), frame.variants())),
            Self::Vector | Self::Optional | Self::ScopeOf | Self::Map | Self::FixedBytes => None,
        }
    }

    pub fn to_schema_text(&self) -> String {
        match self {
            Self::Vector => "Vector".to_owned(),
            Self::Optional => "Optional".to_owned(),
            Self::ScopeOf => "ScopeOf".to_owned(),
            Self::Map => "Map".to_owned(),
            Self::FixedBytes => "FixedBytes".to_owned(),
            Self::Frame(frame) => {
                let parameter_text =
                    Delimiter::SquareBracket.wrap(frame.parameters().iter().map(Name::to_nota));
                let variants = EnumDeclaration::new(Name::new("Frame"), frame.variants().to_vec())
                    .body_schema_text();
                Delimiter::Parenthesis.wrap(["Frame".to_owned(), parameter_text, variants])
            }
        }
    }
}

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
pub struct TrueSchema {
    identity: super::SchemaIdentity,
    imports: Vec<ImportDeclaration>,
    resolved_imports: Vec<super::ResolvedImport>,
    generics: Vec<GenericDefinition>,
    input: Root,
    output: Root,
    namespace: Vec<Declaration>,
    streams: Vec<StreamDeclaration>,
    families: Vec<FamilyDeclaration>,
    relations: Vec<RelationDeclaration>,
    impl_blocks: Vec<ImplBlock>,
}

impl TrueSchema {
    // The schema-language's fields are each a distinct typed section of the model;
    // the constructor takes them as separate typed vectors rather than a
    // bag struct. (Newer clippy raises `too_many_arguments`; the repo's
    // pinned 1.85 toolchain does not.)
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        identity: super::SchemaIdentity,
        imports: Vec<ImportDeclaration>,
        resolved_imports: Vec<super::ResolvedImport>,
        generics: Vec<GenericDefinition>,
        input: Root,
        output: Root,
        namespace: Vec<Declaration>,
        streams: Vec<StreamDeclaration>,
        families: Vec<FamilyDeclaration>,
        relations: Vec<RelationDeclaration>,
    ) -> Self {
        Self {
            identity,
            imports,
            resolved_imports,
            generics,
            input,
            output,
            namespace,
            streams,
            families,
            relations,
            impl_blocks: Vec::new(),
        }
    }

    /// Attach the standalone impl blocks lowered from body-optional
    /// `TypeName {| … |}` entries — impls for types declared elsewhere. The
    /// fused-form catalogs ride on their own `Declaration::impls`; these are
    /// the catalogs whose target type is declared by a separate entry.
    pub(crate) fn with_impl_blocks(mut self, impl_blocks: Vec<ImplBlock>) -> Self {
        self.impl_blocks = impl_blocks;
        self
    }

    pub fn identity(&self) -> &super::SchemaIdentity {
        &self.identity
    }

    pub fn imports(&self) -> &[ImportDeclaration] {
        &self.imports
    }

    /// The imports resolved against dependency crate schemas. Empty
    /// when the schema was lowered without an import resolver or when
    /// the schema declares no imports. The Rust emitter reads these to
    /// reference dependency-emitted types instead of re-declaring them.
    pub fn resolved_imports(&self) -> &[super::ResolvedImport] {
        &self.resolved_imports
    }

    pub fn generics(&self) -> &[GenericDefinition] {
        &self.generics
    }

    pub fn generic_named(&self, name: &str) -> Option<&GenericDefinition> {
        self.generics
            .iter()
            .find(|definition| definition.name().as_str() == name)
    }

    pub fn input(&self) -> &Root {
        &self.input
    }

    pub fn output(&self) -> &Root {
        &self.output
    }

    pub fn input_and_output(&self) -> [&Root; 2] {
        [self.input(), self.output()]
    }

    /// The root carrying the given position name. Either root shape
    /// answers — an enum root by its declaration name, an application root
    /// by its position name — so callers that only need the enum body
    /// chain `.and_then(Root::as_enum)`.
    pub fn root_named(&self, name: &str) -> Option<&Root> {
        self.input_and_output()
            .into_iter()
            .find(|root| root.name().as_str() == name)
    }

    /// The enum body of the root carrying the given position name; `None`
    /// when no such root exists or the root is an application form. Variant
    /// lookups (symbol paths, family records resolving to a root enum) go
    /// through this.
    pub fn root_enum_named(&self, name: &str) -> Option<&EnumDeclaration> {
        self.root_named(name).and_then(Root::as_enum)
    }

    pub fn namespace(&self) -> &[Declaration] {
        &self.namespace
    }

    /// The standalone impl blocks lowered from body-optional
    /// `TypeName {| … |}` entries (impls for elsewhere-declared types).
    pub fn impl_blocks(&self) -> &[ImplBlock] {
        &self.impl_blocks
    }

    /// The single enumerable impl manifest report 695 specifies: every
    /// referenced impl entry across the schema, each paired with the type it
    /// targets. It unions the fused catalogs carried on each `Declaration`
    /// with the standalone body-optional [`ImplBlock`]s. This is the walk
    /// the out-of-band trust boundary ([`RustSurface::verify_catalog`])
    /// consumes.
    pub fn referenced_impls(&self) -> Vec<ReferencedImpl<'_>> {
        let mut references = Vec::new();
        for declaration in &self.namespace {
            for entry in declaration.impls().entries() {
                references.push(ReferencedImpl {
                    target: declaration.name(),
                    entry,
                });
            }
        }
        for block in &self.impl_blocks {
            for entry in block.catalog().entries() {
                references.push(ReferencedImpl {
                    target: block.target(),
                    entry,
                });
            }
        }
        references
    }

    pub fn streams(&self) -> &[StreamDeclaration] {
        &self.streams
    }

    pub fn families(&self) -> &[FamilyDeclaration] {
        &self.families
    }

    pub fn relations(&self) -> &[RelationDeclaration] {
        &self.relations
    }

    /// Confirm every declared family's record type resolves to a
    /// namespace declaration, a root enum, or a declared import. Both
    /// lowering paths call this after assembly, so an unresolvable
    /// record name is a typed error rather than a silent dead family.
    pub(crate) fn families_verified(self) -> Result<Self, SchemaError> {
        for family in &self.families {
            if !self.family_record_resolves(&family.record) {
                return Err(SchemaError::FamilyRecordNotFound {
                    family: family.name.as_str().to_owned(),
                    record: family.record.as_str().to_owned(),
                });
            }
        }
        Ok(self)
    }

    pub(crate) fn product_components_verified(self) -> Result<Self, SchemaError> {
        for declaration in &self.namespace {
            if let TypeDeclaration::Struct(declaration) = declaration.value() {
                declaration.fields.product_components_verified()?;
            }
        }
        Ok(self)
    }

    fn family_record_resolves(&self, record: &Name) -> bool {
        self.type_named(record.as_str()).is_some()
            || self.root_enum_named(record.as_str()).is_some()
            || self
                .imports
                .iter()
                .any(|import| &import.local_name == record)
    }

    pub fn type_named(&self, name: &str) -> Option<&TypeDeclaration> {
        self.namespace
            .iter()
            .find(|declaration| declaration.name().as_str() == name)
            .map(Declaration::value)
    }

    pub fn declared_type_named(&self, name: &str) -> Option<SchemaDeclaredType<'_>> {
        self.type_named(name)
            .map(SchemaDeclaredType::Namespace)
            .or_else(|| self.root_enum_named(name).map(SchemaDeclaredType::Root))
    }

    /// The namespace declaration carrying the given name, with its
    /// declared type parameters attached. Roots are not parameterizable,
    /// so this is the namespace declaration only.
    fn namespace_declaration_named(&self, name: &str) -> Option<&Declaration> {
        self.namespace
            .iter()
            .find(|declaration| declaration.name().as_str() == name)
    }

    /// The declared generic arity of a named namespace type: the number
    /// of type parameters its declaration head introduced. `None` for a
    /// name that is not a namespace declaration (a root enum, an import,
    /// or an unknown name). A non-parameterized declaration reports
    /// `Some(0)`. The import resolver reads this across the crate
    /// boundary so a consumer can validate an imported head's arity.
    pub fn declared_parameter_count(&self, name: &str) -> Option<usize> {
        self.generic_named(name)
            .map(GenericDefinition::parameter_count)
            .or_else(|| {
                self.namespace_declaration_named(name)
                    .map(|declaration| declaration.parameters().len())
            })
    }

    /// The frame body of a declared parameterized enum: its binders and its
    /// variant list, paired for monomorphization at an application site. `None`
    /// when the name is not a namespace declaration or its declaration is not
    /// an enum. The import resolver reads this across the crate boundary so a
    /// consumer applying the imported head can expand the frame in place,
    /// substituting each binder with the application's argument.
    pub fn declared_frame_body(&self, name: &str) -> Option<(&[Name], &[EnumVariant])> {
        if let Some(definition) = self.generic_named(name)
            && let Some(frame) = definition.frame_body()
        {
            return Some(frame);
        }
        let declaration = self.namespace_declaration_named(name)?;
        let TypeDeclaration::Enum(body) = declaration.value() else {
            return None;
        };
        Some((declaration.parameters(), &body.variants))
    }

    /// The binders and variants of the frame an application head names,
    /// wherever the head's declaration lives. A `Local` head resolves first
    /// against this schema-language's namespace (a locally-declared parameterized
    /// enum), then against the resolved imports by local alias; an `Imported`
    /// head reads the body carried on its `ResolvedImport`. `None` when the
    /// head names no parameterized enum frame.
    fn frame_body_for_head<'head>(
        &'head self,
        head: &'head ApplicationHead,
    ) -> Option<(&'head [Name], &'head [EnumVariant])> {
        match head {
            ApplicationHead::Local(name) => self
                .declared_frame_body(name.as_str())
                .or_else(|| self.imported_frame_body(name)),
            ApplicationHead::Imported(import) => Some((import.parameters(), import.variants())),
        }
    }

    /// The frame body carried on the resolved import whose local alias is the
    /// given name. The lowered migrated nexus keeps an applied frame head as
    /// `ApplicationHead::Local(Work)` while the body travels on the matching
    /// `ResolvedImport`, so an applied root over an imported frame resolves
    /// here.
    fn imported_frame_body(&self, name: &Name) -> Option<(&[Name], &[EnumVariant])> {
        self.resolved_imports
            .iter()
            .find(|import| import.local_name() == name)
            .filter(|import| !import.variants().is_empty())
            .map(|import| (import.parameters(), import.variants()))
    }

    /// Monomorphize an application root into the concrete enum declaration it
    /// denotes: resolve the applied frame head's body, expand it by binder ->
    /// argument substitution, and re-aim any nested frame-application argument
    /// at the sibling root it reproduces. The Output root's Continuation leg
    /// binds spirit's own `(Work …)` application — structurally the Input
    /// root's application — so it lowers to a `Plain` reference to the sibling
    /// Input root by name rather than re-expanding inline. Recursion
    /// terminates: the Input frame (Work) carries no Continuation leg.
    ///
    /// The resulting `EnumDeclaration` is named by the root's position
    /// (`Input` / `Output`) and carries no type parameters — a fully concrete
    /// enum that flows through every concrete-enum emitter unchanged. `None`
    /// when the application head names no parameterized enum frame.
    pub fn expand_application_root(
        &self,
        application: &RootApplication,
    ) -> Option<EnumDeclaration> {
        let (parameters, variants) = self.frame_body_for_head(application.head())?;
        let expanded = application
            .expand_with(parameters, variants)
            .into_iter()
            .map(|variant| EnumVariant {
                name: variant.name,
                payload: variant
                    .payload
                    .map(|payload| self.reaim_sibling_application(&payload)),
                stream_relation: variant.stream_relation,
            })
            .collect();
        Some(EnumDeclaration::new(application.name().clone(), expanded))
    }

    /// Rewrite a payload that reproduces a sibling application root into a
    /// `Plain` reference to that root by name. The Output root's Continuation
    /// argument is spirit's own `(Work …)` application, identical to the Input
    /// root's application; the concrete enum must point its `Continue` leg at
    /// the sibling `Input` enum, not embed a second expansion. Any other
    /// payload passes through untouched.
    fn reaim_sibling_application(&self, payload: &TypeReference) -> TypeReference {
        let TypeReference::Application { head, arguments } = payload else {
            return payload.clone();
        };
        for root in self.input_and_output() {
            if let Some(sibling) = root.as_application()
                && sibling.head() == head
                && sibling.arguments() == arguments.as_slice()
            {
                return TypeReference::Plain(sibling.name().clone());
            }
        }
        payload.clone()
    }

    /// The generic arity an `Application` head must supply when the head
    /// resolves to a declared parameterized type. A locally-declared head
    /// reports its declaration's parameter count; a resolved import head
    /// reports the parameter count carried across the crate boundary.
    /// `None` means the head is not a declared parameterized type in this
    /// schema, so no arity is fixed here.
    fn declared_head_arity(&self, head: &ApplicationHead) -> Option<usize> {
        match head {
            ApplicationHead::Local(name) => self
                .generic_named(name.as_str())
                .map(GenericDefinition::parameter_count)
                .or_else(|| {
                    self.namespace_declaration_named(name.as_str())
                        .map(|declaration| declaration.parameters().len())
                })
                .or_else(|| {
                    self.resolved_imports
                        .iter()
                        .find(|import| import.local_name() == name)
                        .and_then(|import| import.parameter_count())
                }),
            ApplicationHead::Imported(import) => import.parameter_count(),
        }
    }

    /// Confirm every generic `Application` whose head resolves to a
    /// declared parameterized type supplies exactly that head's declared
    /// arity. This runs at lowering (decision O8), so a wrong argument
    /// count is a typed `GenericArityMismatch` rather than a deferred
    /// emitter failure. Heads that do not resolve to a declared
    /// parameterized type are left for the closure walk to judge.
    pub(crate) fn arities_verified(self) -> Result<Self, SchemaError> {
        for declaration in &self.namespace {
            self.verify_declaration_arities(declaration.value())?;
        }
        for root in self.input_and_output() {
            self.verify_root_arities(root)?;
        }
        Ok(self)
    }

    /// Verify the impl manifest: every standalone (body-optional) impl block
    /// targets a type declared elsewhere in this schema, and no target carries
    /// a true-duplicate entry. Distinct entries on one target compose; an
    /// identical trait marker or method signature repeated on a target is a
    /// typed error. Both lowering paths call this after assembly, so the
    /// macro/document path and the typed-source path validate the catalog
    /// identically.
    pub(crate) fn impls_verified(self) -> Result<Self, SchemaError> {
        for block in &self.impl_blocks {
            if self.type_named(block.target().as_str()).is_none() {
                return Err(SchemaError::UnresolvedImplTarget {
                    name: block.target().as_str().to_owned(),
                });
            }
        }
        self.impl_entries_distinct()?;
        Ok(self)
    }

    /// Walk the unioned manifest grouped by target; the first target that
    /// carries the same composition key twice is a duplicate. A `&` borrow
    /// holding the `ReferencedImpl` views is fine — the check is read-only.
    fn impl_entries_distinct(&self) -> Result<(), SchemaError> {
        let mut seen: Vec<(String, ImplCompositionKey)> = Vec::new();
        for reference in self.referenced_impls() {
            let target = reference.target().as_str().to_owned();
            let key = reference.entry().composition_key();
            let duplicate = seen.iter().any(|(existing_target, existing_key)| {
                *existing_target == target && *existing_key == key
            });
            if duplicate {
                return Err(SchemaError::DuplicateImplEntry {
                    target,
                    entry: reference.entry().label(),
                });
            }
            seen.push((target, key));
        }
        Ok(())
    }

    /// Arity-verify a root in either shape: an enum root verifies each
    /// variant payload; an application root verifies the application
    /// reference it projects to, so a wrong argument count against a
    /// declared parameterized head is the same typed error at the root
    /// position as at a field position.
    fn verify_root_arities(&self, root: &Root) -> Result<(), SchemaError> {
        match root {
            Root::Enum(declaration) => self.verify_enum_arities(declaration),
            Root::Application(application) => {
                self.verify_reference_arities(&TypeReference::from(application.as_ref()))
            }
        }
    }

    fn verify_declaration_arities(&self, declaration: &TypeDeclaration) -> Result<(), SchemaError> {
        match declaration {
            TypeDeclaration::Struct(body) => {
                for field in body.fields.iter() {
                    self.verify_reference_arities(&field.reference)?;
                }
                Ok(())
            }
            TypeDeclaration::Newtype(body) => self.verify_reference_arities(&body.reference),
            TypeDeclaration::Enum(body) => self.verify_enum_arities(body),
        }
    }

    fn verify_enum_arities(&self, declaration: &EnumDeclaration) -> Result<(), SchemaError> {
        for variant in &declaration.variants {
            if let Some(payload) = &variant.payload {
                if matches!(payload, TypeReference::Optional(_)) {
                    return Err(SchemaError::OptionalVariantPayload {
                        enum_name: declaration.name.as_str().to_owned(),
                        variant_name: variant.name.as_str().to_owned(),
                    });
                }
                if let Some(payload_type) = variant.same_named_direct_payload_type() {
                    return Err(SchemaError::SameNamedVariantPayload {
                        enum_name: declaration.name.as_str().to_owned(),
                        variant_name: variant.name.as_str().to_owned(),
                        payload_type: payload_type.to_owned(),
                    });
                }
                self.verify_reference_arities(payload)?;
            }
        }
        Ok(())
    }

    fn verify_reference_arities(&self, reference: &TypeReference) -> Result<(), SchemaError> {
        match reference {
            TypeReference::String
            | TypeReference::Integer
            | TypeReference::Boolean
            | TypeReference::Path
            | TypeReference::Bytes
            | TypeReference::FixedBytes(_)
            | TypeReference::Plain(_) => Ok(()),
            TypeReference::Vector(inner)
            | TypeReference::Optional(inner)
            | TypeReference::ScopeOf(inner) => self.verify_reference_arities(inner),
            TypeReference::Map(key, value) => {
                self.verify_reference_arities(key)?;
                self.verify_reference_arities(value)
            }
            TypeReference::Application { head, arguments } => {
                if let Some(expected) = self.declared_head_arity(head)
                    && expected != arguments.len()
                {
                    return Err(SchemaError::GenericArityMismatch {
                        head: head.name().as_str().to_owned(),
                        expected,
                        found: arguments.len(),
                    });
                }
                for argument in arguments {
                    self.verify_reference_arities(argument)?;
                }
                Ok(())
            }
        }
    }

    pub fn type_path(&self, type_name: &str) -> Option<SymbolPath> {
        self.type_named(type_name)
            .map(TypeDeclaration::name)
            .map(|name| SymbolPath::type_path(&self.identity, name))
    }

    pub fn root_variant_path(&self, root_name: &str, variant_name: &str) -> Option<SymbolPath> {
        self.root_enum_named(root_name).and_then(|root| {
            root.variants
                .iter()
                .find(|variant| variant.name.as_str() == variant_name)
                .map(|variant| {
                    SymbolPath::root_variant_path(&self.identity, &root.name, &variant.name)
                })
        })
    }

    pub fn field_path(&self, type_name: &str, field_name: &str) -> Option<SymbolPath> {
        let TypeDeclaration::Struct(declaration) = self.type_named(type_name)? else {
            return None;
        };
        declaration
            .fields
            .iter()
            .find(|field| field.name.as_str() == field_name)
            .map(|field| SymbolPath::field_path(&self.identity, &declaration.name, &field.name))
    }

    pub fn enum_variant_path(&self, enum_name: &str, variant_name: &str) -> Option<SymbolPath> {
        let TypeDeclaration::Enum(declaration) = self.type_named(enum_name)? else {
            return None;
        };
        declaration
            .variants
            .iter()
            .find(|variant| variant.name.as_str() == variant_name)
            .map(|variant| {
                SymbolPath::enum_variant_path(&self.identity, &declaration.name, &variant.name)
            })
    }

    pub fn symbol_path_position<'path>(
        &self,
        path: &'path SymbolPath,
    ) -> Option<SymbolPathPosition<'path>> {
        if !path.belongs_to(&self.identity) {
            return None;
        }
        match path.local_segments() {
            [type_name] if self.type_named(type_name.as_str()).is_some() => {
                Some(SymbolPathPosition::Type { type_name })
            }
            [root_name, variant_name]
                if self
                    .root_enum_named(root_name.as_str())
                    .is_some_and(|root| root.has_variant(variant_name)) =>
            {
                Some(SymbolPathPosition::RootVariant {
                    root_name,
                    variant_name,
                })
            }
            [type_name, field_name]
                if self
                    .type_named(type_name.as_str())
                    .is_some_and(|declaration| declaration.has_field_named(field_name)) =>
            {
                Some(SymbolPathPosition::Field {
                    type_name,
                    field_name,
                })
            }
            [enum_name, variant_name]
                if self
                    .type_named(enum_name.as_str())
                    .is_some_and(|declaration| declaration.has_variant_named(variant_name)) =>
            {
                Some(SymbolPathPosition::EnumVariant {
                    enum_name,
                    variant_name,
                })
            }
            _ => None,
        }
    }

    pub fn to_schema_text(&self) -> String {
        let mut roots = vec![
            self.imports_schema_text(),
            self.generics_schema_text(),
            self.input.to_root_schema_text(),
            self.output.to_root_schema_text(),
            self.namespace_schema_text(),
        ];
        if !self.relations.is_empty() {
            roots.push(
                Delimiter::SquareBracket.wrap(
                    self.relations
                        .iter()
                        .map(RelationDeclaration::to_schema_text),
                ),
            );
        }
        roots.join("\n")
    }

    fn imports_schema_text(&self) -> String {
        if self.imports.is_empty() {
            return "{}".to_owned();
        }
        let imports = self
            .imports
            .iter()
            .map(|import| {
                format!(
                    "  {} {}",
                    import.local_name.to_nota(),
                    import.source.to_structural_nota()
                )
            })
            .collect::<Vec<_>>();
        format!("{{\n{}\n}}", imports.join("\n"))
    }

    fn generics_schema_text(&self) -> String {
        if self.generics.is_empty() {
            return "{}".to_owned();
        }
        let generics = self
            .generics
            .iter()
            .map(|definition| format!("  {}", definition.to_schema_text()))
            .collect::<Vec<_>>();
        format!("{{\n{}\n}}", generics.join("\n"))
    }

    fn namespace_schema_text(&self) -> String {
        let mut entries = Vec::new();
        entries.extend(self.namespace.iter().map(Declaration::to_schema_text));
        entries.extend(self.streams.iter().map(StreamDeclaration::to_schema_text));
        entries.extend(self.families.iter().map(FamilyDeclaration::to_schema_text));
        entries.extend(self.impl_blocks.iter().map(ImplBlock::to_schema_text));
        if entries.is_empty() {
            return "{}".to_owned();
        }
        let indented = entries
            .into_iter()
            .map(|entry| format!("  {entry}"))
            .collect::<Vec<_>>();
        format!("{{\n{}\n}}", indented.join("\n"))
    }

    pub fn from_binary_bytes(bytes: &[u8]) -> Result<Self, SchemaError> {
        rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes).map_err(|_| SchemaError::ArchiveDecode)
    }

    pub fn to_binary_bytes(&self) -> Result<Vec<u8>, SchemaError> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .map(|bytes| bytes.to_vec())
            .map_err(|_| SchemaError::ArchiveEncode)
    }
}

impl Root {
    fn to_root_schema_text(&self) -> String {
        match self {
            Self::Enum(declaration) => declaration.body_schema_text(),
            Self::Application(application) => {
                TypeReference::from(application.as_ref()).to_structural_nota()
            }
        }
    }
}

impl Declaration {
    fn to_schema_text(&self) -> String {
        let head = if self.parameters.is_empty() {
            self.name.to_nota()
        } else {
            let mut items = Vec::with_capacity(self.parameters.len() + 1);
            items.push(self.name.to_nota());
            items.extend(self.parameters.iter().map(Name::to_nota));
            Delimiter::PipeParenthesis.wrap(items)
        };
        let mut parts = vec![head, self.value.to_schema_text()];
        if !self.impls.is_empty() {
            parts.push(self.impls.to_schema_text());
        }
        parts.join(" ")
    }
}

impl TypeDeclaration {
    fn to_schema_text(&self) -> String {
        match self {
            Self::Struct(declaration) => declaration.body_schema_text(),
            Self::Enum(declaration) => declaration.body_schema_text(),
            Self::Newtype(declaration) => declaration.reference.to_structural_nota(),
        }
    }
}

impl StructDeclaration {
    fn body_schema_text(&self) -> String {
        self.fields.to_schema_text()
    }
}

impl StructFieldMap {
    fn to_schema_text(&self) -> String {
        if self.is_empty() {
            return "{}".to_owned();
        }
        let fields = self
            .iter()
            .map(|field| field.to_schema_text(self))
            .collect::<Vec<_>>();
        format!("{{ {} }}", fields.join(" "))
    }

    fn product_components_verified(&self) -> Result<(), SchemaError> {
        for field in self.iter() {
            let occurrences = self.reference_count(&field.reference);
            let derived = field.reference.derived_field_name();
            if occurrences == 1 && field.name != derived {
                return Err(SchemaError::ExplicitFieldOnUniqueProductComponent {
                    field: field.name.to_string(),
                    type_name: field.reference.to_structural_nota(),
                });
            }
            if occurrences > 1 && field.name == derived {
                return Err(SchemaError::DuplicateImplicitProductComponent {
                    type_name: field.reference.to_structural_nota(),
                });
            }
            if occurrences > 1
                && self
                    .iter()
                    .filter(|candidate| candidate.reference == field.reference)
                    .filter(|candidate| candidate.name == field.name)
                    .count()
                    > 1
            {
                return Err(SchemaError::DuplicateExplicitProductComponentIdentity {
                    field: field.name.to_string(),
                    type_name: field.reference.to_structural_nota(),
                });
            }
        }
        Ok(())
    }

    fn reference_count(&self, reference: &TypeReference) -> usize {
        self.iter()
            .filter(|field| field.reference == *reference)
            .count()
    }
}

impl FieldDeclaration {
    fn to_schema_text(&self, product: &StructFieldMap) -> String {
        let reference = self.reference.to_structural_nota();
        let derived = self.reference.derived_field_name();
        if product.reference_count(&self.reference) == 1 && self.name == derived {
            reference
        } else {
            format!("{}.{}", self.name.to_nota(), reference)
        }
    }
}

impl EnumDeclaration {
    fn body_schema_text(&self) -> String {
        Delimiter::SquareBracket.wrap(self.variants.iter().map(EnumVariant::to_schema_text))
    }
}

impl EnumVariant {
    fn to_schema_text(&self) -> String {
        match (&self.payload, &self.stream_relation) {
            (None, None) => self.name.to_nota(),
            (Some(payload), None) if payload.plain_name() == Some(&self.name) => {
                Delimiter::Parenthesis.wrap([self.name.to_nota()])
            }
            (Some(payload), None) => {
                Delimiter::Parenthesis.wrap([self.name.to_nota(), payload.to_structural_nota()])
            }
            (Some(payload), Some(relation)) => Delimiter::Parenthesis.wrap([
                self.name.to_nota(),
                payload.to_structural_nota(),
                relation.keyword_text().to_owned(),
                relation.stream_name().to_nota(),
            ]),
            (None, Some(_)) => self.name.to_nota(),
        }
    }
}

impl StreamRelation {
    fn keyword_text(&self) -> &'static str {
        match self {
            Self::Opens(_) => "opens",
            Self::Belongs(_) => "belongs",
        }
    }
}

impl StreamDeclaration {
    fn to_schema_text(&self) -> String {
        format!(
            "{} (Stream {{ token.{} opened.{} event.{} close.{} }})",
            self.name.to_nota(),
            self.token.to_structural_nota(),
            self.opened.to_structural_nota(),
            self.event.to_structural_nota(),
            self.close.to_structural_nota(),
        )
    }
}

impl FamilyDeclaration {
    fn to_schema_text(&self) -> String {
        format!(
            "{} (Family {{ record.{} table.{} key.{} }})",
            self.name.to_nota(),
            self.record.to_nota(),
            self.table.to_nota(),
            self.key.to_nota(),
        )
    }
}

impl ImplBlock {
    fn to_schema_text(&self) -> String {
        format!(
            "{} {}",
            self.target.to_nota(),
            self.catalog.to_schema_text()
        )
    }
}

impl ImplCatalog {
    fn to_schema_text(&self) -> String {
        Delimiter::PipeBrace.wrap(self.entries.iter().map(ImplReference::to_schema_text))
    }
}

impl ImplReference {
    fn to_schema_text(&self) -> String {
        match self {
            Self::Marker(trait_name) => trait_name.to_nota(),
            Self::TraitImpl(trait_name, methods) => format!(
                "{} {}",
                trait_name.to_nota(),
                Delimiter::SquareBracket.wrap(methods.iter().map(MethodSignature::to_schema_text))
            ),
            Self::InherentMethod(signature) => signature.to_schema_text(),
        }
    }
}

impl MethodSignature {
    fn to_schema_text(&self) -> String {
        let parameters =
            Delimiter::Brace.wrap(self.parameters.iter().map(MethodParameter::to_schema_text));
        Delimiter::Parenthesis.wrap([
            self.name.to_nota(),
            parameters,
            self.return_reference.to_structural_nota(),
        ])
    }
}

impl MethodParameter {
    fn to_schema_text(&self) -> String {
        format!(
            "{}.{}",
            self.name.to_nota(),
            self.reference.to_structural_nota()
        )
    }
}

impl RelationDeclaration {
    fn to_schema_text(&self) -> String {
        match self {
            Self::Equivalence(values) => Delimiter::Parenthesis.wrap([
                "Equivalence".to_owned(),
                Delimiter::SquareBracket.wrap(values.iter().map(RelationValue::to_schema_text)),
            ]),
        }
    }
}

impl RelationValue {
    fn to_schema_text(&self) -> String {
        match self.path.as_slice() {
            [] => Delimiter::Parenthesis.wrap(Vec::<String>::new()),
            [name] => name.to_nota(),
            names => Delimiter::Parenthesis.wrap(names.iter().map(Name::to_nota)),
        }
    }
}

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
pub enum RelationDeclaration {
    Equivalence(Vec<RelationValue>),
}

impl RelationDeclaration {
    pub fn values(&self) -> &[RelationValue] {
        match self {
            Self::Equivalence(values) => values,
        }
    }
}

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
pub struct RelationValue {
    path: Vec<Name>,
}

impl RelationValue {
    pub fn new(path: Vec<Name>) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &[Name] {
        &self.path
    }
}

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
pub struct ImportDeclaration {
    pub local_name: Name,
    pub source: TypeReference,
}

#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    nota::NotaDecode,
    nota::NotaEncode,
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
)]
pub enum Visibility {
    Public,
    Private,
}

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
pub struct Declaration {
    visibility: Visibility,
    name: Name,
    parameters: Vec<Name>,
    value: TypeDeclaration,
    impls: ImplCatalog,
}

impl Declaration {
    pub fn public(value: TypeDeclaration) -> Self {
        Self::new(Visibility::Public, value)
    }

    pub fn private(value: TypeDeclaration) -> Self {
        Self::new(Visibility::Private, value)
    }

    fn new(visibility: Visibility, value: TypeDeclaration) -> Self {
        let name = value.name().clone();
        Self {
            visibility,
            name,
            parameters: Vec::new(),
            value,
            impls: ImplCatalog::empty(),
        }
    }

    /// Attach declared type parameters to this declaration. The
    /// parameter names are the binders the parameterized declaration
    /// head `(| Name Param … |)` introduces; references to them inside the
    /// body resolve as type-parameter binders, and their count is the
    /// declaration's generic arity that an `Application` must match.
    pub fn with_parameters(mut self, parameters: Vec<Name>) -> Self {
        self.parameters = parameters;
        self
    }

    /// Attach the lowered `{| … |}` impl catalog to this declaration. The
    /// catalog is a *reference* to impls/methods that already exist on the
    /// Rust side — markers and callable method signatures — not a generated
    /// body. A declaration with no trailing impl block carries
    /// `ImplCatalog::empty()`.
    pub fn with_impls(mut self, impls: ImplCatalog) -> Self {
        self.impls = impls;
        self
    }

    /// The lowered impl catalog referenced by this declaration's trailing
    /// `{| … |}` block. Empty for a declaration with no impl block. This is
    /// the per-type reach of the enumerable manifest; the schema-wide walk
    /// ([`TrueSchema::referenced_impls`]) unions these with the standalone
    /// body-optional impl blocks.
    pub fn impls(&self) -> &ImplCatalog {
        &self.impls
    }

    pub fn name(&self) -> &Name {
        &self.name
    }

    /// The declared type parameters of this declaration, in order. Empty
    /// for an ordinary (non-parameterized) declaration. The length is the
    /// declaration's generic arity.
    pub fn parameters(&self) -> &[Name] {
        &self.parameters
    }

    pub fn visibility(&self) -> Visibility {
        self.visibility
    }

    pub fn is_private(&self) -> bool {
        self.visibility == Visibility::Private
    }

    pub fn value(&self) -> &TypeDeclaration {
        &self.value
    }
}

/// The lowered `{| … |}` impl catalog: an enumerable list of impl
/// *references* — markers, body-bearing trait impls, and inherent method
/// signatures — that name impls/methods already present on the Rust side.
/// It carries no generated body; it is the data the out-of-band trust
/// boundary ([`RustSurface::verify_catalog`]) checks against a real crate
/// surface.
#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    nota::NotaDecode,
    nota::NotaEncode,
    Clone,
    Debug,
    Default,
    Eq,
    PartialEq,
)]
pub struct ImplCatalog {
    entries: Vec<ImplReference>,
}

impl ImplCatalog {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn new(entries: Vec<ImplReference>) -> Self {
        Self { entries }
    }

    pub fn entries(&self) -> &[ImplReference] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// One entry inside a lowered impl catalog. A marker is a bare trait impl
/// with no methods; a trait impl carries the method signatures the trait
/// requires; an inherent method is a single callable signature on the
/// target type.
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
pub enum ImplReference {
    /// A bare trait atom — a marker impl with no method signatures.
    Marker(Name),
    /// A trait atom plus the method signatures it requires.
    TraitImpl(Name, Vec<MethodSignature>),
    /// A single inherent method signature.
    InherentMethod(MethodSignature),
}

impl ImplReference {
    /// The trait this entry names, if any. Inherent methods name no trait.
    pub fn trait_name(&self) -> Option<&Name> {
        match self {
            Self::Marker(trait_name) | Self::TraitImpl(trait_name, _) => Some(trait_name),
            Self::InherentMethod(_) => None,
        }
    }

    /// The method signatures this entry references. A marker references
    /// none; a trait impl references its required methods; an inherent
    /// method references exactly itself.
    pub fn methods(&self) -> &[MethodSignature] {
        match self {
            Self::Marker(_) => &[],
            Self::TraitImpl(_, methods) => methods,
            Self::InherentMethod(signature) => std::slice::from_ref(signature),
        }
    }

    /// The composition identity of this entry — the key that decides whether
    /// two entries on one target *compose* (distinct keys union) or *collide*
    /// (identical keys are a true duplicate). A trait entry (marker or
    /// body-bearing) keys on its trait name, so `Display` and `Display [ … ]`
    /// are the same impl; an inherent method keys on its full signature, so
    /// two methods collide only when their whole signature matches.
    pub fn composition_key(&self) -> ImplCompositionKey {
        match self {
            Self::Marker(trait_name) | Self::TraitImpl(trait_name, _) => {
                ImplCompositionKey::Trait(trait_name.as_str().to_owned())
            }
            Self::InherentMethod(signature) => ImplCompositionKey::Method(signature.render()),
        }
    }

    /// A short legible label naming this entry, for the duplicate-entry
    /// error: the trait name for a trait/marker entry, the full signature
    /// rendering for an inherent method.
    pub fn label(&self) -> String {
        match self {
            Self::Marker(trait_name) | Self::TraitImpl(trait_name, _) => {
                trait_name.as_str().to_owned()
            }
            Self::InherentMethod(signature) => signature.render(),
        }
    }
}

/// The composition identity of an [`ImplReference`] under one target type —
/// distinct keys compose (union), identical keys are a true duplicate. A
/// trait entry keys on its trait name; an inherent method keys on its full
/// signature rendering.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImplCompositionKey {
    Trait(String),
    Method(String),
}

/// A lowered callable method signature `(name { params } Return)` — the
/// Work-frame-leg shape, with parameter and return types resolved to
/// [`TypeReference`]s. It names a signature that must exist on the Rust
/// side; the body is not generated.
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
pub struct MethodSignature {
    name: Name,
    parameters: Vec<MethodParameter>,
    return_reference: TypeReference,
}

impl MethodSignature {
    pub fn new(
        name: Name,
        parameters: Vec<MethodParameter>,
        return_reference: TypeReference,
    ) -> Self {
        Self {
            name,
            parameters,
            return_reference,
        }
    }

    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn parameters(&self) -> &[MethodParameter] {
        &self.parameters
    }

    pub fn return_reference(&self) -> &TypeReference {
        &self.return_reference
    }

    /// A canonical, full rendering of this signature — name, parameter
    /// names and types, and the return type. Two signatures render equal
    /// exactly when they are `Eq`, so the rendering is the identity used
    /// both for duplicate detection and for the legible signature carried
    /// in an unverified-reference error (so a name-matches-but-params-differ
    /// mismatch reads as a full signature, not a bare method name).
    pub fn render(&self) -> String {
        let parameters = self
            .parameters
            .iter()
            .map(|parameter| {
                format!(
                    "{}.{}",
                    parameter.name().as_str(),
                    parameter.reference().to_nota()
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            "{} {{ {} }} {}",
            self.name.as_str(),
            parameters,
            self.return_reference.to_nota()
        )
    }
}

/// One lowered method parameter: a name and its resolved type reference.
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
pub struct MethodParameter {
    name: Name,
    reference: TypeReference,
}

impl MethodParameter {
    pub fn new(name: Name, reference: TypeReference) -> Self {
        Self { name, reference }
    }

    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn reference(&self) -> &TypeReference {
        &self.reference
    }
}

/// A standalone lowered impl block for a type declared elsewhere — the
/// lowered form of a body-optional `TypeName {| … |}` entry. The named
/// `target` is resolved by ordinary symbol lookup; this block mints no
/// type declaration, it only attaches a catalog to the existing type.
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
pub struct ImplBlock {
    target: Name,
    catalog: ImplCatalog,
}

impl ImplBlock {
    pub fn new(target: Name, catalog: ImplCatalog) -> Self {
        Self { target, catalog }
    }

    pub fn target(&self) -> &Name {
        &self.target
    }

    pub fn catalog(&self) -> &ImplCatalog {
        &self.catalog
    }
}

/// A single impl entry from the schema-wide manifest, paired with the type
/// it targets. Borrowed view produced by [`TrueSchema::referenced_impls`]; the
/// `target` is the declaring (fused) type or the body-optional block's
/// referenced type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReferencedImpl<'schema> {
    target: &'schema Name,
    entry: &'schema ImplReference,
}

impl<'schema> ReferencedImpl<'schema> {
    pub fn target(&self) -> &Name {
        self.target
    }

    pub fn entry(&self) -> &ImplReference {
        self.entry
    }
}

/// One fact about the impls/methods actually present on the Rust side: a
/// trait (or marker) implemented for a type, or an inherent method with a
/// specific signature. A [`RustSurface`] is a set of these; it is the
/// out-of-band catalog the schema-language's impl references are verified against,
/// declared from a real crate scan (or, in tests, by hand).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImplFact {
    /// A trait (or marker trait) implemented for the named type.
    Trait { type_name: Name, trait_name: Name },
    /// A method present on the named type, identified by its canonical
    /// signature rendering (name, parameter types, return type) so that a
    /// signature mismatch counts as absent.
    Method {
        type_name: Name,
        signature: MethodSignature,
    },
}

impl ImplFact {
    pub fn trait_impl(type_name: Name, trait_name: Name) -> Self {
        Self::Trait {
            type_name,
            trait_name,
        }
    }

    pub fn method(type_name: Name, signature: MethodSignature) -> Self {
        Self::Method {
            type_name,
            signature,
        }
    }
}

/// The available Rust surface: the set of [`ImplFact`]s a real crate
/// exposes. The schema-language's impl catalog is *out of band* from the crate, so
/// the trust boundary is verifying that every referenced trait/method
/// signature is actually present here before any code trusts the catalog.
/// [`Self::verify_catalog`] is that boundary check.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RustSurface {
    facts: Vec<ImplFact>,
}

impl RustSurface {
    pub fn new(facts: Vec<ImplFact>) -> Self {
        Self { facts }
    }

    /// Verify every impl reference in the schema against this surface. A
    /// marker or trait impl must find a matching trait fact for its target
    /// type; every method signature (a trait impl's required methods, or an
    /// inherent method) must find a matching method fact. The first
    /// reference with no matching fact fails with
    /// [`SchemaError::UnverifiedImplReference`] naming the exact target and
    /// signature; an all-present catalog returns `Ok(())`. This proves the
    /// out-of-band catalog can be checked without parsing a real crate.
    pub fn verify_catalog(&self, schema: &TrueSchema) -> Result<(), SchemaError> {
        for reference in schema.referenced_impls() {
            self.verify_reference(reference)?;
        }
        Ok(())
    }

    fn verify_reference(&self, reference: ReferencedImpl<'_>) -> Result<(), SchemaError> {
        let target = reference.target();
        if let Some(trait_name) = reference.entry().trait_name() {
            self.verify_trait(target, trait_name)?;
        }
        for signature in reference.entry().methods() {
            self.verify_method(target, signature)?;
        }
        Ok(())
    }

    fn verify_trait(&self, target: &Name, trait_name: &Name) -> Result<(), SchemaError> {
        let present = self.facts.iter().any(|fact| {
            matches!(
                fact,
                ImplFact::Trait { type_name, trait_name: present }
                    if type_name == target && present == trait_name
            )
        });
        if present {
            return Ok(());
        }
        Err(SchemaError::UnverifiedImplReference {
            target: target.as_str().to_owned(),
            kind: "trait impl",
            signature: trait_name.as_str().to_owned(),
        })
    }

    fn verify_method(&self, target: &Name, signature: &MethodSignature) -> Result<(), SchemaError> {
        let present = self.facts.iter().any(|fact| {
            matches!(
                fact,
                ImplFact::Method { type_name, signature: present }
                    if type_name == target && present == signature
            )
        });
        if present {
            return Ok(());
        }
        // Report the FULL signature, not the bare method name: a reference
        // whose name matches a present method but whose parameters or return
        // type differ is a real mismatch, and only the full rendering makes
        // that legible. `MethodSignature::render` is the same canonical
        // rendering the duplicate-entry check keys on.
        Err(SchemaError::UnverifiedImplReference {
            target: target.as_str().to_owned(),
            kind: "method signature",
            signature: signature.render(),
        })
    }
}

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
pub enum TypeDeclaration {
    Struct(StructDeclaration),
    Enum(EnumDeclaration),
    Newtype(NewtypeDeclaration),
}

impl TypeDeclaration {
    pub fn name(&self) -> &Name {
        match self {
            Self::Struct(declaration) => &declaration.name,
            Self::Newtype(declaration) => &declaration.name,
            Self::Enum(declaration) => &declaration.name,
        }
    }

    pub fn has_field_named(&self, field_name: &Name) -> bool {
        let Self::Struct(declaration) = self else {
            return false;
        };
        declaration
            .fields
            .iter()
            .any(|field| &field.name == field_name)
    }

    pub fn has_variant_named(&self, variant_name: &Name) -> bool {
        let Self::Enum(declaration) = self else {
            return false;
        };
        declaration.has_variant(variant_name)
    }
}

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
pub struct NewtypeDeclaration {
    pub name: Name,
    pub reference: TypeReference,
}

impl NewtypeDeclaration {
    pub fn new(name: Name, reference: TypeReference) -> Self {
        Self { name, reference }
    }
}

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
pub struct StructDeclaration {
    pub name: Name,
    pub fields: StructFieldMap,
}

impl StructDeclaration {
    pub fn new(name: Name, fields: Vec<FieldDeclaration>) -> Self {
        Self {
            name,
            fields: StructFieldMap::new(fields),
        }
    }
}

/// Ordered key/value representation of a struct definition in schema.
///
/// A struct declaration's long-form data is a brace-map shape:
/// each field name is the key and each `TypeReference` is the value.
/// The Rust storage preserves source order because rkyv layout and
/// generated struct field order are load-bearing, but the object is
/// semantically a field-name -> type-reference map.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct StructFieldMap {
    entries: Vec<FieldDeclaration>,
}

impl StructFieldMap {
    pub fn new(entries: Vec<FieldDeclaration>) -> Self {
        Self { entries }
    }

    pub fn entries(&self) -> &[FieldDeclaration] {
        &self.entries
    }

    pub fn iter(&self) -> std::slice::Iter<'_, FieldDeclaration> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn first(&self) -> Option<&FieldDeclaration> {
        self.entries.first()
    }
}

impl std::ops::Deref for StructFieldMap {
    type Target = [FieldDeclaration];

    fn deref(&self) -> &Self::Target {
        self.entries()
    }
}

impl<'fields> IntoIterator for &'fields StructFieldMap {
    type Item = &'fields FieldDeclaration;
    type IntoIter = std::slice::Iter<'fields, FieldDeclaration>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl NotaDecode for StructFieldMap {
    fn from_nota_block(block: &Block) -> Result<Self, NotaDecodeError> {
        let body = NotaBody::from_delimited(block, Delimiter::Brace, "StructFieldMap")?;
        let root_objects = body.root_objects();
        if root_objects.len() % 2 != 0 {
            return Err(NotaDecodeError::ExpectedRootCount {
                type_name: "StructFieldMap",
                expected: root_objects.len() + 1,
                found: root_objects.len(),
            });
        }
        let mut entries = Vec::new();
        for chunk in root_objects.chunks_exact(2) {
            entries.push(FieldDeclaration {
                name: Name::from_nota_block(&chunk[0])?,
                reference: TypeReference::from_nota_block(&chunk[1])?,
            });
        }
        Ok(Self::new(entries))
    }
}

impl NotaEncode for StructFieldMap {
    fn to_nota(&self) -> String {
        let mut fields = Vec::new();
        for entry in self.entries() {
            fields.push(entry.name.to_nota());
            fields.push(entry.reference.to_nota());
        }
        format!("{{{}}}", fields.join(" "))
    }
}

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
pub struct FieldDeclaration {
    pub name: Name,
    pub reference: TypeReference,
}

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
pub struct EnumDeclaration {
    pub name: Name,
    pub variants: Vec<EnumVariant>,
}

impl EnumDeclaration {
    pub fn new(name: Name, variants: Vec<EnumVariant>) -> Self {
        Self { name, variants }
    }

    pub fn has_variant(&self, variant_name: &Name) -> bool {
        self.variants
            .iter()
            .any(|variant| &variant.name == variant_name)
    }
}

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
pub struct EnumVariant {
    pub name: Name,
    pub payload: Option<TypeReference>,
    pub stream_relation: Option<StreamRelation>,
}

impl EnumVariant {
    pub fn new(name: Name, payload: Option<TypeReference>) -> Self {
        Self {
            name,
            payload,
            stream_relation: None,
        }
    }

    pub fn with_stream_relation(mut self, stream_relation: StreamRelation) -> Self {
        self.stream_relation = Some(stream_relation);
        self
    }

    pub(crate) fn same_named_direct_payload_type(&self) -> Option<&str> {
        let payload_name = self.payload.as_ref()?.plain_name()?;
        if payload_name.local_part() == self.name.local_part() {
            Some(payload_name.local_part())
        } else {
            None
        }
    }
}

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
pub enum StreamRelation {
    Opens(Name),
    Belongs(Name),
}

impl StreamRelation {
    pub fn stream_name(&self) -> &Name {
        match self {
            Self::Opens(name) | Self::Belongs(name) => name,
        }
    }
}

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
pub struct StreamDeclaration {
    pub name: Name,
    pub token: TypeReference,
    pub opened: TypeReference,
    pub event: TypeReference,
    pub close: TypeReference,
}

impl StreamDeclaration {
    pub fn new(
        name: Name,
        token: TypeReference,
        opened: TypeReference,
        event: TypeReference,
        close: TypeReference,
    ) -> Self {
        Self {
            name,
            token,
            opened,
            event,
            close,
        }
    }
}

/// The current storage coordinate of a record family. A table name is
/// not a schema symbol: renaming the table moves only this coordinate,
/// never the family's semantic identity.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct TableName(String);

impl TableName {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl NotaDecode for TableName {
    fn from_nota_block(block: &Block) -> Result<Self, NotaDecodeError> {
        NotaBlock::new(block).parse_string().map(Self::new)
    }
}

impl NotaEncode for TableName {
    fn to_nota(&self) -> String {
        if AtomClassification::classify(self.as_str()) == AtomClassification::SymbolCandidate {
            self.as_str().to_owned()
        } else {
            NotaString::new(self.as_str()).format()
        }
    }
}

impl fmt::Display for TableName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// How a stored record family is keyed: by a domain-supplied record
/// key, or by an engine-assigned record identifier. The two variants
/// mirror the two registration shapes a SEMA engine offers (a keyed
/// table descriptor versus an identified table descriptor).
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
pub enum FamilyKey {
    #[shape(keyword = "Domain")]
    Domain,
    #[shape(keyword = "Identified")]
    Identified,
}

/// A stored record family: schema metadata in the namespace map, on
/// the stream-declaration precedent. The family name is the stable
/// identity; the record type names the declaration whose closure is
/// the family's content identity; the table name is only the current
/// storage coordinate; the key kind selects the engine registration
/// shape.
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
pub struct FamilyDeclaration {
    pub name: Name,
    pub record: Name,
    pub table: TableName,
    pub key: FamilyKey,
}

impl FamilyDeclaration {
    pub fn new(name: Name, record: Name, table: TableName, key: FamilyKey) -> Self {
        Self {
            name,
            record,
            table,
            key,
        }
    }
}

/// The head of a generic application `(Foo A B …)`.
///
/// A head is a typed sum: a generic head may name a locally-declared
/// parameterized type (`Local`) or a cross-crate imported one (`Imported`).
/// NOTA decode never resolves imports, so a freshly-decoded application
/// always carries `Local(Name)`; import resolution rewrites the head to
/// `Imported` once the closure walk proves the name is an import. The
/// canonical NOTA projection of either is the bare head name.
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
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub enum ApplicationHead {
    Local(Name),
    Imported(super::ResolvedImport),
}

impl ApplicationHead {
    /// The head's local name — the name written at the application site,
    /// regardless of whether it has been resolved to an import yet.
    pub fn name(&self) -> &Name {
        match self {
            Self::Local(name) => name,
            Self::Imported(import) => import.local_name(),
        }
    }
}

/// The broad generic-application node `(Foo A B …)`, captured directly by
/// nota's `#[shape(pascal_head, body)]` derive: a PascalCase head atom
/// followed by a variable-arity tail of type-reference arguments. This is the
/// structural-macro seam for the application form — the head decodes as a
/// `Name` (always `Local` at decode time) and the tail decodes as a
/// `Vec<TypeReference>`. The derive is the single source of truth for matching
/// and re-emitting the form; this node lowers into [`TypeReference::Application`].
#[derive(Clone, Debug, Eq, PartialEq, nota::StructuralMacroNode)]
enum ApplicationNode {
    #[shape(pascal_head, body)]
    Application(Name, Vec<TypeReference>),
}

/// The fixed-width byte leaf `(Bytes N)`, captured through nota's
/// headed-atom structural shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq, nota::StructuralMacroNode)]
enum FixedBytesNode {
    #[shape(head = "Bytes", atom)]
    FixedBytes(u64),
}

/// A declaration's type-name position: either a bare `Name` (the ordinary
/// declaration head) or a pipe-parenthesized `(| Name Param Param … |)` head
/// that introduces type-parameter binders. Use-site generic application keeps
/// ordinary parentheses (`(Head Arg …)`); declaration binders use the pipe
/// form so binding syntax and application syntax remain structurally distinct.
/// The parameterized form still decodes through the captured-head +
/// variable-arity tail seam (`ApplicationNode`) after the delimiter gate —
/// each tail item must be a bare binder name (a `Plain` reference), since a
/// parameter is a binder, not an applied type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclarationHead {
    name: Name,
    parameters: Vec<Name>,
}

impl DeclarationHead {
    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn parameters(&self) -> &[Name] {
        &self.parameters
    }

    pub fn into_parts(self) -> (Name, Vec<Name>) {
        (self.name, self.parameters)
    }

    /// Decode the declaration-name position from its block. A bare symbol
    /// atom is an ordinary head with no parameters; a pipe-parenthesized
    /// `(| Name Param … |)` reuses the application seam after delimiter
    /// discrimination and lifts each binder out of the decoded tail.
    pub fn from_block(block: &Block) -> Result<Self, SchemaError> {
        match block {
            Block::Delimited {
                delimiter: Delimiter::PipeParenthesis,
                ..
            } => Err(SchemaError::ExpectedSyntaxDeclaration {
                found: format!(
                    "retired parameterized declaration head {}",
                    block.reemit_fallback()
                ),
            }),
            _ => Ok(Self {
                name: block.schema_name()?,
                parameters: Vec::new(),
            }),
        }
    }
}

/// A type at a reference position — a struct field's type, an enum
/// variant's payload, or an import source.
///
/// `String`, `Integer`, `Boolean`, and `Path` are reserved scalar leaves.
/// `Plain` is a declared-name leaf (`Topic`, `Magnitude`). `Vector`,
/// `Map`, `Optional`, and `ScopeOf` carry inner references, lowered from the
/// single canonical head spelling each: `(Vector T)`, `(Map K V)`,
/// `(Optional T)`, `(ScopeOf T)` — the earlier aliases (`Vec`, `Option`,
/// `Scope`, `KeyValue`) are gone and no longer parse. `Application` is the
/// broad generic-application form `(Foo A B …)`: any other PascalCase head
/// carrying a tail of type-reference arguments, decoded through the
/// `#[shape(pascal_head, body)]` structural-macro seam. Built-in heads are
/// dispatched first; the application form is the fallback.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub enum TypeReference {
    String,
    Integer,
    Boolean,
    Path,
    Bytes,
    FixedBytes(u64),
    Plain(Name),
    Vector(#[rkyv(omit_bounds)] Box<TypeReference>),
    Map(
        #[rkyv(omit_bounds)] Box<TypeReference>,
        #[rkyv(omit_bounds)] Box<TypeReference>,
    ),
    Optional(#[rkyv(omit_bounds)] Box<TypeReference>),
    ScopeOf(#[rkyv(omit_bounds)] Box<TypeReference>),
    Application {
        head: ApplicationHead,
        #[rkyv(omit_bounds)]
        arguments: Vec<TypeReference>,
    },
}

/// Reserved built-in reference heads and their canonical arities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReferenceHead {
    Vector,
    Optional,
    ScopeOf,
    Map,
    Bytes,
}

impl ReferenceHead {
    pub fn classify(head: &str) -> Option<Self> {
        match head {
            "Vector" => Some(Self::Vector),
            "Optional" => Some(Self::Optional),
            "ScopeOf" => Some(Self::ScopeOf),
            "Map" => Some(Self::Map),
            "Bytes" => Some(Self::Bytes),
            _ => None,
        }
    }

    pub fn node_arity(self) -> usize {
        match self {
            Self::Vector | Self::Optional | Self::ScopeOf | Self::Bytes => 2,
            Self::Map => 3,
        }
    }
}

impl NotaDecode for TypeReference {
    fn from_nota_block(block: &Block) -> Result<Self, NotaDecodeError> {
        if let Some(name) = block.demote_to_string() {
            return match name {
                "String" => Ok(Self::String),
                "Integer" => Ok(Self::Integer),
                "Boolean" => Ok(Self::Boolean),
                "Path" => Ok(Self::Path),
                "Bytes" => Ok(Self::Bytes),
                other => Err(NotaDecodeError::UnknownVariant {
                    enum_name: "TypeReference",
                    variant: other.to_owned(),
                }),
            };
        }
        let children = match block {
            Block::Delimited {
                delimiter: Delimiter::Parenthesis,
                root_objects,
                ..
            } => root_objects.as_slice(),
            _ => {
                return Err(NotaDecodeError::ExpectedDelimited {
                    type_name: "TypeReference",
                    delimiter: "(",
                });
            }
        };
        if children.is_empty() {
            return Err(NotaDecodeError::ExpectedRootCount {
                type_name: "TypeReference",
                expected: 1,
                found: 0,
            });
        }
        let variant = children[0]
            .demote_to_string()
            .ok_or(NotaDecodeError::ExpectedAtom {
                type_name: "TypeReference variant",
            })?;
        match variant {
            "Plain" => Ok(Self::Plain(Name::from_nota_block(&children[1])?)),
            "Vector" => Ok(Self::Vector(Box::new(Self::from_nota_block(&children[1])?))),
            "Optional" => Ok(Self::Optional(Box::new(Self::from_nota_block(
                &children[1],
            )?))),
            "ScopeOf" => Ok(Self::ScopeOf(Box::new(Self::from_nota_block(
                &children[1],
            )?))),
            "Map" => Self::from_nota_map_payload(children),
            "FixedBytes" => Ok(Self::FixedBytes(
                children[1]
                    .demote_to_string()
                    .and_then(|text| text.parse::<u64>().ok())
                    .ok_or(NotaDecodeError::ExpectedAtom {
                        type_name: "FixedBytes width",
                    })?,
            )),
            "Application" => Self::from_nota_application_payload(&children[1]),
            other => Err(NotaDecodeError::UnknownVariant {
                enum_name: "TypeReference",
                variant: other.to_owned(),
            }),
        }
    }
}

impl NotaEncode for TypeReference {
    fn to_nota(&self) -> String {
        match self {
            Self::String => "String".to_owned(),
            Self::Integer => "Integer".to_owned(),
            Self::Boolean => "Boolean".to_owned(),
            Self::Path => "Path".to_owned(),
            Self::Bytes => "Bytes".to_owned(),
            Self::FixedBytes(width) => format!("(FixedBytes {width})"),
            Self::Plain(name) => format!("(Plain {})", name.to_nota()),
            Self::Vector(reference) => format!("(Vector {})", reference.to_nota()),
            Self::Map(key, value) => format!("(Map {} {})", key.to_nota(), value.to_nota()),
            Self::Optional(reference) => format!("(Optional {})", reference.to_nota()),
            Self::ScopeOf(reference) => format!("(ScopeOf {})", reference.to_nota()),
            Self::Application { head, arguments } => {
                let arguments = arguments
                    .iter()
                    .map(Self::to_nota)
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("(Application ({} ({arguments})))", head.name().to_nota())
            }
        }
    }
}

/// `TypeReference` is itself a structural-macro node so typed reference
/// slots can decode the source-facing dotted grammar directly. A leaf is a
/// bare atom, unary invocation is `Head.Argument`, and multi-argument
/// invocation keeps the arguments as a grouped positional record such as
/// `Map.(Key Value)`. This is distinct from the canonical-only
/// `NotaEncode`/`NotaDecode` machine codec above.
impl nota::StructuralMacroNode for TypeReference {
    type Error = SchemaError;

    fn structural_position() -> nota::PositionPredicate {
        nota::PositionPredicate::named("TypeReference")
    }

    fn structural_variants() -> Vec<nota::StructuralVariant> {
        vec![
            nota::BlockShape::symbol(Some(nota::CaptureName::new("reference")))
                .into_structural_variant("TypeReference", "symbol reference atom"),
        ]
    }

    fn from_structural_block(
        block: &Block,
    ) -> Result<Self, nota::StructuralMacroError<Self::Error>> {
        TypeReferenceStructuralReader::from_block_slice(std::slice::from_ref(block))
            .map_err(nota::StructuralMacroError::MatchedNode)
    }

    fn from_structural_candidate(
        candidate: nota::MacroCandidate<'_>,
    ) -> Result<Self, nota::StructuralMacroError<Self::Error>> {
        TypeReferenceStructuralReader::from_block_references(candidate.blocks().to_vec())
            .map_err(nota::StructuralMacroError::MatchedNode)
    }

    fn to_structural_nota(&self) -> String {
        match self {
            Self::String => "String".to_owned(),
            Self::Integer => "Integer".to_owned(),
            Self::Boolean => "Boolean".to_owned(),
            Self::Path => "Path".to_owned(),
            Self::Bytes => "Bytes".to_owned(),
            Self::FixedBytes(width) => format!("Bytes.{width}"),
            Self::Plain(name) => name.to_nota(),
            Self::Vector(reference) => {
                Self::structural_invocation_nota(&Name::new("Vector"), &[reference.as_ref()])
            }
            Self::Map(key, value) => {
                Self::structural_invocation_nota(&Name::new("Map"), &[key.as_ref(), value.as_ref()])
            }
            Self::Optional(reference) => {
                Self::structural_invocation_nota(&Name::new("Optional"), &[reference.as_ref()])
            }
            Self::ScopeOf(reference) => {
                Self::structural_invocation_nota(&Name::new("ScopeOf"), &[reference.as_ref()])
            }
            Self::Application { head, arguments } => {
                Self::structural_invocation_nota(head.name(), &arguments.iter().collect::<Vec<_>>())
            }
        }
    }
}

struct TypeReferenceStructuralReader<'block> {
    blocks: Vec<&'block Block>,
    cursor: usize,
}

impl<'block> TypeReferenceStructuralReader<'block> {
    fn from_block_slice(blocks: &'block [Block]) -> Result<TypeReference, SchemaError> {
        Self::from_block_references(blocks.iter().collect())
    }

    fn from_block_references(blocks: Vec<&'block Block>) -> Result<TypeReference, SchemaError> {
        let mut reader = Self::new(blocks);
        let reference = reader.read_reference()?;
        reader.expect_finished()?;
        Ok(reference)
    }

    fn new(blocks: Vec<&'block Block>) -> Self {
        Self { blocks, cursor: 0 }
    }

    fn read_reference(&mut self) -> Result<TypeReference, SchemaError> {
        let Some(block) = self.blocks.get(self.cursor) else {
            return Err(SchemaError::ExpectedSyntaxReferenceArity {
                form: "dotted type reference",
                expected: "a reference atom or dotted argument group",
                found: 0,
            });
        };
        match block {
            Block::Atom(atom) => {
                self.cursor += 1;
                self.read_atom_text(atom.text())
            }
            _ => Err(SchemaError::ExpectedSyntaxReference {
                found: block.reemit_fallback(),
            }),
        }
    }

    fn read_atom_text(&mut self, text: &str) -> Result<TypeReference, SchemaError> {
        let Some(prefix) = text.strip_suffix('.') else {
            return TypeReferenceDottedInvocation::new(text).without_group();
        };
        let Some(arguments_block) = self.blocks.get(self.cursor) else {
            return Err(SchemaError::ExpectedSyntaxReferenceArity {
                form: "dotted type reference invocation",
                expected: "a parenthesized argument group after the trailing dot",
                found: 1,
            });
        };
        self.cursor += 1;
        let arguments = Self::arguments_from_block(arguments_block)?;
        TypeReferenceDottedInvocation::new(prefix).with_group(arguments)
    }

    fn arguments_from_block(block: &'block Block) -> Result<Vec<TypeReference>, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::Parenthesis, "type arguments")?;
        let mut reader = Self::new(body.root_objects().iter().collect());
        let mut arguments = Vec::new();
        while !reader.is_finished() {
            arguments.push(reader.read_reference()?);
        }
        Ok(arguments)
    }

    fn is_finished(&self) -> bool {
        self.cursor >= self.blocks.len()
    }

    fn expect_finished(&self) -> Result<(), SchemaError> {
        if self.is_finished() {
            return Ok(());
        }
        Err(SchemaError::ExpectedSyntaxReferenceArity {
            form: "dotted type reference",
            expected: "one complete reference",
            found: self.blocks.len() - self.cursor,
        })
    }
}

struct TypeReferenceDottedInvocation<'text> {
    text: &'text str,
}

impl<'text> TypeReferenceDottedInvocation<'text> {
    fn new(text: &'text str) -> Self {
        Self { text }
    }

    fn without_group(&self) -> Result<TypeReference, SchemaError> {
        let segments = self.segments()?;
        Self::nest_unary(&segments)
    }

    fn with_group(&self, arguments: Vec<TypeReference>) -> Result<TypeReference, SchemaError> {
        let segments = self.segments()?;
        Self::nest_grouped(&segments, arguments)
    }

    fn segments(&self) -> Result<Vec<Name>, SchemaError> {
        if self.text.is_empty() {
            return Err(SchemaError::ExpectedSyntaxReference {
                found: "empty dotted type reference head".to_owned(),
            });
        }
        let mut segments = Vec::new();
        for segment in self.text.split('.') {
            if segment.is_empty() {
                return Err(SchemaError::ExpectedSyntaxReference {
                    found: self.text.to_owned(),
                });
            }
            segments.push(Name::new(segment));
        }
        Ok(segments)
    }

    fn nest_unary(segments: &[Name]) -> Result<TypeReference, SchemaError> {
        let Some((head, tail)) = segments.split_first() else {
            return Err(SchemaError::ExpectedSyntaxReference {
                found: "empty dotted type reference head".to_owned(),
            });
        };
        if tail.is_empty() {
            return Ok(TypeReference::from_name(head.clone()));
        }
        Self::resolve_invocation(head.clone(), vec![Self::nest_unary(tail)?])
    }

    fn nest_grouped(
        segments: &[Name],
        arguments: Vec<TypeReference>,
    ) -> Result<TypeReference, SchemaError> {
        let Some((head, tail)) = segments.split_first() else {
            return Err(SchemaError::ExpectedSyntaxReference {
                found: "empty dotted type reference head".to_owned(),
            });
        };
        if tail.is_empty() {
            return Self::resolve_invocation(head.clone(), arguments);
        }
        Self::resolve_invocation(head.clone(), vec![Self::nest_grouped(tail, arguments)?])
    }

    fn resolve_invocation(
        head: Name,
        arguments: Vec<TypeReference>,
    ) -> Result<TypeReference, SchemaError> {
        match ReferenceHead::classify(head.as_str()) {
            Some(ReferenceHead::Vector) => Self::resolve_unary_builtin(head, arguments)
                .map(|argument| TypeReference::Vector(Box::new(argument))),
            Some(ReferenceHead::Optional) => Self::resolve_unary_builtin(head, arguments)
                .map(|argument| TypeReference::Optional(Box::new(argument))),
            Some(ReferenceHead::ScopeOf) => Self::resolve_unary_builtin(head, arguments)
                .map(|argument| TypeReference::ScopeOf(Box::new(argument))),
            Some(ReferenceHead::Map) => Self::resolve_map(head, arguments),
            Some(ReferenceHead::Bytes) => Self::resolve_fixed_bytes(head, arguments),
            None => Ok(TypeReference::Application {
                head: ApplicationHead::Local(head),
                arguments,
            }),
        }
    }

    fn resolve_unary_builtin(
        head: Name,
        arguments: Vec<TypeReference>,
    ) -> Result<TypeReference, SchemaError> {
        let [argument]: [TypeReference; 1] =
            arguments.try_into().map_err(|arguments: Vec<_>| {
                SchemaError::GenericArityMismatch {
                    head: head.as_str().to_owned(),
                    expected: 1,
                    found: arguments.len(),
                }
            })?;
        Ok(argument)
    }

    fn resolve_map(
        head: Name,
        arguments: Vec<TypeReference>,
    ) -> Result<TypeReference, SchemaError> {
        let [key, value]: [TypeReference; 2] =
            arguments.try_into().map_err(|arguments: Vec<_>| {
                SchemaError::GenericArityMismatch {
                    head: head.as_str().to_owned(),
                    expected: 2,
                    found: arguments.len(),
                }
            })?;
        Ok(TypeReference::Map(Box::new(key), Box::new(value)))
    }

    fn resolve_fixed_bytes(
        head: Name,
        arguments: Vec<TypeReference>,
    ) -> Result<TypeReference, SchemaError> {
        let [width]: [TypeReference; 1] = arguments.try_into().map_err(|arguments: Vec<_>| {
            SchemaError::GenericArityMismatch {
                head: head.as_str().to_owned(),
                expected: 1,
                found: arguments.len(),
            }
        })?;
        let TypeReference::Plain(width) = width else {
            return Err(SchemaError::ExpectedSyntaxReference {
                found: head.to_nota(),
            });
        };
        width
            .as_str()
            .parse::<u64>()
            .map(TypeReference::FixedBytes)
            .map_err(|_| SchemaError::ExpectedSyntaxReference {
                found: format!("{}.{}", head.to_nota(), width.to_nota()),
            })
    }
}

impl TypeReference {
    fn structural_invocation_nota(head: &Name, arguments: &[&TypeReference]) -> String {
        match arguments {
            [argument] if !argument.needs_group_as_single_structural_argument() => {
                format!("{}.{}", head.to_nota(), argument.to_structural_nota())
            }
            [argument] => format!("{}.({})", head.to_nota(), argument.to_structural_nota()),
            _ => {
                let argument_text = arguments
                    .iter()
                    .map(|argument| argument.to_structural_nota())
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("{}.({argument_text})", head.to_nota())
            }
        }
    }

    fn needs_group_as_single_structural_argument(&self) -> bool {
        match self {
            Self::Map(..) => true,
            Self::Application { arguments, .. } => arguments.len() != 1,
            Self::String
            | Self::Integer
            | Self::Boolean
            | Self::Path
            | Self::Bytes
            | Self::FixedBytes(_)
            | Self::Plain(_)
            | Self::Vector(_)
            | Self::Optional(_)
            | Self::ScopeOf(_) => false,
        }
    }

    /// Construct a reference from a schema name. Reserved scalar names
    /// become scalar leaves; every other name remains a declared-name
    /// leaf.
    pub fn new(name: impl Into<String>) -> Self {
        Self::from_name(Name::new(name))
    }

    pub fn from_name(name: Name) -> Self {
        match name.as_str() {
            "String" => Self::String,
            "Integer" => Self::Integer,
            "Boolean" => Self::Boolean,
            "Path" => Self::Path,
            "Bytes" => Self::Bytes,
            _ => Self::Plain(name),
        }
    }

    pub fn is_reserved_scalar_name(name: &Name) -> bool {
        matches!(
            name.as_str(),
            "String" | "Integer" | "Boolean" | "Path" | "Bytes"
        )
    }

    pub fn scalar_name(&self) -> Option<&'static str> {
        match self {
            Self::String => Some("String"),
            Self::Integer => Some("Integer"),
            Self::Boolean => Some("Boolean"),
            Self::Path => Some("Path"),
            Self::Bytes => Some("Bytes"),
            Self::FixedBytes(_)
            | Self::Plain(_)
            | Self::Vector(_)
            | Self::Map(..)
            | Self::Optional(_)
            | Self::ScopeOf(_)
            | Self::Application { .. } => None,
        }
    }

    pub(crate) fn derived_field_name(&self) -> Name {
        match self {
            Self::String => Name::new("string"),
            Self::Integer => Name::new("integer"),
            Self::Boolean => Name::new("boolean"),
            Self::Path => Name::new("path"),
            Self::Bytes | Self::FixedBytes(_) => Name::new("bytes"),
            Self::Plain(name) => Name::new(name.field_name()),
            Self::Vector(reference) => {
                Name::new(format!("{}_vector", reference.derived_field_name()))
            }
            Self::Optional(reference) => {
                Name::new(format!("optional_{}", reference.derived_field_name()))
            }
            Self::ScopeOf(reference) => {
                Name::new(format!("{}_scope", reference.derived_field_name()))
            }
            Self::Map(key, value) => Name::new(format!(
                "{}_by_{}",
                value.derived_field_name(),
                key.derived_field_name()
            )),
            Self::Application { head, arguments } => {
                let mut derived = Name::new(head.name().field_name()).as_str().to_owned();
                for argument in arguments {
                    derived.push('_');
                    derived.push_str(argument.derived_field_name().as_str());
                }
                Name::new(derived)
            }
        }
    }

    /// The plain name when this reference is a declared-name leaf.
    /// `None` for scalar, collection, or option references — those do
    /// not refer to a user-declared namespace type.
    pub fn plain_name(&self) -> Option<&Name> {
        match self {
            Self::Plain(name) => Some(name),
            Self::String
            | Self::Integer
            | Self::Boolean
            | Self::Path
            | Self::Bytes
            | Self::FixedBytes(_)
            | Self::Vector(_)
            | Self::Map(..)
            | Self::Optional(_)
            | Self::ScopeOf(_)
            | Self::Application { .. } => None,
        }
    }

    /// Whether this reference is a declared-name leaf.
    pub fn is_plain(&self) -> bool {
        matches!(self, Self::Plain(_))
    }

    fn from_nota_map_payload(children: &[Block]) -> Result<Self, NotaDecodeError> {
        if children.len() != 3 {
            return Err(NotaDecodeError::ExpectedRootCount {
                type_name: "TypeReference::Map",
                expected: 3,
                found: children.len(),
            });
        }
        Ok(Self::Map(
            Box::new(Self::from_nota_block(&children[1])?),
            Box::new(Self::from_nota_block(&children[2])?),
        ))
    }

    /// Decode the grouped payload of the canonical `Application` machine
    /// projection — `(head (arg0 arg1 …))`. The head always decodes as
    /// `Local`; import resolution rewrites it to `Imported` later.
    fn from_nota_application_payload(block: &Block) -> Result<Self, NotaDecodeError> {
        let children = NotaBlock::new(block).expect_children(
            Delimiter::Parenthesis,
            "TypeReference::Application payload",
            2,
        )?;
        let head = Name::from_nota_block(&children[0])?;
        let argument_blocks = match &children[1] {
            Block::Delimited {
                delimiter: Delimiter::Parenthesis,
                root_objects,
                ..
            } => root_objects.as_slice(),
            _ => {
                return Err(NotaDecodeError::ExpectedDelimited {
                    type_name: "TypeReference::Application arguments",
                    delimiter: "(",
                });
            }
        };
        let arguments = argument_blocks
            .iter()
            .map(Self::from_nota_block)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::Application {
            head: ApplicationHead::Local(head),
            arguments,
        })
    }

    /// Lower an already-parsed NOTA block at a reference position into
    /// a `TypeReference`.
    ///
    /// A bare PascalCase symbol (`Topic`, `schema-core:mail:Magnitude`)
    /// lowers to `Plain`. TrueSchema type-reference objects lower at this
    /// position: `(Vector T)` -> `Vector`, `(Map K V)` -> `Map`,
    /// `(Optional T)` -> `Optional`, and `(ScopeOf T)` -> `ScopeOf`.
    /// The inner positions recurse, so
    /// `(Vector (Optional Topic))` and `(Map NodeName (Vector Service))`
    /// nest. nota did the structural parse; this is pure semantic
    /// lowering over its `Block`s, not a hand-rolled text parser.
    pub fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let mut context = MacroContext::default();
        Self::from_block_with_registry(block, &MacroRegistry::with_schema_defaults(), &mut context)
    }

    pub(crate) fn from_block_with_registry(
        block: &Block,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<Self, SchemaError> {
        match block {
            Block::Atom(_) => Ok(Self::from_name(block.schema_name()?)),
            Block::Delimited {
                delimiter: Delimiter::SquareBracket,
                root_objects,
                ..
            } => Err(SchemaError::UnknownTypeReferenceForm {
                head: "SquareBracket".to_owned(),
                argument_count: root_objects.len(),
            }),
            Block::Delimited {
                delimiter: Delimiter::Brace,
                root_objects,
                ..
            } => Err(SchemaError::UnknownTypeReferenceForm {
                head: "Brace".to_owned(),
                argument_count: root_objects.len(),
            }),
            Block::Delimited {
                delimiter: Delimiter::Parenthesis,
                root_objects,
                ..
            } => Self::resolve_parenthesis_reference(block, root_objects, registry, context),
            Block::PipeText(_) => Err(SchemaError::ExpectedSymbol {
                found: block.reemit_fallback(),
            }),
            Block::Delimited {
                delimiter: Delimiter::PipeBrace,
                root_objects,
                ..
            } => Self::from_inline_struct(root_objects, registry, context),
            Block::Delimited {
                delimiter: Delimiter::PipeParenthesis,
                root_objects,
                ..
            } => Self::from_inline_enum(root_objects, registry, context),
        }
    }

    fn from_inline_struct(
        objects: &[Block],
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<Self, SchemaError> {
        let name = Self::inline_declaration_name(objects, "inline struct declaration")?;
        let fields = MacroExpansionFields::new(&objects[1..]).lower(registry, context)?;
        if fields.len() == 1 {
            let reference = fields.into_iter().next().expect("length checked").reference;
            context.remember_inline_declaration(Declaration::private(TypeDeclaration::Newtype(
                NewtypeDeclaration::new(name.clone(), reference),
            )));
        } else {
            let declaration = StructDeclaration::new(name.clone(), fields);
            context.remember_inline_declaration(Declaration::private(TypeDeclaration::Struct(
                declaration,
            )));
        }
        Ok(Self::Plain(name))
    }

    fn from_inline_enum(
        objects: &[Block],
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<Self, SchemaError> {
        let name = Self::inline_declaration_name(objects, "inline enum declaration")?;
        let variants = MacroExpansionVariants::new(&objects[1..]).lower(registry, context)?;
        context.remember_inline_declaration(Declaration::private(TypeDeclaration::Enum(
            EnumDeclaration::new(name.clone(), variants),
        )));
        Ok(Self::Plain(name))
    }

    fn inline_declaration_name(objects: &[Block], form: &'static str) -> Result<Name, SchemaError> {
        let Some(name) = objects.first() else {
            return Err(SchemaError::ExpectedSyntaxReferenceArity {
                form,
                expected: "declaration name plus body",
                found: 0,
            });
        };
        name.schema_name()
    }

    /// Construct the `Vector` built-in: `(Vector T)`.
    ///
    /// One of the uniform per-built-in resolvers the schema-language-cc-generated
    /// parenthesis dispatch calls (see `reference_resolver_generated.rs`).
    /// Every `resolve_*` method shares this signature so the generated call
    /// site is uniform; this body is the construction lifted verbatim from
    /// the former hand-written `(Vector, 2)` match arm.
    fn resolve_vector(
        _block: &Block,
        objects: &[Block],
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<Self, SchemaError> {
        Ok(Self::Vector(Box::new(Self::from_block_with_registry(
            &objects[1],
            registry,
            context,
        )?)))
    }

    /// Construct the `Optional` built-in: `(Optional T)`.
    fn resolve_optional(
        _block: &Block,
        objects: &[Block],
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<Self, SchemaError> {
        Ok(Self::Optional(Box::new(Self::from_block_with_registry(
            &objects[1],
            registry,
            context,
        )?)))
    }

    /// Construct the `ScopeOf` built-in: `(ScopeOf T)`.
    fn resolve_scope_of(
        _block: &Block,
        objects: &[Block],
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<Self, SchemaError> {
        Ok(Self::ScopeOf(Box::new(Self::from_block_with_registry(
            &objects[1],
            registry,
            context,
        )?)))
    }

    /// Construct the `Map` built-in: `(Map K V)`.
    fn resolve_map(
        _block: &Block,
        objects: &[Block],
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<Self, SchemaError> {
        Ok(Self::Map(
            Box::new(Self::from_block_with_registry(
                &objects[1],
                registry,
                context,
            )?),
            Box::new(Self::from_block_with_registry(
                &objects[2],
                registry,
                context,
            )?),
        ))
    }

    /// Construct the `Bytes` built-in: `(Bytes N)` — the fixed-width byte
    /// leaf decoded through the HeadedAtom seam.
    fn resolve_bytes(
        block: &Block,
        _objects: &[Block],
        _registry: &MacroRegistry,
        _context: &mut MacroContext,
    ) -> Result<Self, SchemaError> {
        Self::from_fixed_bytes_block(block)
    }

    /// The seam between a DECLARED head (a registered user macro) and the
    /// broad generic-application form. A registered TypeReference macro is a
    /// declared head and wins over the application fallback, so the registry
    /// is consulted first; only when no macro matches does the broad
    /// `(Foo A B …)` form decode through the structural-macro seam. This
    /// ordering is the design's disambiguation and is NOT compiler-checked
    /// (the application form structurally overlaps every PascalCase head), so
    /// it is pinned by tests.
    fn from_macro_or_application(
        block: &Block,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<Self, SchemaError> {
        match Self::from_macro_invocation(block, registry, context) {
            Ok(reference) => Ok(reference),
            Err(SchemaError::MacroDidNotMatch { .. })
            | Err(SchemaError::UnknownTypeReferenceForm { .. }) => Self::from_application(block),
            Err(error) => Err(error),
        }
    }

    /// Decode the broad generic-application form `(Foo A B …)` through the
    /// `#[shape(pascal_head, body)]` structural-macro seam ([`ApplicationNode`]).
    /// The head is always `Local` at decode time; import resolution rewrites
    /// it to `Imported` later.
    fn from_application(block: &Block) -> Result<Self, SchemaError> {
        match ApplicationNode::from_structural_block(block)? {
            ApplicationNode::Application(head, arguments) => Ok(Self::Application {
                head: ApplicationHead::Local(head),
                arguments,
            }),
        }
    }

    /// Lower the fixed-width byte leaf `(Bytes N)` through the HeadedAtom seam.
    fn from_fixed_bytes_block(block: &Block) -> Result<Self, SchemaError> {
        match FixedBytesNode::from_structural_block(block)? {
            FixedBytesNode::FixedBytes(width) => Ok(Self::FixedBytes(width)),
        }
    }

    fn from_macro_invocation(
        block: &Block,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<Self, SchemaError> {
        let invocation = TypeReferenceMacroInvocation::from_block(block)?;
        if !registry
            .node_definition(MacroPosition::TypeReference)
            .is_some_and(|definition| definition.accepts_tagged_invocation())
        {
            return Err(SchemaError::MacroDidNotMatch {
                macro_name: invocation.name().to_owned(),
            });
        }
        match registry.lower(
            MacroObject::Block(block),
            MacroPosition::TypeReference,
            context,
        ) {
            Ok(MacroOutput::Reference(reference)) => Ok(reference),
            Ok(_) => Err(SchemaError::UnexpectedMacroOutput {
                macro_name: invocation.name().to_owned(),
                expected: "type reference",
            }),
            Err(SchemaError::MacroDidNotMatch { .. }) => {
                Err(SchemaError::UnknownTypeReferenceForm {
                    head: invocation.name().to_owned(),
                    argument_count: invocation.argument_count(),
                })
            }
            Err(error) => Err(error),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TypeReferenceMacroInvocation<'schema> {
    name: Name,
    data: MacroInvocationData<'schema>,
}

impl<'schema> TypeReferenceMacroInvocation<'schema> {
    fn from_block(block: &'schema Block) -> Result<Self, SchemaError> {
        if !block.is_parenthesis() {
            return Err(SchemaError::ExpectedDelimiter {
                expected: "(Macro [input])",
            });
        }
        if block.holds_root_objects() != 2 {
            let head = block
                .root_object_at(0)
                .and_then(Block::demote_to_string)
                .unwrap_or("<missing>");
            return Err(SchemaError::UnknownTypeReferenceForm {
                head: head.to_owned(),
                argument_count: block.holds_root_objects().saturating_sub(1),
            });
        }
        let name = block
            .root_object_at(0)
            .ok_or(SchemaError::EmptyTypeReference)?
            .schema_name()?;
        let data = MacroInvocationData::from_block(block.root_object_at(1).expect("count checked"));
        Ok(Self { name, data })
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn argument_count(&self) -> usize {
        self.data.argument_count()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum MacroInvocationData<'schema> {
    Delimited(&'schema [Block]),
    Single(&'schema Block),
}

impl<'schema> MacroInvocationData<'schema> {
    fn from_block(block: &'schema Block) -> Self {
        match block {
            Block::Delimited { root_objects, .. } => Self::Delimited(root_objects),
            Block::PipeText(_) | Block::Atom(_) => Self::Single(block),
        }
    }

    fn argument_count(&self) -> usize {
        match self {
            Self::Delimited(objects) => objects.len(),
            Self::Single(_) => 1,
        }
    }
}

/// Data representation of a schema-node object before macro execution.
///
/// A parenthesized schema node is a tagged/data-carrying variant:
/// `(Normalize [Topic])` has tag `Normalize` and raw vector data
/// `[Topic]`. That vector is macro payload data, not the schema `Vec`
/// type constructor. This type exists so macro calls can be inspected,
/// serialized through assembled schema, and tested as data rather than
/// disappearing into parser control flow.
#[derive(nota::NotaDecode, nota::NotaEncode, Clone, Debug, Eq, PartialEq)]
pub struct SchemaNode {
    tag: Name,
    data: SchemaNodeData,
}

impl SchemaNode {
    pub fn new(tag: Name, data: SchemaNodeData) -> Self {
        Self { tag, data }
    }

    pub fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let children = match block {
            Block::Delimited {
                delimiter: Delimiter::Parenthesis,
                root_objects,
                ..
            } => root_objects,
            _ => {
                return Err(SchemaError::MalformedSchemaNode {
                    found: SchemaNodeNotation::new(block).compact(),
                });
            }
        };
        let tag = children
            .first()
            .ok_or_else(|| SchemaError::MalformedSchemaNode {
                found: SchemaNodeNotation::new(block).compact(),
            })?
            .schema_name()?;
        let data = match children.len() {
            1 => SchemaNodeData::Unit,
            2 => SchemaNodeData::from_block(&children[1])?,
            _ => {
                return Err(SchemaError::MalformedSchemaNode {
                    found: SchemaNodeNotation::new(block).compact(),
                });
            }
        };
        Ok(Self { tag, data })
    }

    pub fn tag(&self) -> &Name {
        &self.tag
    }

    pub fn data(&self) -> &SchemaNodeData {
        &self.data
    }
}

#[derive(nota::NotaDecode, nota::NotaEncode, Clone, Debug, Eq, PartialEq)]
pub enum SchemaNodeData {
    Unit,
    Value(SchemaNodeValue),
    Vector(Vec<SchemaNodeValue>),
    Map(Vec<SchemaNodePair>),
}

impl SchemaNodeData {
    pub fn from_block(block: &Block) -> Result<Self, SchemaError> {
        match block {
            Block::Delimited {
                delimiter: Delimiter::SquareBracket,
                root_objects,
                ..
            } => Ok(Self::Vector(SchemaNodeValues::new(root_objects).read()?)),
            Block::Delimited {
                delimiter: Delimiter::Brace,
                root_objects,
                ..
            } => Ok(Self::Map(SchemaNodeMapEntries::new(root_objects).read()?)),
            _ => Ok(Self::Value(SchemaNodeValue::from_block(block)?)),
        }
    }
}

#[derive(nota::NotaDecode, nota::NotaEncode, Clone, Debug, Eq, PartialEq)]
pub enum SchemaNodeValue {
    Symbol(Name),
    Text(String),
    Node(Box<SchemaNode>),
    Vector(Vec<SchemaNodeValue>),
    Map(Vec<SchemaNodePair>),
}

impl SchemaNodeValue {
    pub fn from_block(block: &Block) -> Result<Self, SchemaError> {
        match block {
            Block::Atom(_) => block.schema_name().map(Self::Symbol),
            Block::PipeText(text) => Ok(Self::Text(text.text.clone())),
            Block::Delimited {
                delimiter: Delimiter::Parenthesis,
                ..
            } => Ok(Self::Node(Box::new(SchemaNode::from_block(block)?))),
            Block::Delimited {
                delimiter: Delimiter::SquareBracket,
                root_objects,
                ..
            } => Ok(Self::Vector(SchemaNodeValues::new(root_objects).read()?)),
            Block::Delimited {
                delimiter: Delimiter::Brace,
                root_objects,
                ..
            } => Ok(Self::Map(SchemaNodeMapEntries::new(root_objects).read()?)),
            Block::Delimited {
                delimiter: Delimiter::PipeParenthesis,
                ..
            }
            | Block::Delimited {
                delimiter: Delimiter::PipeBrace,
                ..
            } => Err(SchemaError::MalformedSchemaNode {
                found: SchemaNodeNotation::new(block).compact(),
            }),
        }
    }
}

#[derive(nota::NotaDecode, nota::NotaEncode, Clone, Debug, Eq, PartialEq)]
pub struct SchemaNodePair {
    key: Name,
    value: SchemaNodeValue,
}

impl SchemaNodePair {
    pub fn new(key: Name, value: SchemaNodeValue) -> Self {
        Self { key, value }
    }

    pub fn key(&self) -> &Name {
        &self.key
    }

    pub fn value(&self) -> &SchemaNodeValue {
        &self.value
    }
}

#[derive(Clone, Copy, Debug)]
struct SchemaNodeValues<'schema> {
    objects: &'schema [Block],
}

impl<'schema> SchemaNodeValues<'schema> {
    fn new(objects: &'schema [Block]) -> Self {
        Self { objects }
    }

    fn read(&self) -> Result<Vec<SchemaNodeValue>, SchemaError> {
        let mut values = Vec::new();
        for object in self.objects {
            values.push(SchemaNodeValue::from_block(object)?);
        }
        Ok(values)
    }
}

#[derive(Clone, Copy, Debug)]
struct SchemaNodeMapEntries<'schema> {
    objects: &'schema [Block],
}

impl<'schema> SchemaNodeMapEntries<'schema> {
    fn new(objects: &'schema [Block]) -> Self {
        Self { objects }
    }

    fn read(&self) -> Result<Vec<SchemaNodePair>, SchemaError> {
        if self.objects.len() % 2 != 0 {
            return Err(SchemaError::ExpectedEvenMapEntries {
                found: self.objects.len(),
            });
        }
        let mut pairs = Vec::new();
        for chunk in self.objects.chunks_exact(2) {
            pairs.push(SchemaNodePair::new(
                chunk[0].schema_name()?,
                SchemaNodeValue::from_block(&chunk[1])?,
            ));
        }
        Ok(pairs)
    }
}

#[derive(Clone, Copy, Debug)]
struct SchemaNodeNotation<'schema> {
    block: &'schema Block,
}

impl<'schema> SchemaNodeNotation<'schema> {
    fn new(block: &'schema Block) -> Self {
        Self { block }
    }

    fn compact(&self) -> String {
        match self.block {
            Block::Delimited {
                delimiter,
                root_objects,
                ..
            } => {
                let children = root_objects
                    .iter()
                    .map(|child| Self::new(child).compact())
                    .collect::<Vec<_>>();
                SchemaNodeDelimitedNotation::new(*delimiter).wrap(&children)
            }
            Block::PipeText(text) => format!("[|{}|]", text.text),
            Block::Atom(atom) => atom.text().to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SchemaNodeDelimitedNotation {
    delimiter: Delimiter,
}

impl SchemaNodeDelimitedNotation {
    fn new(delimiter: Delimiter) -> Self {
        Self { delimiter }
    }

    fn wrap(&self, children: &[String]) -> String {
        if children.is_empty() {
            return format!("{}{}", self.opening(), self.closing());
        }
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

// The parenthesis-reference dispatch (`TypeReference::resolve_parenthesis_reference`)
// is GENERATED by schema-language-cc from `schemas/reference-grammar.nota` and written to
// the committed, freshness-gated `reference_resolver_generated.rs` by `build.rs`.
// It dispatches each built-in head to the uniform `resolve_<snake>` construction
// methods above, then the reserved-head guard, then `from_macro_or_application`.
include!("reference_resolver_generated.rs");
