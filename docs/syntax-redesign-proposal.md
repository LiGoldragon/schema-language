# Schema syntax redesign proposal

Status: proposal for syntax review. The examples are target syntax and do not claim that the current parser accepts them.

## Scope

This proposal covers deterministic schema syntax evolution step one: replacing the authored `.schema` surface with a dotted, table-dispatched structural syntax.

This is not a compatibility proposal.

- No dual-acceptance steady state.
- No permanent old-syntax parser path.
- A one-shot migrator may exist only long enough to rewrite existing authored schemas, then be discarded.
- Forced special cases are design trouble. If a form cannot be explained as the normal structural case, redesign it or ask for review.

Raw data remains NOTA. Schema is a type-declaration language with sweet syntax over NOTA structural machinery; it is not the raw data floor.

## Current grounding

`schema-language` currently owns authored `.schema` parsing, source artifacts, lowering into `TrueSchema`, and build-time generated resolver code. `schema-language-cc` already proves the direction: grammar precedence lives in NOTA data and generates Rust, rather than being a hand-written match ladder.

The redesign should generalize that direction. The current parenthesized reference grammar is too narrow: dotted constructors, declaration forms, imports, product roles, metadata, and relation contexts must all dispatch through one structural mechanism.

## Design goal

All dotted and delimiter-bearing schema forms are recognized through a programmable dispatch table keyed by:

1. schema parse context,
2. prefix pattern,
3. delimiter or tail kind,
4. expansion action.

The same raw spelling may mean different things, or be rejected, in different contexts. Context is part of the type boundary.

Required contexts:

- namespace top,
- input top,
- output top,
- import block,
- type-reference context,
- product-field context,
- metadata context,
- relation context.

Schema code must not hand-split strings to infer schema meaning. The NOTA structural layer should expose normalized dotted forms, including both single-token dotted atoms such as `Vector.X` and trailing-dot plus delimiter forms such as `Map.(Key Value)`.

## Structural dispatch model

A normalized dotted form has:

- prefix segments,
- the delimiter or tail kind after the dot,
- tail objects,
- source span for diagnostics,
- current schema context.

Examples:

```schema
Vector.X
Map.(Key Value)
DomainMatch.[Any Partial.DomainScopes Full.DomainScopes]
RecordSelection.{ DomainMatch SelectedKind }
signal-domain.domain.[ Domain DomainScope ]
by_topic.Map.(Topic RecordIdentifier)
```

The recognizer does not decide schema semantics. It only exposes the structural shape. Dispatch data decides whether the shape is a constructor, declaration, import group, explicit field role, metadata declaration, relation clause, or error.

## Grammar data discipline

Nontrivial grammar and dispatch tables must live as typed NOTA data files, decoded by `schema-language-cc`, validated, and used to generate schema-language code. They must not be hard-coded Rust tables.

The expected NOTA type is known at the grammar-data boundary. Therefore grammar data values must not self-tag the top-level with their own type name. They must be positional values of the expected type.

Illustrative schema for grammar data:

```schema
StructuralDispatchGrammar.{
  PrefixPatternCatalog
  TailKindCatalog
  StructuralDispatchByContext
}

StructuralDispatchByContext.{
  namespaceTop.Vector.ContextRule
  inputTop.Vector.ContextRule
  outputTop.Vector.ContextRule
  importBlock.Vector.ContextRule
  typeReference.Vector.ContextRule
  productField.Vector.ContextRule
  metadata.Vector.ContextRule
  relation.Vector.ContextRule
}

ContextRule.[
  PrefixTail.PrefixTailRule
  ReferenceOnly.ExpansionAction
]

PrefixTailRule.{ PrefixPattern TailKind ExpansionAction }

PrefixPattern.[
  CapitalizedPrefix
  LowerDottedPath
  LowerRole
  ExactPrefix.Identifier
]

TailKind.[
  DotTypeReference
  DotAtom
  DotParenthesis
  DotBracket
  DotBrace
]

ExpansionAction.[
  PlainTypeReference
  UnaryConstructor.Identifier
  BinaryConstructor.Identifier
  FixedBytesConstructor.Identifier
  NamedReferenceDeclaration
  EnumDeclaration
  ProductDeclaration
  ImportGroup
  UnitRootVariant
  RootVariantPayload
  RootInlineProductPayload
  DerivedProductField
  ExplicitProductRole
]
```

Illustrative value, expected type `StructuralDispatchGrammar`:

```nota
(
  [
    CapitalizedPrefix
    LowerDottedPath
    LowerRole
    (ExactPrefix Vector)
    (ExactPrefix Optional)
    (ExactPrefix ScopeOf)
    (ExactPrefix Map)
    (ExactPrefix Bytes)
  ]
  [
    DotTypeReference
    DotAtom
    DotParenthesis
    DotBracket
    DotBrace
  ]
  (
    [
      (PrefixTail CapitalizedPrefix DotTypeReference NamedReferenceDeclaration)
      (PrefixTail CapitalizedPrefix DotBracket EnumDeclaration)
      (PrefixTail CapitalizedPrefix DotBrace ProductDeclaration)
    ]
    [
      (ReferenceOnly UnitRootVariant)
      (PrefixTail CapitalizedPrefix DotTypeReference RootVariantPayload)
      (PrefixTail CapitalizedPrefix DotBrace RootInlineProductPayload)
    ]
    [
      (ReferenceOnly UnitRootVariant)
      (PrefixTail CapitalizedPrefix DotTypeReference RootVariantPayload)
      (PrefixTail CapitalizedPrefix DotBrace RootInlineProductPayload)
    ]
    [
      (PrefixTail LowerDottedPath DotBracket ImportGroup)
    ]
    [
      (ReferenceOnly PlainTypeReference)
      (PrefixTail (ExactPrefix Vector) DotTypeReference (UnaryConstructor Vector))
      (PrefixTail (ExactPrefix Optional) DotTypeReference (UnaryConstructor Optional))
      (PrefixTail (ExactPrefix ScopeOf) DotTypeReference (UnaryConstructor ScopeOf))
      (PrefixTail (ExactPrefix Map) DotParenthesis (BinaryConstructor Map))
      (PrefixTail (ExactPrefix Bytes) DotAtom (FixedBytesConstructor Bytes))
    ]
    [
      (ReferenceOnly DerivedProductField)
      (PrefixTail LowerRole DotTypeReference ExplicitProductRole)
    ]
    []
    []
  )
)
```

The context table is a positional record with one slot per closed context, not a vector of context-tagged rows. The repeated vectors are rule lists inside each context, where order and duplicates are meaningful and validation can reject conflicts.

## Dotted constructors

Type-reference constructors use dotted syntax:

```schema
Vector.X
Optional.X
ScopeOf.Domain
Map.(Key Value)
Bytes.32
```

Unary constructors are right-associative:

```schema
Vector.Optional.X      ;; Vector.(Optional.X)
Optional.Vector.X      ;; Optional.(Vector.X)
Vector.Map.(Key Value) ;; Vector.(Map.(Key Value))
```

Invalid chains are rejected deterministically:

```schema
Map.Key.Value
Bytes.String
Vector.(Key Value)
```

`Map` requires a parenthesized binary tail. `Bytes` requires an atom tail that parses as a width. No constructor head falls through to generic application merely because a string split happened to succeed.

## Declarations

Candidate declaration forms:

```schema
Identifier.String
DomainMatch.[Any Partial.DomainScopes Full.DomainScopes]
RecordSelection.{ DomainMatch SelectedKind }
```

`Identifier.String` declares a named reference/newtype.

`DomainMatch.[Any Partial.DomainScopes Full.DomainScopes]` declares an enum. `Any` is unit. `Partial` and `Full` carry `DomainScopes`. The spelling is structural payload syntax, not a self-tag shortcut.

`RecordSelection.{ DomainMatch SelectedKind }` is the preferred product declaration candidate, pending review of aesthetics.

Open review point: whether product declarations should use the dotted brace form exactly as above, or a nearby form. Any accepted form must be table-dispatched structural syntax, not a parser special case.

## Casing ontology

Casing carries syntax meaning.

- `PascalCase` means a thing: type name, object-bearing candidate, enum variant, constructor head, named declaration, or named payload.
- uncapitalized, `camelCase`, and `snake_case` mean paths, projections, product roles, coordinates, or transparent helpers depending on context.
- Namespace, input, and output top contexts accept capitalized prefix plus dot as declaration or variant syntax.
- Import blocks accept lower dotted paths plus a bracketed list of capitalized imported things.
- Product fields accept lowercase dotted prefixes as explicit field roles, not type declarations.

Transparent helper modeling and object-bearing modeling are related but separate. This proposal commits to the casing/context distinction, not to the final semantic model for every helper type.

## Imports

Imports use grouped, non-aliasing syntax:

```schema
{
  signal-domain.domain.[
    Domain DomainScope DomainScopes ScopeSet
    Health Food Home Finance Work Craft Knowledge Education Language Art
    Kinship Selfhood Spirituality Governance Law Community Nature Travel
    Commerce Leisure Appearance Safety Information Technology
    HardwareLeaf Software ProgrammingLeaf SystemsLeaf DistributedLeaf DataLeaf
    IntelligenceLeaf SecurityLeaf QualityLeaf OperationsLeaf ObservabilityLeaf
    SurfacesLeaf EngineeringLeaf
  ]
}
```

Rules:

- Prefix is the source path, written as lower dotted path segments.
- Brackets contain imported capitalized things.
- No aliases.
- No `as`.
- No reversed alias form.
- Name collisions are schema errors.
- Exported names must be designed to avoid collisions instead of locally renamed.

This replaces key/value import pairs such as:

```schema
Domain signal-domain:domain:Domain
```

The resolver may still lower grouped imports to internal source-path plus local-name data. The authored syntax does not expose aliasing.

## Product fields

Product bodies remain positional. Each field is either a raw type reference or an explicit lowercase role plus a type reference.

Examples:

```schema
RecordSelection.{ DomainMatch SelectedKind }

StorageIndex.{
  Vector.RecordIdentifier
  Optional.Cursor
  by_topic.Map.(Topic RecordIdentifier)
}
```

Raw type references derive field names:

| Reference | Derived field name |
| --- | --- |
| `Domain` | `domain` |
| `RecordIdentifier` | `record_identifier` |
| `Vector.X` | `vector_of_x` |
| `Optional.X` | `optional_x` |
| `ScopeOf.Domain` | `scope_of_domain` |
| `Map.(Key Value)` | `map_from_key_to_value` |
| `Bytes.32` | `bytes_32` |
| `Vector.Map.(Key Value)` | `vector_of_map_from_key_to_value` |
| `Map.(Key Vector.Value)` | `map_from_key_to_vector_of_value` |

Collision rules:

- Two fields deriving the same field name are an error.
- Repeating the same semantic reference requires explicit lowercase roles.
- Explicit role affects the field name only.
- Explicit role does not change the referenced type.
- Explicit role that equals the derived name is redundant and remains an error.
- Explicit role colliding with any other field name is an error.

Open review point: generic application may appear as a raw product field only if its field-name derivation is registered and deterministic. Otherwise raw product fields should be limited to plain type references and registered constructors.

## Enum cleanup

Same-named direct enum payload shortcuts are invalid.

Invalid:

```schema
DomainMatch.[Any Partial.Full Full]
```

Invalid old/self-tag idea:

```schema
(Full)
```

Valid structural payloads:

```schema
DomainMatch.[Any Partial.DomainScopes Full.DomainScopes]
```

A variant payload must name its payload type explicitly. Repeating the variant name is not a payload declaration.

## Help and schema projection

Schema projection should stay semantically precise. Prose help is separate structured data linked to schema nodes.

Do not make schema syntax carry explanatory prose by comments, aliases, or vague labels. If help needs prose, model help as data associated with the schema node.

## Metadata, streams, families, and relations

Metadata and relation syntax must use the same context-dispatch machinery. They must not remain parser probes such as “if the head string is `Stream`”.

Open review point: exact stream, family, and relation syntax. Candidate forms should be reviewed before implementation, but the implementation constraint is fixed: they are ordinary context rules backed by grammar data.

## Implementation plan

Implementation work should happen on feature branches or worktrees named after the epic, for example `schema-dotted-syntax`. Branches should be based on main and merged to main as soon as the branch family is green.

Suggested sequence:

1. Extend NOTA structural machinery to expose normalized dotted forms, including dotted atoms and trailing-dot delimiter pairs.
2. Replace the narrow parenthesis reference grammar with broader structural dispatch grammar data.
3. Add `schema-language-cc` types for prefix patterns, tail kinds, contexts, expansion actions, validation, and code generation.
4. Generate schema-language dispatch from grammar data and freshness-check generated output.
5. Replace schema-language hand parsing in imports, type references, product fields, method parameters, stream/family fields, and relations.
6. Replace parenthesized constructor rendering with dotted canonical rendering.
7. Apply the new product field-name templates.
8. Add positive and negative tests for every context.
9. Migrate schema-language fixtures.
10. Migrate `signal-domain` and regenerate generated artifacts.
11. Migrate `signal-spirit` and regenerate generated artifacts.
12. Run relevant flake checks for all touched repositories.
13. Delete any one-shot migrator before merge, or keep it only on a discarded migration branch.

## Required witnesses

Tests should cover:

- each dotted constructor;
- right-associative unary constructor chains;
- rejection of `Map.Key.Value`;
- grouped imports;
- import collision errors;
- product field derivation templates;
- explicit field roles and collisions;
- repeated semantic references requiring roles;
- context-dependent interpretation of the same dotted shape;
- casing errors in each context;
- generated dispatch freshness from grammar data;
- canonical renderer round trips into new syntax;
- old syntax rejected after migration;
- migrated `signal-domain` schema;
- migrated `signal-spirit` schema.

## Risks and open choices

Open choices:

- exact product declaration form, especially `RecordSelection.{ DomainMatch SelectedKind }`;
- stream, family, and relation syntax;
- final field-name templates for nested forms;
- whether generic application can be used as raw product fields;
- whether generic application is registered only or open;
- first-class `ScopeOf` semantic model;
- transparent helper versus object-bearing declaration modeling.

Risks:

- Table-driven dispatch can become too abstract to inspect. Keep grammar data readable and pair each rule family with examples.
- Dotted recognition crosses token boundaries for forms such as `Map.(Key Value)`. Put that in NOTA structural machinery, not schema-specific string splitting.
- Casing is load-bearing. Diagnostics must say which casing was expected in which context.
- The one-shot migrator can become a compatibility shim. Do not land it as a steady-state tool.
