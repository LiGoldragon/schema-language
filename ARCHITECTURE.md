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

The `Family { record.StoredRecord ... }` / `Stream { token.Token ... }`
named-brace application form is confused-agent drift, not valid schema.
Generic application is positional and dotted; parameter names live only in the
definition. Schemas using the named-brace form must be rewritten to positional
dotted application, for example `Family.(StoredRecord stored_records Domain)`
or `Stream.(Token Opened Event Closed)`.

### Blast radius

The blast radius of the dotted-everywhere change is schema source only:

- the grammar and parser;
- the re-emitter — `to_schema_text` / `to_nota_source` must round-trip semantic
  content;
- checked-in `.schema` files; and
- regenerated Rust.

Runtime state and wire are rkyv binary keyed by field position and are untouched
by the text change. Field order remains the compatibility surface.

## Current implementation and remaining work

The source-facing layer has largely converged on the strict positional design.
Reference reading, the generic-definition model, the document layout, and the
use-site vocabularies are landed and witnessed; the remaining divergence is in
the semantic `TypeReference` and the evolution model. Each item below is current
fact plus, where work remains, the required change.

### Reference parsing: the dotted source reader is the accepted mechanism

The single source-facing reference entry is the hand-written dotted reader in
`src/source.rs`: `SchemaSource::from_schema_text` / `from_document`,
`SourceReference`, and the per-context readers (`SourceImports::from_block`,
`SourceNamespace::from_block`, and the metadata, product, and relation readers).
`TypeReference::from_block` delegates to `SourceReference::from_block` and then
projects to the temporary semantic `TypeReference`; macro template reference
expansion re-parses its expanded object stream through the same reader.

This hand-written per-context reader is the accepted type-reference and dispatch
mechanism, not a stopgap. The generated parenthesized, string-name-keyed
resolver pipeline was deliberately deleted: there is no `build.rs`, no
`src/reference_resolver_generated.rs`, no `schemas/reference-grammar.nota` seed,
and no `schema-language-cc` build-time generator, and their absence is enforced
by `tests/legacy_reference_pipeline.rs`. The rejected alternative — a
programmable grammar-data dispatch table decoded and code-generated by a
`schema-language-cc` pass — is explicitly not the target. Per-context
hand-written reading, backed by single-source-of-truth vocabularies, is the
accepted shape. This settles that the `Stream` and `Family` metadata heads are
recognized by hand-written code (through `MetadataHead`); that recognition is
accepted, not drift.

Parenthesized builtin applications such as `(Vector T)`, `(Optional T)`,
`(ScopeOf T)`, `(Map K V)`, and `(Bytes N)` are rejected rather than routed
through a compatibility resolver, at reference, newtype, root, and
macro-template positions.

### Use-site vocabularies are single-source

Each use-site vocabulary derives from one structural authority, so a name list
is never re-matched by hand:

- reserved scalar names come from `TypeReference::SCALAR_KINDS` paired with
  `scalar_name` (`src/schema.rs`) — `String`, `Integer`, `Boolean`, `Path`,
  `Bytes`; `from_name`, `is_reserved_scalar_name`, and the source-side reference
  derivation all read it;
- the `Stream` / `Family` metadata-head vocabulary is owned by the
  `MetadataHead` enum (`src/source.rs`), read by kind through `from_head_name`
  at every recognize, reject, dispatch, and re-emit site.

Inline guard tests in `schema.rs` and `source.rs` fail if either vocabulary
drifts.

### Generic definitions use the per-kind model on the source side

The source side is on the per-kind model; there is no `GenericBuiltin` per-name
enum. `SourceGenericDefinition` (`src/source.rs`) carries a
`SourceGenericDefinitionKind` — `SingleType`, `MultiType`, or `Value` — and the
builtins (`Vector`, `Optional`, `ScopeOf`, `Map`, `Bytes`) are a static
definition table distinguished by kind and by arity-as-data, not by a name
match. `SourceReference` mirrors the same partition: `SingleTypeApplication`,
`MultiTypeApplication`, `ValueApplication`, plus an open `Application` for any
other head.

Field-name derivation is data carried on the definition and defaulted by kind
(`Vector.X` → `x_vector`, `Optional.X` → `optional_x`, `ScopeOf.X` → `x_scope`,
`Map.(Key Value)` → `value_by_key`, `Bytes.N` → `bytes`), so a user-defined
generic derives correctly without a Rust name match — witnessed by
`single_type_alias_definition_projects_vector_by_definition_data` in
`src/source.rs`, where a `List` alias of the vector projection derives
`topic_list`. `TypeReference::derived_field_name` no longer dispatches
independently; it delegates to `SourceReference`.

### The strict five-slot document layout is enforced

`SchemaSource::from_document` (`src/source.rs`) reads a fixed five-slot layout
through `SchemaDocumentLayout`: imports (a brace block), input, output,
namespace (a brace block), and relations (a square-bracket block), in that
order. A root-object count other than the five slots is rejected with
`SchemaError::ExpectedRootObjectCount`. The imports slot is always present
(possibly empty); the earlier variable four-to-six root count with brace-shape
imports inference is gone. The imports section is read via
`SourceImports::from_block` and the namespace section via
`SourceNamespace::from_block` (both `src/source.rs`).

### Enforced strict-positional rejections

Two rejected source forms are landed and witnessed as invariants:

- the same-named self-tag variant shortcut `(Name)` is gone;
  `SourceVariantSignature` (`src/source.rs`) is now exactly `Unit`, `Data`, and
  `Streaming`, so there is no self-tag path and a variant payload is written
  explicitly (`Name.Name`);
- named-brace generic application (`Stream { … }`, `Family { … }`) is rejected
  through `MetadataHead::named_brace_application_error`, and semantic re-emission
  never reintroduces it — witnessed in `tests/source_codec.rs` and
  `tests/family_declarations.rs`.

`SchemaError::DuplicateTypeParameter` (`src/engine.rs`) is now constructed in
`DeclarationHead::from_parameterized` (`src/schema.rs`) and tested in
`tests/generics.rs`, rejecting duplicate type parameters.

### Remaining collapse: the semantic `TypeReference`

The semantic `TypeReference` (`src/schema.rs`) still carries per-name variants
`Vector`, `Map`, `Optional`, `ScopeOf`, and `FixedBytes` alongside the uniform
`Application { head, arguments }`. Collapsing these to mirror the source kind
partition, so lowering dispatches on kind and never on a name, is the remaining
generic-collapse work. `schemas/root.schema` and the checked-in lowering tests
still consume the per-name semantic variants, so the collapse moves the semantic
type, its `NotaDecode`, `root.schema`, and lowering together.

Vocabulary note: the semantic side names the fixed-width value kind `FixedBytes`
while the source head and grammar name it `Bytes` with a width leaf, and the
semantic side also keeps a separate unit `Bytes` scalar. Unifying the model
should reconcile these names.

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

## Checked-in schema files

- `schemas/root.schema` is the self-describing schema of the schema root type.
  It now uses the dotted source projection for composite references and remains
  active: `tests/lowering.rs`, `tests/operator_271_closed_claims.rs`, and the
  `flake.nix` lint depend on it. It still mirrors the temporary semantic
  `TypeReference` variants until the per-name semantic variants are collapsed
  to the per-kind model.
- `schemas/core.schema` is the builtin-macro-library schema. Its namespace
  declares a type named `CoreSchema` (the macro library), which is unrelated to
  the target stringless `CoreSchema` substrate. This name collision is a
  hazard; the builtin-macro-library `CoreSchema` should be renamed so the
  substrate name is free.

## Boundaries

- `schema-language` owns authored `.schema` parsing, the typed source model,
  lowering into the semantic schema value, and schema identity and evolution.
- `schema-rust` owns Rust source emission from typed schema data.
- The old `schema` repository is the extraction source for this wave and
  remains intact until it is intentionally repurposed as the live runtime
  component.
- This repository is not a runtime daemon, storage owner, or public authority
  surface.

## Sequencing (recommendation)

- The dotted-everywhere source sweep (text projection) and the
  `CoreSchema`/`TrueSchema` data-model build are orthogonal and can proceed in
  parallel.
- The per-name-to-per-kind generic collapse is done on the source side
  (per-kind `SourceGenericDefinition` and `SourceReference`); the remaining work
  collapses the semantic `TypeReference` per-name variants to mirror the kind
  partition, touching the semantic type, `root.schema`, and lowering together.
  The legacy reference-pipeline split is closed.
- Identifier reuse on reload is parked (OPEN) and is sequenced separately.
