use std::fmt;

use nota::{
    Block, Delimiter, DottedExpectation, NotaBlock, NotaBody, NotaDecode, NotaDecodeError,
    NotaEncode, NotaString, StructuralMacroNode,
};

use crate::{
    SchemaError,
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

    /// The lowerCamel projection of this name's local part: the leading
    /// character is lowercased and the remainder is left untouched, so a
    /// PascalCase type name (`StoredRecord`) projects into the lowercase "name"
    /// register (`storedRecord`) while a single-word head (`Map`) becomes `map`.
    /// This is the derivation an indirection linkname is minted from — a hoisted
    /// type's name projected into the lowercase indirection-name register.
    pub fn lower_camel(&self) -> String {
        let mut characters = self.local_part().chars();
        match characters.next() {
            Some(first) => {
                let mut projection = first.to_ascii_lowercase().to_string();
                projection.push_str(characters.as_str());
                projection
            }
            None => String::new(),
        }
    }

    pub fn qualifies_as_symbol_name(&self) -> bool {
        // The structural symbol predicate retained from the NOTA reader
        // (`Atom::qualifies_as_symbol`): a non-empty atom whose every character
        // is bare-safe — no whitespace and none of the delimiter or quote
        // characters. No numeric meaning is inferred; a numeric-looking atom
        // qualifies here and narrows to a number only at decode under its
        // expected type.
        let text = self.as_str();
        !text.is_empty()
            && text.chars().all(|character| {
                !character.is_whitespace()
                    && !matches!(character, '"' | '(' | ')' | '[' | ']' | '{' | '}')
            })
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
    /// an unboxed `RootApplication` would make `Root` (and every `SchemaTree`
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
pub struct SchemaTree {
    identity: super::SchemaIdentity,
    imports: Vec<ImportDeclaration>,
    resolved_imports: Vec<super::ResolvedImport>,
    external_roots: Vec<super::ResolvedExternalRoot>,
    input: Root,
    output: Root,
    namespace: Vec<Declaration>,
    impl_blocks: Vec<ImplBlock>,
}

/// A declaration head that introduces a flat list of type-parameter binders.
/// Both a native [`Declaration`] and an imported [`super::ResolvedImport`] carry
/// their binders this way, and both mint one member identifier per binder when
/// decomposed (`CoreResolvedImport::from_resolved_import` uses the same
/// `declare_member` path a native frame does), so a repeated binder mints
/// the same colliding identifier regardless of which shape carries it. Naming
/// the shared head lets the semantic boundary reject a duplicate through one walk
/// over every binder-bearing shape instead of a per-shape special case.
trait ParameterizedHead {
    /// The name the duplicate-binder error reports as the offending declaration.
    fn head_name(&self) -> &Name;

    /// The flat binder list the head introduces.
    fn binders(&self) -> &[Name];

    /// Reject the first repeated binder with the typed error the source reader
    /// constructs for the same fault; accept a list whose binders are distinct.
    fn verify_distinct_binders(&self) -> Result<(), SchemaError> {
        let mut seen: Vec<&Name> = Vec::new();
        for binder in self.binders() {
            if seen.contains(&binder) {
                return Err(SchemaError::DuplicateTypeParameter {
                    declaration: self.head_name().as_str().to_owned(),
                    parameter: binder.as_str().to_owned(),
                });
            }
            seen.push(binder);
        }
        Ok(())
    }
}

impl ParameterizedHead for Declaration {
    fn head_name(&self) -> &Name {
        self.name()
    }

    fn binders(&self) -> &[Name] {
        self.parameters()
    }
}

impl ParameterizedHead for super::ResolvedImport {
    fn head_name(&self) -> &Name {
        self.local_name()
    }

    fn binders(&self) -> &[Name] {
        self.parameters()
    }
}

impl SchemaTree {
    // The schema-language's fields are each a distinct typed section of the model;
    // the constructor takes them as separate typed vectors rather than a
    // bag struct. (Newer clippy raises `too_many_arguments`; the repo's
    // pinned 1.85 toolchain does not.)
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        identity: super::SchemaIdentity,
        imports: Vec<ImportDeclaration>,
        resolved_imports: Vec<super::ResolvedImport>,
        external_roots: Vec<super::ResolvedExternalRoot>,
        input: Root,
        output: Root,
        namespace: Vec<Declaration>,
    ) -> Self {
        Self {
            identity,
            imports,
            resolved_imports,
            external_roots,
            input,
            output,
            namespace,
            impl_blocks: Vec::new(),
        }
    }

    /// Attach the standalone impl blocks lowered from the impls block's
    /// `TypeName.[ … ]` entries — every impl catalog is keyed by the type it
    /// targets, declared by its own types/generics entry.
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

    /// Resolved terminal contract roots. They are dependency identities, not
    /// local declarations, and therefore never participate in name harvesting.
    pub fn external_roots(&self) -> &[super::ResolvedExternalRoot] {
        &self.external_roots
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

    /// The standalone impl blocks lowered from the impls block's
    /// `TypeName.[ … ]` entries.
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

    pub(crate) fn product_components_verified(self) -> Result<Self, SchemaError> {
        for declaration in &self.namespace {
            if let TypeDeclaration::Struct(declaration) = declaration.value() {
                declaration.fields.product_components_verified()?;
            }
        }
        Ok(self)
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
        self.namespace_declaration_named(name)
            .map(|declaration| declaration.parameters().len())
    }

    /// The frame body of a declared parameterized enum: its binders and its
    /// variant list, paired for monomorphization at an application site. `None`
    /// when the name is not a namespace declaration or its declaration is not
    /// an enum. The import resolver reads this across the crate boundary so a
    /// consumer applying the imported head can expand the frame in place,
    /// substituting each binder with the application's argument.
    pub fn declared_frame_body(&self, name: &str) -> Option<(&[Name], &[EnumVariant])> {
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
                .namespace_declaration_named(name.as_str())
                .map(|declaration| declaration.parameters().len()),
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

    /// Verify that every binder-bearing head's binder list is distinct: no
    /// declaration head repeats a type-parameter name. The source reader
    /// (`SourceGenerics::read_parameters`) already rejects a duplicate binder as
    /// it parses the `generics` block, but that guard sits on the text path
    /// alone. This is the same rule enforced at the SEMANTIC boundary — the
    /// construction/decode surface every schema value passes through
    /// ([`crate::TrueSchema::from_tree`], reached by the programmatic tree
    /// constructor, binary decode, and NOTA decode) — so a schema value that
    /// never touched the source reader still cannot carry a duplicate generic or
    /// frame binder. Two shapes carry binders: a native `Declaration` (plain
    /// generic or parameterized enum frame) in `self.namespace`, and an imported
    /// frame head in `self.resolved_imports`. Both decompose their binders through
    /// the same member-minting path, so both are walked here through the shared
    /// [`ParameterizedHead`] check, and both construct the same
    /// [`SchemaError::DuplicateTypeParameter`] the source form does.
    pub(crate) fn parameters_verified(&self) -> Result<(), SchemaError> {
        for declaration in &self.namespace {
            declaration.verify_distinct_binders()?;
        }
        for import in &self.resolved_imports {
            import.verify_distinct_binders()?;
        }
        Ok(())
    }

    /// Every position in the loaded whole that introduces a top-level
    /// declaration, paired with the site label the duplicate error reports it
    /// by. A loaded schema is ONE namespace: the input and output roots, every
    /// namespace declaration, and every resolved import each name one
    /// declaration in that single space. Brace imports are the source form of
    /// the resolved imports and enter the whole through them, so counting both
    /// would double-count one imported declaration; the resolved import is the
    /// declaration in the whole and is what is walked here.
    fn declaration_heads(&self) -> Vec<(&Name, &'static str)> {
        let mut heads: Vec<(&Name, &'static str)> = Vec::new();
        heads.push((self.input.name(), "the input root"));
        heads.push((self.output.name(), "the output root"));
        for declaration in &self.namespace {
            heads.push((declaration.name(), "a namespace declaration"));
        }
        for import in &self.resolved_imports {
            heads.push((import.local_name(), "a resolved import"));
        }
        heads
    }

    /// Verify the loaded whole is one namespace: no two top-level declarations
    /// share a name. The source reader (`push_declaration`) already rejects a
    /// namespace block that repeats a name as it lowers text, but that guard
    /// sits on the text path alone and sees only the namespace block, not the
    /// roots or the resolved imports. This is the same one-namespace rule
    /// enforced at the SEMANTIC boundary — the construction/decode surface every
    /// schema value passes through ([`crate::TrueSchema::from_tree`], reached by
    /// the programmatic tree constructor, binary decode, and NOTA decode) — so a
    /// schema value that never touched the source reader still cannot carry two
    /// declarations of one name. Decomposition mints every top-level declaration
    /// through the same `NameHarvest::declare` path keyed on (kind, name), so two
    /// heads of one name — an imported `Input` and a local `Input` root, or any
    /// other pair across the whole — would otherwise mint one colliding
    /// identifier and silently merge two declarations into one. The author
    /// resolves a real collision by renaming the more appropriate side: the local
    /// declaration, or the imported one at its source.
    pub(crate) fn declaration_names_unique(&self) -> Result<(), SchemaError> {
        let mut seen: Vec<(&Name, &'static str)> = Vec::new();
        for (name, site) in self.declaration_heads() {
            if let Some((_, first_site)) = seen.iter().find(|(prior, _)| *prior == name) {
                return Err(SchemaError::DuplicateDeclaration {
                    name: name.as_str().to_owned(),
                    first_site,
                    second_site: site,
                });
            }
            seen.push((name, site));
        }
        Ok(())
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
                if matches!(
                    payload,
                    TypeReference::SingleTypeApplication {
                        projection: SingleTypeReferenceProjection::Optional,
                        ..
                    }
                ) {
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
            | TypeReference::ValueApplication { .. }
            | TypeReference::Plain(_)
            | TypeReference::ExternalRoot(_) => Ok(()),
            TypeReference::SingleTypeApplication { argument, .. } => {
                self.verify_reference_arities(argument)
            }
            TypeReference::MultiTypeApplication { arguments, .. } => {
                for argument in arguments {
                    self.verify_reference_arities(argument)?;
                }
                Ok(())
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
        let roots = [
            self.imports_schema_text(),
            self.input.to_root_schema_text(),
            self.output.to_root_schema_text(),
            self.types_schema_text(),
            self.generics_schema_text(),
            self.impls_schema_text(),
        ];
        roots.join("\n")
    }

    fn imports_schema_text(&self) -> String {
        let mut entries = self
            .imports
            .iter()
            .map(|import| import.to_schema_text())
            .collect::<Vec<_>>();
        let mut packages: Vec<(&Name, Vec<&Name>)> = Vec::new();
        for root in &self.external_roots {
            if let Some((_, roots)) = packages
                .iter_mut()
                .find(|(package, _)| *package == root.package().name())
            {
                roots.push(root.reference().root());
            } else {
                packages.push((root.package().name(), vec![root.reference().root()]));
            }
        }
        entries.extend(packages.into_iter().map(|(package, roots)| {
            let targets = roots
                .iter()
                .map(|root| root.to_nota())
                .collect::<Vec<_>>()
                .join(" ");
            format!("{}.[{}]", package.to_nota(), targets)
        }));
        Self::brace_block_text(entries)
    }

    /// The `types` block: every non-parameterized declaration, projected as a
    /// dotted `TypeName.Definition` entry.
    fn types_schema_text(&self) -> String {
        Self::brace_block_text(
            self.namespace
                .iter()
                .filter(|declaration| declaration.parameters().is_empty())
                .map(Declaration::types_entry_text)
                .collect(),
        )
    }

    /// The `generics` block: every parameterized declaration, projected as a
    /// dotted `GenericName.((Params …) Body)` entry.
    fn generics_schema_text(&self) -> String {
        Self::brace_block_text(
            self.namespace
                .iter()
                .filter(|declaration| !declaration.parameters().is_empty())
                .map(Declaration::generics_entry_text)
                .collect(),
        )
    }

    /// The `impls` block: every impl catalog keyed by the type it targets. A
    /// declaration carrying a fused catalog contributes its own entry, and each
    /// standalone impl block contributes its target's entry — the same union
    /// the enumerable manifest walks.
    fn impls_schema_text(&self) -> String {
        let mut entries = Vec::new();
        for declaration in &self.namespace {
            if !declaration.impls().is_empty() {
                entries.push(ImplBlock::impls_entry_text(
                    declaration.name(),
                    declaration.impls(),
                ));
            }
        }
        entries.extend(self.impl_blocks.iter().map(ImplBlock::to_impls_entry_text));
        Self::brace_block_text(entries)
    }

    fn brace_block_text(entries: Vec<String>) -> String {
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
    /// Project a non-parameterized declaration as a `types` block entry:
    /// `TypeName.Definition`.
    fn types_entry_text(&self) -> String {
        format!("{}.{}", self.name.to_nota(), self.value.to_schema_text())
    }

    /// Project a parameterized declaration as a `generics` block entry:
    /// `GenericName.((Params …) Body)`.
    fn generics_entry_text(&self) -> String {
        let binders = self
            .parameters
            .iter()
            .map(Name::to_nota)
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            "{}.(({}) {})",
            self.name.to_nota(),
            binders,
            self.value.to_schema_text()
        )
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
        match &self.payload {
            None => self.name.to_nota(),
            Some(payload) => {
                Delimiter::Parenthesis.wrap([self.name.to_nota(), payload.to_structural_nota()])
            }
        }
    }
}

impl ImplBlock {
    /// Project a standalone impl block as an `impls` block entry keyed by its
    /// target type: `TypeName.[ … ]`.
    fn to_impls_entry_text(&self) -> String {
        Self::impls_entry_text(&self.target, &self.catalog)
    }

    /// Project one `impls` block entry from a target type name and its catalog.
    /// Shared by standalone impl blocks and the fused catalog a declaration
    /// carries, so both project to the same `TypeName.[ … ]` shape.
    fn impls_entry_text(target: &Name, catalog: &ImplCatalog) -> String {
        format!("{}.{}", target.to_nota(), catalog.to_schema_text())
    }
}

impl ImplCatalog {
    fn to_schema_text(&self) -> String {
        Delimiter::SquareBracket.wrap(self.entries.iter().map(ImplReference::to_schema_text))
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

impl ImportDeclaration {
    /// The dotted no-alias import entry text: the single-colon source path
    /// re-projected with dots, `crate.module.Type`. The imported name is the
    /// target's own name, so no alias is written (see ARCHITECTURE "Imports
    /// entry syntax carries no alias").
    fn to_schema_text(&self) -> String {
        self.source
            .plain_name()
            .map(|name| name.as_str().replace(':', "."))
            .unwrap_or_else(|| self.source.to_structural_nota())
    }
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

    /// Attach a lowered impl catalog to this declaration. The
    /// catalog is a *reference* to impls/methods that already exist on the
    /// Rust side — markers and callable method signatures — not a generated
    /// body. A declaration with no trailing impl block carries
    /// `ImplCatalog::empty()`.
    pub fn with_impls(mut self, impls: ImplCatalog) -> Self {
        self.impls = impls;
        self
    }

    /// The lowered impl catalog attached to this declaration. Empty for a
    /// declaration with no attached catalog. This is
    /// the per-type reach of the enumerable manifest; the schema-wide walk
    /// (`SchemaTree::referenced_impls`) unions these with the standalone
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

/// The lowered impl catalog: an enumerable list of impl
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
/// lowered form of an impls-block `TypeName.[ … ]` entry. The named
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
/// it targets. Borrowed view produced by `SchemaTree::referenced_impls`; the
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
/// `Self::verify_catalog` is that boundary check.
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
    pub fn verify_catalog(&self, schema: &crate::TrueSchema) -> Result<(), SchemaError> {
        for reference in schema.referenced_impls() {
            let target = reference.target();
            if let Some(trait_name) = reference.entry().trait_name() {
                self.verify_trait(target, trait_name)?;
            }
            for signature in reference.entry().methods() {
                self.verify_method(target, signature)?;
            }
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
        let mut entries = Vec::new();
        let mut index = 0;
        while index < root_objects.len() {
            let entry = DottedExpectation::Uncapitalized.read_entry(&root_objects[index..])?;
            index += entry.consumed();
            entries.push(FieldDeclaration {
                name: Name::from_nota_block(entry.key())?,
                reference: TypeReference::from_nota_block(entry.value())?,
            });
        }
        Ok(Self::new(entries))
    }
}

impl NotaEncode for StructFieldMap {
    fn to_nota(&self) -> String {
        let fields = self
            .entries()
            .iter()
            .map(|entry| format!("{}.{}", entry.name.to_nota(), entry.reference.to_nota()))
            .collect::<Vec<_>>();
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
}

impl EnumVariant {
    pub fn new(name: Name, payload: Option<TypeReference>) -> Self {
        Self { name, payload }
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

/// A declaration's type-name position: a bare `Name` declaration head. The
/// pipe-parenthesized `(| Name Param Param … |)` binder head is retired along
/// with the structural pipe delimiters, so a declaration head introduces no
/// inline type-parameter binders. Generic type-parameter binders now live in
/// the dedicated generics block (`GenericName.((Params …) Body)`), read by the
/// source-side generics reader, while authored use-site generic application
/// stays dotted (`Head.(Arg …)`); binding and application remain structurally
/// distinct without a pipe fence. The `parameters` vector is accordingly always
/// decoded empty here.
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

    /// Decode the declaration-name position from its block: a bare symbol
    /// atom with no parameters. The retired pipe-parenthesized
    /// `(| Name Param … |)` head is not read here — generic binders live in
    /// the dedicated generics block (`GenericName.((Params …) Body)`), read
    /// by the source-side generics reader.
    pub fn from_block(block: &Block) -> Result<Self, SchemaError> {
        Ok(Self {
            name: block.schema_name()?,
            parameters: Vec::new(),
        })
    }
}

/// The within-kind lowering strategy of a single-type generic application.
///
/// The single-type kind carries exactly one type argument; the projection is
/// the closed set of lowering strategies distinguished by meta-shape, never by
/// the head name. `Vector.T`, `List.T`, and any other single-type generic alias
/// all lower through one of these strategies; the head name is a `NameTable`
/// concern, not a dispatch key.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Copy, Debug, Eq, PartialEq)]
pub enum SingleTypeReferenceProjection {
    Vector,
    Optional,
    ScopeOf,
}

impl SingleTypeReferenceProjection {
    /// The canonical spelling of this projection in the machine NOTA codec.
    /// This is an enum-to-spelling projection (the same shape as
    /// [`TypeReference::scalar_name`]), not a name-dispatch: decoding maps the
    /// spelling back to the projection through the private `from_canonical_name`
    /// lookup over this closed set.
    pub fn canonical_name(self) -> &'static str {
        match self {
            Self::Vector => "Vector",
            Self::Optional => "Optional",
            Self::ScopeOf => "ScopeOf",
        }
    }

    fn from_canonical_name(name: &str) -> Option<Self> {
        [Self::Vector, Self::Optional, Self::ScopeOf]
            .into_iter()
            .find(|projection| projection.canonical_name() == name)
    }
}

/// The within-kind lowering strategy of a multi-type generic application.
///
/// The multi-type kind carries an arity-as-data argument list; the projection
/// names the closed lowering strategy. `Map` is the sole builtin strategy, a
/// keyed pair; user-defined multi-type aliases reuse it by definition data.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Copy, Debug, Eq, PartialEq)]
pub enum MultiTypeReferenceProjection {
    Map,
}

impl MultiTypeReferenceProjection {
    pub fn canonical_name(self) -> &'static str {
        match self {
            Self::Map => "Map",
        }
    }

    fn from_canonical_name(name: &str) -> Option<Self> {
        [Self::Map]
            .into_iter()
            .find(|projection| projection.canonical_name() == name)
    }
}

/// The within-kind lowering strategy of a value/const generic application.
///
/// The value kind carries a value argument (a fixed width, not a type). `Bytes`
/// is the sole builtin strategy: `Bytes.N` is the fixed-width bytes value, named
/// after the same `Bytes` head the source grammar spells. The dynamic-length
/// bytes scalar is the separate [`TypeReference::Bytes`] leaf; the kind — value
/// application versus scalar leaf — is what distinguishes them, exactly as the
/// grammar distinguishes `Bytes.N` from a bare `Bytes` by the width leaf.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueReferenceProjection {
    Bytes,
}

impl ValueReferenceProjection {
    pub fn canonical_name(self) -> &'static str {
        match self {
            Self::Bytes => "Bytes",
        }
    }

    fn from_canonical_name(name: &str) -> Option<Self> {
        [Self::Bytes]
            .into_iter()
            .find(|projection| projection.canonical_name() == name)
    }
}

/// A type at a reference position — a struct field's type, an enum
/// variant's payload, or an import source.
///
/// `String`, `Integer`, `Boolean`, `Path`, and `Bytes` are reserved scalar
/// leaves. `Plain` is a declared-name leaf (`Topic`, `Magnitude`). The generic
/// applications mirror the source kind partition rather than one variant per
/// builtin name: `SingleTypeApplication` (`Vector.T`, `Optional.T`, `ScopeOf.T`),
/// `MultiTypeApplication` (`Map.(K V)`), and `ValueApplication` (`Bytes.N`) each
/// carry a closed projection that names the lowering strategy, so lowering
/// dispatches on kind and projection and never on a head string. `Application`
/// is the broad generic-application form `Foo.(A B …)`: any other PascalCase
/// head carrying a tail of type-reference arguments. Built-in heads are
/// dispatched by the source generic definition table before an application form
/// is produced.
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
    Plain(Name),
    /// A terminal contract root addressed by exact dependency package identity,
    /// never by the consumer's local declaration namespace.
    ExternalRoot(super::ResolvedExternalRoot),
    SingleTypeApplication {
        projection: SingleTypeReferenceProjection,
        #[rkyv(omit_bounds)]
        argument: Box<TypeReference>,
    },
    MultiTypeApplication {
        projection: MultiTypeReferenceProjection,
        #[rkyv(omit_bounds)]
        arguments: Vec<TypeReference>,
    },
    ValueApplication {
        projection: ValueReferenceProjection,
        value: u64,
    },
    Application {
        head: ApplicationHead,
        #[rkyv(omit_bounds)]
        arguments: Vec<TypeReference>,
    },
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
            "ExternalRoot" => Self::from_nota_external_root(children),
            "Application" => Self::from_nota_application_payload(&children[1]),
            other => Self::from_nota_generic_payload(other, children),
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
            Self::Plain(name) => format!("(Plain {})", name.to_nota()),
            Self::ExternalRoot(root) => format!(
                "(ExternalRoot ({} {}) {})",
                root.package().name().to_nota(),
                nota::NotaString::new(root.package().version()).format(),
                root.reference().root().to_nota()
            ),
            Self::SingleTypeApplication {
                projection,
                argument,
            } => format!("({} {})", projection.canonical_name(), argument.to_nota()),
            Self::MultiTypeApplication {
                projection,
                arguments,
            } => {
                let arguments = arguments
                    .iter()
                    .map(Self::to_nota)
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("({} {arguments})", projection.canonical_name())
            }
            Self::ValueApplication { projection, value } => {
                format!("({} {value})", projection.canonical_name())
            }
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

/// `TypeReference` is itself a structural-macro node so the application
/// form's variable-arity tail (`Vec<TypeReference>`, via nota's blanket
/// `StructuralMacroNode for Vec<Item>`) can decode each argument back through
/// the full reference grammar. Decode delegates to [`Self::from_block`] (which
/// owns the public dotted reader), and encode is the source-grammar projection
/// — a bare PascalCase atom for a leaf and dotted positional form for every
/// composite. This is the source-facing grammar projection, distinct from the
/// canonical-only `NotaEncode`/`NotaDecode` machine codec above.
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
        Self::from_block(block).map_err(nota::StructuralMacroError::MatchedNode)
    }

    fn from_structural_candidate(
        candidate: nota::MacroCandidate<'_>,
    ) -> Result<Self, nota::StructuralMacroError<Self::Error>> {
        let blocks = candidate.blocks();
        let source = blocks
            .iter()
            .map(|block| block.reemit_fallback())
            .collect::<Vec<_>>()
            .join(" ");
        let document = nota::Document::parse(&source)
            .map_err(|error| nota::StructuralMacroError::MatchedNode(SchemaError::from(error)))?;
        let mut cursor = 0;
        let reference =
            crate::SourceReference::from_blocks_at(document.root_objects(), &mut cursor)
                .map_err(nota::StructuralMacroError::MatchedNode)?;
        if cursor == document.root_objects().len() {
            Ok(reference.to_type_reference())
        } else {
            Err(nota::StructuralMacroError::ExpectedSingleRoot {
                found: blocks.len(),
            })
        }
    }

    fn to_structural_nota(&self) -> String {
        crate::SourceReference::from_type_reference(self).to_schema_text()
    }
}

impl TypeReference {
    /// The canonical roster of primitive scalar kinds. This is the single
    /// source of the reserved-scalar vocabulary: `from_name`,
    /// `is_reserved_scalar_name`, and every source-side derivation read each
    /// kind's spelling through [`scalar_name`](Self::scalar_name) instead of
    /// repeating the `String | Integer | …` list. A new scalar is therefore
    /// declared in exactly one place — a variant plus its `scalar_name` arm —
    /// and the roster's own guard test fails if the two ever disagree.
    const SCALAR_KINDS: [Self; 5] = [
        Self::String,
        Self::Integer,
        Self::Boolean,
        Self::Path,
        Self::Bytes,
    ];

    /// Construct a reference from a schema name. Reserved scalar names
    /// become scalar leaves; every other name remains a declared-name
    /// leaf.
    pub fn new(name: impl Into<String>) -> Self {
        Self::from_name(Name::new(name))
    }

    pub fn from_name(name: Name) -> Self {
        Self::scalar_kind_for(name.as_str()).unwrap_or(Self::Plain(name))
    }

    pub fn is_reserved_scalar_name(name: &Name) -> bool {
        Self::scalar_kind_for(name.as_str()).is_some()
    }

    /// The scalar kind a name denotes, matched against the one scalar roster
    /// through each kind's own [`scalar_name`](Self::scalar_name). Returns
    /// `None` for every name outside the reserved scalar vocabulary.
    fn scalar_kind_for(name: &str) -> Option<Self> {
        Self::SCALAR_KINDS
            .iter()
            .find(|kind| kind.scalar_name() == Some(name))
            .cloned()
    }

    pub fn scalar_name(&self) -> Option<&'static str> {
        match self {
            Self::String => Some("String"),
            Self::Integer => Some("Integer"),
            Self::Boolean => Some("Boolean"),
            Self::Path => Some("Path"),
            Self::Bytes => Some("Bytes"),
            Self::Plain(_)
            | Self::ExternalRoot(_)
            | Self::SingleTypeApplication { .. }
            | Self::MultiTypeApplication { .. }
            | Self::ValueApplication { .. }
            | Self::Application { .. } => None,
        }
    }

    pub(crate) fn derived_field_name(&self) -> Name {
        crate::SourceReference::from_type_reference(self).derived_field_name()
    }

    /// The plain name when this reference is a declared-name leaf.
    /// `None` for scalar, collection, or option references — those do
    /// not refer to a user-declared namespace type.
    pub fn plain_name(&self) -> Option<&Name> {
        match self {
            Self::Plain(name) => Some(name),
            Self::ExternalRoot(_) => None,
            Self::String
            | Self::Integer
            | Self::Boolean
            | Self::Path
            | Self::Bytes
            | Self::SingleTypeApplication { .. }
            | Self::MultiTypeApplication { .. }
            | Self::ValueApplication { .. }
            | Self::Application { .. } => None,
        }
    }

    /// Whether this reference is a declared-name leaf.
    pub fn is_plain(&self) -> bool {
        matches!(self, Self::Plain(_))
    }

    /// Construct a single-type generic application (`Vector.T`, `Optional.T`,
    /// `ScopeOf.T`) from its projection and single argument.
    pub fn single_type_application(
        projection: SingleTypeReferenceProjection,
        argument: TypeReference,
    ) -> Self {
        Self::SingleTypeApplication {
            projection,
            argument: Box::new(argument),
        }
    }

    /// Construct a multi-type generic application (`Map.(K V)`) from its
    /// projection and argument list.
    pub fn multi_type_application(
        projection: MultiTypeReferenceProjection,
        arguments: Vec<TypeReference>,
    ) -> Self {
        Self::MultiTypeApplication {
            projection,
            arguments,
        }
    }

    /// Construct a value/const generic application (`Bytes.N`) from its
    /// projection and value.
    pub fn value_application(projection: ValueReferenceProjection, value: u64) -> Self {
        Self::ValueApplication { projection, value }
    }

    /// The `Vector.T` single-type application.
    pub fn vector(argument: TypeReference) -> Self {
        Self::single_type_application(SingleTypeReferenceProjection::Vector, argument)
    }

    /// The `Optional.T` single-type application.
    pub fn optional(argument: TypeReference) -> Self {
        Self::single_type_application(SingleTypeReferenceProjection::Optional, argument)
    }

    /// The `ScopeOf.T` single-type application.
    pub fn scope_of(argument: TypeReference) -> Self {
        Self::single_type_application(SingleTypeReferenceProjection::ScopeOf, argument)
    }

    /// The `Map.(K V)` multi-type application.
    pub fn map(key: TypeReference, value: TypeReference) -> Self {
        Self::multi_type_application(MultiTypeReferenceProjection::Map, vec![key, value])
    }

    /// The `Bytes.N` fixed-width bytes value application. The dynamic-length
    /// bytes scalar is the separate [`Self::Bytes`] leaf.
    pub fn fixed_width_bytes(width: u64) -> Self {
        Self::value_application(ValueReferenceProjection::Bytes, width)
    }

    fn from_nota_external_root(children: &[Block]) -> Result<Self, NotaDecodeError> {
        if children.len() != 3 {
            return Err(NotaDecodeError::ExpectedRootCount {
                type_name: "TypeReference::ExternalRoot",
                expected: 3,
                found: children.len(),
            });
        }
        let package_children = NotaBlock::new(&children[1]).expect_children(
            Delimiter::Parenthesis,
            "TypeReference::ExternalRoot package identity",
            2,
        )?;
        let package = Name::from_nota_block(&package_children[0])?;
        let version = NotaBlock::new(&package_children[1]).parse_string()?;
        let root = Name::from_nota_block(&children[2])?;
        let reference =
            super::ExternalRootReference::new(package.clone(), root).map_err(|error| {
                NotaDecodeError::UnknownVariant {
                    enum_name: "TypeReference::ExternalRoot",
                    variant: error.to_string(),
                }
            })?;
        Ok(Self::ExternalRoot(super::ResolvedExternalRoot::new(
            reference,
            super::PackageIdentity::new(package, version),
        )))
    }

    /// Decode a parenthesized generic payload whose head is a projection
    /// canonical name (`Vector`, `Map`, `Bytes`, …). The head is matched against
    /// the closed projection vocabularies, never dispatched as a free string.
    fn from_nota_generic_payload(head: &str, children: &[Block]) -> Result<Self, NotaDecodeError> {
        if let Some(projection) = SingleTypeReferenceProjection::from_canonical_name(head) {
            return Ok(Self::single_type_application(
                projection,
                Self::from_nota_block(&children[1])?,
            ));
        }
        if let Some(projection) = MultiTypeReferenceProjection::from_canonical_name(head) {
            let arguments = children[1..]
                .iter()
                .map(Self::from_nota_block)
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(Self::multi_type_application(projection, arguments));
        }
        if let Some(projection) = ValueReferenceProjection::from_canonical_name(head) {
            let value = children[1]
                .demote_to_string()
                .and_then(|text| text.parse::<u64>().ok())
                .ok_or(NotaDecodeError::ExpectedAtom {
                    type_name: "value application width",
                })?;
            return Ok(Self::value_application(projection, value));
        }
        Err(NotaDecodeError::UnknownVariant {
            enum_name: "TypeReference",
            variant: head.to_owned(),
        })
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
    /// a `TypeReference`. The authored-schema entry accepts the strict dotted
    /// reference projection (`Vector.Topic`, `Map.(Key Value)`) and rejects
    /// the retired parenthesized generic surface.
    pub fn from_block(block: &Block) -> Result<Self, SchemaError> {
        Ok(crate::SourceReference::from_block(block)?.to_type_reference())
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
        let mut pairs = Vec::new();
        let mut index = 0;
        while index < self.objects.len() {
            let entry = DottedExpectation::Uncapitalized.read_entry(&self.objects[index..])?;
            index += entry.consumed();
            pairs.push(SchemaNodePair::new(
                entry.key().schema_name()?,
                SchemaNodeValue::from_block(entry.value())?,
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

/// Drift guard for the primitive scalar vocabulary. The reserved-scalar set
/// lives in exactly one place — [`TypeReference::SCALAR_KINDS`] paired with
/// each kind's [`scalar_name`](TypeReference::scalar_name). Every consuming
/// site (`from_name`, `is_reserved_scalar_name`, and the source-side reference
/// and field derivations) reads that one authority. These tests fail if a
/// scalar is added to the roster without a matching `scalar_name`, if the
/// name/kind bijection stops round-tripping, or if the machine wire tag drifts
/// away from the reserved source name it currently shares.
#[cfg(test)]
mod scalar_vocabulary_guard {
    use super::*;

    #[test]
    fn every_scalar_kind_resolves_through_one_vocabulary() {
        for kind in &TypeReference::SCALAR_KINDS {
            let name = Name::new(
                kind.scalar_name()
                    .expect("a scalar kind exposes its reserved name"),
            );
            assert!(
                TypeReference::is_reserved_scalar_name(&name),
                "scalar name {name:?} must be reserved",
            );
            assert_eq!(
                &TypeReference::from_name(name.clone()),
                kind,
                "scalar name {name:?} must resolve back to its own kind",
            );
        }
    }

    #[test]
    fn declared_name_is_not_a_reserved_scalar() {
        let declared = Name::new("Widget");
        assert!(!TypeReference::is_reserved_scalar_name(&declared));
        assert!(matches!(
            TypeReference::from_name(declared),
            TypeReference::Plain(_),
        ));
    }

    #[test]
    fn machine_wire_tag_matches_reserved_source_name() {
        // The canonical machine codec and the source-grammar vocabulary are
        // distinct concerns that currently share one spelling per scalar.
        // This pins that coincidence so an intentional divergence surfaces as
        // a conscious decision rather than a silent round-trip break.
        for kind in &TypeReference::SCALAR_KINDS {
            assert_eq!(
                Some(kind.to_nota().as_str()),
                kind.scalar_name(),
                "machine wire tag must match the reserved source name",
            );
        }
    }
}

/// Shared witness for the semantic construction/decode boundary. A schema VALUE
/// reaching [`TrueSchema::from_tree`] — tampered after lowering into a shape the
/// source reader would never emit — must be rejected identically at every
/// surface that funnels through `from_tree`: the programmatic tree constructor,
/// binary (rkyv) decode, and structured NOTA decode. Both the duplicate-binder
/// guard and the duplicate-declaration guard are witnessed through this one
/// helper, so the "reject at every surface" scaffold lives here once.
#[cfg(test)]
mod semantic_boundary_rejection {
    use super::*;
    use crate::{NameTable, TrueSchema};

    /// Assert a tampered tree is rejected with `expected` at every surface that
    /// funnels through [`TrueSchema::from_tree`].
    pub(super) fn assert_tree_rejected_across_surfaces(tree: &SchemaTree, expected: &SchemaError) {
        // Programmatic construction surface.
        assert_eq!(
            &TrueSchema::from_tree(tree, &NameTable::empty())
                .expect_err("from_tree must reject the tampered value"),
            expected,
        );

        // Binary (rkyv) decode surface.
        let bytes = tree
            .to_binary_bytes()
            .expect("tampered tree encodes to binary");
        assert_eq!(
            &TrueSchema::from_binary_bytes(&bytes)
                .expect_err("binary decode must reject the tampered value"),
            expected,
        );

        // Structured NOTA decode surface. The view's `NotaDecode` wraps the
        // schema error as `InvalidValue`, so the typed error surfaces through
        // its rendered reason.
        let nota = tree.to_nota();
        let document = nota::Document::parse(&nota).expect("tampered tree NOTA parses");
        match TrueSchema::from_nota_block(&document.root_objects()[0])
            .expect_err("NOTA decode must reject the tampered value")
        {
            NotaDecodeError::InvalidValue { reason, .. } => {
                assert_eq!(reason, expected.to_string());
            }
            other => panic!("expected an InvalidValue wrapping the schema error, got {other:?}"),
        }
    }
}

/// The source reader rejects a duplicate generic or frame binder as it parses
/// the `generics` block (`SourceGenerics::read_parameters`), witnessed by the
/// document-form tests in `tests/generics.rs`. These tests witness the OTHER
/// boundary: a schema VALUE that reaches the semantic construction/decode
/// surface (`TrueSchema::from_tree`) carrying a duplicate binder — one that
/// never passed the source reader, because it was tampered after lowering — is
/// rejected there too, with the same typed [`SchemaError::DuplicateTypeParameter`].
/// The rejection is witnessed across every surface that funnels through
/// `from_tree`: the programmatic tree constructor, binary (rkyv) decode, and
/// structured NOTA decode. Both a plain generic and a parameterized enum frame
/// are covered, since both carry their binders in the one
/// `Declaration::parameters` list.
#[cfg(test)]
mod semantic_duplicate_parameter_rejection {
    use super::semantic_boundary_rejection::assert_tree_rejected_across_surfaces;
    use super::*;
    use crate::{ImportSource, ResolvedImport, SchemaEngine, SchemaIdentity};

    /// Lower a valid single-block `generics` document and hand back the
    /// crate-internal sidecar tree the codec surfaces project through.
    fn lowered_tree(generics_body: &str) -> SchemaTree {
        let source = format!("{{}}\n[]\n[]\n{{}}\n{{ {generics_body} }}\n{{}}");
        SchemaEngine::default()
            .lower_source(
                &source,
                SchemaIdentity::new("semantic-duplicate:lib", "0.1.0"),
            )
            .expect("valid parameterized declaration lowers")
            .tree()
    }

    /// Duplicate the first binder of the first parameterized declaration —
    /// producing a value the source reader would never emit — and return the
    /// typed error the semantic boundary must now construct for it.
    fn tamper_first_binder(tree: &mut SchemaTree) -> SchemaError {
        for declaration in &mut tree.namespace {
            if let Some(first) = declaration.parameters().first().cloned() {
                *declaration = declaration
                    .clone()
                    .with_parameters(vec![first.clone(), first.clone()]);
                return SchemaError::DuplicateTypeParameter {
                    declaration: declaration.name().as_str().to_owned(),
                    parameter: first.as_str().to_owned(),
                };
            }
        }
        panic!("fixture must carry a parameterized declaration");
    }

    fn assert_rejected_across_surfaces(
        generics_body: &str,
        expected_declaration: &str,
        expected_parameter: &str,
    ) {
        let mut tree = lowered_tree(generics_body);
        let expected = tamper_first_binder(&mut tree);
        assert_eq!(
            expected,
            SchemaError::DuplicateTypeParameter {
                declaration: expected_declaration.to_owned(),
                parameter: expected_parameter.to_owned(),
            },
        );
        assert_tree_rejected_across_surfaces(&tree, &expected);
    }

    #[test]
    fn duplicate_generic_parameters_are_rejected_at_the_semantic_boundary() {
        assert_rejected_across_surfaces("Plane.((Wing) { Wing })", "Plane", "Wing");
    }

    #[test]
    fn duplicate_frame_parameters_are_rejected_at_the_semantic_boundary() {
        assert_rejected_across_surfaces(
            "Work.((Event Outcome) [Started.Event Completed.Outcome])",
            "Work",
            "Event",
        );
    }

    /// The other binder-bearing shape: an imported frame head carries its
    /// binders on a `ResolvedImport`, not the namespace. Inject one whose binder
    /// list repeats a name — the tamper the resolver would never emit — into an
    /// otherwise-valid tree, and assert the semantic boundary rejects it with the
    /// same typed error across every decode surface. Without the
    /// `resolved_imports` walk in `parameters_verified`, a crafted binary or NOTA
    /// value carrying this import would mint the same colliding member identifier
    /// the guard exists to prevent.
    #[test]
    fn duplicate_imported_frame_parameters_are_rejected_at_the_semantic_boundary() {
        let mut tree = lowered_tree("Plane.((Wing) { Wing })");
        let binder = Name::new("Frame");
        let source = ImportSource::try_from(&Name::new("dependency-core:mail:Beacon"))
            .expect("well-formed import source parses");
        tree.resolved_imports
            .push(ResolvedImport::from_projected_parts(
                Name::new("Beacon"),
                source,
                Some(2),
                vec![binder.clone(), binder.clone()],
                Vec::new(),
            ));
        let expected = SchemaError::DuplicateTypeParameter {
            declaration: "Beacon".to_owned(),
            parameter: binder.as_str().to_owned(),
        };
        assert_tree_rejected_across_surfaces(&tree, &expected);
    }
}

/// The one-namespace rule at the semantic boundary. A loaded schema is one
/// namespace: no two top-level declarations — across the input and output
/// roots, the namespace, and the resolved imports — may share a name. The
/// source reader's `push_declaration` guard already refuses a namespace block
/// that repeats a name as it lowers text, but it sees only the namespace block
/// and only the text path. These tests witness the OTHER boundary: a schema
/// VALUE reaching `from_tree` carrying a collision the source reader would never
/// emit. Decomposition mints every top-level declaration through the same
/// `(kind, name)` `NameHarvest::declare` path, so an unguarded collision would
/// mint one identifier and silently merge two declarations into one — the fault
/// that produced self-referencing emitted Rust when an imported `Input` met a
/// local `Input` root.
#[cfg(test)]
mod semantic_duplicate_declaration_rejection {
    use super::semantic_boundary_rejection::assert_tree_rejected_across_surfaces;
    use super::*;
    use crate::{
        ImportSource, NameTable, ResolvedImport, SchemaEngine, SchemaIdentity, TrueSchema,
    };

    /// Lower a valid schema whose declarations all carry distinct names, and
    /// hand back the crate-internal sidecar tree the codec surfaces project
    /// through. The roots are `Input`/`Output`; the namespace declares
    /// `Command`, `Report`, and `Topic`; nothing collides.
    fn distinct_named_tree() -> SchemaTree {
        let source = "{}\n[Start.Command]\n[Finish.Report]\n{\n  Command.{ Topic }\n  Report.{ Topic }\n  Topic.String\n}\n{}\n{}";
        SchemaEngine::default()
            .lower_source(
                source,
                SchemaIdentity::new("distinct-declarations:lib", "0.1.0"),
            )
            .expect("a schema of distinct names lowers")
            .tree()
    }

    /// A plain (non-frame) resolved import bearing `local_name` — the tamper the
    /// resolver would never emit into an otherwise-valid tree. It carries no
    /// binders, so it passes the binder guard and reaches the declaration guard.
    fn plain_import(local_name: &str) -> ResolvedImport {
        let source = ImportSource::try_from(&Name::new("plane-crate:signal:Wrapped"))
            .expect("well-formed import source parses");
        ResolvedImport::from_projected_parts(
            Name::new(local_name),
            source,
            Some(0),
            Vec::new(),
            Vec::new(),
        )
    }

    /// The guard rejects only genuine collisions: a whole of all-distinct names
    /// passes the semantic boundary untouched.
    #[test]
    fn distinct_declaration_names_pass_the_semantic_boundary() {
        let tree = distinct_named_tree();
        TrueSchema::from_tree(&tree, &NameTable::empty())
            .expect("a schema whose declarations are all distinct is accepted");
    }

    /// local/local: two namespace declarations of one name. The source reader's
    /// `push_declaration` guard refuses this in text, so inject the second after
    /// lowering and assert every decode surface rejects the merged whole.
    #[test]
    fn duplicate_namespace_declarations_are_rejected_at_the_semantic_boundary() {
        let mut tree = distinct_named_tree();
        let duplicated = tree.namespace[0].clone();
        let name = duplicated.name().as_str().to_owned();
        tree.namespace.push(duplicated);
        let expected = SchemaError::DuplicateDeclaration {
            name,
            first_site: "a namespace declaration",
            second_site: "a namespace declaration",
        };
        assert_tree_rejected_across_surfaces(&tree, &expected);
    }

    /// local/imported: an imported declaration sharing the local input root's
    /// name — the exact collision (imported `Input` vs a local `Input` root)
    /// that silently merged into self-referencing emitted Rust. Inject the
    /// colliding resolved import and assert every decode surface rejects it.
    #[test]
    fn imported_declaration_colliding_with_a_root_is_rejected_at_the_semantic_boundary() {
        let mut tree = distinct_named_tree();
        tree.resolved_imports.push(plain_import("Input"));
        let expected = SchemaError::DuplicateDeclaration {
            name: "Input".to_owned(),
            first_site: "the input root",
            second_site: "a resolved import",
        };
        assert_tree_rejected_across_surfaces(&tree, &expected);
    }

    /// The realistic source path: a schema whose namespace declares a type named
    /// `Input` never touches a tamper, yet its namespace `Input` collides with
    /// the always-present input root. `push_declaration` does not look across the
    /// namespace/root boundary, so the collision reaches `from_tree` through
    /// ordinary lowering and is rejected there.
    #[test]
    fn a_namespace_type_named_for_a_root_is_rejected_at_lowering() {
        let source = "{}\n[Start.Command]\n[Finish.Report]\n{\n  Command.{ Topic }\n  Report.{ Topic }\n  Topic.String\n  Input.Integer\n}\n{}\n{}";
        let error = SchemaEngine::default()
            .lower_source(source, SchemaIdentity::new("root-collision:lib", "0.1.0"))
            .expect_err("a namespace type named Input collides with the input root");
        assert_eq!(
            error,
            SchemaError::DuplicateDeclaration {
                name: "Input".to_owned(),
                first_site: "the input root",
                second_site: "a namespace declaration",
            },
        );
    }
}
