//! Validation lifts the conflict checks that match-arm ordering could not
//! express into typed errors on a `ReferenceGrammar`.

use nota::StructuralMacroNode;
use schema_language_cc::{Error, ReferenceGrammar, ValidatedReferenceGrammar};

const CANONICAL: &str = "(ReferenceGrammar (Builtin Vector 1) (Builtin Optional 1) \
                         (Builtin ScopeOf 1) (Builtin Map 2) (Builtin Bytes Atom) \
                         DeclaredMacro Application)";

fn validate(nota: &str) -> Result<ValidatedReferenceGrammar, Error> {
    let grammar = ReferenceGrammar::from_structural_nota(nota).expect("grammar decodes");
    ValidatedReferenceGrammar::try_from(grammar)
}

#[test]
fn canonical_grammar_validates() {
    let validated = validate(CANONICAL).expect("canonical grammar is sound");
    assert_eq!(validated.forms().len(), 7);
}

#[test]
fn rejects_application_not_last() {
    let nota = "(ReferenceGrammar (Builtin Vector 1) Application DeclaredMacro)";
    let error = validate(nota).expect_err("catch-all before later forms is unsound");
    assert!(
        matches!(
            error,
            Error::ApplicationNotLast {
                position: 1,
                total: 3
            }
        ),
        "got {error:?}"
    );
}

#[test]
fn rejects_duplicate_application() {
    let nota = "(ReferenceGrammar (Builtin Vector 1) DeclaredMacro Application Application)";
    let error = validate(nota).expect_err("two catch-alls make precedence ambiguous");
    assert!(
        matches!(error, Error::DuplicateApplication { count: 2 }),
        "got {error:?}"
    );
}

#[test]
fn rejects_duplicate_builtin_head() {
    let nota = "(ReferenceGrammar (Builtin Vector 1) (Builtin Vector 2) DeclaredMacro Application)";
    let error = validate(nota).expect_err("a head declared twice leaves a dead arm");
    match error {
        Error::DuplicateBuiltinHead(head) => assert_eq!(head.as_str(), "Vector"),
        other => panic!("expected DuplicateBuiltinHead, got {other:?}"),
    }
}

#[test]
fn declared_macro_after_application_is_caught_as_not_last() {
    // A declared-macro marker trailing the catch-all is the same flaw as the
    // catch-all not being last; the single ordering rule catches it.
    let nota = "(ReferenceGrammar (Builtin Vector 1) Application DeclaredMacro)";
    let error = validate(nota).expect_err("a marker after the catch-all is unsound");
    assert!(
        matches!(
            error,
            Error::ApplicationNotLast {
                position: 1,
                total: 3
            }
        ),
        "got {error:?}"
    );
}

#[test]
fn rejects_grammar_without_a_catch_all() {
    // Every coherent reference grammar must end in an application catch-all;
    // without one, a reference matching no built-in and no macro would resolve
    // to nothing. The generator must never invent a catch-all the data omits.
    let nota = "(ReferenceGrammar (Builtin Vector 1) DeclaredMacro)";
    let error = validate(nota).expect_err("a catch-all-free grammar is unsound");
    assert!(matches!(error, Error::MissingApplication), "got {error:?}");
}

#[test]
fn rejects_builtin_after_a_marker() {
    // Built-ins are the most specific forms; one trailing a fallback marker
    // breaks specific-to-general precedence and has no well-defined position
    // relative to the generated reserved-head guard.
    let nota = "(ReferenceGrammar DeclaredMacro (Builtin Vector 1) Application)";
    let error = validate(nota).expect_err("a built-in after a marker is unsound");
    assert!(
        matches!(error, Error::BuiltinAfterMarker { position: 1 }),
        "got {error:?}"
    );
}

#[test]
fn rejects_duplicate_declared_macro() {
    let nota = "(ReferenceGrammar (Builtin Vector 1) DeclaredMacro DeclaredMacro Application)";
    let error = validate(nota).expect_err("the registry stage is a single rung");
    assert!(
        matches!(error, Error::DuplicateDeclaredMacro { count: 2 }),
        "got {error:?}"
    );
}
