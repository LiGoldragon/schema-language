mod core;
mod declarative;
mod engine;
mod environment;
mod expansion;
mod identifier;
mod identity;
mod instance;
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
    CoreApplicationHead, CoreDeclaration, CoreEnum, CoreFamily, CoreField, CoreImplBlock,
    CoreNewtype, CoreReference, CoreRoot, CoreRootApplication, CoreSchema, CoreStream,
    CoreStreamRelation, CoreStruct, CoreType, CoreVariant,
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
pub use identity::{ContentHash, FamilyClosure};
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
    ApplicationHead, Declaration, DeclarationHead, EnumDeclaration, EnumVariant, FamilyDeclaration,
    FamilyKey, FieldDeclaration, ImplBlock, ImplCatalog, ImplCompositionKey, ImplFact,
    ImplReference, ImportDeclaration, MethodParameter, MethodSignature, Name, NewtypeDeclaration,
    RelationDeclaration, RelationValue, Root, RootApplication, RustSurface, SchemaNode,
    SchemaNodeData, SchemaNodePair, SchemaNodeValue, StreamDeclaration, StreamRelation,
    StructDeclaration, StructFieldMap, SymbolPath, SymbolPathPosition, TableName, TypeDeclaration,
    TypeReference, Visibility,
};
pub use source::{
    SchemaSource, SchemaSourceArtifact, SourceDeclaration, SourceDeclarationValue,
    SourceDeclarations, SourceEnumBody, SourceFamilyBody, SourceField, SourceFieldIdentity,
    SourceFieldValue, SourceImplCatalog, SourceImplEntry, SourceImport, SourceImports,
    SourceMethodParameter, SourceMethodSignature, SourceNamespace, SourceNamespaceEntry,
    SourceReference, SourceRelation, SourceRelationValue, SourceRelations, SourceRootBody,
    SourceRootEnum, SourceStreamBody, SourceStructBody, SourceVariantName, SourceVariantPayload,
    SourceVariantSignature, StreamRelationKeyword,
};
pub use upgrade::{
    AddField, AddVariant, ChangeFieldType, DefaultValue, FieldMigration, MigrationSpec, SchemaEdit,
    SchemaEditApplication, SchemaEditReceipt, UpgradeObject, UpgradeReceipt,
};
pub use view::{
    DeclarationView, EnumView, FamilyView, FieldView, ImplBlockView, NewtypeView,
    ReferencedImplView, RootApplicationView, RootView, SchemaDeclaredType, StreamView, StructView,
    TrueSchema, TypeDeclarationView, VariantView,
};
