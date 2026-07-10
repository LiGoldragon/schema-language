use std::fs;

use schema_language::{
    ImplFact, ImplReference, MethodParameter, MethodSignature, Name, RustSurface, SchemaEngine,
    SchemaError, SchemaIdentity, SchemaSourceArtifact, SourceImplEntry, SourceNamespaceEntry,
    SourceReference, TrueSchema, TypeDeclaration, TypeReference,
};

fn impl_catalog_fixture(name: &str) -> String {
    fs::read_to_string(format!("tests/fixtures/impl-catalog/{name}.schema"))
        .unwrap_or_else(|error| panic!("read impl-catalog schema fixture {name}: {error}"))
        .trim_end()
        .to_owned()
}

fn namespace_entries(artifact: &SchemaSourceArtifact) -> Vec<SourceNamespaceEntry> {
    artifact.source().namespace().entries().to_vec()
}

/// Lower a fixture through the typed source archive into a `TrueSchema`, the
/// path that carries the full impl catalog onto each `Declaration` and the
/// standalone `ImplBlock`s.
fn lower_fixture(name: &str) -> TrueSchema {
    let artifact = SchemaSourceArtifact::from_schema_text(&impl_catalog_fixture(name))
        .expect("source decodes");
    SchemaEngine::default()
        .lower_schema_source(artifact.source(), SchemaIdentity::new("example", "0.1.0"))
        .unwrap_or_else(|error| panic!("lower impl-catalog fixture {name}: {error}"))
}

/// The canonical schema-source text must be byte-stable through
/// decode -> to_schema_text -> re-decode, with the `{| … |}` impl block
/// surfaced verbatim. This is the same round-trip contract the source codec
/// tests assert, extended to the new trailing impl-block syntax.
#[test]
fn fused_marker_impls_round_trip() {
    let source = impl_catalog_fixture("fused-markers");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");
    let canonical = artifact.to_schema_text();

    assert_eq!(
        canonical, source,
        "fused marker impls should write a byte-stable canonical surface"
    );
    assert!(
        canonical.contains("RecordIdentifier String {| Display Ord |}"),
        "canonical surface must carry the fused marker impl block: {canonical}"
    );

    let recovered =
        SchemaSourceArtifact::from_schema_text(&canonical).expect("canonical source decodes");
    assert_eq!(
        artifact, recovered,
        "canonical schema source should recover the same source object"
    );
}

#[test]
fn body_optional_impls_round_trip() {
    let source = impl_catalog_fixture("body-optional");
    let artifact = SchemaSourceArtifact::from_schema_text(&source).expect("schema source decodes");
    let canonical = artifact.to_schema_text();

    assert_eq!(
        canonical, source,
        "body-optional impls should write a byte-stable canonical surface"
    );
    assert!(
        canonical.contains("StatementText {| Display (word_count {} Integer) |}"),
        "canonical surface must carry the body-optional impl block: {canonical}"
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
        canonical.contains("{| QueryMatcher [ (matches { candidate.Node } Boolean) ] |}"),
        "canonical surface must carry the trait impl with method signatures: {canonical}"
    );

    let recovered =
        SchemaSourceArtifact::from_schema_text(&canonical).expect("canonical source decodes");
    assert_eq!(artifact, recovered);
}

/// The new typed impl-catalog nouns must survive the rkyv archive boundary —
/// the same binary round-trip the source codec asserts for every typed
/// source noun. This is what proves `SourceImplCatalog` / `SourceImplEntry` /
/// `SourceMethodSignature` are real archive members, not parser-only state.
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

/// The decoded catalog must expose its entries as typed data: markers as
/// trait names, trait impls with their method signatures, and inherent
/// method signatures with typed return references.
#[test]
fn impl_catalog_decodes_each_entry_kind() {
    let fused = SchemaSourceArtifact::from_schema_text(&impl_catalog_fixture("fused-markers"))
        .expect("schema source decodes");
    let entries = namespace_entries(&fused);
    let [record_identifier] = entries.as_slice() else {
        panic!("expected one namespace entry, found {}", entries.len());
    };
    let markers = record_identifier.impls().entries();
    assert_eq!(markers.len(), 2, "two marker impls");
    assert!(matches!(&markers[0], SourceImplEntry::Marker(name) if name.as_str() == "Display"));
    assert!(matches!(&markers[1], SourceImplEntry::Marker(name) if name.as_str() == "Ord"));

    let body_optional =
        SchemaSourceArtifact::from_schema_text(&impl_catalog_fixture("body-optional"))
            .expect("schema source decodes");
    let entries = namespace_entries(&body_optional);
    // Two entries share the name `StatementText`: the body-bearing declaration
    // and the body-optional impl block (Ruling 1: the impl target is declared
    // elsewhere). The catalog rides on the body-optional entry.
    assert_eq!(
        entries.len(),
        2,
        "a declaration entry plus a body-optional impl entry"
    );
    let statement_text = entries
        .iter()
        .find(|entry| !entry.impls().is_empty())
        .expect("the body-optional entry carries the catalog");
    let catalog = statement_text.impls().entries();
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
    let entries = namespace_entries(&trait_sigs);
    let [node_query] = entries.as_slice() else {
        panic!("expected one namespace entry, found {}", entries.len());
    };
    let catalog = node_query.impls().entries();
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

/// The macro/engine namespace walk (the second of the two parallel parsers)
/// must accept the same fused and body-optional shapes: a fused entry lowers
/// its inline body to a type declaration while the trailing `{| … |}` block
/// is skipped, and a body-optional `TypeName {| … |}` mints no declaration on
/// this path. This proves the engine walk and the source walk segment entries
/// identically — the boundary the plan flags as the riskiest divergence.
#[test]
fn engine_namespace_walk_accepts_fused_and_body_optional_entries() {
    // The body-optional `StatementText {| … |}` references a `StatementText`
    // declared by a separate body-bearing entry (Ruling 1).
    let source = "{} [] [] { RecordIdentifier String {| Display Ord |} StatementText String StatementText {| Display |} Topic String }";
    let schema = SchemaEngine::default()
        .lower_source(source, SchemaIdentity::new("example", "0.1.0"))
        .expect("engine lowers fused and body-optional entries");

    let TypeDeclaration::Newtype(record_identifier) = schema
        .type_named("RecordIdentifier")
        .expect("fused body lowers")
    else {
        panic!("RecordIdentifier should lower to a newtype from its inline body");
    };
    assert_eq!(record_identifier.reference, TypeReference::String);

    let TypeDeclaration::Newtype(topic) = schema
        .type_named("Topic")
        .expect("entry after an impl block still lowers")
    else {
        panic!("Topic should lower to a newtype");
    };
    assert_eq!(topic.reference, TypeReference::String);

    // The body-optional entry mints no *second* declaration; the target is
    // the one declared by the body-bearing entry.
    assert_eq!(
        schema
            .namespace()
            .iter()
            .filter(|declaration| declaration.name().as_str() == "StatementText")
            .count(),
        1,
        "the body-optional target is declared once, by its body-bearing entry"
    );

    // The macro/engine path now carries the impl catalog too (Ruling/Fix 3
    // parity): the fused markers ride on RecordIdentifier, and the
    // body-optional block targets StatementText.
    let namespace = schema.namespace();
    let record_identifier_impls = namespace
        .iter()
        .find(|declaration| declaration.name().as_str() == "RecordIdentifier")
        .expect("RecordIdentifier declared")
        .impls()
        .entries();
    assert_eq!(
        record_identifier_impls.len(),
        2,
        "the engine path carries the fused marker catalog"
    );

    let impl_blocks = schema.impl_blocks();
    let [block] = impl_blocks.as_slice() else {
        panic!("expected one standalone impl block on the engine path, found {impl_blocks:?}");
    };
    assert_eq!(block.target().as_str(), "StatementText");
}

/// A `{| … |}` impl block must trail a type name — a leading impl block with
/// no preceding head is rejected, proving the entry walk does not silently
/// swallow a stray pipe-brace.
#[test]
fn leading_impl_block_is_rejected() {
    let source = "{}\n[]\n[]\n{\n  {| Display |}\n}";
    let error =
        SchemaSourceArtifact::from_schema_text(source).expect_err("leading impl block is rejected");
    let message = error.to_string();
    assert!(
        message.contains("impl block") && message.contains("trail"),
        "error should name the leading-impl-block boundary, got: {message}"
    );
}

// ---- STEP 3: lowering the catalog to an enumerable manifest ----

/// A fused `RecordIdentifier String {| Display Ord |}` lowers to a newtype
/// declaration whose `impls()` enumerates both marker traits, in order.
#[test]
fn fused_markers_lower_onto_the_declaration() {
    let schema = lower_fixture("fused-markers");
    let namespace = schema.namespace();
    let declaration = namespace
        .iter()
        .find(|declaration| declaration.name().as_str() == "RecordIdentifier")
        .expect("RecordIdentifier lowers");

    let entries = declaration.impls().entries();
    assert_eq!(
        entries.len(),
        2,
        "two marker impls attach to the declaration"
    );
    assert!(matches!(&entries[0], ImplReference::Marker(name) if name.as_str() == "Display"));
    assert!(matches!(&entries[1], ImplReference::Marker(name) if name.as_str() == "Ord"));

    // The schema-wide manifest names the same target/entries.
    let manifest = schema.referenced_impls();
    assert_eq!(manifest.len(), 2, "manifest enumerates both marker entries");
    assert!(
        manifest
            .iter()
            .all(|reference| reference.target().as_str() == "RecordIdentifier"),
        "every fused entry targets RecordIdentifier"
    );
}

/// A trait + method-signature entry lowers to a `TraitImpl` with resolved
/// parameter and return type references — the catalog carries callable
/// signatures, not opaque atoms.
#[test]
fn trait_method_signatures_lower_with_resolved_references() {
    let schema = lower_fixture("trait-method-sigs");
    let namespace = schema.namespace();
    let declaration = namespace
        .iter()
        .find(|declaration| declaration.name().as_str() == "NodeQuery")
        .expect("NodeQuery lowers");

    let entries = declaration.impls().entries();
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

/// A body-optional `StatementText {| … |}` mints no type declaration of its
/// own but surfaces as a standalone `ImplBlock` targeting `StatementText` —
/// the type declared by a separate body-bearing entry (Ruling 1: the target
/// must resolve to a type declared elsewhere in the same schema). Its catalog
/// is enumerable through the schema-wide manifest.
#[test]
fn body_optional_lowers_to_a_standalone_impl_block() {
    let schema = lower_fixture("body-optional");

    // The target is declared exactly once, by the body-bearing entry — the
    // body-optional impl entry adds no second declaration.
    assert_eq!(
        schema
            .namespace()
            .iter()
            .filter(|declaration| declaration.name().as_str() == "StatementText")
            .count(),
        1,
        "the body-optional target is declared by a separate entry, not minted twice"
    );

    let impl_blocks = schema.impl_blocks();
    let [block] = impl_blocks.as_slice() else {
        panic!("expected one standalone impl block, found {impl_blocks:?}");
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

    // The manifest reaches the body-optional block's entries by their target.
    let manifest = schema.referenced_impls();
    assert_eq!(
        manifest.len(),
        2,
        "manifest reaches the body-optional entries"
    );
    assert!(
        manifest
            .iter()
            .all(|reference| reference.target().as_str() == "StatementText"),
        "the body-optional entries target StatementText"
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
/// present on the declared Rust surface, verification passes. This is the
/// out-of-band catalog check the seam needs — the schema references impls
/// that live on the Rust side, and the boundary confirms they exist.
#[test]
fn present_signatures_pass_verification() {
    let schema = lower_fixture("trait-method-sigs");
    node_query_surface()
        .verify_catalog(&schema)
        .expect("a catalog referencing only present signatures verifies");
}

/// The falsifiable half of the trust boundary: a reference to an ABSENT
/// method signature must FAIL with a typed error naming the exact missing
/// signature. Here the surface knows the `QueryMatcher` trait impl but is
/// missing the `matches` method — the catalog references a method the crate
/// does not provide, and verification rejects it.
#[test]
fn absent_method_signature_fails_verification() {
    let schema = lower_fixture("trait-method-sigs");
    // A surface with the trait impl but WITHOUT the `matches` method.
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
    // Fix 5: the error carries the FULL signature, not just the method name —
    // name plus parameter (candidate.Node) plus return type (Boolean).
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
    // The surface knows `Display` for `RecordIdentifier` but not `Ord`.
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
/// borrowed schema. The order is the manifest's walk order (declarations
/// first, then standalone blocks), which both lowering paths share.
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

/// Ruling 1: a body-optional `TypeName {| … |}` whose target is NOT declared
/// anywhere in the schema is a typed error — not an accepted free-standing
/// impl over an arbitrary name. The fixture references `StatementText`, which
/// is never declared (only `Topic` is), so lowering rejects it.
#[test]
fn unresolved_impl_target_is_rejected() {
    let artifact =
        SchemaSourceArtifact::from_schema_text(&impl_catalog_fixture("unresolved-target"))
            .expect("source decodes");
    let error = SchemaEngine::default()
        .lower_schema_source(artifact.source(), SchemaIdentity::new("example", "0.1.0"))
        .expect_err("an impl block over an undeclared type must be rejected");

    let SchemaError::UnresolvedImplTarget { name } = &error else {
        panic!("expected an UnresolvedImplTarget error, got: {error}");
    };
    assert_eq!(
        name, "StatementText",
        "the error names the undeclared target"
    );
}

/// The same unresolved-target rejection holds on the macro/document path —
/// neither path silently accepts an impl over an undeclared type.
#[test]
fn unresolved_impl_target_is_rejected_on_both_paths() {
    let source = impl_catalog_fixture("unresolved-target");
    let error = SchemaEngine::default()
        .lower_source(&source, SchemaIdentity::new("example", "0.1.0"))
        .expect_err("the macro path must reject an impl over an undeclared type");
    assert!(
        matches!(&error, SchemaError::UnresolvedImplTarget { name } if name == "StatementText"),
        "macro path names the undeclared target, got: {error}"
    );
}

// ---- STEP A, Fix 2: duplicate vs. composing impl blocks ----

/// Ruling 2: multiple impl blocks for the SAME target COMPOSE — their
/// distinct entries union. Here two body-optional blocks target the
/// elsewhere-declared `StatementText`, one carrying `Display`, the other
/// `Ord`; the manifest enumerates both.
#[test]
fn distinct_impl_blocks_for_one_target_compose() {
    let source =
        "{} [] [] { StatementText String StatementText {| Display |} StatementText {| Ord |} }";
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
/// across two blocks — is a typed error. Distinct entries compose; an
/// identical entry collides.
#[test]
fn duplicate_marker_across_blocks_is_rejected() {
    let source =
        "{} [] [] { StatementText String StatementText {| Display |} StatementText {| Display |} }";
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

/// A true duplicate of the same method SIGNATURE on one target is rejected —
/// here the same inherent method appears in a fused catalog and again in a
/// separate body-optional block.
#[test]
fn duplicate_method_signature_on_one_target_is_rejected() {
    let source = "{} [] [] { StatementText String {| (word_count {} Integer) |} StatementText {| (word_count {} Integer) |} }";
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
/// duplicate — they compose. This guards the composition key against
/// collapsing on the method name alone.
#[test]
fn distinct_method_signatures_same_name_compose() {
    let source = "{} [] [] { Topic String StatementText String {| (length {} Integer) (length { unit.Topic } Integer) |} }";
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
/// impl manifest AND the same standalone impl blocks. Before the fix the macro
/// path dropped the catalog entirely; now both carry it identically.
#[test]
fn both_lowering_paths_produce_the_same_impls() {
    let source = "{} [] [] { RecordIdentifier String {| Display Ord |} StatementText String StatementText {| Display (word_count {} Integer) |} }";

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

    // And the manifest is non-empty — the witness would be vacuous if both
    // paths simply produced no impls.
    assert!(
        !manifest_pairs(&macro_schema).is_empty(),
        "the parity witness must compare a non-empty manifest"
    );
}

/// Parity also holds for the fused-only shape (no standalone blocks): the
/// markers ride on the declaration's `impls()` identically on both paths.
#[test]
fn both_lowering_paths_carry_fused_catalogs() {
    let source = "{} [] [] { RecordIdentifier String {| Display Ord |} }";
    let macro_schema = lower_via_macro_path(source);
    let source_schema = lower_via_source_path(source);

    assert_eq!(
        manifest_pairs(&macro_schema),
        manifest_pairs(&source_schema)
    );
    assert_eq!(
        manifest_pairs(&macro_schema).len(),
        2,
        "both paths carry the two fused markers"
    );
    assert!(
        macro_schema.impl_blocks().is_empty() && source_schema.impl_blocks().is_empty(),
        "a fused-only schema mints no standalone blocks on either path"
    );
}

/// Report 702: the collapse to one lowering engine. A nested-namespace
/// document must lower IDENTICALLY through the document entry point
/// (`lower_source`) and the typed-source entry point. The retired second
/// engine had NO nested-namespace case — it lowered a colon-keyed brace as a
/// plain struct — so this exact document used to lower to two different
/// schemas. Now the document path delegates to the source path, so a nested
/// namespace flattens to fully-qualified type names on both entry points.
#[test]
fn both_lowering_paths_flatten_a_nested_namespace_identically() {
    let source = "\
{}
[Deliver.router:routed_object:Envelope]
[]
{
  ActorIdentifier String
  ContractName String
  router:routed_object {
    Destination ActorIdentifier
    Contract ContractName
    Envelope { Destination Contract }
  }
}
";

    let macro_schema = lower_via_macro_path(source);
    let source_schema = lower_via_source_path(source);

    // The nested local `Envelope` flattens to a fully-qualified name on both
    // paths, and the bare local name leaks into neither top-level namespace.
    for (label, schema) in [("document", &macro_schema), ("source", &source_schema)] {
        assert!(
            schema.type_named("router:routed_object:Envelope").is_some(),
            "{label} path flattens the nested Envelope to a qualified type"
        );
        assert!(
            schema.type_named("Envelope").is_none(),
            "{label} path must not leak the bare nested name"
        );
    }

    // The full lowered type vocabulary is identical between the two entry
    // points — the single load-bearing witness that there is one engine.
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
        "both entry points must lower the nested namespace to the same types"
    );
    assert_eq!(
        macro_schema.core_hash(),
        source_schema.core_hash(),
        "one schema text lowers to one core identity regardless of entry path"
    );
}

// ---- STEP B, Fix 4: trait-name validation ----

/// Fix 4: a trait atom inside `{| … |}` must be a PascalCase type name, like
/// every other type reference. A lowercase trait atom is a typed error, not a
/// silently-accepted trait marker.
#[test]
fn lowercase_trait_name_is_rejected() {
    let source = "{} [] [] { RecordIdentifier String {| display |} }";
    let error =
        SchemaSourceArtifact::from_schema_text(source).expect_err("a lowercase trait is rejected");

    let SchemaError::NonTypeNameTrait { found } = &error else {
        panic!("expected a NonTypeNameTrait error, got: {error}");
    };
    assert_eq!(found, "display", "the error names the non-type-name trait");
}

/// The same trait-name gate holds on a body-bearing trait impl entry — a
/// lowercase trait carrying a method-signature vector is still rejected, so the
/// validation is not limited to bare markers.
#[test]
fn lowercase_trait_name_with_methods_is_rejected() {
    let source =
        "{} [] [] { NodeQuery String {| queryMatcher [ (matches { candidate.Node } Boolean) ] |} }";
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
/// must carry the full referenced signature — name, parameters, and return —
/// so the mismatch is legible, not a bare "missing `matches`". Here the surface
/// provides `matches(candidate: Node) -> Boolean` but the catalog references
/// `matches(candidate: Node) -> Boolean`'s name with a different return type on
/// the schema side, so verification fails against the surface's signature.
#[test]
fn signature_mismatch_reports_the_full_signature() {
    let schema = lower_fixture("trait-method-sigs");

    // The surface implements the trait and a `matches` method, but with a
    // DIFFERENT signature than the catalog references: the surface returns a
    // `Node`, while the catalog references the `Boolean`-returning `matches`.
    // Same name, wrong return type — a mismatch, not a missing method.
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
    // The error reports the FULL referenced signature, not just `matches`: the
    // method name, the `candidate.Node` parameter, and the `Boolean` return.
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
/// leaf must be capitalized per the capitalization tenet — a capitalized-leading
/// atom is a type/object, a lowercase one is a name/reference. The seam
/// conversion had dropped this gate, letting a lowercase parameter type through;
/// it is restored with a typed error. Negative witness plus a positive control.
#[test]
fn lowercase_method_parameter_type_is_a_typed_rejection() {
    // `candidate.node` — a lowercase parameter type — must be rejected.
    let lowercase = "{}\n[]\n[]\n{\n  \
        NodeQuery { Differentiator } {| QueryMatcher [ (matches { candidate.node } Boolean) ] |}\n\
        }\n";
    let error = SchemaSourceArtifact::from_schema_text(lowercase)
        .expect_err("a lowercase method-parameter type is rejected at parse");
    assert!(
        matches!(error, SchemaError::ExpectedTypeReferenceLeaf { .. }),
        "expected ExpectedTypeReferenceLeaf, got: {error}"
    );

    // The capitalized control `candidate.Node` still parses cleanly.
    let capitalized = "{}\n[]\n[]\n{\n  \
        NodeQuery { Differentiator } {| QueryMatcher [ (matches { candidate.Node } Boolean) ] |}\n\
        }\n";
    SchemaSourceArtifact::from_schema_text(capitalized)
        .expect("a capitalized method-parameter type parses");
}
