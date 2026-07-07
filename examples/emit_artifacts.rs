use schema_language::{MacroLibrary, MacroLibraryArtifact, SchemaSourceArtifact};

fn main() {
    let macro_library = MacroLibrary::from_source(include_str!("../schemas/builtin-macros.schema"))
        .expect("builtin macro source lowers");
    println!("=== builtin-macros.macro-library ===");
    println!(
        "{}",
        MacroLibraryArtifact::new(macro_library).to_nota_source()
    );

    let core_source = include_str!("../schemas/core.schema");
    let core_artifact =
        SchemaSourceArtifact::from_schema_text(core_source).expect("core schema source decodes");
    println!("=== core.schema ===");
    println!("{}", core_artifact.to_schema_text());
}
