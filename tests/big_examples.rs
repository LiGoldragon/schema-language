use std::path::Path;

use schema_language::{
    Declaration, EnumDeclaration, ImportResolver, MacroContext, SchemaEngine, SchemaIdentity,
    TrueSchema, TypeDeclaration,
};

#[test]
fn big_spirit_example_lowers_to_checked_schema_output() {
    assert_big_fixture(
        "spirit-reactive-large",
        include_str!("fixtures/big-schemas/spirit-reactive-large.schema"),
        None,
    );
}

#[test]
fn big_triad_example_lowers_to_checked_schema_output() {
    assert_big_fixture(
        "triad-reactive-large",
        include_str!("fixtures/big-schemas/triad-reactive-large.schema"),
        None,
    );
}

#[test]
fn big_imported_consumer_example_resolves_cross_crate_imports() {
    let schema_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("marker-core")
        .join("schema");
    let resolver = ImportResolver::new().with_dependency("marker-core", schema_dir, "0.1.0");
    assert_big_fixture(
        "imported-mail-consumer",
        include_str!("fixtures/big-schemas/imported-mail-consumer.schema"),
        Some(resolver),
    );
}

fn assert_big_fixture(name: &str, source: &str, resolver: Option<ImportResolver>) {
    let engine = SchemaEngine::default();
    let mut context = MacroContext::default();
    let identity = SchemaIdentity::new(format!("example:{name}"), "0.1.0");
    let schema = match resolver {
        Some(resolver) => engine
            .lower_source_with_resolver(source, identity, &mut context, &resolver)
            .expect("big schema lowers with imports"),
        None => engine
            .lower_source_with_context(source, identity, &mut context)
            .expect("big schema lowers"),
    };
    assert_schema_data_shape(name, &schema);
}

fn assert_schema_data_shape(name: &str, schema: &TrueSchema) {
    assert_eq!(
        schema.identity().component().as_str(),
        format!("example:{name}")
    );
    assert_eq!(schema.identity().version(), "0.1.0");
    assert!(
        !root_enum(schema.input()).variants.is_empty(),
        "{name} should lower typed input variants"
    );
    assert!(
        !root_enum(schema.output()).variants.is_empty(),
        "{name} should lower typed output variants"
    );
    assert!(
        schema.root_named("Input").is_some(),
        "{name} should expose Input as a direct root enum"
    );
    assert!(
        schema.root_named("Output").is_some(),
        "{name} should expose Output as a direct root enum"
    );
    assert!(
        !schema.namespace().is_empty(),
        "{name} should lower typed namespace declarations"
    );
    match name {
        "spirit-reactive-large" => {
            assert_has_type(schema.namespace(), "Entry");
            assert_has_type(schema.namespace(), "RecordSet");
            assert_has_variant(schema.input(), "Record");
            assert_has_variant(schema.output(), "Recorded");
        }
        "triad-reactive-large" => {
            assert_has_type(schema.namespace(), "SignalRequest");
            assert_has_type(schema.namespace(), "NexusRequest");
            assert_has_type(schema.namespace(), "SemaRequest");
            assert_has_variant(schema.input(), "SignalIn");
            assert_has_variant(schema.output(), "SignalOut");
        }
        "imported-mail-consumer" => {
            assert!(!schema.imports().is_empty());
            assert!(!schema.resolved_imports().is_empty());
            assert_has_variant(schema.output(), "Marked");
        }
        _ => panic!("unhandled big fixture {name}"),
    }
}

fn assert_has_type(declarations: Vec<Declaration>, name: &str) {
    let found = declarations
        .iter()
        .any(|declaration| match declaration.value() {
            TypeDeclaration::Struct(declaration) => declaration.name.as_str() == name,
            TypeDeclaration::Newtype(declaration) => declaration.name.as_str() == name,
            TypeDeclaration::Enum(declaration) => declaration.name.as_str() == name,
        });
    assert!(found, "missing namespace type {name}");
}

fn root_enum(root: schema_language::Root) -> EnumDeclaration {
    root.as_enum().cloned().expect("root is the enum-body form")
}

fn assert_has_variant(root: schema_language::Root, name: &str) {
    let declaration = root_enum(root);
    assert!(
        declaration
            .variants
            .iter()
            .any(|variant| variant.name.as_str() == name),
        "missing variant {name} on {}",
        declaration.name.as_str()
    );
}
