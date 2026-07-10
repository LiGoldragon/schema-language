//! Witnesses for the provisional nominal-identifier substrate: deterministic,
//! order-independent minting; nominal distinctness; rename-through-table
//! identifier preservation; and fresh minting on a re-association miss.

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
