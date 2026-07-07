//! Emission of schema-language's REAL parenthesis-reference dispatch from a
//! validated grammar, in declared precedence.

use nota::StructuralMacroNode;
use schema_language_cc::{ReferenceDispatch, ReferenceGrammar, ValidatedReferenceGrammar};

const CANONICAL: &str = "(ReferenceGrammar (Builtin Vector 1) (Builtin Optional 1) \
                         (Builtin ScopeOf 1) (Builtin Map 2) (Builtin Bytes Atom) \
                         DeclaredMacro Application)";

fn emit(nota: &str) -> ReferenceDispatch {
    let grammar = ReferenceGrammar::from_structural_nota(nota).expect("grammar decodes");
    let validated = ValidatedReferenceGrammar::try_from(grammar).expect("grammar validates");
    ReferenceDispatch::from(&validated)
}

#[test]
fn emitted_tokens_parse_as_valid_rust() {
    let dispatch = emit(CANONICAL);
    syn::parse2::<syn::File>(dispatch.tokens().clone()).expect("emitted dispatch is valid Rust");
}

#[test]
fn emitted_dispatch_keeps_builtin_arms_in_declared_order() {
    let source = emit(CANONICAL).to_dispatch_source();

    let vector = source.find("resolve_vector").expect("Vector arm present");
    let optional = source
        .find("resolve_optional")
        .expect("Optional arm present");
    let scope_of = source
        .find("resolve_scope_of")
        .expect("ScopeOf arm present");
    let map = source.find("resolve_map").expect("Map arm present");
    let bytes = source.find("resolve_bytes").expect("Bytes arm present");

    assert!(
        vector < optional && optional < scope_of && scope_of < map && map < bytes,
        "built-in arms must appear in the grammar's declared order:\n{source}"
    );
}

#[test]
fn builtin_arm_dispatches_to_the_snake_case_resolver_method() {
    let source = emit(CANONICAL).to_dispatch_source();
    // ScopeOf -> resolve_scope_of: the method name is the head in snake_case.
    assert!(
        source.contains("Self::resolve_scope_of(block, objects, registry, context)"),
        "ScopeOf dispatches to resolve_scope_of with the uniform argument list:\n{source}"
    );
    // The Map arm guards on the two-argument object count (head + 2 args).
    assert!(
        source.contains("head == Some(\"Map\") && object_count == 3usize"),
        "the Map arm guards arity 3 (head plus two arguments):\n{source}"
    );
}

#[test]
fn emitted_dispatch_orders_guard_then_application_tail() {
    let source = emit(CANONICAL).to_dispatch_source();

    let last_builtin = source
        .find("resolve_bytes")
        .expect("last built-in arm present");
    let reserved_guard = source
        .find("RESERVED_BUILTIN_HEADS")
        .expect("reserved-head guard present");
    let application_tail = source
        .find("from_macro_or_application")
        .expect("registry-then-application tail present");

    assert!(
        last_builtin < reserved_guard,
        "the reserved-head guard follows every built-in arm:\n{source}"
    );
    assert!(
        reserved_guard < application_tail,
        "the registry-then-application tail is last:\n{source}"
    );
}

#[test]
fn reserved_guard_lists_every_builtin_head() {
    let source = emit(CANONICAL).to_dispatch_source();
    // The guard is derived from the Builtin set, not hand-listed: each head
    // appears both in its dispatch arm guard and in the reserved-head set.
    for head in ["Vector", "Optional", "ScopeOf", "Map", "Bytes"] {
        let needle = format!("\"{head}\"");
        let occurrences = source.matches(&needle).count();
        assert!(
            occurrences >= 2,
            "{head} appears in both its arm and the reserved-head set; found {occurrences}:\n{source}"
        );
    }
}

#[test]
fn emitted_dispatch_targets_schema_types() {
    let source = emit(CANONICAL).to_dispatch_source();
    // The emission names schema-language's real types — co-located, so the
    // generated source compiles into schema, not schema-language-cc.
    assert!(
        source.contains("impl TypeReference"),
        "emits into TypeReference's impl:\n{source}"
    );
    assert!(
        source.contains("-> Result<Self, SchemaError>"),
        "returns schema-language's Self/SchemaError:\n{source}"
    );
    assert!(
        source.contains("SchemaError::UnknownTypeReferenceForm"),
        "the reserved-head guard builds schema-language's error variant:\n{source}"
    );
}

#[test]
fn emitted_dispatch_matches_golden_source() {
    let source = emit(CANONICAL).to_dispatch_source();
    assert_eq!(
        source, GOLDEN,
        "emitted dispatch drifted from the golden source:\n{source}"
    );
}

#[test]
fn grammar_without_a_registry_rung_still_emits_the_application_tail() {
    // The DeclaredMacro and Application markers both map to the one
    // `from_macro_or_application` tail in schema, so dropping the registry
    // rung does not remove a stage from the emitted body — the tail is the same
    // either way. The built-in arm and reserved guard still emit.
    let source = emit("(ReferenceGrammar (Builtin Vector 1) Application)").to_dispatch_source();
    assert!(
        source.contains("Self::resolve_vector(block, objects, registry, context)"),
        "the declared built-in arm is present:\n{source}"
    );
    assert!(
        source.contains("from_macro_or_application"),
        "the application tail is still emitted:\n{source}"
    );
}

const GOLDEN: &str = r#"impl TypeReference {
    /// Lower a parenthesised reference. GENERATED by schema-language-cc
    /// from the canonical `ReferenceGrammar` — do not edit by
    /// hand; edit the grammar and regenerate (see this crate's
    /// `build.rs`).
    ///
    /// Dispatch order is the grammar's declared precedence,
    /// which is deliberately not compiler-checked (the
    /// application form structurally overlaps every built-in
    /// and declared head): each canonical built-in head
    /// (`(Vector T)`, `(Optional T)`, `(ScopeOf T)`,
    /// `(Map K V)`, `(Bytes N)`) is the direct fast path; a
    /// reserved head at the wrong arity is an error, never a
    /// fall-through; then the registry-then-application tail.
    pub(crate) fn resolve_parenthesis_reference(
        block: &Block,
        objects: &[Block],
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<Self, SchemaError> {
        let head = objects.first().and_then(Block::demote_to_string);
        let object_count = objects.len();
        if head == Some("Vector") && object_count == 2usize {
            return Self::resolve_vector(block, objects, registry, context);
        }
        if head == Some("Optional") && object_count == 2usize {
            return Self::resolve_optional(block, objects, registry, context);
        }
        if head == Some("ScopeOf") && object_count == 2usize {
            return Self::resolve_scope_of(block, objects, registry, context);
        }
        if head == Some("Map") && object_count == 3usize {
            return Self::resolve_map(block, objects, registry, context);
        }
        if head == Some("Bytes") && object_count == 2usize {
            return Self::resolve_bytes(block, objects, registry, context);
        }
        const RESERVED_BUILTIN_HEADS: &[&str] = &[
            "Vector",
            "Optional",
            "ScopeOf",
            "Map",
            "Bytes",
        ];
        if let Some(head) = head && RESERVED_BUILTIN_HEADS.contains(&head) {
            return Err(SchemaError::UnknownTypeReferenceForm {
                head: head.to_owned(),
                argument_count: object_count.saturating_sub(1),
            });
        }
        Self::from_macro_or_application(block, registry, context)
    }
}
"#;
