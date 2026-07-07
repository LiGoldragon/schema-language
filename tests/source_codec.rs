use std::fs;

use schema_language::{
    Name, RelationDeclaration, SchemaEngine, SchemaError, SchemaIdentity, SchemaSourceArtifact,
    SourceDeclaration, SourceDeclarationValue, SourceDeclarations, SourceField, SourceReference,
    SourceStructBody, SourceVariantSignature, TypeDeclaration, TypeReference,
};

fn source_codec_fixture(name: &str) -> String {
    fs::read_to_string(format!("tests/fixtures/source-codec/{name}.schema"))
        .unwrap_or_else(|error| panic!("read source-codec schema fixture {name}: {error}"))
        .trim_end()
        .to_owned()
}

#[test]
fn schema_source_artifact_round_trips_module_source_text() {
    let source = fs::read_to_string("tests/fixtures/spirit-crate/schema/lib.schema")
        .expect("read schema source fixture");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");
    let canonical = artifact.to_schema_text();
    let recovered = SchemaSourceArtifact::from_schema_text(&canonical)
        .expect("canonical schema source decodes");

    assert_eq!(
        artifact, recovered,
        "canonical schema source text should recover the same source object"
    );
    assert_eq!(
        "{}\n[Record Observe]\n[RecordAccepted RecordsObserved]\n{\n  Record Entry\n  Observe Query\n  RecordAccepted RecordIdentifier\n  RecordsObserved RecordSet\n  Topic String\n  Topics (Vector Topic)\n  Description String\n  RecordIdentifier Integer\n  Entry { Topics Kind Description Magnitude }\n  Query { Topic Kind }\n  RecordSet (Vector Entry)\n  Kind [Decision Principle Correction Clarification Constraint]\n  Magnitude [Minimum VeryLow Low Medium High VeryHigh Maximum]\n}",
        canonical,
        "source codec should write one canonical schema source surface"
    );
}

#[test]
fn schema_source_lowers_through_engine_schema_source_endpoint() {
    let source = fs::read_to_string("tests/fixtures/spirit-crate/schema/lib.schema")
        .expect("read schema source fixture");
    let identity = SchemaIdentity::new("spirit-next:lib", "0.1.0");
    let engine = SchemaEngine::default();
    let source_artifact =
        SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");
    let through_endpoint = engine
        .lower_schema_source(source_artifact.source(), identity.clone())
        .expect("schema source endpoint lowers");
    let through_object = source_artifact
        .source()
        .lower(&engine, identity)
        .expect("schema source object lowers");

    assert_eq!(
        through_endpoint, through_object,
        "schema source object and engine endpoint should lower the same typed schema"
    );
}

#[test]
fn reheaded_source_declarations_round_trip_help_forms() {
    let declarations = SourceDeclarations::from_schema_text(
        "(Record { Entry Justification })\n(IntentEventStream (Stream { token.SubscriptionToken opened.SubscriptionStarted event.IntentEvent close.SubscriptionToken }))",
    )
    .expect("help declaration document decodes");
    assert_eq!(
        declarations.to_schema_text(),
        "(Record { Entry Justification })\n(IntentEventStream (Stream { token.SubscriptionToken opened.SubscriptionStarted event.IntentEvent close.SubscriptionToken }))"
    );

    let record = SourceDeclaration::new(
        Name::new("Record"),
        Some(SourceDeclarationValue::Struct(SourceStructBody::new(vec![
            SourceField::derived(Name::new("Entry")),
            SourceField::derived(Name::new("Justification")),
        ]))),
    );
    let kind = SourceDeclaration::new(
        Name::new("Kind"),
        Some(SourceDeclarationValue::Enum(
            schema_language::SourceEnumBody::new(vec![
                SourceVariantSignature::from_name(Name::new("Decision")),
                SourceVariantSignature::from_name(Name::new("Principle")),
            ]),
        )),
    );
    let domains = SourceDeclaration::new(
        Name::new("Domains"),
        Some(SourceDeclarationValue::Reference(SourceReference::Vector(
            Box::new(SourceReference::Plain(Name::new("Domain"))),
        ))),
    );
    let version = SourceDeclaration::new(Name::new("Version"), None);
    let encoded = SourceDeclarations::new(vec![record, kind, domains, version]).to_schema_text();
    let decoded =
        SourceDeclarations::from_schema_text(&encoded).expect("encoded declarations decode");

    assert_eq!(decoded.to_schema_text(), encoded);
    assert_eq!(
        encoded,
        "(Record { Entry Justification })\n(Kind [Decision Principle])\n(Domains (Vector Domain))\n(Version)"
    );
}

#[test]
fn schema_source_reference_fields_lower_to_canonical_field_names() {
    let source = source_codec_fixture("reference-fields");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");
    let schema = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("example:lib", "0.1.0"),
        )
        .expect("schema source lowers");
    let Some(TypeDeclaration::Struct(entry)) = schema.type_named("Entry") else {
        panic!("Entry should lower to a struct");
    };

    let field_names = entry
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        field_names,
        vec!["record_identifier", "by_topic"],
        "schema-source lowering must preserve canonical derived field names"
    );
}

#[test]
fn schema_source_explicit_structural_fields_round_trip() {
    let source = "{}\n[]\n[]\n{\n  Topic String\n  Query { Topics.(Vector Topic) Limit.(Optional Integer) }\n}";
    let artifact = SchemaSourceArtifact::from_schema_text(source).expect("schema source decodes");
    let canonical = artifact.to_schema_text();
    let recovered = SchemaSourceArtifact::from_schema_text(&canonical)
        .expect("canonical schema source decodes");
    let schema = recovered
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("example:lib", "0.1.0"),
        )
        .expect("schema source lowers");
    let Some(TypeDeclaration::Struct(query)) = schema.type_named("Query") else {
        panic!("Query should lower to a struct");
    };

    assert_eq!(
        canonical,
        "{}\n[]\n[]\n{\n  Topic String\n  Query { Topics.(Vector Topic) Limit.(Optional Integer) }\n}"
    );
    assert_eq!(query.fields[0].name.as_str(), "topics");
    assert_eq!(query.fields[1].name.as_str(), "limit");
}

#[test]
fn schema_source_exposes_one_level_help_projection_inputs() {
    let source = "{}\n[(Record { Entry Justification })]\n[RecordAccepted]\n{\n  Entry { Domains Kind Description }\n  Domains (Vector Domain)\n  Description String\n  Kind [Decision Principle]\n  RecordIdentifier String\n}";
    let artifact = SchemaSourceArtifact::from_schema_text(source).expect("schema source decodes");
    let input = artifact
        .source()
        .input()
        .body()
        .as_enum()
        .expect("input root enum");
    let record = &input.variants()[0];
    let Some(schema_language::SourceVariantPayload::Declaration(
        schema_language::SourceDeclarationValue::Struct(record_payload),
    )) = record.payload_source()
    else {
        panic!("Record should expose its inline struct payload source");
    };
    let field_text = record_payload
        .fields()
        .iter()
        .map(|field| field.value().to_schema_text())
        .collect::<Vec<_>>();

    assert_eq!(field_text, vec!["*", "*"]);

    let namespace = artifact.source().namespace();
    let domains = namespace
        .entries()
        .iter()
        .find(|entry| entry.name().as_str() == "Domains")
        .expect("Domains declaration");
    let Some(schema_language::SourceDeclarationValue::Reference(reference)) = domains.value()
    else {
        panic!("Domains should expose its reference body");
    };

    assert_eq!(reference.to_schema_text(), "(Vector Domain)");
}

#[test]
fn nested_namespace_router_envelope_round_trips_and_lowers() {
    let source = source_codec_fixture("nested-router-namespace");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");
    let canonical = artifact.to_schema_text();
    let recovered = SchemaSourceArtifact::from_schema_text(&canonical)
        .expect("canonical nested namespace source decodes");
    let recovered_from_binary = SchemaSourceArtifact::from_binary_bytes(
        &artifact
            .to_binary_bytes()
            .expect("nested namespace source archives"),
    )
    .expect("nested namespace source decodes from archive");

    assert_eq!(
        artifact, recovered,
        "canonical nested namespace source should recover the same typed source"
    );
    assert_eq!(
        artifact, recovered_from_binary,
        "source-level nested namespace data should survive the rkyv archive boundary"
    );
    assert!(
        canonical.contains(
            "Envelope { Destination Contract Operation Exchange PayloadSize PayloadOctets }"
        ),
        "canonical source keeps the envelope fields positional and bare"
    );
    assert!(
        !canonical.contains("Destination *"),
        "canonical source must not reintroduce the retired star shorthand"
    );
    assert!(
        !canonical.contains("destination ActorIdentifier"),
        "canonical source must not reintroduce key/value-style field labels"
    );

    let schema = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("router:signal", "0.1.0"),
        )
        .expect("nested namespace source lowers");
    let schema_from_canonical = recovered
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("router:signal", "0.1.0"),
        )
        .expect("canonical nested namespace source lowers");

    assert_eq!(
        schema.content_hash(),
        schema_from_canonical.content_hash(),
        "source projection details must not move semantic content identity"
    );
    assert!(
        schema.type_named("router:routed_object:Envelope").is_some(),
        "nested Envelope should flatten to a fully qualified schema type"
    );
    assert!(
        schema.type_named("Envelope").is_none(),
        "nested local names must not leak into the top-level type namespace"
    );

    let Some(TypeDeclaration::Struct(envelope)) =
        schema.type_named("router:routed_object:Envelope")
    else {
        panic!("router envelope should lower to a struct");
    };
    assert_eq!(
        envelope
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "destination",
            "contract",
            "operation",
            "exchange",
            "payload_size",
            "payload_octets"
        ],
        "field names derive from local role types inside the namespace"
    );
    assert_eq!(
        envelope
            .fields
            .iter()
            .map(|field| &field.reference)
            .collect::<Vec<_>>(),
        vec![
            &TypeReference::Plain(schema_language::Name::new(
                "router:routed_object:Destination"
            )),
            &TypeReference::Plain(schema_language::Name::new("router:routed_object:Contract")),
            &TypeReference::Plain(schema_language::Name::new("router:routed_object:Operation")),
            &TypeReference::Plain(schema_language::Name::new("router:routed_object:Exchange")),
            &TypeReference::Plain(schema_language::Name::new(
                "router:routed_object:PayloadSize"
            )),
            &TypeReference::Plain(schema_language::Name::new(
                "router:routed_object:PayloadOctets"
            )),
        ],
        "local field types resolve to fully qualified semantic references"
    );

    let Some(TypeDeclaration::Newtype(destination)) =
        schema.type_named("router:routed_object:Destination")
    else {
        panic!("Destination should lower to a namespaced newtype");
    };
    assert_eq!(
        destination.reference,
        TypeReference::Plain(schema_language::Name::new("ActorIdentifier")),
        "namespaced declarations can still reference top-level shared types"
    );
}

#[test]
fn namespace_enum_bare_variants_do_not_resolve_to_same_named_payloads() {
    let source = source_codec_fixture("namespace-enum-bare-variants");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");
    let schema = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("example:lib", "0.1.0"),
        )
        .expect("schema source lowers");
    let Some(TypeDeclaration::Enum(kind)) = schema.type_named("Kind") else {
        panic!("Kind should lower to an enum");
    };

    let variants = kind
        .variants
        .iter()
        .map(|variant| (variant.name.as_str(), variant.payload.as_ref()))
        .collect::<Vec<_>>();
    assert_eq!(
        variants,
        vec![("Decision", None), ("Correction", None)],
        "bare namespace enum variants stay unit variants even when same-named schema types exist"
    );
}

#[test]
fn namespace_inline_enum_variant_declarations_are_public_payload_types() {
    let source = source_codec_fixture("namespace-inline-enum-variants");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");
    let schema = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("example:lib", "0.1.0"),
        )
        .expect("schema source lowers");

    assert_eq!(
        schema
            .namespace()
            .iter()
            .map(|declaration| (declaration.name().as_str(), declaration.visibility()))
            .collect::<Vec<_>>(),
        vec![
            ("Craft", schema_language::Visibility::Public),
            ("Information", schema_language::Visibility::Public),
            ("Domain", schema_language::Visibility::Public),
            ("Entry", schema_language::Visibility::Public),
        ],
        "inline enum variants exposed through a public namespace enum must be public payload types"
    );
    let Some(TypeDeclaration::Enum(domain)) = schema.type_named("Domain") else {
        panic!("Domain should lower to an enum");
    };
    assert_eq!(
        domain
            .variants
            .iter()
            .map(|variant| {
                (
                    variant.name.as_str(),
                    variant
                        .payload
                        .as_ref()
                        .and_then(schema_language::TypeReference::plain_name)
                        .map(schema_language::Name::as_str),
                )
            })
            .collect::<Vec<_>>(),
        vec![
            ("Craft", Some("Craft")),
            ("Information", Some("Information"))
        ]
    );
}

#[test]
fn root_header_bare_names_resolve_to_exported_namespace_payloads() {
    let source = source_codec_fixture("root-header-bare-names");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");
    let schema = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("example:lib", "0.1.0"),
        )
        .expect("schema source lowers");

    let input = schema
        .input()
        .as_enum()
        .expect("input is the enum-body form");
    assert_eq!(input.variants[0].name.as_str(), "Lookup");
    assert_eq!(
        input.variants[0]
            .payload
            .as_ref()
            .and_then(schema_language::TypeReference::plain_name)
            .map(schema_language::Name::as_str),
        Some("Lookup")
    );
    assert_eq!(input.variants[1].name.as_str(), "Count");
    assert_eq!(
        input.variants[1]
            .payload
            .as_ref()
            .and_then(schema_language::TypeReference::plain_name)
            .map(schema_language::Name::as_str),
        Some("Count")
    );
    assert!(
        schema.type_named("Lookup").is_some(),
        "root header should resolve through the exported namespace object"
    );
    let Some(TypeDeclaration::Newtype(lookup)) = schema.type_named("Lookup") else {
        panic!("bare namespace binding should lower to a newtype");
    };
    assert_eq!(
        lookup
            .reference
            .plain_name()
            .map(schema_language::Name::as_str),
        Some("RecordIdentifier")
    );
}

#[test]
fn root_header_inline_declarations_are_exported_namespace_payloads() {
    let source = source_codec_fixture("root-inline-payloads");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");
    let schema = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("example:lib", "0.1.0"),
        )
        .expect("schema source lowers");

    assert!(
        schema.type_named("Lookup").is_some(),
        "inline root declaration should enter the exported namespace"
    );
    assert!(
        schema.type_named("Count").is_some(),
        "second inline root declaration should enter the exported namespace"
    );
    assert_eq!(
        schema
            .input()
            .as_enum()
            .expect("input is the enum-body form")
            .variants[0]
            .payload
            .as_ref()
            .and_then(schema_language::TypeReference::plain_name)
            .map(schema_language::Name::as_str),
        Some("Lookup")
    );
    assert_eq!(
        schema
            .namespace()
            .iter()
            .map(|declaration| (declaration.name().as_str(), declaration.visibility()))
            .collect::<Vec<_>>(),
        vec![
            ("RecordIdentifier", schema_language::Visibility::Public),
            ("Query", schema_language::Visibility::Public),
            ("Topic", schema_language::Visibility::Public),
            ("Lookup", schema_language::Visibility::Public),
            ("Count", schema_language::Visibility::Public),
        ]
    );
}

#[test]
fn root_payload_field_declarations_are_exported_namespace_types() {
    let source = source_codec_fixture("root-payload-field-declarations");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");
    let schema = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("example:lib", "0.1.0"),
        )
        .expect("schema source lowers");

    assert_eq!(
        schema
            .namespace()
            .iter()
            .map(|declaration| (declaration.name().as_str(), declaration.visibility()))
            .collect::<Vec<_>>(),
        vec![
            ("Topic", schema_language::Visibility::Public),
            ("Description", schema_language::Visibility::Public),
            ("Record", schema_language::Visibility::Public),
        ]
    );
    let Some(TypeDeclaration::Newtype(topic)) = schema.type_named("Topic") else {
        panic!("Topic should lower to a public newtype");
    };
    assert_eq!(topic.reference, schema_language::TypeReference::String);
    let Some(TypeDeclaration::Struct(record)) = schema.type_named("Record") else {
        panic!("Record should lower to a public struct");
    };
    assert_eq!(
        record
            .fields
            .iter()
            .map(|field| {
                (
                    field.name.as_str(),
                    field
                        .reference
                        .plain_name()
                        .map(schema_language::Name::as_str),
                )
            })
            .collect::<Vec<_>>(),
        vec![
            ("topic", Some("Topic")),
            ("description", Some("Description"))
        ]
    );
}

#[test]
fn later_inline_payloads_resolve_root_payload_field_declarations() {
    let source = source_codec_fixture("later-inline-payloads");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");
    let schema = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("example:lib", "0.1.0"),
        )
        .expect("schema source lowers");

    assert_eq!(
        schema
            .namespace()
            .iter()
            .map(|declaration| (declaration.name().as_str(), declaration.visibility()))
            .collect::<Vec<_>>(),
        vec![
            ("Topic", schema_language::Visibility::Public),
            ("Description", schema_language::Visibility::Public),
            ("Record", schema_language::Visibility::Public),
            ("ByTopic", schema_language::Visibility::Private),
            ("ByDescription", schema_language::Visibility::Private),
            ("Select", schema_language::Visibility::Public),
        ]
    );
    let Some(TypeDeclaration::Newtype(by_topic)) = schema.type_named("ByTopic") else {
        panic!("ByTopic should lower to a private newtype helper");
    };
    assert_eq!(
        by_topic
            .reference
            .plain_name()
            .map(schema_language::Name::as_str),
        Some("Topic")
    );
}

#[test]
fn trailing_namespace_can_reference_root_payload_field_declarations() {
    let source = source_codec_fixture("trailing-namespace-reference");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");
    let schema = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("example:lib", "0.1.0"),
        )
        .expect("schema source lowers");

    let Some(TypeDeclaration::Newtype(wrapper)) = schema.type_named("Wrapper") else {
        panic!("Wrapper should lower to a public newtype");
    };
    assert_eq!(
        wrapper
            .reference
            .plain_name()
            .map(schema_language::Name::as_str),
        Some("Topic")
    );
}

#[test]
fn duplicate_inline_and_namespace_declarations_are_errors() {
    let source = source_codec_fixture("duplicate-inline-and-namespace");
    let error = SchemaSourceArtifact::from_schema_text(&source)
        .expect_err("retired inline pair syntax should fail before lowering");

    assert!(
        matches!(
            error,
            SchemaError::MalformedSchemaNode { ref found }
                if found.contains("retired struct field syntax topic")
        ),
        "got {error:?}"
    );
}

#[test]
fn duplicate_inline_declarations_are_errors() {
    let source = source_codec_fixture("duplicate-inline-fields");
    let artifact = SchemaSourceArtifact::from_schema_text(&source)
        .expect("duplicate inline declaration source still parses");
    let error = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("example:duplicate-inline", "0.1.0"),
        )
        .expect_err("duplicate inline declaration should fail during lowering");

    assert_eq!(
        error,
        SchemaError::DuplicateSourceDeclaration {
            name: "Record".to_owned(),
        }
    );
}

#[test]
fn redundant_dot_field_roles_are_errors() {
    let source = "{}\n[]\n[]\n{\n  Topic String\n  Entry { topic.Topic }\n}";
    let error = SchemaSourceArtifact::from_schema_text(source)
        .expect_err("redundant explicit field role should fail before lowering");
    let rendered = error.to_string();

    assert!(
        rendered.contains("redundant explicit field role topic.Topic")
            && rendered.contains("just use Topic"),
        "got {error:?}"
    );
}

#[test]
fn schema_source_artifact_round_trips_through_binary_archive() {
    let source = source_codec_fixture("root-inline-payloads");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");
    let bytes = artifact
        .to_binary_bytes()
        .expect("schema source artifact archives");
    let recovered =
        SchemaSourceArtifact::from_binary_bytes(&bytes).expect("schema source artifact restores");

    assert_eq!(artifact, recovered);
    assert_eq!(recovered.to_schema_text(), source);
}

#[test]
fn schema_source_lowers_relation_declarations() {
    let source = source_codec_fixture("relations");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");

    assert_eq!(
        artifact.to_schema_text(),
        source,
        "relation declarations should round-trip through canonical schema source"
    );

    let bytes = artifact
        .to_binary_bytes()
        .expect("schema source artifact archives");
    let recovered =
        SchemaSourceArtifact::from_binary_bytes(&bytes).expect("schema source artifact restores");
    assert_eq!(artifact, recovered);

    let schema = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("example:domain", "0.1.0"),
        )
        .expect("schema source lowers");

    assert_eq!(schema.relations().len(), 2);
    let RelationDeclaration::Equivalence(values) = &schema.relations()[0];
    let paths = values
        .iter()
        .map(|value| {
            value
                .path()
                .iter()
                .map(schema_language::Name::as_str)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        vec![
            vec!["Technology", "Hardware", "Networking"],
            vec!["Technology", "Software", "Distributed", "Networking"]
        ],
        "equivalence values lower as schema-name paths"
    );
}

#[test]
fn schema_source_lowers_stream_declarations_and_variant_relations() {
    let source = source_codec_fixture("stream-relations");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");

    assert_eq!(
        artifact.to_schema_text(),
        source,
        "stream declarations and stream variant relations encode as schema source"
    );

    let schema = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("example:lib", "0.1.0"),
        )
        .expect("schema source lowers");

    assert_eq!(schema.streams().len(), 1);
    let stream = &schema.streams()[0];
    assert_eq!(stream.name.as_str(), "RecordStream");
    assert_eq!(
        stream.token.plain_name().map(schema_language::Name::as_str),
        Some("SubscriptionToken")
    );
    assert_eq!(
        stream
            .opened
            .plain_name()
            .map(schema_language::Name::as_str),
        Some("SubscriptionReceipt")
    );
    assert_eq!(
        stream.event.plain_name().map(schema_language::Name::as_str),
        Some("RuntimeEvent")
    );
    assert_eq!(
        stream.close.plain_name().map(schema_language::Name::as_str),
        Some("SubscriptionToken")
    );
    assert!(
        schema.type_named("RecordStream").is_none(),
        "stream declarations are schema metadata, not namespace data types"
    );

    let watch_relation = schema
        .input()
        .as_enum()
        .expect("input is the enum-body form")
        .variants[0]
        .stream_relation
        .as_ref()
        .expect("Watch opens a stream");
    assert!(matches!(
        watch_relation,
        schema_language::StreamRelation::Opens(name) if name.as_str() == "RecordStream"
    ));

    let Some(TypeDeclaration::Enum(runtime_event)) = schema.type_named("RuntimeEvent") else {
        panic!("RuntimeEvent should lower to an enum");
    };
    let event_relation = runtime_event.variants[0]
        .stream_relation
        .as_ref()
        .expect("RecordChanged belongs to a stream");
    assert!(matches!(
        event_relation,
        schema_language::StreamRelation::Belongs(name) if name.as_str() == "RecordStream"
    ));
}

#[test]
fn source_enum_variants_are_typed_structural_macro_nodes() {
    let source = source_codec_fixture("structural-variant-nodes");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");

    assert_eq!(
        artifact.to_schema_text(),
        source,
        "structural enum variant nodes encode back to the same schema source surface"
    );

    let input_variants = artifact
        .source()
        .input()
        .body()
        .as_enum()
        .expect("source input is the enum-body form")
        .variants();
    assert_eq!(input_variants[0].name().as_str(), "Reserved");
    assert_eq!(input_variants[0].payload(), None);
    assert_eq!(input_variants[1].name().as_str(), "Record");
    assert_eq!(
        input_variants[1].payload(),
        Some(&schema_language::SourceReference::Plain(
            schema_language::Name::new("Entry")
        ))
    );
    assert_eq!(input_variants[2].name().as_str(), "Inline");
    assert_eq!(
        input_variants[2].payload(),
        None,
        "inline declaration payload is not a reference at the source layer"
    );

    let schema = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("example:lib", "0.1.0"),
        )
        .expect("schema source lowers");
    let variants = schema
        .input()
        .as_enum()
        .expect("input is the enum-body form")
        .variants
        .iter()
        .map(|variant| {
            (
                variant.name.as_str(),
                variant
                    .payload
                    .as_ref()
                    .and_then(schema_language::TypeReference::plain_name)
                    .map(schema_language::Name::as_str),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        variants,
        vec![
            ("Reserved", None),
            ("Record", Some("Entry")),
            ("Inline", Some("Inline")),
        ],
        "lowering happens after structural variant selection"
    );
    assert!(
        schema.type_named("Inline").is_some(),
        "inline structural payload is exported as the variant's same-named type"
    );
}

#[test]
fn source_enum_variant_reports_structural_macro_expected_shapes() {
    let source = source_codec_fixture("unsupported-three-object-variant");
    let error = SchemaSourceArtifact::from_schema_text(&source)
        .expect_err("three-object variant signature is not a supported structural case");

    let SchemaError::UnsupportedMacroNodeStructure {
        position,
        expected,
        found,
    } = error
    else {
        panic!("expected structural macro-node error, got {error:?}");
    };

    assert_eq!(position, "SourceVariantSignature");
    assert_eq!(found, "parenthesis");
    assert!(
        expected.iter().any(|case| case.contains("Unit")),
        "diagnostic names the unit structural case"
    );
    assert!(
        expected.iter().any(|case| case.contains("Data")),
        "diagnostic names the data structural case"
    );
    assert!(
        expected.iter().any(|case| case.contains("Streaming")),
        "diagnostic names the streaming structural case"
    );
}
