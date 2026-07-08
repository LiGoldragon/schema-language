use nota::{Document, NotaDecode, NotaEncode};
use schema_language::{
    Name, SchemaEngine, SchemaError, SchemaIdentity, TrueSchema, TypeDeclaration, TypeReference,
};

fn fixture_source() -> &'static str {
    "{}\n{\n  Vector Vector\n}\n[(Record Entry)]\n[(Recorded Entry)]\n{\n  Record Entry\n  Recorded Entry\n  Domain String\n  Domains Vector.Domain\n  EntryKind [Belief Principle Constraint]\n  Description String\n  Referents Vector.String\n  Entry { Domains EntryKind Description Referents }\n  Time Integer\n  TimeRange { start.Time end.Time }\n}"
}

fn true_schema_fixture() -> TrueSchema {
    SchemaEngine::default()
        .lower_true_schema_source(
            fixture_source(),
            SchemaIdentity::new("schema:true-schema", "0.1.0"),
        )
        .expect("true schema fixture lowers")
}

#[test]
fn authored_schema_decodes_directly_to_true_schema() {
    let true_schema: TrueSchema = true_schema_fixture();

    assert_eq!(
        true_schema.identity().component().as_str(),
        "schema:true-schema"
    );
    assert!(true_schema.type_named("Entry").is_some());
    assert!(true_schema.type_named("TimeRange").is_some());
}

#[test]
fn true_schema_round_trips_through_binary_and_structured_nota() {
    let true_schema = true_schema_fixture();

    let bytes = true_schema
        .to_binary_bytes()
        .expect("true schema encodes to rkyv bytes");
    let recovered = TrueSchema::from_binary_bytes(&bytes).expect("true schema decodes from rkyv");
    assert_eq!(recovered, true_schema);

    let nota = true_schema.to_nota();
    let document = Document::parse(&nota).expect("true schema NOTA parses");
    let decoded = TrueSchema::from_nota_block(&document.root_objects()[0])
        .expect("true schema decodes from structured NOTA");
    assert_eq!(decoded, true_schema);
}

#[test]
fn true_schema_canonical_schema_text_round_trips_product_components() {
    let true_schema = true_schema_fixture();
    let text = true_schema.to_schema_text();

    assert!(
        text.contains("Entry { Domains EntryKind Description Referents }"),
        "unique product components should encode as bare type names: {text}"
    );
    assert!(
        text.contains("TimeRange { start.Time end.Time }"),
        "duplicate component types should preserve explicit disambiguators: {text}"
    );

    let recovered = SchemaEngine::default()
        .lower_true_schema_source(&text, true_schema.identity().clone())
        .expect("canonical true schema text lowers again");
    assert_eq!(recovered, true_schema);
}

#[test]
fn product_components_accept_implicit_unique_types() {
    let true_schema = SchemaEngine::default()
        .lower_true_schema_source(
            "{}\n{}\n[]\n[]\n{ Topic String Description String Entry { Topic Description } }",
            SchemaIdentity::new("components:implicit", "0.1.0"),
        )
        .expect("implicit unique product components are valid");
    let TypeDeclaration::Struct(entry) = true_schema.type_named("Entry").expect("Entry type")
    else {
        panic!("Entry should be a struct");
    };

    assert_eq!(entry.fields[0].name, Name::new("topic"));
    assert_eq!(entry.fields[0].reference, TypeReference::new("Topic"));
    assert_eq!(entry.fields[1].name, Name::new("description"));
}

#[test]
fn product_components_accept_duplicate_types_with_explicit_identities() {
    let true_schema = SchemaEngine::default()
        .lower_true_schema_source(
            "{}\n{}\n[]\n[]\n{ Time Integer TimeRange { start.Time end.Time } }",
            SchemaIdentity::new("components:duplicate", "0.1.0"),
        )
        .expect("duplicate product components with explicit identities are valid");
    let TypeDeclaration::Struct(range) =
        true_schema.type_named("TimeRange").expect("TimeRange type")
    else {
        panic!("TimeRange should be a struct");
    };

    let fields = range
        .fields
        .iter()
        .map(|field| {
            (
                field.name.as_str(),
                field.reference.plain_name().map(Name::as_str),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(fields, vec![("start", Some("Time")), ("end", Some("Time"))]);
}

#[test]
fn product_components_reject_redundant_explicit_derived_identity() {
    let error = SchemaEngine::default()
        .lower_true_schema_source(
            "{}\n{ Vector Vector }\n[]\n[]\n{ Domains Vector.String Entry { domains.Domains } }",
            SchemaIdentity::new("components:redundant", "0.1.0"),
        )
        .expect_err("derived explicit field identity is redundant");

    assert_eq!(
        error,
        SchemaError::RedundantExplicitFieldRole {
            found: "domains.Domains".to_owned(),
            type_name: "Domains".to_owned(),
        }
    );
}

#[test]
fn product_components_reject_explicit_identity_on_unique_type() {
    let error = SchemaEngine::default()
        .lower_true_schema_source(
            "{}\n{}\n[]\n[]\n{ EntryKind [Belief Principle] Entry { kind.EntryKind } }",
            SchemaIdentity::new("components:unique-explicit", "0.1.0"),
        )
        .expect_err("explicit field on unique product component is invalid");

    assert_eq!(
        error,
        SchemaError::ExplicitFieldOnUniqueProductComponent {
            field: "kind".to_owned(),
            type_name: "EntryKind".to_owned(),
        }
    );
}

#[test]
fn product_components_reject_repeated_bare_type_components() {
    let error = SchemaEngine::default()
        .lower_true_schema_source(
            "{}\n{}\n[]\n[]\n{ Time Integer TimeRange { Time Time } }",
            SchemaIdentity::new("components:repeated-bare", "0.1.0"),
        )
        .expect_err("repeated bare product components are ambiguous");

    assert_eq!(
        error,
        SchemaError::DuplicateImplicitProductComponent {
            type_name: "Time".to_owned(),
        }
    );
}

#[test]
fn product_components_reject_duplicate_explicit_identities_for_one_type() {
    let error = SchemaEngine::default()
        .lower_true_schema_source(
            "{}\n{}\n[]\n[]\n{ Time Integer TimeRange { start.Time start.Time } }",
            SchemaIdentity::new("components:duplicate-explicit", "0.1.0"),
        )
        .expect_err("duplicate explicit product component identities are invalid");

    assert_eq!(
        error,
        SchemaError::DuplicateExplicitProductComponentIdentity {
            field: "start".to_owned(),
            type_name: "Time".to_owned(),
        }
    );
}
