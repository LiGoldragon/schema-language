//! Witnesses for the split TrueSchema view: over the fixture corpus the view's
//! codec projections round-trip value-exactly (the post-flip form of the
//! projection-equivalence witness), derived field names are computed on demand
//! and match what lowering previously materialized, a rename through the
//! `NameTable` changes the projection and the derived names without touching
//! the `CoreSchema` bytes, and only explicit disambiguators are stored as
//! field name data.

use std::path::Path;

use nota::{Document, NotaDecode, NotaEncode};
use schema_language::{
    DeclarationKind, ImportResolver, Name, NameTable, SchemaEngine, SchemaError, SchemaIdentity,
    TrueSchema, TypeDeclaration, TypeDeclarationView,
};

/// One corpus entry: a named `.schema` source and the resolver it needs, if
/// any. Every entry must lower — a lowering failure is a corpus bug, not a
/// skip.
struct CorpusEntry {
    name: &'static str,
    source: &'static str,
    resolver: Option<ImportResolver>,
}

impl CorpusEntry {
    fn plain(name: &'static str, source: &'static str) -> Self {
        Self {
            name,
            source,
            resolver: None,
        }
    }

    fn lower(&self) -> TrueSchema {
        let engine = SchemaEngine::default();
        let identity = SchemaIdentity::new(format!("corpus:{}", self.name), "0.1.0");
        let lowered = match &self.resolver {
            Some(resolver) => {
                engine.lower_true_schema_source_with_resolver(self.source, identity, resolver)
            }
            None => engine.lower_source(self.source, identity),
        };
        lowered.unwrap_or_else(|error| panic!("corpus fixture {} lowers: {error}", self.name))
    }

    fn lower_core(&self) -> Result<(schema_language::CoreSchema, NameTable), SchemaError> {
        let engine = SchemaEngine::default();
        let identity = SchemaIdentity::new(format!("corpus:{}", self.name), "0.1.0");
        match &self.resolver {
            Some(resolver) => engine.lower_core_source_with_resolver(
                self.source,
                identity,
                resolver,
                &NameTable::empty(),
            ),
            None => engine.lower_core_source(self.source, identity, &NameTable::empty()),
        }
    }
}

fn marker_core_resolver() -> ImportResolver {
    let schema_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("marker-core")
        .join("schema");
    ImportResolver::new().with_dependency("marker-core", schema_dir, "0.1.0")
}

/// The source of the explicit-disambiguator fixture: TimeRange duplicates the
/// Time component, so start/end are stored explicit field names, while every
/// Entry field name is derived.
const EXPLICIT_DISAMBIGUATOR_SOURCE: &str = "{}\n[Record.Entry]\n[Recorded.Entry]\n{\n  Record Entry\n  Recorded Entry\n  Domain String\n  Domains Vector.Domain\n  EntryKind [Belief Principle Constraint]\n  Description String\n  Referents Vector.String\n  Entry { Domains EntryKind Description Referents }\n  Time Integer\n  TimeRange { start.Time end.Time }\n}\n[]";

/// The fixture corpus: every checked-in positive-lowering fixture family, the
/// two self-describing repo schemas, and inline fixtures covering explicit
/// field disambiguators and the application-form root.
fn corpus() -> Vec<CorpusEntry> {
    vec![
        CorpusEntry::plain("spirit-min", include_str!("../schemas/spirit-min.schema")),
        CorpusEntry::plain("root-schema", include_str!("../schemas/root.schema")),
        CorpusEntry::plain(
            "spirit-reactive-large",
            include_str!("fixtures/big-schemas/spirit-reactive-large.schema"),
        ),
        CorpusEntry::plain(
            "triad-reactive-large",
            include_str!("fixtures/big-schemas/triad-reactive-large.schema"),
        ),
        CorpusEntry::plain(
            "derived-members",
            include_str!("fixtures/big-schemas/derived-members.schema"),
        ),
        CorpusEntry {
            name: "imported-mail-consumer",
            source: include_str!("fixtures/big-schemas/imported-mail-consumer.schema"),
            resolver: Some(marker_core_resolver()),
        },
        CorpusEntry::plain(
            "reference-fields",
            include_str!("fixtures/source-codec/reference-fields.schema"),
        ),
        CorpusEntry::plain(
            "stream-relations",
            include_str!("fixtures/source-codec/stream-relations.schema"),
        ),
        CorpusEntry::plain(
            "relations",
            include_str!("fixtures/source-codec/relations.schema"),
        ),
        CorpusEntry::plain(
            "family-declarations",
            include_str!("fixtures/source-codec/family-declarations.schema"),
        ),
        CorpusEntry::plain(
            "nested-router-namespace",
            include_str!("fixtures/source-codec/nested-router-namespace.schema"),
        ),
        CorpusEntry::plain(
            "root-inline-payloads",
            include_str!("fixtures/source-codec/root-inline-payloads.schema"),
        ),
        CorpusEntry::plain(
            "namespace-inline-enum-variants",
            include_str!("fixtures/source-codec/namespace-inline-enum-variants.schema"),
        ),
        CorpusEntry::plain(
            "namespace-enum-bare-variants",
            include_str!("fixtures/source-codec/namespace-enum-bare-variants.schema"),
        ),
        CorpusEntry::plain(
            "later-inline-payloads",
            include_str!("fixtures/source-codec/later-inline-payloads.schema"),
        ),
        CorpusEntry::plain(
            "root-payload-field-declarations",
            include_str!("fixtures/source-codec/root-payload-field-declarations.schema"),
        ),
        CorpusEntry::plain(
            "root-header-bare-names",
            include_str!("fixtures/source-codec/root-header-bare-names.schema"),
        ),
        CorpusEntry::plain(
            "fused-markers",
            include_str!("fixtures/impl-catalog/fused-markers.schema"),
        ),
        CorpusEntry::plain(
            "trait-method-sigs",
            include_str!("fixtures/impl-catalog/trait-method-sigs.schema"),
        ),
        CorpusEntry::plain(
            "body-optional",
            include_str!("fixtures/impl-catalog/body-optional.schema"),
        ),
        CorpusEntry::plain("explicit-disambiguators", EXPLICIT_DISAMBIGUATOR_SOURCE),
        // The application-form Input root over a locally-declared
        // four-parameter frame head.
        CorpusEntry::plain(
            "application-root",
            "{} Work.(SignalInput SemaWriteOutput SemaReadOutput EffectOutcome) [] { \
             (| Work In WriteOut ReadOut Outcome |) { In WriteOut ReadOut Outcome } \
             SignalInput String \
             SemaWriteOutput Boolean \
             SemaReadOutput Integer \
             EffectOutcome Boolean \
             } []",
        ),
    ]
}

/// The post-flip projection-equivalence witness: for every corpus fixture the
/// view's codec projections round-trip value-exactly through both structured
/// NOTA and the canonical binary bytes — the encoded form is the projected
/// sidecar tree, so a passing round trip proves the projection reproduces the
/// value lowering built.
#[test]
fn view_codecs_round_trip_value_exactly_over_the_corpus() {
    for entry in corpus() {
        let schema = entry.lower();

        let bytes = schema
            .to_binary_bytes()
            .unwrap_or_else(|error| panic!("fixture {} encodes to rkyv: {error}", entry.name));
        let from_binary = TrueSchema::from_binary_bytes(&bytes)
            .unwrap_or_else(|error| panic!("fixture {} decodes from rkyv: {error}", entry.name));
        assert_eq!(
            from_binary, schema,
            "binary round trip must be value-exact for fixture {}",
            entry.name,
        );

        let nota = schema.to_nota();
        let document = Document::parse(&nota)
            .unwrap_or_else(|error| panic!("fixture {} NOTA parses: {error:?}", entry.name));
        let from_nota = TrueSchema::from_nota_block(&document.root_objects()[0])
            .unwrap_or_else(|error| panic!("fixture {} decodes from NOTA: {error}", entry.name));
        assert_eq!(
            from_nota, schema,
            "NOTA round trip must be value-exact for fixture {}",
            entry.name,
        );
    }
}

/// The retargeted lowering entry produces exactly the split pair the lowered
/// view holds: source → (CoreSchema, NameTable) is the same model.
#[test]
fn lower_core_source_produces_the_view_pair() {
    for entry in corpus() {
        let schema = entry.lower();
        let (core, names) = entry
            .lower_core()
            .unwrap_or_else(|error| panic!("fixture {} lowers to core: {error}", entry.name));
        assert_eq!(
            &core,
            schema.core(),
            "lower_core_source substrate must match the view's for fixture {}",
            entry.name,
        );
        assert_eq!(
            &names,
            schema.names(),
            "lower_core_source name table must match the view's for fixture {}",
            entry.name,
        );
    }
}

/// Lowering is deterministic in the split model: the same source yields equal
/// views, equal substrate canonical bytes, and equal table canonical bytes.
#[test]
fn lowering_the_split_model_is_deterministic() {
    for entry in corpus() {
        let first = entry.lower();
        let second = entry.lower();
        assert_eq!(first, second, "views for {}", entry.name);
        assert_eq!(
            first
                .core()
                .canonical_bytes()
                .expect("first substrate serializes"),
            second
                .core()
                .canonical_bytes()
                .expect("second substrate serializes"),
            "substrate canonical bytes for {}",
            entry.name,
        );
        assert_eq!(
            first
                .names()
                .canonical_bytes()
                .expect("first table serializes"),
            second
                .names()
                .canonical_bytes()
                .expect("second table serializes"),
            "table canonical bytes for {}",
            entry.name,
        );
    }
}

/// Derived field names are computed on demand and match what lowering
/// previously materialized: Entry's fields derive from their references, and
/// TimeRange's duplicated component keeps its explicit disambiguators.
#[test]
fn derived_field_names_project_on_demand_and_match_materialized_names() {
    let schema =
        CorpusEntry::plain("explicit-disambiguators", EXPLICIT_DISAMBIGUATOR_SOURCE).lower();

    let Some(TypeDeclaration::Struct(entry)) = schema.type_named("Entry") else {
        panic!("Entry lowers to a struct");
    };
    let entry_names: Vec<String> = entry
        .fields
        .iter()
        .map(|field| field.name.as_str().to_owned())
        .collect();
    assert_eq!(
        entry_names,
        ["domains", "entry_kind", "description", "referents"],
        "derived field names computed on demand match the previously materialized names",
    );

    let Some(TypeDeclaration::Struct(range)) = schema.type_named("TimeRange") else {
        panic!("TimeRange lowers to a struct");
    };
    let range_names: Vec<String> = range
        .fields
        .iter()
        .map(|field| field.name.as_str().to_owned())
        .collect();
    assert_eq!(
        range_names,
        ["start", "end"],
        "explicit disambiguators survive projection",
    );

    // The view layer reports which names are stored data: TimeRange's are
    // explicit rows, Entry's are on-demand derivations with no row at all.
    let Some(TypeDeclarationView::Struct(entry_view)) = schema.type_view_named("Entry") else {
        panic!("Entry views as a struct");
    };
    assert!(
        entry_view
            .fields()
            .iter()
            .all(|field| !field.has_explicit_name()),
        "derived field names must not be stored as name data",
    );
    let Some(TypeDeclarationView::Struct(range_view)) = schema.type_view_named("TimeRange") else {
        panic!("TimeRange views as a struct");
    };
    assert!(
        range_view
            .fields()
            .iter()
            .all(|field| field.has_explicit_name()),
        "explicit disambiguators are stored as name data",
    );
    // And no Field-kind row exists for a derived name anywhere in the table.
    assert!(
        !schema
            .names()
            .entries()
            .iter()
            .any(|row| row.identifier().kind() == DeclarationKind::Field
                && row.name().as_str().ends_with(":domains")),
        "the table must not hold a row for the derived field name",
    );
}

/// A rename applied through the `NameTable` changes the projection and every
/// derived field name without touching the `CoreSchema` bytes.
#[test]
fn rename_through_the_table_moves_projection_but_not_core_bytes() {
    let mut schema =
        CorpusEntry::plain("explicit-disambiguators", EXPLICIT_DISAMBIGUATOR_SOURCE).lower();

    let core_bytes_before = schema
        .core()
        .canonical_bytes()
        .expect("substrate serializes before rename");

    let domains = schema
        .identifier_named(DeclarationKind::Type, &Name::new("Domains"))
        .expect("the Domains newtype is minted");
    schema
        .rename(&domains, Name::new("TopicSet"))
        .expect("rename through the table succeeds");

    // The projection follows the new name: the old name is gone, the new name
    // resolves, and every reference projects the new spelling.
    assert!(
        schema.type_named("Domains").is_none(),
        "the old name no longer projects",
    );
    assert!(
        schema.type_named("TopicSet").is_some(),
        "the new name projects",
    );

    // The derived field name follows the rename with no stored-name change:
    // Entry's first field derives snake_case of the current type name.
    let Some(TypeDeclaration::Struct(entry)) = schema.type_named("Entry") else {
        panic!("Entry lowers to a struct");
    };
    assert_eq!(
        entry
            .fields
            .first()
            .expect("Entry has fields")
            .name
            .as_str(),
        "topic_set",
        "the derived field name follows the renamed type on demand",
    );

    // The canonical schema text — the full human projection — carries the new
    // name too.
    assert!(
        schema.to_schema_text().contains("TopicSet"),
        "the projected schema text follows the rename",
    );

    // And the substrate is untouched: identical canonical bytes.
    let core_bytes_after = schema
        .core()
        .canonical_bytes()
        .expect("substrate serializes after rename");
    assert_eq!(
        core_bytes_before, core_bytes_after,
        "a rename must not move a single CoreSchema byte",
    );
}
