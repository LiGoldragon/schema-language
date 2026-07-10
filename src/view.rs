//! The `TrueSchema` view: the public semantic schema assembled on demand from
//! the stringless [`CoreSchema`] substrate plus the [`NameTable`], per the
//! "Core and True schema" section of `ARCHITECTURE.md`.
//!
//! `TrueSchema` is not an independent data tree. It stores exactly the
//! substrate, the name table, and the schema identity; every human-facing
//! name is resolved through the table at read time, so a rename through
//! [`TrueSchema::rename`] changes the projection and every derived field name
//! without touching the substrate bytes.
//!
//! Reads go through two surfaces:
//!
//! - borrowing view types — [`RootView`], [`DeclarationView`], [`FieldView`],
//!   and their siblings — which resolve names on demand and never materialize
//!   the tree; and
//! - owned node projections — [`TrueSchema::input`], [`TrueSchema::namespace`],
//!   [`TrueSchema::type_named`], and friends — which project the familiar
//!   semantic node values for consumers that need whole values.
//!
//! The name-bearing tree (`SchemaTree`) survives only as the codec and hash
//! sidecar: NOTA text, canonical schema text, and rkyv binary bytes are
//! projected through it, so every codec surface stays value-exact with the
//! pre-split format. Derived field names are not stored anywhere — a field's
//! name is either its explicit disambiguator row in the table or the composed
//! derivation from its reference, computed at read time.

use nota::{Block, NotaDecode, NotaDecodeError, NotaEncode};

use crate::{
    SchemaError, SchemaIdentity,
    core::{
        CoreDeclaration, CoreEnum, CoreFamily, CoreField, CoreImplBlock, CoreNewtype, CoreRoot,
        CoreRootApplication, CoreSchema, CoreStream, CoreStruct, CoreType, CoreVariant,
    },
    identifier::{DeclarationKind, NameTable, NominalIdentifier},
    resolution::ResolvedImport,
    schema::{
        Declaration, EnumDeclaration, EnumVariant, FamilyDeclaration, FamilyKey, FieldDeclaration,
        ImplBlock, ImplCatalog, ImplReference, ImportDeclaration, Name, NewtypeDeclaration,
        RelationDeclaration, Root, RootApplication, SchemaTree, StreamDeclaration,
        StructDeclaration, SymbolPath, SymbolPathPosition, TableName, TypeDeclaration,
        TypeReference, Visibility,
    },
};

/// The invariant every view read relies on: a `TrueSchema` is only built by
/// total decomposition (or row-preserving rename), so every identifier its
/// substrate carries resolves through its table, except derived field names,
/// which are deliberately absent and derived from the reference instead.
const VIEW_RESOLUTION_INVARIANT: &str =
    "a TrueSchema view resolves every identifier its substrate carries";

/// The semantic schema, viewed. Structure lives in the stringless
/// [`CoreSchema`]; current human names live in the [`NameTable`]; the
/// [`SchemaIdentity`] rides alongside, outside the substrate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrueSchema {
    identity: SchemaIdentity,
    core: CoreSchema,
    names: NameTable,
}

impl TrueSchema {
    /// Wrap a lowered name-bearing tree into the split model, re-associating
    /// identifiers against `prior`.
    pub(crate) fn from_tree(tree: &SchemaTree, prior: &NameTable) -> Result<Self, SchemaError> {
        let (core, names) = tree.decompose(prior)?;
        Ok(Self {
            identity: tree.identity().clone(),
            core,
            names,
        })
    }

    /// Project the full name-bearing sidecar tree. This is the codec and hash
    /// surface: NOTA, canonical schema text, and binary bytes all pass through
    /// it, so they stay value-exact with the pre-split format.
    pub(crate) fn tree(&self) -> SchemaTree {
        self.core
            .project(&self.names, self.identity.clone())
            .expect(VIEW_RESOLUTION_INVARIANT)
    }

    pub fn identity(&self) -> &SchemaIdentity {
        &self.identity
    }

    /// Replace this schema's identity with a new version stamp without
    /// changing its declarations. The identity rides outside the substrate,
    /// so this touches neither the core bytes nor the name table.
    pub fn with_identity(mut self, identity: SchemaIdentity) -> Self {
        self.identity = identity;
        self
    }

    /// The stringless substrate. Renames never change its bytes.
    pub fn core(&self) -> &CoreSchema {
        &self.core
    }

    /// The identifier-to-current-name table the view resolves through.
    pub fn names(&self) -> &NameTable {
        &self.names
    }

    /// Rename a declaration through the name table: the identifier and the
    /// substrate stay untouched; the projection and every derived field name
    /// follow the new name on the next read.
    pub fn rename(
        &mut self,
        identifier: &NominalIdentifier,
        new_name: Name,
    ) -> Result<(), SchemaError> {
        self.names.rename(identifier, new_name)
    }

    /// The identifier currently bound to a name of the given kind, for
    /// addressing a rename.
    pub fn identifier_named(
        &self,
        kind: DeclarationKind,
        name: &Name,
    ) -> Option<NominalIdentifier> {
        self.names.identifier_of(kind, name)
    }

    pub fn imports(&self) -> &[ImportDeclaration] {
        &self.core.imports
    }

    /// The imports resolved against dependency crate schemas, projected from the
    /// substrate. Each imported declaration lives in the whole as a minted
    /// identifier with a name-table row and its frame body as identifier-carrying
    /// structure; projection rebuilds the name-bearing sidecar form.
    pub fn resolved_imports(&self) -> Vec<ResolvedImport> {
        self.tree().resolved_imports().to_vec()
    }

    /// The relation declarations, projected. Each relation-path segment that
    /// names a local declaration resolves its current name through the table,
    /// so a rename of a relation's target follows into the relation.
    pub fn relations(&self) -> Vec<RelationDeclaration> {
        self.core
            .relations
            .iter()
            .map(|relation| {
                relation
                    .project(&self.names)
                    .expect(VIEW_RESOLUTION_INVARIANT)
            })
            .collect()
    }

    pub fn input_view(&self) -> RootView<'_> {
        RootView {
            core: &self.core.input,
            names: &self.names,
        }
    }

    pub fn output_view(&self) -> RootView<'_> {
        RootView {
            core: &self.core.output,
            names: &self.names,
        }
    }

    pub fn input(&self) -> Root {
        self.input_view().to_root()
    }

    pub fn output(&self) -> Root {
        self.output_view().to_root()
    }

    pub fn input_and_output(&self) -> [Root; 2] {
        [self.input(), self.output()]
    }

    fn root_view_named(&self, name: &str) -> Option<RootView<'_>> {
        [self.input_view(), self.output_view()]
            .into_iter()
            .find(|root| root.name().as_str() == name)
    }

    /// The root carrying the given position name, projected.
    pub fn root_named(&self, name: &str) -> Option<Root> {
        self.root_view_named(name).map(|view| view.to_root())
    }

    /// The enum body of the root carrying the given position name, projected;
    /// `None` when no such root exists or the root is an application form.
    pub fn root_enum_named(&self, name: &str) -> Option<EnumDeclaration> {
        self.root_view_named(name)?
            .as_enum()
            .map(|view| view.to_declaration())
    }

    pub fn namespace_views(&self) -> Vec<DeclarationView<'_>> {
        self.core
            .namespace
            .iter()
            .map(|core| DeclarationView {
                core,
                names: &self.names,
            })
            .collect()
    }

    /// The namespace declarations, projected.
    pub fn namespace(&self) -> Vec<Declaration> {
        self.namespace_views()
            .iter()
            .map(DeclarationView::to_declaration)
            .collect()
    }

    pub fn type_view_named(&self, name: &str) -> Option<TypeDeclarationView<'_>> {
        self.namespace_views()
            .into_iter()
            .find(|view| view.name().as_str() == name)
            .map(|view| view.value())
    }

    /// The named namespace declaration's type body, projected.
    pub fn type_named(&self, name: &str) -> Option<TypeDeclaration> {
        self.type_view_named(name)
            .map(|view| view.to_type_declaration())
    }

    pub fn stream_views(&self) -> Vec<StreamView<'_>> {
        self.core
            .streams
            .iter()
            .map(|core| StreamView {
                core,
                names: &self.names,
            })
            .collect()
    }

    pub fn streams(&self) -> Vec<StreamDeclaration> {
        self.stream_views()
            .iter()
            .map(StreamView::to_stream)
            .collect()
    }

    pub fn family_views(&self) -> Vec<FamilyView<'_>> {
        self.core
            .families
            .iter()
            .map(|core| FamilyView {
                core,
                names: &self.names,
            })
            .collect()
    }

    pub fn families(&self) -> Vec<FamilyDeclaration> {
        self.family_views()
            .iter()
            .map(FamilyView::to_family)
            .collect()
    }

    pub fn impl_block_views(&self) -> Vec<ImplBlockView<'_>> {
        self.core
            .impl_blocks
            .iter()
            .map(|core| ImplBlockView {
                core,
                names: &self.names,
            })
            .collect()
    }

    /// The standalone impl blocks lowered from body-optional
    /// `TypeName {| … |}` entries, projected.
    pub fn impl_blocks(&self) -> Vec<ImplBlock> {
        self.impl_block_views()
            .iter()
            .map(ImplBlockView::to_impl_block)
            .collect()
    }

    /// The single enumerable impl manifest: every referenced impl entry across
    /// the schema, each paired with the type it targets. Targets resolve
    /// through the name table; the entries themselves are Rust-surface
    /// contract data borrowed from the substrate.
    pub fn referenced_impls(&self) -> Vec<ReferencedImplView<'_>> {
        let mut references = Vec::new();
        for declaration in &self.core.namespace {
            let target = self
                .names
                .projected_name(&declaration.identifier())
                .expect(VIEW_RESOLUTION_INVARIANT)
                .clone();
            for entry in declaration.impls.entries() {
                references.push(ReferencedImplView {
                    target: target.clone(),
                    entry,
                });
            }
        }
        for block in &self.core.impl_blocks {
            let target = self
                .names
                .projected_name(&block.target)
                .expect(VIEW_RESOLUTION_INVARIANT)
                .clone();
            for entry in block.catalog.entries() {
                references.push(ReferencedImplView {
                    target: target.clone(),
                    entry,
                });
            }
        }
        references
    }

    /// The declared generic arity of a named namespace type; `Some(0)` for a
    /// non-parameterized declaration, `None` for a name that is not a
    /// namespace declaration.
    pub fn declared_parameter_count(&self, name: &str) -> Option<usize> {
        self.namespace_views()
            .into_iter()
            .find(|view| view.name().as_str() == name)
            .map(|view| view.parameters().len())
    }

    /// The frame body of a declared parameterized enum: its binders and
    /// variant list, projected. `None` when the name is not a namespace enum.
    pub fn declared_frame_body(&self, name: &str) -> Option<(Vec<Name>, Vec<EnumVariant>)> {
        let view = self
            .namespace_views()
            .into_iter()
            .find(|view| view.name().as_str() == name)?;
        let TypeDeclarationView::Enum(body) = view.value() else {
            return None;
        };
        Some((
            view.parameters(),
            body.variants()
                .iter()
                .map(VariantView::to_variant)
                .collect(),
        ))
    }

    /// The named declared type — a namespace declaration or a root enum —
    /// projected.
    pub fn declared_type_named(&self, name: &str) -> Option<SchemaDeclaredType> {
        self.type_named(name)
            .map(SchemaDeclaredType::Namespace)
            .or_else(|| self.root_enum_named(name).map(SchemaDeclaredType::Root))
    }

    /// Monomorphize an application root into the concrete enum declaration it
    /// denotes. Delegates through the projected sidecar tree, which owns frame
    /// resolution and sibling re-aiming.
    pub fn expand_application_root(
        &self,
        application: &RootApplication,
    ) -> Option<EnumDeclaration> {
        self.tree().expand_application_root(application)
    }

    pub fn type_path(&self, type_name: &str) -> Option<SymbolPath> {
        self.tree().type_path(type_name)
    }

    pub fn root_variant_path(&self, root_name: &str, variant_name: &str) -> Option<SymbolPath> {
        self.tree().root_variant_path(root_name, variant_name)
    }

    pub fn field_path(&self, type_name: &str, field_name: &str) -> Option<SymbolPath> {
        self.tree().field_path(type_name, field_name)
    }

    pub fn enum_variant_path(&self, enum_name: &str, variant_name: &str) -> Option<SymbolPath> {
        self.tree().enum_variant_path(enum_name, variant_name)
    }

    pub fn symbol_path_position<'path>(
        &self,
        path: &'path SymbolPath,
    ) -> Option<SymbolPathPosition<'path>> {
        self.tree().symbol_path_position(path)
    }

    /// The canonical `.schema` text, projected through the sidecar tree.
    pub fn to_schema_text(&self) -> String {
        self.tree().to_schema_text()
    }

    /// The canonical rkyv bytes of the projected tree — byte-compatible with
    /// the pre-split stored-tree format.
    pub fn to_binary_bytes(&self) -> Result<Vec<u8>, SchemaError> {
        self.tree().to_binary_bytes()
    }

    pub fn from_binary_bytes(bytes: &[u8]) -> Result<Self, SchemaError> {
        Self::from_tree(&SchemaTree::from_binary_bytes(bytes)?, &NameTable::empty())
    }
}

/// A `TrueSchema` decodes from and encodes to the same NOTA projection as the
/// pre-split stored tree: decoding parses the tree shape and decomposes it;
/// encoding projects the sidecar tree. Round trips are value-exact.
impl NotaDecode for TrueSchema {
    fn from_nota_block(block: &Block) -> Result<Self, NotaDecodeError> {
        Self::from_tree(&SchemaTree::from_nota_block(block)?, &NameTable::empty()).map_err(
            |error| NotaDecodeError::InvalidValue {
                type_name: "TrueSchema",
                value: String::new(),
                reason: error.to_string(),
            },
        )
    }
}

impl NotaEncode for TrueSchema {
    fn to_nota(&self) -> String {
        self.tree().to_nota()
    }
}

/// The named declared type a lookup resolved to: a namespace declaration's
/// body or an input/output root enum, projected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchemaDeclaredType {
    Namespace(TypeDeclaration),
    Root(EnumDeclaration),
}

/// One referenced impl entry paired with its projected target type name. The
/// entry borrows the substrate's catalog data; the target is resolved through
/// the name table.
#[derive(Clone, Debug)]
pub struct ReferencedImplView<'schema> {
    target: Name,
    entry: &'schema ImplReference,
}

impl<'schema> ReferencedImplView<'schema> {
    pub fn target(&self) -> &Name {
        &self.target
    }

    pub fn entry(&self) -> &'schema ImplReference {
        self.entry
    }
}

/// A root position, viewed: names resolve through the table on demand.
#[derive(Clone, Copy)]
pub struct RootView<'schema> {
    core: &'schema CoreRoot,
    names: &'schema NameTable,
}

impl<'schema> RootView<'schema> {
    /// The root's identity name: an enum root carries its declaration name,
    /// an application root its position name.
    pub fn name(&self) -> Name {
        match self.core {
            CoreRoot::Enum(declaration) => EnumView {
                core: declaration,
                names: self.names,
            }
            .name(),
            CoreRoot::Application(application) => self
                .names
                .projected_name(&application.identifier)
                .expect(VIEW_RESOLUTION_INVARIANT)
                .clone(),
        }
    }

    pub fn as_enum(&self) -> Option<EnumView<'schema>> {
        match self.core {
            CoreRoot::Enum(declaration) => Some(EnumView {
                core: declaration,
                names: self.names,
            }),
            CoreRoot::Application(_) => None,
        }
    }

    pub fn as_application(&self) -> Option<RootApplicationView<'schema>> {
        match self.core {
            CoreRoot::Application(application) => Some(RootApplicationView {
                core: application,
                names: self.names,
            }),
            CoreRoot::Enum(_) => None,
        }
    }

    pub fn to_root(&self) -> Root {
        self.core
            .project(self.names)
            .expect(VIEW_RESOLUTION_INVARIANT)
    }
}

/// An application-form root, viewed.
#[derive(Clone, Copy)]
pub struct RootApplicationView<'schema> {
    core: &'schema CoreRootApplication,
    names: &'schema NameTable,
}

impl RootApplicationView<'_> {
    pub fn name(&self) -> Name {
        self.names
            .projected_name(&self.core.identifier)
            .expect(VIEW_RESOLUTION_INVARIANT)
            .clone()
    }

    pub fn to_application(&self) -> RootApplication {
        self.core
            .project(self.names)
            .expect(VIEW_RESOLUTION_INVARIANT)
    }
}

/// A namespace declaration, viewed.
#[derive(Clone, Copy)]
pub struct DeclarationView<'schema> {
    core: &'schema CoreDeclaration,
    names: &'schema NameTable,
}

impl<'schema> DeclarationView<'schema> {
    pub fn name(&self) -> Name {
        self.names
            .projected_name(&self.core.identifier())
            .expect(VIEW_RESOLUTION_INVARIANT)
            .clone()
    }

    pub fn visibility(&self) -> Visibility {
        self.core.visibility
    }

    pub fn is_private(&self) -> bool {
        self.core.visibility == Visibility::Private
    }

    /// The declared type-parameter binders, resolved to their local names.
    pub fn parameters(&self) -> Vec<Name> {
        self.core
            .parameters
            .iter()
            .map(|parameter| {
                Name::new(
                    self.names
                        .projected_name(parameter)
                        .expect(VIEW_RESOLUTION_INVARIANT)
                        .local_part(),
                )
            })
            .collect()
    }

    /// The impl catalog — Rust-surface contract data borrowed from the
    /// substrate.
    pub fn impls(&self) -> &'schema ImplCatalog {
        &self.core.impls
    }

    pub fn value(&self) -> TypeDeclarationView<'schema> {
        match &self.core.value {
            CoreType::Struct(declaration) => TypeDeclarationView::Struct(StructView {
                core: declaration,
                names: self.names,
            }),
            CoreType::Enum(declaration) => TypeDeclarationView::Enum(EnumView {
                core: declaration,
                names: self.names,
            }),
            CoreType::Newtype(declaration) => TypeDeclarationView::Newtype(NewtypeView {
                core: declaration,
                names: self.names,
            }),
        }
    }

    pub fn to_declaration(&self) -> Declaration {
        self.core
            .project(self.names)
            .expect(VIEW_RESOLUTION_INVARIANT)
    }
}

/// A declared type body, viewed.
#[derive(Clone, Copy)]
pub enum TypeDeclarationView<'schema> {
    Struct(StructView<'schema>),
    Enum(EnumView<'schema>),
    Newtype(NewtypeView<'schema>),
}

impl TypeDeclarationView<'_> {
    pub fn name(&self) -> Name {
        match self {
            Self::Struct(view) => view.name(),
            Self::Enum(view) => view.name(),
            Self::Newtype(view) => view.name(),
        }
    }

    pub fn to_type_declaration(&self) -> TypeDeclaration {
        match self {
            Self::Struct(view) => TypeDeclaration::Struct(view.to_struct()),
            Self::Enum(view) => TypeDeclaration::Enum(view.to_declaration()),
            Self::Newtype(view) => TypeDeclaration::Newtype(view.to_newtype()),
        }
    }
}

/// A struct declaration, viewed.
#[derive(Clone, Copy)]
pub struct StructView<'schema> {
    core: &'schema CoreStruct,
    names: &'schema NameTable,
}

impl<'schema> StructView<'schema> {
    pub fn name(&self) -> Name {
        self.names
            .projected_name(&self.core.identifier)
            .expect(VIEW_RESOLUTION_INVARIANT)
            .clone()
    }

    pub fn fields(&self) -> Vec<FieldView<'schema>> {
        self.core
            .fields
            .iter()
            .map(|core| FieldView {
                core,
                names: self.names,
            })
            .collect()
    }

    pub fn to_struct(&self) -> StructDeclaration {
        self.core
            .project(self.names)
            .expect(VIEW_RESOLUTION_INVARIANT)
    }
}

/// A struct field, viewed. The name is name data only when the field carries
/// an explicit disambiguator row; otherwise it is the composed on-demand
/// derivation from the field's reference — snake_case of a plain type name,
/// or the generic definition's per-kind pattern (`x_vector`, `optional_x`,
/// `x_scope`) for an application — so a rename of the referenced type moves
/// the derived name with it.
#[derive(Clone, Copy)]
pub struct FieldView<'schema> {
    core: &'schema CoreField,
    names: &'schema NameTable,
}

impl FieldView<'_> {
    pub fn name(&self) -> Name {
        self.core.name(self.names).expect(VIEW_RESOLUTION_INVARIANT)
    }

    /// Whether this field's name is an explicit stored disambiguator rather
    /// than an on-demand derivation.
    pub fn has_explicit_name(&self) -> bool {
        self.names.name_of(&self.core.identifier).is_some()
    }

    pub fn reference(&self) -> TypeReference {
        self.core
            .reference
            .project(self.names)
            .expect(VIEW_RESOLUTION_INVARIANT)
    }

    pub fn to_field(&self) -> FieldDeclaration {
        self.core
            .project(self.names)
            .expect(VIEW_RESOLUTION_INVARIANT)
    }
}

/// An enum declaration, viewed.
#[derive(Clone, Copy)]
pub struct EnumView<'schema> {
    core: &'schema CoreEnum,
    names: &'schema NameTable,
}

impl<'schema> EnumView<'schema> {
    pub fn name(&self) -> Name {
        self.names
            .projected_name(&self.core.identifier)
            .expect(VIEW_RESOLUTION_INVARIANT)
            .clone()
    }

    pub fn variants(&self) -> Vec<VariantView<'schema>> {
        self.core
            .variants
            .iter()
            .map(|core| VariantView {
                core,
                names: self.names,
            })
            .collect()
    }

    pub fn to_declaration(&self) -> EnumDeclaration {
        self.core
            .project(self.names)
            .expect(VIEW_RESOLUTION_INVARIANT)
    }
}

/// An enum variant, viewed.
#[derive(Clone, Copy)]
pub struct VariantView<'schema> {
    core: &'schema CoreVariant,
    names: &'schema NameTable,
}

impl VariantView<'_> {
    pub fn name(&self) -> Name {
        Name::new(
            self.names
                .projected_name(&self.core.identifier)
                .expect(VIEW_RESOLUTION_INVARIANT)
                .local_part(),
        )
    }

    pub fn payload(&self) -> Option<TypeReference> {
        self.core.payload.as_ref().map(|payload| {
            payload
                .project(self.names)
                .expect(VIEW_RESOLUTION_INVARIANT)
        })
    }

    pub fn to_variant(&self) -> EnumVariant {
        self.core
            .project(self.names)
            .expect(VIEW_RESOLUTION_INVARIANT)
    }
}

/// A newtype declaration, viewed.
#[derive(Clone, Copy)]
pub struct NewtypeView<'schema> {
    core: &'schema CoreNewtype,
    names: &'schema NameTable,
}

impl NewtypeView<'_> {
    pub fn name(&self) -> Name {
        self.names
            .projected_name(&self.core.identifier)
            .expect(VIEW_RESOLUTION_INVARIANT)
            .clone()
    }

    pub fn reference(&self) -> TypeReference {
        self.core
            .reference
            .project(self.names)
            .expect(VIEW_RESOLUTION_INVARIANT)
    }

    pub fn to_newtype(&self) -> NewtypeDeclaration {
        self.core
            .project(self.names)
            .expect(VIEW_RESOLUTION_INVARIANT)
    }
}

/// A stream declaration, viewed.
#[derive(Clone, Copy)]
pub struct StreamView<'schema> {
    core: &'schema CoreStream,
    names: &'schema NameTable,
}

impl StreamView<'_> {
    pub fn name(&self) -> Name {
        self.names
            .projected_name(&self.core.identifier)
            .expect(VIEW_RESOLUTION_INVARIANT)
            .clone()
    }

    pub fn to_stream(&self) -> StreamDeclaration {
        self.core
            .project(self.names)
            .expect(VIEW_RESOLUTION_INVARIANT)
    }
}

/// A family declaration, viewed. The table name is a storage coordinate
/// borrowed from the substrate; the family and record names resolve through
/// the table.
#[derive(Clone, Copy)]
pub struct FamilyView<'schema> {
    core: &'schema CoreFamily,
    names: &'schema NameTable,
}

impl FamilyView<'_> {
    pub fn name(&self) -> Name {
        self.names
            .projected_name(&self.core.identifier)
            .expect(VIEW_RESOLUTION_INVARIANT)
            .clone()
    }

    pub fn record(&self) -> Name {
        self.names
            .projected_name(&self.core.record)
            .expect(VIEW_RESOLUTION_INVARIANT)
            .clone()
    }

    pub fn table(&self) -> &TableName {
        &self.core.table
    }

    pub fn key(&self) -> FamilyKey {
        self.core.key
    }

    pub fn to_family(&self) -> FamilyDeclaration {
        self.core
            .project(self.names)
            .expect(VIEW_RESOLUTION_INVARIANT)
    }
}

/// A standalone impl block, viewed: the target resolves through the table;
/// the catalog is contract data borrowed from the substrate.
#[derive(Clone, Copy)]
pub struct ImplBlockView<'schema> {
    core: &'schema CoreImplBlock,
    names: &'schema NameTable,
}

impl<'schema> ImplBlockView<'schema> {
    pub fn target(&self) -> Name {
        self.names
            .projected_name(&self.core.target)
            .expect(VIEW_RESOLUTION_INVARIANT)
            .clone()
    }

    pub fn catalog(&self) -> &'schema ImplCatalog {
        &self.core.catalog
    }

    pub fn to_impl_block(&self) -> ImplBlock {
        self.core
            .project(self.names)
            .expect(VIEW_RESOLUTION_INVARIANT)
    }
}
