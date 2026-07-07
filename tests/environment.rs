use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use schema_language::{
    Name, SchemaEnvironment, SchemaEnvironmentManifest, SchemaNodeType, SchemaPackage,
    SchemaRootBlockKind,
};

struct FixturePackage {
    root: PathBuf,
}

impl FixturePackage {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after unix epoch")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("schema-environment-{}-{nonce}", std::process::id()));
        fs::create_dir_all(root.join("schema")).expect("fixture schema directory exists");
        Self { root }
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn write_schema(&self, module: &str, source: &str) {
        let path = self.root.join("schema").join(format!("{module}.schema"));
        fs::write(path, source).expect("fixture schema writes");
    }

    fn package(&self) -> SchemaPackage {
        SchemaPackage::new(self.root.clone(), "fixture-crate", "0.1.0")
    }
}

impl Drop for FixturePackage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn environment_loads_manifest_selected_modules_and_source_summaries() {
    let fixture = FixturePackage::new();
    fixture.write_schema(
        "lib",
        "{ Shared fixture-crate:shared:Shared }\n[(UseShared Shared)]\n[]\n{\n  UseShared Shared\n}\n[(Equivalence [UseShared Shared])]\n",
    );
    fixture.write_schema("shared", "{}\n[]\n[]\n{\n  Shared String\n}\n");
    fixture.write_schema("ignored", "{}\n[]\n[]\n{\n  Ignored String\n}\n");

    let environment = SchemaEnvironment::new(fixture.package());
    let manifest = SchemaEnvironmentManifest::new(vec![Name::new("lib")]);
    let result = environment
        .load(&manifest)
        .expect("environment loads selected schema modules");

    assert_eq!(result.modules().len(), 1);
    let module = &result.modules()[0];
    assert_eq!(
        module.source().identity().component().as_str(),
        "fixture-crate:lib"
    );
    assert_eq!(
        module.summary().path(),
        fixture.root().join("schema/lib.schema").as_path()
    );
    assert_eq!(
        module
            .summary()
            .root_blocks()
            .iter()
            .map(|block| block.kind())
            .collect::<Vec<_>>(),
        vec![
            SchemaRootBlockKind::Imports,
            SchemaRootBlockKind::Input,
            SchemaRootBlockKind::Output,
            SchemaRootBlockKind::Namespace,
            SchemaRootBlockKind::Relations,
        ]
    );
    assert_eq!(module.summary().file_range().start().line(), 1);
    assert_eq!(
        module
            .summary()
            .root_blocks()
            .iter()
            .find(|block| block.kind() == SchemaRootBlockKind::Namespace)
            .expect("namespace summary exists")
            .range()
            .start()
            .line(),
        4
    );
    assert_eq!(
        module
            .summary()
            .node_type_labels()
            .iter()
            .map(|label| label.node_type())
            .collect::<Vec<_>>(),
        vec![
            SchemaNodeType::Module,
            SchemaNodeType::Imports,
            SchemaNodeType::InputRoot,
            SchemaNodeType::OutputRoot,
            SchemaNodeType::Namespace,
            SchemaNodeType::Relations,
        ]
    );
}

#[test]
fn environment_round_trips_canonical_source_and_resolves_package_imports() {
    let fixture = FixturePackage::new();
    fixture.write_schema(
        "lib",
        "{ Shared fixture-crate:shared:Shared }\n[(UseShared Shared)]\n[]\n{\n  UseShared Shared\n}\n",
    );
    fixture.write_schema("shared", "{}\n[]\n[]\n{\n  Shared String\n}\n");

    let environment = SchemaEnvironment::new(fixture.package());
    let manifest = SchemaEnvironmentManifest::new(vec![Name::new("lib")]);
    let result = environment
        .load(&manifest)
        .expect("environment resolves package imports");
    let module = &result.modules()[0];
    let canonical = module.artifact().to_schema_text();
    let recovered = schema_language::SchemaSourceArtifact::from_schema_text(&canonical)
        .expect("canonical schema text decodes again");

    assert_eq!(recovered.to_schema_text(), canonical);
    assert_eq!(module.true_schema().resolved_imports().len(), 1);
    assert_eq!(
        module.true_schema().resolved_imports()[0]
            .source()
            .rust_path(),
        "fixture_crate::schema::shared::Shared"
    );
    assert!(module.true_schema().type_named("UseShared").is_some());
}
