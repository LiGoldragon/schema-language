use nota::{Document, NotaDecode, NotaEncode};
use schema_language::{Name, SchemaEngine, SchemaIdentity, SymbolPath, SymbolPathPosition};

struct SymbolPathFixture {
    identity: SchemaIdentity,
    source: &'static str,
}

impl SymbolPathFixture {
    fn new() -> Self {
        Self {
            identity: SchemaIdentity::new("spirit-next:lib", "0.1.0"),
            source: "{}\n[Record.Entry]\n[Rejected.SignalRejection]\n{\n  Description.String\n  Entry.{ Description Kind }\n  SignalRejection.{ ValidationError }\n  Kind.[Decision]\n  ValidationError.[EmptyTopic EmptyDescription]\n}\n{}\n{}",
        }
    }

    fn schema(&self) -> schema_language::TrueSchema {
        SchemaEngine::default()
            .lower_source(self.source, self.identity.clone())
            .expect("schema lowers")
    }
}

#[test]
fn schema_derives_canonical_symbol_paths_from_schema_positions() {
    let fixture = SymbolPathFixture::new();
    let schema = fixture.schema();

    assert_eq!(
        schema
            .root_variant_path("Input", "Record")
            .expect("input record variant path"),
        SymbolPath::new([
            Name::new("spirit-next:lib"),
            Name::new("Input"),
            Name::new("Record")
        ])
    );
    assert_eq!(
        schema.type_path("Entry").expect("entry type path"),
        SymbolPath::new([Name::new("spirit-next:lib"), Name::new("Entry")])
    );
    assert_eq!(
        schema
            .field_path("Entry", "description")
            .expect("entry description field path"),
        SymbolPath::new([
            Name::new("spirit-next:lib"),
            Name::new("Entry"),
            Name::new("description")
        ])
    );
    assert_eq!(
        schema
            .enum_variant_path("ValidationError", "EmptyTopic")
            .expect("validation error variant path"),
        SymbolPath::new([
            Name::new("spirit-next:lib"),
            Name::new("ValidationError"),
            Name::new("EmptyTopic")
        ])
    );
}

#[test]
fn schema_resolves_symbol_path_position_roles_from_schema_context() {
    let fixture = SymbolPathFixture::new();
    let schema = fixture.schema();

    let input_record_path = schema
        .root_variant_path("Input", "Record")
        .expect("input record variant path");
    let position = schema
        .symbol_path_position(&input_record_path)
        .expect("input record position");
    let SymbolPathPosition::RootVariant {
        root_name,
        variant_name,
    } = position
    else {
        panic!("expected root variant position");
    };
    assert_eq!(root_name.as_str(), "Input");
    assert_eq!(variant_name.as_str(), "Record");

    let entry_path = schema.type_path("Entry").expect("entry type path");
    let position = schema
        .symbol_path_position(&entry_path)
        .expect("entry type position");
    let SymbolPathPosition::Type { type_name } = position else {
        panic!("expected type position");
    };
    assert_eq!(type_name.as_str(), "Entry");

    let description_path = schema
        .field_path("Entry", "description")
        .expect("entry description field path");
    let position = schema
        .symbol_path_position(&description_path)
        .expect("description field position");
    let SymbolPathPosition::Field {
        type_name,
        field_name,
    } = position
    else {
        panic!("expected field position");
    };
    assert_eq!(type_name.as_str(), "Entry");
    assert_eq!(field_name.as_str(), "description");

    let empty_topic_path = schema
        .enum_variant_path("ValidationError", "EmptyTopic")
        .expect("validation error variant path");
    let position = schema
        .symbol_path_position(&empty_topic_path)
        .expect("empty topic variant position");
    let SymbolPathPosition::EnumVariant {
        enum_name,
        variant_name,
    } = position
    else {
        panic!("expected enum variant position");
    };
    assert_eq!(enum_name.as_str(), "ValidationError");
    assert_eq!(variant_name.as_str(), "EmptyTopic");
}

#[test]
fn schema_rejects_symbol_paths_that_do_not_match_the_schema_context() {
    let fixture = SymbolPathFixture::new();
    let schema = fixture.schema();

    let foreign_component_path = SymbolPath::new([Name::new("other:lib"), Name::new("Entry")]);
    assert_eq!(schema.symbol_path_position(&foreign_component_path), None);

    let unknown_type_path =
        SymbolPath::new([Name::new("spirit-next:lib"), Name::new("UnknownType")]);
    assert_eq!(schema.symbol_path_position(&unknown_type_path), None);

    let unknown_member_path = SymbolPath::new([
        Name::new("spirit-next:lib"),
        Name::new("Entry"),
        Name::new("missing"),
    ]);
    assert_eq!(schema.symbol_path_position(&unknown_member_path), None);

    let overdeep_path = SymbolPath::new([
        Name::new("spirit-next:lib"),
        Name::new("Entry"),
        Name::new("description"),
        Name::new("extra"),
    ]);
    assert_eq!(schema.symbol_path_position(&overdeep_path), None);
}

#[test]
fn symbol_path_round_trips_through_nota_and_rkyv_as_names_not_free_text() {
    let path = SymbolPath::new([
        Name::new("spirit-next:lib"),
        Name::new("Input"),
        Name::new("Record"),
    ]);

    let nota = path.to_nota();
    assert_eq!(nota, "(SymbolPath [spirit-next:lib Input Record])");
    let document = Document::parse(&nota).expect("symbol path nota parses");
    let decoded =
        SymbolPath::from_nota_block(&document.root_objects()[0]).expect("symbol path decodes");
    assert_eq!(decoded, path);
    assert_eq!(decoded.to_string(), "spirit-next:lib/Input/Record");

    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&path).expect("symbol path archives as rkyv");
    let restored = rkyv::from_bytes::<SymbolPath, rkyv::rancor::Error>(&bytes)
        .expect("symbol path decodes from rkyv");
    assert_eq!(restored, path);
}

#[test]
fn symbol_path_rejects_opaque_string_shapes() {
    let document = Document::parse("(SymbolPath spirit-next:lib/Input/Record)")
        .expect("opaque path shape still parses as nota");
    let _error = SymbolPath::from_nota_block(&document.root_objects()[0])
        .expect_err("symbol path body must be a vector of names");
}
