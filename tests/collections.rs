//! Collection + Option type references.
//!
//! A struct field or enum-variant payload can now wrap its referenced
//! type in a collection or option. The surface forms are TrueSchema
//! type-reference objects:
//! `(Vector T)`, `(Map K V)`, and `(Optional T)`. They lower to
//! `TypeReference::Vector / Map / Optional`. Bare-symbol fields keep
//! the declared-name shape, while reserved scalar names lower to
//! scalar references instead of pretending to be user namespace types.

use schema_language::{Name, SchemaEngine, SchemaIdentity, TypeDeclaration, TypeReference};

fn lower(source: &str) -> schema_language::TrueSchema {
    SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("collections:lib", "0.1.0"))
        .expect("schema lowers")
}

fn roots(namespace: &str) -> String {
    format!("[] [] {{ {namespace} }}")
}

fn struct_fields<'schema>(
    schema: &'schema schema_language::TrueSchema,
    name: &str,
) -> &'schema [schema_language::FieldDeclaration] {
    match schema.type_named(name).expect("type present") {
        TypeDeclaration::Struct(declaration) => &declaration.fields,
        TypeDeclaration::Newtype(_) | TypeDeclaration::Enum(_) => {
            panic!("{name} should be a struct")
        }
    }
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
fn vec_field_lowers_to_vector_reference() {
    let schema = lower(&roots("Service String Cluster (Vector Service)"));
    assert_eq!(
        single_reference(&schema, "Cluster"),
        &TypeReference::Vector(Box::new(TypeReference::new("Service")))
    );
}

#[test]
fn scalar_type_components_lower_to_reserved_references() {
    let schema = lower(&roots("Entry { String Integer Boolean Path }"));
    let fields = struct_fields(&schema, "Entry");
    assert_eq!(fields[0].name.as_str(), "string");
    assert_eq!(fields[0].reference, TypeReference::String);
    assert_eq!(fields[1].name.as_str(), "integer");
    assert_eq!(fields[1].reference, TypeReference::Integer);
    assert_eq!(fields[2].name.as_str(), "boolean");
    assert_eq!(fields[2].reference, TypeReference::Boolean);
    assert_eq!(fields[3].name.as_str(), "path");
    assert_eq!(fields[3].reference, TypeReference::Path);
}

#[test]
fn scalar_references_nest_inside_collections() {
    let schema = lower(&roots(
        "Query { (Optional Integer) (Vector String) (Map String Boolean) (Optional Path) }",
    ));
    let fields = struct_fields(&schema, "Query");
    assert_eq!(
        fields[0].reference,
        TypeReference::Optional(Box::new(TypeReference::Integer))
    );
    assert_eq!(
        fields[1].reference,
        TypeReference::Vector(Box::new(TypeReference::String))
    );
    assert_eq!(
        fields[2].reference,
        TypeReference::Map(
            Box::new(TypeReference::String),
            Box::new(TypeReference::Boolean)
        )
    );
    assert_eq!(
        fields[3].reference,
        TypeReference::Optional(Box::new(TypeReference::Path))
    );
}

#[test]
fn explicit_structural_field_roles_lower_recursively() {
    // The single lowering engine (the typed-source path) reads an explicit
    // PascalCase-named field `Topics.(Vector Topic)` as an inline namespace
    // declaration: a newtype `Topics` aliasing `Vector<Topic>` is minted into
    // the namespace, and the struct field references that minted type by name.
    let schema = lower(&roots(
        "Topic String Query { Topics.(Vector Topic) Limit.(Optional Integer) }",
    ));
    let fields = struct_fields(&schema, "Query");

    assert_eq!(fields[0].name.as_str(), "topics");
    assert_eq!(
        fields[0].reference,
        TypeReference::Plain(Name::new("Topics"))
    );
    assert_eq!(fields[1].name.as_str(), "limit");
    assert_eq!(
        fields[1].reference,
        TypeReference::Plain(Name::new("Limit"))
    );

    // The inline-minted types carry the collection/option reference.
    assert_eq!(
        single_reference(&schema, "Topics"),
        &TypeReference::Vector(Box::new(TypeReference::new("Topic")))
    );
    assert_eq!(
        single_reference(&schema, "Limit"),
        &TypeReference::Optional(Box::new(TypeReference::Integer))
    );
}

#[test]
fn implicit_composite_field_lowers_directly() {
    let schema = lower(&roots(
        "Topic String Description String Query { (Vector Topic) Description }",
    ));
    let fields = struct_fields(&schema, "Query");

    assert_eq!(fields[0].name.as_str(), "topic_vector");
    assert_eq!(
        fields[0].reference,
        TypeReference::Vector(Box::new(TypeReference::new("Topic")))
    );
    assert!(
        schema.type_named("Topics").is_none(),
        "implicit composite components do not mint a role newtype"
    );
}

#[test]
fn pascal_case_dot_composite_field_can_match_derived_composite_name() {
    let schema = lower(&roots(
        "Antecedent String Quote { OptionalAntecedent.(Optional Antecedent) }",
    ));

    assert_eq!(
        single_reference(&schema, "OptionalAntecedent"),
        &TypeReference::Optional(Box::new(TypeReference::new("Antecedent")))
    );
    assert_eq!(
        single_reference(&schema, "Quote"),
        &TypeReference::Plain(Name::new("OptionalAntecedent"))
    );
}

#[test]
fn parenthesized_explicit_composite_field_syntax_is_retired() {
    let error = SchemaEngine::default()
        .lower_source(
            &roots("Topic String Query { (Topics (Vector Topic)) }"),
            schema_language::SchemaIdentity::new("example:lib", "0.1.0"),
        )
        .expect_err("old parenthesized explicit field syntax is retired");

    assert!(matches!(
        error,
        schema_language::SchemaError::RetiredStructFieldSyntax { .. }
    ));
}

#[test]
fn scalar_names_are_reserved_at_namespace_declaration_position() {
    let error = SchemaEngine::default()
        .lower_source(
            "[] [] { String Integer }",
            SchemaIdentity::new("collections:lib", "0.1.0"),
        )
        .expect_err("reserved scalar names cannot be user-declared schema types");
    assert_eq!(
        error,
        schema_language::SchemaError::ReservedScalarTypeName {
            name: "String".to_owned(),
        }
    );
}

#[test]
fn key_value_field_lowers_to_map_reference() {
    let schema = lower(&roots(
        "NodeName String NodeProposal String Cluster (Map NodeName NodeProposal)",
    ));
    assert_eq!(
        single_reference(&schema, "Cluster"),
        &TypeReference::Map(
            Box::new(TypeReference::new("NodeName")),
            Box::new(TypeReference::new("NodeProposal")),
        )
    );
}

#[test]
fn option_field_lowers_to_optional_reference() {
    let schema = lower(&roots("Cache String Cluster (Optional Cache)"));
    assert_eq!(
        single_reference(&schema, "Cluster"),
        &TypeReference::Optional(Box::new(TypeReference::new("Cache")))
    );
}

#[test]
fn square_bracket_field_is_not_vec_type_syntax() {
    let error = SchemaEngine::default()
        .lower_source(
            "[] [] { Service String Cluster { [Service] } }",
            SchemaIdentity::new("collections:lib", "0.1.0"),
        )
        .expect_err("raw square bracket is not a Vec reference");
    // The single lowering engine (the typed-source path) rejects a raw
    // square-bracket block at a field position as a non-symbol reference.
    assert_eq!(
        error,
        schema_language::SchemaError::ExpectedSymbol {
            found: "square bracket block".to_owned(),
        }
    );
}

#[test]
fn brace_field_is_not_map_type_syntax() {
    let error = SchemaEngine::default()
        .lower_source(
            "[] [] { NodeName String NodeProposal String Cluster { {NodeName NodeProposal} } }",
            SchemaIdentity::new("collections:lib", "0.1.0"),
        )
        .expect_err("raw brace map is not a Map reference");
    // The single lowering engine (the typed-source path) rejects a raw brace
    // block at a field position as a non-symbol reference.
    assert_eq!(
        error,
        schema_language::SchemaError::ExpectedSymbol {
            found: "brace block".to_owned(),
        }
    );
}

#[test]
fn collection_field_and_plain_field_coexist_in_one_struct() {
    let schema = lower(&roots(
        "Trust String Service String Cluster { Trust (Vector Service) (Optional Trust) }",
    ));
    let fields = struct_fields(&schema, "Cluster");
    // Bare symbol stays a plain field with its name derived from type.
    assert_eq!(fields[0].name.as_str(), "trust");
    assert_eq!(fields[0].reference, TypeReference::new("Trust"));
    assert_eq!(fields[1].name.as_str(), "service_vector");
    assert!(matches!(fields[1].reference, TypeReference::Vector(_)));
    assert_eq!(fields[2].name.as_str(), "optional_trust");
    assert!(matches!(fields[2].reference, TypeReference::Optional(_)));
}

#[test]
fn nested_collections_lower_recursively() {
    // A map whose value is itself a vector of an optional leaf.
    let schema = lower(&roots(
        "Leaf String Key String Nest (Map Key (Vector (Optional Leaf)))",
    ));
    assert_eq!(
        single_reference(&schema, "Nest"),
        &TypeReference::Map(
            Box::new(TypeReference::new("Key")),
            Box::new(TypeReference::Vector(Box::new(TypeReference::Optional(
                Box::new(TypeReference::new("Leaf"))
            )))),
        )
    );
}

#[test]
fn collection_payload_lowers_in_an_output_variant() {
    // Output variant carrying a map payload — the projection result
    // shape Horizon needs (Projected -> a map of node configs).
    let schema =
        lower("[] [(Projected (Map NodeName NodeConfig))] { NodeName String NodeConfig String }");
    let payload = schema
        .output()
        .as_enum()
        .expect("output is the enum-body form")
        .variants[0]
        .payload
        .as_ref()
        .expect("projected payload");
    assert_eq!(
        payload,
        &TypeReference::Map(
            Box::new(TypeReference::new("NodeName")),
            Box::new(TypeReference::new("NodeConfig")),
        )
    );
}

#[test]
fn non_builtin_pascal_head_lowers_to_application() {
    // A PascalCase head that is not a built-in collection is no longer an
    // error — it is the generic application form `(Foo A …)`. The head is a
    // local generic until import resolution proves otherwise, and the
    // argument nests through the full reference grammar (here a built-in
    // `(Vector Leaf)`).
    let schema = lower(&roots("Leaf String Bad (HashSet (Vector Leaf))"));
    assert_eq!(
        single_reference(&schema, "Bad"),
        &TypeReference::Application {
            head: schema_language::ApplicationHead::Local(schema_language::Name::new("HashSet")),
            arguments: vec![TypeReference::Vector(Box::new(TypeReference::new("Leaf")))],
        }
    );
}

#[test]
fn dropped_vec_alias_no_longer_lowers_to_vector() {
    // `Vec` is a dropped alias: it must NOT lower to `Vector`. It is now an
    // ordinary PascalCase head, so it lowers to the generic application form
    // rather than the collection.
    let schema = lower(&roots("Service String Cluster (Vec Service)"));
    assert_eq!(
        single_reference(&schema, "Cluster"),
        &TypeReference::Application {
            head: schema_language::ApplicationHead::Local(schema_language::Name::new("Vec")),
            arguments: vec![TypeReference::new("Service")],
        }
    );
}

#[test]
fn map_with_wrong_argument_count_is_rejected() {
    let error = SchemaEngine::default()
        .lower_source(
            "[] [] { Leaf String Bad (Map (Leaf)) }",
            SchemaIdentity::new("collections:lib", "0.1.0"),
        )
        .expect_err("Map needs two arguments");
    // The single lowering engine (the typed-source path) gates a built-in
    // reference head against its declared arity.
    assert_eq!(
        error,
        schema_language::SchemaError::ExpectedSyntaxReferenceArity {
            form: "built-in reference head",
            expected: "the head's declared arity",
            found: 2,
        }
    );
}
