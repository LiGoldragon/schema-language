//! Rendering the per-instance schema trace through the schema encoder.
//!
//! Each test decodes a real value with the decoder-driven `NotaDecodeTraced`
//! (no hand walk), renders the captured trace with `InstanceSchemaText` (every
//! reference token through the schema encoder), asserts the rendered text
//! matches the endorsed form, and round-trips the rendered reference tokens
//! back through schema's `SourceReference::from_block`.

use nota::{InstanceSchema, NotaDecodeTraced, NotaSource};
use schema_language::{InstanceSchemaText, SourceReference};

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
enum Magnitude {
    Zero,
    Low,
    Medium,
    High,
}

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct Certainty(Magnitude);

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct Importance(Magnitude);

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct Privacy(Magnitude);

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
enum Kind {
    Decision,
    Principle,
    Constraint,
}

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct Description(String);

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct Referent(String);

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct Referents(Vec<Referent>);

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
enum Programming {
    CodeGeneration,
    Parsing,
}

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
enum Software {
    Programming(Programming),
    Theory,
}

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
enum Technology {
    Software(Software),
}

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
enum Domain {
    Technology(Technology),
}

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct Domains(Vec<Domain>);

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct DomainScopes(Vec<Domain>);

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct Partial(DomainScopes);

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct Full(DomainScopes);

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
enum DomainMatch {
    Any,
    Partial(Partial),
    Full(Full),
}

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct Entry {
    domains: Domains,
    kind: Kind,
    description: Description,
    certainty: Certainty,
    importance: Importance,
    privacy: Privacy,
    referents: Referents,
}

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct QuoteText(String);

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct Antecedent(String);

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct OptionalAntecedent(Option<Antecedent>);

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct VerbatimQuote {
    quote_text: QuoteText,
    optional_antecedent: OptionalAntecedent,
}

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct Testimony(Vec<VerbatimQuote>);

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct Reasoning(String);

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct Justification {
    testimony: Testimony,
    reasoning: Reasoning,
}

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct RecordRequest {
    entry: Entry,
    justification: Justification,
}

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
struct Record(RecordRequest);

#[derive(Debug, PartialEq, Eq, nota::NotaDecode, nota::NotaDecodeTraced)]
enum Input {
    Record(Record),
    Version,
}

fn schema_of<Value>(source: &str) -> InstanceSchema
where
    Value: NotaDecodeTraced,
{
    let block = NotaSource::new(source)
        .parse_root()
        .expect("parse a single root object");
    Value::from_nota_block_traced(&block)
        .expect("decode value and capture its instance schema")
        .into_parts()
        .1
}

/// Every parenthesised reference token the renderer emits must parse back
/// through schema's own reference reader.
fn round_trips_as_reference(text: &str) {
    let block = NotaSource::new(text)
        .parse_root()
        .expect("rendered reference parses as a NOTA root");
    SourceReference::from_block(&block)
        .expect("rendered reference round-trips through SourceReference::from_block");
}

#[test]
fn enum_value_renders_the_enum_name() {
    let schema = schema_of::<Kind>("Decision");
    assert_eq!(InstanceSchemaText::new(&schema).aligned(), "Kind");
    assert_eq!(InstanceSchemaText::new(&schema).expanded(), "Kind");
}

#[test]
fn entry_renders_its_field_type_names() {
    let source = "([(Technology (Software (Programming CodeGeneration)))] Decision [a description] High Medium Zero [spirit])";
    let schema = schema_of::<Entry>(source);
    let rendered = InstanceSchemaText::new(&schema).aligned();
    assert_eq!(
        rendered,
        "{ Domains Kind Description Certainty Importance Privacy Referents }"
    );
}

#[test]
fn domain_match_partial_renders_enum_name_with_payload_reference() {
    let schema = schema_of::<DomainMatch>(
        "(Partial [(Technology (Software (Programming CodeGeneration)))])",
    );
    // The aligned enum payload collapses the transparent `Partial` wrapper to
    // its inner `DomainScopes` newtype name.
    let rendered = InstanceSchemaText::new(&schema).aligned();
    assert_eq!(rendered, "(DomainMatch DomainScopes)");
    round_trips_as_reference("(DomainMatch DomainScopes)");
}

#[test]
fn empty_domains_still_names_its_element_type() {
    let schema = schema_of::<Domains>("[]");
    // Aligned: the newtype wrapper name.
    assert_eq!(InstanceSchemaText::new(&schema).aligned(), "Domains");
    // Expanded: the newtype name plus the (Vector Domain) container reference.
    assert_eq!(
        InstanceSchemaText::new(&schema).expanded(),
        "(Domains (Vector Domain))"
    );
    round_trips_as_reference("(Vector Domain)");
}

#[test]
fn root_input_record_renders_the_endorsed_root_form() {
    let source = "(Record (([(Technology (Software (Programming CodeGeneration)))] Decision [a description] Medium Medium Zero [the spirit]) ([([a quote] None)] [the reasoning])))";
    let schema = schema_of::<Input>(source);
    // The endorsed one-to-one positional root form: enum name, the transparent
    // Record/RecordRequest wrappers collapsed, the payload a paren group of the
    // two aligned struct fields.
    let rendered = InstanceSchemaText::new(&schema).aligned();
    assert_eq!(
        rendered,
        "(Input ({ Domains Kind Description Certainty Importance Privacy Referents } { Testimony Reasoning }))"
    );
}

#[test]
fn certainty_newtype_renders_wrapper_then_magnitude() {
    let schema = schema_of::<Certainty>("High");
    assert_eq!(InstanceSchemaText::new(&schema).aligned(), "Certainty");
    assert_eq!(
        InstanceSchemaText::new(&schema).expanded(),
        "(Certainty Magnitude)"
    );
    round_trips_as_reference("(Certainty Magnitude)");
}
