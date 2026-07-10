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
  segment, or an indirection name. It introduces no new lowered object.

Capitalization is not a runtime decoder input. The fixed slot type decides
string-versus-variant; a bare atom value may be capitalized, and a capitalized
string needs no delimiter. Structural lowering never reads case to choose a
value category.

## Core and True schema

The semantic schema is one model viewed two ways. The split is landed: the
stored model is the stringless `CoreSchema` substrate (`src/core.rs`) plus the
`NameTable` (`src/identifier.rs`), and `TrueSchema` (`src/view.rs`) is the
projected view over that pair. The name-bearing tree survives only as the
crate-internal codec and hash sidecar (`SchemaTree` in `src/schema.rs`): NOTA
text, canonical schema text, and rkyv binary bytes project through it, so every
codec surface stays value-exact with the pre-split format. Derived field names
are stored nowhere — a field's name is either its explicit disambiguator row in
the `NameTable` or the composed on-demand derivation from its reference — so a
rename through the table moves the projection and every derived name without
touching a substrate byte. Member declarations — struct fields, enum variants,
and generic binders — are anchored to their owner's identifier rather than the
owner's current name: a member's identifier is minted from its owner identifier
and its own local name, so renaming the owner leaves every member identifier and
therefore every substrate byte fixed, and the name table row for a member never
carries a stale owner-name prefix.

A loaded schema is one WHOLE. Import resolution happens at load, and after it
there is one substrate, one identifier space, and one `NameTable`, with no
differentiation between "local" and "imported" anywhere inside: a declaration
that arrived through an import is a declaration like any other — a minted
identifier, a `NameTable` row, and names held in the table rather than the
structure. A resolved import's frame body, its binder identifiers and its
variant list, therefore decomposes exactly as a natively declared frame does,
and a relation-path segment always names a declaration in the whole — a local
one or an imported one — and is minted to that declaration's identifier, so a
rename of a relation's target follows into the relation; a segment that resolves
to no declaration is a typed error, never a silently retained name.
Rename-stability of the substrate is therefore universal over the loaded whole,
not a local-only property: renaming any declaration, imported ones included,
moves only the `NameTable` and leaves every substrate byte fixed. What stays as
data inside the substrate is only what is genuinely not a declaration in the
whole — the cross-crate import SOURCE path a resolved import carries, which is
provenance the principle leaves in source form; impl catalogs; and table names —
under the tenet that a use-site name may be a reference/path/name value.

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
- The core hash is a lineage address: equal core hash means compatible, shared
  ancestry.
- Lineage is stored as a graph of receipt edges. Each accepted structural edit
  records a parent-core-hash-to-child-core-hash pair carrying its migration
  receipt. The historical-to-current conversion chain between two versions is the
  composition of the receipts along the path between them, and common-ancestor
  search is a walk over the stored receipt edges.
- A `Rename` edit records a `NameTable` delta on the same chain but contributes
  no receipt, because it is a zero-migration edit.
- A separate true/name hash exists per version for the human view. It lives
  outside the receipt chain and moves on rename; the core hash does not.

### Evolution runs on the core

- Migration and evolution run on `CoreSchema`.
- A `Rename` edit touches only the `NameTable` and emits zero migration code.
- Structural edits (`AddField`, `ChangeFieldType`, `AddVariant`) change core
  bytes and emit historical-to-current `From` implementations.

### OPEN: deterministic identifier and NameTable creation

Lowering into `CoreSchema` needs stable identifiers, and stable identifiers come
from the `NameTable`. Selecting which persisted `NameTable` applies to a source
being loaded is itself a lineage question — but lineage is answered by the core
hash, and the core hash cannot be computed until identifiers are assigned. That
is a bootstrap circularity: identity depends on the table, and choosing the table
depends on identity.

The assignment scheme compounds it. Any assignment that walks the source makes
identifiers a function of walk order, so the same schema with its items written
in a different order yields different core identifiers and a different core hash.
A name-derived scheme instead makes identifiers a function of rename history
rather than source order.

The desired property is deterministic `NameTable` and identifier creation
regardless of item order in the source: loading the same source always creates
the same stable identifiers and the same `NameTable`. This subsumes the narrower
reload re-association problem — unchanged declarations and renames keep their
identifiers, and only genuinely new declarations mint fresh ones.

This is not an outright implementation blocker. A provisional mechanism may be
implemented now, even if flawed, as long as it is explicitly marked possibly
unreliable; the real answer may emerge only after the system is implemented and
used. The section stays OPEN.

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
- Field-name derivation composes by reference shape. A plain type field derives
  its name as the snake_case of the type name. A generic application derives
  per-kind, with the pattern carried as data on the generic definition (`Vector.X`
  gives `x_vector`, `Optional.X` gives `optional_x`, `ScopeOf.X` gives `x_scope`);
  the pattern is defaulted by kind, never a `match "Vector"` in Rust, so a defined
  generic such as a `List` or `Maybe` derives correctly (`x_list`, `maybe_x`). An
  explicit lowercase disambiguator is stored only when the field type is
  duplicated within the struct.
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

### Dot-splitting is decided by expectation, never by scanning

Whether a leading atom carries a dotted prefix is decided purely by expectation
mode, never by scanning content. The parser is fully typed and positional and
always knows whether the next position can carry a dotted prefix; only in that
mode does it look for a top-level dot in the leading atom. In every other mode —
expected `String` above all — a period is an ordinary character, since strings
can contain periods and the dot is never a primary parsing character.

There are exactly two dotted-prefix expectation kinds:

- CAPITALIZED — the head is a capitalized object, as in a type application
  (`Vector.X`, `Map.(Key Value)`); and
- UNCAPITALIZED — the head is one or more leading lowercase name segments, as in
  map keys, import path segments, and field disambiguators.

The mechanism is shared with NOTA: it is implemented once in the NOTA reader and
exported, and `schema-language` reuses it rather than hand-rolling dot-splitting
in `src/source.rs`.

There are no space-separated pair forms anywhere in the language. Map entries are
the dotted `key.value` form, and no other space-separated key-value form remains.

### The three legitimate lowercase-name uses

A leading lowercase name (`name.Value`) is legitimate in exactly three places:

- struct-field disambiguation, required only when the field type is duplicated
  within the struct;
- dotted import paths; and
- namespace indirection names — the lowercase alias a human writes to name a
  hoisted subtree (see below).

### Indirection names and the round-trip contract

An indirection name is one construct with two authors. The lowercase alias a
human writes in the namespace section to avoid writing a deeply nested datatype,
and the linkname the encoder synthesizes when it decomposes a deep structure,
are the same mechanism. Both name a hoisted subtree, both stay in the lowercase
"name" register of the capitalization semantics, and both inline at lowering:
they have no `CoreSchema` or `TrueSchema` representation.

The encoder synthesizes indirection names under a decoding configuration that
caps how deeply nested a type may be before it is decomposed behind a lowercase
indirection name, derived programmatically from the names in the concerned
structures. The configuration is a typed record with no boolean flags. It carries
two independent depths: the main-structure depth cap, and the linked-structure
expansion level. Hoisted structures print after the main structures, each on new
lines and introduced by its linkname. A derived linkname is the lowerCamel
projection of the type name — keeping it visibly in the lowercase name register —
with the standard duplicate-disambiguation rule applied when two hoisted types
would collide.

Schema help printing is one configuration of this same record. A help print may
truncate; truncation is a projection, distinct from encoding. Encoding always
emits the complete value.

The round-trip contract governs what survives. Schema syntax round-trips both
ways: decoding a text form and re-encoding the resulting value is value-exact.
The only permitted loss is the factoring — which subtrees are hoisted behind
lowercase indirection names, and what those names are. The text layout does not
promise identical factoring, but the value always survives exactly. The depth cap
is therefore not non-round-tripping: the value round-trips, and only the factoring
is lossy.

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

What is hand-written and accepted is the per-context dispatch — which context
expects which dotted-prefix expectation kind. The low-level dotted-prefix split
itself is not hand-rolled here: it is the shared expectation-mode mechanism
exported from the NOTA reader (see "Dot-splitting is decided by expectation"), so
`src/source.rs` chooses the expectation and the NOTA reader performs the split.

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

### Rename edit is landed

`SchemaEdit` (`src/upgrade.rs`) carries `AddField`, `ChangeFieldType`,
`AddVariant`, and `Rename`. A `Rename` addresses one declaration by its stable
`NominalIdentifier` — so it applies uniformly to a type, a member, or an
imported declaration — touches only the `NameTable`, and emits zero migration
(the `AddVariant` no-migration-spec shape is the precedent). Applying it
produces a receipt whose parent and child core hashes are equal, carrying the
`NameTableDelta` it recorded on the chain, so a rename never moves the core
hash.

### Hashing and lineage are landed

The whole-schema hash has been split into two domains in `src/identity.rs`, each
under a freshly minted blake3 context (the retired identity-bearing whole-schema
context is gone, not reused): `TrueSchema::core_hash` hashes the stringless
`CoreSchema` substrate bytes — which exclude both `SchemaIdentity` and every
name — and is the structural lineage address a rename never moves;
`TrueSchema::true_name_hash` hashes the projected sidecar tree including
`SchemaIdentity` and every name, and is the per-version human-view address that
moves on rename. The per-family-closure domain is unchanged. Lineage is a graph
of receipt edges (`SchemaEditReceipt`, keyed by the parent-core-hash-to-child-
core-hash pair) walked by `LineageGraph` (`src/lineage.rs`): the
historical-to-current conversion chain is the composition of the structural
receipts along a path, and common-ancestor search is a backward walk over the
stored edges. Receipt storage is this in-crate typed representation; the schema
daemon persists it later and this crate invents no persistence of its own.

## Checked-in schema files

- `schemas/root.schema` is the self-describing schema of the schema root type.
  It now uses the dotted source projection for composite references and remains
  active: `tests/lowering.rs`, `tests/operator_271_closed_claims.rs`, and the
  `flake.nix` lint depend on it. It still mirrors the temporary semantic
  `TypeReference` variants until the per-name semantic variants are collapsed
  to the per-kind model.
- `schemas/core.schema` is the builtin-macro-library schema. Its namespace
  declares a type named `BuiltinMacroLibrary`, the macro library, which is
  unrelated to the target stringless `CoreSchema` substrate. The earlier name
  collision — the macro library was itself once named `CoreSchema` — has been
  resolved: the macro-library type was renamed to `BuiltinMacroLibrary`, so the
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
- Deterministic identifier and `NameTable` creation is OPEN and sequenced
  separately; it subsumes the narrower reload re-association problem. It is not an
  outright blocker: a provisional mechanism may be built now if it is marked
  possibly unreliable.
