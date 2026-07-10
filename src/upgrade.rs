use crate::{
    ContentHash, Declaration, EnumDeclaration, EnumVariant, FieldDeclaration, Name,
    NominalIdentifier, SchemaError, SchemaIdentity, StructDeclaration, TrueSchema, TypeDeclaration,
    TypeReference,
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
pub enum SchemaEdit {
    AddField(AddField),
    ChangeFieldType(ChangeFieldType),
    AddVariant(AddVariant),
    Rename(Rename),
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
pub struct AddField {
    pub target_type: Name,
    pub field_name: Name,
    pub field_type: TypeReference,
    pub default_value: DefaultValue,
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
pub struct ChangeFieldType {
    pub target_type: Name,
    pub field_name: Name,
    pub new_type: TypeReference,
    pub migration: FieldMigration,
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
pub struct AddVariant {
    pub target_type: Name,
    pub variant_name: Name,
    pub payload: Option<TypeReference>,
}

/// A name-only edit: rebind one declaration's nominal identifier to a new human
/// name. It touches ONLY the [`crate::NameTable`] and emits zero migration code
/// — `AddVariant`'s no-migration-spec shape is the precedent — so it must not
/// move the core hash. The declaration is addressed by its stable identifier, so
/// a rename applies uniformly to a top-level type, a member (field or variant),
/// or an imported declaration, none of which the current name can disambiguate
/// on its own.
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
pub struct Rename {
    pub identifier: NominalIdentifier,
    pub new_name: Name,
}

/// The `NameTable` change a [`Rename`] records on the lineage chain: the
/// declaration's identifier and the name it moved from and to. The core hash is
/// unchanged across this delta by construction — only the table moved — so the
/// receipt edge it rides on has an equal parent and child core hash.
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
pub struct NameTableDelta {
    pub identifier: NominalIdentifier,
    pub previous_name: Name,
    pub new_name: Name,
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
pub enum FieldMigration {
    WrapSingleton,
    SetDefault(DefaultValue),
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
pub enum DefaultValue {
    String(String),
    Integer(u64),
    Boolean(bool),
    Unit,
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
pub struct MigrationSpec {
    pub target_type: Name,
    pub field_name: Name,
    pub previous_type: Option<TypeReference>,
    pub next_type: TypeReference,
    pub migration: FieldMigration,
}

/// What one accepted edit did to the substrate, in the register the lineage
/// chain composes over. A `Structural` edit moved the core hash and carries the
/// field migration the historical-to-current emission needs — `Some` for
/// `AddField`/`ChangeFieldType`, `None` for `AddVariant`, which changes bytes
/// but needs no per-field migration. A `Rename` moved only the `NameTable`,
/// carries the name delta, and emits zero migration.
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
pub enum EditEffect {
    // The migration is boxed: a `MigrationSpec` dwarfs a `NameTableDelta`, so
    // an unboxed union would pad every rename edge to the structural size.
    Structural(Option<Box<MigrationSpec>>),
    Rename(NameTableDelta),
}

impl EditEffect {
    /// The field migration this edit contributes to a historical-to-current
    /// conversion, or `None` when it emits no migration code. Both a rename and
    /// an `AddVariant` contribute nothing; a rename because it is name-only, an
    /// `AddVariant` because a new variant needs no per-field migration.
    pub fn migration_spec(&self) -> Option<&MigrationSpec> {
        match self {
            Self::Structural(migration) => migration.as_deref(),
            Self::Rename(_) => None,
        }
    }
}

/// One receipt edge in the lineage graph: the (parent core hash -> child core
/// hash) pair the edit produced, plus what it did. A structural edit moves the
/// core hash, so its parent and child hashes differ; a rename leaves the core
/// hash fixed, so they are equal. This is the typed edge the schema daemon will
/// later persist and walk — receipt storage lives here as data, not as daemon
/// machinery.
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
pub struct SchemaEditReceipt {
    pub parent_core_hash: ContentHash,
    pub child_core_hash: ContentHash,
    pub effect: EditEffect,
}

impl SchemaEditReceipt {
    /// A receipt for a structural edit: the core hash moved from `parent` to
    /// `child`, carrying the field migration the edit emits (`None` for
    /// `AddVariant`).
    fn structural(
        parent_core_hash: ContentHash,
        child_core_hash: ContentHash,
        migration: Option<MigrationSpec>,
    ) -> Self {
        Self {
            parent_core_hash,
            child_core_hash,
            effect: EditEffect::Structural(migration.map(Box::new)),
        }
    }

    pub fn parent_core_hash(&self) -> &ContentHash {
        &self.parent_core_hash
    }

    pub fn child_core_hash(&self) -> &ContentHash {
        &self.child_core_hash
    }

    pub fn effect(&self) -> &EditEffect {
        &self.effect
    }

    /// The field migration this edit contributes to a conversion, or `None`.
    pub fn migration_spec(&self) -> Option<&MigrationSpec> {
        self.effect.migration_spec()
    }

    /// Whether this edit left the core hash fixed — true exactly for a rename,
    /// whose parent and child core hashes are equal by construction.
    pub fn is_core_preserving(&self) -> bool {
        self.parent_core_hash == self.child_core_hash
    }
}

pub struct SchemaEditApplication {
    schema: TrueSchema,
    edit: SchemaEdit,
}

impl SchemaEdit {
    pub fn add_field(
        target_type: impl Into<String>,
        field_name: impl Into<String>,
        field_type: TypeReference,
        default_value: DefaultValue,
    ) -> Self {
        Self::AddField(AddField {
            target_type: Name::new(target_type),
            field_name: Name::new(field_name),
            field_type,
            default_value,
        })
    }

    pub fn change_field_type(
        target_type: impl Into<String>,
        field_name: impl Into<String>,
        new_type: TypeReference,
        migration: FieldMigration,
    ) -> Self {
        Self::ChangeFieldType(ChangeFieldType {
            target_type: Name::new(target_type),
            field_name: Name::new(field_name),
            new_type,
            migration,
        })
    }

    pub fn add_variant(
        target_type: impl Into<String>,
        variant_name: impl Into<String>,
        payload: Option<TypeReference>,
    ) -> Self {
        Self::AddVariant(AddVariant {
            target_type: Name::new(target_type),
            variant_name: Name::new(variant_name),
            payload,
        })
    }

    /// Rename the declaration carrying `identifier` to `new_name`. The
    /// identifier is stable across the rename, so this addresses a type, a
    /// member, or an imported declaration uniformly and never moves the core
    /// hash.
    pub fn rename(identifier: NominalIdentifier, new_name: impl Into<String>) -> Self {
        Self::Rename(Rename {
            identifier,
            new_name: Name::new(new_name),
        })
    }

    pub fn apply_to(
        self,
        schema: TrueSchema,
    ) -> Result<(TrueSchema, SchemaEditReceipt), SchemaError> {
        SchemaEditApplication::new(schema, self).apply()
    }
}

impl SchemaEditApplication {
    pub fn new(schema: TrueSchema, edit: SchemaEdit) -> Self {
        Self { schema, edit }
    }

    pub fn apply(self) -> Result<(TrueSchema, SchemaEditReceipt), SchemaError> {
        let Self { schema, edit } = self;
        match edit {
            SchemaEdit::AddField(operation) => Self::apply_add_field(schema, operation),
            SchemaEdit::ChangeFieldType(operation) => {
                Self::apply_change_field_type(schema, operation)
            }
            SchemaEdit::AddVariant(operation) => Self::apply_add_variant(schema, operation),
            SchemaEdit::Rename(operation) => Self::apply_rename(schema, operation),
        }
    }

    fn apply_add_field(
        schema: TrueSchema,
        edit: AddField,
    ) -> Result<(TrueSchema, SchemaEditReceipt), SchemaError> {
        let parent_core_hash = schema.core_hash()?;
        let field_type = edit.field_type.clone();
        let migration = FieldMigration::SetDefault(edit.default_value);
        let (schema, previous_type) =
            SchemaEditor::new(schema).update_struct(edit.target_type.clone(), |declaration| {
                if declaration
                    .fields
                    .iter()
                    .any(|field| field.name == edit.field_name)
                {
                    return Err(SchemaError::SchemaEditDuplicateField {
                        type_name: edit.target_type.to_string(),
                        field_name: edit.field_name.to_string(),
                    });
                }
                let mut fields = declaration.fields.entries().to_vec();
                fields.push(FieldDeclaration {
                    name: edit.field_name.clone(),
                    reference: field_type.clone(),
                });
                Ok((
                    StructDeclaration::new(declaration.name.clone(), fields),
                    None,
                ))
            })?;
        let receipt = SchemaEditReceipt::structural(
            parent_core_hash,
            schema.core_hash()?,
            Some(MigrationSpec {
                target_type: edit.target_type,
                field_name: edit.field_name,
                previous_type,
                next_type: field_type,
                migration,
            }),
        );
        Ok((schema, receipt))
    }

    fn apply_change_field_type(
        schema: TrueSchema,
        edit: ChangeFieldType,
    ) -> Result<(TrueSchema, SchemaEditReceipt), SchemaError> {
        let parent_core_hash = schema.core_hash()?;
        let next_type = edit.new_type.clone();
        let (schema, previous_type) =
            SchemaEditor::new(schema).update_struct(edit.target_type.clone(), |declaration| {
                let mut fields = declaration.fields.entries().to_vec();
                let Some(field) = fields
                    .iter_mut()
                    .find(|field| field.name == edit.field_name)
                else {
                    return Err(SchemaError::SchemaEditFieldNotFound {
                        type_name: edit.target_type.to_string(),
                        field_name: edit.field_name.to_string(),
                    });
                };
                let previous_type = field.reference.clone();
                field.reference = next_type.clone();
                Ok((
                    StructDeclaration::new(declaration.name.clone(), fields),
                    Some(previous_type),
                ))
            })?;
        let receipt = SchemaEditReceipt::structural(
            parent_core_hash,
            schema.core_hash()?,
            Some(MigrationSpec {
                target_type: edit.target_type,
                field_name: edit.field_name,
                previous_type,
                next_type,
                migration: edit.migration,
            }),
        );
        Ok((schema, receipt))
    }

    fn apply_add_variant(
        schema: TrueSchema,
        edit: AddVariant,
    ) -> Result<(TrueSchema, SchemaEditReceipt), SchemaError> {
        let parent_core_hash = schema.core_hash()?;
        let schema =
            SchemaEditor::new(schema).update_enum(edit.target_type.clone(), |declaration| {
                if declaration
                    .variants
                    .iter()
                    .any(|variant| variant.name == edit.variant_name)
                {
                    return Err(SchemaError::SchemaEditDuplicateVariant {
                        type_name: edit.target_type.to_string(),
                        variant_name: edit.variant_name.to_string(),
                    });
                }
                let mut variants = declaration.variants.clone();
                variants.push(EnumVariant::new(
                    edit.variant_name.clone(),
                    edit.payload.clone(),
                ));
                Ok(EnumDeclaration::new(declaration.name.clone(), variants))
            })?;
        let receipt = SchemaEditReceipt::structural(parent_core_hash, schema.core_hash()?, None);
        Ok((schema, receipt))
    }

    fn apply_rename(
        schema: TrueSchema,
        edit: Rename,
    ) -> Result<(TrueSchema, SchemaEditReceipt), SchemaError> {
        let parent_core_hash = schema.core_hash()?;
        // The previous name must be read before the table moves; a declaration
        // addressed by a live identifier always has a row, and `rename` rejects
        // an absent identifier, so a successful rename guarantees it was `Some`.
        let previous_name = schema
            .names()
            .name_of(&edit.identifier)
            .cloned()
            .ok_or_else(|| SchemaError::NameTableIdentifierAbsent {
                identifier: edit.identifier.to_hex(),
            })?;
        let mut schema = schema;
        schema.rename(&edit.identifier, edit.new_name.clone())?;
        // A rename touches only the NameTable, so the core hash is fixed: the
        // receipt edge's parent and child core hashes are equal by construction.
        let child_core_hash = schema.core_hash()?;
        let receipt = SchemaEditReceipt {
            parent_core_hash,
            child_core_hash,
            effect: EditEffect::Rename(NameTableDelta {
                identifier: edit.identifier,
                previous_name,
                new_name: edit.new_name,
            }),
        };
        Ok((schema, receipt))
    }
}

struct SchemaEditor {
    identity: SchemaIdentity,
    imports: Vec<crate::ImportDeclaration>,
    resolved_imports: Vec<crate::ResolvedImport>,
    input: crate::Root,
    output: crate::Root,
    namespace: Vec<Declaration>,
    streams: Vec<crate::StreamDeclaration>,
    families: Vec<crate::FamilyDeclaration>,
    // The pre-edit name table, carried as the re-association prior when the
    // edited tree is decomposed back into the split model. Without it the
    // rebuild re-mints every identifier from the CURRENT name against an empty
    // prior, so a renamed declaration is re-minted from its new name and the
    // child core hash becomes a function of edit order — two orderings reaching
    // identical text would produce different core hashes. Threading the prior
    // preserves each unchanged and renamed declaration's identifier, so only a
    // genuinely new structural addition mints fresh.
    prior: crate::NameTable,
}

impl SchemaEditor {
    fn new(schema: TrueSchema) -> Self {
        let prior = schema.names().clone();
        let tree = schema.tree();
        Self {
            identity: tree.identity().clone(),
            imports: tree.imports().to_vec(),
            resolved_imports: tree.resolved_imports().to_vec(),
            input: tree.input().clone(),
            output: tree.output().clone(),
            namespace: tree.namespace().to_vec(),
            streams: tree.streams().to_vec(),
            families: tree.families().to_vec(),
            prior,
        }
    }

    fn update_struct(
        mut self,
        target_type: Name,
        update: impl FnOnce(
            &StructDeclaration,
        ) -> Result<(StructDeclaration, Option<TypeReference>), SchemaError>,
    ) -> Result<(TrueSchema, Option<TypeReference>), SchemaError> {
        let Some(index) = self
            .namespace
            .iter()
            .position(|declaration| declaration.name() == &target_type)
        else {
            return Err(SchemaError::SchemaEditTargetNotFound {
                type_name: target_type.to_string(),
            });
        };
        let visibility = self.namespace[index].visibility();
        let TypeDeclaration::Struct(declaration) = self.namespace[index].value() else {
            return Err(SchemaError::SchemaEditExpectedStruct {
                type_name: target_type.to_string(),
            });
        };
        let (declaration, previous_type) = update(declaration)?;
        self.namespace[index] = match visibility {
            crate::Visibility::Public => Declaration::public(TypeDeclaration::Struct(declaration)),
            crate::Visibility::Private => {
                Declaration::private(TypeDeclaration::Struct(declaration))
            }
        };
        Ok((self.into_true_schema()?, previous_type))
    }

    fn update_enum(
        mut self,
        target_type: Name,
        update: impl FnOnce(&EnumDeclaration) -> Result<EnumDeclaration, SchemaError>,
    ) -> Result<TrueSchema, SchemaError> {
        let Some(index) = self
            .namespace
            .iter()
            .position(|declaration| declaration.name() == &target_type)
        else {
            return Err(SchemaError::SchemaEditTargetNotFound {
                type_name: target_type.to_string(),
            });
        };
        let visibility = self.namespace[index].visibility();
        let TypeDeclaration::Enum(declaration) = self.namespace[index].value() else {
            return Err(SchemaError::SchemaEditExpectedEnum {
                type_name: target_type.to_string(),
            });
        };
        let declaration = update(declaration)?;
        self.namespace[index] = match visibility {
            crate::Visibility::Public => Declaration::public(TypeDeclaration::Enum(declaration)),
            crate::Visibility::Private => Declaration::private(TypeDeclaration::Enum(declaration)),
        };
        self.into_true_schema()
    }

    fn into_true_schema(self) -> Result<TrueSchema, SchemaError> {
        let tree = crate::schema::SchemaTree::new(
            self.identity,
            self.imports,
            self.resolved_imports,
            self.input,
            self.output,
            self.namespace,
            self.streams,
            self.families,
            Vec::new(),
        );
        TrueSchema::from_tree(&tree, &self.prior)
    }
}

/// A complete schema upgrade — the durable record the schema daemon stores
/// in SEMA and the schema-rust emitter reads to produce migration
/// code. The wrapper holds the previous/next identity pair (mints the
/// version bump explicitly) and the ordered list of `SchemaEdit`
/// operations.
///
/// Per designer 447 §"Block 1": this is the typed object an
/// `UpgradeSchema(UpgradeObject)` signal payload carries. Applying it to
/// the stored schema returns the new schema + the receipts every
/// operation produced, in the order applied.
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
pub struct UpgradeObject {
    pub previous_identity: SchemaIdentity,
    pub next_identity: SchemaIdentity,
    pub edits: Vec<SchemaEdit>,
}

impl UpgradeObject {
    pub fn new(
        previous_identity: SchemaIdentity,
        next_identity: SchemaIdentity,
        edits: Vec<SchemaEdit>,
    ) -> Self {
        Self {
            previous_identity,
            next_identity,
            edits,
        }
    }

    pub fn previous_identity(&self) -> &SchemaIdentity {
        &self.previous_identity
    }

    pub fn next_identity(&self) -> &SchemaIdentity {
        &self.next_identity
    }

    pub fn edits(&self) -> &[SchemaEdit] {
        &self.edits
    }

    /// Apply every edit in order against `previous`, returning the new
    /// schema stamped with `next_identity` and the receipts every edit
    /// produced.
    ///
    /// Identity mismatch is a typed failure — if `previous.identity()` is
    /// not equal to `self.previous_identity`, the upgrade is rejected
    /// rather than applied against a schema it was not authored against.
    pub fn apply(
        &self,
        previous: &TrueSchema,
    ) -> Result<(TrueSchema, UpgradeReceipt), SchemaError> {
        if previous.identity() != &self.previous_identity {
            return Err(SchemaError::SchemaEditIdentityMismatch {
                expected: format!(
                    "{}@{}",
                    self.previous_identity.component().as_str(),
                    self.previous_identity.version()
                ),
                found: format!(
                    "{}@{}",
                    previous.identity().component().as_str(),
                    previous.identity().version()
                ),
            });
        }
        let mut schema = previous.clone();
        let mut edit_receipts = Vec::with_capacity(self.edits.len());
        for edit in &self.edits {
            let (next, receipt) = SchemaEditApplication::new(schema, edit.clone()).apply()?;
            schema = next;
            edit_receipts.push(receipt);
        }
        let schema = schema.with_identity(self.next_identity.clone());
        let upgrade_receipt = UpgradeReceipt {
            previous_identity: self.previous_identity.clone(),
            next_identity: self.next_identity.clone(),
            edit_receipts,
        };
        Ok((schema, upgrade_receipt))
    }
}

/// The aggregated receipt the schema daemon records when an `UpgradeObject`
/// applies. Carries the identity transition plus each edit's per-edit
/// receipt for later emission and audit.
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
pub struct UpgradeReceipt {
    pub previous_identity: SchemaIdentity,
    pub next_identity: SchemaIdentity,
    pub edit_receipts: Vec<SchemaEditReceipt>,
}

impl UpgradeReceipt {
    pub fn previous_identity(&self) -> &SchemaIdentity {
        &self.previous_identity
    }

    pub fn next_identity(&self) -> &SchemaIdentity {
        &self.next_identity
    }

    pub fn edit_receipts(&self) -> &[SchemaEditReceipt] {
        &self.edit_receipts
    }
}
