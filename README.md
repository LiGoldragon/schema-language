# schema-language

`schema-language` is the build-time Rust library for the current authored `.schema` language. It parses NOTA-shaped `.schema` source, owns the temporary string-bearing source bridge, lowers through `SchemaSource` into `TrueSchema`, and exposes the typed values consumed by `schema-rust`.

This repository is extraction staging for the TrueSchema evolution. It is not the live runtime `schema` component and must not become a permanent compatibility shim for the old `schema` crate name. Runtime components should depend on generated Rust and strict binary/text contract surfaces rather than linking this build-time parser/lowering library.

The active pipeline remains:

```text
.schema source -> SchemaSource -> TrueSchema -> schema-rust emission
```

`TrueSchema` is the current semantic endpoint used by producers during this migration. Contract-root dependencies use canonical package-qualified references: `{ signal-lojix.[Input Output] }` declares the dependency and `signal-lojix.Input` / `signal-lojix.Output` reference its roots. Those roots retain their dependency package and resolved version, emit as `signal_lojix::Input` / `signal_lojix::Output`, and never enter the local `Input`/`Output` declaration namespace. Ordinary non-root imports retain their existing source-truth behavior.

The accepted end design splits the semantic model into a stringless `CoreSchema` substrate (each declaration carries a minted nominal identifier, preserved across every edit including rename), a `NameTable` from identifier to current name, and a `TrueSchema` that is a view assembled from the two. Schema identity is pulled out of the core hash so the core hash is a structural lineage address; schema evolution runs on the core, and a rename touches only the `NameTable` and emits zero migration code. The authored `.schema` source is moving to a strictly positional dotted projection. See `ARCHITECTURE.md` for the full target design, current-code state, and open items.

Rust code emission is not here. It lives in `schema-rust`.
