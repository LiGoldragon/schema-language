use std::path::{Path, PathBuf};

use nota::{Delimiter, Document, SourcePosition, SourceSpan};

use crate::{
    ImportResolver, Name, SchemaEngine, SchemaError, SchemaModuleSource, SchemaPackage,
    SchemaSourceArtifact, TrueSchema,
};

pub struct SchemaEnvironment {
    package: SchemaPackage,
    engine: SchemaEngine,
    resolver: ImportResolver,
}

impl SchemaEnvironment {
    pub fn new(package: SchemaPackage) -> Self {
        Self {
            package,
            engine: SchemaEngine::default(),
            resolver: ImportResolver::new(),
        }
    }

    pub fn with_engine(mut self, engine: SchemaEngine) -> Self {
        self.engine = engine;
        self
    }

    pub fn with_resolver(mut self, resolver: ImportResolver) -> Self {
        self.resolver = resolver;
        self
    }

    pub fn package(&self) -> &SchemaPackage {
        &self.package
    }

    pub fn load(
        &self,
        manifest: &SchemaEnvironmentManifest,
    ) -> Result<SchemaEnvironmentResult, SchemaError> {
        let resolver = self.resolver.clone().with_package(self.package.clone());
        manifest
            .module_names()
            .iter()
            .map(|name| {
                let source = self.package.load_module(name.clone())?;
                SchemaEnvironmentModule::from_source(
                    source,
                    SchemaEnvironmentLowering::new(&self.engine, &resolver),
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map(SchemaEnvironmentResult::new)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaEnvironmentManifest {
    module_names: Vec<Name>,
}

impl SchemaEnvironmentManifest {
    pub fn new(module_names: Vec<Name>) -> Self {
        Self { module_names }
    }

    pub fn module_names(&self) -> &[Name] {
        &self.module_names
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaEnvironmentResult {
    modules: Vec<SchemaEnvironmentModule>,
}

impl SchemaEnvironmentResult {
    pub fn new(modules: Vec<SchemaEnvironmentModule>) -> Self {
        Self { modules }
    }

    pub fn modules(&self) -> &[SchemaEnvironmentModule] {
        &self.modules
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaEnvironmentModule {
    source: SchemaModuleSource,
    artifact: SchemaSourceArtifact,
    true_schema: TrueSchema,
    summary: SchemaSourceSummary,
}

impl SchemaEnvironmentModule {
    pub fn source(&self) -> &SchemaModuleSource {
        &self.source
    }

    pub fn artifact(&self) -> &SchemaSourceArtifact {
        &self.artifact
    }

    pub fn true_schema(&self) -> &TrueSchema {
        &self.true_schema
    }

    pub fn summary(&self) -> &SchemaSourceSummary {
        &self.summary
    }

    fn from_source(
        source: SchemaModuleSource,
        lowering: SchemaEnvironmentLowering<'_>,
    ) -> Result<Self, SchemaError> {
        let parsed = source.to_schema_source()?;
        let artifact = SchemaSourceArtifact::new(parsed);
        let true_schema = lowering.engine().lower_schema_source_with_resolver(
            artifact.source(),
            source.identity().clone(),
            lowering.resolver(),
        )?;
        let summary = SchemaSourceSummary::from_module_source(&source)?;
        Ok(Self {
            source,
            artifact,
            true_schema,
            summary,
        })
    }
}

#[derive(Clone, Copy)]
struct SchemaEnvironmentLowering<'environment> {
    engine: &'environment SchemaEngine,
    resolver: &'environment ImportResolver,
}

impl<'environment> SchemaEnvironmentLowering<'environment> {
    fn new(engine: &'environment SchemaEngine, resolver: &'environment ImportResolver) -> Self {
        Self { engine, resolver }
    }

    fn engine(&self) -> &'environment SchemaEngine {
        self.engine
    }

    fn resolver(&self) -> &'environment ImportResolver {
        self.resolver
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaSourceSummary {
    path: PathBuf,
    file_range: SchemaSourceRange,
    root_blocks: Vec<SchemaRootBlockSummary>,
    node_type_labels: Vec<SchemaNodeTypeLabel>,
}

impl SchemaSourceSummary {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file_range(&self) -> &SchemaSourceRange {
        &self.file_range
    }

    pub fn root_blocks(&self) -> &[SchemaRootBlockSummary] {
        &self.root_blocks
    }

    pub fn node_type_labels(&self) -> &[SchemaNodeTypeLabel] {
        &self.node_type_labels
    }

    fn from_module_source(source: &SchemaModuleSource) -> Result<Self, SchemaError> {
        let document = Document::parse(source.source())?;
        let root_blocks = SchemaRootBlockSummarySet::from_document(&document).into_blocks();
        let file_range = SchemaSourceRange::from_source_text(source.source());
        let node_type_labels =
            SchemaNodeTypeLabelSet::from_summary_parts(file_range.clone(), root_blocks.as_slice())
                .into_labels();
        Ok(Self {
            path: source.path().to_path_buf(),
            file_range,
            root_blocks,
            node_type_labels,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SchemaRootBlockSummarySet {
    blocks: Vec<SchemaRootBlockSummary>,
}

impl SchemaRootBlockSummarySet {
    fn from_document(document: &Document) -> Self {
        let first_is_imports = document
            .root_object_at(0)
            .is_some_and(|block| block.is_delimited_with(Delimiter::Brace));
        let mut blocks = Vec::new();
        if first_is_imports {
            blocks.extend(SchemaRootBlockSummary::from_document_block(
                document,
                0,
                SchemaRootBlockKind::Imports,
            ));
        }
        let input_index = if first_is_imports { 1 } else { 0 };
        blocks.extend(SchemaRootBlockSummary::from_document_block(
            document,
            input_index,
            SchemaRootBlockKind::Input,
        ));
        blocks.extend(SchemaRootBlockSummary::from_document_block(
            document,
            input_index + 1,
            SchemaRootBlockKind::Output,
        ));
        blocks.extend(SchemaRootBlockSummary::from_document_block(
            document,
            input_index + 2,
            SchemaRootBlockKind::Namespace,
        ));
        blocks.extend(SchemaRootBlockSummary::from_document_block(
            document,
            input_index + 3,
            SchemaRootBlockKind::Relations,
        ));
        Self { blocks }
    }

    fn into_blocks(self) -> Vec<SchemaRootBlockSummary> {
        self.blocks
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaRootBlockSummary {
    kind: SchemaRootBlockKind,
    range: SchemaSourceRange,
    node_type: SchemaNodeType,
}

impl SchemaRootBlockSummary {
    pub fn kind(&self) -> SchemaRootBlockKind {
        self.kind
    }

    pub fn range(&self) -> &SchemaSourceRange {
        &self.range
    }

    pub fn node_type(&self) -> SchemaNodeType {
        self.node_type
    }

    fn from_document_block(
        document: &Document,
        index: usize,
        kind: SchemaRootBlockKind,
    ) -> Option<Self> {
        let block = document.root_object_at(index)?;
        Some(Self {
            kind,
            range: SchemaSourceRange::from(block.source_span()),
            node_type: SchemaNodeType::from_root_block_kind(kind),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchemaRootBlockKind {
    Imports,
    Input,
    Output,
    Namespace,
    Relations,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchemaNodeType {
    Module,
    Imports,
    InputRoot,
    OutputRoot,
    Namespace,
    Relations,
}

impl SchemaNodeType {
    fn from_root_block_kind(kind: SchemaRootBlockKind) -> Self {
        match kind {
            SchemaRootBlockKind::Imports => Self::Imports,
            SchemaRootBlockKind::Input => Self::InputRoot,
            SchemaRootBlockKind::Output => Self::OutputRoot,
            SchemaRootBlockKind::Namespace => Self::Namespace,
            SchemaRootBlockKind::Relations => Self::Relations,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaNodeTypeLabel {
    node_type: SchemaNodeType,
    range: SchemaSourceRange,
}

impl SchemaNodeTypeLabel {
    pub fn node_type(&self) -> SchemaNodeType {
        self.node_type
    }

    pub fn range(&self) -> &SchemaSourceRange {
        &self.range
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SchemaNodeTypeLabelSet {
    labels: Vec<SchemaNodeTypeLabel>,
}

impl SchemaNodeTypeLabelSet {
    fn from_summary_parts(
        file_range: SchemaSourceRange,
        root_blocks: &[SchemaRootBlockSummary],
    ) -> Self {
        let mut labels = vec![SchemaNodeTypeLabel {
            node_type: SchemaNodeType::Module,
            range: file_range,
        }];
        labels.extend(root_blocks.iter().map(SchemaNodeTypeLabel::from));
        Self { labels }
    }

    fn into_labels(self) -> Vec<SchemaNodeTypeLabel> {
        self.labels
    }
}

impl From<&SchemaRootBlockSummary> for SchemaNodeTypeLabel {
    fn from(value: &SchemaRootBlockSummary) -> Self {
        Self {
            node_type: value.node_type(),
            range: value.range().clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaSourceRange {
    start: SchemaSourcePosition,
    end: SchemaSourcePosition,
}

impl SchemaSourceRange {
    pub fn start(&self) -> &SchemaSourcePosition {
        &self.start
    }

    pub fn end(&self) -> &SchemaSourcePosition {
        &self.end
    }

    fn from_source_text(source: &str) -> Self {
        Self {
            start: SchemaSourcePosition {
                byte: 0,
                line: 1,
                column: 1,
            },
            end: SchemaSourcePosition::from_source_text(source),
        }
    }
}

impl From<SourceSpan> for SchemaSourceRange {
    fn from(value: SourceSpan) -> Self {
        Self {
            start: SchemaSourcePosition::from(value.start),
            end: SchemaSourcePosition::from(value.end),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaSourcePosition {
    byte: usize,
    line: usize,
    column: usize,
}

impl SchemaSourcePosition {
    pub fn byte(&self) -> usize {
        self.byte
    }

    pub fn line(&self) -> usize {
        self.line
    }

    pub fn column(&self) -> usize {
        self.column
    }

    fn from_source_text(source: &str) -> Self {
        source
            .chars()
            .fold(Self::new_document_start(), |position, character| {
                position.after_character(character)
            })
    }

    fn new_document_start() -> Self {
        Self {
            byte: 0,
            line: 1,
            column: 1,
        }
    }

    fn after_character(mut self, character: char) -> Self {
        self.byte += character.len_utf8();
        if character == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        self
    }
}

impl From<SourcePosition> for SchemaSourcePosition {
    fn from(value: SourcePosition) -> Self {
        Self {
            byte: value.byte_offset,
            line: value.line,
            column: value.column,
        }
    }
}
