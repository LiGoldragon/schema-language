//! The shared reaction frame fixture (`Work` / `Action`) and a full-frame
//! spirit pilot that imports the frame and APPLIES it at the Input/Output root
//! positions, binding spirit's payload vocabulary.
//!
//! Three fixtures drive the slice, all under `tests/fixtures/reaction/schema/`:
//!
//! - `reaction.schema` — the maximal SHARED reaction frame, declared once as
//!   the two parameterized declarations `(| Work Event WriteDone ReadDone
//!   EffectDone |)` and `(| Action Reply Write Read Effect Continuation |)`. It
//!   has no plane roots (empty Input/Output): a file of parameterized
//!   declarations meant to be imported.
//! - `spirit-nexus.schema` — spirit's nexus plane MIGRATED onto the frame:
//!   it imports `Work`/`Action` from the reaction fixture and applies them at
//!   the Input/Output ROOT positions, binding spirit's payload vocabulary. The
//!   `Nexus*` prefix is dropped (decision O9). Spirit binds ALL four Work legs
//!   and ALL five Action legs — a full-frame component, so it does not exercise
//!   the omittable-leg mechanism (decision O3, proven separately in step 6).
//! - `spirit-nexus-concrete.schema` — the SAME nexus plane hand-written the
//!   pre-migration way, with concrete enum-body Input/Output roots whose
//!   variants carry the payloads directly. This is the equivalence baseline:
//!   the migrated frame application must expand to exactly the concrete roots.
//!
//! The tests prove: (1) the frame lowers and its parameterized declarations
//! close over their binders; (2) the migrated nexus lowers through the import +
//! root-application path, the imported frame heads resolve into the closure,
//! and the roots are `Root::Application`; (3) EQUIVALENCE — expanding the frame
//! application (binder -> argument substitution over the frame's variants)
//! yields exactly the concrete schema's hand-written Input/Output enum roots,
//! leg for leg, payload for payload.

use std::path::PathBuf;

use schema_language::{
    ApplicationHead, EnumVariant, ImportResolver, MacroContext, Name, Root, RootApplication,
    SchemaEngine, SchemaIdentity, TrueSchema, TypeReference,
};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/reaction/schema")
}

fn read_fixture(file_name: &str) -> String {
    std::fs::read_to_string(fixture_dir().join(file_name))
        .unwrap_or_else(|error| panic!("read {file_name}: {error}"))
}

fn reaction_resolver() -> ImportResolver {
    ImportResolver::new().with_dependency("reaction", fixture_dir(), "0.1.0")
}

fn lower_reaction() -> TrueSchema {
    SchemaEngine::default()
        .lower_source(
            &read_fixture("reaction.schema"),
            SchemaIdentity::new("reaction:reaction", "0.1.0"),
        )
        .expect("reaction frame lowers")
}

fn lower_migrated() -> TrueSchema {
    SchemaEngine::default()
        .lower_source_with_resolver(
            &read_fixture("spirit-nexus.schema"),
            SchemaIdentity::new("spirit:nexus", "0.1.0"),
            &mut MacroContext::default(),
            &reaction_resolver(),
        )
        .expect("migrated spirit nexus lowers through the import + root-application path")
}

fn lower_concrete() -> TrueSchema {
    SchemaEngine::default()
        .lower_source(
            &read_fixture("spirit-nexus-concrete.schema"),
            SchemaIdentity::new("spirit:nexus", "0.1.0"),
        )
        .expect("concrete (pre-migration) spirit nexus lowers")
}

/// Monomorphize a migrated root application against the reaction frame, using
/// the LIBRARY expansion path now under test: read the named frame's body
/// (binders + variants) from the lowered reaction schema via
/// [`TrueSchema::declared_frame_body`], then expand the application with
/// [`RootApplication::expand_with`]. The result is the concrete `EnumVariant`
/// list the migrated root denotes — what the equivalence assertions check
/// leg-for-leg against the hand-written concrete baseline.
fn expand_root(
    reaction: &TrueSchema,
    frame_name: &str,
    application: &RootApplication,
) -> Vec<EnumVariant> {
    let (parameters, variants) = reaction
        .declared_frame_body(frame_name)
        .unwrap_or_else(|| panic!("reaction frame declares parameterized enum {frame_name}"));
    assert_eq!(
        parameters.len(),
        application.arguments().len(),
        "frame head {frame_name} arity must match the application argument count",
    );
    application.expand_with(&parameters, &variants)
}

fn application_root(schema: &TrueSchema, position: &str) -> RootApplication {
    schema
        .root_named(position)
        .unwrap_or_else(|| panic!("{position} root present"))
        .as_application()
        .unwrap_or_else(|| panic!("{position} root is the application form"))
        .clone()
}

fn concrete_root_variants(schema: &TrueSchema, position: &str) -> Vec<EnumVariant> {
    let Root::Enum(declaration) = schema
        .root_named(position)
        .unwrap_or_else(|| panic!("concrete {position} root present"))
    else {
        panic!("concrete {position} root is the enum-body form");
    };
    declaration.variants
}

// ----------------------------------------------------------------------
// (1) The shared frame lowers; its parameterized declarations close over
//     their binders.
// ----------------------------------------------------------------------

#[test]
fn reaction_frame_lowers_with_its_two_parameterized_declarations() {
    let reaction = lower_reaction();

    // The frame declares exactly Work and Action, each parameterized. The
    // binders are read through the same library accessor the import resolver
    // now carries across the crate boundary.
    let (work_parameters, _) = reaction
        .declared_frame_body("Work")
        .expect("Work is a declared parameterized enum");
    assert_eq!(
        work_parameters,
        &[
            Name::new("Event"),
            Name::new("WriteDone"),
            Name::new("ReadDone"),
            Name::new("EffectDone"),
        ],
    );
    let (action_parameters, _) = reaction
        .declared_frame_body("Action")
        .expect("Action is a declared parameterized enum");
    assert_eq!(
        action_parameters,
        &[
            Name::new("Reply"),
            Name::new("Write"),
            Name::new("Read"),
            Name::new("Effect"),
            Name::new("Continuation"),
        ],
    );

    // The frame has no plane roots — empty Input/Output enum bodies.
    assert!(matches!(reaction.input(), Root::Enum(_)));
    assert!(matches!(reaction.output(), Root::Enum(_)));
    assert!(
        reaction
            .root_enum_named("Input")
            .expect("empty Input root")
            .variants
            .is_empty()
    );
    assert!(
        reaction
            .root_enum_named("Output")
            .expect("empty Output root")
            .variants
            .is_empty()
    );
}

// ----------------------------------------------------------------------
// (2) The migrated nexus lowers through the import + root-application path:
//     the imported frame heads resolve, the roots are Root::Application, and
//     the closure records the frame imports.
// ----------------------------------------------------------------------

#[test]
fn migrated_nexus_lowers_to_application_roots_over_the_imported_frame() {
    let migrated = lower_migrated();

    // Both roots are the application form, named by their position, headed by
    // the imported frame type.
    let input = application_root(&migrated, "Input");
    assert_eq!(input.head(), &ApplicationHead::Local(Name::new("Work")));
    assert_eq!(
        input.arguments(),
        &[
            TypeReference::new("SignalInput"),
            TypeReference::new("SemaWriteOutput"),
            TypeReference::new("SemaReadOutput"),
            TypeReference::new("EffectOutcome"),
        ],
    );

    let output = application_root(&migrated, "Output");
    assert_eq!(output.head(), &ApplicationHead::Local(Name::new("Action")));
    assert_eq!(
        output.arguments(),
        &[
            TypeReference::new("SignalOutput"),
            TypeReference::new("SemaWriteSet"),
            TypeReference::new("SemaReadInput"),
            TypeReference::new("EffectCommand"),
            // The Continuation leg binds to spirit's OWN Work application — a
            // nested application argument over the same frame head.
            TypeReference::Application {
                head: ApplicationHead::Local(Name::new("Work")),
                arguments: vec![
                    TypeReference::new("SignalInput"),
                    TypeReference::new("SemaWriteOutput"),
                    TypeReference::new("SemaReadOutput"),
                    TypeReference::new("EffectOutcome"),
                ],
            },
        ],
    );

    // The frame heads are imported, not locally declared.
    assert!(migrated.type_named("Work").is_none());
    assert!(migrated.type_named("Action").is_none());
    let frame_imports = migrated
        .resolved_imports()
        .iter()
        .map(|import| import.local_name().as_str().to_owned())
        .collect::<Vec<_>>();
    assert!(frame_imports.contains(&"Work".to_owned()));
    assert!(frame_imports.contains(&"Action".to_owned()));

    // The imported frame heads carry their arity across the crate boundary:
    // Work is a 4-parameter import, Action a 5-parameter import.
    for (local, arity) in [("Work", 4usize), ("Action", 5usize)] {
        let resolved = migrated.resolved_imports();
        let import = resolved
            .iter()
            .find(|import| import.local_name().as_str() == local)
            .unwrap_or_else(|| panic!("{local} import resolved"));
        assert_eq!(
            import.parameter_count(),
            Some(arity),
            "{local} carries its frame arity across the boundary",
        );
    }
}

// ----------------------------------------------------------------------
// (3) EQUIVALENCE — the migrated frame application expands to exactly the
//     concrete (pre-migration) hand-written Input/Output enum roots.
// ----------------------------------------------------------------------

#[test]
fn migrated_input_frame_expands_to_the_concrete_input_root() {
    let reaction = lower_reaction();
    let migrated = lower_migrated();
    let concrete = lower_concrete();

    // Expand `(Work SignalInput SemaWriteOutput SemaReadOutput EffectOutcome)`.
    let input = application_root(&migrated, "Input");
    let expanded = expand_root(&reaction, "Work", &input);

    // The concrete Input root was hand-written as the same four legs.
    let concrete_variants = concrete_root_variants(&concrete, "Input");

    assert_eq!(
        expanded.as_slice(),
        concrete_variants,
        "the migrated Work application expands to the concrete Input root, leg for leg",
    );

    // Spot the exact per-leg variant -> payload mapping the frame produced.
    let mapping = expanded
        .iter()
        .map(|variant| {
            (
                variant.name.as_str().to_owned(),
                variant.payload.as_ref().map(|payload| match payload {
                    TypeReference::Plain(name) => name.as_str().to_owned(),
                    other => format!("{other:?}"),
                }),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        mapping,
        vec![
            ("SignalArrived".to_owned(), Some("SignalInput".to_owned())),
            (
                "SemaWriteCompleted".to_owned(),
                Some("SemaWriteOutput".to_owned())
            ),
            (
                "SemaReadCompleted".to_owned(),
                Some("SemaReadOutput".to_owned())
            ),
            (
                "EffectCompleted".to_owned(),
                Some("EffectOutcome".to_owned())
            ),
        ],
    );
}

#[test]
fn migrated_output_frame_expands_to_the_concrete_output_root() {
    let reaction = lower_reaction();
    let migrated = lower_migrated();
    let concrete = lower_concrete();

    // Expand `(Action SignalOutput SemaWriteSet SemaReadInput EffectCommand
    // (Work …))`. The Continuation leg's argument is the nested Work
    // application; the concrete baseline binds Continue to the `Work` enum
    // declaration by name, so the concrete payload at that leg is `Work` and
    // the migrated leg's payload is the applied `(Work …)`. Compare the four
    // payload-binding legs exactly, then confirm the Continuation leg name and
    // that its migrated payload is the Work application.
    let output = application_root(&migrated, "Output");
    let expanded = expand_root(&reaction, "Action", &output);
    let concrete_variants = concrete_root_variants(&concrete, "Output");

    // Same variant names, same order, for all five legs.
    assert_eq!(
        expanded
            .iter()
            .map(|variant| variant.name.as_str().to_owned())
            .collect::<Vec<_>>(),
        concrete_variants
            .iter()
            .map(|variant| variant.name.as_str().to_owned())
            .collect::<Vec<_>>(),
        "the migrated Action application expands to the concrete Output legs, in order",
    );

    // The four payload-binding legs match exactly.
    for leg in 0..4 {
        assert_eq!(
            expanded[leg], concrete_variants[leg],
            "Output leg {} matches the concrete root",
            expanded[leg].name,
        );
    }

    // The fifth leg is the Continuation: same name; its migrated payload is the
    // nested Work application (the recursive frame self-reference), whereas the
    // concrete baseline named the local `Work` enum. Both denote spirit's own
    // Work — the migration replaces the by-name reference with the explicit
    // frame application.
    let continuation = &expanded[4];
    assert_eq!(continuation.name.as_str(), "Continue");
    assert_eq!(concrete_variants[4].name.as_str(), "Continue");
    assert_eq!(
        continuation.payload,
        Some(TypeReference::Application {
            head: ApplicationHead::Local(Name::new("Work")),
            arguments: vec![
                TypeReference::new("SignalInput"),
                TypeReference::new("SemaWriteOutput"),
                TypeReference::new("SemaReadOutput"),
                TypeReference::new("EffectOutcome"),
            ],
        }),
    );
    assert_eq!(
        concrete_variants[4].payload,
        Some(TypeReference::new("Work")),
        "the concrete baseline binds Continue to the local Work enum by name",
    );
}

// ----------------------------------------------------------------------
// Spirit is a FULL-FRAME component: it binds all four Work legs and all five
// Action legs, so the migrated roots expand to a complete, gap-free leg set —
// no omittable-leg (uninhabitable-payload) mechanism is exercised here.
// ----------------------------------------------------------------------

#[test]
fn spirit_binds_every_frame_leg_full_frame() {
    let reaction = lower_reaction();
    let migrated = lower_migrated();

    let (work_parameters, _) = reaction
        .declared_frame_body("Work")
        .expect("Work frame body");
    let (action_parameters, _) = reaction
        .declared_frame_body("Action")
        .expect("Action frame body");
    let input = application_root(&migrated, "Input");
    let output = application_root(&migrated, "Output");

    // Every Work binder and every Action binder receives a real argument —
    // arity is full on both heads.
    assert_eq!(work_parameters.len(), input.arguments().len());
    assert_eq!(work_parameters.len(), 4);
    assert_eq!(action_parameters.len(), output.arguments().len());
    assert_eq!(action_parameters.len(), 5);

    // Every expanded leg carries a payload — no leg bound to an absent /
    // uninhabitable type (the omittable-leg mechanism stays unexercised).
    let input_legs = expand_root(&reaction, "Work", &input);
    let output_legs = expand_root(&reaction, "Action", &output);
    for leg in input_legs.iter().chain(output_legs.iter()) {
        assert!(
            leg.payload.is_some(),
            "full-frame leg {} binds a real payload",
            leg.name,
        );
    }
    assert_eq!(input_legs.len(), 4);
    assert_eq!(output_legs.len(), 5);
}
