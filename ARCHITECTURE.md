# Architecture

`schema-language` is the build-time authored `.schema` parser and lowering
bridge. It parses NOTA-shaped `.schema` source, holds the typed source model,
lowers into the semantic schema value, owns schema identity and evolution, and
exposes the typed values consumed by `schema-rust`. It contains the schema
library source extracted from the old `schema` repository so producers can move
off that repository before `schema` is repurposed as the future live runtime
component.

This document records both the current implementation and the accepted target
design of the schema/NOTA type system. The target design is the design of
record: where current code diverges, the divergence is named as work to do, not
as settled shape. Open questions are marked OPEN and are not to be treated as
decided.

## Direction

This is replacement-oriented staging, not compatibility as design. The crate is
named `schema_language`, the package is `schema-language`, and this repo does
not provide a permanent `schema` re-export or shim. The current implementation
still carries a temporary string-bearing authored-schema model needed by
existing producers; that is acceptable only as execution staging.

The accepted end design has two orthogonal movements that can proceed in
parallel:

- a data-model split of the semantic schema into a stringless `CoreSchema`
  substrate, a `NameTable`, and a `TrueSchema` view; and
- a text-projection change (dotted-everywhere) that makes `.schema` source
  strictly positional and removes every name-adjacency form.

Runtime components should not link this build-time parser/lowering bridge; they
depend on generated Rust and on strict binary/text contract surfaces.

## Foundational tenets

These tenets govern both the source language and the semantic model. They are
the gate every schema/NOTA form is checked against.

- Strict typed positional data. The expected type plus position determines
  every value. The type is always known ahead at every NOTA boundary: file
  kind, schema slot, field position, variant payload, operation argument, and
  reply slot. Type is never inferred from surface syntax.
- Positionality is the golden rule. There is no named binding, no keyword
  argument, and no label-in-value anywhere. A name at a use site is only ever a
  schema-required disambiguator or a reference/path/name value; it never
  identifies a position.
- Strict known root count. A schema document has exactly the root slots its
  document type requires, and every slot is always present. Optionality is an
  empty typed slot, never a changed root count.
- No repetition. Convoluted, special-cased, or duplicated code is evidence that
  the model is wrong or incomplete, not a thing to be maintained.

### Capitalization is semantic, not a decoder input

Capitalization carries meaning at the schema-source layer and mirrors the
always-known type at the value layer:

- A capitalized leading atom is an object: a type, generic, or variant. It
  lowers to a Rust object.
- A lowercase-leading atom is a name or reference: a field role, a path
  segment, or an alias. It introduces no new lowered object.

Capitalization is not a runtime decoder input. The fixed slot type decides
string-versus-variant; a bare atom value may be capitalized, and a capitalized
string needs no delimiter. Structural lowering never reads case to choose a
value category.

## Core and True schema

The semantic schema is one model viewed two ways. Today the model is a single
string-bearing `TrueSchema` data tree; the target design splits it.

### Target model

- `CoreSchema` is the stringless substrate. Every declaration — type, field,
  variant, and generic — carries a minted nominal identifier allocated once at
  introduction and preserved across all edits, including rename. Nominal
  identity is preserved: two declarations with equal structure but distinct
  identifiers (for example `Meters` and `Seconds`) are different types.
- `NameTable` maps identifier to current name.
- `TrueSchema` is a view assembled from `CoreSchema` plus `NameTable` through
  methods. It is not an independent data tree; at most it is a small codec
  sidecar. Human-facing names come from the `NameTable`, structure from the
  `CoreSchema`.

### Hashing and lineage

- The core hash is over `CoreSchema` — nominal identifiers plus structure —
  with `SchemaIdentity` (component name and authored version) pulled out of the
  core-hashed bytes.
- The core hash is a lineage address. Equal core hash means compatible, shared
  ancestry; a common ancestor is found by core hash.
- A separate true/name hash may exist for the human view. It moves on rename;
  the core hash does not.

### Evolution runs on the core

- Migration and evolution run on `CoreSchema`.
- A `Rename` edit touches only the `NameTable` and emits zero migration code.
- Structural edits (`AddField`, `ChangeFieldType`, `AddVariant`) change core
  bytes and emit historical-to-current `From` implementations.

### OPEN: identifier reuse on reload

The mechanism by which the schema daemon re-associates a reloaded or modified
schema's declarations with their already-allocated identifiers is OPEN and being
weighed separately. It is the linchpin of "allocated once, preserved across
edits": unchanged declarations and renames must keep their identifiers, and only
genuinely new declarations mint fresh ones. Do not design or assume a specific
mechanism here until it is decided.

## Generics

There are no builtin generics; `Vector` is not magic. The builtin mechanism is a
closed set of generic-definition kinds, distinguished by meta-shape (lowering
strategy), not by arity and not by name.

- Kinds are cheap; a Rust enum has ample variant headroom. There should be as
  many kinds as needed so each meta-shape gets its simplest syntax, and simple
  cases must not pay for complex ones. Candidate kinds (names illustrative, not
  final): single-type-parameter (`Name.Arg`), multi-type-parameter
  (`Name.(A B)`, where arity is data in the definition, not a kind-per-count),
  value/const-parameter (a fixed-bytes-style kind whose argument is a value),
  and template/frame (named parameters plus a body).
- `Vector`, `Optional`, and `ScopeOf` are named definitions of the
  single-type-parameter kind. They are not separate kinds.
- Application is dotted and positional: `Vector.Domain`, `Map.(Key Value)`,
  `Work.(A B)`. `X.Y.Z` is left-associative unary nesting; a multi-argument
  application requires a group; `Map.Key.Value` must fail Map's arity.
- Lowering dispatches on kind or variant, never on the string name. The
  reference type mirrors the kind partition (single, multi, const, template
  application variants); it is neither one uniform application variant nor a
  per-name variant set.
- Field-name derivation from the type name is intended per-generic behavior and
  is correct (`Vector.X` gives `x_vector`, `Optional.X` gives `optional_x`,
  `ScopeOf.X` gives `x_scope`). Its pattern must be data carried on the
  definition and defaulted by kind, not a `match "Vector"` in Rust, so a defined
  generic such as a `List` or `Maybe` derives correctly (`x_list`, `maybe_x`).
- Validation rejects duplicate generic rows and duplicate frame parameters.

## Dotted-everywhere source projection

The dotted-everywhere change is a text-projection change only. It is data-safe:
the semantic model, runtime state, and wire are untouched.

The dot replaces all name-adjacency-value forms:

- data-carrying variants `(Variant Data)` become `Variant.Data`;
- inline enum `Variant.[...]` and inline struct `Variant.{...}`;
- raw NOTA map entries `{ k v }` become `{ k.v }`;
- imports and generic adjacency become dotted;
- import path colons become dots: `signal-spirit.signal.Entry` is a
  left-associative segment chain of lowercase segments ending in the
  capitalized target.

A map entry splits on the first top-level dot. The key is one dotless block;
keys are atoms only, with no non-atom or structured keys. The value may be
dotted.

### The three legitimate lowercase-name uses

A leading lowercase name (`name.Value`) is legitimate in exactly three places:

- struct-field disambiguation, required only when the field type is duplicated
  within the struct;
- dotted import paths; and
- namespace readability aliases.

### Readability aliases

Readability aliases are source-only sugar. They have no `CoreSchema` or
`TrueSchema` representation; the referenced object inlines at lowering, and
re-emission does not restore the alias. Aliases legitimately do not round-trip.

OPEN: the exact rules for readability aliases are not fully pinned. OPEN: an
optional decode-out depth cap could programmatically invent names for legible
help printing; it would be intentionally non-round-tripping. Neither is decided.

### Named-brace application is not valid schema

The `Family { record.StoredRecord ... }` named-brace application form is
confused-agent drift, not valid schema. Generic application is positional and
dotted; parameter names live only in the definition. Schemas using the
named-brace form must be rewritten to positional dotted application.

### Blast radius

The blast radius of the dotted-everywhere change is schema source only:

- the grammar and parser;
- the re-emitter — `to_schema_text` / `to_nota_source` must round-trip semantic
  content;
- checked-in `.schema` files; and
- regenerated Rust.

Runtime state and wire are rkyv binary keyed by field position and are untouched
by the text change. Field order remains the compatibility surface.

## Current implementation and required removals

The current code carries the temporary string-bearing model. The target design
above requires the following removals and unifications. Each is stated as
current fact plus the required change.

### Two reference pipelines to unify

Two reference-resolution pipelines exist:

- A legacy parenthesized, string-name-keyed resolver. Its entry is
  `TypeReference::from_block` (`src/schema.rs`), which delegates to
  `from_block_with_registry` (`src/schema.rs`) and, on a parenthesis block,
  dispatches through the generated `reference_resolver_generated.rs` seeded by
  `schemas/reference-grammar.nota`. It classifies heads with
  `ReferenceHead::classify` (`src/schema.rs`) and accepts `(Vector T)`,
  `(Optional T)`, `(ScopeOf T)`, `(Map K V)`, and `(Bytes N)`.
- A newer dotted, variant-dispatched source reader in `src/source.rs`. Its
  entry is `SchemaSource::from_schema_text` / `from_document`; its reference
  type is `SourceReference`, read by `SourceReferenceReader` over the dotted
  grammar and resolved by variant (not by string) through
  `SourceVariantResolver` on the `SourceGenericBuiltin` variant.

The target is to unify onto the dotted reader, delete the parenthesized
name-keyed resolver, retire the name-keyed grammar seed
(`schemas/reference-grammar.nota` and its `schema-language-cc` generator), and
repoint the parenthesized-pipeline callers.

Note on the design's earlier phrasing: `from_block_with_registry` belongs to the
legacy parenthesized pipeline in `src/schema.rs`, not to the dotted reader in
`src/source.rs`. The dotted reader's entry is `SourceReference` /
`SchemaSource::from_document`. The unification intent is unchanged; the callers
to repoint are the parenthesized-pipeline consumers.

### Per-name generics collapse to per-kind

`GenericBuiltin` (`src/schema.rs`) currently has per-name variants `Vector`,
`Optional`, `ScopeOf`, `Map`, `FixedBytes`, plus `Frame(GenericFrame)`.
`TypeReference` currently carries both per-name variants (`Vector`, `Optional`,
`ScopeOf`, `Map`, `Bytes`, `FixedBytes`) and a single uniform
`Application { head, arguments }` variant. The collapse removes the per-name
variants and makes the reference type mirror the kind partition, so lowering
dispatches on kind and never on a name.

Vocabulary note: the semantic side names the value kind `FixedBytes`; the
reference-head and grammar side names it `Bytes` with a width leaf. Unifying the
model should reconcile this to one name.

### Self-tagged shortcut to remove

`SourceVariantSignature::SelfTagged` (`src/source.rs`) implements the
same-named self-tag `(Name)` shortcut (a variant named `Name` carrying type
`Name`). It is invalid under strict positional design and is to be removed; the
form must be written explicitly (`Name.Name`).

### Field-name derivation to definition data

`derived_field_name` exists in two copies — `TypeReference::derived_field_name`
(`src/schema.rs`) and `SourceReference::derived_field_name` (`src/source.rs`) —
and both dispatch on per-name variants and on scalar string names. The target
moves the derivation pattern onto the generic definition, defaulted by kind, so
defined generics derive correctly without a name match.

### Validation to wire up

`SchemaError::DuplicateTypeParameter` (`src/engine.rs`) is defined but never
constructed. Duplicate generic rows and duplicate frame parameters must be
rejected, wiring this error to a real check.

### Rename edit to add

`SchemaEdit` (`src/upgrade.rs`) has exactly `AddField`, `ChangeFieldType`, and
`AddVariant`; there is no `Rename` edit. `AddVariant` already produces a receipt
with no migration spec, so a zero-migration edit shape has precedent, but a
name-only `Rename` edit that touches only the `NameTable` does not yet exist.

### Identity is inside the core-hashed bytes today

`TrueSchema` (`src/schema.rs`) holds `identity: SchemaIdentity` as its first
field. Two hash domains already exist in `src/identity.rs`: the whole-schema
domain hashes the full semantic value including `SchemaIdentity`, so it is not a
pure-structure address, and a rename moves it. The per-family-closure domain
already excludes `SchemaIdentity` and is a pure-structure address, but it is
per-family, not the whole-schema lineage address the target design needs. The
target pulls `SchemaIdentity` out of the whole-schema core hash so the core hash
becomes the structural lineage address, and renames stop moving it.

### Document root count is variable today

`SchemaSource::from_document` (`src/source.rs`) accepts a variable root count of
four to six objects and infers imports-presence from whether the first two roots
are brace maps. Order is optional leading imports, then generics, input, output,
namespace, then optional trailing relations. This variable count and
brace-shape inference is the anti-pattern the strict-known-root-count tenet
removes: the document type should fix the slot count, with an always-present
(possibly empty) imports slot. The imports section is read in `from_document`
via `SourceImports::from_block`; the namespace section is read via
`SourceNamespace::from_block` (both `src/source.rs`).

## Checked-in schema files

- `schemas/root.schema` is the self-describing schema of the schema root type.
  It still teaches legacy forms — its `TypeReference` declaration mixes dotted
  (`Vector.TypeReference`, `Optional.TypeReference`) with parenthesized
  (`(Map TypeReferencePair)`, `(Plain Name)`). It is not unused:
  `tests/lowering.rs`, `tests/operator_271_closed_claims.rs`, and the
  `flake.nix` lint depend on it. It must be rewritten to the strict dotted,
  per-kind model with its dependents updated, not deleted.
- `schemas/core.schema` is the builtin-macro-library schema. Its namespace
  declares a type named `CoreSchema` (the macro library), which is unrelated to
  the target stringless `CoreSchema` substrate. This name collision is a
  hazard; the builtin-macro-library `CoreSchema` should be renamed so the
  substrate name is free.
- `schemas/reference-grammar.nota` is the seed for the legacy parenthesized
  resolver. It is slated for retirement with that resolver.

## Boundaries

- `schema-language` owns authored `.schema` parsing, the typed source model,
  lowering into the semantic schema value, schema identity and evolution, and
  (until retired) the build-time resolver generated from the checked-in
  reference grammar.
- `schema-rust` owns Rust source emission from typed schema data.
- The old `schema` repository is the extraction source for this wave and
  remains intact until it is intentionally repurposed as the live runtime
  component.
- This repository is not a runtime daemon, storage owner, or public authority
  surface.

## Build-time resolver generation (legacy, slated for removal)

The workspace member `schema-language-cc` is a build-time generator. It decodes
and validates `schemas/reference-grammar.nota`, emits the parenthesis-reference
resolver source, and the root build script freshness-checks the committed
generated file. It never links into runtime components. This whole path is the
legacy name-keyed pipeline and is slated for removal once the dotted reader is
the single reference path.

## Sequencing (recommendation)

- The dotted-everywhere source sweep (text projection) and the
  `CoreSchema`/`TrueSchema` data-model build are orthogonal and can proceed in
  parallel.
- The per-name-to-per-kind generic collapse and the two-pipeline unification are
  the core simplification work; they touch generic definition, reference
  resolution, and lowering together.
- Identifier reuse on reload is parked (OPEN) and is sequenced separately.
