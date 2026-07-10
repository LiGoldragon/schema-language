use std::fs;

use schema_language::{
    ImplFact, ImplReference, MethodParameter, MethodSignature, Name, RustSurface, SchemaEngine,
    SchemaError, SchemaIdentity, SchemaSourceArtifact, SourceImplEntry, SourceReference,
    TrueSchema, TypeDeclaration, TypeReference,
};

fn impl_catalog_fixture(name: &str) -> String {
    fs::read_to_string(format!("tests/fixtures/impl-catalog/{name}.schema"))
        .unwrap_or_else(|error| panic!("read impl-catalog schema fixture {name}: {error}"))
        .trim_end()
        .to_owned()
}

/// Lower a fixture through the typed source archive into a `TrueSchema`, the
/// path that carries every impl catalog as a standalone `ImplBlock` keyed by
/// its target type.
fn lower_fixture(name: &str) -> TrueSchema {
    let artifact = SchemaSourceArtifact::from_schema_text(&impl_catalog_fixture(name))
        .expect("source decodes");
    SchemaEngine::default()
        .lower_schema_source(artifact.source(), SchemaIdentity::new("example", "0.1.0"))
        .unwrap_or_else(|error| panic!("lower impl-catalog fixture {name}: {error}"))
}

/// The canonical schema-source text must be byte-stable through
/// decode -> to_schema_text -> re-decode, with the `impls` block entry
/// `TypeName.[ … ]` surfaced verbatim. This is the same round-trip contract the
/// source codec tests assert, extended to the six-block impls surface.
#[test]
fn marker_impls_round_trip() {
    let source = impl_catalog_fixture("fused-markers");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");
    let canonical = artifact.to_schema_text();

    assert_eq!(
        canonical, source,
        "marker impls should write a byte-stable canonical surface"
    );
    assert!(
        canonical.contains("RecordIdentifier.[ Display Ord ]"),
        "canonical surface must carry the impls-block entry: {canonical}"
    );

    let recovered =
        SchemaSourceArtifact::from_schema_text(&canonical).expect("canonical source decodes");
    assert_eq!(
        artifact, recovered,
        "canonical schema source should recover the same source object"
    );
}

#[test]
fn method_signature_impls_round_trip() {
    let source = impl_catalog_fixture("body-optional");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");
    let canonical = artifact.to_schema_text();

    assert_eq!(
        canonical, source,
        "method-signature impls should write a byte-stable canonical surface"
    );
    assert!(
        canonical.contains("StatementText.[ Display (word_count {} Integer) ]"),
        "canonical surface must carry the impls-block entry: {canonical}"
    );

    let recovered =
        SchemaSourceArtifact::from_schema_text(&canonical).expect("canonical source decodes");
    assert_eq!(artifact, recovered);
}

#[test]
fn trait_method_signature_impls_round_trip() {
    let source = impl_catalog_fixture("trait-method-sigs");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");
    let canonical = artifact.to_schema_text();

    assert_eq!(
        canonical, source,
        "trait + method-signature impls should write a byte-stable canonical surface"
    );
    assert!(
        canonical.contains("NodeQuery.[ QueryMatcher [ (matches { candidate.Node } Boolean) ] ]"),
        "canonical surface must carry the trait impl with method signatures: {canonical}"
    );

    let recovered =
        SchemaSourceArtifact::from_schema_text(&canonical).expect("canonical source decodes");
    assert_eq!(artifact, recovered);
}

/// The typed impl-catalog nouns must survive the rkyv archive boundary — the
/// same binary round-trip the source codec asserts for every typed source noun.
/// This is what proves `SourceImpls` / `SourceImplsEntry` / `SourceImplCatalog`
/// / `SourceImplEntry` are real archive members, not parser-only state.
#[test]
fn impl_catalog_round_trips_through_binary_archive() {
    for name in ["fused-markers", "body-optional", "trait-method-sigs"] {
        let source = impl_catalog_fixture(name);
        let artifact =
            SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");
        let bytes = artifact
            .to_binary_bytes()
            .expect("schema source artifact archives");
        let recovered = SchemaSourceArtifact::from_binary_bytes(&bytes)
            .expect("schema source artifact restores");

        assert_eq!(artifact, recovered, "binary round-trip for {name}");
        assert_eq!(
            recovered.to_schema_text(),
            source,
            "binary-restored {name} should re-emit the canonical surface"
        );
    }
}

/// The decoded `impls` block must expose its entries as typed data: markers as
/// trait names, trait impls with their method signatures, and inherent method
/// signatures with typed return references. Each entry is keyed by the type it
/// targets.
#[test]
fn impl_catalog_decodes_each_entry_kind() {
    let fused = SchemaSourceArtifact::from_schema_text(&impl_catalog_fixture("fused-markers"))
        .expect("schema source decodes");
    let entries = fused.source().impls().entries();
    let [record_identifier] = entries else {
        panic!("expected one impls entry, found {}", entries.len());
    };
    assert_eq!(record_identifier.target().as_str(), "RecordIdentifier");
    let markers = record_identifier.catalog().entries();
    assert_eq!(markers.len(), 2, "two marker impls");
    assert!(matches!(&markers[0], SourceImplEntry::Marker(name) if name.as_str() == "Display"));
    assert!(matches!(&markers[1], SourceImplEntry::Marker(name) if name.as_str() == "Ord"));

    let body_optional =
        SchemaSourceArtifact::from_schema_text(&impl_catalog_fixture("body-optional"))
            .expect("schema source decodes");
    let entries = body_optional.source().impls().entries();
    let [statement_text] = entries else {
        panic!("expected one impls entry, found {}", entries.len());
    };
    assert_eq!(statement_text.target().as_str(), "StatementText");
    let catalog = statement_text.catalog().entries();
    assert_eq!(catalog.len(), 2, "one marker plus one inherent method");
    assert!(matches!(&catalog[0], SourceImplEntry::Marker(name) if name.as_str() == "Display"));
    let SourceImplEntry::InherentMethod(signature) = &catalog[1] else {
        panic!("expected an inherent method, found {:?}", catalog[1]);
    };
    assert_eq!(signature.name().as_str(), "word_count");
    assert!(signature.parameters().is_empty(), "nullary method");
    assert!(
        matches!(signature.return_reference(), SourceReference::Plain(name) if name.as_str() == "Integer"),
        "return reference resolves to Integer"
    );

    let trait_sigs =
        SchemaSourceArtifact::from_schema_text(&impl_catalog_fixture("trait-method-sigs"))
            .expect("schema source decodes");
    let entries = trait_sigs.source().impls().entries();
    let [node_query] = entries else {
        panic!("expected one impls entry, found {}", entries.len());
    };
    assert_eq!(node_query.target().as_str(), "NodeQuery");
    let catalog = node_query.catalog().entries();
    assert_eq!(catalog.len(), 1, "one trait impl");
    let SourceImplEntry::TraitImpl(trait_name, signatures) = &catalog[0] else {
        panic!("expected a trait impl, found {:?}", catalog[0]);
    };
    assert_eq!(trait_name.as_str(), "QueryMatcher");
    assert_eq!(
        signatures.len(),
        1,
        "one method signature on the trait impl"
    );
    assert_eq!(signatures[0].name().as_str(), "matches");
    assert_eq!(signatures[0].parameters().len(), 1, "one parameter");
    assert_eq!(signatures[0].parameters()[0].name().as_str(), "candidate");
    assert!(
        matches!(signatures[0].parameters()[0].reference(), SourceReference::Plain(name) if name.as_str() == "Node"),
        "parameter type resolves to Node"
    );
    assert!(
        matches!(signatures[0].return_reference(), SourceReference::Plain(name) if name.as_str() == "Boolean"),
        "return reference resolves to Boolean"
    );
}

/// The macro/engine path and the typed-source path must segment the six-block
/// declaration and impls blocks identically: type entries lower to declarations
/// and every impls entry lowers to a standalone `ImplBlock` keyed by its target.
#[test]
fn engine_walk_accepts_type_and_impls_entries() {
    let source = "{}\n[]\n[]\n{ RecordIdentifier.String StatementText.String Topic.String }\n{}\n{ RecordIdentifier.[ Display Ord ] StatementText.[ Display ] }";
    let schema = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("example", "0.1.0"))
        .expect("engine lowers type and impls entries");

    let TypeDeclaration::Newtype(record_identifier) = schema
        .type_named("RecordIdentifier")
        .expect("type entry lowers")
    else {
        panic!("RecordIdentifier should lower to a newtype from its type entry");
    };
    assert_eq!(record_identifier.reference, TypeReference::String);

    let TypeDeclaration::Newtype(topic) = schema
        .type_named("Topic")
        .expect("entry after other entries still lowers")
    else {
        panic!("Topic should lower to a newtype");
    };
    assert_eq!(topic.reference, TypeReference::String);

    // Every declared type is minted exactly once; the impls block mints no
    // second declaration for its target.
    assert_eq!(
        schema
            .namespace()
            .iter()
            .filter(|declaration| declaration.name().as_str() == "StatementText")
            .count(),
        1,
        "the impls target is declared once, by its type entry"
    );

    // Impls always live in standalone blocks, keyed by the type they target.
    let mut blocks: Vec<(String, usize)> = schema
        .impl_blocks()
        .iter()
        .map(|block| {
            (
                block.target().as_str().to_owned(),
                block.catalog().entries().len(),
            )
        })
        .collect();
    blocks.sort();
    assert_eq!(
        blocks,
        vec![
            ("RecordIdentifier".to_owned(), 2),
            ("StatementText".to_owned(), 1)
        ],
        "the engine path carries every catalog as a standalone impl block"
    );

    // No catalog rides on a declaration under the uniform-standalone model.
    assert!(
        schema
            .namespace()
            .iter()
            .all(|declaration| declaration.impls().entries().is_empty()),
        "no declaration carries a fused catalog"
    );
}

/// An `impls` block entry keyed by a lowercase, undotted name is rejected — an
/// impls entry must be a capitalized `TypeName.[ … ]`, so the reader never
/// silently accepts a malformed key.
#[test]
fn undotted_impls_entry_is_rejected() {
    let source = "{}\n[]\n[]\n{ RecordIdentifier.String }\n{}\n{ recordIdentifier }";
    let error =
        SchemaSourceArtifact::from_schema_text(source).expect_err("an undotted impls entry fails");
    assert!(
        matches!(error, SchemaError::ExpectedSyntaxDeclaration { .. }),
        "expected an ExpectedSyntaxDeclaration error, got: {error}"
    );
}

// ---- STEP 3: lowering the catalog to an enumerable manifest ----

/// A `RecordIdentifier.[ Display Ord ]` impls entry lowers to a standalone
/// `ImplBlock` whose catalog enumerates both marker traits, in order, and the
/// declaration it targets carries no fused catalog.
#[test]
fn markers_lower_to_a_standalone_block() {
    let schema = lower_fixture("fused-markers");
    let namespace = schema.namespace();
    let declaration = namespace
        .iter()
        .find(|declaration| declaration.name().as_str() == "RecordIdentifier")
        .expect("RecordIdentifier lowers");
    assert!(
        declaration.impls().entries().is_empty(),
        "impls do not ride on the declaration under the uniform-standalone model"
    );

    let blocks = schema.impl_blocks();
    let [block] = blocks.as_slice() else {
        panic!("expected one standalone impl block, found {blocks:?}");
    };
    assert_eq!(block.target().as_str(), "RecordIdentifier");
    let entries = block.catalog().entries();
    assert_eq!(entries.len(), 2, "two marker impls attach to the block");
    assert!(matches!(&entries[0], ImplReference::Marker(name) if name.as_str() == "Display"));
    assert!(matches!(&entries[1], ImplReference::Marker(name) if name.as_str() == "Ord"));

    // The schema-wide manifest names the same target/entries.
    let manifest = schema.referenced_impls();
    assert_eq!(manifest.len(), 2, "manifest enumerates both marker entries");
    assert!(
        manifest
            .iter()
            .all(|reference| reference.target().as_str() == "RecordIdentifier"),
        "every entry targets RecordIdentifier"
    );
}

/// A trait + method-signature entry lowers to a `TraitImpl` with resolved
/// parameter and return type references — the catalog carries callable
/// signatures, not opaque atoms.
#[test]
fn trait_method_signatures_lower_with_resolved_references() {
    let schema = lower_fixture("trait-method-sigs");
    let blocks = schema.impl_blocks();
    let [block] = blocks.as_slice() else {
        panic!("expected one standalone impl block, found {blocks:?}");
    };
    assert_eq!(block.target().as_str(), "NodeQuery");

    let entries = block.catalog().entries();
    let [ImplReference::TraitImpl(trait_name, methods)] = entries else {
        panic!("expected one trait impl, found {entries:?}");
    };
    assert_eq!(trait_name.as_str(), "QueryMatcher");
    assert_eq!(methods.len(), 1, "one method signature on the trait impl");
    let signature = &methods[0];
    assert_eq!(signature.name().as_str(), "matches");
    assert_eq!(signature.parameters().len(), 1, "one parameter");
    assert_eq!(signature.parameters()[0].name().as_str(), "candidate");
    assert_eq!(
        signature.parameters()[0].reference(),
        &TypeReference::Plain(Name::new("Node")),
        "parameter type resolves to a Node reference"
    );
    assert_eq!(
        signature.return_reference(),
        &TypeReference::Boolean,
        "return type resolves to the Boolean scalar"
    );
}

/// A `StatementText.[ … ]` impls entry surfaces as a standalone `ImplBlock`
/// targeting `StatementText` — the type declared by a separate type entry
/// (Ruling 1: the target must resolve to a type declared elsewhere in the same
/// schema). Its catalog is enumerable through the schema-wide manifest.
#[test]
fn impls_entry_lowers_to_a_standalone_impl_block() {
    let schema = lower_fixture("body-optional");

    // The target is declared exactly once, by its type entry — the impls entry
    // adds no second declaration.
    assert_eq!(
        schema
            .namespace()
            .iter()
            .filter(|declaration| declaration.name().as_str() == "StatementText")
            .count(),
        1,
        "the impls target is declared by a type entry, not minted twice"
    );

    let blocks = schema.impl_blocks();
    let [block] = blocks.as_slice() else {
        panic!("expected one standalone impl block, found {blocks:?}");
    };
    assert_eq!(block.target().as_str(), "StatementText");
    let entries = block.catalog().entries();
    assert_eq!(entries.len(), 2, "one marker plus one inherent method");
    assert!(matches!(&entries[0], ImplReference::Marker(name) if name.as_str() == "Display"));
    let ImplReference::InherentMethod(signature) = &entries[1] else {
        panic!("expected an inherent method, found {:?}", entries[1]);
    };
    assert_eq!(signature.name().as_str(), "word_count");
    assert!(signature.parameters().is_empty(), "nullary method");
    assert_eq!(signature.return_reference(), &TypeReference::Integer);

    // The manifest reaches the block's entries by their target.
    let manifest = schema.referenced_impls();
    assert_eq!(manifest.len(), 2, "manifest reaches the impls entries");
    assert!(
        manifest
            .iter()
            .all(|reference| reference.target().as_str() == "StatementText"),
        "the impls entries target StatementText"
    );
}

// ---- STEP 3: the out-of-band trust-boundary verification ----

/// The "available Rust surface" for the trait-method-sigs fixture: the exact
/// facts a real crate would expose — the `QueryMatcher` trait implemented for
/// `NodeQuery`, and the `matches(candidate: Node) -> Boolean` method present
/// on it. Declared by hand here so the seam is exercised without parsing a
/// real crate.
fn node_query_surface() -> RustSurface {
    RustSurface::new(vec![
        ImplFact::trait_impl(Name::new("NodeQuery"), Name::new("QueryMatcher")),
        ImplFact::method(
            Name::new("NodeQuery"),
            MethodSignature::new(
                Name::new("matches"),
                vec![MethodParameter::new(
                    Name::new("candidate"),
                    TypeReference::Plain(Name::new("Node")),
                )],
                TypeReference::Boolean,
            ),
        ),
    ])
}

/// The trust boundary: when every referenced trait/method signature is
/// present on the declared Rust surface, verification passes.
#[test]
fn present_signatures_pass_verification() {
    let schema = lower_fixture("trait-method-sigs");
    node_query_surface()
        .verify_catalog(&schema)
        .expect("a catalog referencing only present signatures verifies");
}

/// The falsifiable half of the trust boundary: a reference to an ABSENT
/// method signature must FAIL with a typed error naming the exact missing
/// signature.
#[test]
fn absent_method_signature_fails_verification() {
    let schema = lower_fixture("trait-method-sigs");
    let surface = RustSurface::new(vec![ImplFact::trait_impl(
        Name::new("NodeQuery"),
        Name::new("QueryMatcher"),
    )]);

    let error = surface
        .verify_catalog(&schema)
        .expect_err("a reference to an absent method must fail verification");

    let SchemaError::UnverifiedImplReference {
        target,
        kind,
        signature,
    } = &error
    else {
        panic!("expected an UnverifiedImplReference error, got: {error}");
    };
    assert_eq!(target, "NodeQuery");
    assert_eq!(*kind, "method signature");
    assert!(
        signature.contains("matches")
            && signature.contains("candidate")
            && signature.contains("Node")
            && signature.contains("Boolean"),
        "the error names the full unverified method signature, got: {signature}"
    );
}

/// The boundary also rejects a reference to an absent TRAIT impl: a marker
/// entry whose trait the surface does not provide fails, naming the trait.
#[test]
fn absent_trait_impl_fails_verification() {
    let schema = lower_fixture("fused-markers");
    let surface = RustSurface::new(vec![ImplFact::trait_impl(
        Name::new("RecordIdentifier"),
        Name::new("Display"),
    )]);

    let error = surface
        .verify_catalog(&schema)
        .expect_err("a reference to an absent trait impl must fail verification");

    let SchemaError::UnverifiedImplReference {
        target,
        kind,
        signature,
    } = &error
    else {
        panic!("expected an UnverifiedImplReference error, got: {error}");
    };
    assert_eq!(target, "RecordIdentifier");
    assert_eq!(*kind, "trait impl");
    assert_eq!(signature, "Ord", "the error names the absent trait");
}

// ---- STEP A, Fix 1: impl-target resolution ----

/// Project a schema's whole impl manifest to comparable owned data: each
/// `(target, entry)` pair from `referenced_impls`, cloned so it outlives the
/// borrowed schema.
fn manifest_pairs(schema: &TrueSchema) -> Vec<(String, ImplReference)> {
    schema
        .referenced_impls()
        .into_iter()
        .map(|reference| {
            (
                reference.target().as_str().to_owned(),
                reference.entry().clone(),
            )
        })
        .collect()
}

/// Lower a schema text through the macro/document path (`lower_source`).
fn lower_via_macro_path(source: &str) -> TrueSchema {
    SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("example", "0.1.0"))
        .expect("macro path lowers")
}

/// Lower a schema text through the typed-source path (`lower_schema_source`).
fn lower_via_source_path(source: &str) -> TrueSchema {
    let artifact = SchemaSourceArtifact::from_schema_text(source).expect("source decodes");
    SchemaEngine::default()
        .lower_schema_source(artifact.source(), SchemaIdentity::new("example", "0.1.0"))
        .expect("source path lowers")
}

/// Ruling 1: an `impls` entry whose target is NOT declared anywhere in the
/// schema is a typed error. The fixture references `StatementText`, which is
/// never declared (only `Topic` is), so lowering rejects it.
#[test]
fn unresolved_impl_target_is_rejected() {
    let artifact =
        SchemaSourceArtifact::from_schema_text(&impl_catalog_fixture("unresolved-target"))
            .expect("source decodes");
    let error = SchemaEngine::default()
        .lower_schema_source(artifact.source(), SchemaIdentity::new("example", "0.1.0"))
        .expect_err("an impls entry over an undeclared type must be rejected");

    let SchemaError::UnresolvedImplTarget { name } = &error else {
        panic!("expected an UnresolvedImplTarget error, got: {error}");
    };
    assert_eq!(
        name, "StatementText",
        "the error names the undeclared target"
    );
}

/// The same unresolved-target rejection holds on the macro/document path.
#[test]
fn unresolved_impl_target_is_rejected_on_both_paths() {
    let source = impl_catalog_fixture("unresolved-target");
    let error = SchemaEngine::default()
        .lower_source(&source, SchemaIdentity::new("example", "0.1.0"))
        .expect_err("the macro path must reject an impls entry over an undeclared type");
    assert!(
        matches!(&error, SchemaError::UnresolvedImplTarget { name } if name == "StatementText"),
        "macro path names the undeclared target, got: {error}"
    );
}

// ---- STEP A, Fix 2: duplicate vs. composing impl blocks ----

/// Ruling 2: multiple impls entries for the SAME target COMPOSE — their
/// distinct entries union. Here two entries target `StatementText`, one
/// carrying `Display`, the other `Ord`; the manifest enumerates both.
#[test]
fn distinct_impls_entries_for_one_target_compose() {
    let source = "{}\n[]\n[]\n{ StatementText.String }\n{}\n{ StatementText.[ Display ] StatementText.[ Ord ] }";
    let schema = lower_via_source_path(source);

    let pairs = manifest_pairs(&schema);
    assert_eq!(pairs.len(), 2, "two distinct entries union onto one target");
    assert!(
        pairs.iter().all(|(target, _)| target == "StatementText"),
        "both entries target StatementText"
    );
    let traits: Vec<&str> = pairs
        .iter()
        .filter_map(|(_, entry)| match entry {
            ImplReference::Marker(name) => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(traits, vec!["Display", "Ord"], "the two markers compose");
}

/// Ruling 2: a TRUE duplicate — the same trait marker twice on one target,
/// across two entries — is a typed error.
#[test]
fn duplicate_marker_across_entries_is_rejected() {
    let source = "{}\n[]\n[]\n{ StatementText.String }\n{}\n{ StatementText.[ Display ] StatementText.[ Display ] }";
    let artifact = SchemaSourceArtifact::from_schema_text(source).expect("source decodes");
    let error = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("example", "0.1.0"),
        )
        .expect_err("the same marker twice on one target must be rejected");

    let SchemaError::DuplicateImplEntry { target, entry } = &error else {
        panic!("expected a DuplicateImplEntry error, got: {error}");
    };
    assert_eq!(target, "StatementText");
    assert_eq!(entry, "Display", "the error names the duplicated marker");
}

/// A true duplicate of the same method SIGNATURE on one target is rejected.
#[test]
fn duplicate_method_signature_on_one_target_is_rejected() {
    let source = "{}\n[]\n[]\n{ StatementText.String }\n{}\n{ StatementText.[ (word_count {} Integer) ] StatementText.[ (word_count {} Integer) ] }";
    let artifact = SchemaSourceArtifact::from_schema_text(source).expect("source decodes");
    let error = artifact
        .source()
        .lower(
            &SchemaEngine::default(),
            SchemaIdentity::new("example", "0.1.0"),
        )
        .expect_err("the same method signature twice on one target must be rejected");

    let SchemaError::DuplicateImplEntry { target, entry } = &error else {
        panic!("expected a DuplicateImplEntry error, got: {error}");
    };
    assert_eq!(target, "StatementText");
    assert!(
        entry.contains("word_count") && entry.contains("Integer"),
        "the error carries the full signature, got: {entry}"
    );
}

/// Two methods with the SAME name but DIFFERENT signatures are distinct, not a
/// duplicate — they compose.
#[test]
fn distinct_method_signatures_same_name_compose() {
    let source = "{}\n[]\n[]\n{ Topic.String StatementText.String }\n{}\n{ StatementText.[ (length {} Integer) (length { unit.Topic } Integer) ] }";
    let schema = lower_via_source_path(source);
    let pairs = manifest_pairs(&schema);
    assert_eq!(
        pairs.len(),
        2,
        "two same-named methods with different signatures compose"
    );
}

// ---- STEP A, Fix 3: lowering-path parity ----

/// The load-bearing correctness witness: one schema text lowered through the
/// macro/document path and through the typed-source path must produce the SAME
/// impl manifest AND the same standalone impl blocks.
#[test]
fn both_lowering_paths_produce_the_same_impls() {
    let source = "{}\n[]\n[]\n{ RecordIdentifier.String StatementText.String }\n{}\n{ RecordIdentifier.[ Display Ord ] StatementText.[ Display (word_count {} Integer) ] }";

    let macro_schema = lower_via_macro_path(source);
    let source_schema = lower_via_source_path(source);

    assert_eq!(
        manifest_pairs(&macro_schema),
        manifest_pairs(&source_schema),
        "the two lowering paths must enumerate the same impl manifest"
    );

    let macro_blocks: Vec<(String, usize)> = macro_schema
        .impl_blocks()
        .iter()
        .map(|block| {
            (
                block.target().as_str().to_owned(),
                block.catalog().entries().len(),
            )
        })
        .collect();
    let source_blocks: Vec<(String, usize)> = source_schema
        .impl_blocks()
        .iter()
        .map(|block| {
            (
                block.target().as_str().to_owned(),
                block.catalog().entries().len(),
            )
        })
        .collect();
    assert_eq!(
        macro_blocks, source_blocks,
        "the two paths must surface the same standalone impl blocks"
    );

    assert!(
        !manifest_pairs(&macro_schema).is_empty(),
        "the parity witness must compare a non-empty manifest"
    );
}

/// Parity also holds for a single-target impls block: the entry lowers to the
/// same standalone block on both paths, and no catalog rides on a declaration.
#[test]
fn both_lowering_paths_carry_standalone_catalogs() {
    let source =
        "{}\n[]\n[]\n{ RecordIdentifier.String }\n{}\n{ RecordIdentifier.[ Display Ord ] }";
    let macro_schema = lower_via_macro_path(source);
    let source_schema = lower_via_source_path(source);

    assert_eq!(
        manifest_pairs(&macro_schema),
        manifest_pairs(&source_schema)
    );
    assert_eq!(
        manifest_pairs(&macro_schema).len(),
        2,
        "both paths carry the two markers"
    );
    assert_eq!(
        macro_schema.impl_blocks().len(),
        1,
        "the catalog lowers to one standalone block"
    );
    assert_eq!(
        macro_schema.impl_blocks().len(),
        source_schema.impl_blocks().len()
    );
    assert!(
        macro_schema
            .namespace()
            .iter()
            .all(|declaration| declaration.impls().entries().is_empty()),
        "no declaration carries a fused catalog on either path"
    );
}

/// Report 702: the collapse to one lowering engine. A six-block document must
/// lower IDENTICALLY through the document entry point (`lower_source`) and the
/// typed-source entry point — the single load-bearing witness that there is one
/// engine.
#[test]
fn both_lowering_paths_agree_on_a_six_block_schema() {
    let source = "\
{}
[]
[]
{
  ActorIdentifier.String
  ContractName.String
  Envelope.{ ActorIdentifier ContractName }
}
{}
{ Envelope.[ Display ] }
";

    let macro_schema = lower_via_macro_path(source);
    let source_schema = lower_via_source_path(source);

    let macro_types: Vec<String> = macro_schema
        .namespace()
        .iter()
        .map(|declaration| declaration.name().as_str().to_owned())
        .collect();
    let source_types: Vec<String> = source_schema
        .namespace()
        .iter()
        .map(|declaration| declaration.name().as_str().to_owned())
        .collect();
    assert_eq!(
        macro_types, source_types,
        "both entry points must lower the schema to the same types"
    );
    assert_eq!(
        macro_schema.core_hash(),
        source_schema.core_hash(),
        "one schema text lowers to one core identity regardless of entry path"
    );
}

// ---- STEP B, Fix 4: trait-name validation ----

/// Fix 4: a trait atom inside an impls catalog must be a PascalCase type name,
/// like every other type reference. A lowercase trait atom is a typed error.
#[test]
fn lowercase_trait_name_is_rejected() {
    let source = "{}\n[]\n[]\n{ RecordIdentifier.String }\n{}\n{ RecordIdentifier.[ display ] }";
    let error =
        SchemaSourceArtifact::from_schema_text(source).expect_err("a lowercase trait is rejected");

    let SchemaError::NonTypeNameTrait { found } = &error else {
        panic!("expected a NonTypeNameTrait error, got: {error}");
    };
    assert_eq!(found, "display", "the error names the non-type-name trait");
}

/// The same trait-name gate holds on a body-bearing trait impl entry.
#[test]
fn lowercase_trait_name_with_methods_is_rejected() {
    let source = "{}\n[]\n[]\n{ NodeQuery.String }\n{}\n{ NodeQuery.[ queryMatcher [ (matches { candidate.Node } Boolean) ] ] }";
    let error =
        SchemaSourceArtifact::from_schema_text(source).expect_err("a lowercase trait is rejected");
    assert!(
        matches!(&error, SchemaError::NonTypeNameTrait { found } if found == "queryMatcher"),
        "the body-bearing trait impl path also rejects a non-type-name trait, got: {error}"
    );
}

// ---- STEP B, Fix 5: full signature in the unverified-reference error ----

/// Fix 5: a reference whose method NAME matches a present method but whose
/// PARAMETERS or RETURN type differ is a real mismatch. The verification error
/// must carry the full referenced signature.
#[test]
fn signature_mismatch_reports_the_full_signature() {
    let schema = lower_fixture("trait-method-sigs");

    let surface = RustSurface::new(vec![
        ImplFact::trait_impl(Name::new("NodeQuery"), Name::new("QueryMatcher")),
        ImplFact::method(
            Name::new("NodeQuery"),
            MethodSignature::new(
                Name::new("matches"),
                vec![MethodParameter::new(
                    Name::new("candidate"),
                    TypeReference::Plain(Name::new("Node")),
                )],
                TypeReference::Plain(Name::new("Node")),
            ),
        ),
    ]);

    let error = surface
        .verify_catalog(&schema)
        .expect_err("a present name with a mismatched signature must fail verification");

    let SchemaError::UnverifiedImplReference {
        target,
        kind,
        signature,
    } = &error
    else {
        panic!("expected an UnverifiedImplReference error, got: {error}");
    };
    assert_eq!(target, "NodeQuery");
    assert_eq!(*kind, "method signature");
    assert!(
        signature.contains("matches")
            && signature.contains("candidate")
            && signature.contains("Node")
            && signature.contains("Boolean"),
        "the error carries the full mismatched signature, got: {signature}"
    );
    assert_ne!(
        signature, "matches",
        "the error must not report the bare method name alone"
    );
}

/// Finding 4 (validation): a method parameter's TYPE is a type reference, so its
/// leaf must be capitalized per the capitalization tenet. A lowercase parameter
/// type is a typed error. Negative witness plus a positive control.
#[test]
fn lowercase_method_parameter_type_is_a_typed_rejection() {
    let lowercase = "{}\n[]\n[]\n{ NodeQuery.{ Differentiator } }\n{}\n{ NodeQuery.[ QueryMatcher [ (matches { candidate.node } Boolean) ] ] }";
    let error = SchemaSourceArtifact::from_schema_text(lowercase)
        .expect_err("a lowercase method-parameter type is rejected at parse");
    assert!(
        matches!(error, SchemaError::ExpectedTypeReferenceLeaf { .. }),
        "expected ExpectedTypeReferenceLeaf, got: {error}"
    );

    let capitalized = "{}\n[]\n[]\n{ NodeQuery.{ Differentiator } }\n{}\n{ NodeQuery.[ QueryMatcher [ (matches { candidate.Node } Boolean) ] ] }";
    SchemaSourceArtifact::from_schema_text(capitalized)
        .expect("a capitalized method-parameter type parses");
}
