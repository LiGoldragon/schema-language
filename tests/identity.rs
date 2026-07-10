use schema_language::{ContentHash, SchemaEngine, SchemaIdentity, TrueSchema};

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
";

    fn schema(&self, source: &str) -> TrueSchema {
        SchemaEngine::default()
            .lower_source(source, self.identity.clone())
            .expect("fixture schema lowers")
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
}

#[test]
fn deep_field_type_change_changes_the_core_hash() {
    let fixture = IdentityFixture::new();

    assert_ne!(
        fixture.schema_hash(IdentityFixture::BASE),
        fixture.schema_hash(IdentityFixture::DEEP_CHANGE),
    );
}

#[test]
fn unrelated_declaration_change_changes_the_core_hash() {
    let fixture = IdentityFixture::new();

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
}
