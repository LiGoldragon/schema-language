//! Design-illustrating tests for the schema stack.
//!
//! Each test illustrates ONE load-bearing design point with a short
//! fixture and a focused assertion. Test names start with
//! `design_example_` so a reader scanning the file knows which tests
//! are for design representation vs broader coverage.
//!
//! Companion to `tests/lowering.rs` (the broader test surface). When
//! a design report cites a test, the test in this file should be the
//! canonical example.

use nota::{Document, StructureShape};
use schema_language::{
    EnumDeclaration, MacroContext, Name, Root, SchemaEngine, SchemaError, SchemaIdentity,
    SchemaNode, SchemaNodeData, SchemaNodeValue, TypeDeclaration, TypeReference,
};

/// The enum body of a root known to be the enum-body form — the shape of
/// every root in these design fixtures.
fn root_enum(root: Root) -> EnumDeclaration {
    root.as_enum().cloned().expect("root is the enum-body form")
}

/// Illustrates: a schema document is positional and has exactly six root
/// slots: imports, input, output, types, generics, and impls. Empty optional
/// roots are still present as `{}` or `[]`; slot omission is not inference.
#[test]
fn design_example_schema_document_has_six_strict_roots() {
    let too_few = "[]";
    let error = SchemaEngine::default()
        .lower_source(too_few, SchemaIdentity::new("example", "0.1.0"))
        .expect_err("one root object should fail");
    assert_eq!(
        error,
        SchemaError::ExpectedRootObjectCount {
            expected: "6 root slots (imports input output types generics impls)",
            found: 1,
        }
    );

    let too_many = "{} [] [] {} {} {} []";
    let error = SchemaEngine::default()
        .lower_source(too_many, SchemaIdentity::new("example", "0.1.0"))
        .expect_err("seven root objects should fail");
    assert_eq!(
        error,
        SchemaError::ExpectedRootObjectCount {
            expected: "6 root slots (imports input output types generics impls)",
            found: 7,
        }
    );

    SchemaEngine::default()
        .lower_source("{} [] [] {} {} {}", SchemaIdentity::new("example", "0.1.0"))
        .expect("six-root schema lowers");
}

/// Illustrates: the schema namespace is an honest brace key/value map.
/// Each declaration is two objects: the type name key and the definition
/// value. The declaration no longer repeats its name inside the value object.
///
/// This is the positive complement of
/// `brace_namespace_rejects_parenthesized_named_objects` in
/// `lowering.rs` — that test PROVES the rejection; this test PROVES
/// the pair-style positive path.
#[test]
fn design_example_namespace_brace_contains_key_value_declarations() {
    let source = "{} [] [] { Topic.String Kind.[Decision Constraint] } {} {}";
    let schema = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("example", "0.1.0"))
        .expect("key/value namespace lowers");

    let namespace = schema.namespace();
    let names: Vec<&str> = namespace
        .iter()
        .map(|declaration| declaration.name().as_str())
        .collect();
    assert_eq!(names, vec!["Topic", "Kind"]);

    let TypeDeclaration::Newtype(topic) = namespace[0].value() else {
        panic!("Topic should lower as a newtype");
    };
    assert_eq!(topic.reference, TypeReference::String);
    let TypeDeclaration::Enum(kind) = namespace[1].value() else {
        panic!("Kind should lower as an enum");
    };
    let variant_names: Vec<&str> = kind
        .variants
        .iter()
        .map(|variant| variant.name.as_str())
        .collect();
    assert_eq!(variant_names, vec!["Decision", "Constraint"]);
}

/// Illustrates: a colon-qualified name like `schema:spirit:Entry`
/// decomposes into ordered segments by single-colon, and `local_part`
/// returns the final segment.
///
/// Intent records 895 + 902 (Maximum / High): namespace separator is
/// a SINGLE colon mirroring Rust crate:module:Type structure (not
/// Rust's double-colon).
///
/// Focused complement of `colon_qualified_names_lower_as_schema_names`
/// in `lowering.rs` — that test exercises colon names through a full
/// lowering; this one isolates the `Name` decomposition method
/// without parsing a schema.
#[test]
fn design_example_colon_qualified_name_decomposes_into_segments() {
    let qualified = Name::new("schema:spirit:Entry");

    assert_eq!(
        qualified.namespace_segments(),
        vec!["schema", "spirit", "Entry"]
    );
    assert_eq!(qualified.local_part(), "Entry");
    assert_eq!(qualified.field_name(), "entry");

    let bare = Name::new("Topic");
    assert_eq!(bare.namespace_segments(), vec!["Topic"]);
    assert_eq!(bare.local_part(), "Topic");
    assert_eq!(bare.field_name(), "topic");
}

/// Illustrates: the schema engine consumes the NOTA first-pass
/// structure header. The header is recorded before semantic macro
/// lowering so macro dispatch can be tested against the same compact
/// first-two-level shape witness that will later feed signal-style
/// triage.
#[test]
fn design_example_schema_lowering_records_source_structure_header() {
    let source = "{} [Record.Entry] [Accepted] { Value.String Entry.{ Value } } {} {}";
    let mut context = MacroContext::default();
    SchemaEngine::default()
        .lower_source_with_context(
            source,
            SchemaIdentity::new("example", "0.1.0"),
            &mut context,
        )
        .expect("schema lowers");

    let header = context
        .structure_headers()
        .first()
        .expect("schema lowering records the source structure header");
    let observed: Vec<(StructureShape, u8)> = header
        .slots()
        .iter()
        .map(|slot| (slot.shape(), slot.child_count()))
        .collect();

    assert_eq!(
        observed,
        vec![
            (StructureShape::Document, 6),
            (StructureShape::Brace, 0),
            (StructureShape::SquareBracket, 1),
            (StructureShape::Application, 0),
            (StructureShape::SquareBracket, 1),
            (StructureShape::Atom, 0),
            (StructureShape::Brace, 2),
            (StructureShape::Unknown, 15),
        ],
    );
    assert_ne!(header.packed_word(), 0, "header packs into a u64 word");
}

/// Illustrates: a schema-node macro call is data. `(Normalize [Topic])`
/// parses as a tagged node named `Normalize` carrying a vector data payload
/// containing the symbol `Topic`. No sigil is needed because this is
/// read at a known schema-node position.
#[test]
fn design_example_schema_node_macro_call_is_tagged_data() {
    let document = Document::parse("(Normalize [Topic])").expect("nota parses");
    let node = SchemaNode::from_block(document.root_object_at(0).expect("macro node"))
        .expect("schema node parses");

    assert_eq!(node.tag().as_str(), "Normalize");
    assert_eq!(
        node.data(),
        &SchemaNodeData::Vector(vec![SchemaNodeValue::Symbol(Name::new("Topic"))])
    );
}

/// Illustrates: root enum payloads are authored directly inside the
/// known root enum body. Payload-carrying reference variants use `Variant.Payload`;
/// unit variants use bare symbols.
#[test]
fn design_example_root_enum_uses_direct_variant_shapes() {
    let source = "{} [Record.Entry Drop] [] {} {} {}";

    let schema = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("example", "0.1.0"))
        .expect("direct variants lower");

    let input = root_enum(schema.input());
    let variants: Vec<(&str, Option<&str>)> = input
        .variants
        .iter()
        .map(|variant| {
            (
                variant.name.as_str(),
                variant
                    .payload
                    .as_ref()
                    .map(|payload| payload.plain_name().expect("plain payload").as_str()),
            )
        })
        .collect();

    assert_eq!(variants, vec![("Record", Some("Entry")), ("Drop", None)]);
}

/// Illustrates: same-name payload variants are rejected because the
/// variant and direct payload type collapse in text projection. Use a
/// distinct payload type name such as `RecordLeaf` instead.
#[test]
fn design_example_same_name_payload_variant_is_rejected() {
    let source = std::fs::read_to_string("tests/fixtures/design/same-name-payload-variant.schema")
        .expect("read same-name payload fixture");
    let error = SchemaEngine::default()
        .lower_source(&source, SchemaIdentity::new("example", "0.1.0"))
        .expect_err("self-tagged same-name variants are rejected");

    assert_eq!(
        error,
        schema_language::SchemaError::SameNamedVariantPayload {
            enum_name: "Input".to_owned(),
            variant_name: "Record".to_owned(),
            payload_type: "Record".to_owned(),
        }
    );
}

/// Illustrates: the same schema language names the three runtime
/// planes. Signal roots remain the schema's Input/Output, while
/// Nexus and SEMA vocabularies are ordinary schema objects in the
/// namespace until the plane-specific file split lands.
///
/// Intent records 964 and 965 rename the execution plane to Nexus
/// and classify Signal, Nexus, and SEMA as schema-driven planes.
#[test]
fn design_example_signal_nexus_and_sema_are_schema_declared_planes() {
    let source = "
        {}
        [Record.Entry Observe.Query]
        [RecordAccepted.RecordIdentifier RecordsObserved.RecordSet]
        {
          NexusInput.[Signal.Input Sema.SemaOutput]
          NexusOutput.[Sema.SemaInput Signal.Output]
          SemaInput.[Record.Entry Observe.Query]
          SemaOutput.[Recorded.RecordIdentifier Observed.RecordSet]
          Topic.String
          RecordIdentifier.Integer
          Entry.{ Topic }
          Query.{ Topic }
          RecordSet.Vector.Entry
        }
        {}
        {}
    ";
    let schema = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("spirit-next:lib", "0.1.0"))
        .expect("schema planes lower");

    assert_eq!(schema.input().name().as_str(), "Input");
    assert_eq!(schema.output().name().as_str(), "Output");

    let namespace = schema.namespace();
    let names: Vec<&str> = namespace
        .iter()
        .map(|declaration| declaration.name().as_str())
        .collect();
    for plane_type in ["NexusInput", "NexusOutput", "SemaInput", "SemaOutput"] {
        assert!(
            names.contains(&plane_type),
            "{plane_type} is declared as schema data, not a hidden runtime enum",
        );
    }
}
