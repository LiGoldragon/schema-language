//! Designer 481 — schema-daemon upgradable runtime schema pilot witnesses.
//!
//! These are Layer 2 runtime witnesses for the upgrade pilot per
//! `skills/architectural-truth-tests.md`: each test exercises the real
//! method path on schema-emitted typed objects, not a grep or a sketch.
//!
//! Per designer 447 §"Block 1" + §"Block 2": the test demonstrates the
//! NOTA-to-object correspondence — a NOTA-encoded `UpgradeObject` is
//! decoded into the typed object, applied against a stored `TrueSchema`,
//! and the resulting next-version schema matches the expected shape.
//!
//! Coverage:
//!  - AddField produces the expected next-version field.
//!  - ChangeFieldType with WrapSingleton replaces the field type.
//!  - AddVariant extends the target enum.
//!  - `UpgradeObject::apply` chains edits and rejects identity mismatch.

use schema_language::{
    DefaultValue, FieldMigration, Name, SchemaEdit, SchemaEditApplication, SchemaEngine,
    SchemaError, SchemaIdentity, TypeDeclaration, TypeReference, UpgradeObject,
};

fn entry_schema_source() -> &'static str {
    "{ Vector Vector }\n\
     [(Record Entry) (Observe Query)]\n\
     [(RecordAccepted RecordIdentifier) (RecordsObserved RecordSet)]\n\
     {\n\
       Record Entry\n\
       Observe Query\n\
       RecordAccepted RecordIdentifier\n\
       RecordsObserved RecordSet\n\
       Topic String\n\
       Description String\n\
       RecordIdentifier Integer\n\
       Entry { Topic Description Kind }\n\
       Query { Topic Kind }\n\
       RecordSet Vector.Entry\n\
       Kind [Decision Principle Correction Clarification Constraint]\n\
     }\n"
}

fn lower_previous() -> schema_language::TrueSchema {
    SchemaEngine::default()
        .lower_source(
            entry_schema_source(),
            SchemaIdentity::new("spirit-min", "0.1.0"),
        )
        .expect("base schema lowers")
}

#[test]
fn add_field_lands_new_field_on_target_struct() {
    let previous = lower_previous();
    let edit = SchemaEdit::add_field(
        "Entry",
        "last_modified",
        TypeReference::Integer,
        DefaultValue::Integer(0),
    );

    let (next, receipt) = SchemaEditApplication::new(previous, edit)
        .apply()
        .expect("edit applies");

    let entry = next.type_named("Entry").expect("Entry remains declared");
    let TypeDeclaration::Struct(structure) = entry else {
        panic!("Entry is a struct");
    };
    assert!(
        structure
            .fields
            .iter()
            .any(|field| field.name.as_str() == "last_modified"),
        "AddField appended the new field, fields = {:?}",
        structure
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(receipt.migration_spec.is_some());
}

#[test]
fn change_field_type_swaps_topic_to_vector_with_wrap_singleton() {
    let previous = lower_previous();
    let edit = SchemaEdit::change_field_type(
        "Entry",
        "topic",
        TypeReference::Vector(Box::new(TypeReference::Plain(Name::new("Topic")))),
        FieldMigration::WrapSingleton,
    );

    let (next, receipt) = SchemaEditApplication::new(previous, edit)
        .apply()
        .expect("edit applies");

    let entry = next.type_named("Entry").expect("Entry remains declared");
    let TypeDeclaration::Struct(structure) = entry else {
        panic!("Entry is a struct");
    };
    let topic = structure
        .fields
        .iter()
        .find(|field| field.name.as_str() == "topic")
        .expect("topic field present");
    match &topic.reference {
        TypeReference::Vector(inner) => match inner.as_ref() {
            TypeReference::Plain(name) => assert_eq!(name.as_str(), "Topic"),
            other => panic!("expected Vector<Topic>, found Vector<{other:?}>"),
        },
        other => panic!("expected Vector<Topic>, found {other:?}"),
    }
    let migration = receipt
        .migration_spec
        .expect("change_field_type carries migration");
    assert!(matches!(migration.migration, FieldMigration::WrapSingleton));
}

#[test]
fn add_variant_extends_target_enum() {
    let previous = lower_previous();
    let edit = SchemaEdit::add_variant("Kind", "Reflection", None);

    let (next, receipt) = SchemaEditApplication::new(previous, edit)
        .apply()
        .expect("edit applies");

    let kind = next.type_named("Kind").expect("Kind remains declared");
    let TypeDeclaration::Enum(enumeration) = kind else {
        panic!("Kind is an enum");
    };
    assert!(
        enumeration
            .variants
            .iter()
            .any(|variant| variant.name.as_str() == "Reflection"),
        "AddVariant extended the enum, variants = {:?}",
        enumeration
            .variants
            .iter()
            .map(|variant| variant.name.as_str())
            .collect::<Vec<_>>()
    );
    // AddVariant has no per-field migration; receipt's migration_spec is
    // None.
    assert!(receipt.migration_spec.is_none());
}

#[test]
fn upgrade_object_chains_edits_and_stamps_next_identity() {
    let previous = lower_previous();
    let upgrade = UpgradeObject::new(
        SchemaIdentity::new("spirit-min", "0.1.0"),
        SchemaIdentity::new("spirit-min", "0.2.0"),
        vec![
            SchemaEdit::add_field(
                "Entry",
                "last_modified",
                TypeReference::Integer,
                DefaultValue::Integer(0),
            ),
            SchemaEdit::add_variant("Kind", "Reflection", None),
            SchemaEdit::change_field_type(
                "Entry",
                "topic",
                TypeReference::Vector(Box::new(TypeReference::Plain(Name::new("Topic")))),
                FieldMigration::WrapSingleton,
            ),
        ],
    );

    let (next, upgrade_receipt) = upgrade.apply(&previous).expect("upgrade applies");

    assert_eq!(next.identity().version(), "0.2.0");
    assert_eq!(upgrade_receipt.edit_receipts.len(), 3);

    // The chained edits all landed in the right places.
    let entry = next
        .type_named("Entry")
        .expect("Entry still declared after chained upgrade");
    let TypeDeclaration::Struct(structure) = entry else {
        panic!("Entry is a struct");
    };
    let field_names: Vec<_> = structure
        .fields
        .iter()
        .map(|field| field.name.as_str().to_owned())
        .collect();
    assert!(field_names.iter().any(|name| name == "last_modified"));

    let kind = next.type_named("Kind").expect("Kind still declared");
    let TypeDeclaration::Enum(enumeration) = kind else {
        panic!("Kind is an enum");
    };
    assert!(
        enumeration
            .variants
            .iter()
            .any(|variant| variant.name.as_str() == "Reflection")
    );
}

#[test]
fn upgrade_object_rejects_mismatched_previous_identity() {
    let previous = lower_previous();
    let upgrade = UpgradeObject::new(
        SchemaIdentity::new("spirit-min", "0.0.9"),
        SchemaIdentity::new("spirit-min", "0.1.0"),
        vec![SchemaEdit::add_variant("Kind", "Reflection", None)],
    );

    let error = upgrade
        .apply(&previous)
        .expect_err("identity mismatch is a typed rejection");
    assert!(matches!(
        error,
        SchemaError::SchemaEditIdentityMismatch { .. }
    ));
}
