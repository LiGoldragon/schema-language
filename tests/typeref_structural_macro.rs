//! `TypeReference` exposes the structural reference grammar through
//! `StructuralMacroNode`: full-word built-ins use `Vector.T`,
//! `Optional.T`, `ScopeOf.T`, grouped `Map.(K V)`, and `Bytes.N`, while any
//! other PascalCase head is the generic application form.
//!
//! Round-trip is the witness: every variant decodes from its canonical NOTA
//! form and re-encodes to byte-identical text through the structural node.

use nota::{Document, MacroCandidate, StructuralMacroNode};
use schema_language::{ApplicationHead, Name, TypeReference};

fn decode_reference(input: &str) -> Result<TypeReference, String> {
    let document = Document::parse(input).map_err(|error| error.to_string())?;
    TypeReference::from_structural_candidate(MacroCandidate::new(
        TypeReference::structural_position(),
        document.root_objects().iter().collect(),
    ))
    .map_err(|error| error.to_string())
}

/// Decode the input through the structural candidate, assert the node, then
/// re-encode and assert the text round-trips byte-identically — and that
/// re-decoding the output yields the same node. Multi-argument dotted
/// invocations are two raw NOTA root objects (`Head.` plus the grouped
/// argument record), so the witness uses the candidate boundary instead of the
/// single-root convenience decoder.
fn assert_round_trip(input: &str, expected: TypeReference) {
    let decoded =
        decode_reference(input).unwrap_or_else(|error| panic!("{input} decodes: {error}"));
    assert_eq!(decoded, expected, "{input} decodes to the expected node");
    let encoded = decoded.to_structural_nota();
    assert_eq!(encoded, input, "{input} re-encodes byte-identically");
    let redecoded =
        decode_reference(&encoded).unwrap_or_else(|error| panic!("{encoded} re-decodes: {error}"));
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
    assert_round_trip("Bytes.32", TypeReference::FixedBytes(32));
}

#[test]
fn plain_name_round_trips_through_a_bare_pascal_case_atom() {
    assert_round_trip("Topic", plain("Topic"));
}

#[test]
fn vector_round_trips_through_the_full_word_head() {
    assert_round_trip(
        "Vector.Topic",
        TypeReference::Vector(Box::new(plain("Topic"))),
    );
}

#[test]
fn optional_round_trips_through_the_optional_head() {
    assert_round_trip(
        "Optional.Topic",
        TypeReference::Optional(Box::new(plain("Topic"))),
    );
}

#[test]
fn scope_round_trips_through_the_scope_of_head() {
    assert_round_trip(
        "ScopeOf.Topic",
        TypeReference::ScopeOf(Box::new(plain("Topic"))),
    );
}

#[test]
fn map_round_trips_through_the_grouped_map_head() {
    assert_round_trip(
        "Map.(Topic RecordIdentifier)",
        TypeReference::Map(
            Box::new(plain("Topic")),
            Box::new(plain("RecordIdentifier")),
        ),
    );
}

#[test]
fn nested_grammar_forms_recurse_and_round_trip() {
    assert_round_trip(
        "Vector.Optional.Topic",
        TypeReference::Vector(Box::new(TypeReference::Optional(Box::new(plain("Topic"))))),
    );
    assert_round_trip(
        "Map.(Topic Vector.Entry)",
        TypeReference::Map(
            Box::new(plain("Topic")),
            Box::new(TypeReference::Vector(Box::new(plain("Entry")))),
        ),
    );
}

#[test]
fn scalar_leaf_nests_inside_a_grammar_form() {
    assert_round_trip(
        "Map.(String Boolean)",
        TypeReference::Map(
            Box::new(TypeReference::String),
            Box::new(TypeReference::Boolean),
        ),
    );
}

#[test]
fn dropped_short_heads_lower_to_generic_applications() {
    for (source, head, arguments) in [
        ("Vec.Topic", "Vec", vec![plain("Topic")]),
        ("Option.Topic", "Option", vec![plain("Topic")]),
        ("Scope.Topic", "Scope", vec![plain("Topic")]),
        (
            "KeyValue.(Topic RecordIdentifier)",
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
fn retired_map_payloads_no_longer_parse() {
    for source in [
        "(Map (Topic RecordIdentifier))",
        "Map.Topic.RecordIdentifier",
    ] {
        let decoded = decode_reference(source);
        assert!(
            decoded.is_err(),
            "only grouped Map.(K V) is the Map grammar form now, got {decoded:?}"
        );
    }
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
