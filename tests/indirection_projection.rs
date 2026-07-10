//! Witnesses for the indirection-name / depth-cap projection.
//!
//! A deeply nested reference printed under a depth cap hoists its beyond-cap
//! subtrees behind lowercase (lowerCamel) linknames, prints the hoisted
//! structures after the main structure, disambiguates colliding hoisted type
//! names, round-trips the complete encoding value-exactly, and — for a
//! truncating help configuration — renders a projection that is typed so it
//! cannot be fed back as an encoding.

use schema_language::{
    ApplicationHead, FactoredEncoding, IndirectionProjection, LinkedStructureExpansion,
    MainStructureDepthCap, MultiTypeReferenceProjection, Name, SingleTypeReferenceProjection,
    SourceReference, TypeReference,
};

/// `Vector.(Map.(StoredRecord.Alpha StoredRecord.Beta))` — nested two levels
/// deep, with two distinct hoisted subtrees that share the head type name
/// `StoredRecord`.
fn deeply_nested_reference() -> TypeReference {
    let stored_alpha = TypeReference::Application {
        head: ApplicationHead::Local(Name::new("StoredRecord")),
        arguments: vec![TypeReference::Plain(Name::new("Alpha"))],
    };
    let stored_beta = TypeReference::Application {
        head: ApplicationHead::Local(Name::new("StoredRecord")),
        arguments: vec![TypeReference::Plain(Name::new("Beta"))],
    };
    let map = TypeReference::multi_type_application(
        MultiTypeReferenceProjection::Map,
        vec![stored_alpha, stored_beta],
    );
    TypeReference::single_type_application(SingleTypeReferenceProjection::Vector, map)
}

fn complete_encoding(cap: usize, reference: &SourceReference) -> FactoredEncoding {
    IndirectionProjection::new(
        MainStructureDepthCap::new(cap),
        LinkedStructureExpansion::Complete,
    )
    .encode(reference)
    .expect("a complete expansion always encodes")
}

#[test]
fn beyond_cap_subtrees_hoist_behind_lower_camel_linknames() {
    let reference = SourceReference::from_type_reference(&deeply_nested_reference());
    let encoding = complete_encoding(1, &reference);

    // The main structure keeps its shape to the cap, then carries lowercase
    // linknames where the beyond-cap subtrees used to be.
    assert_eq!(
        encoding.main_text(),
        "Vector.(Map.(storedRecord storedRecord2))"
    );

    // Each hoisted structure prints after the main, introduced by its linkname.
    let links = encoding.links();
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].name().as_str(), "storedRecord");
    assert_eq!(links[0].structure_text(), "StoredRecord.Alpha");
    assert_eq!(links[1].name().as_str(), "storedRecord2");
    assert_eq!(links[1].structure_text(), "StoredRecord.Beta");

    assert_eq!(
        encoding.to_schema_text(),
        "Vector.(Map.(storedRecord storedRecord2))\n\
         storedRecord StoredRecord.Alpha\n\
         storedRecord2 StoredRecord.Beta"
    );
}

#[test]
fn colliding_hoisted_type_names_disambiguate() {
    let reference = SourceReference::from_type_reference(&deeply_nested_reference());
    let encoding = complete_encoding(1, &reference);

    // Two hoisted subtrees share the head type name `StoredRecord`; the first
    // takes the bare lowerCamel projection, the second the disambiguated form.
    let names: Vec<&str> = encoding
        .links()
        .iter()
        .map(|link| link.name().as_str())
        .collect();
    assert_eq!(names, vec!["storedRecord", "storedRecord2"]);
}

#[test]
fn capped_but_complete_encoding_round_trips_value_exact() {
    let original = deeply_nested_reference();
    let reference = SourceReference::from_type_reference(&original);
    let encoding = complete_encoding(1, &reference);

    // The encoding hoisted subtrees (the factoring is lost), but the value is
    // complete: re-lowering reproduces the original reference exactly.
    assert_eq!(encoding.to_type_reference(), original);
}

#[test]
fn a_cap_that_hoists_nothing_is_the_plain_encoding() {
    let original = deeply_nested_reference();
    let reference = SourceReference::from_type_reference(&original);
    let encoding = complete_encoding(9, &reference);

    assert!(encoding.links().is_empty());
    assert_eq!(
        encoding.main_text(),
        "Vector.(Map.(StoredRecord.Alpha StoredRecord.Beta))"
    );
    assert_eq!(encoding.to_type_reference(), original);
}

#[test]
fn truncating_help_configuration_renders_and_cannot_encode() {
    let reference = SourceReference::from_type_reference(&deeply_nested_reference());
    let truncating = IndirectionProjection::new(
        MainStructureDepthCap::new(1),
        LinkedStructureExpansion::Truncated { visible_links: 1 },
    );

    // A truncating configuration renders help text...
    let help = truncating.help(&reference);
    let lines: Vec<&str> = help.text().lines().collect();
    assert_eq!(
        lines,
        vec![
            "Vector.(Map.(storedRecord storedRecord2))",
            "storedRecord StoredRecord.Alpha",
        ]
    );

    // ...but is typed so it cannot stand in for an encoding: a truncating
    // configuration yields no `FactoredEncoding`, and the `HelpRendering` it
    // does produce has no path back to a value.
    assert!(truncating.encode(&reference).is_none());
}

#[test]
fn complete_help_renders_every_hoisted_structure() {
    let reference = SourceReference::from_type_reference(&deeply_nested_reference());
    let complete = IndirectionProjection::new(
        MainStructureDepthCap::new(1),
        LinkedStructureExpansion::Complete,
    );

    // Help under a complete expansion is the same rendering as the encoding's
    // full text — help printing is one configuration of the same record.
    let help = complete.help(&reference);
    assert_eq!(
        help.text(),
        complete_encoding(1, &reference).to_schema_text()
    );
    assert_eq!(help.text().lines().count(), 3);
}
