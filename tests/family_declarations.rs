use std::fs;

use schema_language::{
    FamilyKey, SchemaEngine, SchemaError, SchemaIdentity, SchemaSourceArtifact, TrueSchema,
    TypeDeclaration,
};

fn family_fixture() -> String {
    fs::read_to_string("tests/fixtures/source-codec/family-declarations.schema")
        .expect("read family-declarations schema fixture")
        .trim_end()
        .to_owned()
}

fn lower(source: &str) -> Result<schema_language::TrueSchema, SchemaError> {
    SchemaEngine::default().lower_source(source, SchemaIdentity::new("example:lib", "0.1.0"))
}

#[test]
fn family_declarations_round_trip_through_canonical_schema_source() {
    let source = family_fixture();
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");

    assert_eq!(
        artifact.to_schema_text(),
        source,
        "family declarations encode back to the same canonical schema source surface"
    );

    let bytes = artifact
        .to_binary_bytes()
        .expect("schema source artifact archives");
    let recovered =
        SchemaSourceArtifact::from_binary_bytes(&bytes).expect("schema source artifact restores");
    assert_eq!(artifact, recovered);
}

#[test]
fn family_declarations_lower_to_semantic_schema_metadata() {
    let schema = lower(&family_fixture()).expect("schema source lowers");

    assert_eq!(schema.families().len(), 2);

    let keyed = &schema.families()[0];
    assert_eq!(keyed.name.as_str(), "EntryFamily");
    assert_eq!(keyed.record.as_str(), "Entry");
    assert_eq!(keyed.table.as_str(), "entries");
    assert_eq!(keyed.key, FamilyKey::Domain);

    let identified = &schema.families()[1];
    assert_eq!(identified.name.as_str(), "ObservationFamily");
    assert_eq!(identified.record.as_str(), "Observation");
    assert_eq!(identified.table.as_str(), "observations");
    assert_eq!(identified.key, FamilyKey::Identified);

    assert!(
        schema.type_named("EntryFamily").is_none(),
        "family declarations are schema metadata, not namespace data types"
    );
    assert!(
        matches!(schema.type_named("Entry"), Some(TypeDeclaration::Struct(_))),
        "the record type stays an ordinary namespace declaration"
    );
}

#[test]
fn semantic_schema_carrying_families_round_trips_through_rkyv() {
    let schema = lower(&family_fixture()).expect("schema source lowers");
    assert_eq!(schema.families().len(), 2);

    let bytes = schema
        .to_binary_bytes()
        .expect("schema with families serialises to rkyv bytes");
    let recovered =
        TrueSchema::from_binary_bytes(&bytes).expect("rkyv bytes decode back to TrueSchema");

    assert_eq!(recovered, schema);
    assert_eq!(recovered.families(), schema.families());
}

#[test]
fn family_record_closure_hashes_through_the_content_identity_surface() {
    let schema = lower(&family_fixture()).expect("schema source lowers");

    for family in schema.families() {
        let closure = schema
            .family_closure(family.record.as_str())
            .expect("family record closure builds");
        assert_eq!(closure.root(), &family.record);
        closure.content_hash().expect("family closure hashes");
    }
}

#[test]
fn family_record_must_resolve_to_a_declared_type() {
    let source = "\
{}
[(Record Entry)]
[Recorded]
{
  Body String
  Entry { Body }
  GhostFamily (Family { record.Ghost table.ghosts key.Domain })
}
";

    let error = lower(source).expect_err("unresolved family record is a typed error");
    assert_eq!(
        error,
        SchemaError::FamilyRecordNotFound {
            family: "GhostFamily".to_owned(),
            record: "Ghost".to_owned(),
        }
    );
}

#[test]
fn duplicate_family_names_are_a_typed_error() {
    let source = "\
{}
[(Record Entry)]
[Recorded]
{
  Body String
  Entry { Body }
  EntryFamily (Family { record.Entry table.entries key.Domain })
  EntryFamily (Family { record.Entry table.archive key.Domain })
}
";

    let error = lower(source).expect_err("duplicate family name is a typed error");
    assert_eq!(
        error,
        SchemaError::DuplicateFamilyName {
            name: "EntryFamily".to_owned(),
        }
    );
}

#[test]
fn duplicate_family_tables_are_a_typed_error() {
    let source = "\
{}
[(Record Entry) (Observe Query)]
[Recorded]
{
  Body String
  Topic String
  Entry { Body }
  Query { Topic }
  EntryFamily (Family { record.Entry table.entries key.Domain })
  QueryFamily (Family { record.Query table.entries key.Identified })
}
";

    let error = lower(source).expect_err("duplicate family table is a typed error");
    assert_eq!(
        error,
        SchemaError::DuplicateFamilyTable {
            table: "entries".to_owned(),
        }
    );
}

#[test]
fn family_key_kind_is_a_closed_structural_choice() {
    let source = "\
{}
[(Record Entry)]
[Recorded]
{
  Body String
  Entry { Body }
  EntryFamily (Family { record.Entry table.entries key.Sideways })
}
";

    lower(source).expect_err("an unknown family key kind does not lower");
}
