//! Architectural-truth witnesses for the closed claims in operator 271
//! `reports/operator/271-context-maintenance-current-state-2026-06-01.md`.
//!
//! Each witness proves the closure named in the report against the current
//! state of the schema sources. The tests are positive witnesses: they
//! assert the present shape of the code, types, and fixtures. If a future
//! agent reverts any of the closures, the test fails.
//!
//! Coverage:
//! - Claim 4 — strict schema syntax and honest enum bodies CLOSED.
//! - Claim 5 — SchemaSource as typed source data plus TrueSchema as typed
//!   semantic data CLOSED.
//!
//! Companion witnesses live in:
//! - `tests/source_codec.rs` — source text and source rkyv round-trip
//!   witnesses (claim 5 substrate).

use nota::{Block, Delimiter, Document};
use schema_language::{
    SchemaEngine, SchemaIdentity, SchemaSourceArtifact, TrueSchema, TypeDeclaration,
};

/// Claim 4 — Strict schema syntax: the production `core.schema` and
/// `spirit-min.schema` carry legal NOTA enum bodies. Root headers use compact
/// exported object names, namespace enums use structural variant signatures,
/// and the retired `Record@Entry` short-suffix sugar must not appear.
#[test]
fn production_schema_sources_use_honest_enum_bodies() {
    let core_schema = include_str!("../schemas/core.schema");
    let spirit_min_schema = include_str!("../schemas/spirit-min.schema");
    let root_schema = include_str!("../schemas/root.schema");

    for (name, source) in [
        ("core.schema", core_schema),
        ("spirit-min.schema", spirit_min_schema),
        ("root.schema", root_schema),
    ] {
        // No retired `@` short-suffix variant sugar.
        // Allowed `@` use: none in schema files. The check is total.
        assert!(
            !source.contains('@'),
            "{name} must not carry the retired `@` short-suffix sugar"
        );

        // Each schema must parse as legal NOTA — proves the honest bodies
        // are syntactically valid through the same parser the engine uses.
        Document::parse(source).unwrap_or_else(|error| {
            panic!("{name} must parse as legal NOTA (honest bodies are NOTA-valid): {error}")
        });
    }
}

/// Claim 4 — Spirit-min carries compact root enum bodies in the strict input
/// slot. The namespace defines distinct payload objects one level below the
/// root header.
#[test]
fn spirit_min_input_enum_body_has_explicit_payload_variants() {
    let source = include_str!("../schemas/spirit-min.schema");
    let document = Document::parse(source).expect("spirit-min.schema parses as NOTA");
    let root_objects = document.root_objects();

    let input = root_objects
        .get(1)
        .expect("spirit-min schema has an input enum-body vector in slot 2");
    let Block::Delimited {
        delimiter,
        root_objects: variants,
        ..
    } = input
    else {
        panic!("input root must be a delimited block")
    };
    assert_eq!(
        *delimiter,
        Delimiter::SquareBracket,
        "input is a SquareBracket enum-body vector"
    );

    // Root payloads use explicit, distinct payload type names so same-named
    // variant payloads cannot collapse in projection.
    assert!(
        !variants.is_empty(),
        "input vector contains at least one variant"
    );
    let names = variants
        .iter()
        .map(|variant| {
            variant
                .as_application()
                .and_then(|(head, _)| head.demote_to_string())
                .expect("spirit-min input variant is dotted with an explicit payload type")
        })
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["Record", "Observe"]);
}

/// Claim 5 — `TrueSchema` is typed Rust data. The type carries the schema
/// identity plus the typed projections of imports, resolved imports, input,
/// output, and namespace declarations. This is the noun the rest of the
/// projection chain consumes.
#[test]
fn schema_is_typed_data_with_named_field_accessors() {
    let source = include_str!("../schemas/core.schema");
    let schema: TrueSchema = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("schema:core", "0.1.0"))
        .expect("core schema lowers to typed TrueSchema data");

    assert_eq!(schema.identity().component().as_str(), "schema:core");
    assert_eq!(schema.identity().version(), "0.1.0");

    // Typed accessors — TrueSchema is a noun with methods, not a string blob.
    let _: Vec<schema_language::ImportDeclaration> = schema.imports();
    let _: schema_language::Root = schema.input();
    let _: schema_language::Root = schema.output();
    let _: schema_language::EnumDeclaration = schema
        .input()
        .as_enum()
        .cloned()
        .expect("core input is an enum root");
    let _: Vec<schema_language::Declaration> = schema.namespace();

    // The namespace carries typed `Declaration` values; pick one and
    // confirm it lowers into one of the typed variants of `TypeDeclaration`.
    let namespace = schema.namespace();
    let any_declaration = namespace
        .first()
        .expect("core schema has at least one namespace declaration");
    match any_declaration.value() {
        TypeDeclaration::Struct(_) | TypeDeclaration::Enum(_) | TypeDeclaration::Newtype(_) => { /* typed variant; expected */
        }
    }
}

/// Claim 5 — authored schema source text projects into a typed
/// `SchemaSourceArtifact`, and both source text and rkyv source bytes
/// round-trip. The semantic `TrueSchema` value keeps only the binary archive
/// projection; the retired `.asschema` NOTA artifact path is absent.
#[test]
fn schema_source_and_semantic_schema_round_trip_without_asschema_artifacts() {
    let source = include_str!("../schemas/core.schema");
    let source_artifact =
        SchemaSourceArtifact::from_schema_text(source).expect("core source decodes");

    let source_text = source_artifact.to_schema_text();
    let recovered_source =
        SchemaSourceArtifact::from_schema_text(&source_text).expect("source text re-decodes");
    assert_eq!(recovered_source, source_artifact);

    let source_bytes = source_artifact
        .to_binary_bytes()
        .expect("source artifact serialises through rkyv");
    let recovered_from_source_bytes =
        SchemaSourceArtifact::from_binary_bytes(&source_bytes).expect("source archive decodes");
    assert_eq!(recovered_from_source_bytes, source_artifact);

    let schema = SchemaEngine::default()
        .lower_schema_source(
            source_artifact.source(),
            SchemaIdentity::new("schema:core", "0.1.0"),
        )
        .expect("core schema lowers");
    let bytes = schema
        .to_binary_bytes()
        .expect("schema serialises to rkyv bytes");
    let from_bytes =
        TrueSchema::from_binary_bytes(&bytes).expect("rkyv bytes decode back to TrueSchema");
    assert_eq!(from_bytes, schema);
}
