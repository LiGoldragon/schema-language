# schema-language-cc — agent contract

Read `ARCHITECTURE.md` first, then this file, then `skills.md`.

schema-language-cc is the schema **compiler-compiler**: the compiler's own definition kept
as typed data that *generates* the schema compiler, bottoming out in the
nota seed. It is **build-time only** — it never links into a runtime binary
(Spirit `9rjq`).

## Where it sits

`nota` (the frozen seed) → **`schema-language-cc`** (the definition as data) →
`schema-language` / `schema-rust` (the generated compiler). schema-language-cc must
**not** depend on `schema-language` — it generates into it; the reverse edge is a
cycle.

## Discipline (before authoring Rust, read `skills.md`)

- Verbs on data-bearing nouns; **no** free functions (outside `fn main` /
  `#[cfg(test)]`) and **no** ZST namespace holders.
- One typed `Error` enum via `thiserror` in `src/error.rs`; no `anyhow`/`eyre`.
- Decode NOTA through nota's `StructuralMacroNode` — **never** a hand-rolled
  parser (the format already has one: the seed). A leaf the derive vocabulary
  can't express gets a hand-written `StructuralMacroNode` trait impl, not string
  slicing.
- Domain values are typed newtypes (private field); one concern per src file;
  tests under `tests/`; cross-crate deps are `git=`, never `path=`.
- **Generate, don't interpret:** schema-language-cc emits compiler Rust; it never resolves
  references at runtime. A runtime grammar-interpreter would be a second,
  inconsistent mechanism.

## Workspace contract

This repo lives under the primary workspace; its cross-cutting agent contract,
roles, intent layer, and version-control discipline govern. The design rationale,
the leans taken, and the migration roadmap are in designer reports `649`
(precedence-as-data) and `652` (schema-language-cc design); operator review in `384`.
