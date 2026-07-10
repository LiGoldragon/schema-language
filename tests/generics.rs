//! Generic application references — `Foo.(A B …)` at a reference position —
//! and parameterized DECLARATION heads — `(| Name Param … |)` at a
//! declaration's type-name position.
//!
//! `TypeReference::Application { head, arguments }` is the broad
//! generic-application form. The authored schema projection is dotted and
//! positional; macro-expansion internals still carry a parenthesized node seam.
//! The first block of tests pins the application-form behaviours:
//!
//! (a) a multi-arg user generic application lowers to `Application` and
//!     round-trips byte-stable through both the rkyv codec and the canonical
//!     NOTA codec;
//! (b) the built-in heads `Vector.X`, `Optional.X`, and `Map.(K V)` still
//!     lower to their dedicated variants through the same dispatch, and a
//!     built-in head wins over the broad application form (dispatch ORDER);
//! (c) a dropped alias `Vec.X` no longer lowers to the collection — it is
//!     an ordinary application head now;
//! (d) the closure walk over an imported generic head records that head's
//!     import.
//!
//! The second block pins the parameterized-declaration-head behaviours
//! (the head analogue of the application form): binders resolve inside the
//! body instead of failing the closure walk, an `Application` of a declared
//! parameterized head is arity-checked at lowering, and the declared head
//! is consulted before the broad application form.

use nota::{Document, NotaDecode, NotaEncode};
use schema_language::{
    ApplicationHead, Name, Root, SchemaEngine, SchemaError, SchemaIdentity, SchemaSourceArtifact,
    SingleTypeReferenceProjection, TypeDeclaration, TypeReference,
};

fn lower(namespace: &str) -> schema_language::TrueSchema {
    try_lower(namespace).expect("schema lowers")
}

fn try_lower(namespace: &str) -> Result<schema_language::TrueSchema, SchemaError> {
    SchemaEngine::default().lower_source(
        &format!("{{}}\n[]\n[]\n{{ {namespace} }}"),
        SchemaIdentity::new("generics:lib", "0.1.0"),
    )
}

fn single_reference(schema: &schema_language::TrueSchema, name: &str) -> TypeReference {
    match schema.type_named(name).expect("type present") {
        TypeDeclaration::Newtype(declaration) => declaration.reference,
        TypeDeclaration::Struct(_) | TypeDeclaration::Enum(_) => {
            panic!("{name} should be a single-reference declaration")
        }
    }
}

// (a) multi-arg user generic application lowers to Application and round-trips.

#[test]
fn multi_argument_application_lowers_to_application() {
    let schema = lower("Alpha String Beta String Holder Foo.(Alpha Beta)");
    assert_eq!(
        single_reference(&schema, "Holder"),
        TypeReference::Application {
            head: ApplicationHead::Local(Name::new("Foo")),
            arguments: vec![TypeReference::new("Alpha"), TypeReference::new("Beta")],
        }
    );
}

#[test]
fn application_round_trips_byte_stable_through_rkyv() {
    let reference = TypeReference::Application {
        head: ApplicationHead::Local(Name::new("Foo")),
        arguments: vec![TypeReference::new("Alpha"), TypeReference::new("Beta")],
    };
    let bytes =
        rkyv::to_bytes::<rkyv::rancor::Error>(&reference).expect("application archives as rkyv");
    let restored = rkyv::from_bytes::<TypeReference, rkyv::rancor::Error>(&bytes)
        .expect("application decodes from rkyv");
    assert_eq!(restored, reference);
    // Archiving the restored value yields identical bytes — byte-stable.
    let again =
        rkyv::to_bytes::<rkyv::rancor::Error>(&restored).expect("re-archive the restored value");
    assert_eq!(bytes.as_slice(), again.as_slice());
}

#[test]
fn application_round_trips_through_canonical_nota_codec() {
    let reference = TypeReference::Application {
        head: ApplicationHead::Local(Name::new("Foo")),
        arguments: vec![TypeReference::new("Alpha"), TypeReference::new("Beta")],
    };
    let text = reference.to_nota();
    let document = Document::parse(&text).expect("application NOTA parses");
    let decoded = TypeReference::from_nota_block(&document.root_objects()[0])
        .expect("application decodes from canonical NOTA");
    assert_eq!(decoded, reference);
    // The re-encode is byte-identical to the first projection.
    assert_eq!(decoded.to_nota(), text);
}

// (b) built-ins still lower through the seam, and a built-in head wins over
// the broad application form (dispatch ORDER).

#[test]
fn builtin_heads_still_lower_to_their_variants() {
    let schema = lower(
        "Key String Value String VectorField Vector.Value OptionalField Optional.Value MapField Map.(Key Value)",
    );
    assert_eq!(
        single_reference(&schema, "VectorField"),
        TypeReference::vector(TypeReference::new("Value"))
    );
    assert_eq!(
        single_reference(&schema, "OptionalField"),
        TypeReference::optional(TypeReference::new("Value"))
    );
    assert_eq!(
        single_reference(&schema, "MapField"),
        TypeReference::map(TypeReference::new("Key"), TypeReference::new("Value"))
    );
}

#[test]
fn builtin_head_wins_over_broad_application_form() {
    // `Vector.Value` matches the broad `Foo.Value` shape too (Vector is a
    // PascalCase head), but the built-in fast path is dispatched first, so it
    // must NOT lower to an application named `Vector`.
    let schema = lower("Value String Field Vector.Value");
    let reference = single_reference(&schema, "Field");
    assert!(
        matches!(
            reference,
            TypeReference::SingleTypeApplication {
                projection: SingleTypeReferenceProjection::Vector,
                ..
            }
        ),
        "a built-in head must win over the application form, got {reference:?}",
    );
    assert!(
        !matches!(reference, TypeReference::Application { .. }),
        "the built-in head must not fall through to the application form",
    );
}

// (c) a dropped alias no longer lowers to the collection.

#[test]
fn dropped_vec_alias_no_longer_lowers_to_vector() {
    let schema = lower("Service String Cluster Vec.Service");
    let reference = single_reference(&schema, "Cluster");
    assert!(
        !matches!(
            reference,
            TypeReference::SingleTypeApplication {
                projection: SingleTypeReferenceProjection::Vector,
                ..
            }
        ),
        "the dropped `Vec` alias must not lower to a Vector",
    );
    assert_eq!(
        reference,
        TypeReference::Application {
            head: ApplicationHead::Local(Name::new("Vec")),
            arguments: vec![TypeReference::new("Service")],
        }
    );
}

// ----------------------------------------------------------------------
// Parameterized DECLARATION heads `(| Name Param … |)` — the head analogue of
// the application form. A declaration's type-name position becomes a
// pipe-parenthesized `(| Name Param Param … |)` head that introduces
// type-parameter binders; the binders resolve inside the body, and an
// `Application` of a declared parameterized head is arity-checked at lowering
// (decision O8).
// ----------------------------------------------------------------------

fn declaration_parameters(schema: &schema_language::TrueSchema, name: &str) -> Vec<Name> {
    schema
        .namespace()
        .into_iter()
        .find(|declaration| declaration.name().as_str() == name)
        .expect("declaration present")
        .parameters()
        .to_vec()
}

// (a) A parameterized declaration whose body uses its parameters as Plain
//     references lowers, and its family closure resolves the binders
//     instead of failing with FamilyReferenceNotFound.

#[test]
fn parameterized_declaration_resolves_its_parameters_as_binders() {
    let schema = lower("(| Plane Input Output |) { Input Output }");

    // The binders are recorded on the declaration, in order.
    assert_eq!(
        declaration_parameters(&schema, "Plane"),
        &[Name::new("Input"), Name::new("Output")],
    );
}

// (b) An Application supplying the WRONG argument count to a resolved
//     parameterized head is a typed arity error AT LOWERING — not a panic,
//     not a deferred emitter failure.

#[test]
fn application_with_wrong_argument_count_is_an_arity_error_at_lowering() {
    let error = try_lower("(| Plane Input Output |) { Input Output } Holder Plane.String")
        .expect_err("one argument against a two-parameter head must fail at lowering");
    assert_eq!(
        error,
        SchemaError::GenericArityMismatch {
            head: "Plane".to_owned(),
            expected: 2,
            found: 1,
        },
    );
}

// (c) The correct argument count matching the declared arity lowers and
//     the application reference is present.

#[test]
fn application_with_correct_argument_count_lowers() {
    let schema = lower("(| Plane Input Output |) { Input Output } Holder Plane.(String Integer)");
    assert_eq!(
        single_reference(&schema, "Holder"),
        TypeReference::Application {
            head: ApplicationHead::Local(Name::new("Plane")),
            arguments: vec![TypeReference::String, TypeReference::Integer],
        },
    );
}

// (d) A declared parameterized head is consulted BEFORE the broad
//     Application form: applying `(Plane …)` resolves to the declared
//     `Plane` (so its arity binds), whereas an undeclared head fixes no
//     arity and any count is accepted as an unresolved generic application.

#[test]
fn declared_parameterized_head_wins_over_unresolved_application() {
    // The declared head's arity binds — a wrong count is rejected.
    assert_eq!(
        try_lower("(| Plane Input Output |) { Input Output } Holder Plane.String")
            .expect_err("declared head is consulted, so its arity binds"),
        SchemaError::GenericArityMismatch {
            head: "Plane".to_owned(),
            expected: 2,
            found: 1,
        },
    );

    // An UNDECLARED head fixes no arity, so the same single-argument
    // application is an ordinary unresolved generic application — proving
    // the declared head, not the broad form, governed the case above.
    let schema = lower("Holder Foo.String");
    assert_eq!(
        single_reference(&schema, "Holder"),
        TypeReference::Application {
            head: ApplicationHead::Local(Name::new("Foo")),
            arguments: vec![TypeReference::String],
        },
    );
}

#[test]
fn duplicate_generic_parameters_are_rejected_at_the_declaration_head() {
    let error = try_lower("(| Plane Input Input |) { Input }")
        .expect_err("duplicate declaration parameters must be rejected");
    assert_eq!(
        error,
        SchemaError::DuplicateTypeParameter {
            declaration: "Plane".to_owned(),
            parameter: "Input".to_owned(),
        },
    );
}

#[test]
fn duplicate_frame_parameters_are_rejected_at_the_frame_head() {
    let error = try_lower("(| Work Event Event Outcome |) [Started.Event Completed.Outcome]")
        .expect_err("duplicate frame parameters must be rejected");
    assert_eq!(
        error,
        SchemaError::DuplicateTypeParameter {
            declaration: "Work".to_owned(),
            parameter: "Event".to_owned(),
        },
    );
}

#[test]
fn map_grouped_payload_lowers_and_dot_chain_arity_fails() {
    let schema = lower("Key String Value String Holder Map.(Key Value)");
    assert_eq!(
        single_reference(&schema, "Holder"),
        TypeReference::map(TypeReference::new("Key"), TypeReference::new("Value")),
    );

    let error = try_lower("Key String Value String Holder Map.Key.Value")
        .expect_err("Map.Key.Value is unary nesting and must not satisfy Map arity");
    assert_eq!(
        error,
        SchemaError::GenericArityMismatch {
            head: "Map".to_owned(),
            expected: 2,
            found: 1,
        },
    );
}

// The parameterized head survives the source-codec archive: the entry key
// projects back to `(| Plane Input Output |)` text and re-decodes to the same
// source object, and lowering through the source endpoint records the same
// binders as the macro-engine path (edit site 2).

#[test]
fn parameterized_head_round_trips_through_the_source_codec() {
    let source = "{}\n[]\n[]\n{\n  (| Plane Input Output |) { Input Output }\n}";
    let artifact = SchemaSourceArtifact::from_schema_text(source).expect("source decodes");
    let canonical = artifact.to_schema_text();
    assert!(
        canonical.contains("(|Plane Input Output|) { Input Output }"),
        "the parameterized head must project back to source text, got {canonical}",
    );
    let recovered =
        SchemaSourceArtifact::from_schema_text(&canonical).expect("canonical source decodes");
    assert_eq!(artifact, recovered, "the source archive round-trips");

    let schema = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("generics:lib", "0.1.0"),
        )
        .expect("source endpoint lowers the parameterized declaration");
    assert_eq!(
        declaration_parameters(&schema, "Plane"),
        &[Name::new("Input"), Name::new("Output")],
    );
}

// Arity validation is shared by both lowering paths: the source-codec
// endpoint rejects a wrong-arity application at lowering, exactly as the
// macro-engine path does.

#[test]
fn source_codec_path_also_validates_application_arity() {
    let source =
        "{}\n[]\n[]\n{\n  (| Plane Input Output |) { Input Output }\n  Holder Plane.String\n}";
    let artifact = SchemaSourceArtifact::from_schema_text(source).expect("source decodes");
    let error = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("generics:lib", "0.1.0"),
        )
        .expect_err("source-codec lowering must arity-check the application");
    assert_eq!(
        error,
        SchemaError::GenericArityMismatch {
            head: "Plane".to_owned(),
            expected: 2,
            found: 1,
        },
    );
}

// ----------------------------------------------------------------------
// Root-position application `(Head Arg …)` — the component-root Input /
// Output position as a typed sum. A root is now `Root::Enum` (the enum-body
// form `[Variant …]`) OR `Root::Application` (an application of an
// imported/declared parameterized head). The application root is
// closure-walked identically to a field-position application: its head and
// arguments route through the SAME `visit_reference` Application arm, so the
// content-address stays deterministic and incorporates the arguments.
// ----------------------------------------------------------------------

/// A document whose Input root is the application form `(Work A B C D)` over
/// a locally-declared four-parameter head, with an empty Output enum. The
/// four argument types are declared so the closure walk resolves them; the
/// head's arity (4) matches the application's argument count.
fn application_root_source(read_output: &str) -> String {
    format!(
        "{{}} Work.(SignalInput SemaWriteOutput {read_output} EffectOutcome) [] {{ \
         (| Work In WriteOut ReadOut Outcome |) {{ In WriteOut ReadOut Outcome }} \
         SignalInput String \
         SemaWriteOutput Boolean \
         SemaReadOutput Integer \
         AltReadOutput Integer \
         EffectOutcome Boolean \
         }}"
    )
}

fn lower_application_root(read_output: &str) -> schema_language::TrueSchema {
    SchemaEngine::default()
        .lower_source(
            &application_root_source(read_output),
            SchemaIdentity::new("reaction-frame:lib", "0.1.0"),
        )
        .expect("application-root schema lowers")
}

// (a) An Input root in the application form lowers to a root carrying
//     `TypeReference::Application`; `family_root`/`root_named` return the
//     application root WITHOUT panicking.

#[test]
fn root_position_application_lowers_to_root_application() {
    let schema = lower_application_root("SemaReadOutput");

    // The Input root is the application form; the Output root stays an enum.
    assert!(
        matches!(schema.input(), Root::Application(_)),
        "Input root should be the application form, got {:?}",
        schema.input(),
    );
    let input = schema.input();
    let application = input
        .as_application()
        .expect("Input root is the application form");
    assert_eq!(application.name().as_str(), "Input");
    assert_eq!(
        application.head(),
        &ApplicationHead::Local(Name::new("Work")),
    );
    assert_eq!(
        application.arguments(),
        &[
            TypeReference::new("SignalInput"),
            TypeReference::new("SemaWriteOutput"),
            TypeReference::new("SemaReadOutput"),
            TypeReference::new("EffectOutcome"),
        ],
    );
    assert!(
        matches!(schema.output(), Root::Enum(_)),
        "the empty Output position stays the enum-body form",
    );

    // The application root projects back to a field-position application
    // reference — the exact value the closure walk consumes.
    assert_eq!(
        TypeReference::from(application),
        TypeReference::Application {
            head: ApplicationHead::Local(Name::new("Work")),
            arguments: vec![
                TypeReference::new("SignalInput"),
                TypeReference::new("SemaWriteOutput"),
                TypeReference::new("SemaReadOutput"),
                TypeReference::new("EffectOutcome"),
            ],
        },
    );

    // `root_named` returns the application root without panicking.
    let root = schema.root_named("Input").expect("Input root present");
    assert!(
        root.as_application().is_some(),
        "root_named yields the application root"
    );
}

// (b) The existing enum-body root form `[Variant.Payload …]` STILL lowers to a
//     `Root::Enum(EnumDeclaration)` — no regression.

#[test]
fn enum_body_root_still_lowers_to_root_enum() {
    let schema = SchemaEngine::default()
        .lower_source(
            "{} [Record.Entry] [Recorded.Receipt] { Topic String Ok Boolean Entry { Topic } Receipt { Ok } }",
            SchemaIdentity::new("enum-root:lib", "0.1.0"),
        )
        .expect("enum-body root schema lowers");

    let Root::Enum(input) = schema.input() else {
        panic!(
            "Input root should be the enum-body form, got {:?}",
            schema.input()
        );
    };
    assert_eq!(input.name.as_str(), "Input");
    assert_eq!(input.variants[0].name.as_str(), "Record");
    assert!(
        matches!(schema.output(), Root::Enum(_)),
        "the Output root is also the enum-body form",
    );
}
