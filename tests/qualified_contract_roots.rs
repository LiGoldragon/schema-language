use std::path::PathBuf;

use nota::{Document, NotaDecode, NotaEncode};
use schema_language::{ImportResolver, MacroContext, SchemaEngine, SchemaIdentity, TrueSchema};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/contract-roots")
}

fn schema_directory(package: &str) -> PathBuf {
    fixture_root().join(package).join("schema")
}

fn consumer_source() -> String {
    std::fs::read_to_string(schema_directory("consumer").join("lib.schema"))
        .expect("read qualified contract-root consumer")
}

fn resolver() -> ImportResolver {
    ImportResolver::new()
        .with_dependency("signal-lojix", schema_directory("signal-lojix"), "1.2.3")
        .with_dependency(
            "meta-signal-lojix",
            schema_directory("meta-signal-lojix"),
            "4.5.6",
        )
}

fn lower_consumer() -> TrueSchema {
    SchemaEngine::default()
        .lower_source_with_resolver(
            &consumer_source(),
            SchemaIdentity::new("qualified-consumer:lib", "0.3.0"),
            &mut MacroContext::default(),
            &resolver(),
        )
        .expect("qualified roots resolve without entering the local namespace")
}

#[test]
fn two_contract_roots_with_the_same_local_names_remain_package_qualified() {
    let schema = lower_consumer();
    let roots = schema.external_roots();
    assert_eq!(roots.len(), 4);
    assert_eq!(roots[0].package().name().as_str(), "signal-lojix");
    assert_eq!(roots[0].package().version(), "1.2.3");
    assert_eq!(roots[0].rust_path(), "signal_lojix::Input");
    assert_eq!(roots[3].package().name().as_str(), "meta-signal-lojix");
    assert_eq!(roots[3].package().version(), "4.5.6");
    assert_eq!(roots[3].rust_path(), "meta_signal_lojix::Output");
    assert!(schema.root_named("Input").is_some());
    assert!(schema.root_named("Output").is_some());
    assert!(schema.type_named("Input").is_none());
    assert!(schema.resolved_imports().is_empty());
}

#[test]
fn qualified_contract_root_source_and_codecs_are_deterministic() {
    let first = lower_consumer();
    let second = lower_consumer();
    assert_eq!(first, second, "independent resolution is deterministic");
    let projected = first.to_schema_text();
    assert!(projected.contains("signal-lojix.[Input Output]"));
    assert!(projected.contains("meta-signal-lojix.[Input Output]"));
    assert!(projected.contains("signal-lojix.Input"));
    assert!(projected.contains("meta-signal-lojix.Output"));

    let bytes = first.to_binary_bytes().expect("archive qualified roots");
    assert_eq!(
        TrueSchema::from_binary_bytes(&bytes).expect("decode qualified-root archive"),
        first
    );
    let document = Document::parse(first.to_nota()).expect("parse qualified-root NOTA");
    assert_eq!(
        TrueSchema::from_nota_block(&document.root_objects()[0])
            .expect("decode qualified-root NOTA"),
        first
    );
}
