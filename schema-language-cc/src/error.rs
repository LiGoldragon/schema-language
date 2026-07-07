//! The one typed error for `schema-language-cc`.

use crate::grammar::BuiltinHead;
use thiserror::Error;

/// Everything that can go wrong decoding, validating, or generating from a
/// [`ReferenceGrammar`](crate::grammar::ReferenceGrammar).
#[derive(Debug, Clone, Error)]
pub enum Error {
    /// The NOTA text did not decode into a `ReferenceGrammar` value.
    #[error("could not decode reference grammar from NOTA: {0}")]
    Decode(String),

    /// The grammar carries more than one application catch-all; precedence is
    /// then ambiguous because two tails could both claim a block.
    #[error("more than one Application catch-all (found {count}); the catch-all must be unique")]
    DuplicateApplication { count: usize },

    /// The application catch-all is present but is not the final form, so the
    /// forms after it are unreachable.
    #[error(
        "Application catch-all is not last (at position {position} of {total}); \
         it shadows every later form"
    )]
    ApplicationNotLast { position: usize, total: usize },

    /// Two `Builtin` forms claim the same head, so the second arm is dead.
    #[error("Builtin head {0} is declared more than once; the second arm is unreachable")]
    DuplicateBuiltinHead(BuiltinHead),

    /// The grammar has no application catch-all, so a reference whose head
    /// matches no built-in and no declared macro would resolve to nothing.
    /// Every coherent reference grammar ends in a catch-all.
    #[error("no Application catch-all; every reference grammar must end in one")]
    MissingApplication,

    /// More than one declared-macro marker; the registry stage is one rung.
    #[error(
        "more than one DeclaredMacro marker (found {count}); the registry stage is a single rung"
    )]
    DuplicateDeclaredMacro { count: usize },

    /// A `Builtin` form follows a fallback marker. Built-ins are the most
    /// specific forms and must precede the declared-macro and application
    /// fallbacks, so the precedence reads specific-to-general top to bottom.
    #[error(
        "Builtin form at position {position} follows a fallback marker; \
         built-ins must precede the DeclaredMacro and Application markers"
    )]
    BuiltinAfterMarker { position: usize },
}
