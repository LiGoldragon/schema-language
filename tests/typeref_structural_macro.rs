//! `TypeReference` exposes the authored structural reference grammar through
//! `StructuralMacroNode`: generics are dotted and positional. Unary forms use
//! `Head.Payload`; multi-argument forms use `Head.(A B)`. The legacy
//! parenthesized generic surface is rejected at this public boundary.

use nota::StructuralMacroNode;
use schema_language::{
    ApplicationHead, Name, SchemaEngine, SchemaIdentity, TypeDeclaration, TypeReference,
};

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

fn lower_reference(namespace: &str, name: &str) -> TypeReference {
    let schema = SchemaEngine::default()
        .lower_source(
            &format!("{{}}\n[]\n[]\n{{ {namespace} }}\n[]"),
            SchemaIdentity::new("typeref:test", "0.1.0"),
        )
        .expect("schema lowers");
    match schema.type_named(name).expect("type present") {
        TypeDeclaration::Newtype(declaration) => declaration.reference.clone(),
        _ => panic!("{name} should be a newtype"),
    }
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
fn fixed_bytes_round_trips_through_the_bytes_definition() {
    assert_round_trip("Bytes.32", TypeReference::fixed_width_bytes(32));
}

#[test]
fn plain_name_round_trips_through_a_bare_pascal_case_atom() {
    assert_round_trip("Topic", plain("Topic"));
}

#[test]
fn unary_generic_definitions_round_trip_through_dotted_forms() {
    assert_round_trip("Vector.Topic", TypeReference::vector(plain("Topic")));
    assert_round_trip("Optional.Topic", TypeReference::optional(plain("Topic")));
    assert_round_trip("ScopeOf.Topic", TypeReference::scope_of(plain("Topic")));
}

#[test]
fn map_lowers_through_grouped_positional_payload() {
    assert_eq!(
        lower_reference(
            "Topic String RecordIdentifier String Holder Map.(Topic RecordIdentifier)",
            "Holder"
        ),
        TypeReference::map(plain("Topic"), plain("RecordIdentifier")),
    );
}

#[test]
fn nested_dotted_forms_recurse() {
    assert_round_trip(
        "Vector.Optional.Topic",
        TypeReference::vector(TypeReference::optional(plain("Topic"))),
    );
    assert_eq!(
        lower_reference(
            "Topic String Entry String Holder Map.(Topic Vector.Entry)",
            "Holder"
        ),
        TypeReference::map(plain("Topic"), TypeReference::vector(plain("Entry"))),
    );
}

#[test]
fn scalar_leaf_nests_inside_a_grouped_generic_form() {
    assert_eq!(
        lower_reference("Holder Map.(String Boolean)", "Holder"),
        TypeReference::map(TypeReference::String, TypeReference::Boolean),
    );
}

#[test]
fn non_builtin_heads_lower_to_generic_applications() {
    for (source, head, arguments) in [
        ("Vec.Topic", "Vec", vec![plain("Topic")]),
        ("Option.Topic", "Option", vec![plain("Topic")]),
        ("Scope.Topic", "Scope", vec![plain("Topic")]),
        ("KeyValue.Topic", "KeyValue", vec![plain("Topic")]),
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
fn legacy_parenthesized_generic_forms_are_rejected() {
    for source in [
        "(Vector Topic)",
        "(Optional Topic)",
        "(ScopeOf Topic)",
        "(Map Topic RecordIdentifier)",
        "(Bytes 32)",
    ] {
        let decoded = TypeReference::from_structural_nota(source);
        assert!(
            decoded.is_err(),
            "{source} must be rejected, got {decoded:?}"
        );
    }
}

#[test]
fn map_dot_key_dot_value_is_unary_nesting_and_rejected_by_map_arity() {
    let decoded = TypeReference::from_structural_nota("Map.Topic.RecordIdentifier");
    assert!(
        decoded.is_err(),
        "Map.Topic.RecordIdentifier supplies one nested payload to Map, got {decoded:?}"
    );
}

#[test]
fn generic_names_are_plain_when_bare() {
    assert_round_trip("Vector", plain("Vector"));
    assert_round_trip("Option", plain("Option"));
    assert_round_trip("ScopeOf", plain("ScopeOf"));
    assert_round_trip("KeyValue", plain("KeyValue"));
}
