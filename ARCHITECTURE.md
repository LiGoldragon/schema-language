# Architecture

`schema-language` is the extracted build-time authored `.schema` parser and lowering bridge. It contains the current schema-language library source from the old `schema` repository so producers can move off that repository before `schema` is repurposed as the future live runtime component.

## Direction

This is replacement-oriented staging, not compatibility as design. The crate is named `schema_language`, the package is `schema-language`, and this repo does not provide a permanent `schema` re-export or shim.

The current implementation still carries the temporary string-bearing authored-schema model needed by existing producers. That is acceptable only as execution staging. The accepted end design remains:

- `TrueSchema = CoreTrueSchema + TextProjection`.
- `SchemaEvolution X_to_Y` is separate from the schema value.
- Rename is always an evolution no-op.
- Runtime components should not link the build-time `.schema` parser/lowering bridge.

## Boundaries

- `schema-language` owns authored `.schema` parsing, typed source artifacts, current lowering into `TrueSchema`, schema identity helpers, and the build-time resolver generated from the checked-in reference grammar.
- `schema-rust` owns Rust source emission from typed schema data.
- The old `schema` repository is the extraction source for this wave and remains otherwise intact until it is intentionally repurposed as the live runtime component.
- This repository is not a runtime daemon, storage owner, or public authority surface.

## Build-time resolver generation

The workspace member `schema-language-cc` is a build-time generator. It decodes and validates `schemas/reference-grammar.nota`, emits the parenthesis-reference resolver source, and the root build script freshness-checks the committed generated file. It never links into runtime components.
