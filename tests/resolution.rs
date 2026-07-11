use std::path::PathBuf;

use schema_language::{
    ImportDeclaration, ImportResolver, ImportSource, MacroContext, Name, SchemaEngine, SchemaError,
    SchemaIdentity, TypeReference,
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
fn dotted_import_target_in_a_bracket_is_rejected() {
    // A trailing-dot import path collects its bracket atoms as targets. A target
    // must be a simple capitalized type name; a dotted atom like `Deep.Type` is
    // not a target — its path segments belong before the bracket. The leading
    // uppercase alone must not pass it.
    let error = SchemaEngine::default()
        .lower_source(
            "{ crate.module.[Deep.Type Plain] }\n[]\n[]\n{ Local.String }\n{}\n{}",
            SchemaIdentity::new("import-target:lib", "0.1.0"),
        )
        .expect_err("a dotted import target is malformed");
    assert_eq!(
        error,
        SchemaError::MalformedImportTarget {
            target: "Deep.Type".to_owned(),
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

    // The resolver resolves an import of a dependency ROOT enum (signal's
    // `Input`): a root reports no arity (`parameter_count` is None, since roots
    // are not parameterizable) and re-exports under its own no-alias name.
    let declaration = ImportDeclaration {
        local_name: Name::new("Input"),
        source: TypeReference::Plain(Name::new("plane-crate:signal:Input")),
    };
    let resolved = resolver
        .resolve(&declaration, &engine)
        .expect("resolver resolves the dependency root enum");
    assert_eq!(resolved.parameter_count(), None);
    assert_eq!(
        resolved.use_item(),
        "pub use plane_crate::schema::signal::Input as Input;"
    );

    // But a consumer cannot BUILD a whole that imports that root under the name
    // `Input` while it already declares its own `Input` root: a loaded schema is
    // one namespace, so the imported `Input` and the local `Input` root are two
    // declarations of one name. Emitting both would place `pub use …::Input as
    // Input;` beside a local `pub enum Input`, the self-referencing name clash
    // the semantic boundary now rejects.
    let consumer_source = "{ plane-crate.signal.Input }\n[Observe.Input]\n[]\n{}\n{}\n{}";
    let error = engine
        .lower_source_with_resolver(
            consumer_source,
            SchemaIdentity::new("root-import-consumer", "0.1.0"),
            &mut MacroContext::default(),
            &resolver,
        )
        .expect_err("importing a dependency root into a same-named local root collides");
    assert_eq!(
        error,
        SchemaError::DuplicateDeclaration {
            name: "Input".to_owned(),
            first_site: "the input root",
            second_site: "a resolved import",
        },
    );
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
    let consumer_source = "{ marker-core.mail.Missing }\n[]\n[]\n{\n  Topic.String\n}\n{}\n{}";

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
        "{ marker-core.mail.DatabaseMarker }\n[]\n[]\n{\n  Topic.String\n}\n{}\n{}";

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
