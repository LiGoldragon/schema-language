{
  description = "schema-language — build-time .schema parser and lowering bridge";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-build = {
      url = "github:LiGoldragon/rust-build";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-build }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        rust = rust-build.lib.${system}.fromPkgs pkgs;
        inherit (rust) craneLib toolchain;
        schemaFilter = path: type:
          type == "regular" && (
            pkgs.lib.hasSuffix ".schema" path
            || pkgs.lib.hasSuffix ".asschema" path
            || pkgs.lib.hasSuffix ".macro-library" path
          );
        src = rust.cleanSource {
          root = ./.;
          extraFilters = [ schemaFilter ];
        };
        cargoVendorDirectory = craneLib.vendorCargoDeps { inherit src; };
        commonArguments = {
          inherit src cargoVendorDirectory;
          strictDeps = true;
        };
        cargoArtifacts = craneLib.buildDepsOnly commonArguments;
      in
      {
        packages.default = craneLib.buildPackage (commonArguments // { inherit cargoArtifacts; });
        checks = {
          build = craneLib.cargoBuild (commonArguments // { inherit cargoArtifacts; });
          test = craneLib.cargoTest (commonArguments // { inherit cargoArtifacts; });
          design-examples = pkgs.runCommand "schema-design-examples" { } ''
            grep -R "design_example_schema_document_has_six_strict_roots" ${src}/tests/design_examples.rs >/dev/null
            grep -R "design_example_namespace_brace_contains_key_value_declarations" ${src}/tests/design_examples.rs >/dev/null
            grep -R "design_example_type_reference_macro_captures_use_dollar_sigils" ${src}/tests/design_examples.rs >/dev/null
            grep -R "design_example_colon_qualified_name_decomposes_into_segments" ${src}/tests/design_examples.rs >/dev/null
            grep -R "design_example_default_engine_uses_strict_structural_macros" ${src}/tests/design_examples.rs >/dev/null
            grep -R "design_example_schema_lowering_records_source_structure_header" ${src}/tests/design_examples.rs >/dev/null
            grep -R "design_example_macro_node_definitions_separate_structural_from_tagged_invocation" ${src}/tests/design_examples.rs >/dev/null
            grep -R "design_example_macro_node_definition_lists_structural_cases" ${src}/tests/design_examples.rs >/dev/null
            grep -R "design_example_schema_node_macro_call_is_tagged_data" ${src}/tests/design_examples.rs >/dev/null
            grep -R "design_example_user_declared_macros_extend_structural_and_named_slots" ${src}/tests/design_examples.rs >/dev/null
            grep -R "design_example_root_enum_uses_direct_variant_shapes" ${src}/tests/design_examples.rs >/dev/null
            grep -R "design_example_same_name_payload_variant_is_rejected" ${src}/tests/design_examples.rs >/dev/null
            grep -R "design_example_signal_nexus_and_sema_are_schema_declared_planes" ${src}/tests/design_examples.rs >/dev/null
            touch $out
          '';
          no-nested-root-enum-examples = pkgs.runCommand "schema-no-nested-root-enum-examples" { } ''
            if find ${src} -name '*.witness.txt' -print -quit | grep .; then
              echo "line-format .witness.txt goldens must not remain in schema" >&2
              exit 1
            fi
            if grep -R -n -E '^\s*\((Input|Output) \(' ${src}/schemas ${src}/tests/fixtures; then
              echo "schema examples must not reintroduce labeled Input/Output root enums" >&2
              exit 1
            fi
            if grep -R -n -E '@(Vec|Option|KeyValue|Bag|HashSet)' ${src}/schemas ${src}/tests ${src}/src; then
              echo "schema examples must not reintroduce the old @ macro sigil" >&2
              exit 1
            fi
            if grep -R -n -E '\[\[[A-Z]|\((records|kinds|services|Listed) \[[A-Z]|\((byTopic|Projected|nodes) \{[A-Z]' ${src}/schemas ${src}/tests/fixtures; then
              echo "schema examples must use typed NOTA composite references: Vector.T, Map.(K V), Optional.T" >&2
              exit 1
            fi
            if grep -R -n -E '\((Vector|Optional|ScopeOf|Map|Bytes) [A-Za-z0-9_$]' ${src}/schemas ${src}/tests/fixtures; then
              echo "schema examples must not reintroduce parenthesized generic applications" >&2
              exit 1
            fi
            if grep -R -n -E '\((Vec|Option|KeyValue|Map) \[' ${src}/schemas ${src}/tests; then
              echo "schema examples must not put raw vectors inside composite type constructors" >&2
              exit 1
            fi
            if grep -R -n -E '[A-Za-z][A-Za-z0-9]*\*' ${src}/tests/fixtures ${src}/schemas/spirit-min.schema; then
              echo "schema examples must not reintroduce star-suffix same-name payload sugar" >&2
              exit 1
            fi
            if grep -R -n -E 'SchemaEnumDefinitionBrace|BraceEnum|ExpectedEvenBraceEnumPairs' ${src}/src ${src}/schemas ${src}/tests; then
              echo "brace enum sugar must not reappear; braces are key/value maps" >&2
              exit 1
            fi
            touch $out
          '';
          no-btree-canonical = pkgs.runCommand "schema-no-btree-canonical" { } ''
            if grep -R "BTreeMap" ${src}/src ${src}/tests ${src}/schemas; then
              echo "BTreeMap must not be canonical assembled-schema storage" >&2
              exit 1
            fi
            touch $out
          '';
          no-obsolete-asschema-syntax = pkgs.runCommand "schema-no-obsolete-asschema-syntax" { } ''
            if find ${src} -name '*.asschema' ! -path '*/schemas/core.asschema' -print -quit | grep .; then
              echo "obsolete .asschema syntax fixtures must not remain in schema" >&2
              exit 1
            fi
            grep -R "schema_source_and_semantic_schema_round_trip_without_asschema_artifacts" ${src}/tests/operator_271_closed_claims.rs >/dev/null
            grep -R "raw_core_schema_fixture_is_legal_nota_before_schema_reading" ${src}/tests/raw_core_schema.rs >/dev/null
            if grep -R -n -E '\[Input \[|\[Output \[|\(Struct \[|\(Enum \[|\(Newtype \[|\(Map \[\(Plain|\(Carries \(Plain' ${src}/src ${src}/tests ${src}/schemas; then
              echo "obsolete ASSchema vector-record syntax must not remain in active code or fixtures" >&2
              exit 1
            fi
            touch $out
          '';
          no-authored-features = pkgs.runCommand "schema-no-authored-features" { } ''
            if grep -R "EffectTable\\|FanOutTargets\\|StorageDescriptor\\|Features" ${src}; then
              echo "retracted authored schema features are forbidden" >&2
              exit 1
            fi
            touch $out
          '';
          macro-registry-used = pkgs.runCommand "schema-macro-registry-used" { } ''
            grep -R "pub struct MacroRegistry" ${src}/src/macros.rs >/dev/null
            grep -R "SchemaEngine::with_registry" ${src}/tests/lowering.rs >/dev/null
            grep -R "default_engine_lowers_through_registered_structural_forms" ${src}/tests/lowering.rs >/dev/null
            grep -R "root_enum_named(\"Input\")" ${src}/tests/lowering.rs >/dev/null
            grep -R "root_enum_named(\"Output\")" ${src}/tests/lowering.rs >/dev/null
            ! grep -R "type_declaration_macro:" ${src}/src/engine.rs
            ! grep -R "surface_macro:" ${src}/src/engine.rs
            ! grep -R "matches_pair" ${src}/src/engine.rs
            touch $out
          '';
          declarative-schema-macros = pkgs.runCommand "schema-declarative-schema-macros" { } ''
            # Per operator 271 claim 1 — schema 99078b20 collapsed
            # the macro library source/artifact split. The previous check
            # asserted presence of `DeclarativeMacroLibrary::builtin` and
            # `pub struct MacroLibraryData`; both were retired in the
            # collapse, so the assertions are inverted (must NOT contain)
            # and the present canonical nouns are asserted positively.
            grep -R "pub fn builtin_source" ${src}/src/declarative.rs >/dev/null
            grep -R "pub struct MacroLibrary {" ${src}/src/declarative.rs >/dev/null
            grep -R "pub struct MacroLibraryArtifact {" ${src}/src/declarative.rs >/dev/null
            grep -R "pub enum MacroLibrarySourceEntry {" ${src}/src/declarative.rs >/dev/null
            grep -R "builtin_macro_library_round_trips_as_typed_data_and_still_executes" ${src}/tests/macro_exploration.rs >/dev/null
            grep -R "SchemaStructDefinition" ${src}/schemas/builtin-macros.schema >/dev/null
            grep -R '\$Name' ${src}/schemas/builtin-macros.schema >/dev/null
            grep -R '\$\*Fields' ${src}/schemas/builtin-macros.schema >/dev/null
            grep -R "builtin_macro_file_defines_visible_dollar_captures" ${src}/tests/lowering.rs >/dev/null
            ! grep -R "expanded_templates" ${src}/tests/lowering.rs
            ! grep -R "struct TypeDeclarationMacro" ${src}/src
            ! grep -R "struct StructFieldsMacro" ${src}/src
            ! grep -R "struct EnumVariantsMacro" ${src}/src
            # The collapsed-mirrors regression guard — present-shape
            # negative witness covers the retired-data names.
            ! grep -R "pub struct MacroLibraryData" ${src}/src
            ! grep -R "pub struct DeclarativeMacroLibrary" ${src}/src
            ! grep -R "MacroLibrarySourceEntryData" ${src}/src
            ! grep -R "MacroDefinitionData" ${src}/src
            ! grep -R "MacroPatternData" ${src}/src
            ! grep -R "MacroTemplateData" ${src}/src
            touch $out
          '';
          operator-271-closed-claims = pkgs.runCommand "schema-operator-271-closed-claims" { } ''
            # Architectural-truth witnesses for the closed claims in
            # operator 271. The test file at
            # tests/operator_271_closed_claims.rs runs through cargo test;
            # this Nix check verifies each named witness function is present
            # so future drift is caught by the flake before reaching cargo.
            test -f ${src}/tests/operator_271_closed_claims.rs
            # Claim 1 — macro library source/artifact datatype split CLOSED.
            grep -R "macro_library_source_entries_are_one_type" ${src}/tests/operator_271_closed_claims.rs >/dev/null
            grep -R "macro_library_artifact_wraps_the_one_library_type" ${src}/tests/operator_271_closed_claims.rs >/dev/null
            grep -R "macro_library_split_does_not_return_through_public_surface" ${src}/tests/operator_271_closed_claims.rs >/dev/null
            # Claim 4 — honest enum bodies CLOSED.
            grep -R "production_schema_sources_use_honest_enum_bodies" ${src}/tests/operator_271_closed_claims.rs >/dev/null
            grep -R "spirit_min_input_enum_body_has_explicit_payload_variants" ${src}/tests/operator_271_closed_claims.rs >/dev/null
            # Claim 5 — SchemaSource plus semantic TrueSchema own the retired Asschema path.
            grep -R "schema_is_typed_data_with_named_field_accessors" ${src}/tests/operator_271_closed_claims.rs >/dev/null
            grep -R "schema_source_and_semantic_schema_round_trip_without_asschema_artifacts" ${src}/tests/operator_271_closed_claims.rs >/dev/null
            touch $out
          '';
          true-schema-public-surface = pkgs.runCommand "schema-true-schema-public-surface" { } ''
            test -f ${src}/tests/true_schema.rs
            test -f ${src}/tests/core_projection.rs
            # TrueSchema is the projected view over the split model: it lives in
            # src/view.rs holding exactly identity + CoreSchema + NameTable, with
            # borrowing view types as the on-demand read surface. The
            # name-bearing tree survives only as the crate-internal codec/hash
            # sidecar (SchemaTree) and is never re-exported.
            grep -R "pub struct TrueSchema" ${src}/src/view.rs >/dev/null
            grep -R "pub struct RootView" ${src}/src/view.rs >/dev/null
            grep -R "pub struct DeclarationView" ${src}/src/view.rs >/dev/null
            grep -R "pub struct FieldView" ${src}/src/view.rs >/dev/null
            grep -R "pub struct CoreSchema" ${src}/src/core.rs >/dev/null
            if grep -R -n "pub struct TrueSchema" ${src}/src/schema.rs; then
              echo "the stored name-bearing TrueSchema tree returned to src/schema.rs" >&2
              exit 1
            fi
            if grep -R -n -E 'pub use .*SchemaTree' ${src}/src/lib.rs; then
              echo "the codec sidecar tree must not be re-exported" >&2
              exit 1
            fi
            grep -R "rename_through_the_table_moves_projection_but_not_core_bytes" ${src}/tests/core_projection.rs >/dev/null
            grep -R "derived_field_names_project_on_demand_and_match_materialized_names" ${src}/tests/core_projection.rs >/dev/null
            grep -R "view_codecs_round_trip_value_exactly_over_the_corpus" ${src}/tests/core_projection.rs >/dev/null
            grep -R "authored_schema_decodes_directly_to_true_schema" ${src}/tests/true_schema.rs >/dev/null
            grep -R "true_schema_round_trips_through_binary_and_structured_nota" ${src}/tests/true_schema.rs >/dev/null
            grep -R "product_components_accept_implicit_unique_types" ${src}/tests/true_schema.rs >/dev/null
            grep -R "product_components_accept_duplicate_types_with_explicit_identities" ${src}/tests/true_schema.rs >/dev/null
            grep -R "product_components_reject_redundant_explicit_derived_identity" ${src}/tests/true_schema.rs >/dev/null
            grep -R "product_components_reject_explicit_identity_on_unique_type" ${src}/tests/true_schema.rs >/dev/null
            if grep -R -n -E 'pub struct (Schema|SpecifiedSchema)([^A-Za-z0-9_]|$)' ${src}/src; then
              echo "legacy Schema or SpecifiedSchema public struct returned" >&2
              exit 1
            fi
            if grep -R -n -E 'SpecifiedSchema|Specified[A-Za-z0-9_]*|mod specified|specified::' ${src}/src ${src}/tests ${src}/schemas ${src}/README.md ${src}/ARCHITECTURE.md; then
              echo "legacy specified semantic names returned" >&2
              exit 1
            fi
            if grep -R -n -E '(^|[^A-Za-z0-9_])Schema::|`Schema`' ${src}/src ${src}/tests ${src}/schemas ${src}/README.md ${src}/ARCHITECTURE.md; then
              echo "legacy public Schema type references returned" >&2
              exit 1
            fi
            if grep -R -n -E 'pub use .*([^A-Za-z0-9_]|^)Schema([^A-Za-z0-9_]|[,;])' ${src}/src; then
              echo "legacy public Schema alias returned" >&2
              exit 1
            fi
            touch $out
          '';
          namespace-braces-are-key-value = pkgs.runCommand "schema-namespace-braces-are-key-value" { } ''
            grep -R "brace_namespace_rejects_parenthesized_named_objects" ${src}/tests/lowering.rs >/dev/null
            grep -R "brace_namespace_rejects_redundant_key_value_declarations" ${src}/tests/lowering.rs >/dev/null
            ! grep -R "NamedTypeDefinition" ${src}/src ${src}/schemas ${src}/tests
            ! grep -R -n -E '^  \([A-Z][A-Za-z0-9]* [\[\(]' ${src}/schemas/root.schema ${src}/schemas/core.schema ${src}/schemas/spirit-min.schema
            touch $out
          '';
          schema-module-entrypoint = pkgs.runCommand "schema-schema-module-entrypoint" { } ''
            grep -R "pub struct SchemaPackage" ${src}/src/module.rs >/dev/null
            grep -R "lib.schema" ${src}/src/module.rs >/dev/null
            grep -R "package_loader_reads_schema_lib_entrypoint" ${src}/tests/lowering.rs >/dev/null
            grep -R "package_loader_reads_all_schema_modules_in_crate" ${src}/tests/lowering.rs >/dev/null
            grep -R "resolver_resolves_import_of_dependency_root_enum" ${src}/tests/resolution.rs >/dev/null
            test -f ${src}/tests/fixtures/spirit-crate/schema/lib.schema
            test -f ${src}/tests/fixtures/plane-crate/schema/signal.schema
            test -f ${src}/tests/fixtures/plane-crate/schema/nexus.schema
            test -f ${src}/tests/fixtures/plane-crate/schema/sema.schema
            grep -R "colon_qualified_names_lower_as_schema_names" ${src}/tests/lowering.rs >/dev/null
            touch $out
          '';
          raw-core-schema-example = pkgs.runCommand "schema-raw-core-schema-example" { } ''
            test -f ${src}/tests/fixtures/raw-core/core.schema
            test -f ${src}/tests/fixtures/raw-core/non-map-root.schema
            test -f ${src}/tests/fixtures/raw-core/odd-map.schema
            grep -R "RawSchemaFile::from_path_and_source" ${src}/tests/raw_core_schema.rs >/dev/null
            grep -R "raw_core_schema_fixture_is_legal_nota_before_schema_reading" ${src}/tests/raw_core_schema.rs >/dev/null
            grep -R "raw_core_schema_file_root_name_comes_from_filename" ${src}/tests/raw_core_schema.rs >/dev/null
            grep -R "raw_core_schema_reads_datatype_key_value_map" ${src}/tests/raw_core_schema.rs >/dev/null
            grep -R "raw_core_schema_preserves_native_key_value_and_enum_forms" ${src}/tests/raw_core_schema.rs >/dev/null
            grep -R "RawDatatypeMap" ${src}/tests/fixtures/raw-core/core.schema >/dev/null
            grep -F "{ key.Name value.RawDatatype }" ${src}/tests/fixtures/raw-core/core.schema >/dev/null
            touch $out
          '';
          no-production-free-functions = pkgs.runCommand "schema-no-production-free-functions" { } ''
            if grep -R -n -E '^(pub(\([^)]*\))? )?fn ' ${src}/src; then
              echo "production Rust must not use module-level free functions" >&2
              exit 1
            fi
            touch $out
          '';
          no-production-unit-structs = pkgs.runCommand "schema-no-production-unit-structs" { } ''
            if grep -R -n -E '^struct [A-Za-z][A-Za-z0-9_]*;' ${src}/src; then
              echo "production Rust must not use unit structs as namespace/method holders" >&2
              exit 1
            fi
            touch $out
          '';
          doc = craneLib.cargoDoc (commonArguments // {
            inherit cargoArtifacts;
            RUSTDOCFLAGS = "-D warnings";
          });
          fmt = craneLib.cargoFmt { inherit src; };
          clippy = craneLib.cargoClippy (commonArguments // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          });
        };
        devShells.default = pkgs.mkShell {
          name = "schema-language";
          packages = [ pkgs.jujutsu pkgs.pkg-config toolchain ];
        };
      });
}
