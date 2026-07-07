use crate::{
    Declaration, EnumDeclaration, EnumVariant, FieldDeclaration, Name, SchemaError, SchemaIdentity,
    StructDeclaration, TrueSchema, TypeDeclaration, TypeReference,
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
    pub schema_identity: SchemaIdentity,
    pub migration_spec: Option<MigrationSpec>,
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
        }
    }

    fn apply_add_field(
        schema: TrueSchema,
        edit: AddField,
    ) -> Result<(TrueSchema, SchemaEditReceipt), SchemaError> {
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
        let receipt = schema.edit_receipt(Some(MigrationSpec {
            target_type: edit.target_type,
            field_name: edit.field_name,
            previous_type,
            next_type: field_type,
            migration,
        }));
        Ok((schema, receipt))
    }

    fn apply_change_field_type(
        schema: TrueSchema,
        edit: ChangeFieldType,
    ) -> Result<(TrueSchema, SchemaEditReceipt), SchemaError> {
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
        let receipt = schema.edit_receipt(Some(MigrationSpec {
            target_type: edit.target_type,
            field_name: edit.field_name,
            previous_type,
            next_type,
            migration: edit.migration,
        }));
        Ok((schema, receipt))
    }

    fn apply_add_variant(
        schema: TrueSchema,
        edit: AddVariant,
    ) -> Result<(TrueSchema, SchemaEditReceipt), SchemaError> {
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
        let receipt = schema.edit_receipt(None);
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
}

impl SchemaEditor {
    fn new(schema: TrueSchema) -> Self {
        Self {
            identity: schema.identity().clone(),
            imports: schema.imports().to_vec(),
            resolved_imports: schema.resolved_imports().to_vec(),
            input: schema.input().clone(),
            output: schema.output().clone(),
            namespace: schema.namespace().to_vec(),
            streams: schema.streams().to_vec(),
            families: schema.families().to_vec(),
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
        Ok((self.into_true_schema(), previous_type))
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
        Ok(self.into_true_schema())
    }

    fn into_true_schema(self) -> TrueSchema {
        TrueSchema::new(
            self.identity,
            self.imports,
            self.resolved_imports,
            self.input,
            self.output,
            self.namespace,
            self.streams,
            self.families,
            Vec::new(),
        )
    }
}

impl TrueSchema {
    fn edit_receipt(&self, migration_spec: Option<MigrationSpec>) -> SchemaEditReceipt {
        SchemaEditReceipt {
            schema_identity: self.identity().clone(),
            migration_spec,
        }
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

impl TrueSchema {
    /// Replace this schema's identity with a new version stamp without
    /// changing its declarations. `UpgradeObject::apply` calls this once
    /// at the end of applying every edit, so the stored schema records
    /// the new version.
    pub fn with_identity(self, identity: SchemaIdentity) -> Self {
        let imports = self.imports().to_vec();
        let resolved_imports = self.resolved_imports().to_vec();
        let input = self.input().clone();
        let output = self.output().clone();
        let namespace = self.namespace().to_vec();
        let streams = self.streams().to_vec();
        let families = self.families().to_vec();
        let relations = self.relations().to_vec();
        Self::new(
            identity,
            imports,
            resolved_imports,
            input,
            output,
            namespace,
            streams,
            families,
            relations,
        )
    }
}
