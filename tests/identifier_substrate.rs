//! Witnesses for the provisional nominal-identifier substrate: deterministic,
//! order-independent minting; nominal distinctness; rename-through-table
//! identifier preservation; and fresh minting on a re-association miss.

use nota::{Document, NotaDecode, NotaEncode};
use schema_language::{DeclarationKind, Name, NameTable, NominalIdentifier};

fn declaration(name: &str) -> (DeclarationKind, Name) {
    (DeclarationKind::Type, Name::new(name))
}

#[test]
fn minting_is_order_independent_in_identifiers_and_table_bytes() {
    let forward = NameTable::build(
        &NameTable::empty(),
        [
            declaration("Meters"),
            declaration("Seconds"),
            declaration("Grams"),
        ],
    );
    let reversed = NameTable::build(
        &NameTable::empty(),
        [
            declaration("Grams"),
            declaration("Seconds"),
            declaration("Meters"),
        ],
    );

    // Each declaration mints the same identifier regardless of source order.
    for name in ["Meters", "Seconds", "Grams"] {
        let handle = Name::new(name);
        assert_eq!(
            forward.identifier_of(DeclarationKind::Type, &handle),
            reversed.identifier_of(DeclarationKind::Type, &handle),
            "identifier for {name} must not depend on order",
        );
    }

    // And the whole table serializes to identical canonical bytes.
    assert_eq!(
        forward.canonical_bytes().expect("forward table serializes"),
        reversed
            .canonical_bytes()
            .expect("reversed table serializes"),
        "canonical NameTable bytes must not depend on declaration order",
    );
}

#[test]
fn equal_structure_distinct_names_mint_distinct_identifiers() {
    let meters = NominalIdentifier::mint(DeclarationKind::Type, "Meters");
    let seconds = NominalIdentifier::mint(DeclarationKind::Type, "Seconds");
    assert_ne!(
        meters, seconds,
        "nominal identity distinguishes Meters from Seconds despite equal structure",
    );
}

#[test]
fn kind_separates_identically_named_declarations() {
    let as_type = NominalIdentifier::mint(DeclarationKind::Type, "Value");
    let as_field = NominalIdentifier::mint(DeclarationKind::Field, "Value");
    assert_ne!(
        as_type, as_field,
        "the kind dimension keeps a type and a field of the same name distinct",
    );
    assert_eq!(as_type.kind(), DeclarationKind::Type);
    assert_eq!(as_field.kind(), DeclarationKind::Field);
}

#[test]
fn rename_through_table_preserves_the_identifier() {
    let mut table = NameTable::build(&NameTable::empty(), [declaration("Meters")]);
    let original = table
        .identifier_of(DeclarationKind::Type, &Name::new("Meters"))
        .expect("Meters is minted");

    table
        .rename(&original, Name::new("Length"))
        .expect("rename of a present identifier succeeds");

    // The identifier is unchanged, reachable by the new current name, and the
    // old name no longer resolves.
    assert_eq!(table.name_of(&original), Some(&Name::new("Length")));
    assert_eq!(
        table.identifier_of(DeclarationKind::Type, &Name::new("Length")),
        Some(original),
        "renamed declaration stays reachable by its current name",
    );
    assert_eq!(
        table.identifier_of(DeclarationKind::Type, &Name::new("Meters")),
        None,
        "the old name no longer resolves after an in-band rename",
    );

    // Reloading a source that now carries the renamed declaration reuses the
    // identifier rather than minting fresh: lineage is preserved.
    let reloaded = NameTable::build(&table, [declaration("Length")]);
    assert_eq!(
        reloaded.identifier_of(DeclarationKind::Type, &Name::new("Length")),
        Some(original),
        "re-association reuses the identifier bound to the current name",
    );
}

#[test]
fn rename_of_absent_identifier_is_a_typed_error() {
    let mut table = NameTable::empty();
    let stray = NominalIdentifier::mint(DeclarationKind::Type, "Unbound");
    assert!(
        table.rename(&stray, Name::new("Whatever")).is_err(),
        "renaming an identifier the table does not hold is an error",
    );
}

#[test]
fn build_dedups_identical_duplicate_rows() {
    // The same declaration supplied twice must not leave two identical rows in
    // the table; multiplicity would make the canonical bytes depend on how many
    // times a declaration was listed.
    let once = NameTable::build(&NameTable::empty(), [declaration("Meters")]);
    let twice = NameTable::build(
        &NameTable::empty(),
        [declaration("Meters"), declaration("Meters")],
    );
    assert_eq!(
        once.entries().len(),
        1,
        "a single declaration yields a single row",
    );
    assert_eq!(
        twice.entries().len(),
        1,
        "a duplicated declaration is collapsed to one row",
    );
    assert_eq!(
        once.canonical_bytes().expect("once serializes"),
        twice.canonical_bytes().expect("twice serializes"),
        "duplicate multiplicity must not change the canonical bytes",
    );
}

#[test]
fn rename_to_a_name_held_by_a_different_identifier_is_rejected() {
    let mut table = NameTable::build(
        &NameTable::empty(),
        [declaration("Meters"), declaration("Seconds")],
    );
    let meters = table
        .identifier_of(DeclarationKind::Type, &Name::new("Meters"))
        .expect("Meters is minted");

    // Renaming Meters onto the name Seconds already holds must be rejected: the
    // per-kind name mapping stays injective rather than double-binding Seconds.
    assert!(
        table.rename(&meters, Name::new("Seconds")).is_err(),
        "a name held by a different identifier of the same kind cannot be taken",
    );
    // The table is untouched: both names still resolve to their own identifiers.
    assert_eq!(table.name_of(&meters), Some(&Name::new("Meters")));
    assert!(
        table
            .identifier_of(DeclarationKind::Type, &Name::new("Seconds"))
            .is_some(),
        "Seconds is still bound to its original identifier",
    );

    // Reassigning an identifier its own current name is a legal no-op rename,
    // not a self-collision.
    assert!(
        table.rename(&meters, Name::new("Meters")).is_ok(),
        "reassigning an identifier its own current name is a legal no-op rename",
    );
}

#[test]
fn nominal_identifier_round_trips_through_nota() {
    let identifier = NominalIdentifier::mint(DeclarationKind::Field, "Entry.domains");
    let nota = identifier.to_nota();
    let document = Document::parse(&nota).expect("identifier NOTA parses");
    let decoded = NominalIdentifier::from_nota_block(&document.root_objects()[0])
        .expect("identifier decodes from its (Kind hex-digest) projection");
    assert_eq!(
        decoded, identifier,
        "a nominal identifier survives a NOTA encode/decode round-trip exactly",
    );
}

#[test]
fn name_table_round_trips_through_canonical_bytes() {
    let table = NameTable::build(
        &NameTable::empty(),
        [
            declaration("Meters"),
            declaration("Seconds"),
            declaration("Grams"),
        ],
    );
    let bytes = table.canonical_bytes().expect("table serializes");
    let recovered = NameTable::from_canonical_bytes(&bytes).expect("table reads back from rkyv");
    assert_eq!(
        recovered, table,
        "a NameTable archived to canonical bytes reads back equal",
    );
}

#[test]
fn re_association_miss_mints_a_fresh_identifier() {
    let prior = NameTable::build(&NameTable::empty(), [declaration("Meters")]);
    let established = prior
        .identifier_of(DeclarationKind::Type, &Name::new("Meters"))
        .expect("Meters is minted");

    // A declaration whose current name is absent from the prior table mints a
    // fresh identifier, distinct from the established one.
    let rebuilt = NameTable::build(&prior, [declaration("Meters"), declaration("Kelvin")]);
    let reused = rebuilt
        .identifier_of(DeclarationKind::Type, &Name::new("Meters"))
        .expect("Meters is present");
    let fresh = rebuilt
        .identifier_of(DeclarationKind::Type, &Name::new("Kelvin"))
        .expect("Kelvin is minted");

    assert_eq!(reused, established, "a present name reuses its identifier");
    assert_ne!(
        fresh, established,
        "a missing name mints a fresh identifier"
    );
    assert_eq!(
        fresh,
        NominalIdentifier::mint(DeclarationKind::Type, "Kelvin"),
        "the fresh identifier is the deterministic mint of its kind and name",
    );
}
