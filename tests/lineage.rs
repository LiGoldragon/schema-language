//! Witnesses for the hash/lineage slice of the Core/True split.
//!
//! Each test drives the real method path on lowered `TrueSchema` values, per the
//! "Hashing and lineage" design of record in `ARCHITECTURE.md`:
//!
//! - a rename moves the true/name hash but never the core hash;
//! - a structural edit moves the core hash and records a (parent -> child)
//!   receipt edge, and composing edges along a two-edit chain carries a
//!   two-versions-old value to current;
//! - a `Rename` edit records a `NameTable` delta on the chain, emits zero
//!   migration, and leaves the core hash fixed; and
//! - common-ancestor lookup by core hash is a walk over the stored edges.

use std::path::PathBuf;

use schema_language::{
    DeclarationKind, DefaultValue, EditEffect, ImportResolver, LineageGraph, MacroContext, Name,
    SchemaEdit, SchemaEditApplication, SchemaEngine, SchemaIdentity, TrueSchema, TypeReference,
};

const BASE: &str = "\
{}
[Record.Entry]
[Logged.Receipt]
{
  Topic String
  Detail String
  Kind [Decision Principle Correction]
  Entry { Topic Kind }
  Receipt { Topic Detail }
}
[]
";

fn lower(source: &str) -> TrueSchema {
    SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("lineage-fixture:lib", "0.1.0"))
        .expect("fixture schema lowers")
}

fn type_identifier(schema: &TrueSchema, name: &str) -> schema_language::NominalIdentifier {
    schema
        .identifier_named(DeclarationKind::Type, &Name::new(name))
        .unwrap_or_else(|| panic!("type {name} has an identifier"))
}

#[test]
fn renaming_a_type_moves_the_true_name_hash_but_not_the_core_hash() {
    let mut schema = lower(BASE);
    let core_before = schema.core_hash().expect("core hash");
    let name_before = schema.true_name_hash().expect("true/name hash");

    let entry = type_identifier(&schema, "Entry");
    schema
        .rename(&entry, Name::new("LogEntry"))
        .expect("type rename applies");

    assert_eq!(
        core_before,
        schema.core_hash().expect("core hash"),
        "a type rename must not move the core hash",
    );
    assert_ne!(
        name_before,
        schema.true_name_hash().expect("true/name hash"),
        "a type rename must move the true/name hash",
    );
}

#[test]
fn renaming_a_member_moves_the_true_name_hash_but_not_the_core_hash() {
    let mut schema = lower(BASE);
    let core_before = schema.core_hash().expect("core hash");
    let name_before = schema.true_name_hash().expect("true/name hash");

    // A variant is a member: minted from its owner enum's identifier, so it is
    // addressed through that owner and renamed by identifier.
    let kind = type_identifier(&schema, "Kind");
    let decision = schema
        .names()
        .member_identifier_of(DeclarationKind::Variant, &kind, &Name::new("Decision"))
        .expect("Kind has a Decision variant");
    schema
        .rename(&decision, Name::new("Ruling"))
        .expect("variant rename applies");

    assert_eq!(
        core_before,
        schema.core_hash().expect("core hash"),
        "a member rename must not move the core hash",
    );
    assert_ne!(
        name_before,
        schema.true_name_hash().expect("true/name hash"),
        "a member rename must move the true/name hash",
    );
}

#[test]
fn renaming_an_imported_declaration_moves_the_true_name_hash_but_not_the_core_hash() {
    let resolver = ImportResolver::new().with_dependency(
        "marker-core",
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/marker-core/schema"),
        "0.1.0",
    );
    let consumer_source = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/import-consumer/schema/lib.schema"),
    )
    .expect("read consumer schema");
    let mut schema = SchemaEngine::default()
        .lower_source_with_resolver(
            &consumer_source,
            SchemaIdentity::new("import-consumer", "0.1.0"),
            &mut MacroContext::default(),
            &resolver,
        )
        .expect("consumer schema lowers");

    let core_before = schema.core_hash().expect("core hash");
    let name_before = schema.true_name_hash().expect("true/name hash");

    // An imported declaration is a declaration in the loaded whole: a top-level
    // identifier addressable by its local name.
    let marker = type_identifier(&schema, "DatabaseMarker");
    schema
        .rename(&marker, Name::new("StoreMarker"))
        .expect("imported declaration rename applies");

    assert_eq!(
        core_before,
        schema.core_hash().expect("core hash"),
        "renaming an imported declaration must not move the core hash",
    );
    assert_ne!(
        name_before,
        schema.true_name_hash().expect("true/name hash"),
        "renaming an imported declaration must move the true/name hash",
    );
}

#[test]
fn rename_edit_records_a_name_delta_with_zero_migration_and_a_fixed_core_hash() {
    let schema = lower(BASE);
    let entry = type_identifier(&schema, "Entry");

    let (renamed, receipt) =
        SchemaEditApplication::new(schema, SchemaEdit::rename(entry, "LogEntry"))
            .apply()
            .expect("rename edit applies");

    assert!(
        receipt.is_core_preserving(),
        "a rename leaves the core hash fixed, so parent and child core hashes are equal",
    );
    assert!(
        receipt.migration_spec().is_none(),
        "a rename emits zero migration",
    );
    match receipt.effect() {
        EditEffect::Rename(delta) => {
            assert_eq!(delta.previous_name.as_str(), "Entry");
            assert_eq!(delta.new_name.as_str(), "LogEntry");
        }
        other => panic!("expected a rename effect, found {other:?}"),
    }
    assert!(
        renamed.type_named("LogEntry").is_some(),
        "the rename lands in the projection",
    );
    assert!(
        renamed.type_named("Entry").is_none(),
        "the old name no longer projects",
    );
}

#[test]
fn structural_edits_record_receipt_edges_that_compose_across_two_versions() {
    let version_one = lower(BASE);
    let version_one_hash = version_one.core_hash().expect("v1 core hash");

    let (version_two, first_receipt) = SchemaEditApplication::new(
        version_one,
        SchemaEdit::add_field(
            "Entry",
            "last_modified",
            TypeReference::Integer,
            DefaultValue::Integer(0),
        ),
    )
    .apply()
    .expect("add-field edit applies");
    let version_two_hash = version_two.core_hash().expect("v2 core hash");

    let (version_three, second_receipt) = SchemaEditApplication::new(
        version_two,
        SchemaEdit::add_variant("Kind", "Reflection", None),
    )
    .apply()
    .expect("add-variant edit applies");
    let version_three_hash = version_three.core_hash().expect("v3 core hash");

    // Each structural edit moved the core hash, and its receipt edge keys the
    // exact (parent -> child) pair.
    assert_ne!(version_one_hash, version_two_hash);
    assert_ne!(version_two_hash, version_three_hash);
    assert_eq!(first_receipt.parent_core_hash(), &version_one_hash);
    assert_eq!(first_receipt.child_core_hash(), &version_two_hash);
    assert_eq!(second_receipt.parent_core_hash(), &version_two_hash);
    assert_eq!(second_receipt.child_core_hash(), &version_three_hash);

    let graph = LineageGraph::from_edges([first_receipt, second_receipt]);

    // Composing the receipts along the two-edge path converts a value authored
    // two versions ago to current: the chain is exactly the ordered edges, and
    // its migrations are the ones the edits emitted.
    let chain = graph
        .conversion_chain(&version_one_hash, &version_three_hash)
        .expect("a two-edge conversion path connects v1 to v3");
    assert_eq!(chain.len(), 2, "the conversion composes two receipt edges");
    let migrations: Vec<_> = chain
        .iter()
        .filter_map(|edge| edge.migration_spec())
        .collect();
    assert_eq!(
        migrations.len(),
        1,
        "only the add-field edge carries a migration; add-variant emits none",
    );
    assert_eq!(migrations[0].field_name.as_str(), "last_modified");
}

#[test]
fn common_ancestor_is_a_walk_over_stored_receipt_edges() {
    let root = lower(BASE);
    let root_hash = root.core_hash().expect("root core hash");

    // One branch advances two structural versions: root -> left_one -> left_two.
    let (left_one, edge_a) = SchemaEditApplication::new(
        root.clone(),
        SchemaEdit::add_field(
            "Entry",
            "alpha",
            TypeReference::Integer,
            DefaultValue::Integer(0),
        ),
    )
    .apply()
    .expect("branch edit A applies");
    let (left_two, edge_b) =
        SchemaEditApplication::new(left_one, SchemaEdit::add_variant("Kind", "Extra", None))
            .apply()
            .expect("branch edit B applies");
    let left_two_hash = left_two.core_hash().expect("left_two core hash");

    // A second branch diverges directly from the root: root -> right_one.
    let (right_one, edge_c) =
        SchemaEditApplication::new(root, SchemaEdit::add_variant("Kind", "Divergent", None))
            .apply()
            .expect("branch edit C applies");
    let right_one_hash = right_one.core_hash().expect("right_one core hash");

    let graph = LineageGraph::from_edges([edge_a, edge_b, edge_c]);

    assert_eq!(
        graph.common_ancestor(&left_two_hash, &right_one_hash),
        Some(root_hash),
        "the nearest core hash both versions descend from is the root",
    );
}

/// Finding 6 (edge-set hygiene): the lineage graph keeps no duplicate receipt
/// edges. An edge is fully determined by its (parent core hash, child core hash,
/// effect) triple, so a second identical edge carries no information a walk could
/// use; both `record` and `from_edges` drop it.
#[test]
fn the_lineage_graph_deduplicates_identical_receipt_edges() {
    let version_one = lower(BASE);
    let (_version_two, receipt) = SchemaEditApplication::new(
        version_one,
        SchemaEdit::add_field(
            "Entry",
            "last_modified",
            TypeReference::Integer,
            DefaultValue::Integer(0),
        ),
    )
    .apply()
    .expect("add-field edit applies");

    // `from_edges` collapses the duplicate to a single edge.
    let from_edges = LineageGraph::from_edges([receipt.clone(), receipt.clone()]);
    assert_eq!(
        from_edges.edges().len(),
        1,
        "from_edges discards a duplicate receipt edge",
    );

    // `record` refuses to store a second identical edge.
    let mut recorded = LineageGraph::new();
    recorded.record(receipt.clone());
    recorded.record(receipt);
    assert_eq!(
        recorded.edges().len(),
        1,
        "record discards a duplicate receipt edge",
    );
}
