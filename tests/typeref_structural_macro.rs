//! `TypeReference` exposes the structural reference grammar through
//! `StructuralMacroNode`: full-word built-ins use `(Vector T)`,
//! `(Optional T)`, `(ScopeOf T)`, flat `(Map K V)`, and `(Bytes N)`, while any
//! other PascalCase head is the generic application form.
//!
//! Round-trip is the witness: every variant decodes from its canonical NOTA
//! form and re-encodes to byte-identical text through the structural node.

use nota::StructuralMacroNode;
use schema_language::{ApplicationHead, Name, TypeReference};

/// Decode the input through the derive, assert the node, then re-encode and
/// assert the text round-trips byte-identically — and that re-decoding the
/// output yields the same node.
fn assert_round_trip(input: &str, expected: TypeReference) {
    let decoded = TypeReference::from_structural_nota(input)
        .unwrap_or_else(|error| panic!("{input} decodes: {error}"));
    assert_eq!(decoded, expected, "{input} decodes to the expected node");
    let encoded = decoded.to_structural_nota();
    assert_eq!(encoded, input, "{input} re-encodes byte-identically");
    let redecoded = TypeReference::from_structural_nota(&encoded)
        .unwrap_or_else(|error| panic!("{encoded} re-decodes: {error}"));
    assert_eq!(redecoded, expected, "{encoded} re-decodes to the same node");
}

fn plain(name: &str) -> TypeReference {
    TypeReference::Plain(Name::new(name))
}

#[test]
fn scalar_leaves_round_trip_through_their_bare_atoms() {
    assert_round_trip("String", TypeReference::String);
    assert_round_trip("Integer", TypeReference::Integer);
    assert_round_trip("Boolean", TypeReference::Boolean);
    assert_round_trip("Path", TypeReference::Path);
    assert_round_trip("Bytes", TypeReference::Bytes);
}

#[test]
fn fixed_bytes_round_trips_through_the_bytes_head_with_a_width() {
    assert_round_trip("(Bytes 32)", TypeReference::FixedBytes(32));
}

#[test]
fn plain_name_round_trips_through_a_bare_pascal_case_atom() {
    assert_round_trip("Topic", plain("Topic"));
}

#[test]
fn vector_round_trips_through_the_full_word_head() {
    assert_round_trip(
        "(Vector Topic)",
        TypeReference::Vector(Box::new(plain("Topic"))),
    );
}

#[test]
fn optional_round_trips_through_the_optional_head() {
    assert_round_trip(
        "(Optional Topic)",
        TypeReference::Optional(Box::new(plain("Topic"))),
    );
}

#[test]
fn scope_round_trips_through_the_scope_of_head() {
    assert_round_trip(
        "(ScopeOf Topic)",
        TypeReference::ScopeOf(Box::new(plain("Topic"))),
    );
}

#[test]
fn map_round_trips_through_the_flat_map_head() {
    assert_round_trip(
        "(Map Topic RecordIdentifier)",
        TypeReference::Map(
            Box::new(plain("Topic")),
            Box::new(plain("RecordIdentifier")),
        ),
    );
}

#[test]
fn nested_grammar_forms_recurse_and_round_trip() {
    assert_round_trip(
        "(Vector (Optional Topic))",
        TypeReference::Vector(Box::new(TypeReference::Optional(Box::new(plain("Topic"))))),
    );
    assert_round_trip(
        "(Map Topic (Vector Entry))",
        TypeReference::Map(
            Box::new(plain("Topic")),
            Box::new(TypeReference::Vector(Box::new(plain("Entry")))),
        ),
    );
}

#[test]
fn scalar_leaf_nests_inside_a_grammar_form() {
    assert_round_trip(
        "(Map String Boolean)",
        TypeReference::Map(
            Box::new(TypeReference::String),
            Box::new(TypeReference::Boolean),
        ),
    );
}

#[test]
fn dropped_short_heads_lower_to_generic_applications() {
    for (source, head, arguments) in [
        ("(Vec Topic)", "Vec", vec![plain("Topic")]),
        ("(Option Topic)", "Option", vec![plain("Topic")]),
        ("(Scope Topic)", "Scope", vec![plain("Topic")]),
        (
            "(KeyValue Topic RecordIdentifier)",
            "KeyValue",
            vec![plain("Topic"), plain("RecordIdentifier")],
        ),
    ] {
        assert_round_trip(
            source,
            TypeReference::Application {
                head: ApplicationHead::Local(Name::new(head)),
                arguments,
            },
        );
    }
}

#[test]
fn dropped_nested_map_payload_no_longer_parses() {
    // The old nested payload shape is gone; only the flat `(Map K V)` is the
    // Map grammar form now.
    let decoded = TypeReference::from_structural_nota("(Map (Topic RecordIdentifier))");
    assert!(
        decoded.is_err(),
        "the nested Map payload is no longer a grammar form, got {decoded:?}"
    );
}

#[test]
fn dropped_alias_heads_are_only_bare_declared_names() {
    // As bare atoms these spellings are ordinary PascalCase names: they
    // fall through to `Plain`, carrying no wrapper meaning.
    assert_round_trip("Vector", plain("Vector"));
    assert_round_trip("Option", plain("Option"));
    assert_round_trip("ScopeOf", plain("ScopeOf"));
    assert_round_trip("KeyValue", plain("KeyValue"));
}
