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

### No aliases anywhere

There are no aliases anywhere in the language, by psyche decision. An alias is a
second human-chosen name bound to a declaration at a use site — an import alias,
a namespace alias, a rename-on-import. None exist. Every declaration is known by
its own name, and a name at a use site is only ever a reference, a path segment,
a schema-required disambiguator, or an encoder-synthesized indirection linkname
(see "Indirection names and the round-trip contract"), never a human-authored
rebinding. Imports therefore never rename (see "Imports entry syntax carries no
alias"), and the retired nested lowercase type sub-namespace (see "Retired
constructs") does not come back as an aliasing form. Any surface that binds a
new human-chosen name to an existing declaration is drift, to be retired rather
than migrated.

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
and any path segment that names a declaration always names a declaration in the
whole, a local one or an imported one, and is
minted to that declaration's identifier, so a rename of the target follows into
the referencing segment; a segment that resolves to no declaration is a typed
error, never a silently retained name.
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
- A `Rename` edit records a name-delta receipt on the same chain: the receipt
  carries the `NameTable` delta and emits zero migration, and its parent and
  child core hashes are equal, so it is a core-preserving self-loop that does not
  advance the structural chain.
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

As a long-term goal rather than near-term work, the psyche's stated resolution
direction is that the real solution is to perpetually develop schema in the
daemon, using files only for bootstrap. In that architecture schema evolves
through the daemon's transactional edits, which preserve identifiers natively
via the live `NameTable`, so reload re-association stops being a steady-state
problem: the minting ambiguity is paid once, at bootstrap import of a file, and
the daemon's database thereafter owns the schema and its lineage. The
provisional minting mechanism is the bridge until the daemon exists. This
direction does not close the item — the bridge remains provisional and the
daemon is unbuilt — so the section stays OPEN.

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

### Imports entry syntax carries no alias

The imports block is a set of dotted import entries, and an import entry is a
lowercase dotted path ending in either one capitalized target or a
square-bracket vector of capitalized targets:

- `path.to.Object` imports the single target `Object`; and
- `path.to.[X Y Z]` imports the several targets `X`, `Y`, and `Z` from the same
  path.

There is no alias key. An imported declaration keeps its own name; the language
has no `Alias.dotted.path.Target` form and no rename-on-import. This is the
absolute rule, not merely the entry shape: imports never rename, because the
language has no aliases anywhere (see "No aliases anywhere"). An import entry is
therefore purely a path plus one-or-many targets, and the imported names that
enter the loaded whole are the targets' own names.

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
- dotted import path segments; and
- encoder-synthesized indirection names — the lowercase linkname the encoder
  synthesizes when it decomposes a deep structure (see below). A human never
  authors one.

A human never writes a lowercase name to introduce, alias, or rename a
declaration. There is no human-authored namespace alias, no import alias, and no
nested lowercase sub-namespace; those forms are drift, not language.

### Indirection names and the round-trip contract

An indirection name is exclusively encoder-synthesized. It is the linkname the
encoder synthesizes when it decomposes a deep structure — machine-derived
factoring, never a human-authored alias. It names a hoisted subtree, stays in
the lowercase "name" register of the capitalization semantics, and inlines at
lowering: it has no `CoreSchema` or `TrueSchema` representation. The earlier
two-author framing — that a human could write the same lowercase name in source
to hoist a subtree — is rescinded: there are no aliases anywhere in the language
(see "No aliases anywhere"), so a human never authors an indirection name, and
the only author is the encoder.

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

### Blast radius

The blast radius of the dotted-everywhere change is schema source only:

- the grammar and parser;
- the re-emitter — `to_schema_text` / `to_nota_source` must round-trip semantic
  content;
- checked-in `.schema` files; and
- regenerated Rust.

Runtime state and wire are rkyv binary keyed by field position and are untouched
by the text change. Field order remains the compatibility surface.

## Retired constructs

Four source constructs are RETIRED from the schema language by psyche decision,
and their removal from grammar, parser, substrate, and fixtures has landed. The
current code, the substrate, and the checked-in `.schema` fixtures no longer
carry them; this is settled shape.

- Relations / Equivalence. Declaring path-name equivalences does not belong in
  schema. The feature was traced to an uncited 2026-06-11 commit with no Spirit
  intent record backing it, and the psyche has settled that it leaves the
  language. The relations block and its relation declarations are gone: the
  landed six-block document layout has no relations slot (see "The six-block
  document layout is enforced"), and no relation reader survives in the source
  model.
- Streams. The `Stream.(…)` metadata head and the `streams` substrate vector are
  removed; streams earn no dedicated per-kind block. The `Streaming` source
  variant signature retired with them, so a variant is now `Unit` or `Data`.
- Families. The `Family.(…)` metadata head, the `families` substrate vector, and
  the `FamilyClosure` per-family hash domain are removed together; families earn
  no dedicated per-kind block. The `Family.(…)` construct was the narrow,
  mis-shaped first implementation of per-component storage-type declaration; its
  successor is not a block in any document but the separate `sema.schema`
  document kind (see "Schema document kinds").
- Nested type-namespaces. The lowercase colon-qualified sub-namespace — a
  namespace entry whose value is another brace of declarations, keyed by a
  lowercase segmented name — is retired as drift by psyche decision. It was an
  aliasing form (a human-authored lowercase name introducing a sub-block), and
  there are no aliases anywhere (see "No aliases anywhere"). No dotted sub-block
  survives: the `types` block holds only dotted `TypeName.Definition` entries,
  each keyed by a capitalized type name, and never a nested lowercase
  sub-namespace. Removal from the source model and its fixtures has landed.

The surviving declared object classes are types, generics, and impls, and the
landed root-slot layout is built from those alone (see "Per-kind declaration
blocks").

## Per-kind declaration blocks

Every class of object in a schema file gets its own dedicated container block,
strictly typed all the way down without stopping. A schema file is partitioned by
the kind of object declared, and a block holds exactly one class of object.

What has been called "the namespace" is really the TYPE namespace: it holds only
type declarations, written as dotted `TypeName.Definition` entries, and nothing
else. Generic definitions are a different class of object and get their own block
— a generics declaration namespace, separate from the type namespace. Defining a
new generic is defining a new kind of data type, a meta-type; and when a whole new
class of meta-objects arises, it earns its own dedicated block in the schema file
rather than a marker, head, or tag squeezed into an existing block.

This principle is settled and landed. The construct set it partitions is settled
with it by psyche decision: relations, streams, and families are retired from the
language (see "Retired constructs"), so the surviving declared object classes are
types, generics, and impls, and only those earn dedicated blocks:

- The former namespace is now the `types` block, holding only dotted
  `TypeName.Definition` entries and nothing else.
- Generic definitions live in their own dedicated `generics` block.
- The trailing `{| impl |}` object that rode at the document tail is gone,
  superseded by entries in the dedicated `impls` block.

The principle makes the source text match the model that lives underneath, minus
the retired classes. The stored substrate now partitions more coarsely than the
source text: `CoreSchema` (`src/core.rs`) carries a `namespace` vector and an
`impl_blocks` vector, and the retired `streams` and `families` vectors are gone.
Generic definitions are parameterized `namespace` declarations rather than a
separate substrate vector; the source `generics` block projects exactly those
parameterized declarations (`SchemaTree::generics_schema_text`, `src/schema.rs`),
while plain types and generics share the one `namespace` vector. The source text
therefore separates the kept object classes into their own blocks even where the
substrate folds types and generics into one namespace vector and holds impls
apart.

### Landed root-slot layout of the per-kind blocks

The construct set and the root-slot ordering are settled by psyche decision and
landed in the parser. The schema document is six per-kind blocks, in order:
imports, input, output, types, generics, impls. Every slot is always present —
optionality is an empty typed slot, never a changed root count. The `types`
block holds only dotted
`TypeName.Definition` entries, each keyed by a capitalized type name and never a
nested lowercase sub-namespace (see "Retired constructs"); generics and impls
each live in their own dedicated block. The `generics` block holds dotted
`GenericName.((Params …) Body)` entries — each a capitalized generic name
carrying its binder group and body — and the `impls` block holds dotted
`TypeName.[ … ]` entries — each a capitalized type name carrying a
square-bracket catalog of impl entries. There is no relations slot, no streams
block, and no families block, because those constructs are retired.

## Schema document kinds

The six per-kind blocks describe one document kind — the general schema
document. It is not the language's only document kind. The SEMA declarations
that define a component's storage types live in a separate document kind: the
per-component storage-declaration document, by convention the `sema.schema`
file (existing usage: `orchestrate/schema/sema.schema`,
`spirit/schema/sema.schema`, each generated into `src/schema/sema.rs`). Its
root shape is not the six-block layout; the document holds a set of
storage-type declarations, one per stored record type, and nothing else.

These are distinct KINDS, not one document with an optional block. The
distinction is a direct reading of the foundational tenet that the expected
type is known ahead at every NOTA boundary starting with file kind: the file
kind fixes the expected root type, so a `sema.schema` file expects the
storage-declaration root and a general schema file expects the six-block root,
and neither borrows a slot from the other. A storage declaration is therefore
never a block folded into the general schema document; it is the entire content
of its own document kind.

Each storage-type declaration names a stored record type together with the
parts the storage engine consumes from it: its record type, its key or identity
style, and its indices and projections. All storage descriptors are generated
from these declarations — no daemon hand-constructs a descriptor. The fuller
vision of what a declaration carries lives with the storage engine that
consumes it (see the sema-engine architecture). The exact entry shape — how an
index or projection declaration reads as surface syntax — is not yet designed;
it is reserved for a psyche design session. This document names the fields the
engine consumes without fixing their surface form.

## Current implementation and remaining work

The source-facing layer has converged on the strict positional design. Reference
reading, the generic-definition model, the six-block document layout, the
use-site vocabularies, the semantic `TypeReference`, and the evolution model
(rename, hashing, and lineage) are all landed and witnessed. The remaining
divergence is not in these surfaces but in the OPEN deterministic identifier and
`NameTable` bootstrap (see above) and the undesigned `sema.schema` entry shape
(see "Schema document kinds"). Each item below is current fact plus, where work
remains, the required change.

### Reference parsing: the dotted source reader is the accepted mechanism

The single source-facing reference entry is the hand-written dotted reader in
`src/source.rs`: `SchemaSource::from_schema_text` / `from_document`,
`SourceReference`, and the per-context readers (`SourceImports::from_block`,
`SourceTypes::from_block`, `SourceGenerics::from_block`, `SourceImpls::from_block`,
and the root product reader `SourceRootEnum::from_blocks`).
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
accepted shape. The `Stream` and `Family` metadata heads and the `MetadataHead`
recognizer that once read them are gone with the retired streams and families
constructs (see "Retired constructs"); the per-context readers now recognize only
the surviving type, generic, and impl entry forms.

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

The use-site scalar vocabulary derives from one structural authority, so the name
list is never re-matched by hand: reserved scalar names come from
`TypeReference::SCALAR_KINDS` paired with `scalar_name` (`src/schema.rs`) —
`String`, `Integer`, `Boolean`, `Path`, `Bytes`; `from_name`,
`is_reserved_scalar_name`, and the source-side reference derivation all read it.
The retired `Stream` / `Family` metadata-head vocabulary and its `MetadataHead`
authority are gone (see "Retired constructs"). An inline guard test in
`schema.rs` fails if the scalar vocabulary drifts.

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

### The six-block document layout is enforced

`SchemaSource::from_document` (`src/source.rs`) reads the fixed six-slot layout
through `SchemaDocumentLayout`: imports (a brace block), input, output, types (a
brace block), generics (a brace block), and impls (a brace block), in that order.
A root-object count other than the six slots is rejected with
`SchemaError::ExpectedRootObjectCount`, whose message names the "6 root slots
(imports input output types generics impls)". Every slot is always present,
possibly empty; a wrong delimiter on a declaration block is rejected in the same
pass. The imports section is read via `SourceImports::from_block`, the types
section via `SourceTypes::from_block`, the generics section via
`SourceGenerics::from_block`, and the impls section via `SourceImpls::from_block`
(all `src/source.rs`).

This six-block layout is landed, not pending. There is no relations slot: the
retired relations square-bracket block, the retired nested lowercase
sub-namespace inside the former single namespace slot, and the trailing `{| impl
|}` tail are all removed, not migrated (see "Retired constructs"). The former
single namespace slot is now the dotted `types` block; generic definitions and
impl catalogs each read from their own dedicated block. Streams and families left
the language rather than becoming blocks. The root-slot layout is settled and
implemented (see "Per-kind declaration blocks").

### Enforced strict-positional rejections

The same-named self-tag variant shortcut `(Name)` is gone and witnessed as an
invariant: `SourceVariantSignature` (`src/source.rs`) is now exactly `Unit` and
`Data` — the `Streaming` variant retired with streams — so there is no self-tag
path and a variant payload is written explicitly (`Name.Name`), witnessed by the
`self-tagged-variant` fixture in `tests/lowering.rs` and the rejection in
`tests/design_examples.rs`. Named-brace generic application was the rejection of
the retired `Stream { … }` / `Family { … }` heads; that form and its
`MetadataHead` rejector left the language with streams and families.

`SchemaError::DuplicateTypeParameter` (`src/engine.rs`) is constructed in
`SourceGenerics::read_parameters` (`src/source.rs`), which reads the dedicated
`generics` block's binder group, and is tested in `tests/generics.rs`, rejecting
duplicate type parameters.

### The semantic `TypeReference` is on the per-kind model

The semantic `TypeReference` (`src/schema.rs`) mirrors the source kind partition
rather than one variant per builtin name. The per-name `Vector`, `Map`,
`Optional`, `ScopeOf`, and `FixedBytes` variants are gone; in their place are
`SingleTypeApplication { projection, argument }`, `MultiTypeApplication {
projection, arguments }`, and `ValueApplication { projection, value }`, each
carrying a closed projection — `SingleTypeReferenceProjection`
(`Vector`/`Optional`/`ScopeOf`), `MultiTypeReferenceProjection` (`Map`), and
`ValueReferenceProjection` (`Bytes`) — that names the within-kind lowering
strategy. Lowering dispatches on kind and projection and never on a head string;
the projection enums are the single authority shared by the source model
(`SourceReference` applications, the generic-definition table), the substrate
mirror (`CoreReference` in `src/core.rs`), and the semantic type, so there is one
Vector/Optional/ScopeOf/Map/Bytes vocabulary rather than three parallel ones. A
user-defined single-type alias such as `List` still lowers through the `Vector`
projection by definition data; the head name is a `NameTable`/source concern, not
a dispatch key. `schemas/root.schema` describes this partition (a `SingleType` /
`MultiType` / `Value` application variant carrying a projection enum), and the
machine `NotaEncode`/`NotaDecode` spells each projection by its canonical name so
`(Vector T)`, `(Map K V)`, and `(Bytes N)` round-trip value-exact — the only
canonical-spelling change from the old model is `(FixedBytes N)` becoming
`(Bytes N)`.

The `CoreReference` substrate mirror was collapsed in the same move, so the
stringless substrate and the semantic type stay aligned: changing
`CoreReference`'s shape changes `CoreSchema` canonical bytes and therefore core
hashes, but the lineage witnesses pin hash relationships (equality, inequality,
rename- and order-stability), not absolute values, so they survive the reshape.

Vocabulary resolution — `FixedBytes` versus `Bytes`: the model now spells the
fixed-width value kind `Bytes`, matching the source head and grammar (the
psyche-designed surface), and the `FixedBytes` name is retired everywhere. The
dynamic-length bytes scalar remains the separate `TypeReference::Bytes` leaf. The
two are distinguished by kind — a value application (`Bytes.N`, a width leaf)
versus a scalar leaf (bare `Bytes`) — exactly as the grammar already
distinguishes them by the presence of the width leaf. Fixed-width bytes is
therefore the `Bytes` value generic applied to a width, not a special-cased
variant name; the special case dissolves into the normal value-application case.

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
moves on rename. The retired per-family-closure hash domain is gone with families
(see "Retired constructs"), leaving these two domains. Lineage is a graph
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
  `flake.nix` lint depend on it. It describes the collapsed per-kind
  `TypeReference` partition (a `SingleType` / `MultiType` / `Value` application
  variant carrying a projection enum), not the retired per-name variants.
- `schemas/core.schema` is the builtin-macro-library schema. Its `types` block
  declares a type named `BuiltinMacroLibrary`, the macro library, which is
  unrelated to the stringless `CoreSchema` substrate; its `generics` and `impls`
  blocks are present and empty. The earlier name
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
- The per-name-to-per-kind generic collapse is done on both the source side
  (per-kind `SourceGenericDefinition` and `SourceReference`) and the semantic
  side (per-kind `TypeReference` and its `CoreReference` substrate mirror, with
  the projection vocabulary shared across all three). The legacy
  reference-pipeline split is closed.
- Deterministic identifier and `NameTable` creation is OPEN and sequenced
  separately; it subsumes the narrower reload re-association problem. It is not an
  outright blocker: a provisional mechanism may be built now if it is marked
  possibly unreliable.
