//! The stringless `CoreSchema` substrate: schema structure addressed entirely
//! by minted [`NominalIdentifier`]s, with every human name held apart in the
//! [`NameTable`]. See the "Core and True schema" section of `ARCHITECTURE.md`.
//!
//! Two orthogonal walks live here:
//!
//! - decomposition — `SchemaTree::decompose` splits today's name-bearing
//!   semantic tree into `(CoreSchema, NameTable)`, minting or re-associating an
//!   identifier for every local declaration; and
//! - projection — `CoreSchema::project` reassembles the human-facing tree
//!   from the substrate plus a table, so a rename through the table changes the
//!   projection without touching the substrate.
//!
//! The two walks are inverse by construction: `project(decompose(tree)) ==
//! tree`, witnessed over the fixture corpus in `tests/core_projection.rs`.
//!
//! # What carries an identifier, and what stays data
//!
//! Every *local declaration* is identifier-addressed: namespace types (struct,
//! enum, newtype), fields, enum variants, generic parameter binders, roots,
//! plain type references, local application heads, and impl-block targets.
//! Member declarations (fields,
//! variants, binders) mint from their OWNER's identifier and their local name
//! (see [`crate::NominalIdentifier::mint_member`]), so equal member names under
//! different owners stay distinct AND a member identifier is stable when its
//! owner is renamed; their row stores the owner identifier plus the local name,
//! and projection takes the local name directly. Top-level names mint and
//! project verbatim.
//!
//! A loaded schema is a WHOLE. Import resolution happens at load, and after it
//! there is one substrate, one identifier space, one name table — no "local"
//! versus "imported" distinction anywhere inside. A declaration that arrived
//! through an import is a declaration like any other: a minted identifier with a
//! name-table row, its names in the table and not the structure. A resolved
//! import's frame body — its binder identifiers and its variant list —
//! therefore decomposes exactly as a natively declared frame does (see
//! [`CoreResolvedImport`]).
//!
//! What stays as data is only what is genuinely NOT a declaration in the whole,
//! under the tenet that a use-site name may be "a reference/path/name value
//! under the expected type":
//!
//! - the cross-crate import SOURCE path (`crate:module:Type`) a resolved import
//!   carries — provenance naming a location in another crate's source, not a
//!   declaration in this whole, and the source text the principle leaves in
//!   source form; and
//! - impl catalogs — Rust-surface contract signatures verified against
//!   [`crate::RustSurface`] facts.
//!
//! Explicit field disambiguators are name data and therefore live on the
//! `NameTable` side (as the field's current name), never in the substrate.
//!
//! `SchemaIdentity` is deliberately absent: the target core hash is over the
//! substrate with identity pulled out, so the identity rides on the view, and
//! `CoreSchema::project` takes it as an argument.

use crate::{
    SchemaError, SchemaIdentity,
    identifier::{DeclarationKind, NameHarvest, NameTable, NominalIdentifier},
    resolution::{ImportSource, ResolvedImport},
    schema::{
        ApplicationHead, Declaration, EnumDeclaration, EnumVariant, FieldDeclaration, ImplBlock,
        ImplCatalog, ImportDeclaration, MultiTypeReferenceProjection, Name, NewtypeDeclaration,
        Root, RootApplication, SchemaTree, SingleTypeReferenceProjection, StructDeclaration,
        TypeDeclaration, TypeReference, ValueReferenceProjection, Visibility,
    },
};

impl SchemaTree {
    /// Split this name-bearing tree into the stringless substrate and its name
    /// table, re-associating identifiers against `prior` (use
    /// [`NameTable::empty`] when there is none). Decomposition is total: every
    /// local declaration receives an identifier, and every identifier the
    /// substrate carries has a row in the returned table.
    pub(crate) fn decompose(
        &self,
        prior: &NameTable,
    ) -> Result<(CoreSchema, NameTable), SchemaError> {
        let mut harvest = NameHarvest::new(prior);
        let imports = self
            .imports()
            .iter()
            .map(|import| CoreImportDeclaration::from_import_declaration(import, &mut harvest))
            .collect();
        let resolved_imports = self
            .resolved_imports()
            .iter()
            .map(|import| CoreResolvedImport::from_resolved_import(import, &mut harvest))
            .collect();
        let input = CoreRoot::from_root(self.input(), &mut harvest);
        let output = CoreRoot::from_root(self.output(), &mut harvest);
        let namespace = self
            .namespace()
            .iter()
            .map(|declaration| CoreDeclaration::from_declaration(declaration, &mut harvest))
            .collect();
        let impl_blocks = self
            .impl_blocks()
            .iter()
            .map(|block| CoreImplBlock::from_impl_block(block, &mut harvest))
            .collect();
        let core = CoreSchema {
            imports,
            resolved_imports,
            input,
            output,
            namespace,
            impl_blocks,
        };
        Ok((core, harvest.into_table()))
    }
}

/// The stringless schema substrate. Structure only: every local declaration is
/// carried by its [`NominalIdentifier`], and the human names live in the
/// [`NameTable`] produced by the same decomposition. The identity is not part
/// of the substrate; `CoreSchema::project` takes it as an argument.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct CoreSchema {
    pub(crate) imports: Vec<CoreImportDeclaration>,
    pub(crate) resolved_imports: Vec<CoreResolvedImport>,
    pub(crate) input: CoreRoot,
    pub(crate) output: CoreRoot,
    pub(crate) namespace: Vec<CoreDeclaration>,
    pub(crate) impl_blocks: Vec<CoreImplBlock>,
}

impl CoreSchema {
    /// Reassemble the human-facing tree from this substrate plus a name table.
    /// Every identifier the substrate carries must resolve through the table;
    /// a miss is the typed [`SchemaError::CoreProjectionNameAbsent`] error.
    pub(crate) fn project(
        &self,
        names: &NameTable,
        identity: SchemaIdentity,
    ) -> Result<SchemaTree, SchemaError> {
        Ok(SchemaTree::new(
            identity,
            self.imports
                .iter()
                .map(|import| import.project(names))
                .collect::<Result<Vec<_>, _>>()?,
            self.resolved_imports
                .iter()
                .map(|import| import.project(names))
                .collect::<Result<Vec<_>, _>>()?,
            self.input.project(names)?,
            self.output.project(names)?,
            self.namespace
                .iter()
                .map(|declaration| declaration.project(names))
                .collect::<Result<_, _>>()?,
        )
        .with_impl_blocks(
            self.impl_blocks
                .iter()
                .map(|block| block.project(names))
                .collect::<Result<_, _>>()?,
        ))
    }

    pub fn namespace(&self) -> &[CoreDeclaration] {
        &self.namespace
    }

    /// The substrate's canonical rkyv bytes. Renames through the `NameTable`
    /// must leave these bytes untouched — the substrate carries no names.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, SchemaError> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .map(|bytes| bytes.to_vec())
            .map_err(|_| SchemaError::ArchiveEncode)
    }
}

/// A root position in the substrate: the enum-body form or the application
/// form, mirroring [`Root`] with the names replaced by identifiers.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum CoreRoot {
    Enum(CoreEnum),
    Application(Box<CoreRootApplication>),
}

impl CoreRoot {
    pub(crate) fn from_root(root: &Root, harvest: &mut NameHarvest<'_>) -> Self {
        match root {
            Root::Enum(declaration) => Self::Enum(CoreEnum::from_enum(declaration, harvest)),
            Root::Application(application) => Self::Application(Box::new(
                CoreRootApplication::from_application(application, harvest),
            )),
        }
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<Root, SchemaError> {
        Ok(match self {
            Self::Enum(declaration) => Root::Enum(declaration.project(names)?),
            Self::Application(application) => Root::application(application.project(names)?),
        })
    }
}

/// An application-form root: the position identifier plus the applied head and
/// arguments, mirroring [`RootApplication`].
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct CoreRootApplication {
    pub(crate) identifier: NominalIdentifier,
    pub(crate) head: CoreApplicationHead,
    pub(crate) arguments: Vec<CoreReference>,
}

impl CoreRootApplication {
    pub(crate) fn from_application(
        application: &RootApplication,
        harvest: &mut NameHarvest<'_>,
    ) -> Self {
        Self {
            identifier: harvest.declare(DeclarationKind::Type, application.name()),
            head: CoreApplicationHead::from_head(application.head(), harvest),
            arguments: application
                .arguments()
                .iter()
                .map(|argument| CoreReference::from_reference(argument, harvest))
                .collect(),
        }
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<RootApplication, SchemaError> {
        Ok(RootApplication::new(
            names.projected_name(&self.identifier)?.clone(),
            self.head.project(names)?,
            self.arguments
                .iter()
                .map(|argument| argument.project(names))
                .collect::<Result<_, _>>()?,
        ))
    }
}

/// A generic-application head in the substrate. Under the loaded-whole
/// principle both a locally-declared head and an imported one name a
/// declaration in the whole and are identifier-addressed; an imported head
/// additionally carries its resolved import so the applied frame's body travels
/// with it, mirroring [`ApplicationHead`].
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum CoreApplicationHead {
    Local(NominalIdentifier),
    Imported(CoreResolvedImport),
}

impl CoreApplicationHead {
    pub(crate) fn from_head(head: &ApplicationHead, harvest: &mut NameHarvest<'_>) -> Self {
        match head {
            ApplicationHead::Local(name) => {
                Self::Local(harvest.declare(DeclarationKind::Type, name))
            }
            ApplicationHead::Imported(import) => {
                Self::Imported(CoreResolvedImport::from_resolved_import(import, harvest))
            }
        }
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<ApplicationHead, SchemaError> {
        Ok(match self {
            Self::Local(identifier) => {
                ApplicationHead::Local(names.projected_name(identifier)?.clone())
            }
            Self::Imported(import) => ApplicationHead::Imported(import.project(names)?),
        })
    }
}

/// A brace-declared import in the substrate. The alias a human writes in the
/// imports brace IS the imported declaration's current name — a declaration in
/// the loaded whole like any other — so it is identifier-addressed and its name
/// lives in the [`NameTable`], carried by the very identifier the matching
/// [`CoreResolvedImport`] mints. Storing the alias identifier rather than the
/// alias string keeps it out of the substrate's canonical bytes and makes a
/// rename of the imported declaration move the imports brace and the body
/// together, so the projection stays internally consistent and reloadable.
///
/// Only the cross-crate SOURCE reference stays as data — genuine provenance
/// naming a location in another crate's source, which the stringless principle
/// leaves in source form.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct CoreImportDeclaration {
    identifier: NominalIdentifier,
    source: TypeReference,
}

impl CoreImportDeclaration {
    pub(crate) fn from_import_declaration(
        declaration: &ImportDeclaration,
        harvest: &mut NameHarvest<'_>,
    ) -> Self {
        // The alias is the imported declaration's top-level name; declaring it
        // here mints (or re-associates) exactly the identifier the matching
        // resolved import carries, and the duplicate row collapses in the built
        // table, so brace and body address the one declaration.
        Self {
            identifier: harvest.declare(DeclarationKind::Type, &declaration.local_name),
            source: declaration.source.clone(),
        }
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<ImportDeclaration, SchemaError> {
        Ok(ImportDeclaration {
            local_name: names.projected_name(&self.identifier)?.clone(),
            source: self.source.clone(),
        })
    }
}

/// A resolved import in the substrate. Under the loaded-whole principle an
/// imported declaration is a declaration like any other: its identity is a
/// minted identifier with a name-table row, and its frame body — the binder
/// identifiers and the variant list — decomposes exactly as a natively declared
/// frame does. Only the cross-crate SOURCE path stays as data, naming a location
/// in another crate's source rather than a declaration in this whole.
///
/// The recursive `variants` field is `omit_bounds`, and the container carries
/// the same archive / serialize / deserialize bound attributes
/// [`CoreReference`] uses, so the `CoreReference -> CoreApplicationHead ->
/// CoreResolvedImport -> CoreVariant -> CoreReference` cycle closes for rkyv the
/// same way the name-bearing [`ResolvedImport`] closes its own.
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
pub struct CoreResolvedImport {
    identifier: NominalIdentifier,
    source: ImportSource,
    parameter_count: Option<u32>,
    parameters: Vec<NominalIdentifier>,
    #[rkyv(omit_bounds)]
    variants: Vec<CoreVariant>,
}

impl CoreResolvedImport {
    pub(crate) fn from_resolved_import(
        import: &ResolvedImport,
        harvest: &mut NameHarvest<'_>,
    ) -> Self {
        // The imported declaration joins the loaded whole under its own name:
        // its identity is a minted identifier with a name-table row, and its
        // frame binders and variants decompose as members of that identifier,
        // exactly as a native parameterized enum does. Deterministic minting
        // makes those member identifiers identical to the ones the dependency
        // mints when it is lowered standalone.
        let identifier = harvest.declare(DeclarationKind::Type, import.local_name());
        let parameters = import
            .parameters()
            .iter()
            .map(|parameter| {
                harvest.declare_member(DeclarationKind::Generic, identifier, parameter)
            })
            .collect();
        let variants = import
            .variants()
            .iter()
            .map(|variant| CoreVariant::from_variant(variant, identifier, harvest))
            .collect();
        Self {
            identifier,
            source: import.source().clone(),
            parameter_count: import.parameter_count().map(|count| count as u32),
            parameters,
            variants,
        }
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<ResolvedImport, SchemaError> {
        Ok(ResolvedImport::from_projected_parts(
            names.projected_name(&self.identifier)?.clone(),
            self.source.clone(),
            self.parameter_count,
            self.parameters
                .iter()
                .map(|parameter| Ok(Name::new(names.projected_name(parameter)?.local_part())))
                .collect::<Result<_, SchemaError>>()?,
            self.variants
                .iter()
                .map(|variant| variant.project(names))
                .collect::<Result<_, _>>()?,
        ))
    }
}

/// A namespace declaration in the substrate, mirroring [`Declaration`]. The
/// declaration's identifier lives on its value — the same invariant as
/// [`Declaration`], whose name is always its value's name.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct CoreDeclaration {
    pub(crate) visibility: Visibility,
    pub(crate) parameters: Vec<NominalIdentifier>,
    pub(crate) value: CoreType,
    pub(crate) impls: ImplCatalog,
}

impl CoreDeclaration {
    pub(crate) fn from_declaration(
        declaration: &Declaration,
        harvest: &mut NameHarvest<'_>,
    ) -> Self {
        // A declaration's identity is its value's type identifier. Associate it
        // up front so the generic binders anchor to the owner IDENTIFIER, then
        // the value declares the very same identifier as it decomposes.
        let owner = harvest.associate(DeclarationKind::Type, declaration.name());
        Self {
            visibility: declaration.visibility(),
            parameters: declaration
                .parameters()
                .iter()
                .map(|parameter| harvest.declare_member(DeclarationKind::Generic, owner, parameter))
                .collect(),
            value: CoreType::from_type_declaration(declaration.value(), harvest),
            impls: declaration.impls().clone(),
        }
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<Declaration, SchemaError> {
        let value = self.value.project(names)?;
        let declaration = match self.visibility {
            Visibility::Public => Declaration::public(value),
            Visibility::Private => Declaration::private(value),
        };
        Ok(declaration
            .with_parameters(
                self.parameters
                    .iter()
                    .map(|parameter| Ok(Name::new(names.projected_name(parameter)?.local_part())))
                    .collect::<Result<_, SchemaError>>()?,
            )
            .with_impls(self.impls.clone()))
    }

    /// The declaration's identifier — carried by its value, mirroring the
    /// [`Declaration`] invariant that the declaration name is the value's name.
    pub fn identifier(&self) -> NominalIdentifier {
        self.value.identifier()
    }
}

/// A declared type body in the substrate, mirroring [`TypeDeclaration`].
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum CoreType {
    Struct(CoreStruct),
    Enum(CoreEnum),
    Newtype(CoreNewtype),
}

impl CoreType {
    pub(crate) fn from_type_declaration(
        declaration: &TypeDeclaration,
        harvest: &mut NameHarvest<'_>,
    ) -> Self {
        match declaration {
            TypeDeclaration::Struct(declaration) => {
                Self::Struct(CoreStruct::from_struct(declaration, harvest))
            }
            TypeDeclaration::Enum(declaration) => {
                Self::Enum(CoreEnum::from_enum(declaration, harvest))
            }
            TypeDeclaration::Newtype(declaration) => {
                Self::Newtype(CoreNewtype::from_newtype(declaration, harvest))
            }
        }
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<TypeDeclaration, SchemaError> {
        Ok(match self {
            Self::Struct(declaration) => TypeDeclaration::Struct(declaration.project(names)?),
            Self::Enum(declaration) => TypeDeclaration::Enum(declaration.project(names)?),
            Self::Newtype(declaration) => TypeDeclaration::Newtype(declaration.project(names)?),
        })
    }

    fn identifier(&self) -> NominalIdentifier {
        match self {
            Self::Struct(declaration) => declaration.identifier,
            Self::Enum(declaration) => declaration.identifier,
            Self::Newtype(declaration) => declaration.identifier,
        }
    }
}

/// A struct declaration in the substrate, mirroring [`StructDeclaration`].
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct CoreStruct {
    pub(crate) identifier: NominalIdentifier,
    pub(crate) fields: Vec<CoreField>,
}

impl CoreStruct {
    pub(crate) fn from_struct(
        declaration: &StructDeclaration,
        harvest: &mut NameHarvest<'_>,
    ) -> Self {
        let identifier = harvest.declare(DeclarationKind::Type, &declaration.name);
        Self {
            identifier,
            fields: declaration
                .fields
                .iter()
                .map(|field| CoreField::from_field(field, identifier, harvest))
                .collect(),
        }
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<StructDeclaration, SchemaError> {
        Ok(StructDeclaration::new(
            names.projected_name(&self.identifier)?.clone(),
            self.fields
                .iter()
                .map(|field| field.project(names))
                .collect::<Result<_, _>>()?,
        ))
    }
}

/// A struct field in the substrate, mirroring [`FieldDeclaration`]. The field
/// mints its identifier from its owner-qualified current name; only an
/// explicit disambiguator is stored as a table row. A derived field name is a
/// pure projection — snake_case of a plain type name, or the generic
/// definition's per-kind pattern for an application — recomputed on demand
/// from the reference, so a rename of the referenced type moves the derived
/// name without any stored name data.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct CoreField {
    pub(crate) identifier: NominalIdentifier,
    pub(crate) reference: CoreReference,
}

impl CoreField {
    pub(crate) fn from_field(
        field: &FieldDeclaration,
        owner: NominalIdentifier,
        harvest: &mut NameHarvest<'_>,
    ) -> Self {
        // The field mints from its OWNER's identifier and its local name, so its
        // identity survives an owner rename by construction. A field whose
        // current name equals its reference's derivation carries no name data:
        // the identifier still mints, but no table row is stored, and the name
        // is derived on demand.
        let identifier = if field.name == field.reference.derived_field_name() {
            harvest.associate_member(DeclarationKind::Field, owner, &field.name)
        } else {
            harvest.declare_member(DeclarationKind::Field, owner, &field.name)
        };
        Self {
            identifier,
            reference: CoreReference::from_reference(&field.reference, harvest),
        }
    }

    /// The field's projected name — its explicit disambiguator row when one is
    /// stored, otherwise the on-demand derivation from its reference. This is
    /// the single source for the "stored disambiguator else derived name" rule;
    /// both the owned [`CoreField::project`] and the borrowing
    /// [`crate::view::FieldView::name`] read it, so the rule lives in exactly
    /// one place.
    pub(crate) fn name(&self, names: &NameTable) -> Result<Name, SchemaError> {
        Ok(match names.name_of(&self.identifier) {
            Some(stored) => stored.clone(),
            None => self.reference.project(names)?.derived_field_name(),
        })
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<FieldDeclaration, SchemaError> {
        Ok(FieldDeclaration {
            name: self.name(names)?,
            reference: self.reference.project(names)?,
        })
    }
}

/// An enum declaration in the substrate, mirroring [`EnumDeclaration`].
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct CoreEnum {
    pub(crate) identifier: NominalIdentifier,
    pub(crate) variants: Vec<CoreVariant>,
}

impl CoreEnum {
    pub(crate) fn from_enum(declaration: &EnumDeclaration, harvest: &mut NameHarvest<'_>) -> Self {
        let identifier = harvest.declare(DeclarationKind::Type, &declaration.name);
        Self {
            identifier,
            variants: declaration
                .variants
                .iter()
                .map(|variant| CoreVariant::from_variant(variant, identifier, harvest))
                .collect(),
        }
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<EnumDeclaration, SchemaError> {
        Ok(EnumDeclaration::new(
            names.projected_name(&self.identifier)?.clone(),
            self.variants
                .iter()
                .map(|variant| variant.project(names))
                .collect::<Result<_, _>>()?,
        ))
    }
}

/// An enum variant in the substrate, mirroring [`EnumVariant`]. The variant
/// mints from its enum-qualified name and projects the local part back.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct CoreVariant {
    pub(crate) identifier: NominalIdentifier,
    pub(crate) payload: Option<CoreReference>,
}

impl CoreVariant {
    pub(crate) fn from_variant(
        variant: &EnumVariant,
        owner: NominalIdentifier,
        harvest: &mut NameHarvest<'_>,
    ) -> Self {
        Self {
            identifier: harvest.declare_member(DeclarationKind::Variant, owner, &variant.name),
            payload: variant
                .payload
                .as_ref()
                .map(|payload| CoreReference::from_reference(payload, harvest)),
        }
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<EnumVariant, SchemaError> {
        Ok(EnumVariant {
            name: Name::new(names.projected_name(&self.identifier)?.local_part()),
            payload: self
                .payload
                .as_ref()
                .map(|payload| payload.project(names))
                .transpose()?,
        })
    }
}

/// A newtype declaration in the substrate, mirroring [`NewtypeDeclaration`].
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct CoreNewtype {
    pub(crate) identifier: NominalIdentifier,
    pub(crate) reference: CoreReference,
}

impl CoreNewtype {
    pub(crate) fn from_newtype(
        declaration: &NewtypeDeclaration,
        harvest: &mut NameHarvest<'_>,
    ) -> Self {
        Self {
            identifier: harvest.declare(DeclarationKind::Type, &declaration.name),
            reference: CoreReference::from_reference(&declaration.reference, harvest),
        }
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<NewtypeDeclaration, SchemaError> {
        Ok(NewtypeDeclaration::new(
            names.projected_name(&self.identifier)?.clone(),
            self.reference.project(names)?,
        ))
    }
}

/// A standalone impl block in the substrate, mirroring [`ImplBlock`]: the
/// target is a local declaration and is identifier-addressed; the catalog is
/// Rust-surface contract data.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct CoreImplBlock {
    pub(crate) target: NominalIdentifier,
    pub(crate) catalog: ImplCatalog,
}

impl CoreImplBlock {
    pub(crate) fn from_impl_block(block: &ImplBlock, harvest: &mut NameHarvest<'_>) -> Self {
        Self {
            target: harvest.declare(DeclarationKind::Type, block.target()),
            catalog: block.catalog().clone(),
        }
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<ImplBlock, SchemaError> {
        Ok(ImplBlock::new(
            names.projected_name(&self.target)?.clone(),
            self.catalog.clone(),
        ))
    }
}

/// A type at a reference position in the substrate, mirroring the collapsed
/// per-kind [`TypeReference`] partition one-for-one. Scalar leaves and the
/// value/const width are structure, and each generic application carries the
/// closed projection that names its lowering strategy — so the substrate
/// dispatches on kind and projection, never on a head name. `Plain` and local
/// application heads are identifier-addressed, keeping the substrate stringless.
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
pub enum CoreReference {
    String,
    Integer,
    Boolean,
    Path,
    Bytes,
    Plain(NominalIdentifier),
    SingleTypeApplication {
        projection: SingleTypeReferenceProjection,
        #[rkyv(omit_bounds)]
        argument: Box<CoreReference>,
    },
    MultiTypeApplication {
        projection: MultiTypeReferenceProjection,
        #[rkyv(omit_bounds)]
        arguments: Vec<CoreReference>,
    },
    ValueApplication {
        projection: ValueReferenceProjection,
        value: u64,
    },
    Application {
        head: CoreApplicationHead,
        #[rkyv(omit_bounds)]
        arguments: Vec<CoreReference>,
    },
}

impl CoreReference {
    pub(crate) fn from_reference(reference: &TypeReference, harvest: &mut NameHarvest<'_>) -> Self {
        match reference {
            TypeReference::String => Self::String,
            TypeReference::Integer => Self::Integer,
            TypeReference::Boolean => Self::Boolean,
            TypeReference::Path => Self::Path,
            TypeReference::Bytes => Self::Bytes,
            TypeReference::Plain(name) => Self::Plain(harvest.declare(DeclarationKind::Type, name)),
            TypeReference::SingleTypeApplication {
                projection,
                argument,
            } => Self::SingleTypeApplication {
                projection: *projection,
                argument: Box::new(Self::from_reference(argument, harvest)),
            },
            TypeReference::MultiTypeApplication {
                projection,
                arguments,
            } => Self::MultiTypeApplication {
                projection: *projection,
                arguments: arguments
                    .iter()
                    .map(|argument| Self::from_reference(argument, harvest))
                    .collect(),
            },
            TypeReference::ValueApplication { projection, value } => Self::ValueApplication {
                projection: *projection,
                value: *value,
            },
            TypeReference::Application { head, arguments } => Self::Application {
                head: CoreApplicationHead::from_head(head, harvest),
                arguments: arguments
                    .iter()
                    .map(|argument| Self::from_reference(argument, harvest))
                    .collect(),
            },
        }
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<TypeReference, SchemaError> {
        Ok(match self {
            Self::String => TypeReference::String,
            Self::Integer => TypeReference::Integer,
            Self::Boolean => TypeReference::Boolean,
            Self::Path => TypeReference::Path,
            Self::Bytes => TypeReference::Bytes,
            Self::Plain(identifier) => {
                TypeReference::Plain(names.projected_name(identifier)?.clone())
            }
            Self::SingleTypeApplication {
                projection,
                argument,
            } => TypeReference::single_type_application(*projection, argument.project(names)?),
            Self::MultiTypeApplication {
                projection,
                arguments,
            } => TypeReference::multi_type_application(
                *projection,
                arguments
                    .iter()
                    .map(|argument| argument.project(names))
                    .collect::<Result<_, _>>()?,
            ),
            Self::ValueApplication { projection, value } => {
                TypeReference::value_application(*projection, *value)
            }
            Self::Application { head, arguments } => TypeReference::Application {
                head: head.project(names)?,
                arguments: arguments
                    .iter()
                    .map(|argument| argument.project(names))
                    .collect::<Result<_, _>>()?,
            },
        })
    }
}
