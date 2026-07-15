use std::{
    fs,
    path::{Path, PathBuf},
};

use nota::{
    Block, CaptureName, Delimiter, Document, DottedExpectation, MacroCandidate, NotaBody,
    NotaDecodeError, NotaEncode, NotaString, StructuralMacroError, StructuralMacroNode,
    StructuralVariant,
};

use crate::{
    Declaration, DeclarationHead, EnumDeclaration, EnumVariant, FieldDeclaration, ImplBlock,
    ImplCatalog, ImplReference, ImportDeclaration, MethodParameter, MethodSignature,
    MultiTypeReferenceProjection, Name, NewtypeDeclaration, ResolvedImport, Root, RootApplication,
    SchemaEngine, SchemaError, SchemaIdentity, SingleTypeReferenceProjection, StructDeclaration,
    TypeDeclaration, TypeReference, ValueReferenceProjection,
    macros::{BlockDebug, SchemaBlockExt},
    schema::SchemaTree,
};

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SchemaSource {
    imports: SourceImports,
    input: SourceRootEnum,
    output: SourceRootEnum,
    types: SourceTypes,
    generics: SourceGenerics,
    impls: SourceImpls,
}

impl SchemaSource {
    pub fn from_schema_text(source: &str) -> Result<Self, SchemaError> {
        let document = Document::parse(source)?;
        Self::from_document(&document)
    }

    pub fn from_document(document: &Document) -> Result<Self, SchemaError> {
        let layout = SchemaDocumentLayout::from_document(document)?;

        Ok(Self {
            imports: SourceImports::from_block(layout.imports().block(document))?,
            input: SourceRootEnum::from_blocks(
                Name::new("Input"),
                layout.input().blocks(document),
            )?,
            output: SourceRootEnum::from_blocks(
                Name::new("Output"),
                layout.output().blocks(document),
            )?,
            types: SourceTypes::from_block(layout.types().block(document))?,
            generics: SourceGenerics::from_block(layout.generics().block(document))?,
            impls: SourceImpls::from_block(layout.impls().block(document))?,
        })
    }

    pub fn imports(&self) -> &SourceImports {
        &self.imports
    }

    pub fn input(&self) -> &SourceRootEnum {
        &self.input
    }

    pub fn output(&self) -> &SourceRootEnum {
        &self.output
    }

    pub fn types(&self) -> &SourceTypes {
        &self.types
    }

    pub fn generics(&self) -> &SourceGenerics {
        &self.generics
    }

    pub fn impls(&self) -> &SourceImpls {
        &self.impls
    }

    pub fn to_schema_text(&self) -> String {
        [
            self.imports.to_schema_text(),
            self.input.body().to_schema_text(),
            self.output.body().to_schema_text(),
            self.types.to_schema_text(),
            self.generics.to_schema_text(),
            self.impls.to_schema_text(),
        ]
        .join("\n")
    }

    pub fn from_binary_bytes(bytes: &[u8]) -> Result<Self, SchemaError> {
        rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes).map_err(|_| SchemaError::ArchiveDecode)
    }

    pub fn to_binary_bytes(&self) -> Result<Vec<u8>, SchemaError> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .map(|bytes| bytes.to_vec())
            .map_err(|_| SchemaError::ArchiveEncode)
    }

    pub fn lower(
        &self,
        engine: &SchemaEngine,
        identity: SchemaIdentity,
    ) -> Result<crate::TrueSchema, SchemaError> {
        engine.lower_schema_source(self, identity)
    }

    pub(crate) fn to_true_schema(
        &self,
        identity: SchemaIdentity,
        imports: Vec<ImportDeclaration>,
        resolved_imports: Vec<ResolvedImport>,
    ) -> Result<crate::TrueSchema, SchemaError> {
        let resolver = SourceTypeResolver::from_source(self);
        let mut namespace = SourceLoweredNamespace::from_source(
            &self.types,
            &self.generics,
            &self.impls,
            &resolver,
        )?;
        namespace.push_public_declarations(self.input.public_inline_declarations(&resolver)?)?;
        namespace.push_public_declarations(self.output.public_inline_declarations(&resolver)?)?;
        let input = self.input.to_root(&namespace)?;
        let output = self.output.to_root(&namespace)?;
        let impl_blocks = namespace.impl_blocks().to_vec();
        // The name-bearing tree exists only transiently: it is validated and
        // immediately decomposed into the split (CoreSchema, NameTable) model
        // the returned view holds.
        let tree = SchemaTree::new(
            identity,
            imports,
            resolved_imports,
            input,
            output,
            namespace.into_declarations(),
        )
        .with_impl_blocks(impl_blocks)
        .product_components_verified()
        .and_then(SchemaTree::arities_verified)
        .and_then(SchemaTree::impls_verified)?;
        crate::TrueSchema::from_tree(&tree, &crate::NameTable::empty())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SchemaDocumentLayout {
    imports: SchemaDocumentSlot,
    input: SchemaDocumentSlot,
    output: SchemaDocumentSlot,
    types: SchemaDocumentSlot,
    generics: SchemaDocumentSlot,
    impls: SchemaDocumentSlot,
}

impl SchemaDocumentLayout {
    pub(crate) fn from_document(document: &Document) -> Result<Self, SchemaError> {
        let objects = document.root_objects();
        let mut cursor = 0;
        let imports = SchemaDocumentSlot::consume_delimited(
            objects,
            &mut cursor,
            Delimiter::Brace,
            "imports",
        )?;
        let input = SchemaDocumentSlot::consume_root_body(objects, &mut cursor, "input")?;
        let output = SchemaDocumentSlot::consume_root_body(objects, &mut cursor, "output")?;
        let types =
            SchemaDocumentSlot::consume_delimited(objects, &mut cursor, Delimiter::Brace, "types")?;
        let generics = SchemaDocumentSlot::consume_delimited(
            objects,
            &mut cursor,
            Delimiter::Brace,
            "generics",
        )?;
        let impls =
            SchemaDocumentSlot::consume_delimited(objects, &mut cursor, Delimiter::Brace, "impls")?;
        if cursor != objects.len() {
            return Err(SchemaError::ExpectedRootObjectCount {
                expected: "6 root slots (imports input output types generics impls)",
                found: document.holds_root_objects(),
            });
        }
        Ok(Self {
            imports,
            input,
            output,
            types,
            generics,
            impls,
        })
    }

    pub(crate) fn imports(&self) -> SchemaDocumentSlot {
        self.imports
    }

    pub(crate) fn input(&self) -> SchemaDocumentSlot {
        self.input
    }

    pub(crate) fn output(&self) -> SchemaDocumentSlot {
        self.output
    }

    pub(crate) fn types(&self) -> SchemaDocumentSlot {
        self.types
    }

    pub(crate) fn generics(&self) -> SchemaDocumentSlot {
        self.generics
    }

    pub(crate) fn impls(&self) -> SchemaDocumentSlot {
        self.impls
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SchemaDocumentSlot {
    start: usize,
    width: usize,
}

impl SchemaDocumentSlot {
    fn new(start: usize, width: usize) -> Self {
        Self { start, width }
    }

    fn consume_delimited(
        objects: &[Block],
        cursor: &mut usize,
        delimiter: Delimiter,
        slot_name: &'static str,
    ) -> Result<Self, SchemaError> {
        let start = *cursor;
        let Some(block) = objects.get(start) else {
            return Err(SchemaError::ExpectedRootObjectCount {
                expected: "6 root slots (imports input output types generics impls)",
                found: objects.len(),
            });
        };
        if !block.is_delimited_with(delimiter) {
            return Err(SchemaError::ExpectedSyntaxDeclaration {
                found: format!(
                    "{slot_name} root slot {}, expected {} block",
                    block.reemit_fallback(),
                    delimiter.description()
                ),
            });
        }
        *cursor += 1;
        Ok(Self::new(start, 1))
    }

    fn consume_root_body(
        objects: &[Block],
        cursor: &mut usize,
        slot_name: &'static str,
    ) -> Result<Self, SchemaError> {
        let start = *cursor;
        if objects.get(start).is_none() {
            return Err(SchemaError::ExpectedRootObjectCount {
                expected: "6 root slots (imports input output types generics impls)",
                found: objects.len(),
            });
        }
        let width = SourceReference::block_span_width_at(objects, start)?;
        *cursor += width;
        if *cursor > objects.len() {
            return Err(SchemaError::ExpectedSyntaxReferenceArity {
                form: slot_name,
                expected: "a complete root value",
                found: objects.len().saturating_sub(start),
            });
        }
        Ok(Self::new(start, width))
    }

    pub(crate) fn blocks<'document>(&self, document: &'document Document) -> &'document [Block] {
        &document.root_objects()[self.start..self.start + self.width]
    }

    pub(crate) fn block<'document>(&self, document: &'document Document) -> &'document Block {
        self.blocks(document)
            .first()
            .expect("document layout slot always has one block")
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SchemaSourceArtifact(SchemaSource);

impl SchemaSourceArtifact {
    pub fn new(source: SchemaSource) -> Self {
        Self(source)
    }

    pub fn source(&self) -> &SchemaSource {
        &self.0
    }

    pub fn into_source(self) -> SchemaSource {
        self.0
    }

    pub fn from_schema_text(source: &str) -> Result<Self, SchemaError> {
        SchemaSource::from_schema_text(source).map(Self::new)
    }

    pub fn to_schema_text(&self) -> String {
        self.0.to_schema_text()
    }

    pub fn from_binary_bytes(bytes: &[u8]) -> Result<Self, SchemaError> {
        SchemaSource::from_binary_bytes(bytes).map(Self::new)
    }

    pub fn to_binary_bytes(&self) -> Result<Vec<u8>, SchemaError> {
        self.0.to_binary_bytes()
    }

    pub fn read_schema_file(path: impl AsRef<Path>) -> Result<Self, SchemaError> {
        let artifact_path = SchemaSourceArtifactPath::new(path.as_ref());
        let source = fs::read_to_string(artifact_path.path())
            .map_err(|error| artifact_path.io_error(error))?;
        Self::from_schema_text(&source)
    }

    pub fn write_schema_file(&self, path: impl AsRef<Path>) -> Result<(), SchemaError> {
        let artifact_path = SchemaSourceArtifactPath::new(path.as_ref());
        fs::write(artifact_path.path(), self.to_schema_text())
            .map_err(|error| artifact_path.io_error(error))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SchemaSourceArtifactPath(PathBuf);

impl SchemaSourceArtifactPath {
    fn new(path: &Path) -> Self {
        Self(path.to_path_buf())
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn io_error(&self, error: std::io::Error) -> SchemaError {
        SchemaError::Io {
            path: self.0.display().to_string(),
            reason: error.to_string(),
        }
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub struct SourceDeclarations {
    #[rkyv(omit_bounds)]
    declarations: Vec<SourceDeclaration>,
}

impl SourceDeclarations {
    pub fn new(declarations: Vec<SourceDeclaration>) -> Self {
        Self { declarations }
    }

    pub fn from_schema_text(source: &str) -> Result<Self, SchemaError> {
        let document = Document::parse(source)?;
        Self::from_document(&document)
    }

    pub fn from_document(document: &Document) -> Result<Self, SchemaError> {
        document
            .root_objects()
            .iter()
            .map(SourceDeclaration::from_block)
            .collect::<Result<Vec<_>, _>>()
            .map(Self::new)
    }

    pub fn declarations(&self) -> &[SourceDeclaration] {
        &self.declarations
    }

    pub fn to_schema_text(&self) -> String {
        self.declarations
            .iter()
            .map(SourceDeclaration::to_schema_text)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub struct SourceDeclaration {
    name: Name,
    #[rkyv(omit_bounds)]
    value: Option<SourceDeclarationValue>,
}

impl SourceDeclaration {
    pub fn new(name: Name, value: Option<SourceDeclarationValue>) -> Self {
        Self { name, value }
    }

    pub fn from_schema_text(source: &str) -> Result<Self, SchemaError> {
        let document = Document::parse(source)?;
        if document.holds_root_objects() != 1 {
            return Err(SchemaError::ExpectedRootObjectCount {
                expected: "one re-headed schema declaration",
                found: document.holds_root_objects(),
            });
        }
        Self::from_block(
            document
                .root_object_at(0)
                .expect("checked root object count"),
        )
    }

    pub fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::Parenthesis, "source declaration")?;
        let objects = body.root_objects();
        let Some((head, tail)) = objects.split_first() else {
            return Err(SchemaError::ExpectedSyntaxDeclaration {
                found: "empty source declaration".to_owned(),
            });
        };
        let (name, _parameters) = DeclarationHead::from_block(head)?.into_parts();
        let value = match tail {
            [] => None,
            body => Some(SourceDeclarationValue::from_blocks(body)?),
        };
        Ok(Self::new(name, value))
    }

    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn value(&self) -> Option<&SourceDeclarationValue> {
        self.value.as_ref()
    }

    pub fn to_schema_text(&self) -> String {
        match &self.value {
            Some(value) => {
                Delimiter::Parenthesis.wrap([self.name.to_nota(), value.to_schema_text()])
            }
            None => Delimiter::Parenthesis.wrap([self.name.to_nota()]),
        }
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SourceImports {
    entries: Vec<SourceImport>,
}

impl SourceImports {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn entries(&self) -> &[SourceImport] {
        &self.entries
    }

    pub(crate) fn to_schema_imports(&self) -> Result<Vec<ImportDeclaration>, SchemaError> {
        self.entries
            .iter()
            .flat_map(SourceImport::to_schema_imports)
            .collect()
    }

    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::Brace, "source imports")?;
        let objects = body.root_objects();
        let mut entries = Vec::new();
        let mut index = 0;
        while index < objects.len() {
            entries.push(SourceImport::from_blocks_at(objects, &mut index)?);
        }
        Ok(Self { entries })
    }

    fn to_schema_text(&self) -> String {
        if self.entries.is_empty() {
            return "{}".to_owned();
        }
        let entries = self
            .entries
            .iter()
            .map(|entry| format!("  {}", entry.to_schema_text()))
            .collect::<Vec<_>>();
        format!("{{\n{}\n}}", entries.join("\n"))
    }
}

/// One import entry: a lowercase dotted source path and the one-or-many
/// capitalized targets imported from it. There is no alias — an imported
/// declaration keeps its own name (see ARCHITECTURE "Imports entry syntax
/// carries no alias"). `path.to.Object` imports the single target `Object`;
/// `path.to.[X Y Z]` imports several targets from the same path.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SourceImport {
    path: Vec<Name>,
    targets: Vec<Name>,
}

impl SourceImport {
    pub fn path(&self) -> &[Name] {
        &self.path
    }

    pub fn targets(&self) -> &[Name] {
        &self.targets
    }

    /// Read one import entry from the right-associated application tree. A
    /// dotted atom chain carries one target (`path.to.Object`); an application
    /// whose payload is a target vector carries several (`path.to.[X Y Z]`).
    /// This consumes one structural block because the raw parser owns the dot.
    fn from_blocks_at(blocks: &[Block], index: &mut usize) -> Result<Self, SchemaError> {
        let block = blocks
            .get(*index)
            .ok_or_else(|| SchemaError::MalformedImportSource {
                found: String::new(),
            })?;
        let mut path = Vec::new();
        let mut current = block;
        loop {
            let Some((head, payload)) = current.as_application() else {
                if current.as_delimited(Delimiter::SquareBracket).is_some() {
                    let mut target_index = 0;
                    let targets =
                        Self::target_vector(std::slice::from_ref(current), &mut target_index)?;
                    *index += 1;
                    return Self::new(path, targets);
                }
                let terminal =
                    current
                        .dotted_text()
                        .ok_or_else(|| SchemaError::MalformedImportSource {
                            found: block.reemit_fallback(),
                        })?;
                path.extend(terminal.split('.').map(Name::new));
                let target = path
                    .pop()
                    .ok_or_else(|| SchemaError::MalformedImportSource {
                        found: block.reemit_fallback(),
                    })?;
                *index += 1;
                return Self::new(path, vec![target]);
            };
            let head_text =
                head.dotted_text()
                    .ok_or_else(|| SchemaError::MalformedImportSource {
                        found: block.reemit_fallback(),
                    })?;
            path.extend(head_text.split('.').map(Name::new));
            current = payload;
        }
    }

    /// Read the `[X Y Z]` bracket that follows a trailing-dot import path,
    /// consuming it and collecting its capitalized target atoms.
    fn target_vector(blocks: &[Block], index: &mut usize) -> Result<Vec<Name>, SchemaError> {
        let block = blocks
            .get(*index)
            .ok_or_else(|| SchemaError::MalformedImportSource {
                found: "import path ending in a dot with no target vector".to_owned(),
            })?;
        let Block::Delimited {
            delimiter: Delimiter::SquareBracket,
            root_objects,
            ..
        } = block
        else {
            return Err(SchemaError::MalformedImportSource {
                found: block.reemit_fallback(),
            });
        };
        let mut targets = Vec::new();
        for object in root_objects {
            if object.is_application() {
                return Err(SchemaError::MalformedImportTarget {
                    target: object
                        .dotted_text()
                        .unwrap_or_else(|| object.reemit_fallback()),
                });
            }
            let atom = object
                .atom()
                .ok_or_else(|| SchemaError::MalformedImportSource {
                    found: object.reemit_fallback(),
                })?;
            targets.push(Name::new(atom.text()));
        }
        *index += 1;
        Ok(targets)
    }

    fn new(path: Vec<Name>, targets: Vec<Name>) -> Result<Self, SchemaError> {
        if path.is_empty() || targets.is_empty() {
            return Err(SchemaError::MalformedImportSource {
                found: Self::joined_path(&path, &targets),
            });
        }
        for segment in &path {
            if !SourceIdentifierCase::new(segment).is_namespace() {
                return Err(SchemaError::MalformedImportSource {
                    found: segment.to_nota(),
                });
            }
        }
        for target in &targets {
            if !SourceIdentifierCase::new(target).is_simple_type() {
                return Err(SchemaError::MalformedImportTarget {
                    target: target.to_nota(),
                });
            }
        }
        Ok(Self { path, targets })
    }

    /// The single-colon namespace source name the resolver consumes for one
    /// target: the dotted path segments and the target joined by `:`, the shape
    /// `ImportSource` splits back into crate, module, and type.
    fn colon_source(&self, target: &Name) -> Name {
        let mut segments: Vec<&str> = self.path.iter().map(Name::as_str).collect();
        segments.push(target.as_str());
        Name::new(segments.join(":"))
    }

    fn joined_path(path: &[Name], targets: &[Name]) -> String {
        let dotted = path.iter().map(Name::as_str).collect::<Vec<_>>().join(".");
        match targets {
            [single] => format!("{dotted}.{}", single.as_str()),
            _ => format!(
                "{dotted}.[{}]",
                targets
                    .iter()
                    .map(Name::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        }
    }

    fn to_schema_text(&self) -> String {
        Self::joined_path(&self.path, &self.targets)
    }

    fn to_schema_imports(&self) -> Vec<Result<ImportDeclaration, SchemaError>> {
        self.targets
            .iter()
            .map(|target| {
                Ok(ImportDeclaration {
                    local_name: target.clone(),
                    source: TypeReference::from_name(self.colon_source(target)),
                })
            })
            .collect()
    }
}

/// A root Input/Output position in the source codec. The body is a typed
/// sum mirroring the semantic [`Root`]: the enum-body form `[Variant …]`
/// or the dotted application form `Head.(Arg …)`. The name is the position name
/// (`Input` / `Output`) — for an enum root it also names the lowered enum
/// declaration; for an application root it is only the position identity.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SourceRootEnum {
    name: Name,
    body: SourceRootBody,
}

impl SourceRootEnum {
    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn body(&self) -> &SourceRootBody {
        &self.body
    }

    fn from_blocks(name: Name, blocks: &[Block]) -> Result<Self, SchemaError> {
        Ok(Self {
            name,
            body: SourceRootBody::from_blocks(blocks)?,
        })
    }

    fn public_inline_declarations(
        &self,
        resolver: &SourceTypeResolver,
    ) -> Result<Vec<Declaration>, SchemaError> {
        self.body.public_inline_declarations(resolver)
    }

    fn to_root(&self, namespace: &SourceLoweredNamespace) -> Result<Root, SchemaError> {
        self.body.to_root(self.name.clone(), namespace)
    }
}

/// The two shapes a source-codec root body can take, mirroring the
/// semantic [`Root`] sum. The application form holds a [`SourceReference`]
/// known to be its `Application` variant; lowering projects it through
/// `SourceReference::to_type_reference`, the same conversion a
/// field-position source reference takes.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub enum SourceRootBody {
    Enum(#[rkyv(omit_bounds)] SourceEnumBody),
    Application(#[rkyv(omit_bounds)] SourceReference),
}

impl SourceRootBody {
    /// The enum body when this root is the enum-body form; `None` for an
    /// application root.
    pub fn as_enum(&self) -> Option<&SourceEnumBody> {
        match self {
            Self::Enum(body) => Some(body),
            Self::Application(_) => None,
        }
    }

    /// The applied reference when this root is the application form; `None`
    /// for an enum root. It is the `SourceReference::Application` variant by
    /// construction.
    pub fn as_application(&self) -> Option<&SourceReference> {
        match self {
            Self::Application(reference) => Some(reference),
            Self::Enum(_) => None,
        }
    }

    fn from_blocks(blocks: &[Block]) -> Result<Self, SchemaError> {
        if let [block] = blocks
            && block.is_delimited_with(Delimiter::SquareBracket)
        {
            return Ok(Self::Enum(SourceEnumBody::from_block(block)?));
        }
        let mut cursor = 0;
        let reference = SourceReference::from_blocks_at(blocks, &mut cursor)?;
        if cursor != blocks.len() {
            return Err(SchemaError::ExpectedRootApplication {
                position: "root",
                found: blocks
                    .iter()
                    .map(Block::reemit_fallback)
                    .collect::<Vec<_>>()
                    .join(" "),
            });
        }
        let SourceReference::Application { .. } = &reference else {
            return Err(SchemaError::ExpectedRootApplication {
                position: "root",
                found: reference.to_schema_text(),
            });
        };
        Ok(Self::Application(reference))
    }

    pub fn to_schema_text(&self) -> String {
        match self {
            Self::Enum(body) => body.to_schema_text(),
            Self::Application(reference) => reference.to_schema_text(),
        }
    }

    fn public_inline_declarations(
        &self,
        resolver: &SourceTypeResolver,
    ) -> Result<Vec<Declaration>, SchemaError> {
        match self {
            Self::Enum(body) => body.public_inline_declarations(resolver),
            // An application root introduces no inline declarations — its
            // head and arguments are references to names declared elsewhere.
            Self::Application(_) => Ok(Vec::new()),
        }
    }

    pub fn inline_declaration_names(&self) -> Vec<Name> {
        match self {
            Self::Enum(body) => body.inline_declaration_names(),
            Self::Application(_) => Vec::new(),
        }
    }

    pub fn public_inline_field_declaration_names(&self) -> Vec<Name> {
        match self {
            Self::Enum(body) => body.public_inline_field_declaration_names(),
            Self::Application(_) => Vec::new(),
        }
    }

    fn to_root(&self, name: Name, namespace: &SourceLoweredNamespace) -> Result<Root, SchemaError> {
        match self {
            Self::Enum(body) => body.to_schema_enum(name, namespace, None).map(Root::Enum),
            Self::Application(reference) => {
                let TypeReference::Application { head, arguments } = reference.to_type_reference()
                else {
                    return Err(SchemaError::ExpectedRootApplication {
                        position: "root",
                        found: reference.to_schema_text(),
                    });
                };
                Ok(Root::application(RootApplication::new(
                    name, head, arguments,
                )))
            }
        }
    }
}

/// The `types` per-kind block: a brace of dotted `TypeName.Definition` entries,
/// each keyed by a capitalized type name (see ARCHITECTURE "Per-kind
/// declaration blocks"). It holds only type declarations — no generics, no
/// impls, and no retired nested lowercase sub-namespace.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SourceTypes {
    entries: Vec<SourceTypeEntry>,
}

impl SourceTypes {
    pub fn entries(&self) -> &[SourceTypeEntry] {
        &self.entries
    }

    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::Brace, "source types")?;
        let objects = body.root_objects();
        let mut entries = Vec::new();
        let mut cursor = 0;
        while cursor < objects.len() {
            let entry = SourceKindEntry::read(objects, cursor)?;
            if entry.value_blocks().is_empty() {
                return Err(SchemaError::ExpectedSyntaxDeclaration {
                    found: format!("type {} with no definition", entry.key().to_nota()),
                });
            }
            let width = SourceDeclarationValue::block_span_width_at(entry.value_blocks(), 0)?;
            let value = SourceDeclarationValue::from_blocks(&entry.value_blocks()[..width])?;
            cursor += entry.advance(width);
            entries.push(SourceTypeEntry {
                name: entry.into_key(),
                value,
            });
        }
        Ok(Self { entries })
    }

    fn to_schema_text(&self) -> String {
        SourceKindBlockText::new(self.entries.iter().map(SourceTypeEntry::to_schema_text)).text()
    }

    fn type_declaration_names(&self) -> Vec<Name> {
        self.entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect()
    }
}

/// One `types` entry: a capitalized `TypeName` and its declaration definition.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub struct SourceTypeEntry {
    name: Name,
    #[rkyv(omit_bounds)]
    value: SourceDeclarationValue,
}

impl SourceTypeEntry {
    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn value(&self) -> &SourceDeclarationValue {
        &self.value
    }

    fn to_schema_text(&self) -> String {
        format!("{}.{}", self.name.to_nota(), self.value.to_schema_text())
    }

    fn to_declaration_group(
        &self,
        resolver: &SourceTypeResolver,
    ) -> Result<SourceDeclarationGroup, SchemaError> {
        self.value
            .to_namespace_declaration_group(self.name.clone(), resolver, None)
    }
}

/// The `generics` per-kind block: a brace of dotted
/// `GenericName.((Params …) Body)` entries, each a capitalized generic name
/// carrying its binder group and body (see ARCHITECTURE "Target root-slot
/// layout of the per-kind blocks").
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SourceGenerics {
    entries: Vec<SourceGenericEntry>,
}

impl SourceGenerics {
    pub fn entries(&self) -> &[SourceGenericEntry] {
        &self.entries
    }

    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::Brace, "source generics")?;
        let objects = body.root_objects();
        let mut entries = Vec::new();
        let mut cursor = 0;
        while cursor < objects.len() {
            let entry = SourceKindEntry::read(objects, cursor)?;
            let value_block = entry.value_blocks().first().ok_or_else(|| {
                SchemaError::ExpectedSyntaxDeclaration {
                    found: format!("generic {} with no binder group", entry.key().to_nota()),
                }
            })?;
            let generic = SourceGenericEntry::from_key_and_block(entry.key().clone(), value_block)?;
            cursor += entry.advance(1);
            entries.push(generic);
        }
        Ok(Self { entries })
    }

    fn to_schema_text(&self) -> String {
        SourceKindBlockText::new(self.entries.iter().map(SourceGenericEntry::to_schema_text)).text()
    }

    fn type_declaration_names(&self) -> Vec<Name> {
        self.entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect()
    }
}

/// One `generics` entry: a capitalized `GenericName`, its ordered binder
/// parameters, and the body they parameterize.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub struct SourceGenericEntry {
    name: Name,
    parameters: Vec<Name>,
    #[rkyv(omit_bounds)]
    value: SourceDeclarationValue,
}

impl SourceGenericEntry {
    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn parameters(&self) -> &[Name] {
        &self.parameters
    }

    pub fn value(&self) -> &SourceDeclarationValue {
        &self.value
    }

    /// Decode the `((Params …) Body)` value of a generic entry: the leading
    /// group is the binder parameter list, the trailing blocks are the body
    /// read as an ordinary declaration value.
    fn from_key_and_block(name: Name, block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(
            block,
            Delimiter::Parenthesis,
            "generic binder group and body",
        )?;
        let objects = body.root_objects();
        let Some((binder_block, body_blocks)) = objects.split_first() else {
            return Err(SchemaError::ExpectedSyntaxReferenceArity {
                form: "generic definition GenericName.((Params …) Body)",
                expected: "a binder group and a body",
                found: 0,
            });
        };
        let parameters = Self::read_parameters(&name, binder_block)?;
        if body_blocks.is_empty() {
            return Err(SchemaError::ExpectedSyntaxDeclaration {
                found: format!("generic {} with no body", name.to_nota()),
            });
        }
        let value = SourceDeclarationValue::from_blocks(body_blocks)?;
        Ok(Self {
            name,
            parameters,
            value,
        })
    }

    /// Read the `(Params …)` binder group into its ordered type-parameter
    /// names, rejecting a lowercase binder and a duplicate binder.
    fn read_parameters(name: &Name, block: &Block) -> Result<Vec<Name>, SchemaError> {
        let body =
            NotaBody::from_delimited(block, Delimiter::Parenthesis, "generic parameter binders")?;
        let mut parameters = Vec::new();
        for object in body.root_objects() {
            let parameter = SourceAtom::from_block(object)?.into_name()?;
            if !SourceIdentifierCase::new(&parameter).is_type() {
                return Err(SchemaError::ExpectedTypeParameterName {
                    declaration: name.as_str().to_owned(),
                    found: parameter.to_nota(),
                });
            }
            if parameters.iter().any(|existing| existing == &parameter) {
                return Err(SchemaError::DuplicateTypeParameter {
                    declaration: name.as_str().to_owned(),
                    parameter: parameter.as_str().to_owned(),
                });
            }
            parameters.push(parameter);
        }
        Ok(parameters)
    }

    fn to_schema_text(&self) -> String {
        let binders = self
            .parameters
            .iter()
            .map(Name::to_nota)
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            "{}.(({}) {})",
            self.name.to_nota(),
            binders,
            self.value.to_schema_text()
        )
    }

    fn to_declaration_group(
        &self,
        resolver: &SourceTypeResolver,
    ) -> Result<SourceDeclarationGroup, SchemaError> {
        self.value
            .to_namespace_declaration_group(self.name.clone(), resolver, None)
            .map(|group| group.with_parameters(self.parameters.clone()))
    }
}

/// The `impls` per-kind block: a brace of dotted `TypeName.[ … ]` entries, each
/// a capitalized type name carrying a square-bracket catalog of impl entries
/// (see ARCHITECTURE "Target root-slot layout of the per-kind blocks"). Every
/// entry lowers to a standalone [`ImplBlock`] keyed by its `TypeName`; the
/// old fused-versus-standalone distinction is gone — impls always live in this
/// block, keyed by the type they target.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SourceImpls {
    entries: Vec<SourceImplsEntry>,
}

impl SourceImpls {
    pub fn entries(&self) -> &[SourceImplsEntry] {
        &self.entries
    }

    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::Brace, "source impls")?;
        let objects = body.root_objects();
        let mut entries = Vec::new();
        let mut cursor = 0;
        while cursor < objects.len() {
            let entry = SourceKindEntry::read(objects, cursor)?;
            let catalog_block = entry.value_blocks().first().ok_or_else(|| {
                SchemaError::ExpectedSyntaxDeclaration {
                    found: format!("impls entry {} with no catalog", entry.key().to_nota()),
                }
            })?;
            let catalog = SourceImplCatalog::from_block(catalog_block)?;
            cursor += entry.advance(1);
            entries.push(SourceImplsEntry {
                target: entry.into_key(),
                catalog,
            });
        }
        Ok(Self { entries })
    }

    fn to_schema_text(&self) -> String {
        SourceKindBlockText::new(self.entries.iter().map(SourceImplsEntry::to_schema_text)).text()
    }
}

/// One `impls` entry: the capitalized `TypeName` it targets and the
/// square-bracket impl catalog attached to it.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub struct SourceImplsEntry {
    target: Name,
    #[rkyv(omit_bounds)]
    catalog: SourceImplCatalog,
}

impl SourceImplsEntry {
    pub fn target(&self) -> &Name {
        &self.target
    }

    pub fn catalog(&self) -> &SourceImplCatalog {
        &self.catalog
    }

    fn to_schema_text(&self) -> String {
        format!(
            "{}.{}",
            self.target.to_nota(),
            self.catalog.to_schema_text()
        )
    }

    /// Lower this entry to a standalone [`ImplBlock`] keyed by its target type
    /// name. Every impls-block entry lowers the same way; the target must name
    /// a type declared in the `types` (or `generics`) block, verified by
    /// `SchemaTree::impls_verified`.
    fn to_impl_block(&self, resolver: &SourceTypeResolver) -> ImplBlock {
        ImplBlock::new(self.target.clone(), self.catalog.lower(resolver, None))
    }
}

/// One capitalized dotted entry read from a per-kind block body under the
/// shared CAPITALIZED dotted expectation. The key is split off the leading
/// atom's first top-level dot (the shared NOTA primitive
/// [`nota::Atom::split_at_first_dot`]); the value block sequence is the inline
/// remainder atom — when the key atom carries text past its dot — followed by
/// the blocks after the key atom. The three per-kind readers share this split
/// and differ only in how they consume the value front, so the old special
/// cases dissolve into one uniform walk.
struct SourceKindEntry {
    key: Name,
    value_blocks: Vec<Block>,
    inline_remainder: bool,
}

impl SourceKindEntry {
    fn read(objects: &[Block], cursor: usize) -> Result<Self, SchemaError> {
        let block = objects
            .get(cursor)
            .ok_or_else(|| SchemaError::ExpectedSyntaxDeclaration {
                found: "missing per-kind declaration entry".to_owned(),
            })?;
        let (head, payload) =
            block
                .as_application()
                .ok_or_else(|| SchemaError::ExpectedSyntaxDeclaration {
                    found: format!("undotted declaration key {}", block.reemit_fallback()),
                })?;
        let key = head.schema_name()?;
        // The key names a type/generic: its local part must be PascalCase. A
        // colon-qualified name (`schema:spirit:Topic`) is judged by its final
        // segment, so a namespaced type key is accepted while the retired
        // lowercase nested-namespace key (`router:routed_object`) stays rejected.
        if !key.qualifies_as_pascal_case() {
            return Err(SchemaError::ExpectedSyntaxDeclaration {
                found: format!("uncapitalized declaration key {}", key.to_nota()),
            });
        }
        let inline_remainder = true;
        let mut value_blocks = vec![payload.clone()];
        value_blocks.extend(objects[cursor + 1..].iter().cloned());
        Ok(Self {
            key,
            value_blocks,
            inline_remainder,
        })
    }

    fn key(&self) -> &Name {
        &self.key
    }

    fn into_key(self) -> Name {
        self.key
    }

    fn value_blocks(&self) -> &[Block] {
        &self.value_blocks
    }

    /// The original-cursor advance after the value spanned `value_width`
    /// reconstructed value blocks: the key atom (1) plus the following blocks
    /// the value consumed. The inline remainder atom is synthesized from the
    /// key atom, so it never advances the original cursor.
    fn advance(&self, value_width: usize) -> usize {
        1 + value_width - usize::from(self.inline_remainder)
    }
}

/// The brace-block text projection shared by the three per-kind blocks: an
/// empty block re-emits as `{}`, a non-empty block as one indented entry per
/// line inside `{ … }`.
struct SourceKindBlockText {
    entries: Vec<String>,
}

impl SourceKindBlockText {
    fn new(entries: impl IntoIterator<Item = String>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    fn text(&self) -> String {
        if self.entries.is_empty() {
            return "{}".to_owned();
        }
        let entries = self
            .entries
            .iter()
            .map(|entry| format!("  {entry}"))
            .collect::<Vec<_>>();
        format!("{{\n{}\n}}", entries.join("\n"))
    }
}

/// The decoded `[ … ]` square-bracket impl catalog an impls-block entry
/// carries. It is a *catalog* of impl references, not a generated body:
/// each entry names an impl/trait/method that already exists on the Rust
/// side.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub struct SourceImplCatalog {
    #[rkyv(omit_bounds)]
    entries: Vec<SourceImplEntry>,
}

impl SourceImplCatalog {
    pub fn entries(&self) -> &[SourceImplEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Decode a `Block::Delimited { delimiter: SquareBracket, .. }` — the
    /// square-bracket catalog carried by one `impls` block entry
    /// `TypeName.[ … ]`. Each root object inside the bracket is exactly one
    /// impl entry — a bare trait atom (marker), a trait atom followed by a
    /// `[ method-sigs ]` vector, or a bare `(name { params } Return)` inherent
    /// method signature. Entries are NOT paired: the walk reads one object,
    /// then peeks the next to decide whether it is the trait's
    /// `[ method-sigs ]` partner.
    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::SquareBracket, "impl catalog")?;
        let objects = body.root_objects();
        let mut entries = Vec::new();
        let mut index = 0;
        while index < objects.len() {
            let head = &objects[index];
            // An inherent method signature is a bare parenthesis record
            // `(name { params } Return)`; consume it alone.
            if head.is_parenthesis() {
                entries.push(SourceImplEntry::InherentMethod(
                    SourceMethodSignature::from_block(head)?,
                ));
                index += 1;
                continue;
            }
            // Otherwise the head is a trait atom. A following square-bracket
            // vector is its method-signature list (body-bearing trait impl);
            // its absence is a marker impl. A trait reference obeys the same
            // PascalCase type-name gate as every other type reference, so a
            // lowercase or otherwise non-type-name atom is rejected here.
            let trait_name = SourceAtom::from_block(head)?.into_name()?;
            if !trait_name.qualifies_as_pascal_case() {
                return Err(SchemaError::NonTypeNameTrait {
                    found: trait_name.as_str().to_owned(),
                });
            }
            if let Some(next) = objects.get(index + 1)
                && next.is_square_bracket()
            {
                entries.push(SourceImplEntry::TraitImpl(
                    trait_name,
                    SourceMethodSignature::from_vector_block(next)?,
                ));
                index += 2;
            } else {
                entries.push(SourceImplEntry::Marker(trait_name));
                index += 1;
            }
        }
        Ok(Self { entries })
    }

    fn to_schema_text(&self) -> String {
        SourceDelimitedText::new(
            Delimiter::SquareBracket,
            self.entries
                .iter()
                .map(SourceImplEntry::to_schema_text)
                .collect(),
        )
        .inline()
    }

    /// Lower the source catalog into the enumerable schema-side
    /// [`ImplCatalog`], resolving every method parameter and return type
    /// reference through the namespace's type resolver so impl references
    /// obey namespace qualification like every other reference.
    fn lower(&self, resolver: &SourceTypeResolver, namespace: Option<&Name>) -> ImplCatalog {
        ImplCatalog::new(
            self.entries
                .iter()
                .map(|entry| entry.lower(resolver, namespace))
                .collect(),
        )
    }
}

/// One entry inside an `[ … ]` impl catalog.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub enum SourceImplEntry {
    /// A bare trait atom — a marker impl with no method signatures
    /// (`Display`, `Ord`).
    Marker(Name),
    /// A trait atom plus its `[ method-sigs ]` vector — a body-bearing
    /// trait impl (`QueryMatcher [ (matches { candidate.Node } Boolean) ]`).
    TraitImpl(Name, #[rkyv(omit_bounds)] Vec<SourceMethodSignature>),
    /// A bare `(name { params } Return)` — an inherent method signature.
    InherentMethod(#[rkyv(omit_bounds)] SourceMethodSignature),
}

impl SourceImplEntry {
    fn to_schema_text(&self) -> String {
        match self {
            Self::Marker(trait_name) => trait_name.to_nota(),
            Self::TraitImpl(trait_name, signatures) => {
                let signatures = SourceDelimitedText::new(
                    Delimiter::SquareBracket,
                    signatures
                        .iter()
                        .map(SourceMethodSignature::to_schema_text)
                        .collect(),
                )
                .inline();
                format!("{} {}", trait_name.to_nota(), signatures)
            }
            Self::InherentMethod(signature) => signature.to_schema_text(),
        }
    }

    fn lower(&self, resolver: &SourceTypeResolver, namespace: Option<&Name>) -> ImplReference {
        match self {
            Self::Marker(trait_name) => ImplReference::Marker(trait_name.clone()),
            Self::TraitImpl(trait_name, signatures) => ImplReference::TraitImpl(
                trait_name.clone(),
                signatures
                    .iter()
                    .map(|signature| signature.lower(resolver, namespace))
                    .collect(),
            ),
            Self::InherentMethod(signature) => {
                ImplReference::InherentMethod(signature.lower(resolver, namespace))
            }
        }
    }
}

/// A method signature `(name { params } Return)` — the same surface as a
/// Work-frame leg. It names a *callable signature* of an impl that exists on
/// the Rust side, not a generated body. `name` is a camel-case method name,
/// `parameters` are positional `paramName.Type` fields (nullary `{}` is
/// allowed), and `return_reference` is the return type at a reference
/// position.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub struct SourceMethodSignature {
    name: Name,
    #[rkyv(omit_bounds)]
    parameters: Vec<SourceMethodParameter>,
    #[rkyv(omit_bounds)]
    return_reference: SourceReference,
}

impl SourceMethodSignature {
    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn parameters(&self) -> &[SourceMethodParameter] {
        &self.parameters
    }

    pub fn return_reference(&self) -> &SourceReference {
        &self.return_reference
    }

    /// Decode a `(name { params } Return)` parenthesis record. The three
    /// positional slots are the camel method name, the brace parameter
    /// block, and the trailing return reference.
    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::Parenthesis, "method signature")?;
        let objects = body.root_objects();
        let [name_block, params_block, return_block] = objects else {
            return Err(SchemaError::ExpectedSyntaxReferenceArity {
                form: "method signature (name { params } Return)",
                expected: "a name, a brace parameter block, and a return reference",
                found: objects.len(),
            });
        };
        let name = SourceAtom::from_block(name_block)?.into_name()?;
        if !SourceIdentifierCase::new(&name).is_method() {
            return Err(SchemaError::ExpectedSyntaxReference {
                found: format!("method name {}", name.to_nota()),
            });
        }
        let parameters = SourceMethodParameter::from_brace_block(params_block)?;
        let return_reference = SourceReference::from_block(return_block)?;
        Ok(Self {
            name,
            parameters,
            return_reference,
        })
    }

    /// Decode a `[ sig sig … ]` square-bracket vector of method signatures —
    /// the trait-impl entry's method list.
    fn from_vector_block(block: &Block) -> Result<Vec<Self>, SchemaError> {
        let body = NotaBody::from_delimited(
            block,
            Delimiter::SquareBracket,
            "trait impl method signatures",
        )?;
        body.root_objects()
            .iter()
            .map(Self::from_block)
            .collect::<Result<Vec<_>, _>>()
    }

    fn to_schema_text(&self) -> String {
        let params = if self.parameters.is_empty() {
            "{}".to_owned()
        } else {
            SourceDelimitedText::new(
                Delimiter::Brace,
                self.parameters
                    .iter()
                    .map(SourceMethodParameter::to_schema_text)
                    .collect(),
            )
            .inline()
        };
        Delimiter::Parenthesis.wrap([
            self.name.to_nota(),
            params,
            self.return_reference.to_schema_text(),
        ])
    }

    fn lower(&self, resolver: &SourceTypeResolver, namespace: Option<&Name>) -> MethodSignature {
        MethodSignature::new(
            self.name.clone(),
            self.parameters
                .iter()
                .map(|parameter| parameter.lower(resolver, namespace))
                .collect(),
            resolver.resolve_reference(namespace, &self.return_reference),
        )
    }
}

/// One positional parameter of a method signature: a `paramName.Type` field
/// inside the `{ params }` block, mirroring a positional struct field. The
/// `name` is the camel parameter name; `reference` is its type at a
/// reference position.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub struct SourceMethodParameter {
    name: Name,
    #[rkyv(omit_bounds)]
    reference: SourceReference,
}

impl SourceMethodParameter {
    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn reference(&self) -> &SourceReference {
        &self.reference
    }

    /// Decode a `{ paramName.Type … }` brace block into the ordered
    /// parameter list. A nullary `{}` yields no parameters.
    fn from_brace_block(block: &Block) -> Result<Vec<Self>, SchemaError> {
        let body =
            NotaBody::from_delimited(block, Delimiter::Brace, "method signature parameters")?;
        let mut parameters = Vec::new();
        let mut index = 0;
        let objects = body.root_objects();
        while index < objects.len() {
            parameters.push(Self::from_blocks_at(objects, &mut index)?);
        }
        Ok(parameters)
    }

    /// Decode one parameter. A bare `paramName.Type` atom splits into a
    /// camel name and a plain reference. A composite reference is written as
    /// two sibling objects, `paramName.` followed by the reference object.
    fn from_blocks_at(blocks: &[Block], index: &mut usize) -> Result<Self, SchemaError> {
        // A method parameter is the UNCAPITALIZED dotted form `paramName.Type`,
        // whether the type is an inline atom or the following block. The shared
        // NOTA reader performs the split; this reader validates the name and
        // reads the value block as an ordinary reference.
        let entry = DottedExpectation::Uncapitalized
            .read_entry(&blocks[*index..])
            .map_err(|_| SchemaError::ExpectedSyntaxReference {
                found: format!(
                    "method parameter {}",
                    blocks
                        .get(*index)
                        .map(Block::reemit_fallback)
                        .unwrap_or_default()
                ),
            })?;
        *index += entry.consumed();
        let name = Name::new(SourceReference::dotted_key_text(&entry));
        Self::validate_name(&name)?;
        let reference = SourceReference::from_block(entry.value())?;
        // A parameter's type is a type reference, so its leaf must be
        // capitalized: `(m { p.lowercase } R)` is rejected here.
        reference.require_type_leaf()?;
        Ok(Self { name, reference })
    }

    fn validate_name(name: &Name) -> Result<(), SchemaError> {
        if name.as_str().is_empty() || !SourceIdentifierCase::new(name).is_method() {
            return Err(SchemaError::ExpectedSyntaxReference {
                found: format!("method parameter name {}", name.to_nota()),
            });
        }
        Ok(())
    }

    fn to_schema_text(&self) -> String {
        match &self.reference {
            SourceReference::Plain(reference) => {
                format!("{}.{}", self.name.to_nota(), reference.to_nota())
            }
            reference => format!("{}.{}", self.name.to_nota(), reference.to_schema_text()),
        }
    }

    fn lower(&self, resolver: &SourceTypeResolver, namespace: Option<&Name>) -> MethodParameter {
        MethodParameter::new(
            self.name.clone(),
            resolver.resolve_reference(namespace, &self.reference),
        )
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub enum SourceDeclarationValue {
    Reference(SourceReference),
    Text(String),
    Struct(#[rkyv(omit_bounds)] SourceStructBody),
    Enum(#[rkyv(omit_bounds)] SourceEnumBody),
}

impl SourceDeclarationValue {
    pub fn from_schema_text(source: &str) -> Result<Self, SchemaError> {
        let document = Document::parse(source)?;
        Self::from_blocks(document.root_objects())
    }

    fn from_blocks(blocks: &[Block]) -> Result<Self, SchemaError> {
        if let [block] = blocks {
            return Self::from_block(block);
        }
        let mut cursor = 0;
        let reference = SourceReference::from_blocks_at(blocks, &mut cursor)?;
        if cursor == blocks.len() {
            return Self::from_reference(reference);
        }
        Err(SchemaError::ExpectedRootObjectCount {
            expected: "one schema declaration body",
            found: blocks.len(),
        })
    }

    fn block_span_width_at(blocks: &[Block], index: usize) -> Result<usize, SchemaError> {
        SourceReference::block_span_width_at(blocks, index)
    }

    /// Decode a single declaration body block into the typed value, the
    /// inverse of [`Self::to_schema_text`]. The body's delimiter is the
    /// discriminant — `{ }` is a struct, `[ ]` an enum, a bare atom or
    /// application a reference — so this is the schema declaration decoder a
    /// re-headed help declaration round-trips through, with no parallel codec.
    pub fn from_block(block: &Block) -> Result<Self, SchemaError> {
        match block {
            Block::Atom(_) | Block::Application { .. } => {
                Self::from_reference(SourceReference::from_block(block)?)
            }
            Block::Delimited {
                delimiter: Delimiter::Parenthesis,
                ..
            } => Self::from_reference(SourceReference::from_block(block)?),
            Block::PipeText(text) => Ok(Self::Text(text.text.clone())),
            Block::Delimited {
                delimiter: Delimiter::Brace,
                ..
            } => Ok(Self::Struct(SourceStructBody::from_block(block)?)),
            Block::Delimited {
                delimiter: Delimiter::SquareBracket,
                ..
            } => Ok(Self::Enum(SourceEnumBody::from_block(block)?)),
        }
    }

    fn from_reference(reference: SourceReference) -> Result<Self, SchemaError> {
        Ok(Self::Reference(reference))
    }

    pub fn to_schema_text(&self) -> String {
        match self {
            Self::Reference(reference) => reference.to_schema_text(),
            Self::Text(text) => NotaString::new(text).format(),
            Self::Struct(body) => body.to_schema_text(),
            Self::Enum(body) => body.to_schema_text(),
        }
    }

    fn to_declaration_group(
        &self,
        name: Name,
        resolver: &SourceTypeResolver,
        namespace: Option<&Name>,
    ) -> Result<SourceDeclarationGroup, SchemaError> {
        match self {
            Self::Reference(reference) => {
                Ok(SourceDeclarationGroup::primary(TypeDeclaration::Newtype(
                    NewtypeDeclaration::new(name, resolver.resolve_reference(namespace, reference)),
                )))
            }
            Self::Text(_) => Err(SchemaError::ExpectedSyntaxDeclaration {
                found: "text declaration".to_owned(),
            }),
            Self::Struct(body) => body.to_declaration_group(name, resolver, namespace),
            Self::Enum(body) => body.to_declaration_group(name, resolver, namespace),
        }
    }

    fn to_namespace_declaration_group(
        &self,
        name: Name,
        resolver: &SourceTypeResolver,
        namespace: Option<&Name>,
    ) -> Result<SourceDeclarationGroup, SchemaError> {
        match self {
            Self::Enum(body) => body.to_public_declaration_group(name, resolver, namespace),
            Self::Reference(_) | Self::Text(_) | Self::Struct(_) => {
                self.to_declaration_group(name, resolver, namespace)
            }
        }
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub struct SourceStructBody {
    #[rkyv(omit_bounds)]
    fields: Vec<SourceField>,
}

impl SourceStructBody {
    pub fn new(fields: Vec<SourceField>) -> Self {
        Self { fields }
    }

    pub fn fields(&self) -> &[SourceField] {
        &self.fields
    }

    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::Brace, "source struct body")?;
        let fields = SourceField::from_positional_blocks(body.root_objects())?;
        let body = Self { fields };
        body.validate_product_components()?;
        Ok(body)
    }

    fn to_schema_text(&self) -> String {
        if self.fields.is_empty() {
            return "{}".to_owned();
        }
        let fields = self
            .fields
            .iter()
            .map(SourceField::to_schema_text)
            .collect::<Vec<_>>();
        SourceDelimitedText::new(Delimiter::Brace, fields).inline()
    }

    fn to_declaration_group(
        &self,
        name: Name,
        resolver: &SourceTypeResolver,
        namespace: Option<&Name>,
    ) -> Result<SourceDeclarationGroup, SchemaError> {
        self.to_declaration_group_with_visibility(
            name,
            resolver,
            namespace,
            SourceInlineDeclarationVisibility::PrivateHelper,
        )
    }

    fn to_declaration_group_with_visibility(
        &self,
        name: Name,
        resolver: &SourceTypeResolver,
        namespace: Option<&Name>,
        field_visibility: SourceInlineDeclarationVisibility,
    ) -> Result<SourceDeclarationGroup, SchemaError> {
        let mut private = Vec::new();
        let mut public = Vec::new();
        let mut fields = Vec::new();
        for field in &self.fields {
            let lowered = field.to_lowered_field(resolver, namespace, field_visibility)?;
            public.extend(lowered.public_declarations);
            private.extend(lowered.private_declarations);
            fields.push(lowered.field);
        }
        let primary = if fields.len() == 1 {
            TypeDeclaration::Newtype(NewtypeDeclaration::new(name, fields[0].reference.clone()))
        } else {
            TypeDeclaration::Struct(StructDeclaration::new(name, fields))
        };
        Ok(SourceDeclarationGroup::new(public, private, primary))
    }

    fn inline_field_declaration_names(&self) -> Vec<Name> {
        self.fields
            .iter()
            .filter_map(SourceField::inline_declaration_name)
            .collect()
    }

    fn validate_product_components(&self) -> Result<(), SchemaError> {
        for field in &self.fields {
            let Some(reference) = field.product_reference() else {
                continue;
            };
            let occurrences = self.product_reference_count(&reference);
            if occurrences == 1 && field.has_explicit_product_identity() {
                return Err(SchemaError::ExplicitFieldOnUniqueProductComponent {
                    field: field.name().to_string(),
                    type_name: reference.to_schema_text(),
                });
            }
            if occurrences > 1 && !field.has_explicit_product_identity() {
                return Err(SchemaError::DuplicateImplicitProductComponent {
                    type_name: reference.to_schema_text(),
                });
            }
            if occurrences > 1
                && field.has_explicit_product_identity()
                && self
                    .fields
                    .iter()
                    .filter(|candidate| candidate.product_reference() == Some(reference.clone()))
                    .filter(|candidate| candidate.has_explicit_product_identity())
                    .filter(|candidate| candidate.name() == field.name())
                    .count()
                    > 1
            {
                return Err(SchemaError::DuplicateExplicitProductComponentIdentity {
                    field: field.name().to_string(),
                    type_name: reference.to_schema_text(),
                });
            }
        }
        Ok(())
    }

    fn product_reference_count(&self, reference: &SourceReference) -> usize {
        self.fields
            .iter()
            .filter(|field| field.product_reference().as_ref() == Some(reference))
            .count()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceDelimitedText {
    delimiter: Delimiter,
    children: Vec<String>,
}

impl SourceDelimitedText {
    fn new(delimiter: Delimiter, children: Vec<String>) -> Self {
        Self {
            delimiter,
            children,
        }
    }

    fn inline(&self) -> String {
        if self.children.is_empty() {
            return format!(
                "{}{}",
                self.delimiter.opening_text(),
                self.delimiter.closing_text()
            );
        }
        format!(
            "{} {} {}",
            self.delimiter.opening_text(),
            self.children.join(" "),
            self.delimiter.closing_text()
        )
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceFieldIdentity {
    Implicit,
    Explicit,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SourceField {
    name: Name,
    value: SourceFieldValue,
    identity: SourceFieldIdentity,
}

impl SourceField {
    pub fn derived(name: Name) -> Self {
        Self {
            name,
            value: SourceFieldValue::Derived,
            identity: SourceFieldIdentity::Implicit,
        }
    }

    pub fn from_reference(name: Name, reference: SourceReference) -> Self {
        Self {
            name,
            value: SourceFieldValue::Reference(reference),
            identity: SourceFieldIdentity::Explicit,
        }
    }

    pub fn from_type_reference(name: Name, reference: &TypeReference) -> Self {
        let source = SourceReference::from_type_reference(reference);
        match &source {
            SourceReference::Plain(reference_name)
                if Name::new(reference_name.field_name()) == name =>
            {
                Self::derived(reference_name.clone())
            }
            _ => Self::from_reference(name, source),
        }
    }

    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn value(&self) -> &SourceFieldValue {
        &self.value
    }

    fn to_schema_text(&self) -> String {
        match (&self.value, self.identity) {
            (SourceFieldValue::Derived, SourceFieldIdentity::Implicit) => self.name.to_nota(),
            (SourceFieldValue::Derived, SourceFieldIdentity::Explicit) => {
                format!("{} {}", self.name.to_nota(), self.value.to_schema_text())
            }
            (SourceFieldValue::Reference(reference), SourceFieldIdentity::Implicit) => {
                reference.to_schema_text()
            }
            (
                SourceFieldValue::Reference(SourceReference::Plain(reference)),
                SourceFieldIdentity::Explicit,
            ) => {
                format!("{}.{}", self.name.to_nota(), reference.to_nota())
            }
            (SourceFieldValue::Reference(reference), SourceFieldIdentity::Explicit) => {
                format!("{}.{}", self.name.to_nota(), reference.to_schema_text())
            }
            (SourceFieldValue::Declaration(_), _) => {
                format!("{} {}", self.name.to_nota(), self.value.to_schema_text())
            }
        }
    }

    fn from_positional_blocks(blocks: &[Block]) -> Result<Vec<Self>, SchemaError> {
        let mut fields = Vec::new();
        let mut index = 0;
        while index < blocks.len() {
            fields.push(Self::from_positional_blocks_at(blocks, &mut index)?);
        }
        Ok(fields)
    }

    fn from_positional_blocks_at(blocks: &[Block], index: &mut usize) -> Result<Self, SchemaError> {
        if let Some(Block::Atom(atom)) = blocks.get(*index)
            && let Some(name_text) = atom.text().strip_suffix('.')
        {
            if name_text.is_empty() {
                return Err(SchemaError::RetiredStructFieldSyntax {
                    found: atom.text().to_owned(),
                });
            }
            if SourceGenericDefinitions::default()
                .definition(&Name::new(name_text))
                .is_some()
            {
                let reference = SourceReference::from_blocks_at(blocks, index)?;
                return Ok(Self {
                    name: reference.derived_field_name(),
                    value: SourceFieldValue::Reference(reference),
                    identity: SourceFieldIdentity::Implicit,
                });
            }
            *index += 1;
            let reference = SourceReference::from_blocks_at(blocks, index)?;
            return Self::from_explicit_reference(Name::new(name_text), reference);
        }
        let block = &blocks[*index];
        *index += 1;
        Self::from_positional_block(block)
    }

    fn from_positional_block(block: &Block) -> Result<Self, SchemaError> {
        if let Some((head, payload)) = block.as_application() {
            let field_name = head.schema_name()?;
            if SourceGenericDefinitions::default()
                .definition(&field_name)
                .is_some()
            {
                let reference = SourceReference::from_block(block)?;
                return Ok(Self {
                    name: reference.derived_field_name(),
                    value: SourceFieldValue::Reference(reference),
                    identity: SourceFieldIdentity::Implicit,
                });
            }
            return Self::from_explicit_reference(
                field_name,
                SourceReference::from_block(payload)?,
            );
        }
        if block.is_parenthesis() {
            if Self::is_retired_explicit_structural_field(block)? {
                return Err(SchemaError::RetiredStructFieldSyntax {
                    found: block.reemit_fallback(),
                });
            }
            let reference = SourceReference::from_block(block)?;
            return Ok(Self {
                name: reference.derived_field_name(),
                value: SourceFieldValue::Reference(reference),
                identity: SourceFieldIdentity::Implicit,
            });
        }
        let atom = SourceAtom::from_block(block)?;
        if atom.0 == "*" {
            return Err(SchemaError::RetiredStructFieldSyntax {
                found: atom.0.to_owned(),
            });
        }
        if let Some((head, payload)) = atom.0.split_once('.') {
            let head_name = Name::new(head);
            if SourceGenericDefinitions::default()
                .definition(&head_name)
                .is_some()
            {
                let reference = SourceReference::from_atom_text(atom.0)?;
                return Ok(Self {
                    name: reference.derived_field_name(),
                    value: SourceFieldValue::Reference(reference),
                    identity: SourceFieldIdentity::Implicit,
                });
            }
            return Self::from_explicit_field_reference(head, payload);
        }
        let name = atom.into_name()?;
        if SourceIdentifierCase::new(&name).is_type() {
            return Ok(Self {
                name,
                value: SourceFieldValue::Derived,
                identity: SourceFieldIdentity::Implicit,
            });
        }
        Err(SchemaError::RetiredStructFieldSyntax {
            found: name.to_nota(),
        })
    }

    fn is_retired_explicit_structural_field(block: &Block) -> Result<bool, SchemaError> {
        let body =
            NotaBody::from_delimited(block, Delimiter::Parenthesis, "explicit structural field")?;
        let objects = body.root_objects();
        if objects.len() != 2 || matches!(objects[1], Block::Atom(_) | Block::Application { .. }) {
            return Ok(false);
        }
        let name = SourceAtom::from_block(&objects[0])?.into_name()?;
        Ok(SourceIdentifierCase::new(&name).is_type()
            && !TypeReference::is_reserved_scalar_name(&name))
    }

    fn from_explicit_field_reference(
        field_name: &str,
        reference_text: &str,
    ) -> Result<Self, SchemaError> {
        let name = Name::new(field_name);
        if field_name.is_empty()
            || reference_text.is_empty()
            || field_name.contains('.')
            || !name.qualifies_as_symbol_name()
        {
            return Err(SchemaError::RetiredStructFieldSyntax {
                found: format!("{field_name}.{reference_text}"),
            });
        }
        let reference = SourceReference::from_atom_text(reference_text)?;
        Self::from_explicit_reference(name, reference)
    }

    fn from_explicit_reference(
        name: Name,
        reference: SourceReference,
    ) -> Result<Self, SchemaError> {
        if name.as_str().is_empty() || !name.qualifies_as_symbol_name() {
            return Err(SchemaError::RetiredStructFieldSyntax {
                found: format!("{}.{}", name.to_nota(), reference.to_schema_text()),
            });
        }
        let derived = reference.derived_field_name();
        if !SourceIdentifierCase::new(&name).is_type() && name.field_name() == derived.as_str() {
            return Err(SchemaError::RedundantExplicitFieldRole {
                found: format!("{}.{}", name.to_nota(), reference.to_schema_text()),
                type_name: reference.to_schema_text(),
            });
        }
        Ok(Self {
            name,
            value: SourceFieldValue::Reference(reference),
            identity: SourceFieldIdentity::Explicit,
        })
    }

    fn to_lowered_field(
        &self,
        resolver: &SourceTypeResolver,
        namespace: Option<&Name>,
        visibility: SourceInlineDeclarationVisibility,
    ) -> Result<SourceLoweredField, SchemaError> {
        match &self.value {
            SourceFieldValue::Derived => Ok(SourceLoweredField::new(
                Vec::new(),
                Vec::new(),
                FieldDeclaration {
                    name: Name::new(self.name.field_name()),
                    reference: resolver.resolve_name(namespace, &self.name),
                },
            )),
            SourceFieldValue::Reference(reference)
                if SourceIdentifierCase::new(&self.name).is_type() =>
            {
                let declaration = TypeDeclaration::Newtype(NewtypeDeclaration::new(
                    self.name.qualified_under(namespace),
                    resolver.resolve_reference(namespace, reference),
                ));
                let declarations = SourceLoweredInlineDeclarations::new(visibility, declaration);
                Ok(SourceLoweredField::new(
                    declarations.public,
                    declarations.private,
                    FieldDeclaration {
                        name: Name::new(self.name.field_name()),
                        reference: resolver.resolve_name(namespace, &self.name),
                    },
                ))
            }
            SourceFieldValue::Reference(reference) => Ok(SourceLoweredField::new(
                Vec::new(),
                Vec::new(),
                FieldDeclaration {
                    name: Name::new(self.name.field_name()),
                    reference: resolver.resolve_reference(namespace, reference),
                },
            )),
            SourceFieldValue::Declaration(value)
                if SourceIdentifierCase::new(&self.name).is_type() =>
            {
                let group = value.to_declaration_group(
                    self.name.qualified_under(namespace),
                    resolver,
                    namespace,
                )?;
                let declarations = group.into_field_declarations(visibility);
                Ok(SourceLoweredField::new(
                    declarations.public,
                    declarations.private,
                    FieldDeclaration {
                        name: Name::new(self.name.field_name()),
                        reference: resolver.resolve_name(namespace, &self.name),
                    },
                ))
            }
            SourceFieldValue::Declaration(_) => Err(SchemaError::ExpectedSyntaxDeclaration {
                found: format!("inline declaration field {}", self.name),
            }),
        }
    }

    fn inline_declaration_name(&self) -> Option<Name> {
        match &self.value {
            SourceFieldValue::Reference(_) | SourceFieldValue::Declaration(_)
                if SourceIdentifierCase::new(&self.name).is_type() =>
            {
                Some(self.name.clone())
            }
            SourceFieldValue::Derived
            | SourceFieldValue::Reference(_)
            | SourceFieldValue::Declaration(_) => None,
        }
    }

    fn has_explicit_product_identity(&self) -> bool {
        self.identity == SourceFieldIdentity::Explicit
            && !SourceIdentifierCase::new(&self.name).is_type()
    }

    fn product_reference(&self) -> Option<SourceReference> {
        match &self.value {
            SourceFieldValue::Derived => Some(SourceReference::Plain(self.name.clone())),
            SourceFieldValue::Reference(reference) => Some(reference.clone()),
            SourceFieldValue::Declaration(_) if SourceIdentifierCase::new(&self.name).is_type() => {
                Some(SourceReference::Plain(self.name.clone()))
            }
            SourceFieldValue::Declaration(_) => None,
        }
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub enum SourceFieldValue {
    Derived,
    Reference(SourceReference),
    Declaration(#[rkyv(omit_bounds)] SourceDeclarationValue),
}

impl SourceFieldValue {
    pub fn to_schema_text(&self) -> String {
        match self {
            Self::Derived => "*".to_owned(),
            Self::Reference(reference) => reference.to_schema_text(),
            Self::Declaration(value) => value.to_schema_text(),
        }
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub struct SourceEnumBody {
    #[rkyv(omit_bounds)]
    variants: Vec<SourceVariantSignature>,
}

impl SourceEnumBody {
    pub fn new(variants: Vec<SourceVariantSignature>) -> Self {
        Self { variants }
    }

    pub fn variants(&self) -> &[SourceVariantSignature] {
        &self.variants
    }

    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::SquareBracket, "source enum body")?;
        Self::from_blocks(body.root_objects())
    }

    fn from_blocks(blocks: &[Block]) -> Result<Self, SchemaError> {
        // A data variant whose payload is a grouped reference spans two sibling
        // blocks (`Projected.` then `(Map.(Key Value))`), so the reader threads a
        // consumed-count cursor exactly as `TypeReference::from_blocks_at` does
        // for struct fields, rather than iterating one block per variant.
        let mut variants = Vec::new();
        let mut index = 0;
        while index < blocks.len() {
            variants.push(SourceVariantSignature::from_blocks_at(blocks, &mut index)?);
        }
        Ok(Self { variants })
    }

    fn to_schema_text(&self) -> String {
        Delimiter::SquareBracket.wrap(
            self.variants
                .iter()
                .map(SourceVariantSignature::to_structural_nota),
        )
    }

    fn to_declaration_group(
        &self,
        name: Name,
        resolver: &SourceTypeResolver,
        namespace: Option<&Name>,
    ) -> Result<SourceDeclarationGroup, SchemaError> {
        let mut private = Vec::new();
        for variant in &self.variants {
            private.extend(variant.private_inline_declarations(resolver, namespace)?);
        }
        Ok(SourceDeclarationGroup::new(
            Vec::new(),
            private,
            TypeDeclaration::Enum(self.to_schema_enum(
                name,
                &SourceVariantPayloadResolution::explicit_only(),
                namespace,
            )?),
        ))
    }

    fn to_public_declaration_group(
        &self,
        name: Name,
        resolver: &SourceTypeResolver,
        namespace: Option<&Name>,
    ) -> Result<SourceDeclarationGroup, SchemaError> {
        let mut public = Vec::new();
        for variant in &self.variants {
            public.extend(
                variant
                    .public_inline_declaration(resolver, namespace)?
                    .into_type_declarations(),
            );
        }
        Ok(SourceDeclarationGroup::new(
            public,
            Vec::new(),
            TypeDeclaration::Enum(self.to_schema_enum(
                name,
                &SourceVariantPayloadResolution::explicit_only(),
                namespace,
            )?),
        ))
    }

    fn public_inline_declarations(
        &self,
        resolver: &SourceTypeResolver,
    ) -> Result<Vec<Declaration>, SchemaError> {
        let mut declarations = Vec::new();
        for variant in &self.variants {
            let group = variant.public_inline_declaration(resolver, None)?;
            declarations.extend(group.into_public_declarations());
        }
        Ok(declarations)
    }

    fn inline_declaration_names(&self) -> Vec<Name> {
        self.variants
            .iter()
            .filter_map(SourceVariantSignature::inline_declaration_name)
            .collect()
    }

    fn public_inline_field_declaration_names(&self) -> Vec<Name> {
        self.variants
            .iter()
            .flat_map(SourceVariantSignature::public_inline_field_declaration_names)
            .collect()
    }

    fn to_schema_enum(
        &self,
        name: Name,
        resolver: &impl SourceVariantResolver,
        namespace: Option<&Name>,
    ) -> Result<EnumDeclaration, SchemaError> {
        let variants = self
            .variants
            .iter()
            .map(|variant| variant.to_enum_variant(resolver, namespace))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(EnumDeclaration::new(name, variants))
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub enum SourceVariantSignature {
    Unit(SourceVariantName),
    Data(SourceVariantName, #[rkyv(omit_bounds)] SourceVariantPayload),
}

impl SourceVariantSignature {
    pub fn from_name(name: Name) -> Self {
        Self::Unit(SourceVariantName::new(name))
    }

    pub fn from_payload(name: Name, payload: SourceVariantPayload) -> Self {
        Self::Data(SourceVariantName::new(name), payload)
    }

    pub fn name(&self) -> &Name {
        match self {
            Self::Unit(name) | Self::Data(name, _) => name.name(),
        }
    }

    pub fn payload(&self) -> Option<&SourceReference> {
        match self.payload_value() {
            Some(SourceVariantPayload::Reference(reference)) => Some(reference),
            Some(SourceVariantPayload::Declaration(_)) | None => None,
        }
    }

    pub fn payload_source(&self) -> Option<&SourceVariantPayload> {
        self.payload_value()
    }

    fn payload_value(&self) -> Option<&SourceVariantPayload> {
        match self {
            Self::Data(_, payload) => Some(payload),
            Self::Unit(_) => None,
        }
    }

    fn to_enum_variant(
        &self,
        resolver: &impl SourceVariantResolver,
        namespace: Option<&Name>,
    ) -> Result<EnumVariant, SchemaError> {
        let name = self.name().clone();
        let payload = match self {
            Self::Data(_, SourceVariantPayload::Reference(reference)) => {
                Some(resolver.resolve_reference(namespace, reference))
            }
            Self::Data(_, SourceVariantPayload::Declaration(_)) => {
                Some(resolver.resolve_name(namespace, &name))
            }
            Self::Unit(_) if resolver.resolves_variant_payload(&name) => {
                Some(resolver.resolve_name(namespace, &name))
            }
            Self::Unit(_) => None,
        };
        Ok(EnumVariant::new(name, payload))
    }

    fn public_inline_declaration(
        &self,
        resolver: &SourceTypeResolver,
        namespace: Option<&Name>,
    ) -> Result<SourceDeclarationGroup, SchemaError> {
        match self.payload_value() {
            Some(SourceVariantPayload::Declaration(SourceDeclarationValue::Struct(body))) => body
                .to_declaration_group_with_visibility(
                    self.name().qualified_under(namespace),
                    resolver,
                    namespace,
                    SourceInlineDeclarationVisibility::PublicSourceScope,
                ),
            Some(SourceVariantPayload::Declaration(value)) => value.to_declaration_group(
                self.name().qualified_under(namespace),
                resolver,
                namespace,
            ),
            Some(SourceVariantPayload::Reference(_)) | None => Ok(SourceDeclarationGroup::empty()),
        }
    }

    fn private_inline_declarations(
        &self,
        resolver: &SourceTypeResolver,
        namespace: Option<&Name>,
    ) -> Result<Vec<TypeDeclaration>, SchemaError> {
        match self.payload_value() {
            Some(SourceVariantPayload::Declaration(value)) => Ok(value
                .to_declaration_group(self.name().qualified_under(namespace), resolver, namespace)?
                .into_type_declarations()),
            Some(SourceVariantPayload::Reference(_)) | None => Ok(Vec::new()),
        }
    }

    fn inline_declaration_name(&self) -> Option<Name> {
        match self.payload_value() {
            Some(SourceVariantPayload::Declaration(_)) => Some(self.name().clone()),
            Some(SourceVariantPayload::Reference(_)) | None => None,
        }
    }

    fn public_inline_field_declaration_names(&self) -> Vec<Name> {
        match self.payload_value() {
            Some(SourceVariantPayload::Declaration(SourceDeclarationValue::Struct(body))) => {
                body.inline_field_declaration_names()
            }
            Some(SourceVariantPayload::Declaration(_))
            | Some(SourceVariantPayload::Reference(_))
            | None => Vec::new(),
        }
    }
}

impl StructuralMacroNode for SourceVariantSignature {
    type Error = SchemaError;

    fn structural_position() -> nota::PositionPredicate {
        nota::PositionPredicate::named("source enum variant")
    }

    fn structural_variants() -> Vec<StructuralVariant> {
        vec![
            nota::BlockShape::pascal_atom(Some(CaptureName::new("name")))
                .into_structural_variant("unit", "PascalCase unit variant"),
            nota::BlockShape::any(None)
                .into_structural_variant("payload", "dotted reference or inline payload variant"),
        ]
    }

    fn from_structural_block(block: &Block) -> Result<Self, StructuralMacroError<Self::Error>> {
        match block {
            Block::Atom(atom) => {
                Self::from_atom_text(atom.text()).map_err(StructuralMacroError::MatchedNode)
            }
            Block::Delimited {
                delimiter: Delimiter::Parenthesis,
                root_objects,
                ..
            } => Self::from_parenthesis(root_objects).map_err(StructuralMacroError::MatchedNode),
            _ => Err(StructuralMacroError::MatchedNode(
                SchemaError::ExpectedSyntaxEnumVariant {
                    found: block.reemit_fallback(),
                },
            )),
        }
    }

    fn from_structural_candidate(
        candidate: MacroCandidate<'_>,
    ) -> Result<Self, StructuralMacroError<Self::Error>> {
        match candidate.blocks() {
            [block] => Self::from_structural_block(block),
            blocks => Err(StructuralMacroError::ExpectedSingleRoot {
                found: blocks.len(),
            }),
        }
    }

    fn to_structural_nota(&self) -> String {
        match self {
            Self::Unit(name) => name.to_structural_nota(),
            Self::Data(name, SourceVariantPayload::Reference(reference)) => {
                // Minimal grouping, matching the struct-field application
                // emitter: a payload that stays a single inline token emits bare
                // (`Projected.ProjectedPayload`, `Listed.Vector.NodeName`), while
                // a multi-token payload is wrapped in the group the dot rule
                // requires (`Projected.(Map.(NodeName NodeConfig))`) so it
                // re-parses.
                let payload = reference.to_schema_text();
                if SourceDottedArgumentText::new(&payload).can_inline() {
                    format!("{}.{}", name.to_structural_nota(), payload)
                } else {
                    format!("{}.({})", name.to_structural_nota(), payload)
                }
            }
            Self::Data(name, SourceVariantPayload::Declaration(payload)) => {
                Delimiter::Parenthesis.wrap([name.to_structural_nota(), payload.to_schema_text()])
            }
        }
    }
}

impl SourceVariantSignature {
    /// Read one variant from the head of a block sequence, advancing `index` by
    /// the blocks it consumes. A unit variant or an inline data variant is a
    /// single atom; a data variant whose payload is a grouped reference
    /// (`Projected.(Map.(Key Value))`, `Listed.(Vector.NodeName)`) is a
    /// dot-terminated head atom plus the following group, which the shared
    /// dotted reader binds through its consumed count — the same machinery
    /// `SourceReference::from_blocks_at` uses, so the two payload shapes never
    /// grow parallel walks.
    fn from_blocks_at(blocks: &[Block], index: &mut usize) -> Result<Self, SchemaError> {
        let block = blocks.get(*index).ok_or(SchemaError::ExpectedEnumVariant)?;
        match block {
            Block::Application { .. } => {
                let entry = DottedExpectation::Capitalized.read_entry(&blocks[*index..])?;
                let name = SourceVariantName::from_text(SourceReference::dotted_key_text(&entry))?;
                let payload = SourceReference::from_variant_payload(name.name(), entry.value())?;
                *index += entry.consumed();
                Ok(Self::Data(name, SourceVariantPayload::Reference(payload)))
            }
            Block::Atom(atom) => match DottedExpectation::Capitalized.read_entry(&blocks[*index..])
            {
                Ok(entry) => {
                    let name =
                        SourceVariantName::from_text(SourceReference::dotted_key_text(&entry))?;
                    let payload =
                        SourceReference::from_variant_payload(name.name(), entry.value())?;
                    *index += entry.consumed();
                    Ok(Self::Data(name, SourceVariantPayload::Reference(payload)))
                }
                // No top-level dot: the atom is a plain unit variant.
                Err(NotaDecodeError::ExpectedDottedEntry { .. }) => {
                    *index += 1;
                    Ok(Self::Unit(SourceVariantName::from_text(atom.text())?))
                }
                Err(error) => Err(SchemaError::from(error)),
            },
            Block::Delimited {
                delimiter: Delimiter::Parenthesis,
                root_objects,
                ..
            } => {
                *index += 1;
                Self::from_parenthesis(root_objects)
            }
            _ => Err(SchemaError::ExpectedSyntaxEnumVariant {
                found: block.reemit_fallback(),
            }),
        }
    }

    fn from_atom_text(text: &str) -> Result<Self, SchemaError> {
        // A data variant is a CAPITALIZED variant head dotted onto its payload
        // reference; the split is the shared string-level dotted reader's. No
        // top-level dot is the unit-variant case, not an error.
        match DottedExpectation::Capitalized.read_string_entry(text) {
            Ok((name, payload)) => Ok(Self::Data(
                SourceVariantName::from_text(name)?,
                SourceVariantPayload::Reference(SourceReference::from_atom_text(payload)?),
            )),
            Err(NotaDecodeError::ExpectedDottedEntry { .. }) => {
                Ok(Self::Unit(SourceVariantName::from_text(text)?))
            }
            Err(error) => Err(SchemaError::from(error)),
        }
    }

    fn from_parenthesis(objects: &[Block]) -> Result<Self, SchemaError> {
        match objects {
            [name] => Err(SchemaError::SameNamedVariantPayload {
                enum_name: "<source>".to_owned(),
                variant_name: name.schema_name()?.as_str().to_owned(),
                payload_type: name.schema_name()?.as_str().to_owned(),
            }),
            [name, payload] => Ok(Self::Data(
                SourceVariantName::from_structural_block(name).map_err(SchemaError::from)?,
                SourceVariantPayload::from_structural_block(payload).map_err(SchemaError::from)?,
            )),
            _ => Err(SchemaError::ExpectedSyntaxEnumVariant {
                found: format!("parenthesized variant with {} objects", objects.len()),
            }),
        }
    }
}

/// A PascalCase schema symbol at a variant-name or stream-name position. It owns
/// the lowered `Name` and decodes itself from a bare PascalCase atom, so the
/// `SourceVariantSignature` derive can recurse into each name field.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SourceVariantName(Name);

impl SourceVariantName {
    pub fn new(name: Name) -> Self {
        Self(name)
    }

    fn from_text(text: &str) -> Result<Self, SchemaError> {
        if Self::qualifies(text) {
            Ok(Self(Name::new(text)))
        } else {
            Err(SchemaError::ExpectedSyntaxEnumVariant {
                found: text.to_owned(),
            })
        }
    }

    fn name(&self) -> &Name {
        &self.0
    }

    fn qualifies(value: &str) -> bool {
        value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_uppercase())
            && !value.contains('@')
    }
}

impl StructuralMacroNode for SourceVariantName {
    type Error = SchemaError;

    fn structural_position() -> nota::PositionPredicate {
        nota::PositionPredicate::named("variant name")
    }

    fn structural_variants() -> Vec<StructuralVariant> {
        vec![
            nota::BlockShape::pascal_atom(Some(CaptureName::new("name")))
                .into_structural_variant("symbol", "PascalCase atom"),
        ]
    }

    fn from_structural_block(block: &Block) -> Result<Self, StructuralMacroError<Self::Error>> {
        let Some(text) = block.demote_to_string() else {
            return Err(StructuralMacroError::MatchedNode(
                SchemaError::ExpectedSymbol {
                    found: block.reemit_fallback(),
                },
            ));
        };
        if !Self::qualifies(text) {
            return Err(StructuralMacroError::MatchedNode(
                SchemaError::ExpectedSyntaxEnumVariant {
                    found: block.reemit_fallback(),
                },
            ));
        }
        Ok(Self(Name::new(text)))
    }

    fn from_structural_candidate(
        candidate: MacroCandidate<'_>,
    ) -> Result<Self, StructuralMacroError<Self::Error>> {
        match candidate.blocks() {
            [block] => Self::from_structural_block(block),
            blocks => Err(StructuralMacroError::ExpectedSingleRoot {
                found: blocks.len(),
            }),
        }
    }

    fn to_structural_nota(&self) -> String {
        self.0.to_nota()
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub enum SourceVariantPayload {
    Reference(SourceReference),
    Declaration(#[rkyv(omit_bounds)] SourceDeclarationValue),
}

impl SourceVariantPayload {
    pub fn to_schema_text(&self) -> String {
        match self {
            Self::Reference(reference) => reference.to_schema_text(),
            Self::Declaration(value) => value.to_schema_text(),
        }
    }
}

impl StructuralMacroNode for SourceVariantPayload {
    type Error = SchemaError;

    fn structural_position() -> nota::PositionPredicate {
        nota::PositionPredicate::named("variant payload")
    }

    fn structural_variants() -> Vec<StructuralVariant> {
        Vec::new()
    }

    fn from_structural_block(block: &Block) -> Result<Self, StructuralMacroError<Self::Error>> {
        let decoded = match SourceReference::from_block(block) {
            Ok(reference) => Self::Reference(reference),
            Err(_) => SourceDeclarationValue::from_block(block)
                .map(Self::Declaration)
                .map_err(StructuralMacroError::MatchedNode)?,
        };
        Ok(decoded)
    }

    fn from_structural_candidate(
        candidate: MacroCandidate<'_>,
    ) -> Result<Self, StructuralMacroError<Self::Error>> {
        match candidate.blocks() {
            [block] => Self::from_structural_block(block),
            blocks => Err(StructuralMacroError::ExpectedSingleRoot {
                found: blocks.len(),
            }),
        }
    }

    fn to_structural_nota(&self) -> String {
        self.to_schema_text()
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub enum SourceReference {
    Plain(Name),
    ValueApplication(#[rkyv(omit_bounds)] Box<SourceValueApplication>),
    SingleTypeApplication(#[rkyv(omit_bounds)] Box<SourceSingleTypeApplication>),
    MultiTypeApplication(#[rkyv(omit_bounds)] Box<SourceMultiTypeApplication>),
    Application {
        head: Name,
        #[rkyv(omit_bounds)]
        arguments: Vec<SourceReference>,
    },
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub struct SourceValueApplication {
    head: Name,
    projection: ValueReferenceProjection,
    field_name_pattern: SourceApplicationFieldNamePattern,
    value: SourceGenericValue,
}

impl SourceValueApplication {
    fn new(
        head: Name,
        projection: ValueReferenceProjection,
        field_name_pattern: SourceApplicationFieldNamePattern,
        value: SourceGenericValue,
    ) -> Self {
        Self {
            head,
            projection,
            field_name_pattern,
            value,
        }
    }

    fn to_schema_text(&self) -> String {
        SourceGenericDefinition::application_text_for_head(
            &self.head,
            [self.value.to_schema_text()],
        )
    }

    fn derived_field_name(&self) -> Name {
        self.field_name_pattern.derived_field_name([])
    }

    fn to_type_reference(&self) -> TypeReference {
        TypeReference::value_application(self.projection, self.value.unsigned_integer())
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub struct SourceSingleTypeApplication {
    head: Name,
    projection: SingleTypeReferenceProjection,
    field_name_pattern: SourceApplicationFieldNamePattern,
    #[rkyv(omit_bounds)]
    argument: Box<SourceReference>,
}

impl SourceSingleTypeApplication {
    fn new(
        head: Name,
        projection: SingleTypeReferenceProjection,
        field_name_pattern: SourceApplicationFieldNamePattern,
        argument: SourceReference,
    ) -> Self {
        Self {
            head,
            projection,
            field_name_pattern,
            argument: Box::new(argument),
        }
    }

    fn to_schema_text(&self) -> String {
        SourceGenericDefinition::application_text_for_head(
            &self.head,
            [self.argument.to_schema_text()],
        )
    }

    fn derived_field_name(&self) -> Name {
        self.field_name_pattern
            .derived_field_name([self.argument.as_ref()])
    }

    fn to_type_reference(&self) -> TypeReference {
        TypeReference::single_type_application(self.projection, self.argument.to_type_reference())
    }

    fn resolve_reference_with<Resolver: SourceVariantResolver + ?Sized>(
        &self,
        resolver: &Resolver,
        namespace: Option<&Name>,
    ) -> TypeReference {
        TypeReference::single_type_application(
            self.projection,
            resolver.resolve_reference(namespace, &self.argument),
        )
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
#[rkyv(
    bytecheck(bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source
    )),
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source)
)]
pub struct SourceMultiTypeApplication {
    head: Name,
    projection: MultiTypeReferenceProjection,
    field_name_pattern: SourceApplicationFieldNamePattern,
    #[rkyv(omit_bounds)]
    arguments: Vec<SourceReference>,
}

impl SourceMultiTypeApplication {
    fn new(
        head: Name,
        projection: MultiTypeReferenceProjection,
        field_name_pattern: SourceApplicationFieldNamePattern,
        arguments: Vec<SourceReference>,
    ) -> Self {
        Self {
            head,
            projection,
            field_name_pattern,
            arguments,
        }
    }

    fn to_schema_text(&self) -> String {
        SourceGenericDefinition::application_text_for_head(
            &self.head,
            self.arguments.iter().map(SourceReference::to_schema_text),
        )
    }

    fn derived_field_name(&self) -> Name {
        self.field_name_pattern
            .derived_field_name(self.arguments.iter())
    }

    fn to_type_reference(&self) -> TypeReference {
        TypeReference::multi_type_application(
            self.projection,
            self.arguments
                .iter()
                .map(SourceReference::to_type_reference)
                .collect(),
        )
    }

    fn resolve_reference_with<Resolver: SourceVariantResolver + ?Sized>(
        &self,
        resolver: &Resolver,
        namespace: Option<&Name>,
    ) -> TypeReference {
        TypeReference::multi_type_application(
            self.projection,
            self.arguments
                .iter()
                .map(|argument| resolver.resolve_reference(namespace, argument))
                .collect(),
        )
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
enum SourceApplicationFieldNamePattern {
    Prefix(Name),
    Suffix(Name),
    ValueByKey,
    Constant(Name),
}

impl SourceApplicationFieldNamePattern {
    fn derived_field_name<'reference>(
        &self,
        arguments: impl IntoIterator<Item = &'reference SourceReference>,
    ) -> Name {
        let arguments = arguments.into_iter().collect::<Vec<_>>();
        match self {
            Self::Prefix(prefix) => Name::new(format!(
                "{}_{}",
                prefix.as_str(),
                arguments[0].derived_field_name()
            )),
            Self::Suffix(suffix) => Name::new(format!(
                "{}_{}",
                arguments[0].derived_field_name(),
                suffix.as_str()
            )),
            Self::ValueByKey => Name::new(format!(
                "{}_by_{}",
                arguments[1].derived_field_name(),
                arguments[0].derived_field_name()
            )),
            Self::Constant(name) => name.clone(),
        }
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Copy, Debug, Eq, PartialEq)]
enum SourceGenericValue {
    UnsignedInteger(u64),
}

impl SourceGenericValue {
    fn to_schema_text(self) -> String {
        match self {
            Self::UnsignedInteger(value) => value.to_string(),
        }
    }

    fn unsigned_integer(self) -> u64 {
        match self {
            Self::UnsignedInteger(value) => value,
        }
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Copy, Debug, Eq, PartialEq)]
enum SourceGenericValueKind {
    UnsignedInteger,
}

impl SourceGenericValueKind {
    fn read_argument(self, argument: SourceReference) -> Result<SourceGenericValue, SchemaError> {
        match self {
            Self::UnsignedInteger => {
                let value = argument.unsigned_integer_argument().ok_or_else(|| {
                    SchemaError::ExpectedSyntaxReference {
                        found: argument.to_schema_text(),
                    }
                })?;
                Ok(SourceGenericValue::UnsignedInteger(value))
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SourceGenericDefinitions {
    definitions: &'static [SourceGenericDefinition],
}

impl Default for SourceGenericDefinitions {
    fn default() -> Self {
        Self {
            definitions: Self::builtin_definitions(),
        }
    }
}

impl SourceGenericDefinitions {
    fn builtin_definitions() -> &'static [SourceGenericDefinition] {
        static DEFINITIONS: [SourceGenericDefinition; 5] = [
            SourceGenericDefinition::single_type(
                "Vector",
                SingleTypeReferenceProjection::Vector,
                SourceGenericFieldNamePattern::Suffix("vector"),
            ),
            SourceGenericDefinition::single_type(
                "Optional",
                SingleTypeReferenceProjection::Optional,
                SourceGenericFieldNamePattern::Prefix("optional"),
            ),
            SourceGenericDefinition::single_type(
                "ScopeOf",
                SingleTypeReferenceProjection::ScopeOf,
                SourceGenericFieldNamePattern::Suffix("scope"),
            ),
            SourceGenericDefinition::multi_type(
                "Map",
                2,
                MultiTypeReferenceProjection::Map,
                SourceGenericFieldNamePattern::ValueByKey,
            ),
            SourceGenericDefinition::value(
                "Bytes",
                SourceGenericValueKind::UnsignedInteger,
                ValueReferenceProjection::Bytes,
                SourceGenericFieldNamePattern::Constant("bytes"),
            ),
        ];
        &DEFINITIONS
    }

    fn definitions(&self) -> &'static [SourceGenericDefinition] {
        self.definitions
    }

    fn definition(&self, name: &Name) -> Option<SourceGenericDefinition> {
        self.definitions()
            .iter()
            .copied()
            .find(|definition| definition.name == name.as_str())
    }

    fn value_definition(
        &self,
        projection: ValueReferenceProjection,
    ) -> Option<SourceGenericDefinition> {
        self.definitions()
            .iter()
            .copied()
            .find(|definition| definition.matches_value_projection(projection))
    }

    fn single_type_definition(
        &self,
        projection: SingleTypeReferenceProjection,
    ) -> Option<SourceGenericDefinition> {
        self.definitions()
            .iter()
            .copied()
            .find(|definition| definition.matches_single_type_projection(projection))
    }

    fn multi_type_definition(
        &self,
        projection: MultiTypeReferenceProjection,
    ) -> Option<SourceGenericDefinition> {
        self.definitions()
            .iter()
            .copied()
            .find(|definition| definition.matches_multi_type_projection(projection))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceGenericDefinitionKind {
    Value(SourceValueGenericDefinition),
    SingleType(SourceSingleTypeGenericDefinition),
    MultiType(SourceMultiTypeGenericDefinition),
}

impl SourceGenericDefinitionKind {
    fn argument_count(self) -> usize {
        match self {
            Self::Value(_) | Self::SingleType(_) => 1,
            Self::MultiType(definition) => definition.argument_count,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceValueGenericDefinition {
    value_kind: SourceGenericValueKind,
    projection: ValueReferenceProjection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceSingleTypeGenericDefinition {
    projection: SingleTypeReferenceProjection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceMultiTypeGenericDefinition {
    argument_count: usize,
    projection: MultiTypeReferenceProjection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceGenericFieldNamePattern {
    Prefix(&'static str),
    Suffix(&'static str),
    ValueByKey,
    Constant(&'static str),
}

impl SourceGenericFieldNamePattern {
    fn into_application_pattern(self) -> SourceApplicationFieldNamePattern {
        match self {
            Self::Prefix(prefix) => SourceApplicationFieldNamePattern::Prefix(Name::new(prefix)),
            Self::Suffix(suffix) => SourceApplicationFieldNamePattern::Suffix(Name::new(suffix)),
            Self::ValueByKey => SourceApplicationFieldNamePattern::ValueByKey,
            Self::Constant(name) => SourceApplicationFieldNamePattern::Constant(Name::new(name)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceGenericDefinition {
    name: &'static str,
    kind: SourceGenericDefinitionKind,
    field_name_pattern: SourceGenericFieldNamePattern,
}

impl SourceGenericDefinition {
    const fn value(
        name: &'static str,
        value_kind: SourceGenericValueKind,
        projection: ValueReferenceProjection,
        field_name_pattern: SourceGenericFieldNamePattern,
    ) -> Self {
        Self {
            name,
            kind: SourceGenericDefinitionKind::Value(SourceValueGenericDefinition {
                value_kind,
                projection,
            }),
            field_name_pattern,
        }
    }

    const fn single_type(
        name: &'static str,
        projection: SingleTypeReferenceProjection,
        field_name_pattern: SourceGenericFieldNamePattern,
    ) -> Self {
        Self {
            name,
            kind: SourceGenericDefinitionKind::SingleType(SourceSingleTypeGenericDefinition {
                projection,
            }),
            field_name_pattern,
        }
    }

    const fn multi_type(
        name: &'static str,
        argument_count: usize,
        projection: MultiTypeReferenceProjection,
        field_name_pattern: SourceGenericFieldNamePattern,
    ) -> Self {
        Self {
            name,
            kind: SourceGenericDefinitionKind::MultiType(SourceMultiTypeGenericDefinition {
                argument_count,
                projection,
            }),
            field_name_pattern,
        }
    }

    fn lower(self, arguments: Vec<SourceReference>) -> Result<SourceReference, SchemaError> {
        self.verify_argument_count(arguments.len())?;
        let head = Name::new(self.name);
        let field_name_pattern = self.field_name_pattern.into_application_pattern();
        match self.kind {
            SourceGenericDefinitionKind::Value(definition) => {
                let argument = arguments
                    .into_iter()
                    .next()
                    .expect("argument count checked");
                Ok(SourceReference::ValueApplication(Box::new(
                    SourceValueApplication::new(
                        head,
                        definition.projection,
                        field_name_pattern,
                        definition.value_kind.read_argument(argument)?,
                    ),
                )))
            }
            SourceGenericDefinitionKind::SingleType(definition) => {
                let argument = arguments
                    .into_iter()
                    .next()
                    .expect("argument count checked");
                Ok(SourceReference::SingleTypeApplication(Box::new(
                    SourceSingleTypeApplication::new(
                        head,
                        definition.projection,
                        field_name_pattern,
                        argument,
                    ),
                )))
            }
            SourceGenericDefinitionKind::MultiType(definition) => Ok(
                SourceReference::MultiTypeApplication(Box::new(SourceMultiTypeApplication::new(
                    head,
                    definition.projection,
                    field_name_pattern,
                    arguments,
                ))),
            ),
        }
    }

    fn source_value_application(self, value: SourceGenericValue) -> SourceReference {
        let SourceGenericDefinitionKind::Value(definition) = self.kind else {
            panic!("generic definition must be a value application")
        };
        SourceReference::ValueApplication(Box::new(SourceValueApplication::new(
            Name::new(self.name),
            definition.projection,
            self.field_name_pattern.into_application_pattern(),
            value,
        )))
    }

    fn source_single_type_application(self, argument: SourceReference) -> SourceReference {
        let SourceGenericDefinitionKind::SingleType(definition) = self.kind else {
            panic!("generic definition must be a single-type application")
        };
        SourceReference::SingleTypeApplication(Box::new(SourceSingleTypeApplication::new(
            Name::new(self.name),
            definition.projection,
            self.field_name_pattern.into_application_pattern(),
            argument,
        )))
    }

    fn source_multi_type_application(self, arguments: Vec<SourceReference>) -> SourceReference {
        let SourceGenericDefinitionKind::MultiType(definition) = self.kind else {
            panic!("generic definition must be a multi-type application")
        };
        SourceReference::MultiTypeApplication(Box::new(SourceMultiTypeApplication::new(
            Name::new(self.name),
            definition.projection,
            self.field_name_pattern.into_application_pattern(),
            arguments,
        )))
    }

    fn verify_argument_count(self, found: usize) -> Result<(), SchemaError> {
        let expected = self.kind.argument_count();
        if expected == found {
            return Ok(());
        }
        Err(SchemaError::GenericArityMismatch {
            head: self.name.to_owned(),
            expected,
            found,
        })
    }

    fn matches_value_projection(self, projection: ValueReferenceProjection) -> bool {
        matches!(
            self.kind,
            SourceGenericDefinitionKind::Value(definition) if definition.projection == projection
        )
    }

    fn matches_single_type_projection(self, projection: SingleTypeReferenceProjection) -> bool {
        matches!(
            self.kind,
            SourceGenericDefinitionKind::SingleType(definition) if definition.projection == projection
        )
    }

    fn matches_multi_type_projection(self, projection: MultiTypeReferenceProjection) -> bool {
        matches!(
            self.kind,
            SourceGenericDefinitionKind::MultiType(definition) if definition.projection == projection
        )
    }

    fn application_text_for_head(
        head: &Name,
        arguments: impl IntoIterator<Item = String>,
    ) -> String {
        let arguments = arguments.into_iter().collect::<Vec<_>>();
        match arguments.as_slice() {
            [single] if SourceDottedArgumentText::new(single).can_inline() => {
                format!("{}.{}", head.to_nota(), single)
            }
            _ => format!("{}.({})", head.to_nota(), arguments.join(" ")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceDottedArgumentText<'text>(&'text str);

impl<'text> SourceDottedArgumentText<'text> {
    fn new(text: &'text str) -> Self {
        Self(text)
    }

    fn can_inline(self) -> bool {
        !self.0.contains(char::is_whitespace) && !self.0.contains('(') && !self.0.contains(')')
    }
}

impl SourceReference {
    pub fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let blocks = std::slice::from_ref(block);
        let mut cursor = 0;
        let reference = Self::from_blocks_at(blocks, &mut cursor)?;
        if cursor == blocks.len() {
            Ok(reference)
        } else {
            Err(SchemaError::ExpectedSyntaxReference {
                found: block.reemit_fallback(),
            })
        }
    }

    /// Require that this reference names a TYPE at its leaf, per the
    /// capitalization tenet: a capitalized-leading atom is an object/type, a
    /// lowercase-leading atom is a name/reference. An application head is
    /// capitalized by construction, so only a plain lowercase leaf violates the
    /// rule — as when a method parameter's type is written lowercase
    /// (`(m { p.lowercase } R)`), which this rejects with a typed error.
    fn require_type_leaf(&self) -> Result<(), SchemaError> {
        if let Self::Plain(name) = self {
            if !SourceIdentifierCase::new(name).is_type() {
                return Err(SchemaError::ExpectedTypeReferenceLeaf {
                    found: name.to_nota(),
                });
            }
        }
        Ok(())
    }

    pub(crate) fn from_blocks_at(blocks: &[Block], index: &mut usize) -> Result<Self, SchemaError> {
        let Some(block) = blocks.get(*index) else {
            return Err(SchemaError::ExpectedSyntaxReferenceArity {
                form: "dotted reference",
                expected: "a head and payload",
                found: 0,
            });
        };
        match block {
            Block::Application { .. } => {
                let entry = DottedExpectation::Capitalized.read_entry(&blocks[*index..])?;
                *index += entry.consumed();
                let head = Name::new(Self::dotted_key_text(&entry));
                let arguments = Self::arguments_from_dotted_value(entry.value())?;
                Self::from_application_parts(head, arguments)
            }
            Block::Atom(atom) => {
                // A type application carries a CAPITALIZED dotted prefix. The
                // low-level split is the shared NOTA reader's; this reader only
                // chooses the expectation and dispatches on the value shape.
                match DottedExpectation::Capitalized.read_entry(&blocks[*index..]) {
                    Ok(entry) => {
                        *index += entry.consumed();
                        let head = Name::new(Self::dotted_key_text(&entry));
                        let arguments = Self::arguments_from_dotted_value(entry.value())?;
                        Self::from_application_parts(head, arguments)
                    }
                    // No top-level dot: the atom is a plain leaf reference.
                    Err(NotaDecodeError::ExpectedDottedEntry { .. }) => {
                        *index += 1;
                        Ok(Self::Plain(Name::new(atom.text())))
                    }
                    Err(error) => Err(SchemaError::from(error)),
                }
            }
            Block::Delimited {
                delimiter: Delimiter::Parenthesis,
                root_objects,
                ..
            } => Err(SchemaError::UnknownTypeReferenceForm {
                head: root_objects
                    .first()
                    .and_then(Block::demote_to_string)
                    .unwrap_or("<missing>")
                    .to_owned(),
                argument_count: root_objects.len().saturating_sub(1),
            }),
            Block::Delimited {
                delimiter: Delimiter::SquareBracket,
                root_objects,
                ..
            } => Err(SchemaError::UnknownTypeReferenceForm {
                head: "SquareBracket".to_owned(),
                argument_count: root_objects.len(),
            }),
            Block::Delimited {
                delimiter: Delimiter::Brace,
                root_objects,
                ..
            } => Err(SchemaError::UnknownTypeReferenceForm {
                head: "Brace".to_owned(),
                argument_count: root_objects.len(),
            }),
            Block::PipeText(_) => Err(SchemaError::ExpectedSyntaxReference {
                found: block.reemit_fallback(),
            }),
        }
    }

    pub(crate) fn block_span_width_at(
        blocks: &[Block],
        index: usize,
    ) -> Result<usize, SchemaError> {
        match blocks.get(index) {
            Some(_) => Ok(1),
            None => Err(SchemaError::ExpectedSyntaxReferenceArity {
                form: "dotted reference",
                expected: "a head and payload",
                found: 0,
            }),
        }
    }

    /// The atom text of a dotted entry's key. The shared reader guarantees the
    /// key is the atom split from the leading prefix.
    fn dotted_key_text(entry: &nota::DottedEntry) -> &str {
        entry
            .key()
            .atom()
            .map(|prefix| prefix.text())
            .expect("a dotted prefix splits to an atom key")
    }

    /// The applied arguments carried by a dotted application's value. A grouped
    /// value `(A B …)` is a positional argument list; any other single value —
    /// an inline remainder atom such as the `A` of `Vector.A`, or a following
    /// block — is one nested argument read through the same reader.
    fn arguments_from_dotted_value(value: &Block) -> Result<Vec<Self>, SchemaError> {
        if let Block::Delimited {
            delimiter: Delimiter::Parenthesis,
            root_objects,
            ..
        } = value
        {
            let mut arguments = Vec::new();
            let mut cursor = 0;
            while cursor < root_objects.len() {
                arguments.push(Self::from_blocks_at(root_objects, &mut cursor)?);
            }
            return Ok(arguments);
        }
        let mut cursor = 0;
        Ok(vec![Self::from_blocks_at(
            std::slice::from_ref(value),
            &mut cursor,
        )?])
    }

    /// Read the payload reference a variant head's dot bound as its value. A
    /// grouped value `(Map.(Key Value))` unwraps to the single reference it
    /// wraps; a plain inline value (`ProjectedPayload`, `Vector.Leaf`) is that
    /// reference directly. An inline value that is itself an incomplete
    /// application head (`Projected.Map.` sitting before a sibling group) is the
    /// ungrouped multi-argument spelling the dot rule forbids, and it is
    /// rejected here with the grouped form named rather than silently mis-bound.
    fn from_variant_payload(variant: &Name, value: &Block) -> Result<Self, SchemaError> {
        match value {
            Block::Delimited {
                delimiter: Delimiter::Parenthesis,
                root_objects,
                ..
            } => {
                let mut cursor = 0;
                let reference = Self::from_blocks_at(root_objects, &mut cursor)?;
                if cursor == root_objects.len() {
                    Ok(reference)
                } else {
                    Err(SchemaError::ExpectedSyntaxReference {
                        found: value.reemit_fallback(),
                    })
                }
            }
            Block::Application { head, .. }
                if head
                    .schema_name()
                    .ok()
                    .and_then(|name| SourceGenericDefinitions::default().definition(&name))
                    .is_some_and(|definition| definition.kind.argument_count() > 1) =>
            {
                Err(SchemaError::UngroupedVariantPayloadApplication {
                    variant: variant.to_nota(),
                    head: head.dotted_text().unwrap_or_else(|| head.reemit_fallback()),
                })
            }
            Block::Atom(atom) if atom.text().ends_with('.') => {
                Err(SchemaError::UngroupedVariantPayloadApplication {
                    variant: variant.to_nota(),
                    head: atom.text().trim_end_matches('.').to_owned(),
                })
            }
            _ => Self::from_block(value),
        }
    }

    fn from_atom_text(text: &str) -> Result<Self, SchemaError> {
        // A dotted reference head is a CAPITALIZED type application; the split is
        // the shared string-level dotted reader's, so this reader only declares
        // the expectation. No top-level dot is the plain-leaf case, not an error.
        match DottedExpectation::Capitalized.read_string_entry(text) {
            Ok((head, payload)) => {
                let argument = Self::from_atom_text(payload)?;
                Self::from_application_parts(Name::new(head), vec![argument])
            }
            Err(NotaDecodeError::ExpectedDottedEntry { .. }) => Ok(Self::Plain(Name::new(text))),
            Err(error) => Err(SchemaError::from(error)),
        }
    }

    fn from_application_parts(head: Name, arguments: Vec<Self>) -> Result<Self, SchemaError> {
        if let Some(definition) = SourceGenericDefinitions::default().definition(&head) {
            return definition.lower(arguments);
        }
        if !head.qualifies_as_pascal_case() {
            return Err(SchemaError::ExpectedSyntaxReference {
                found: head.to_nota(),
            });
        }
        Ok(Self::Application { head, arguments })
    }

    pub fn from_type_reference(reference: &TypeReference) -> Self {
        let definitions = SourceGenericDefinitions::default();
        match reference {
            TypeReference::String
            | TypeReference::Integer
            | TypeReference::Boolean
            | TypeReference::Path
            | TypeReference::Bytes => Self::Plain(Name::new(
                reference
                    .scalar_name()
                    .expect("a scalar reference exposes its canonical name"),
            )),
            TypeReference::Plain(name) => Self::Plain(name.clone()),
            TypeReference::SingleTypeApplication {
                projection,
                argument,
            } => definitions
                .single_type_definition(*projection)
                .expect("single-type definition is installed")
                .source_single_type_application(Self::from_type_reference(argument)),
            TypeReference::MultiTypeApplication {
                projection,
                arguments,
            } => definitions
                .multi_type_definition(*projection)
                .expect("multi-type definition is installed")
                .source_multi_type_application(
                    arguments.iter().map(Self::from_type_reference).collect(),
                ),
            TypeReference::ValueApplication { projection, value } => definitions
                .value_definition(*projection)
                .expect("value definition is installed")
                .source_value_application(SourceGenericValue::UnsignedInteger(*value)),
            TypeReference::Application { head, arguments } => Self::Application {
                head: head.name().clone(),
                arguments: arguments.iter().map(Self::from_type_reference).collect(),
            },
        }
    }

    /// The plain type name when this reference is a bare named type, else
    /// `None`. Help's one-level name resolution uses this to follow a node
    /// that is a bare reference to its declared struct/enum shape.
    pub fn plain_name(&self) -> Option<&Name> {
        match self {
            Self::Plain(name) => Some(name),
            Self::ValueApplication(_)
            | Self::SingleTypeApplication(_)
            | Self::MultiTypeApplication(_)
            | Self::Application { .. } => None,
        }
    }

    /// Render this reference through the schema encoder. This is the public
    /// entry the per-instance schema projection uses so that every reference
    /// token it emits comes from the one schema encoder, never a hand-written
    /// printer.
    pub fn rendered_schema_text(&self) -> String {
        self.to_schema_text()
    }

    /// Project a nota instance-schema [`TypeReference`] into a source
    /// reference. The per-instance trace captured by the decoder carries
    /// nota references; this lifts them into schema's reference
    /// vocabulary so they render through the same encoder as the contract.
    pub fn from_instance_reference(reference: &nota::TypeReference) -> Self {
        let definitions = SourceGenericDefinitions::default();
        match reference {
            nota::TypeReference::Named(name) => Self::Plain(Name::new(*name)),
            nota::TypeReference::Vector(element) => definitions
                .single_type_definition(SingleTypeReferenceProjection::Vector)
                .expect("vector definition is installed")
                .source_single_type_application(Self::from_instance_reference(element)),
            nota::TypeReference::Optional(inner) => definitions
                .single_type_definition(SingleTypeReferenceProjection::Optional)
                .expect("optional definition is installed")
                .source_single_type_application(Self::from_instance_reference(inner)),
            nota::TypeReference::Map(key, value) => definitions
                .multi_type_definition(MultiTypeReferenceProjection::Map)
                .expect("map definition is installed")
                .source_multi_type_application(vec![
                    Self::from_instance_reference(key),
                    Self::from_instance_reference(value),
                ]),
            nota::TypeReference::FixedBytes(width) => definitions
                .value_definition(ValueReferenceProjection::Bytes)
                .expect("fixed bytes definition is installed")
                .source_value_application(SourceGenericValue::UnsignedInteger(*width as u64)),
        }
    }

    fn unsigned_integer_argument(&self) -> Option<u64> {
        match self {
            Self::Plain(name) => name.as_str().parse::<u64>().ok(),
            Self::ValueApplication(_)
            | Self::SingleTypeApplication(_)
            | Self::MultiTypeApplication(_)
            | Self::Application { .. } => None,
        }
    }

    pub fn to_schema_text(&self) -> String {
        match self {
            Self::Plain(name) => name.to_nota(),
            Self::ValueApplication(application) => application.to_schema_text(),
            Self::SingleTypeApplication(application) => application.to_schema_text(),
            Self::MultiTypeApplication(application) => application.to_schema_text(),
            Self::Application { head, arguments } => {
                SourceGenericDefinition::application_text_for_head(
                    head,
                    arguments.iter().map(Self::to_schema_text),
                )
            }
        }
    }

    pub(crate) fn derived_field_name(&self) -> Name {
        match self {
            Self::Plain(name) => Name::new(name.field_name()),
            Self::ValueApplication(application) => application.derived_field_name(),
            Self::SingleTypeApplication(application) => application.derived_field_name(),
            Self::MultiTypeApplication(application) => application.derived_field_name(),
            Self::Application { head, arguments } => {
                let mut derived = Name::new(head.field_name()).as_str().to_owned();
                for argument in arguments {
                    derived.push('_');
                    derived.push_str(argument.derived_field_name().as_str());
                }
                Name::new(derived)
            }
        }
    }

    pub(crate) fn to_type_reference(&self) -> TypeReference {
        match self {
            Self::Plain(name) => TypeReference::from_name(name.clone()),
            Self::ValueApplication(application) => application.to_type_reference(),
            Self::SingleTypeApplication(application) => application.to_type_reference(),
            Self::MultiTypeApplication(application) => application.to_type_reference(),
            Self::Application { head, arguments } => TypeReference::Application {
                head: crate::ApplicationHead::Local(head.clone()),
                arguments: arguments.iter().map(Self::to_type_reference).collect(),
            },
        }
    }
}

use std::collections::{HashMap, HashSet, VecDeque};

/// A depth-capped indirection projection over a [`SourceReference`].
///
/// An indirection name is exclusively encoder-synthesized: the linkname this
/// projection synthesizes when it decomposes a deep structure. It names a
/// hoisted subtree, stays in the lowercase "name" register of the
/// capitalization semantics, and inlines at lowering.
///
/// The projection carries two independent depths. The main-structure depth cap
/// is the nesting depth beyond which a composite subtree is replaced by a
/// lowercase linkname and hoisted. The linked-structure expansion is how many of
/// those hoisted structures print after the main structure — [`Complete`] for an
/// encoding (every hoisted structure retained, so the value is complete and only
/// the factoring is lost) or [`Truncated`] for a help projection that drops the
/// hoisted structures past a visible count.
///
/// [`Complete`]: LinkedStructureExpansion::Complete
/// [`Truncated`]: LinkedStructureExpansion::Truncated
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndirectionProjection {
    main_structure_depth_cap: MainStructureDepthCap,
    linked_structure_expansion: LinkedStructureExpansion,
}

impl IndirectionProjection {
    pub fn new(
        main_structure_depth_cap: MainStructureDepthCap,
        linked_structure_expansion: LinkedStructureExpansion,
    ) -> Self {
        Self {
            main_structure_depth_cap,
            linked_structure_expansion,
        }
    }

    /// Encode `reference` as a complete, value-exact factored encoding — every
    /// beyond-cap composite subtree hoisted behind a lowercase linkname, every
    /// hoisted structure retained. `None` when the expansion is truncating: a
    /// truncated projection drops hoisted structures, so it can never stand in
    /// for an encoding whose only permitted loss is the factoring itself. The
    /// returned [`FactoredEncoding`] is the only shape that lowers back to a
    /// value; a [`HelpRendering`] never does.
    pub fn encode(&self, reference: &SourceReference) -> Option<FactoredEncoding> {
        match self.linked_structure_expansion {
            LinkedStructureExpansion::Complete => Some(FactoredEncoding::factor(
                reference,
                self.main_structure_depth_cap,
            )),
            LinkedStructureExpansion::Truncated { .. } => None,
        }
    }

    /// Render `reference` as help text. Help printing is one configuration of
    /// this same record and MAY truncate: a truncating expansion drops the
    /// hoisted structures past its visible count. The result is a
    /// [`HelpRendering`] — a text projection with no path back to a value, so a
    /// truncating print is structurally unusable where an encoding is expected.
    pub fn help(&self, reference: &SourceReference) -> HelpRendering {
        let encoding = FactoredEncoding::factor(reference, self.main_structure_depth_cap);
        HelpRendering::new(encoding.render(self.linked_structure_expansion))
    }
}

/// The nesting depth beyond which a composite subtree is decomposed behind a
/// lowercase linkname. A subtree at depth `<= cap` prints inline; a composite
/// subtree deeper than the cap is hoisted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MainStructureDepthCap(usize);

impl MainStructureDepthCap {
    pub fn new(depth: usize) -> Self {
        Self(depth)
    }

    fn depth(self) -> usize {
        self.0
    }
}

/// How many hoisted structures a projection prints after the main structure.
/// The two variants are the round-trip contract in the type system: `Complete`
/// keeps every hoisted structure, so the value survives exactly and only the
/// factoring is lost; `Truncated` drops the hoisted structures past its visible
/// count, so the rendering is lossy and cannot be an encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinkedStructureExpansion {
    Complete,
    Truncated { visible_links: usize },
}

impl LinkedStructureExpansion {
    fn visible_link_count(self, total: usize) -> usize {
        match self {
            Self::Complete => total,
            Self::Truncated { visible_links } => visible_links.min(total),
        }
    }
}

/// A complete, value-exact factoring of a reference: the main structure with its
/// beyond-cap subtrees replaced by lowercase linknames, plus the hoisted
/// structures each named by its linkname. Only the factoring — which subtrees
/// are hoisted and what the linknames are — is a projection choice; the value
/// [`lower`](Self::lower)s back exactly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FactoredEncoding {
    main: SourceReference,
    links: Vec<IndirectionLink>,
}

impl FactoredEncoding {
    fn factor(reference: &SourceReference, cap: MainStructureDepthCap) -> Self {
        let cap = cap.depth();
        let mut allocator = LinknameAllocator::new();
        let mut queue: VecDeque<(Name, SourceReference)> = VecDeque::new();
        let main = reference.factor_capped(0, cap, &mut allocator, &mut queue);
        let mut links = Vec::new();
        // Each hoisted structure is re-capped at a fresh depth so its own
        // beyond-cap subtrees hoist in turn; the head sits at depth 0 and never
        // re-hoists itself, so the worklist terminates.
        while let Some((name, structure)) = queue.pop_front() {
            let structure = structure.factor_capped(0, cap, &mut allocator, &mut queue);
            links.push(IndirectionLink { name, structure });
        }
        Self { main, links }
    }

    /// The original reference, reconstructed by inlining every linkname back
    /// into its hoisted structure. The factoring is discarded; the value is
    /// exact.
    pub fn lower(&self) -> SourceReference {
        let links: HashMap<&Name, &SourceReference> = self
            .links
            .iter()
            .map(|link| (&link.name, &link.structure))
            .collect();
        self.main.inline_links(&links)
    }

    /// The lowered value as a semantic [`TypeReference`]. This is the round-trip
    /// target: `from_type_reference` then `encode` then `to_type_reference` is
    /// the identity on the value.
    pub fn to_type_reference(&self) -> TypeReference {
        self.lower().to_type_reference()
    }

    /// The complete factored text: the main structure, then every hoisted
    /// structure on its own line introduced by its linkname.
    pub fn to_schema_text(&self) -> String {
        self.render(LinkedStructureExpansion::Complete)
    }

    fn render(&self, expansion: LinkedStructureExpansion) -> String {
        let visible = expansion.visible_link_count(self.links.len());
        let mut lines = Vec::with_capacity(visible + 1);
        lines.push(self.main.to_schema_text());
        for link in self.links.iter().take(visible) {
            lines.push(link.to_schema_text());
        }
        lines.join("\n")
    }

    pub fn main_text(&self) -> String {
        self.main.to_schema_text()
    }

    pub fn links(&self) -> &[IndirectionLink] {
        &self.links
    }
}

/// One hoisted subtree named by its linkname — the lowercase indirection name
/// standing in for the subtree in the main structure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndirectionLink {
    name: Name,
    structure: SourceReference,
}

impl IndirectionLink {
    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn structure_text(&self) -> String {
        self.structure.to_schema_text()
    }

    fn to_schema_text(&self) -> String {
        format!(
            "{} {}",
            self.name.to_nota(),
            self.structure.to_schema_text()
        )
    }
}

/// A help rendering: text only, with no path back to a value. Its distinctness
/// from [`FactoredEncoding`] is the structural guard that a truncating print
/// cannot be fed where an encoding is expected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HelpRendering {
    text: String,
}

impl HelpRendering {
    fn new(text: String) -> Self {
        Self { text }
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

impl std::fmt::Display for HelpRendering {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.text)
    }
}

/// Mints a fresh lowercase linkname from a hoisted type's name, applying the
/// duplicate-disambiguation rule when two hoisted types would collide: the
/// first `Map` is `map`, the next `map2`, and so on.
struct LinknameAllocator {
    used: HashSet<String>,
}

impl LinknameAllocator {
    fn new() -> Self {
        Self {
            used: HashSet::new(),
        }
    }

    fn allocate(&mut self, head: &Name) -> Name {
        let base = head.lower_camel();
        if self.used.insert(base.clone()) {
            return Name::new(base);
        }
        let mut ordinal = 2usize;
        loop {
            let candidate = format!("{base}{ordinal}");
            if self.used.insert(candidate.clone()) {
                return Name::new(candidate);
            }
            ordinal += 1;
        }
    }
}

impl SourceReference {
    /// The head type name a hoist would derive its linkname from, or `None` for
    /// a leaf that carries no nested datatype to hoist (a plain name or a value
    /// application).
    fn hoistable_head(&self) -> Option<&Name> {
        match self {
            Self::SingleTypeApplication(application) => Some(&application.head),
            Self::MultiTypeApplication(application) => Some(&application.head),
            Self::Application { head, .. } => Some(head),
            Self::Plain(_) | Self::ValueApplication(_) => None,
        }
    }

    /// Rewrite this reference under the depth cap: a composite subtree deeper
    /// than the cap is replaced by a fresh lowercase linkname and enqueued for
    /// hoisting; everything at or within the cap is rebuilt with its children
    /// rewritten one level deeper. Leaves are copied unchanged.
    fn factor_capped(
        &self,
        depth: usize,
        cap: usize,
        allocator: &mut LinknameAllocator,
        queue: &mut VecDeque<(Name, SourceReference)>,
    ) -> SourceReference {
        if depth > cap {
            if let Some(head) = self.hoistable_head() {
                let linkname = allocator.allocate(head);
                queue.push_back((linkname.clone(), self.clone()));
                return Self::Plain(linkname);
            }
            return self.clone();
        }
        match self {
            Self::Plain(_) | Self::ValueApplication(_) => self.clone(),
            Self::SingleTypeApplication(application) => {
                let argument = application
                    .argument
                    .factor_capped(depth + 1, cap, allocator, queue);
                Self::SingleTypeApplication(Box::new(SourceSingleTypeApplication::new(
                    application.head.clone(),
                    application.projection,
                    application.field_name_pattern.clone(),
                    argument,
                )))
            }
            Self::MultiTypeApplication(application) => {
                let arguments = application
                    .arguments
                    .iter()
                    .map(|argument| argument.factor_capped(depth + 1, cap, allocator, queue))
                    .collect();
                Self::MultiTypeApplication(Box::new(SourceMultiTypeApplication::new(
                    application.head.clone(),
                    application.projection,
                    application.field_name_pattern.clone(),
                    arguments,
                )))
            }
            Self::Application { head, arguments } => {
                let arguments = arguments
                    .iter()
                    .map(|argument| argument.factor_capped(depth + 1, cap, allocator, queue))
                    .collect();
                Self::Application {
                    head: head.clone(),
                    arguments,
                }
            }
        }
    }

    /// Inline every linkname in this reference back into its hoisted structure,
    /// recursively, reproducing the pre-factoring reference exactly. A lowercase
    /// plain leaf that matches a linkname is an indirection name; every other
    /// leaf is genuine and copied unchanged.
    fn inline_links(&self, links: &HashMap<&Name, &SourceReference>) -> SourceReference {
        match self {
            Self::Plain(name) => match links.get(name) {
                Some(structure) => structure.inline_links(links),
                None => Self::Plain(name.clone()),
            },
            Self::ValueApplication(_) => self.clone(),
            Self::SingleTypeApplication(application) => {
                let argument = application.argument.inline_links(links);
                Self::SingleTypeApplication(Box::new(SourceSingleTypeApplication::new(
                    application.head.clone(),
                    application.projection,
                    application.field_name_pattern.clone(),
                    argument,
                )))
            }
            Self::MultiTypeApplication(application) => {
                let arguments = application
                    .arguments
                    .iter()
                    .map(|argument| argument.inline_links(links))
                    .collect();
                Self::MultiTypeApplication(Box::new(SourceMultiTypeApplication::new(
                    application.head.clone(),
                    application.projection,
                    application.field_name_pattern.clone(),
                    arguments,
                )))
            }
            Self::Application { head, arguments } => {
                let arguments = arguments
                    .iter()
                    .map(|argument| argument.inline_links(links))
                    .collect();
                Self::Application {
                    head: head.clone(),
                    arguments,
                }
            }
        }
    }
}

#[cfg(test)]
mod source_reference_tests {
    use super::*;

    #[test]
    fn single_type_alias_definition_projects_vector_by_definition_data() {
        let reference = SourceGenericDefinition::single_type(
            "List",
            SingleTypeReferenceProjection::Vector,
            SourceGenericFieldNamePattern::Suffix("list"),
        )
        .lower(vec![SourceReference::Plain(Name::new("Topic"))])
        .expect("List definition lowers by single-type kind data");

        assert_eq!(reference.to_schema_text(), "List.Topic");
        assert_eq!(reference.derived_field_name(), Name::new("topic_list"));
        assert_eq!(
            reference.to_type_reference(),
            TypeReference::vector(TypeReference::new("Topic")),
        );
    }

    #[test]
    fn single_type_alias_definition_projects_optional_by_definition_data() {
        let reference = SourceGenericDefinition::single_type(
            "Maybe",
            SingleTypeReferenceProjection::Optional,
            SourceGenericFieldNamePattern::Prefix("maybe"),
        )
        .lower(vec![SourceReference::Plain(Name::new("Event"))])
        .expect("Maybe definition lowers by single-type kind data");

        assert_eq!(reference.to_schema_text(), "Maybe.Event");
        assert_eq!(reference.derived_field_name(), Name::new("maybe_event"));
        assert_eq!(
            reference.to_type_reference(),
            TypeReference::optional(TypeReference::new("Event")),
        );
    }
}

trait SourceVariantResolver {
    fn resolves_variant_payload(&self, name: &Name) -> bool;

    fn resolves_type_name(&self, name: &Name) -> bool;

    fn resolve_name(&self, namespace: Option<&Name>, name: &Name) -> TypeReference {
        TypeReference::from_name(self.visible_name(namespace, name))
    }

    fn resolve_reference(
        &self,
        namespace: Option<&Name>,
        reference: &SourceReference,
    ) -> TypeReference {
        match reference {
            SourceReference::Plain(name) => self.resolve_name(namespace, name),
            SourceReference::ValueApplication(application) => application.to_type_reference(),
            SourceReference::SingleTypeApplication(application) => {
                application.resolve_reference_with(self, namespace)
            }
            SourceReference::MultiTypeApplication(application) => {
                application.resolve_reference_with(self, namespace)
            }
            SourceReference::Application { head, arguments } => TypeReference::Application {
                head: crate::ApplicationHead::Local(self.visible_name(namespace, head)),
                arguments: arguments
                    .iter()
                    .map(|argument| self.resolve_reference(namespace, argument))
                    .collect(),
            },
        }
    }

    fn visible_name(&self, namespace: Option<&Name>, name: &Name) -> Name {
        if name.has_namespace() {
            return name.clone();
        }
        if let Some(namespace) = namespace
            && let Some(scoped_name) = self.deepest_visible_scoped_name(namespace, name)
        {
            return scoped_name;
        }
        name.clone()
    }

    fn deepest_visible_scoped_name(&self, namespace: &Name, name: &Name) -> Option<Name> {
        let segments = namespace.namespace_segments();
        for segment_count in (1..=segments.len()).rev() {
            let candidate = Name::new(format!(
                "{}:{}",
                segments[..segment_count].join(":"),
                name.as_str()
            ));
            if self.resolves_type_name(&candidate) {
                return Some(candidate);
            }
        }
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceVariantPayloadResolution {
    resolves_bare_names: bool,
}

impl SourceVariantPayloadResolution {
    fn explicit_only() -> Self {
        Self {
            resolves_bare_names: false,
        }
    }
}

impl SourceVariantResolver for SourceVariantPayloadResolution {
    fn resolves_variant_payload(&self, _name: &Name) -> bool {
        self.resolves_bare_names
    }

    fn resolves_type_name(&self, _name: &Name) -> bool {
        false
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceTypeResolver {
    names: Vec<Name>,
}

impl SourceTypeResolver {
    fn from_source(source: &SchemaSource) -> Self {
        let mut names = source.types().type_declaration_names();
        names.extend(source.generics().type_declaration_names());
        names.extend(source.input().body().inline_declaration_names());
        names.extend(source.output().body().inline_declaration_names());
        names.extend(
            source
                .input()
                .body()
                .public_inline_field_declaration_names(),
        );
        names.extend(
            source
                .output()
                .body()
                .public_inline_field_declaration_names(),
        );
        Self { names }
    }

    fn contains(&self, name: &Name) -> bool {
        self.names.iter().any(|candidate| candidate == name)
    }
}

impl SourceVariantResolver for SourceTypeResolver {
    fn resolves_variant_payload(&self, name: &Name) -> bool {
        self.contains(name)
    }

    fn resolves_type_name(&self, name: &Name) -> bool {
        self.contains(name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceLoweredNamespace {
    declarations: Vec<Declaration>,
    /// Standalone impl blocks lowered from `impls` block entries
    /// `TypeName.[ … ]`. They mint no type declaration; they attach a catalog
    /// to a type declared in the `types` (or `generics`) block, surfaced
    /// through `TrueSchema::impl_blocks`.
    impl_blocks: Vec<ImplBlock>,
}

impl SourceLoweredNamespace {
    fn from_source(
        types: &SourceTypes,
        generics: &SourceGenerics,
        impls: &SourceImpls,
        resolver: &SourceTypeResolver,
    ) -> Result<Self, SchemaError> {
        let mut namespace = Self {
            declarations: Vec::new(),
            impl_blocks: Vec::new(),
        };
        for entry in types.entries() {
            namespace.reject_reserved_scalar(entry.name())?;
            namespace.push_public_group(entry.to_declaration_group(resolver)?)?;
        }
        for entry in generics.entries() {
            namespace.reject_reserved_scalar(entry.name())?;
            namespace.push_public_group(entry.to_declaration_group(resolver)?)?;
        }
        for entry in impls.entries() {
            namespace.impl_blocks.push(entry.to_impl_block(resolver));
        }
        Ok(namespace)
    }

    /// A reserved scalar name (`String`, `Integer`, …) cannot be user-declared
    /// at a declaration-block position. The field-position machinery already
    /// gates these names; this is the matching declaration-position gate, so
    /// the single lowering path rejects `{ String.… }` the same way the retired
    /// second engine did.
    fn reject_reserved_scalar(&self, name: &Name) -> Result<(), SchemaError> {
        if TypeReference::is_reserved_scalar_name(name) {
            return Err(SchemaError::ReservedScalarTypeName {
                name: name.as_str().to_owned(),
            });
        }
        Ok(())
    }

    fn push_public_group(&mut self, group: SourceDeclarationGroup) -> Result<(), SchemaError> {
        self.push_public_declarations(group.into_public_declarations())
    }

    fn push_public_declarations(
        &mut self,
        declarations: Vec<Declaration>,
    ) -> Result<(), SchemaError> {
        for declaration in declarations {
            self.push_declaration(declaration)?;
        }
        Ok(())
    }

    fn push_declaration(&mut self, declaration: Declaration) -> Result<(), SchemaError> {
        if self
            .declarations
            .iter()
            .any(|existing| existing.name() == declaration.name())
        {
            return Err(SchemaError::DuplicateSourceDeclaration {
                name: declaration.name().as_str().to_owned(),
            });
        }
        self.declarations.push(declaration);
        Ok(())
    }

    fn into_declarations(self) -> Vec<Declaration> {
        self.declarations
    }

    fn impl_blocks(&self) -> &[ImplBlock] {
        &self.impl_blocks
    }
}

impl SourceVariantResolver for SourceLoweredNamespace {
    fn resolves_variant_payload(&self, name: &Name) -> bool {
        self.declarations
            .iter()
            .any(|declaration| declaration.name() == name)
    }

    fn resolves_type_name(&self, name: &Name) -> bool {
        self.declarations
            .iter()
            .any(|declaration| declaration.name() == name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceDeclarationGroup {
    public: Vec<TypeDeclaration>,
    private: Vec<TypeDeclaration>,
    primary: Option<TypeDeclaration>,
    /// Declared type parameters carried from a generics entry's binder group.
    /// They attach to the group's primary declaration; the inline helper
    /// declarations (public / private) are not parameterized.
    parameters: Vec<Name>,
}

impl SourceDeclarationGroup {
    fn empty() -> Self {
        Self {
            public: Vec::new(),
            private: Vec::new(),
            primary: None,
            parameters: Vec::new(),
        }
    }

    fn primary(primary: TypeDeclaration) -> Self {
        Self {
            public: Vec::new(),
            private: Vec::new(),
            primary: Some(primary),
            parameters: Vec::new(),
        }
    }

    fn new(
        public: Vec<TypeDeclaration>,
        private: Vec<TypeDeclaration>,
        primary: TypeDeclaration,
    ) -> Self {
        Self {
            public,
            private,
            primary: Some(primary),
            parameters: Vec::new(),
        }
    }

    /// Attach declared type parameters to the group's primary
    /// declaration. The binders belong to the named declaration the generics
    /// entry introduced, not to its inline helpers.
    fn with_parameters(mut self, parameters: Vec<Name>) -> Self {
        self.parameters = parameters;
        self
    }

    fn into_public_declarations(self) -> Vec<Declaration> {
        let mut declarations = self
            .public
            .into_iter()
            .map(Declaration::public)
            .collect::<Vec<_>>();
        declarations.extend(self.private.into_iter().map(Declaration::private));
        if let Some(primary) = self.primary {
            declarations.push(Declaration::public(primary).with_parameters(self.parameters));
        }
        declarations
    }

    fn into_type_declarations(self) -> Vec<TypeDeclaration> {
        let mut declarations = self.public;
        declarations.extend(self.private);
        if let Some(primary) = self.primary {
            declarations.push(primary);
        }
        declarations
    }

    fn into_field_declarations(
        self,
        visibility: SourceInlineDeclarationVisibility,
    ) -> SourceLoweredInlineDeclarations {
        let mut public = self.public;
        let mut private = self.private;
        match visibility {
            SourceInlineDeclarationVisibility::PublicSourceScope => {
                if let Some(primary) = self.primary {
                    public.push(primary);
                }
            }
            SourceInlineDeclarationVisibility::PrivateHelper => {
                if let Some(primary) = self.primary {
                    private.push(primary);
                }
            }
        }
        SourceLoweredInlineDeclarations { public, private }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceInlineDeclarationVisibility {
    PublicSourceScope,
    PrivateHelper,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceLoweredInlineDeclarations {
    public: Vec<TypeDeclaration>,
    private: Vec<TypeDeclaration>,
}

impl SourceLoweredInlineDeclarations {
    fn new(visibility: SourceInlineDeclarationVisibility, declaration: TypeDeclaration) -> Self {
        match visibility {
            SourceInlineDeclarationVisibility::PublicSourceScope => Self {
                public: vec![declaration],
                private: Vec::new(),
            },
            SourceInlineDeclarationVisibility::PrivateHelper => Self {
                public: Vec::new(),
                private: vec![declaration],
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceLoweredField {
    public_declarations: Vec<TypeDeclaration>,
    private_declarations: Vec<TypeDeclaration>,
    field: FieldDeclaration,
}

impl SourceLoweredField {
    fn new(
        public_declarations: Vec<TypeDeclaration>,
        private_declarations: Vec<TypeDeclaration>,
        field: FieldDeclaration,
    ) -> Self {
        Self {
            public_declarations,
            private_declarations,
            field,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceIdentifierCase<'name>(&'name Name);

impl<'name> SourceIdentifierCase<'name> {
    fn new(name: &'name Name) -> Self {
        Self(name)
    }

    fn is_type(&self) -> bool {
        self.0
            .as_str()
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_uppercase())
    }

    /// A simple capitalized type identifier: a type name with no interior dotted
    /// path. Import targets require this stricter shape so a dotted atom such as
    /// the `X.Y` of `crate.module.[X.Y Z]` — which `is_type` would pass on its
    /// leading uppercase alone — is rejected instead of silently kept whole.
    fn is_simple_type(&self) -> bool {
        self.is_type() && !self.0.as_str().contains('.')
    }

    fn is_namespace(&self) -> bool {
        self.0
            .as_str()
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_lowercase())
    }

    /// A method name or method-parameter name — a camel-case (lowercase-led)
    /// identifier, the same casing rule as a namespace name but read at a
    /// method-signature position.
    fn is_method(&self) -> bool {
        self.0
            .as_str()
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_lowercase())
    }
}

#[derive(Clone, Copy, Debug)]
struct SourceAtom<'source>(&'source str);

impl<'source> SourceAtom<'source> {
    fn from_block(block: &'source Block) -> Result<Self, SchemaError> {
        let Block::Atom(atom) = block else {
            return Err(SchemaError::ExpectedSymbol {
                found: SourceBlockNotation::new(block).description(),
            });
        };
        Ok(Self(atom.text()))
    }

    /// Read this source atom as a local declaration or reference name, enforcing
    /// the well-formedness the `Name` namespace machinery assumes: a
    /// source-derived local name carries no `:` namespace separator and no empty
    /// segment. Import source paths do not pass through here — they are read as
    /// dotted [`SourceReference`] paths — so this boundary rejects only a
    /// malformed local name.
    fn into_name(self) -> Result<Name, SchemaError> {
        if self.0.is_empty() || self.0.contains(':') {
            return Err(SchemaError::MalformedLocalName {
                name: self.0.to_owned(),
            });
        }
        Ok(Name::new(self.0))
    }
}

#[derive(Clone, Copy, Debug)]
struct SourceBlockNotation<'source>(&'source Block);

impl<'source> SourceBlockNotation<'source> {
    fn new(block: &'source Block) -> Self {
        Self(block)
    }

    fn description(&self) -> String {
        match self.0 {
            Block::Delimited { delimiter, .. } => {
                format!("{} block", delimiter.description())
            }
            Block::PipeText(_) => "pipe text".to_owned(),
            Block::Application { .. } => format!(
                "application {}",
                self.0
                    .dotted_text()
                    .unwrap_or_else(|| self.0.reemit_fallback())
            ),
            Block::Atom(atom) => format!("atom {}", atom.text()),
        }
    }
}
