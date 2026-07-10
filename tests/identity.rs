use std::path::PathBuf;

use schema_language::{
    ContentHash, ImportResolver, MacroContext, SchemaEngine, SchemaError, SchemaIdentity,
    TrueSchema,
};

/// One semantic schema spelled as several `.schema` source texts. The
/// `Entry` family closes over `Entry -> Detail -> Magnitude` (deep) and
/// `Entry -> Topic` (shallow); `Unrelated` is outside that closure.
struct IdentityFixture {
    identity: SchemaIdentity,
}

impl IdentityFixture {
    fn new() -> Self {
        Self {
            identity: SchemaIdentity::new("identity-fixture:lib", "0.1.0"),
        }
    }

    const BASE: &'static str = "\
{}
[Record.Entry]
[Recorded.Receipt Rejected.Rejection]
{
  Topic String
  Magnitude Integer
  Reason String
  Note String
  Detail { Magnitude }
  Entry { Topic Detail }
  Receipt { Topic }
  Rejection { Reason }
  Unrelated { Note }
}
[]
";

    /// BASE re-spelled with different whitespace and `;;` comments —
    /// the same semantic schema in a different textual coat.
    const REFORMATTED: &'static str = "\
;; the input root
{}
[ Record.Entry ]
;; the output root
[ Recorded.Receipt   Rejected.Rejection ]
{
  Topic     String   ;; alias
  Magnitude Integer
  Reason    String
  Note      String

  Detail    { Magnitude }
  Entry     { Topic Detail }
  Receipt   { Topic }
  Rejection { Reason }
  Unrelated { Note }
}
[]
";

    /// BASE with `Magnitude` — two reference hops below `Entry` —
    /// changed from `Integer` to `String`.
    const DEEP_CHANGE: &'static str = "\
{}
[Record.Entry]
[Recorded.Receipt Rejected.Rejection]
{
  Topic String
  Magnitude String
  Reason String
  Note String
  Detail { Magnitude }
  Entry { Topic Detail }
  Receipt { Topic }
  Rejection { Reason }
  Unrelated { Note }
}
[]
";

    /// BASE with only `Unrelated` — unreachable from `Entry` — changed.
    const UNRELATED_CHANGE: &'static str = "\
{}
[Record.Entry]
[Recorded.Receipt Rejected.Rejection]
{
  Topic String
  Magnitude Integer
  Reason String
  Note Integer
  Detail { Magnitude }
  Entry { Topic Detail }
  Receipt { Topic }
  Rejection { Reason }
  Unrelated { Note }
}
[]
";

    fn schema(&self, source: &str) -> TrueSchema {
        SchemaEngine::default()
            .lower_source(source, self.identity.clone())
            .expect("fixture schema lowers")
    }

    fn family_hash(&self, source: &str, family: &str) -> ContentHash {
        self.schema(source)
            .family_closure(family)
            .expect("family closure builds")
            .content_hash()
            .expect("family closure hashes")
    }

    fn schema_hash(&self, source: &str) -> ContentHash {
        self.schema(source).core_hash().expect("schema hashes")
    }
}

#[test]
fn identical_schema_produces_identical_hashes() {
    let fixture = IdentityFixture::new();

    assert_eq!(
        fixture.schema_hash(IdentityFixture::BASE),
        fixture.schema_hash(IdentityFixture::BASE),
    );
    assert_eq!(
        fixture.family_hash(IdentityFixture::BASE, "Entry"),
        fixture.family_hash(IdentityFixture::BASE, "Entry"),
    );
    assert_eq!(
        fixture.family_hash(IdentityFixture::BASE, "Input"),
        fixture.family_hash(IdentityFixture::BASE, "Input"),
    );
}

#[test]
fn deep_field_type_change_changes_the_family_hash() {
    let fixture = IdentityFixture::new();

    assert_ne!(
        fixture.family_hash(IdentityFixture::BASE, "Entry"),
        fixture.family_hash(IdentityFixture::DEEP_CHANGE, "Entry"),
    );
    assert_ne!(
        fixture.schema_hash(IdentityFixture::BASE),
        fixture.schema_hash(IdentityFixture::DEEP_CHANGE),
    );
}

#[test]
fn unrelated_declaration_change_leaves_the_family_hash_unchanged() {
    let fixture = IdentityFixture::new();

    assert_eq!(
        fixture.family_hash(IdentityFixture::BASE, "Entry"),
        fixture.family_hash(IdentityFixture::UNRELATED_CHANGE, "Entry"),
    );
    assert_ne!(
        fixture.schema_hash(IdentityFixture::BASE),
        fixture.schema_hash(IdentityFixture::UNRELATED_CHANGE),
    );
}

#[test]
fn formatting_differences_do_not_change_any_hash() {
    let fixture = IdentityFixture::new();

    assert_eq!(
        fixture.schema_hash(IdentityFixture::BASE),
        fixture.schema_hash(IdentityFixture::REFORMATTED),
    );
    assert_eq!(
        fixture.family_hash(IdentityFixture::BASE, "Entry"),
        fixture.family_hash(IdentityFixture::REFORMATTED, "Entry"),
    );
    assert_eq!(
        fixture.family_hash(IdentityFixture::BASE, "Input"),
        fixture.family_hash(IdentityFixture::REFORMATTED, "Input"),
    );
}

#[test]
fn family_closure_collects_only_reachable_declarations_sorted_by_name() {
    let fixture = IdentityFixture::new();
    let schema = fixture.schema(IdentityFixture::BASE);

    let closure = schema.family_closure("Entry").expect("entry closure");
    assert_eq!(closure.root().as_str(), "Entry");
    let names = closure
        .declarations()
        .iter()
        .map(|declaration| declaration.name().as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Detail", "Entry", "Magnitude", "Topic"]);
    assert!(closure.imports().is_empty());
    assert!(closure.streams().is_empty());
}

#[test]
fn root_enum_family_closes_over_its_variant_payloads() {
    let fixture = IdentityFixture::new();
    let schema = fixture.schema(IdentityFixture::BASE);

    let closure = schema.family_closure("Output").expect("output closure");
    let names = closure
        .declarations()
        .iter()
        .map(|declaration| declaration.name().as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Output", "Reason", "Receipt", "Rejection", "Topic"]);
}

#[test]
fn unknown_family_root_is_a_typed_error() {
    let fixture = IdentityFixture::new();
    let schema = fixture.schema(IdentityFixture::BASE);

    let error = schema
        .family_closure("Absent")
        .expect_err("no Absent declaration exists");
    assert_eq!(
        error,
        SchemaError::FamilyRootNotFound {
            name: "Absent".to_owned()
        }
    );
}

#[test]
fn family_reaching_an_import_includes_its_stable_identity() {
    let resolver = ImportResolver::new().with_dependency(
        "marker-core",
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/marker-core/schema"),
        "0.1.0",
    );
    let consumer_source = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/import-consumer/schema/lib.schema"),
    )
    .expect("read consumer schema");
    let lower = |source: &str| {
        SchemaEngine::default()
            .lower_source_with_resolver(
                source,
                SchemaIdentity::new("import-consumer", "0.1.0"),
                &mut MacroContext::default(),
                &resolver,
            )
            .expect("consumer schema lowers")
    };

    let schema = lower(&consumer_source);
    let closure = schema.family_closure("Output").expect("output closure");
    let imports = closure
        .imports()
        .iter()
        .map(|import| import.local_name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(imports, ["DatabaseMarker"]);

    let again = lower(&consumer_source);
    assert_eq!(
        closure.content_hash().expect("closure hashes"),
        again
            .family_closure("Output")
            .expect("output closure")
            .content_hash()
            .expect("closure hashes"),
    );
}
