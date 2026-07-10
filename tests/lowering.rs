use schema_language::{
    EnumDeclaration, MacroContext, MacroLibrary, MacroObject, MacroOutput, MacroPosition,
    MacroRegistry, Name, Root, SchemaEngine, SchemaIdentity, SchemaMacroHandler, SchemaPackage,
    SchemaSourceArtifact, TypeDeclaration, TypeReference, Visibility,
};

/// The enum body of a root known to be the enum-body form, for the fixtures
/// in this file whose roots are all `[Variant …]`.
fn root_enum(root: Root) -> EnumDeclaration {
    root.as_enum().cloned().expect("root is the enum-body form")
}

/// The projected type body of the namespace declaration at `index`, owned, so
/// let-else matching does not borrow from a projection temporary.
fn declaration_value(schema: &schema_language::TrueSchema, index: usize) -> TypeDeclaration {
    schema.namespace()[index].value().clone()
}

#[test]
fn lowers_spirit_schema_into_ordered_schema() {
    let source = include_str!("../schemas/spirit-min.schema");
    let artifact = SchemaSourceArtifact::from_schema_text(source).expect("schema source decodes");
    let schema = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("spirit", "0.1.0"),
        )
        .expect("schema lowers");

    assert_eq!(schema.imports().len(), 0);
    assert_eq!(schema.input().name().as_str(), "Input");
    assert_eq!(schema.output().name().as_str(), "Output");
    assert_eq!(
        schema
            .root_enum_named("Input")
            .expect("input root")
            .variants[0]
            .name
            .as_str(),
        "Record"
    );
    assert_eq!(schema.input().name().as_str(), "Input");
    assert_eq!(
        root_enum(schema.input()).variants[0].name.as_str(),
        "Record"
    );
    assert_eq!(
        root_enum(schema.input()).variants[0]
            .payload
            .as_ref()
            .expect("payload")
            .plain_name()
            .expect("plain payload")
            .as_str(),
        "RecordPayload"
    );
    assert_eq!(
        schema
            .namespace()
            .iter()
            .map(|declaration| declaration.name().as_str())
            .collect::<Vec<_>>(),
        vec![
            "RecordPayload",
            "ObservePayload",
            "RecordAcceptedPayload",
            "RecordsObservedPayload",
            "Topic",
            "Topics",
            "Description",
            "RecordIdentifier",
            "Entry",
            "Query",
            "RecordSet",
            "Kind",
            "Magnitude",
        ]
    );
}

#[test]
fn strict_key_value_declarations_lower_to_structs_and_enums() {
    let source = "{} [] [] { Topic.String Entry.{ Topic Kind } Kind.[Decision Constraint] } {} {}";
    let schema = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("example", "0.1.0"))
        .expect("schema lowers");

    assert!(matches!(
        schema.namespace()[1].value(),
        TypeDeclaration::Struct(_)
    ));
    assert!(matches!(
        schema.namespace()[2].value(),
        TypeDeclaration::Enum(_)
    ));
}

#[test]
fn bare_reference_declarations_lower_to_newtypes() {
    // The bare `Name Type` form declares a distinct newtype, not a transparent
    // alias (record qz6j: aliases offer no correctness and are not used).
    let source = "{} [] [] { Topic.String Topics.Vector.Topic } {} {}";
    let schema = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("example", "0.1.0"))
        .expect("bare reference forms lower");

    let TypeDeclaration::Newtype(topic) = schema.type_named("Topic").expect("topic type") else {
        panic!("Topic should be a newtype");
    };
    assert_eq!(topic.reference, TypeReference::String);

    let TypeDeclaration::Newtype(topics) = schema.type_named("Topics").expect("topics type") else {
        panic!("Topics should be a newtype");
    };
    assert_eq!(
        topics.reference,
        TypeReference::vector(TypeReference::new("Topic"))
    );
}

#[test]
fn same_named_direct_variant_payloads_are_rejected() {
    for fixture_name in ["self-tagged-variant", "explicit-repeated-variant"] {
        let source =
            std::fs::read_to_string(format!("tests/fixtures/lowering/{fixture_name}.schema"))
                .unwrap_or_else(|error| panic!("read {fixture_name} fixture: {error}"));
        let error = SchemaEngine::default()
            .lower_source(&source, SchemaIdentity::new("example", "0.1.0"))
            .expect_err("same-name payload is rejected");

        assert_eq!(
            error,
            schema_language::SchemaError::SameNamedVariantPayload {
                enum_name: "Input".to_owned(),
                variant_name: "Entry".to_owned(),
                payload_type: "Entry".to_owned(),
            },
            "{fixture_name} should fail with the structural same-name payload error"
        );
    }
}

#[test]
fn distinct_leaf_variant_payload_is_accepted() {
    let source = "{} [Entry.EntryLeaf] [] { Value.String EntryLeaf.{ Value } } {} {}";
    let schema = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("example", "0.1.0"))
        .expect("distinct leaf payload lowers");

    let variant = &root_enum(schema.input()).variants[0];
    assert_eq!(variant.name.as_str(), "Entry");
    assert_eq!(
        variant
            .payload
            .as_ref()
            .expect("payload")
            .plain_name()
            .expect("plain payload")
            .as_str(),
        "EntryLeaf"
    );
}

#[test]
fn bytes_is_a_reserved_scalar_leaf_not_a_declared_name() {
    let schema = SchemaEngine::default()
        .lower_source(
            "{} [] [] { Digest.Bytes } {} {}",
            SchemaIdentity::new("example", "0.1.0"),
        )
        .expect("bytes scalar lowers");

    let TypeDeclaration::Newtype(digest) = schema.type_named("Digest").expect("digest type") else {
        panic!("Digest should lower to a newtype over the Bytes scalar");
    };
    assert_eq!(digest.reference, TypeReference::Bytes);
}

#[test]
fn fixed_size_bytes_lowers_to_a_fixed_bytes_reference() {
    let schema = SchemaEngine::default()
        .lower_source(
            "{} [] [] { Digest.Bytes.32 } {} {}",
            SchemaIdentity::new("example", "0.1.0"),
        )
        .expect("fixed-size bytes lowers");

    let declaration = schema.type_named("Digest").expect("digest type");
    let TypeDeclaration::Newtype(digest) = declaration else {
        panic!("Digest should be a newtype over fixed-size Bytes, got {declaration:?}");
    };
    assert_eq!(digest.reference, TypeReference::fixed_width_bytes(32));
}

#[test]
fn single_field_brace_declarations_lower_to_newtypes() {
    let source = "{} [] [] { Topic.String Entry.{ Topic } Wrapper.{ Topic } } {} {}";
    let schema = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("example", "0.1.0"))
        .expect("single-field brace declarations lower");

    let TypeDeclaration::Newtype(entry) = schema.type_named("Entry").expect("entry type") else {
        panic!("Entry should be a transparent newtype");
    };
    assert_eq!(entry.reference, TypeReference::Plain(Name::new("Topic")));

    let TypeDeclaration::Newtype(wrapper) = schema.type_named("Wrapper").expect("wrapper type")
    else {
        panic!("Wrapper should be a transparent newtype");
    };
    assert_eq!(wrapper.reference, TypeReference::Plain(Name::new("Topic")));
}

#[test]
fn redundant_dot_field_roles_are_rejected() {
    let source = "{} [] [] { Topic.String Entry.{ topic.Topic } } {} {}";
    let error = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("example", "0.1.0"))
        .expect_err("redundant explicit field role is rejected");

    assert_eq!(
        error,
        schema_language::SchemaError::RedundantExplicitFieldRole {
            found: "topic.Topic".to_owned(),
            type_name: "Topic".to_owned(),
        }
    );
}

#[test]
fn optional_enum_variant_payload_is_rejected() {
    // Strict positional NOTA: a variant payload always occupies the text
    // form, so `Optional.T` is forbidden as a variant payload. The optional
    // case must instead be modeled as an explicit member carrying a required
    // payload (for example a leaf enum with an explicit `All` member). Named
    // brace-record fields keep `Optional.T` (see `tests/collections.rs`).
    let source = "{} [] [] { Leaf.String Category.[Plain Sub.Optional.Leaf] } {} {}";
    let error = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("example", "0.1.0"))
        .expect_err("optional enum-variant payload is rejected");

    assert_eq!(
        error,
        schema_language::SchemaError::OptionalVariantPayload {
            enum_name: "Category".to_owned(),
            variant_name: "Sub".to_owned(),
        }
    );
}

#[test]
fn single_field_inline_pascal_declarations_lower_to_newtypes() {
    let source = "{} [] [] { RecordIdentifier.Integer Receipt.{ RecordIdentifier } Entry.{ Receipt } } {} {}";
    let schema = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("example", "0.1.0"))
        .expect("inline single-field declaration lowers");

    assert_eq!(
        schema
            .namespace()
            .iter()
            .map(|declaration| (declaration.name().as_str(), declaration.visibility()))
            .collect::<Vec<_>>(),
        vec![
            ("RecordIdentifier", Visibility::Public),
            ("Receipt", Visibility::Public),
            ("Entry", Visibility::Public),
        ]
    );

    let TypeDeclaration::Newtype(receipt) = schema.type_named("Receipt").expect("receipt type")
    else {
        panic!("Receipt should be a transparent newtype");
    };
    assert_eq!(
        receipt.reference,
        TypeReference::Plain(Name::new("RecordIdentifier"))
    );

    let TypeDeclaration::Newtype(entry) = schema.type_named("Entry").expect("entry type") else {
        panic!("Entry should be a transparent newtype");
    };
    assert_eq!(entry.reference, TypeReference::Plain(Name::new("Receipt")));
}

#[test]
fn brace_namespace_rejects_parenthesized_named_objects() {
    let source = "{} [] [] { (Entry Entry { Topic Kind }) } {} {}";
    let error = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("example", "0.1.0"))
        .expect_err("brace namespaces contain key/value declarations only");

    // The single lowering engine (the typed-source path) rejects a
    // parenthesized named object in the types block: a per-kind entry key must
    // be a dotted-capitalized atom, not a parenthesis.
    assert!(matches!(
        error,
        schema_language::SchemaError::ExpectedSyntaxDeclaration { .. }
            | schema_language::SchemaError::ExpectedSymbol { .. }
            | schema_language::SchemaError::NotaDecode(_)
            | schema_language::SchemaError::ExpectedDelimiter { .. }
            | schema_language::SchemaError::MacroDidNotMatch { .. }
            | schema_language::SchemaError::UnsupportedMacroNodeStructure { .. }
    ));
}

#[test]
fn brace_namespace_rejects_redundant_key_value_declarations() {
    let source = "{} [] [] { Entry Entry { Topic Kind } } {} {}";
    let error = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("example", "0.1.0"))
        .expect_err("namespace declarations must be key/value pairs without duplicated names");

    // The single lowering engine (the typed-source path) rejects a redundant
    // `Entry Entry { … }` triple: a per-kind entry key must be a dotted
    // `TypeName.Definition` atom, so the undotted space-separated form fails.
    assert!(matches!(
        error,
        schema_language::SchemaError::ExpectedSyntaxDeclaration { .. }
            | schema_language::SchemaError::ExpectedSymbol { .. }
            | schema_language::SchemaError::NotaDecode(_)
            | schema_language::SchemaError::ExpectedDelimiter { .. }
            | schema_language::SchemaError::UnsupportedMacroNodeStructure { .. }
    ));
}

#[test]
fn colon_qualified_names_lower_as_schema_names() {
    let source = "{} [Record.schema:spirit:Entry] [] { schema:spirit:Topic.String schema:spirit:Entry.schema:spirit:Topic } {} {}";
    let schema = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("schema:spirit:lib", "0.1.0"))
        .expect("schema lowers");

    assert_eq!(
        root_enum(schema.input()).variants[0]
            .payload
            .as_ref()
            .expect("record payload")
            .plain_name()
            .expect("plain payload")
            .as_str(),
        "schema:spirit:Entry"
    );
    assert_eq!(
        schema.namespace()[1].name().namespace_segments(),
        vec!["schema", "spirit", "Entry"]
    );
    let TypeDeclaration::Newtype(topic) = declaration_value(&schema, 0) else {
        panic!("topic should be an alias");
    };
    assert_eq!(topic.name.local_part(), "Topic");
    let TypeDeclaration::Newtype(entry) = declaration_value(&schema, 1) else {
        panic!("entry should be an alias");
    };
    assert_eq!(
        entry.reference,
        TypeReference::Plain(Name::new("schema:spirit:Topic"))
    );
}

#[test]
fn package_loader_reads_schema_lib_entrypoint() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("spirit-crate");
    let package = SchemaPackage::new(root, "spirit-next", "0.1.0");
    let source = package.load_lib().expect("load lib.schema");
    let schema = source
        .lower(&SchemaEngine::default())
        .expect("schema lowers");

    assert_eq!(source.path(), package.lib_schema_path());
    assert_eq!(schema.identity().component().as_str(), "spirit-next:lib");
    assert!(schema.type_named("Entry").is_some());
}

#[test]
fn package_loader_reads_all_schema_modules_in_crate() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("plane-crate");
    let package = SchemaPackage::new(root, "plane-crate", "0.1.0");
    let sources = package.load_modules().expect("load all schema modules");

    assert_eq!(
        sources
            .iter()
            .map(|source| source.identity().component().as_str())
            .collect::<Vec<_>>(),
        vec![
            "plane-crate:nexus",
            "plane-crate:sema",
            "plane-crate:signal"
        ]
    );

    let schemas = package
        .lower_modules(&SchemaEngine::default())
        .expect("lower all schema modules with intra-crate imports");
    let nexus = schemas
        .iter()
        .find(|schema| schema.identity().component().as_str() == "plane-crate:nexus")
        .expect("nexus schema");
    assert_eq!(nexus.resolved_imports().len(), 2);
    assert_eq!(
        nexus
            .resolved_imports()
            .iter()
            .map(|import| import.source().rust_path())
            .collect::<Vec<_>>(),
        vec![
            "plane_crate::schema::signal::Input",
            "plane_crate::schema::signal::Output",
        ]
    );
    assert_eq!(
        root_enum(nexus.input()).variants[0]
            .payload
            .as_ref()
            .expect("nexus input payload")
            .plain_name()
            .expect("plain nexus input payload")
            .as_str(),
        // No-alias imports keep the producer's own name: the nexus imports
        // signal's `Input` under its own name, not a `SignalInput` alias.
        "Input"
    );

    let sema = schemas
        .iter()
        .find(|schema| schema.identity().component().as_str() == "plane-crate:sema")
        .expect("sema schema");
    assert!(sema.type_named("Entry").is_some());
}

#[test]
fn root_schema_describes_the_schema_root_type() {
    let source = include_str!("../schemas/root.schema");
    let schema = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("schema", "0.1.0"))
        .expect("root schema lowers");

    assert_eq!(schema.input().name().as_str(), "Input");
    assert_eq!(schema.output().name().as_str(), "Output");

    let TypeDeclaration::Struct(schema_struct) = schema
        .type_named("TrueSchema")
        .expect("schema type declaration")
    else {
        panic!("TrueSchema should be a struct");
    };

    assert_eq!(
        schema_struct
            .fields
            .iter()
            .map(|field| field.reference.plain_name().expect("plain field").as_str())
            .collect::<Vec<_>>(),
        vec!["Input", "Output", "Namespace"]
    );

    let TypeDeclaration::Enum(type_declaration) = schema
        .type_named("TypeDeclaration")
        .expect("type declaration enum")
    else {
        panic!("TypeDeclaration should be an enum");
    };
    assert_eq!(
        type_declaration
            .variants
            .iter()
            .map(|variant| (
                variant.name.as_str(),
                variant
                    .payload
                    .as_ref()
                    .map(|payload| payload.plain_name().expect("plain payload").as_str())
            ))
            .collect::<Vec<_>>(),
        vec![
            ("Struct", Some("StructDeclaration")),
            ("Enum", Some("EnumDeclaration")),
            ("Newtype", Some("NewtypeDeclaration")),
        ]
    );

    let TypeDeclaration::Enum(declaration) =
        schema.type_named("Declaration").expect("declaration enum")
    else {
        panic!("Declaration should be an enum");
    };
    assert_eq!(
        declaration
            .variants
            .iter()
            .map(|variant| (
                variant.name.as_str(),
                variant
                    .payload
                    .as_ref()
                    .map(|payload| payload.plain_name().expect("plain payload").as_str())
            ))
            .collect::<Vec<_>>(),
        vec![
            ("Public", Some("NamedTypeDeclaration")),
            ("Private", Some("NamedTypeDeclaration")),
        ]
    );
}

#[test]
fn core_schema_describes_default_builtin_macro_positions() {
    let source = include_str!("../schemas/core.schema");
    let schema = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("schema-core", "0.1.0"))
        .expect("core schema lowers");

    let TypeDeclaration::Struct(macro_library) = schema
        .type_named("BuiltinMacroLibrary")
        .expect("builtin macro library declaration")
    else {
        panic!("BuiltinMacroLibrary should be a struct");
    };
    assert_eq!(
        macro_library
            .fields
            .iter()
            .map(|field| field.reference.plain_name().expect("plain field").as_str())
            .collect::<Vec<_>>(),
        vec![
            "BuiltinMacroPositions",
            "BuiltinMacroShapes",
            "BuiltinMacroOutputs",
            "BuiltinMacroDefinitions",
        ]
    );

    let TypeDeclaration::Enum(macro_position) = schema
        .type_named("MacroPosition")
        .expect("macro position enum")
    else {
        panic!("MacroPosition should be an enum");
    };
    assert_eq!(
        macro_position
            .variants
            .iter()
            .map(|variant| variant.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "RootImports",
            "RootInput",
            "RootOutput",
            "RootNamespace",
            "NamespaceDeclaration",
            "StructFields",
            "EnumVariants",
            "TypeReference",
        ]
    );

    let TypeDeclaration::Newtype(macro_pattern) = schema
        .type_named("MacroPattern")
        .expect("macro pattern alias")
    else {
        panic!("MacroPattern should be an alias");
    };
    assert_eq!(
        macro_pattern
            .reference
            .plain_name()
            .expect("macro pattern object reference")
            .as_str(),
        "MacroPatternObject"
    );

    let TypeDeclaration::Enum(macro_pattern_object) = schema
        .type_named("MacroPatternObject")
        .expect("macro pattern object enum")
    else {
        panic!("MacroPatternObject should be an enum");
    };
    assert_eq!(
        macro_pattern_object
            .variants
            .iter()
            .map(|variant| {
                (
                    variant.name.as_str(),
                    variant
                        .payload
                        .as_ref()
                        .and_then(|payload| payload.plain_name())
                        .map(Name::as_str),
                )
            })
            .collect::<Vec<_>>(),
        vec![
            ("Capture", Some("MacroCaptureName")),
            ("RestCapture", Some("MacroCaptureName")),
            ("Atom", Some("MacroAtom")),
            ("Delimited", Some("MacroPatternDelimited")),
        ]
    );

    let TypeDeclaration::Enum(macro_template_object) = schema
        .type_named("MacroTemplateObject")
        .expect("macro template object enum")
    else {
        panic!("MacroTemplateObject should be an enum");
    };
    assert_eq!(
        macro_template_object
            .variants
            .iter()
            .map(|variant| {
                (
                    variant.name.as_str(),
                    variant
                        .payload
                        .as_ref()
                        .and_then(|payload| payload.plain_name())
                        .map(Name::as_str),
                )
            })
            .collect::<Vec<_>>(),
        vec![
            ("Capture", Some("MacroCaptureName")),
            ("RestCapture", Some("MacroCaptureName")),
            ("Atom", Some("MacroAtom")),
            ("Delimited", Some("MacroTemplateDelimited")),
        ]
    );
}

#[test]
fn builtin_macro_file_defines_visible_dollar_captures() {
    let library = MacroLibrary::builtin().expect("builtin macros parse");
    let definitions = library.definitions();
    let names = definitions
        .iter()
        .map(|definition| definition.name().as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "SchemaStructDefinition",
            "SchemaEnumDefinition",
            "SchemaNewtypeDefinition",
            "SchemaStructFields",
            "SchemaEnumVariants",
        ]
    );

    let struct_definition = definitions
        .iter()
        .find(|definition| definition.name().as_str() == "SchemaStructDefinition")
        .expect("struct macro definition");
    assert_eq!(struct_definition.capture_names(), vec!["$Name", "$*Fields"]);

    let enum_definition = definitions
        .iter()
        .find(|definition| definition.name().as_str() == "SchemaEnumDefinition")
        .expect("enum macro definition");
    assert_eq!(enum_definition.capture_names(), vec!["$Name", "$*Variants"]);
}

#[test]
fn macro_lowering_receives_macro_position() {
    struct ProbeMacro;

    impl SchemaMacroHandler for ProbeMacro {
        fn name(&self) -> &str {
            "Probe"
        }

        fn matches(&self, object: MacroObject<'_>, position: MacroPosition) -> bool {
            position == MacroPosition::RootInput && object.block().is_some()
        }

        fn lower(
            &self,
            _object: MacroObject<'_>,
            position: MacroPosition,
            context: &mut MacroContext,
            _registry: &MacroRegistry,
        ) -> Result<MacroOutput, schema_language::SchemaError> {
            context.remember_macro(self.name());
            context.remember_position(position);
            Ok(MacroOutput::References(Vec::new()))
        }
    }

    let document = nota::Document::parse("(Input)").expect("nota parses");
    let mut context = MacroContext::default();
    let object = document.root_object_at(0).expect("root object");
    let probe = ProbeMacro;

    assert!(probe.matches(MacroObject::Block(object), MacroPosition::RootInput));
    probe
        .lower(
            MacroObject::Block(object),
            MacroPosition::RootInput,
            &mut context,
            &MacroRegistry::new(),
        )
        .expect("probe lower");
    assert_eq!(context.positions_seen(), &[MacroPosition::RootInput]);
    assert_eq!(
        context
            .macros_applied()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["Probe"]
    );
}

#[test]
fn field_names_are_derived_from_type_names() {
    let source = "{} [] [] { RecordIdentifier.Integer Description.String Entry.{ RecordIdentifier Description } } {} {}";
    let schema = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("example", "0.1.0"))
        .expect("schema lowers");
    let TypeDeclaration::Struct(entry) = declaration_value(&schema, 2) else {
        panic!("entry should be a struct");
    };

    assert_eq!(entry.fields[0].name, Name::new("record_identifier"));
    assert_eq!(entry.fields[1].name, Name::new("description"));
}

#[test]
fn default_engine_lowers_through_registered_structural_forms() {
    let source = include_str!("../schemas/spirit-min.schema");
    let artifact = SchemaSourceArtifact::from_schema_text(source).expect("schema source decodes");
    let schema = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("spirit", "0.1.0"),
        )
        .expect("schema lowers through macros");

    let input = schema.root_enum_named("Input").expect("input root");
    assert_eq!(
        input
            .variants
            .iter()
            .map(|variant| variant.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Record", "Observe"]
    );

    let output = schema.root_enum_named("Output").expect("output root");
    assert_eq!(
        output
            .variants
            .iter()
            .map(|variant| variant.name.as_str())
            .collect::<Vec<_>>(),
        vec!["RecordAccepted", "RecordsObserved"]
    );

    let namespace = schema.namespace();
    let entry = namespace
        .iter()
        .find(|declaration| declaration.name().as_str() == "Entry")
        .expect("entry declaration");
    let TypeDeclaration::Struct(entry) = entry.value() else {
        panic!("entry should lower as a struct");
    };
    assert_eq!(
        entry
            .fields
            .iter()
            .map(|field| (
                field.name.as_str(),
                field.reference.plain_name().map(Name::as_str)
            ))
            .collect::<Vec<_>>(),
        vec![
            ("topics", Some("Topics")),
            ("kind", Some("Kind")),
            ("description", Some("Description")),
            ("magnitude", Some("Magnitude")),
        ]
    );

    let kind = namespace
        .iter()
        .find(|declaration| declaration.name().as_str() == "Kind")
        .expect("kind declaration");
    let TypeDeclaration::Enum(kind) = kind.value() else {
        panic!("kind should lower as an enum");
    };
    assert_eq!(
        kind.variants
            .iter()
            .map(|variant| variant.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "Decision",
            "Principle",
            "Correction",
            "Clarification",
            "Constraint",
        ]
    );
}

/// Report 702: there is one lowering engine — the typed-source path. The
/// `MacroRegistry` is still the public type-reference vocabulary an engine is
/// built from (`with_registry`), but it no longer drives the root/namespace
/// lowering semantics: those come from the typed-source archive on every entry
/// path. So an engine assembled from a custom registry lowers a valid document
/// through the same single path the default engine uses. (The retired second
/// engine let a registry handler reject at a root position; that mechanism is
/// gone with the engine it belonged to.)
#[test]
fn schema_engine_can_be_built_from_a_macro_registry() {
    let engine = SchemaEngine::with_registry(MacroRegistry::with_schema_defaults());
    let schema = engine
        .lower_source(
            "{} [] [] { Topic.String } {} {}",
            SchemaIdentity::new("example", "0.1.0"),
        )
        .expect("an engine built from a registry lowers through the single path");

    assert_eq!(
        schema
            .namespace()
            .iter()
            .map(|declaration| declaration.name().as_str())
            .collect::<Vec<_>>(),
        vec!["Topic"],
    );
}

#[test]
fn brace_body_lowers_as_struct_map_with_inline_private_types() {
    let source = "{} [] [] { Address.String ToInbox.Address ToOutbox.Address Routing.{ ToInbox ToOutbox } } {} {}";
    let schema = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("example", "0.1.0"))
        .expect("brace values are struct maps, not enum sugar");

    assert_eq!(
        schema
            .namespace()
            .iter()
            .map(|declaration| (declaration.name().as_str(), declaration.visibility()))
            .collect::<Vec<_>>(),
        vec![
            ("Address", Visibility::Public),
            ("ToInbox", Visibility::Public),
            ("ToOutbox", Visibility::Public),
            ("Routing", Visibility::Public),
        ]
    );
    assert!(matches!(
        schema.type_named("Routing").expect("routing type"),
        TypeDeclaration::Struct(_)
    ));
}

#[test]
fn strict_declaration_field_pairs_lower_through_default_engine() {
    let source = "{} [] [] { RecordIdentifier.Integer Description.String Entry.{ RecordIdentifier Description } } {} {}";
    let schema = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("example", "0.1.0"))
        .expect("at declaration lowers");
    let TypeDeclaration::Struct(entry) = declaration_value(&schema, 2) else {
        panic!("entry should be a struct");
    };

    assert_eq!(entry.fields[0].name, Name::new("record_identifier"));
    assert_eq!(
        entry.fields[0].reference,
        TypeReference::Plain(Name::new("RecordIdentifier"))
    );
    assert_eq!(entry.fields[1].name, Name::new("description"));
}

#[test]
fn star_shorthand_derives_fields_and_data_variant_payloads_from_real_schema() {
    let source = include_str!("fixtures/big-schemas/derived-members.schema");
    let schema = SchemaEngine::default()
        .lower_source(
            source,
            SchemaIdentity::new("example:derived-members", "0.1.0"),
        )
        .expect("derived member schema lowers");

    let TypeDeclaration::Struct(entry) = schema.type_named("Entry").expect("entry type") else {
        panic!("entry should be a struct");
    };
    assert_eq!(
        entry
            .fields
            .iter()
            .map(|field| (
                field.name.as_str(),
                field.reference.plain_name().map(Name::as_str)
            ))
            .collect::<Vec<_>>(),
        vec![
            ("topics", Some("Topics")),
            ("kind", Some("Kind")),
            ("description", Some("Description")),
            ("magnitude", Some("Magnitude")),
        ]
    );

    let TypeDeclaration::Struct(query) = schema.type_named("Query").expect("query type") else {
        panic!("query should remain a struct");
    };
    assert_eq!(query.fields[0].name.as_str(), "topics");
    assert_eq!(
        query.fields[1].reference,
        TypeReference::optional(TypeReference::Integer)
    );

    let TypeDeclaration::Enum(some_enum) = schema.type_named("SomeEnum").expect("some enum type")
    else {
        panic!("SomeEnum should be an enum");
    };
    assert_eq!(
        some_enum
            .variants
            .iter()
            .map(|variant| variant.name.as_str())
            .collect::<Vec<_>>(),
        vec!["SomethingHoldingSomething", "SomethingElse", "SomeString"]
    );
    assert_eq!(
        some_enum.variants[0].payload,
        Some(TypeReference::Plain(Name::new(
            "SomethingHoldingSomethingPayload"
        )))
    );
    assert_eq!(some_enum.variants[1].payload, None);
    assert_eq!(some_enum.variants[2].payload, Some(TypeReference::String));

    let TypeDeclaration::Newtype(topic) = schema.type_named("Topic").expect("topic type") else {
        panic!("Topic should be an alias");
    };
    assert_eq!(topic.reference, TypeReference::String);
}

#[test]
fn inline_pascal_declaration_creates_ordered_namespace_type() {
    let source = "{} [] [] { RecordIdentifier.Integer Receipt.{ RecordIdentifier } Entry.{ current.Receipt later.Receipt } } {} {}";
    let schema = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("example", "0.1.0"))
        .expect("inline declaration lowers");

    assert_eq!(
        schema
            .namespace()
            .iter()
            .map(|declaration| (declaration.name().as_str(), declaration.visibility()))
            .collect::<Vec<_>>(),
        vec![
            ("RecordIdentifier", Visibility::Public),
            ("Receipt", Visibility::Public),
            ("Entry", Visibility::Public),
        ]
    );

    let TypeDeclaration::Struct(entry) = declaration_value(&schema, 2) else {
        panic!("entry should be a struct");
    };
    assert_eq!(entry.fields[0].name, Name::new("current"));
    assert_eq!(
        entry.fields[0].reference,
        TypeReference::Plain(Name::new("Receipt"))
    );
    assert_eq!(entry.fields[1].name, Name::new("later"));
    assert_eq!(
        entry.fields[1].reference,
        TypeReference::Plain(Name::new("Receipt"))
    );
}

#[test]
fn root_enum_positions_supply_input_and_output_names() {
    let schema = SchemaEngine::default()
        .lower_source(
            "{} [Record.Entry] [] {} {} {}",
            SchemaIdentity::new("example", "0.1.0"),
        )
        .expect("bare input root lowers because the root position names it");
    assert_eq!(schema.input().name().as_str(), "Input");
    assert_eq!(
        root_enum(schema.input()).variants[0].name.as_str(),
        "Record"
    );
    assert_eq!(schema.output().name().as_str(), "Output");

    let error = SchemaEngine::default()
        .lower_source(
            "{} [] Reply {} {} {}",
            SchemaIdentity::new("example", "0.1.0"),
        )
        .expect_err("labeled root wrapper should not be accepted");
    // The single lowering engine (the typed-source path) reads the output
    // slot as either an enum body or a dotted application root; a bare
    // declared-name wrapper is therefore rejected at decode.
    assert!(matches!(
        error,
        schema_language::SchemaError::UnsupportedMacroNodeStructure { .. }
            | schema_language::SchemaError::MacroDidNotMatch { .. }
            | schema_language::SchemaError::ExpectedRootApplication { .. }
            | schema_language::SchemaError::NotaDecode(_)
    ));
}
