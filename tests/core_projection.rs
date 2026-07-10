//! Projection-equivalence witnesses for the stringless `CoreSchema` substrate:
//! over the fixture corpus, decomposing today's stored `TrueSchema` tree into
//! `(CoreSchema, NameTable)` and projecting back yields a tree equal to the
//! original, and the retargeted lowering entry (`lower_core_source`) produces
//! exactly that pair.

use std::path::Path;

use schema_language::{
    ImportResolver, NameTable, SchemaEngine, SchemaError, SchemaIdentity, TrueSchema,
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
        // The explicit-disambiguator fixture: TimeRange duplicates the Time
        // component, so start/end are stored explicit field names.
        CorpusEntry::plain(
            "explicit-disambiguators",
            "{}\n[Record.Entry]\n[Recorded.Entry]\n{\n  Record Entry\n  Recorded Entry\n  Domain String\n  Domains Vector.Domain\n  EntryKind [Belief Principle Constraint]\n  Description String\n  Referents Vector.String\n  Entry { Domains EntryKind Description Referents }\n  Time Integer\n  TimeRange { start.Time end.Time }\n}\n[]",
        ),
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

/// For every corpus fixture, decomposing the stored tree and projecting the
/// substrate back through its name table yields exactly the stored tree.
#[test]
fn projection_over_the_fixture_corpus_equals_the_stored_tree() {
    for entry in corpus() {
        let schema = entry.lower();
        let (core, names) = schema.decompose(&NameTable::empty());
        let projected = core
            .project(&names, schema.identity().clone())
            .unwrap_or_else(|error| panic!("fixture {} projects: {error}", entry.name));
        assert_eq!(
            projected, schema,
            "projected TrueSchema view must equal the stored tree for fixture {}",
            entry.name,
        );
    }
}

/// The retargeted lowering entry produces exactly the pair the stored tree
/// decomposes to: source → (CoreSchema, NameTable) is the same split model.
#[test]
fn lower_core_source_produces_the_decomposed_pair() {
    for entry in corpus() {
        let schema = entry.lower();
        let (expected_core, expected_names) = schema.decompose(&NameTable::empty());
        let (core, names) = entry
            .lower_core()
            .unwrap_or_else(|error| panic!("fixture {} lowers to core: {error}", entry.name));
        assert_eq!(
            core, expected_core,
            "lower_core_source substrate must match decomposition for fixture {}",
            entry.name,
        );
        assert_eq!(
            names, expected_names,
            "lower_core_source name table must match decomposition for fixture {}",
            entry.name,
        );
    }
}

/// Decomposition is deterministic: decomposing the same tree twice yields
/// identical substrate values and identical canonical table bytes.
#[test]
fn decomposition_is_deterministic() {
    for entry in corpus() {
        let schema = entry.lower();
        let (first_core, first_names) = schema.decompose(&NameTable::empty());
        let (second_core, second_names) = schema.decompose(&NameTable::empty());
        assert_eq!(first_core, second_core, "substrate for {}", entry.name);
        assert_eq!(
            first_names
                .canonical_bytes()
                .expect("first table serializes"),
            second_names
                .canonical_bytes()
                .expect("second table serializes"),
            "canonical table bytes for {}",
            entry.name,
        );
    }
}
