# schema-language

`schema-language` is the build-time Rust library for the current authored `.schema` language. It parses NOTA-shaped `.schema` source, owns the temporary string-bearing source bridge, lowers through `SchemaSource` into `TrueSchema`, and exposes the typed values consumed by `schema-rust`.

This repository is extraction staging for the TrueSchema evolution. It is not the live runtime `schema` component and must not become a permanent compatibility shim for the old `schema` crate name. Runtime components should depend on generated Rust and strict binary/text contract surfaces rather than linking this build-time parser/lowering library.

The active pipeline remains:

```text
.schema source -> SchemaSource -> TrueSchema -> schema-rust emission
```

`TrueSchema` is the current semantic endpoint used by producers during this migration. The accepted end design separates `CoreTrueSchema` from its text projection, keeps `SchemaEvolution X_to_Y` as a separate concern, and treats renames as evolution no-ops.

Rust code emission is not here. It lives in `schema-rust`.
