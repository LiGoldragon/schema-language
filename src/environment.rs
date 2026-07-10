use std::path::{Path, PathBuf};

use nota::{Document, SourcePosition, SourceSpan};

use crate::{
    ImportResolver, Name, SchemaEngine, SchemaError, SchemaModuleSource, SchemaPackage,
    SchemaSourceArtifact, TrueSchema, source::SchemaDocumentLayout,
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
        let Ok(layout) = SchemaDocumentLayout::from_document(document) else {
            return Self { blocks: Vec::new() };
        };
        Self {
            blocks: vec![
                SchemaRootBlockSummary::from_document_slot(
                    document,
                    layout.imports(),
                    SchemaRootBlockKind::Imports,
                ),
                SchemaRootBlockSummary::from_document_slot(
                    document,
                    layout.input(),
                    SchemaRootBlockKind::Input,
                ),
                SchemaRootBlockSummary::from_document_slot(
                    document,
                    layout.output(),
                    SchemaRootBlockKind::Output,
                ),
                SchemaRootBlockSummary::from_document_slot(
                    document,
                    layout.types(),
                    SchemaRootBlockKind::Types,
                ),
                SchemaRootBlockSummary::from_document_slot(
                    document,
                    layout.generics(),
                    SchemaRootBlockKind::Generics,
                ),
                SchemaRootBlockSummary::from_document_slot(
                    document,
                    layout.impls(),
                    SchemaRootBlockKind::Impls,
                ),
            ],
        }
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

    fn from_document_slot(
        document: &Document,
        slot: crate::source::SchemaDocumentSlot,
        kind: SchemaRootBlockKind,
    ) -> Self {
        let blocks = slot.blocks(document);
        let first = blocks
            .first()
            .expect("document layout slot always has a first block");
        let last = blocks
            .last()
            .expect("document layout slot always has a last block");
        Self {
            kind,
            range: SchemaSourceRange::from_span_bounds(first.source_span(), last.source_span()),
            node_type: SchemaNodeType::from_root_block_kind(kind),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchemaRootBlockKind {
    Imports,
    Input,
    Output,
    Types,
    Generics,
    Impls,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchemaNodeType {
    Module,
    Imports,
    InputRoot,
    OutputRoot,
    Types,
    Generics,
    Impls,
}

impl SchemaNodeType {
    fn from_root_block_kind(kind: SchemaRootBlockKind) -> Self {
        match kind {
            SchemaRootBlockKind::Imports => Self::Imports,
            SchemaRootBlockKind::Input => Self::InputRoot,
            SchemaRootBlockKind::Output => Self::OutputRoot,
            SchemaRootBlockKind::Types => Self::Types,
            SchemaRootBlockKind::Generics => Self::Generics,
            SchemaRootBlockKind::Impls => Self::Impls,
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

    fn from_span_bounds(first: SourceSpan, last: SourceSpan) -> Self {
        Self {
            start: SchemaSourcePosition::from(first.start),
            end: SchemaSourcePosition::from(last.end),
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
