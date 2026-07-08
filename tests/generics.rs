//! Generic definitions live in the positional generics section, and generic
//! invocation uses dotted structural syntax. Unary invocation may stay flat
//! (`Vector.Topic`); multi-argument invocation groups its arguments as data
//! (`Map.(Key Value)`). A flat chain such as `Map.Key.Value` is unary nesting,
//! not a two-argument map.

use schema_language::{
    ApplicationHead, Name, Root, SchemaEngine, SchemaError, SchemaIdentity, SchemaSourceArtifact,
    TypeDeclaration, TypeReference,
};

const BUILTIN_GENERIC_ROWS: &str =
    "Vector Vector Optional Optional ScopeOf ScopeOf Map Map Bytes FixedBytes";

fn try_lower_with_generics(
    generics: &str,
    namespace: &str,
) -> Result<schema_language::TrueSchema, SchemaError> {
    SchemaEngine::default().lower_source(
        &format!("{{ {BUILTIN_GENERIC_ROWS} {generics} }} [] [] {{ {namespace} }}"),
        SchemaIdentity::new("generics:lib", "0.1.0"),
    )
}

fn lower(namespace: &str) -> schema_language::TrueSchema {
    try_lower_with_generics("", namespace).expect("schema lowers")
}

fn single_reference<'schema>(
    schema: &'schema schema_language::TrueSchema,
    name: &str,
) -> &'schema TypeReference {
    match schema.type_named(name).expect("type present") {
        TypeDeclaration::Newtype(declaration) => &declaration.reference,
        TypeDeclaration::Struct(_) | TypeDeclaration::Enum(_) => {
            panic!("{name} should be a single-reference declaration")
        }
    }
}

#[test]
fn grouped_map_invocation_lowers_to_map_builtin() {
    let schema = lower("Key String Value String Field Map.(Key Value)");
    assert_eq!(
        single_reference(&schema, "Field"),
        &TypeReference::Map(
            Box::new(TypeReference::new("Key")),
            Box::new(TypeReference::new("Value")),
        )
    );
}

#[test]
fn nested_grouped_invocation_preserves_argument_structure() {
    let schema = lower(
        "Key String Value String VectorField Vector.(Map.(Key Value)) MapField Map.(Key Vector.Value)",
    );
    assert_eq!(
        single_reference(&schema, "VectorField"),
        &TypeReference::Vector(Box::new(TypeReference::Map(
            Box::new(TypeReference::new("Key")),
            Box::new(TypeReference::new("Value")),
        )))
    );
    assert_eq!(
        single_reference(&schema, "MapField"),
        &TypeReference::Map(
            Box::new(TypeReference::new("Key")),
            Box::new(TypeReference::Vector(Box::new(TypeReference::new("Value")))),
        )
    );
}

#[test]
fn flat_multi_segment_chain_is_unary_nesting_not_multi_argument_map() {
    let error = try_lower_with_generics("", "Key String Value String Field Map.Key.Value")
        .expect_err("Map.Key.Value is unary nesting and fails Map's two-argument arity");
    assert_eq!(
        error,
        SchemaError::GenericArityMismatch {
            head: "Map".to_owned(),
            expected: 2,
            found: 1,
        },
    );
}

#[test]
fn old_parenthesized_generic_invocation_is_rejected() {
    let error = try_lower_with_generics("", "Key String Value String Field (Map Key Value)")
        .expect_err("parenthesized generic invocation is retired");
    assert!(
        matches!(error, SchemaError::ExpectedSyntaxReference { .. }),
        "old parenthesized invocation should fail as syntax, got {error:?}",
    );
}

#[test]
fn old_pipe_parameterized_declaration_head_is_rejected() {
    let error = SchemaEngine::default()
        .lower_source(
            "{} [] [] { (| Plane Input Output |) [Entered] }",
            SchemaIdentity::new("generics:lib", "0.1.0"),
        )
        .expect_err("parameterized declaration heads are retired");
    assert!(
        matches!(error, SchemaError::ExpectedSyntaxDeclaration { .. }),
        "old pipe declaration head should fail as syntax, got {error:?}",
    );
}

#[test]
fn source_codec_round_trips_grouped_invocation() {
    let source = "{}\n[]\n[]\n{\n  Key String\n  Value String\n  Field Map.(Key Value)\n}";
    let artifact = SchemaSourceArtifact::from_schema_text(source).expect("source decodes");
    let canonical = artifact.to_schema_text();
    assert!(
        canonical.contains("Field Map.(Key Value)"),
        "canonical source must keep grouped map invocation, got {canonical}",
    );
    let recovered =
        SchemaSourceArtifact::from_schema_text(&canonical).expect("canonical source decodes");
    assert_eq!(artifact, recovered, "source codec round-trips");
}

fn work_frame_generic() -> &'static str {
    "Work (Frame [Input Output] [(Entered Input) (Left Output)])"
}

#[test]
fn root_position_application_uses_grouped_arguments() {
    let source = format!(
        "{{ {work} }} Work.(SignalInput SignalOutput) [] {{ SignalInput String SignalOutput Integer }}",
        work = work_frame_generic()
    );
    let schema = SchemaEngine::default()
        .lower_source(&source, SchemaIdentity::new("work-frame:lib", "0.1.0"))
        .expect("application-root schema lowers");
    let application = schema
        .input()
        .as_application()
        .expect("Input root is an application");
    assert_eq!(
        application.head(),
        &ApplicationHead::Local(Name::new("Work"))
    );
    assert_eq!(
        application.arguments(),
        &[
            TypeReference::new("SignalInput"),
            TypeReference::new("SignalOutput"),
        ],
    );
    assert!(matches!(schema.output(), Root::Enum(_)));
}

#[test]
fn grouped_root_application_arity_is_checked() {
    let source = format!(
        "{{ {work} }} Work.(SignalInput) [] {{ SignalInput String SignalOutput Integer }}",
        work = work_frame_generic()
    );
    let error = SchemaEngine::default()
        .lower_source(&source, SchemaIdentity::new("work-frame:lib", "0.1.0"))
        .expect_err("wrong root application arity fails at lowering");
    assert_eq!(
        error,
        SchemaError::GenericArityMismatch {
            head: "Work".to_owned(),
            expected: 2,
            found: 1,
        },
    );
}
