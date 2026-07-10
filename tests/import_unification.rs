//! Witnesses for the whole-schema import-unification slice: a loaded schema is
//! one WHOLE, so a declaration that arrives through an import is a declaration
//! like any other — a minted identifier with a name-table row, its frame body
//! decomposed into identifier-carrying structure.
//!
//! Three properties are proven, all over the shared reaction frame fixture
//! (`Work` / `Action`) that a consumer imports and applies at its root
//! positions:
//!
//! - (a) a dependency's declarations mint IDENTICAL identifiers whether the
//!   dependency is lowered standalone or arrives via import resolution into a
//!   consumer's loaded whole — deterministic minting gives this for free;
//! - (b) projection equivalence and reload-against-prior stay green over the
//!   import-bearing fixture; and
//! - (c) the substrate canonical bytes are insensitive to a rename of an
//!   imported declaration performed through the name table — the universal
//!   rename-stability property, now holding over imported declarations too.

use std::path::PathBuf;

use nota::{Document, NotaDecode, NotaEncode};
use schema_language::{
    DeclarationKind, ImportResolver, MacroContext, Name, SchemaEngine, SchemaIdentity, TrueSchema,
};

fn reaction_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/reaction/schema")
}

fn read_reaction_fixture(file_name: &str) -> String {
    std::fs::read_to_string(reaction_fixture_dir().join(file_name))
        .unwrap_or_else(|error| panic!("read {file_name}: {error}"))
}

fn reaction_resolver() -> ImportResolver {
    ImportResolver::new().with_dependency("reaction", reaction_fixture_dir(), "0.1.0")
}

/// The reaction frame lowered standalone — the dependency on its own.
fn lower_reaction_standalone() -> TrueSchema {
    SchemaEngine::default()
        .lower_source(
            &read_reaction_fixture("reaction.schema"),
            SchemaIdentity::new("reaction:reaction", "0.1.0"),
        )
        .expect("reaction frame lowers standalone")
}

/// The migrated spirit nexus — a consumer that imports `Work`/`Action` from the
/// reaction frame and applies them at its Input/Output root positions.
fn lower_migrated_consumer() -> TrueSchema {
    SchemaEngine::default()
        .lower_source_with_resolver(
            &read_reaction_fixture("spirit-nexus.schema"),
            SchemaIdentity::new("spirit:nexus", "0.1.0"),
            &mut MacroContext::default(),
            &reaction_resolver(),
        )
        .expect("migrated consumer lowers through the import + root-application path")
}

/// The identifier a frame variant carries: minted from the frame type's
/// identifier and the variant's local name, addressed through the name table.
fn frame_variant_identifier(schema: &TrueSchema, frame: &str, variant: &str) -> Option<u128> {
    let frame_identifier = schema.identifier_named(DeclarationKind::Type, &Name::new(frame))?;
    let variant_identifier = schema.names().member_identifier_of(
        DeclarationKind::Variant,
        &frame_identifier,
        &Name::new(variant),
    )?;
    // Fold the identifier to a comparable scalar through its stable hex address.
    Some(u128::from_str_radix(&variant_identifier.to_hex(), 16).expect("32 hex digits"))
}

/// (a) A dependency's declarations mint identical identifiers whether the
/// dependency is lowered standalone or arrives via import resolution into a
/// consumer's loaded whole. The imported `Work` frame decomposes into the
/// consumer's substrate exactly as it does standalone, so its type identifier
/// and every variant identifier match address-for-address.
#[test]
fn imported_declarations_mint_identical_identifiers_to_standalone_lowering() {
    let standalone = lower_reaction_standalone();
    let migrated = lower_migrated_consumer();

    // The frame type identifier is identical in both wholes.
    let standalone_work = standalone
        .identifier_named(DeclarationKind::Type, &Name::new("Work"))
        .expect("Work is minted standalone");
    let migrated_work = migrated
        .identifier_named(DeclarationKind::Type, &Name::new("Work"))
        .expect("Work is minted in the consumer's whole via import resolution");
    assert_eq!(
        standalone_work, migrated_work,
        "the imported frame type mints the same identifier as the standalone dependency",
    );

    // Every Work variant — decomposed from the resolved import in the consumer,
    // and from the native declaration standalone — mints the same identifier.
    for variant in [
        "SignalArrived",
        "SemaWriteCompleted",
        "SemaReadCompleted",
        "EffectCompleted",
    ] {
        let standalone_variant = frame_variant_identifier(&standalone, "Work", variant)
            .unwrap_or_else(|| panic!("standalone Work.{variant} is minted"));
        let migrated_variant = frame_variant_identifier(&migrated, "Work", variant)
            .unwrap_or_else(|| panic!("imported Work.{variant} is minted in the whole"));
        assert_eq!(
            standalone_variant, migrated_variant,
            "imported Work.{variant} mints the standalone dependency's identifier",
        );
    }

    // Likewise for the Action frame's variants.
    for variant in [
        "ReplyToSignal",
        "CommandSemaWrite",
        "CommandSemaRead",
        "CommandEffect",
        "Continue",
    ] {
        let standalone_variant = frame_variant_identifier(&standalone, "Action", variant)
            .unwrap_or_else(|| panic!("standalone Action.{variant} is minted"));
        let migrated_variant = frame_variant_identifier(&migrated, "Action", variant)
            .unwrap_or_else(|| panic!("imported Action.{variant} is minted in the whole"));
        assert_eq!(
            standalone_variant, migrated_variant,
            "imported Action.{variant} mints the standalone dependency's identifier",
        );
    }
}

/// (b) Projection equivalence over the import-bearing fixture: the migrated
/// consumer's view round-trips value-exactly through both the canonical binary
/// bytes and structured NOTA, so the projection reproduces exactly the value
/// lowering built — imported frame bodies included.
#[test]
fn import_bearing_view_codecs_round_trip_value_exactly() {
    let migrated = lower_migrated_consumer();

    let bytes = migrated
        .to_binary_bytes()
        .expect("migrated encodes to rkyv");
    let from_binary = TrueSchema::from_binary_bytes(&bytes).expect("migrated decodes from rkyv");
    assert_eq!(
        from_binary, migrated,
        "binary round trip is value-exact over the import-bearing fixture",
    );

    let nota = migrated.to_nota();
    let document = Document::parse(&nota).expect("migrated NOTA parses");
    let from_nota = TrueSchema::from_nota_block(&document.root_objects()[0])
        .expect("migrated decodes from NOTA");
    assert_eq!(
        from_nota, migrated,
        "NOTA round trip is value-exact over the import-bearing fixture",
    );
}

/// (b) Reload-against-prior over the import-bearing fixture: projecting the
/// migrated consumer to source and re-lowering it against its own name table as
/// prior reproduces the substrate byte-for-byte and the table exactly — reload
/// re-associates every identifier, imported declarations included, minting
/// nothing fresh.
#[test]
fn import_bearing_reload_against_prior_preserves_substrate_and_table() {
    let engine = SchemaEngine::default();
    let identity = SchemaIdentity::new("spirit:nexus", "0.1.0");
    let migrated = lower_migrated_consumer();

    let core_bytes_before = migrated
        .core()
        .canonical_bytes()
        .expect("substrate serializes before reload");
    let table_bytes_before = migrated
        .names()
        .canonical_bytes()
        .expect("table serializes before reload");

    let projected = migrated.to_schema_text();
    let (reloaded_core, reloaded_names) = engine
        .lower_core_source_with_resolver(
            projected.as_str(),
            identity,
            &reaction_resolver(),
            migrated.names(),
        )
        .expect("projected import-bearing source re-lowers against its own table as prior");

    assert_eq!(
        core_bytes_before,
        reloaded_core
            .canonical_bytes()
            .expect("reloaded substrate serializes"),
        "reload against prior reproduces the substrate byte-for-byte over imports",
    );
    assert_eq!(
        table_bytes_before,
        reloaded_names
            .canonical_bytes()
            .expect("reloaded table serializes"),
        "reload re-associates every identifier, imported declarations included",
    );
}

/// (c) The universal rename-stability property, now over an imported
/// declaration: renaming an imported frame variant through the name table moves
/// the projection but does not move a single substrate byte, because the
/// variant's name lives in the table and its identifier lives in the substrate —
/// exactly as for a natively declared variant.
#[test]
fn renaming_an_imported_declaration_through_the_table_leaves_the_substrate_fixed() {
    let mut migrated = lower_migrated_consumer();

    let core_bytes_before = migrated
        .core()
        .canonical_bytes()
        .expect("substrate serializes before rename");

    let work = migrated
        .identifier_named(DeclarationKind::Type, &Name::new("Work"))
        .expect("the imported Work frame is minted in the whole");
    let signal_arrived = migrated
        .names()
        .member_identifier_of(DeclarationKind::Variant, &work, &Name::new("SignalArrived"))
        .expect("the imported Work.SignalArrived variant is minted under its frame");

    migrated
        .rename(&signal_arrived, Name::new("SignalReceived"))
        .expect("renaming an imported declaration through the table succeeds");

    // The projection follows the new name: the resolved import now carries the
    // renamed variant.
    let renamed_present = migrated
        .resolved_imports()
        .iter()
        .filter(|import| import.local_name().as_str() == "Work")
        .flat_map(|import| import.variants().to_vec())
        .any(|variant| variant.name.as_str() == "SignalReceived");
    assert!(
        renamed_present,
        "the renamed imported variant projects its new name",
    );

    // And the substrate is untouched: identical canonical bytes.
    let core_bytes_after = migrated
        .core()
        .canonical_bytes()
        .expect("substrate serializes after rename");
    assert_eq!(
        core_bytes_before, core_bytes_after,
        "a rename of an imported declaration must not move a single substrate byte",
    );
}

/// Finding 2 (imported-declaration rename under the no-alias projection): an
/// imported declaration is a declaration like any other in the loaded whole — a
/// minted identifier with its name held in the table — so renaming it through the
/// table follows into every referencing segment, the applied body above all. The
/// imports brace, by contrast, carries the cross-crate import SOURCE path, which
/// is provenance the language leaves in source form (there is no alias key), so it
/// names the producer's exported declaration and a consumer-side rename does NOT
/// move it. The brace provenance and the renamed body therefore legitimately
/// diverge, the substrate bytes stay fixed (the name lives in the table, the
/// identifier in the substrate), and the projection still re-lowers cleanly
/// against the renamed prior table.
#[test]
fn renaming_an_imported_declaration_keeps_the_imports_brace_consistent_and_reloadable() {
    let engine = SchemaEngine::default();
    let identity = SchemaIdentity::new("spirit:nexus", "0.1.0");
    let mut migrated = lower_migrated_consumer();

    let core_bytes_before = migrated
        .core()
        .canonical_bytes()
        .expect("substrate serializes before the rename");

    let work = migrated
        .identifier_named(DeclarationKind::Type, &Name::new("Work"))
        .expect("the imported Work frame is minted in the whole");
    migrated
        .rename(&work, Name::new("WorkFrame"))
        .expect("renaming the imported declaration through the table succeeds");

    // The projected schema text keeps the imports brace on the producer's SOURCE
    // path (provenance in source form, unmoved by a consumer-side rename) while
    // the rename follows into the applied body. There is no alias, so the brace
    // never carries the renamed local name, and the alias-era `Name source` pair
    // form never reappears.
    let projected = migrated.to_schema_text();
    assert!(
        projected.contains("reaction.reaction.Work"),
        "the imports brace keeps the producer's source path as provenance:\n{projected}",
    );
    assert!(
        projected.contains("WorkFrame.("),
        "the rename follows into the applied body:\n{projected}",
    );
    assert!(
        !projected.contains("WorkFrame reaction"),
        "the retired alias-pair imports form must not reappear:\n{projected}",
    );

    // The substrate is untouched by the rename: the name lives in the table, its
    // identifier lives in the substrate, so not a byte moves.
    let core_bytes_after = migrated
        .core()
        .canonical_bytes()
        .expect("substrate serializes after the rename");
    assert_eq!(
        core_bytes_before, core_bytes_after,
        "renaming an imported declaration must not move a single substrate byte",
    );

    // The consistent projection re-lowers cleanly against the renamed table as
    // prior, reproducing the substrate byte-for-byte.
    let (reloaded_core, _reloaded_names) = engine
        .lower_core_source_with_resolver(
            projected.as_str(),
            identity,
            &reaction_resolver(),
            migrated.names(),
        )
        .expect("the renamed-import projection re-lowers cleanly against the renamed prior");
    assert_eq!(
        core_bytes_after,
        reloaded_core
            .canonical_bytes()
            .expect("reloaded substrate serializes"),
        "the renamed-import projection reproduces the substrate byte-for-byte",
    );
}
