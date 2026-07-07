//! schema-language-cc — the schema compiler-compiler.
//!
//! The compiler's own definition, kept as typed data, that GENERATES the schema
//! compiler (schema-language / schema-rust), bottoming out in the nota
//! seed. See `INTENT.md` and `ARCHITECTURE.md`.
//!
//! First inhabitant: the reference-resolution grammar — the parenthesis-reference
//! dispatch precedence reified as an ordered typed value that generates the
//! resolver, instead of hand-writing its match-arm ordering. The flow is one
//! direction of typed transformation:
//!
//! ```text
//! NOTA text ─▶ ReferenceGrammar ─(TryFrom)▶ ValidatedReferenceGrammar ─(From)▶ ReferenceDispatch ─▶ Rust source
//! ```
//!
//! - [`grammar`]  — `ReferenceGrammar`: the dispatch precedence as data, decoded
//!   by the nota seed (no hand-rolled parser).
//! - [`validate`] — `ValidatedReferenceGrammar`: catch-all unique and last,
//!   declared-macro before it, no built-in head collision.
//! - [`dispatch`] — `ReferenceDispatch`: emits schema-language's REAL parenthesis
//!   dispatch (a method body over schema-language's own types) from a validated
//!   grammar. It generates code; it never resolves references at runtime.

pub mod dispatch;
pub mod error;
pub mod grammar;
pub mod validate;

pub use dispatch::ReferenceDispatch;
pub use error::Error;
pub use grammar::{ArgumentCount, BuiltinArity, BuiltinHead, ReferenceForm, ReferenceGrammar};
pub use validate::ValidatedReferenceGrammar;
