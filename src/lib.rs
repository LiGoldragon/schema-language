mod core;
mod declarative;
mod engine;
mod environment;
mod identifier;
mod identity;
mod instance;
mod lineage;
mod macros;
mod module;
mod raw;
mod resolution;
mod schema;
mod source;
mod upgrade;
mod view;

pub use instance::InstanceSchemaText;

pub use crate::core::{
    CoreApplicationHead, CoreDeclaration, CoreEnum, CoreField, CoreImplBlock, CoreNewtype,
    CoreReference, CoreResolvedImport, CoreRoot, CoreRootApplication, CoreSchema, CoreStruct,
    CoreType, CoreVariant,
};
pub use declarative::{
    MacroDelimiter, MacroLibrary, MacroLibraryArtifact, MacroLibrarySourceEntry, MacroPattern,
    MacroPatternDelimited, MacroPatternObject, MacroTemplate, MacroTemplateDelimited,
    MacroTemplateObject, SchemaMacro, TypeTemplate,
};
pub use engine::{SchemaEngine, SchemaError, SchemaIdentity};
pub use environment::{
    SchemaEnvironment, SchemaEnvironmentManifest, SchemaEnvironmentModule, SchemaEnvironmentResult,
    SchemaNodeType, SchemaNodeTypeLabel, SchemaRootBlockKind, SchemaRootBlockSummary,
    SchemaSourcePosition, SchemaSourceRange, SchemaSourceSummary,
};
pub use identifier::{
    DeclarationKind, NameDeclaration, NameEntry, NameHarvest, NameTable, NominalIdentifier,
};
pub use identity::ContentHash;
pub use lineage::LineageGraph;
pub use macros::{
    MacroContext, MacroDispatch, MacroNodeDefinition, MacroObject, MacroOutput, MacroPair,
    MacroPosition, MacroRegistry, SchemaMacroHandler,
};
pub use module::{SchemaModuleSource, SchemaPackage};
pub use nota::{
    AtomCase, AtomShape, CaptureName, DelimitedShape, MacroCandidate,
    MacroDelimiter as NotaMacroDelimiter, MacroNodeDefinition as NotaMacroNodeDefinition,
    MacroObjectCount, Pattern, PatternElement, PositionPredicate, SigilPosition, SigilSpec,
};
pub use raw::{RawDatatypeEntry, RawDatatypeMap, RawNotaDatatype, RawNotaSequence, RawSchemaFile};
pub use resolution::{ImportResolver, ImportSource, ResolvedImport};
pub use schema::{
    ApplicationHead, Declaration, DeclarationHead, EnumDeclaration, EnumVariant, FieldDeclaration,
    ImplBlock, ImplCatalog, ImplCompositionKey, ImplFact, ImplReference, ImportDeclaration,
    MethodParameter, MethodSignature, MultiTypeReferenceProjection, Name, NewtypeDeclaration, Root,
    RootApplication, RustSurface, SchemaNode, SchemaNodeData, SchemaNodePair, SchemaNodeValue,
    SingleTypeReferenceProjection, StructDeclaration, StructFieldMap, SymbolPath,
    SymbolPathPosition, TypeDeclaration, TypeReference, ValueReferenceProjection, Visibility,
};
pub use source::{
    FactoredEncoding, HelpRendering, IndirectionLink, IndirectionProjection,
    LinkedStructureExpansion, MainStructureDepthCap, SchemaSource, SchemaSourceArtifact,
    SourceDeclaration, SourceDeclarationValue, SourceDeclarations, SourceEnumBody, SourceField,
    SourceFieldIdentity, SourceFieldValue, SourceGenericEntry, SourceGenerics, SourceImplCatalog,
    SourceImplEntry, SourceImpls, SourceImplsEntry, SourceImport, SourceImports,
    SourceMethodParameter, SourceMethodSignature, SourceReference, SourceRootBody, SourceRootEnum,
    SourceStructBody, SourceTypeEntry, SourceTypes, SourceVariantName, SourceVariantPayload,
    SourceVariantSignature,
};
pub use upgrade::{
    AddField, AddVariant, ChangeFieldType, DefaultValue, EditEffect, FieldMigration, MigrationSpec,
    NameTableDelta, Rename, SchemaEdit, SchemaEditApplication, SchemaEditReceipt, UpgradeObject,
    UpgradeReceipt,
};
pub use view::{
    DeclarationView, EnumView, FieldView, ImplBlockView, NewtypeView, ReferencedImplView,
    RootApplicationView, RootView, SchemaDeclaredType, StructView, TrueSchema, TypeDeclarationView,
    VariantView,
};
