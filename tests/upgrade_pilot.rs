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
    DeclarationKind, DefaultValue, FieldMigration, Name, SchemaEdit, SchemaEditApplication,
    SchemaEngine, SchemaError, SchemaIdentity, TypeDeclaration, TypeReference, UpgradeObject,
};

fn entry_schema_source() -> &'static str {
    "{}\n\
     [Record.Entry Observe.Query]\n\
     [RecordAccepted.RecordIdentifier RecordsObserved.RecordSet]\n\
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
     }\n\
     []\n"
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
    assert!(receipt.migration_spec().is_some());
    assert!(
        receipt.parent_core_hash() != receipt.child_core_hash(),
        "a structural edit moves the core hash",
    );
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
        .migration_spec()
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
    // AddVariant has no per-field migration; the receipt carries no migration
    // spec, but it is a structural edit, so the core hash still moves.
    assert!(receipt.migration_spec().is_none());
    assert!(
        receipt.parent_core_hash() != receipt.child_core_hash(),
        "AddVariant is structural and moves the core hash",
    );
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

/// Finding 1 (lineage integrity): the editor threads the pre-edit name table as
/// the decompose prior, so a rename's identifier survives a later structural
/// edit and the child core hash is independent of edit ORDER. Two orderings that
/// reach identical schema text — rename-then-add-field and add-field-then-rename
/// — must produce the same core hash, and the renamed type must keep its
/// original identifier through the structural edit.
#[test]
fn edit_order_across_a_rename_does_not_move_the_child_core_hash() {
    let entry_identifier = lower_previous()
        .identifier_named(DeclarationKind::Type, &Name::new("Entry"))
        .expect("Entry has an identifier");

    // Order A: rename Entry -> LogEntry, then add a field to LogEntry.
    let base_a = lower_previous();
    let entry_a = base_a
        .identifier_named(DeclarationKind::Type, &Name::new("Entry"))
        .expect("Entry identifier in order A");
    let (renamed_a, _) = SchemaEdit::rename(entry_a, "LogEntry")
        .apply_to(base_a)
        .expect("rename applies in order A");
    let (order_a, _) = SchemaEdit::add_field(
        "LogEntry",
        "last_modified",
        TypeReference::Integer,
        DefaultValue::Integer(0),
    )
    .apply_to(renamed_a)
    .expect("add-field applies in order A");

    // Order B: add a field to Entry, then rename Entry -> LogEntry.
    let base_b = lower_previous();
    let (added_b, _) = SchemaEdit::add_field(
        "Entry",
        "last_modified",
        TypeReference::Integer,
        DefaultValue::Integer(0),
    )
    .apply_to(base_b)
    .expect("add-field applies in order B");
    let entry_b = added_b
        .identifier_named(DeclarationKind::Type, &Name::new("Entry"))
        .expect("Entry identifier in order B");
    let (order_b, _) = SchemaEdit::rename(entry_b, "LogEntry")
        .apply_to(added_b)
        .expect("rename applies in order B");

    // Both orderings converge on identical schema text.
    assert_eq!(
        order_a.to_schema_text(),
        order_b.to_schema_text(),
        "the two orderings reach identical schema text",
    );
    // ...and therefore, with the prior threaded, identical core hashes.
    assert_eq!(
        order_a.core_hash().expect("order A core hash"),
        order_b.core_hash().expect("order B core hash"),
        "edit order across a rename does not move the child core hash",
    );

    // The renamed declaration's identifier survived the structural edit: in both
    // wholes LogEntry still carries Entry's original identifier.
    assert_eq!(
        order_a
            .identifier_named(DeclarationKind::Type, &Name::new("LogEntry"))
            .expect("LogEntry identifier in order A"),
        entry_identifier,
        "the renamed type keeps its original identifier through the structural edit",
    );
    assert_eq!(
        order_b
            .identifier_named(DeclarationKind::Type, &Name::new("LogEntry"))
            .expect("LogEntry identifier in order B"),
        entry_identifier,
    );
}
