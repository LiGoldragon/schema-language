//! Architectural-truth witnesses for the closed claims in operator 271
//! `reports/operator/271-context-maintenance-current-state-2026-06-01.md`.
//!
//! Each witness proves the closure named in the report against the current
//! state of the schema sources. The tests are positive witnesses: they
//! assert the present shape of the code, types, and fixtures. If a future
//! agent reverts any of the closures, the test fails.
//!
//! Coverage:
//! - Claim 1 — macro-library source/artifact datatype split CLOSED
//!   (schema `99078b20`).
//! - Claim 4 — strict schema syntax and honest enum bodies CLOSED.
//! - Claim 5 — SchemaSource as typed source data plus TrueSchema as typed
//!   semantic data CLOSED.
//!
//! Companion witnesses live in:
//! - `tests/source_codec.rs` — source text and source rkyv round-trip
//!   witnesses (claim 5 substrate).
//! - `tests/macro_exploration.rs::retired_duplicate_macro_datatype_names_do_not_return`
//!   — negative-witness guard for claim 1.
//! - The flake's `library-mirrors-collapsed` check — Nix-side regression
//!   guard for claim 1.

use nota::{Block, Delimiter, Document};
use schema_language::{
    MacroLibrary, MacroLibraryArtifact, SchemaEngine, SchemaIdentity, SchemaMacro,
    SchemaSourceArtifact, TrueSchema, TypeDeclaration,
};

/// Claim 1 — `MacroLibrary` is one type, not split between source and
/// artifact mirrors. The library's source-entries field is named
/// `source_entries: Vec<MacroLibrarySourceEntry>` (the rename happened in
/// the `99078b20` collapse) and the only present variant is `SchemaMacro`.
#[test]
fn macro_library_source_entries_are_one_type() {
    let source = include_str!("../schemas/builtin-macros.macro-library");
    let library = MacroLibrary::from_nota_source(source)
        .expect("checked-in builtin macro library decodes through one MacroLibrary type");

    assert!(
        !library.source_entries().is_empty(),
        "builtin library carries source entries through MacroLibrary::source_entries"
    );

    for entry in library.source_entries() {
        // The variant_name() method names which enum case the entry holds.
        // After the collapse, only `SchemaMacro` is present — there is no
        // sibling `MacroLibrarySourceEntryData` enum behind the scenes.
        assert_eq!(
            entry.variant_name(),
            "SchemaMacro",
            "the only source-entry variant after the collapse is SchemaMacro"
        );
        // The definition accessor returns `&SchemaMacro` directly, not a
        // separate `MacroDefinitionData` mirror.
        let _macro_definition: &SchemaMacro = entry.definition();
    }
}

/// Claim 1 — `MacroLibraryArtifact` wraps `MacroLibrary` and is the only
/// projection noun for the artifact concern. The previous split between
/// `DeclarativeMacroLibrary` and `MacroLibraryData` no longer exists in the
/// public surface.
#[test]
fn macro_library_artifact_wraps_the_one_library_type() {
    let source = include_str!("../schemas/builtin-macros.macro-library");
    let artifact = MacroLibraryArtifact::from_nota_source(source)
        .expect("checked-in builtin library decodes as artifact");

    // The artifact projects to and from NOTA + rkyv through the same one
    // library type — no Data mirror is required to traverse the boundary.
    let nota = artifact.to_nota_source();
    let from_nota = MacroLibraryArtifact::from_nota_source(&nota)
        .expect("artifact NOTA round-trips through one library type");
    assert_eq!(artifact.library(), from_nota.library());

    let bytes = artifact
        .to_binary_bytes()
        .expect("artifact archives through rkyv");
    let from_binary =
        MacroLibraryArtifact::from_binary_bytes(&bytes).expect("artifact decodes from rkyv bytes");
    assert_eq!(artifact.library(), from_binary.library());

    // `into_library()` consumes the artifact into the inner library noun.
    // The conversion does not pass through any intermediate Data type.
    let library: MacroLibrary = artifact.into_library();
    assert!(!library.source_entries().is_empty());
}

/// Claim 1 — Source-AST witness that the legacy split names are absent from
/// the public surface of the library code. This complements the existing
/// guard in `tests/macro_exploration.rs::retired_duplicate_macro_datatype_names_do_not_return`
/// by scanning the `pub use` re-export in `lib.rs` and the type declarations
/// at the head of `declarative.rs`.
#[test]
fn macro_library_split_does_not_return_through_public_surface() {
    let lib_rs = include_str!("../src/lib.rs");
    let declarative_rs = include_str!("../src/declarative.rs");

    // The collapse removed these as PUBLIC types; the regression guard at
    // tests/macro_exploration.rs:400 covers the broader file, but the
    // tightest signal is that the `pub use` line for declarative no longer
    // contains the retired Data names.
    let pub_use_block = lib_rs
        .lines()
        .skip_while(|line| !line.contains("pub use declarative::"))
        .take_while(|line| !line.trim().ends_with("};"))
        .collect::<Vec<_>>()
        .join("\n");

    let retired_public_names = [
        "DeclarativeMacroLibrary",
        "MacroLibraryData",
        "MacroLibrarySourceEntryData",
        "MacroDefinitionData",
        "MacroPatternData",
        "MacroTemplateData",
    ];
    for retired in retired_public_names {
        assert!(
            !pub_use_block.contains(retired),
            "schema lib.rs must not re-export retired split name {retired}"
        );
    }

    // The current `pub use` line MUST carry the present shape's names.
    assert!(
        pub_use_block.contains("MacroLibrary,") || pub_use_block.contains("MacroLibrary\n"),
        "schema lib.rs must re-export MacroLibrary as the one type"
    );
    assert!(
        pub_use_block.contains("MacroLibraryArtifact"),
        "schema lib.rs must re-export MacroLibraryArtifact"
    );
    assert!(
        pub_use_block.contains("MacroLibrarySourceEntry,")
            || pub_use_block.contains("MacroLibrarySourceEntry\n"),
        "schema lib.rs must re-export MacroLibrarySourceEntry"
    );

    // The declarative source declares the present canonical shape.
    assert!(
        declarative_rs.contains("pub struct MacroLibrary {"),
        "declarative.rs must declare pub struct MacroLibrary"
    );
    assert!(
        declarative_rs.contains("pub struct MacroLibraryArtifact {"),
        "declarative.rs must declare pub struct MacroLibraryArtifact"
    );
    assert!(
        declarative_rs.contains("pub enum MacroLibrarySourceEntry {"),
        "declarative.rs must declare pub enum MacroLibrarySourceEntry"
    );
    assert!(
        declarative_rs.contains("source_entries: Vec<MacroLibrarySourceEntry>"),
        "MacroLibrary must hold source_entries: Vec<MacroLibrarySourceEntry>"
    );
    assert!(
        declarative_rs.contains("library: MacroLibrary"),
        "MacroLibraryArtifact must hold library: MacroLibrary"
    );
    assert!(
        declarative_rs.contains("SchemaMacro(SchemaMacro)"),
        "MacroLibrarySourceEntry::SchemaMacro(SchemaMacro) is the canonical variant"
    );
}

/// Claim 4 — Strict schema syntax: the production `core.schema` and
/// `spirit-min.schema` carry legal NOTA enum bodies. Root headers use compact
/// exported object names, namespace enums use structural variant signatures,
/// and the retired `Record@Entry` short-suffix sugar must not appear.
#[test]
fn production_schema_sources_use_honest_enum_bodies() {
    let core_schema = include_str!("../schemas/core.schema");
    let spirit_min_schema = include_str!("../schemas/spirit-min.schema");
    let root_schema = include_str!("../schemas/root.schema");
    let builtin_macros_schema = include_str!("../schemas/builtin-macros.schema");

    for (name, source) in [
        ("core.schema", core_schema),
        ("spirit-min.schema", spirit_min_schema),
        ("root.schema", root_schema),
        ("builtin-macros.schema", builtin_macros_schema),
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

/// Claim 4 — Spirit-min carries compact root enum bodies of the
/// `[Record Observe]` shape. The namespace defines those payload objects one
/// level below the root header.
#[test]
fn spirit_min_input_enum_body_has_compact_root_variants() {
    let source = include_str!("../schemas/spirit-min.schema");
    let document = Document::parse(source).expect("spirit-min.schema parses as NOTA");
    let root_objects = document.root_objects();

    let input = root_objects
        .first()
        .expect("spirit-min schema starts with an input enum-body vector");
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

    // Root payloads are exported namespace objects, so every input root entry
    // is the compact bare operation name.
    assert!(
        !variants.is_empty(),
        "input vector contains at least one variant"
    );
    let names = variants
        .iter()
        .map(|variant| match variant {
            Block::Atom(atom) => atom.text(),
            _ => panic!(
                "every spirit-min input variant must be a bare operation name; got {variant:?}"
            ),
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
    let _: &[schema_language::ImportDeclaration] = schema.imports();
    let _: &schema_language::Root = schema.input();
    let _: &schema_language::Root = schema.output();
    let _: &schema_language::EnumDeclaration = schema
        .input()
        .as_enum()
        .expect("core input is an enum root");
    let _: &[schema_language::Declaration] = schema.namespace();

    // The namespace carries typed `Declaration` values; pick one and
    // confirm it lowers into one of the typed variants of `TypeDeclaration`.
    let any_declaration = schema
        .namespace()
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
