//! Decode and round-trip the reference grammar through the nota seed.

use nota::StructuralMacroNode;
use schema_language_cc::{BuiltinArity, BuiltinHead, ReferenceForm, ReferenceGrammar};

const CANONICAL: &str = "(ReferenceGrammar (Builtin Vector 1) (Builtin Optional 1) \
                         (Builtin ScopeOf 1) (Builtin Map 2) (Builtin Bytes Atom) \
                         DeclaredMacro Application)";

fn builtin(head: &str, arity: BuiltinArity) -> ReferenceForm {
    ReferenceForm::Builtin(BuiltinHead::try_from(head).expect("PascalCase head"), arity)
}

#[test]
fn decodes_canonical_grammar_in_declared_order() {
    let grammar = ReferenceGrammar::from_structural_nota(CANONICAL).expect("canonical decodes");

    assert_eq!(
        grammar.forms(),
        &[
            builtin("Vector", BuiltinArity::count(1)),
            builtin("Optional", BuiltinArity::count(1)),
            builtin("ScopeOf", BuiltinArity::count(1)),
            builtin("Map", BuiltinArity::count(2)),
            builtin("Bytes", BuiltinArity::Atom),
            ReferenceForm::DeclaredMacro,
            ReferenceForm::Application,
        ]
    );
}

#[test]
fn builtin_form_exposes_its_head() {
    let grammar = ReferenceGrammar::from_structural_nota(CANONICAL).expect("canonical decodes");
    let heads: Vec<&str> = grammar
        .forms()
        .iter()
        .filter_map(ReferenceForm::builtin_head)
        .map(BuiltinHead::as_str)
        .collect();
    assert_eq!(heads, ["Vector", "Optional", "ScopeOf", "Map", "Bytes"]);
}

#[test]
fn atom_arity_is_distinct_from_a_count() {
    let bytes = builtin("Bytes", BuiltinArity::Atom);
    let ReferenceForm::Builtin(_, arity) = &bytes else {
        panic!("Bytes is a Builtin form");
    };
    assert_eq!(*arity, BuiltinArity::Atom);
    assert_eq!(arity.block_object_count(), 2);
    // A single type argument also occupies one trailing slot, but it is a
    // different value than the Atom marker.
    assert_ne!(BuiltinArity::count(1), BuiltinArity::Atom);
    assert_eq!(BuiltinArity::count(1).block_object_count(), 2);
    assert_eq!(BuiltinArity::count(2).block_object_count(), 3);
}

#[test]
fn canonical_grammar_round_trips_through_nota() {
    let grammar = ReferenceGrammar::from_structural_nota(CANONICAL).expect("canonical decodes");
    let reencoded = grammar.to_structural_nota();
    let redecoded =
        ReferenceGrammar::from_structural_nota(&reencoded).expect("re-encoded grammar decodes");
    assert_eq!(redecoded, grammar);
    assert_eq!(reencoded, CANONICAL);
}
