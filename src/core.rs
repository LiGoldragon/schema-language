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
//! streams, families, plain type references, local application heads, and
//! impl-block targets. Member declarations (fields, variants, binders) mint
//! from their owner-qualified name (`Owner:member`) so equal member names under
//! different owners stay distinct; their projection takes the qualified name's
//! local part. Top-level names mint and project verbatim.
//!
//! A closed set of *reference and contract values* stays as data, under the
//! tenet that a use-site name may be "a reference/path/name value under the
//! expected type":
//!
//! - import declarations and resolved imports — cross-crate reference paths and
//!   the imported contract bodies they carry;
//! - impl catalogs — Rust-surface contract signatures verified against
//!   [`crate::RustSurface`] facts;
//! - relation values — symbol paths whose segment kinds are positional
//!   reference data, not local declarations; and
//! - table names — storage coordinates, explicitly not schema symbols.
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
    resolution::ResolvedImport,
    schema::{
        ApplicationHead, Declaration, EnumDeclaration, EnumVariant, FamilyDeclaration, FamilyKey,
        FieldDeclaration, ImplBlock, ImplCatalog, ImportDeclaration, Name, NewtypeDeclaration,
        RelationDeclaration, Root, RootApplication, SchemaTree, StreamDeclaration, StreamRelation,
        StructDeclaration, TableName, TypeDeclaration, TypeReference, Visibility,
    },
};

impl SchemaTree {
    /// Split this name-bearing tree into the stringless substrate and its name
    /// table, re-associating identifiers against `prior` (use
    /// [`NameTable::empty`] when there is none). Decomposition is total: every
    /// local declaration receives an identifier, and every identifier the
    /// substrate carries has a row in the returned table.
    pub(crate) fn decompose(&self, prior: &NameTable) -> (CoreSchema, NameTable) {
        let mut harvest = NameHarvest::new(prior);
        let core = CoreSchema {
            imports: self.imports().to_vec(),
            resolved_imports: self.resolved_imports().to_vec(),
            input: CoreRoot::from_root(self.input(), &mut harvest),
            output: CoreRoot::from_root(self.output(), &mut harvest),
            namespace: self
                .namespace()
                .iter()
                .map(|declaration| CoreDeclaration::from_declaration(declaration, &mut harvest))
                .collect(),
            streams: self
                .streams()
                .iter()
                .map(|stream| CoreStream::from_stream(stream, &mut harvest))
                .collect(),
            families: self
                .families()
                .iter()
                .map(|family| CoreFamily::from_family(family, &mut harvest))
                .collect(),
            relations: self.relations().to_vec(),
            impl_blocks: self
                .impl_blocks()
                .iter()
                .map(|block| CoreImplBlock::from_impl_block(block, &mut harvest))
                .collect(),
        };
        (core, harvest.into_table())
    }
}

/// The stringless schema substrate. Structure only: every local declaration is
/// carried by its [`NominalIdentifier`], and the human names live in the
/// [`NameTable`] produced by the same decomposition. The identity is not part
/// of the substrate; `CoreSchema::project` takes it as an argument.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct CoreSchema {
    pub(crate) imports: Vec<ImportDeclaration>,
    pub(crate) resolved_imports: Vec<ResolvedImport>,
    pub(crate) input: CoreRoot,
    pub(crate) output: CoreRoot,
    pub(crate) namespace: Vec<CoreDeclaration>,
    pub(crate) streams: Vec<CoreStream>,
    pub(crate) families: Vec<CoreFamily>,
    pub(crate) relations: Vec<RelationDeclaration>,
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
            self.imports.clone(),
            self.resolved_imports.clone(),
            self.input.project(names)?,
            self.output.project(names)?,
            self.namespace
                .iter()
                .map(|declaration| declaration.project(names))
                .collect::<Result<_, _>>()?,
            self.streams
                .iter()
                .map(|stream| stream.project(names))
                .collect::<Result<_, _>>()?,
            self.families
                .iter()
                .map(|family| family.project(names))
                .collect::<Result<_, _>>()?,
            self.relations.clone(),
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

/// A generic-application head in the substrate: a local head is
/// identifier-addressed; an imported head keeps its resolved import verbatim as
/// cross-crate contract data, mirroring [`ApplicationHead`].
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum CoreApplicationHead {
    Local(NominalIdentifier),
    Imported(ResolvedImport),
}

impl CoreApplicationHead {
    pub(crate) fn from_head(head: &ApplicationHead, harvest: &mut NameHarvest<'_>) -> Self {
        match head {
            ApplicationHead::Local(name) => {
                Self::Local(harvest.declare(DeclarationKind::Type, name))
            }
            ApplicationHead::Imported(import) => Self::Imported(import.clone()),
        }
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<ApplicationHead, SchemaError> {
        Ok(match self {
            Self::Local(identifier) => {
                ApplicationHead::Local(names.projected_name(identifier)?.clone())
            }
            Self::Imported(import) => ApplicationHead::Imported(import.clone()),
        })
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
        let owner = declaration.name();
        Self {
            visibility: declaration.visibility(),
            parameters: declaration
                .parameters()
                .iter()
                .map(|parameter| {
                    harvest.declare(
                        DeclarationKind::Generic,
                        &parameter.qualified_under(Some(owner)),
                    )
                })
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
        Self {
            identifier: harvest.declare(DeclarationKind::Type, &declaration.name),
            fields: declaration
                .fields
                .iter()
                .map(|field| CoreField::from_field(field, &declaration.name, harvest))
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
        owner: &Name,
        harvest: &mut NameHarvest<'_>,
    ) -> Self {
        let qualified = field.name.qualified_under(Some(owner));
        // A field whose current name equals its reference's derivation carries
        // no name data: the identifier still mints from the qualified current
        // name, but no table row is stored, and the name is derived on demand.
        let identifier = if field.name == field.reference.derived_field_name() {
            harvest.associate(DeclarationKind::Field, &qualified)
        } else {
            harvest.declare(DeclarationKind::Field, &qualified)
        };
        Self {
            identifier,
            reference: CoreReference::from_reference(&field.reference, harvest),
        }
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<FieldDeclaration, SchemaError> {
        let reference = self.reference.project(names)?;
        let name = match names.name_of(&self.identifier) {
            Some(stored) => Name::new(stored.local_part()),
            None => reference.derived_field_name(),
        };
        Ok(FieldDeclaration { name, reference })
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
        Self {
            identifier: harvest.declare(DeclarationKind::Type, &declaration.name),
            variants: declaration
                .variants
                .iter()
                .map(|variant| CoreVariant::from_variant(variant, &declaration.name, harvest))
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
    pub(crate) stream_relation: Option<CoreStreamRelation>,
}

impl CoreVariant {
    pub(crate) fn from_variant(
        variant: &EnumVariant,
        owner: &Name,
        harvest: &mut NameHarvest<'_>,
    ) -> Self {
        Self {
            identifier: harvest.declare(
                DeclarationKind::Variant,
                &variant.name.qualified_under(Some(owner)),
            ),
            payload: variant
                .payload
                .as_ref()
                .map(|payload| CoreReference::from_reference(payload, harvest)),
            stream_relation: variant
                .stream_relation
                .as_ref()
                .map(|relation| CoreStreamRelation::from_relation(relation, harvest)),
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
            stream_relation: self
                .stream_relation
                .as_ref()
                .map(|relation| relation.project(names))
                .transpose()?,
        })
    }
}

/// A variant's stream relation in the substrate, mirroring [`StreamRelation`]
/// with the stream name identifier-addressed.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum CoreStreamRelation {
    Opens(NominalIdentifier),
    Belongs(NominalIdentifier),
}

impl CoreStreamRelation {
    pub(crate) fn from_relation(relation: &StreamRelation, harvest: &mut NameHarvest<'_>) -> Self {
        match relation {
            StreamRelation::Opens(name) => {
                Self::Opens(harvest.declare(DeclarationKind::Type, name))
            }
            StreamRelation::Belongs(name) => {
                Self::Belongs(harvest.declare(DeclarationKind::Type, name))
            }
        }
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<StreamRelation, SchemaError> {
        Ok(match self {
            Self::Opens(identifier) => {
                StreamRelation::Opens(names.projected_name(identifier)?.clone())
            }
            Self::Belongs(identifier) => {
                StreamRelation::Belongs(names.projected_name(identifier)?.clone())
            }
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

/// A stream declaration in the substrate, mirroring [`StreamDeclaration`].
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct CoreStream {
    pub(crate) identifier: NominalIdentifier,
    pub(crate) token: CoreReference,
    pub(crate) opened: CoreReference,
    pub(crate) event: CoreReference,
    pub(crate) close: CoreReference,
}

impl CoreStream {
    pub(crate) fn from_stream(stream: &StreamDeclaration, harvest: &mut NameHarvest<'_>) -> Self {
        Self {
            identifier: harvest.declare(DeclarationKind::Type, &stream.name),
            token: CoreReference::from_reference(&stream.token, harvest),
            opened: CoreReference::from_reference(&stream.opened, harvest),
            event: CoreReference::from_reference(&stream.event, harvest),
            close: CoreReference::from_reference(&stream.close, harvest),
        }
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<StreamDeclaration, SchemaError> {
        Ok(StreamDeclaration::new(
            names.projected_name(&self.identifier)?.clone(),
            self.token.project(names)?,
            self.opened.project(names)?,
            self.event.project(names)?,
            self.close.project(names)?,
        ))
    }
}

/// A family declaration in the substrate, mirroring [`FamilyDeclaration`]. The
/// family and its record reference are identifier-addressed; the table name is
/// a storage coordinate — explicitly not a schema symbol — and stays data.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct CoreFamily {
    pub(crate) identifier: NominalIdentifier,
    pub(crate) record: NominalIdentifier,
    pub(crate) table: TableName,
    pub(crate) key: FamilyKey,
}

impl CoreFamily {
    pub(crate) fn from_family(family: &FamilyDeclaration, harvest: &mut NameHarvest<'_>) -> Self {
        Self {
            identifier: harvest.declare(DeclarationKind::Type, &family.name),
            record: harvest.declare(DeclarationKind::Type, &family.record),
            table: family.table.clone(),
            key: family.key,
        }
    }

    pub(crate) fn project(&self, names: &NameTable) -> Result<FamilyDeclaration, SchemaError> {
        Ok(FamilyDeclaration::new(
            names.projected_name(&self.identifier)?.clone(),
            names.projected_name(&self.record)?.clone(),
            self.table.clone(),
            self.key,
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

/// A type at a reference position in the substrate, mirroring the current
/// per-name [`TypeReference`] variants one-for-one (the per-kind collapse is
/// separate, tracked work). Scalar leaves and the fixed-bytes width are
/// structure; `Plain` and local application heads are identifier-addressed.
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
    FixedBytes(u64),
    Plain(NominalIdentifier),
    Vector(#[rkyv(omit_bounds)] Box<CoreReference>),
    Map(
        #[rkyv(omit_bounds)] Box<CoreReference>,
        #[rkyv(omit_bounds)] Box<CoreReference>,
    ),
    Optional(#[rkyv(omit_bounds)] Box<CoreReference>),
    ScopeOf(#[rkyv(omit_bounds)] Box<CoreReference>),
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
            TypeReference::FixedBytes(width) => Self::FixedBytes(*width),
            TypeReference::Plain(name) => Self::Plain(harvest.declare(DeclarationKind::Type, name)),
            TypeReference::Vector(inner) => {
                Self::Vector(Box::new(Self::from_reference(inner, harvest)))
            }
            TypeReference::Map(key, value) => Self::Map(
                Box::new(Self::from_reference(key, harvest)),
                Box::new(Self::from_reference(value, harvest)),
            ),
            TypeReference::Optional(inner) => {
                Self::Optional(Box::new(Self::from_reference(inner, harvest)))
            }
            TypeReference::ScopeOf(inner) => {
                Self::ScopeOf(Box::new(Self::from_reference(inner, harvest)))
            }
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
            Self::FixedBytes(width) => TypeReference::FixedBytes(*width),
            Self::Plain(identifier) => {
                TypeReference::Plain(names.projected_name(identifier)?.clone())
            }
            Self::Vector(inner) => TypeReference::Vector(Box::new(inner.project(names)?)),
            Self::Map(key, value) => TypeReference::Map(
                Box::new(key.project(names)?),
                Box::new(value.project(names)?),
            ),
            Self::Optional(inner) => TypeReference::Optional(Box::new(inner.project(names)?)),
            Self::ScopeOf(inner) => TypeReference::ScopeOf(Box::new(inner.project(names)?)),
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
