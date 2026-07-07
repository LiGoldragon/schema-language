use nota::Document;
use schema_language::{
    RawDatatypeMap, RawNotaDatatype, RawNotaSequence, RawSchemaFile, SchemaError,
};

const CORE_SCHEMA: &str = include_str!("fixtures/raw-core/core.schema");
const NON_MAP_ROOT_SCHEMA: &str = include_str!("fixtures/raw-core/non-map-root.schema");
const ODD_MAP_SCHEMA: &str = include_str!("fixtures/raw-core/odd-map.schema");

#[test]
fn raw_core_schema_fixture_is_legal_nota_before_schema_reading() {
    let document = Document::parse(CORE_SCHEMA).expect("core.schema parses as NOTA");

    assert_eq!(document.holds_root_objects(), 1);
}

#[test]
fn raw_core_schema_file_root_name_comes_from_filename() {
    let schema = RawSchemaFile::from_path_and_source("schemas/core.schema", CORE_SCHEMA)
        .expect("raw core schema parses");

    assert_eq!(schema.root_name().as_str(), "Core");
}

#[test]
fn raw_core_schema_reads_datatype_key_value_map() {
    let schema = RawSchemaFile::from_path_and_source("schemas/core.schema", CORE_SCHEMA)
        .expect("raw core schema parses");

    assert!(
        schema.datatypes().entries().len() >= 40,
        "large core schema fixture should expose a substantial datatype map"
    );

    assert_eq!(
        schema
            .datatypes()
            .datatype_named("Integer")
            .expect("Integer entry")
            .as_atom(),
        Some("AtomInteger")
    );

    let text = schema
        .datatypes()
        .datatype_named("Text")
        .expect("Text entry")
        .as_vector()
        .expect("Text uses raw bracket form");
    assert_atom_sequence(text, &["String"]);

    let documentation = schema
        .datatypes()
        .datatype_named("Documentation")
        .expect("Documentation entry")
        .as_text()
        .expect("Documentation uses pipe text");
    assert!(
        documentation.contains("apostrophe's text"),
        "pipe text should keep apostrophes without double quotes"
    );
    assert!(
        documentation.contains("closing bracket ]"),
        "pipe text should keep ordinary closing brackets"
    );

    let magnitude = schema
        .datatypes()
        .datatype_named("Magnitude")
        .expect("Magnitude entry")
        .as_record()
        .expect("Magnitude uses raw parenthesis form");
    assert_atom_sequence(magnitude, &["Trace", "Low", "Medium", "High", "Maximum"]);
}

#[test]
fn raw_core_schema_preserves_native_key_value_and_enum_forms() {
    let schema = RawSchemaFile::from_path_and_source("schemas/core.schema", CORE_SCHEMA)
        .expect("raw core schema parses");

    let raw_datatype_map = schema
        .datatypes()
        .datatype_named("RawDatatypeMap")
        .expect("RawDatatypeMap entry")
        .as_key_value()
        .expect("braces are native key-value maps");
    assert_map_atoms(
        raw_datatype_map,
        &[("key", "Name"), ("value", "RawDatatype")],
    );

    let entry_struct = schema
        .datatypes()
        .datatype_named("StructDeclaration")
        .expect("StructDeclaration entry")
        .as_key_value()
        .expect("struct declaration uses key-value map");
    assert_map_atoms(entry_struct, &[("name", "TypeName"), ("fields", "Fields")]);

    let datatype_enum = schema
        .datatypes()
        .datatype_named("DatatypeDeclaration")
        .expect("DatatypeDeclaration entry")
        .as_vector()
        .expect("enum declaration uses bracket vector");
    assert_atom_sequence(
        datatype_enum,
        &["StructDeclaration", "EnumDeclaration", "NewtypeDeclaration"],
    );
}

#[test]
fn raw_core_schema_rejects_non_map_root() {
    Document::parse(NON_MAP_ROOT_SCHEMA).expect("negative fixture remains NOTA-legal");

    let error = RawSchemaFile::from_path_and_source("schemas/core.schema", NON_MAP_ROOT_SCHEMA)
        .expect_err("root must be the known datatype key-value map");

    assert_eq!(
        error,
        SchemaError::ExpectedDelimiter {
            expected: "root key-value datatype map",
        }
    );
}

#[test]
fn raw_core_schema_rejects_odd_key_value_count() {
    Document::parse(ODD_MAP_SCHEMA).expect("negative fixture remains NOTA-legal");

    let error = RawSchemaFile::from_path_and_source("schemas/core.schema", ODD_MAP_SCHEMA)
        .expect_err("brace map must have key/value pairs");

    assert_eq!(error, SchemaError::ExpectedEvenMapEntries { found: 3 });
}

fn assert_atom_sequence(sequence: &RawNotaSequence, expected: &[&str]) {
    assert_eq!(
        sequence
            .items()
            .iter()
            .map(|item| item.as_atom())
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|value| Some(*value))
            .collect::<Vec<_>>()
    );
}

fn assert_map_atoms(map: &RawDatatypeMap, expected: &[(&str, &str)]) {
    let actual = expected
        .iter()
        .map(|(key, _)| {
            let RawNotaDatatype::Atom(value) = map.datatype_named(key).expect("map entry") else {
                panic!("expected atom value for map key {key}");
            };
            (*key, value.as_str())
        })
        .collect::<Vec<_>>();

    assert_eq!(actual, expected);
}
