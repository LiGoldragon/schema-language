use std::path::PathBuf;

use schema_language::{
    ImportResolver, ImportSource, MacroContext, Name, SchemaEngine, SchemaError, SchemaIdentity,
};

fn fixture_schema_dir(crate_dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(crate_dir)
        .join("schema")
}

#[test]
fn import_source_splits_single_colon_target_into_crate_module_type() {
    let source = ImportSource::try_from(&Name::new("marker-core:mail:DatabaseMarker"))
        .expect("well-formed import target");
    assert_eq!(source.crate_name().as_str(), "marker-core");
    assert_eq!(source.module().as_str(), "mail");
    assert_eq!(source.type_name().as_str(), "DatabaseMarker");
    assert_eq!(
        source.rust_path(),
        "marker_core::schema::mail::DatabaseMarker"
    );
}

#[test]
fn import_source_rejects_target_without_crate_module_type() {
    let error = ImportSource::try_from(&Name::new("DatabaseMarker"))
        .expect_err("a bare type is not a cross-crate import target");
    assert_eq!(
        error,
        SchemaError::MalformedImportSource {
            found: "DatabaseMarker".to_owned()
        }
    );
}

#[test]
fn resolver_resolves_import_against_dependency_schema_directory() {
    let resolver = ImportResolver::new().with_dependency(
        "marker-core",
        fixture_schema_dir("marker-core"),
        "0.1.0",
    );
    let engine = SchemaEngine::default();
    let consumer_source =
        std::fs::read_to_string(fixture_schema_dir("import-consumer").join("lib.schema"))
            .expect("read consumer schema");

    let schema = engine
        .lower_source_with_resolver(
            &consumer_source,
            SchemaIdentity::new("import-consumer", "0.1.0"),
            &mut MacroContext::default(),
            &resolver,
        )
        .expect("consumer schema resolves its imports");

    // The imported type is NOT in the consumer's own namespace — it is
    // declared by the dependency crate and only referenced here.
    assert!(schema.type_named("DatabaseMarker").is_none());

    let resolved = schema.resolved_imports();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].local_name().as_str(), "DatabaseMarker");
    assert_eq!(
        resolved[0].source().rust_path(),
        "marker_core::schema::mail::DatabaseMarker"
    );
    assert_eq!(
        resolved[0].use_item(),
        "pub use marker_core::schema::mail::DatabaseMarker as DatabaseMarker;"
    );
}

#[test]
fn resolver_resolves_import_of_dependency_root_enum() {
    let resolver = ImportResolver::new().with_dependency(
        "plane-crate",
        fixture_schema_dir("plane-crate"),
        "0.1.0",
    );
    let engine = SchemaEngine::default();
    let consumer_source =
        "{ SignalInput plane-crate:signal:Input } {} [(Observe SignalInput)] [] {}";

    let schema = engine
        .lower_source_with_resolver(
            consumer_source,
            SchemaIdentity::new("root-import-consumer", "0.1.0"),
            &mut MacroContext::default(),
            &resolver,
        )
        .expect("consumer schema resolves dependency root imports");

    assert_eq!(schema.resolved_imports().len(), 1);
    assert_eq!(
        schema.resolved_imports()[0].use_item(),
        "pub use plane_crate::schema::signal::Input as SignalInput;"
    );
    assert!(schema.type_named("SignalInput").is_none());
}

#[test]
fn resolver_preserves_caller_dependencies_through_local_plane_imports() {
    let runtime_root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/nested-runtime");
    let runtime_package =
        schema_language::SchemaPackage::new(runtime_root, "nested-runtime", "0.1.0");
    let resolver = ImportResolver::new().with_dependency(
        "nested-signal",
        fixture_schema_dir("nested-signal"),
        "0.1.0",
    );
    let engine = SchemaEngine::default();

    let schemas = runtime_package
        .lower_modules_with_resolver(&engine, &resolver)
        .expect("local plane imports keep the dependency resolver");

    let nexus = schemas
        .iter()
        .find(|schema| schema.identity().component().as_str() == "nested-runtime:nexus")
        .expect("nexus schema");
    assert_eq!(
        nexus
            .resolved_imports()
            .iter()
            .map(|import| import.source().rust_path())
            .collect::<Vec<_>>(),
        vec![
            "nested_runtime::schema::sema::ReadInput",
            "nested_runtime::schema::sema::ReadOutput"
        ]
    );

    let sema = schemas
        .iter()
        .find(|schema| schema.identity().component().as_str() == "nested-runtime:sema")
        .expect("sema schema");
    assert_eq!(
        sema.resolved_imports()[0].source().rust_path(),
        "nested_signal::schema::lib::Observation"
    );
}

#[test]
fn resolver_rejects_import_of_a_type_the_dependency_does_not_declare() {
    let resolver = ImportResolver::new().with_dependency(
        "marker-core",
        fixture_schema_dir("marker-core"),
        "0.1.0",
    );
    let engine = SchemaEngine::default();
    let consumer_source = "{ Missing marker-core:mail:Missing } {} [] [] { Topic String }";

    let error = engine
        .lower_source_with_resolver(
            consumer_source,
            SchemaIdentity::new("import-consumer", "0.1.0"),
            &mut MacroContext::default(),
            &resolver,
        )
        .expect_err("a type the dependency does not declare cannot resolve");

    assert_eq!(
        error,
        SchemaError::ImportedTypeNotFound {
            crate_name: "marker-core".to_owned(),
            module: "mail".to_owned(),
            type_name: "Missing".to_owned(),
        }
    );
}

#[test]
fn unregistered_dependency_crate_is_reported() {
    let resolver = ImportResolver::new();
    let engine = SchemaEngine::default();
    let consumer_source =
        "{ DatabaseMarker marker-core:mail:DatabaseMarker } {} [] [] { Topic String }";

    let error = engine
        .lower_source_with_resolver(
            consumer_source,
            SchemaIdentity::new("import-consumer", "0.1.0"),
            &mut MacroContext::default(),
            &resolver,
        )
        .expect_err("an import whose crate was never registered cannot resolve");

    assert_eq!(
        error,
        SchemaError::UnresolvedImportCrate {
            crate_name: "marker-core".to_owned(),
        }
    );
}
