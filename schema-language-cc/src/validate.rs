//! `ValidatedReferenceGrammar` — a [`ReferenceGrammar`] that carries the
//! invariant the generator relies on.
//!
//! These are exactly the conflict checks that hand-written match-arm ordering
//! could never express as data: a resolver written as a `match` *implies* its
//! precedence in the source order of its arms, and nothing checks that the
//! catch-all is last or that two arms don't claim the same head. Lifting the
//! precedence into a [`ReferenceGrammar`] value makes those rules checkable —
//! this is the registry-aware analogue of nota's
//! `StructuralVariantSet::validate_no_silent_conflicts`.

use crate::error::Error;
use crate::grammar::{BuiltinHead, ReferenceForm, ReferenceGrammar};
use std::collections::BTreeSet;

/// A [`ReferenceGrammar`] proven to generate a sound resolver. The enforced
/// shape is `Builtin* DeclaredMacro? Application`: built-ins form the
/// specific-first prefix, then at most one declared-macro registry rung, then a
/// required application catch-all that is unique and last; and no built-in head
/// is declared twice. So no valid grammar can generate a resolver stage the
/// grammar does not itself declare.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedReferenceGrammar(ReferenceGrammar);

impl ValidatedReferenceGrammar {
    /// The validated forms in declared precedence order.
    pub fn forms(&self) -> &[ReferenceForm] {
        self.0.forms()
    }
}

impl TryFrom<ReferenceGrammar> for ValidatedReferenceGrammar {
    type Error = Error;

    fn try_from(grammar: ReferenceGrammar) -> Result<Self, Self::Error> {
        let forms = grammar.forms();
        let total = forms.len();

        // Built-ins are the most specific forms; they must precede every
        // fallback marker, so the precedence reads specific-to-general and the
        // generated reserved-head guard has a well-defined position.
        let first_marker = forms
            .iter()
            .position(|form| !matches!(form, ReferenceForm::Builtin(..)));
        if let Some(marker) = first_marker
            && let Some((position, _)) = forms
                .iter()
                .enumerate()
                .skip(marker + 1)
                .find(|(_, form)| matches!(form, ReferenceForm::Builtin(..)))
        {
            return Err(Error::BuiltinAfterMarker { position });
        }

        // No built-in head declared twice — the second arm would be dead.
        let mut seen_heads: BTreeSet<&BuiltinHead> = BTreeSet::new();
        for head in forms.iter().filter_map(ReferenceForm::builtin_head) {
            if !seen_heads.insert(head) {
                return Err(Error::DuplicateBuiltinHead(head.clone()));
            }
        }

        // The registry stage is consulted at most once.
        let declared_macro_count = forms
            .iter()
            .filter(|form| matches!(form, ReferenceForm::DeclaredMacro))
            .count();
        if declared_macro_count > 1 {
            return Err(Error::DuplicateDeclaredMacro {
                count: declared_macro_count,
            });
        }

        // Exactly one application catch-all, and it is the final form.
        let application_positions: Vec<usize> = forms
            .iter()
            .enumerate()
            .filter(|(_, form)| matches!(form, ReferenceForm::Application))
            .map(|(position, _)| position)
            .collect();
        match application_positions.as_slice() {
            [] => return Err(Error::MissingApplication),
            [position] if *position != total - 1 => {
                return Err(Error::ApplicationNotLast {
                    position: *position,
                    total,
                });
            }
            [_] => {}
            many => return Err(Error::DuplicateApplication { count: many.len() }),
        }

        Ok(Self(grammar))
    }
}
