use nota::StructuralMacroNode;
use schema_language::{
    MacroContext, MacroLibrary, MacroRegistry, SchemaEngine, SchemaError, SchemaIdentity,
    TypeDeclaration, TypeReference,
};

fn schema_roots(namespace: &str) -> String {
    format!("{{}} [] [] {{ {namespace} }}")
}

fn lower_namespace(namespace: &str) -> Result<schema_language::TrueSchema, SchemaError> {
    SchemaEngine::default().lower_source(
        &schema_roots(namespace),
        SchemaIdentity::new("legacy-reference-pipeline:test", "0.1.0"),
    )
}

#[test]
fn public_type_reference_reader_rejects_parenthesized_builtin_applications() {
    for source in [
        "(Vector Topic)",
        "(Optional Topic)",
        "(ScopeOf Topic)",
        "(Map Topic RecordIdentifier)",
        "(Bytes 32)",
    ] {
        let error = TypeReference::from_structural_nota(source)
            .expect_err("parenthesized builtin reference is not public syntax");
        assert!(
            matches!(
                SchemaError::from(error),
                SchemaError::UnknownTypeReferenceForm { .. }
            ),
            "{source} should be rejected as an unknown parenthesized reference form"
        );
    }
}

#[test]
fn schema_source_rejects_legacy_newtype_reference_at_former_engine_call_site() {
    let error = lower_namespace("Topic String Topics (Vector Topic)")
        .expect_err("namespace newtype bodies must use dotted references");

    assert!(
        matches!(error, SchemaError::UnknownTypeReferenceForm { .. }),
        "old newtype reference should be rejected by the source reader, got {error:?}"
    );
}

#[test]
fn schema_source_rejects_legacy_root_application_at_former_engine_call_site() {
    let error = SchemaEngine::default()
        .lower_source(
            "{}\n(Vector Topic)\n[]\n{ Topic String }",
            SchemaIdentity::new("legacy-reference-pipeline:test", "0.1.0"),
        )
        .expect_err("root applications must be dotted");

    assert!(
        matches!(error, SchemaError::UnknownTypeReferenceForm { .. }),
        "old root reference should be rejected by the source reader, got {error:?}"
    );
}

#[test]
fn macro_reference_templates_use_dotted_reader_and_reject_old_builtin_body() {
    let user_macros = MacroLibrary::from_source(
        "(SchemaMacro Bag TypeReference (Bag $Type) (Reference (Vector $Type)))",
    )
    .expect("legacy-shaped macro definition still parses as macro data");
    let mut registry = MacroRegistry::with_schema_defaults();
    for schema_macro in user_macros.into_macros() {
        registry.register_box(schema_macro);
    }
    let engine = SchemaEngine::with_registry(registry);
    let error = engine
        .lower_source_with_context(
            &schema_roots("Topic String Topics (Bag Topic)"),
            SchemaIdentity::new("legacy-reference-pipeline:test", "0.1.0"),
            &mut MacroContext::default(),
        )
        .expect_err("macro templates must expand to dotted references");

    assert!(
        matches!(error, SchemaError::UnknownTypeReferenceForm { .. }),
        "old macro template body should be rejected by the dotted reader, got {error:?}"
    );
}

#[test]
fn macro_reference_templates_accept_dotted_builtin_body() {
    let user_macros = MacroLibrary::from_source(
        "(SchemaMacro Bag TypeReference (Bag $Type) (Reference Vector. $Type))",
    )
    .expect("dotted macro definition parses");
    let mut registry = MacroRegistry::with_schema_defaults();
    for schema_macro in user_macros.into_macros() {
        registry.register_box(schema_macro);
    }
    let engine = SchemaEngine::with_registry(registry);
    let schema = engine
        .lower_source_with_context(
            &schema_roots("Topic String Topics (Bag Topic)"),
            SchemaIdentity::new("legacy-reference-pipeline:test", "0.1.0"),
            &mut MacroContext::default(),
        )
        .expect("dotted macro template lowers");

    let TypeDeclaration::Newtype(declaration) = schema.type_named("Topics").expect("Topics type")
    else {
        panic!("Topics should be a newtype");
    };
    assert_eq!(
        declaration.reference,
        TypeReference::vector(TypeReference::new("Topic"))
    );
}

#[test]
fn generated_parenthesized_resolver_artifacts_are_absent() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for retired_path in [
        "build.rs",
        "src/reference_resolver_generated.rs",
        "schemas/reference-grammar.nota",
        "schema-language-cc",
    ] {
        assert!(
            !root.join(retired_path).exists(),
            "retired reference pipeline artifact {retired_path} must not exist"
        );
    }

    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("read manifest");
    assert!(!manifest.contains("schema-language-cc"));
    assert!(!manifest.contains("build.rs"));

    let schema_rs = std::fs::read_to_string(root.join("src/schema.rs")).expect("read schema.rs");
    assert!(!schema_rs.contains("from_block_with_registry"));
    assert!(!schema_rs.contains("resolve_parenthesis_reference"));
    assert!(!schema_rs.contains("reference_resolver_generated"));
}
