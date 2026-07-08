use std::{
    fs,
    path::{Path, PathBuf},
};

use nota::{
    Block, CaptureName, Delimiter, Document, MacroCandidate, NotaBody, NotaEncode, NotaString,
    StructuralMacroError, StructuralMacroNode, StructuralVariant,
};

use crate::{
    Declaration, DeclarationHead, EnumDeclaration, EnumVariant, FamilyDeclaration, FamilyKey,
    FieldDeclaration, GenericBuiltin, GenericDefinition, GenericFrame, ImplBlock, ImplCatalog,
    ImplReference, ImportDeclaration, MethodParameter, MethodSignature, Name, NewtypeDeclaration,
    RelationDeclaration, RelationValue, ResolvedImport, Root, RootApplication, SchemaEngine,
    SchemaError, SchemaIdentity, StreamDeclaration, StreamRelation, StructDeclaration, TableName,
    TrueSchema, TypeDeclaration, TypeReference,
    macros::{BlockDebug, SchemaBlockExt},
};

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SchemaSource {
    imports: SourceImports,
    generics: SourceGenerics,
    input: SourceRootEnum,
    output: SourceRootEnum,
    namespace: SourceNamespace,
    relations: SourceRelations,
}

impl SchemaSource {
    pub fn from_schema_text(source: &str) -> Result<Self, SchemaError> {
        let document = Document::parse(source)?;
        Self::from_document(&document)
    }

    pub fn from_document(document: &Document) -> Result<Self, SchemaError> {
        if document.holds_root_objects() < 4 {
            return Err(SchemaError::ExpectedRootObjectCount {
                expected: "generics, input, output, and namespace roots, with optional leading imports and optional trailing relations",
                found: document.holds_root_objects(),
            });
        }

        let roots = document.root_objects();
        let first_two_are_braces =
            roots.first().is_some_and(Block::is_brace) && roots.get(1).is_some_and(Block::is_brace);
        let imports_present = first_two_are_braces && roots.len() >= 5;
        let mut cursor = 0;
        let imports = if imports_present {
            let imports = SourceImports::from_block(&roots[cursor])?;
            cursor += 1;
            imports
        } else {
            SourceImports::empty()
        };
        let generics = SourceGenerics::from_block(roots.get(cursor).ok_or(
            SchemaError::ExpectedRootObjectCount {
                expected: "generics root",
                found: roots.len(),
            },
        )?)?;
        cursor += 1;
        let input = SourceRootEnum::from_blocks_at(Name::new("Input"), roots, &mut cursor)?;
        let output = SourceRootEnum::from_blocks_at(Name::new("Output"), roots, &mut cursor)?;
        let namespace = SourceNamespace::from_block(roots.get(cursor).ok_or(
            SchemaError::ExpectedRootObjectCount {
                expected: "namespace root after generics, input, and output",
                found: roots.len(),
            },
        )?)?;
        cursor += 1;
        let relations = match roots.get(cursor) {
            Some(relations) if cursor + 1 == roots.len() => SourceRelations::from_block(relations)?,
            Some(_) => {
                return Err(SchemaError::ExpectedRootObjectCount {
                    expected: "at most one trailing relations root",
                    found: roots.len() - cursor,
                });
            }
            None => SourceRelations::empty(),
        };

        Ok(Self {
            imports,
            generics,
            input,
            output,
            namespace,
            relations,
        })
    }

    pub fn imports(&self) -> &SourceImports {
        &self.imports
    }

    pub fn generics(&self) -> &SourceGenerics {
        &self.generics
    }

    pub fn input(&self) -> &SourceRootEnum {
        &self.input
    }

    pub fn output(&self) -> &SourceRootEnum {
        &self.output
    }

    pub fn namespace(&self) -> &SourceNamespace {
        &self.namespace
    }

    pub fn relations(&self) -> &SourceRelations {
        &self.relations
    }

    pub fn stream_declarations(&self) -> Result<Vec<StreamDeclaration>, SchemaError> {
        self.namespace.stream_declarations()
    }

    pub fn family_declarations(&self) -> Result<Vec<FamilyDeclaration>, SchemaError> {
        self.namespace.family_declarations()
    }

    pub fn to_schema_text(&self) -> String {
        let mut roots = vec![
            self.imports.to_schema_text(),
            self.generics.to_schema_text(),
            self.input.body().to_schema_text(),
            self.output.body().to_schema_text(),
            self.namespace.to_schema_text(),
        ];
        if !self.relations.is_empty() {
            roots.push(self.relations.to_schema_text());
        }
        roots.join("\n")
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
    ) -> Result<TrueSchema, SchemaError> {
        let resolver = SourceTypeResolver::from_source(self);
        let generics = self.generics.to_definitions(&resolver)?;
        let mut namespace = SourceLoweredNamespace::from_source(&self.namespace, &resolver)?;
        namespace.push_public_declarations(self.input.public_inline_declarations(&resolver)?)?;
        namespace.push_public_declarations(self.output.public_inline_declarations(&resolver)?)?;
        let streams = self.namespace.stream_declarations()?;
        let families = self.namespace.family_declarations()?;
        let input = self.input.to_root(&namespace, &resolver)?;
        let output = self.output.to_root(&namespace, &resolver)?;
        let impl_blocks = namespace.impl_blocks().to_vec();
        TrueSchema::new(
            identity,
            imports,
            resolved_imports,
            generics,
            input,
            output,
            namespace.into_declarations(),
            streams,
            families,
            self.relations.to_schema_relations(),
        )
        .with_impl_blocks(impl_blocks)
        .families_verified()
        .and_then(TrueSchema::product_components_verified)
        .and_then(TrueSchema::arities_verified)
        .and_then(TrueSchema::impls_verified)
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
        let (name, parameters) = DeclarationHead::from_block(head)?.into_parts();
        if !parameters.is_empty() {
            return Err(SchemaError::ExpectedSyntaxDeclaration {
                found: format!(
                    "parameterized help declaration head {}",
                    head.reemit_fallback()
                ),
            });
        }
        let value = match tail {
            [] => None,
            [body] => Some(SourceDeclarationValue::from_block(body)?),
            _ => {
                return Err(SchemaError::ExpectedSyntaxDeclaration {
                    found: block.reemit_fallback(),
                });
            }
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
            .map(SourceImport::to_schema_import)
            .collect()
    }

    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::Brace, "source imports")?;
        if body.root_objects().len() % 2 != 0 {
            return Err(SchemaError::ExpectedEvenMapEntries {
                found: body.root_objects().len(),
            });
        }

        let mut entries = Vec::new();
        for pair in body.root_objects().chunks_exact(2) {
            entries.push(SourceImport {
                local_name: SourceAtom::from_block(&pair[0])?.into_name(),
                source: SourceReference::from_block(&pair[1])?,
            });
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

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SourceImport {
    local_name: Name,
    source: SourceReference,
}

impl SourceImport {
    pub fn local_name(&self) -> &Name {
        &self.local_name
    }

    pub fn source(&self) -> &SourceReference {
        &self.source
    }

    fn to_schema_text(&self) -> String {
        format!(
            "{} {}",
            self.local_name.to_nota(),
            self.source.to_schema_text()
        )
    }

    fn to_schema_import(&self) -> Result<ImportDeclaration, SchemaError> {
        Ok(ImportDeclaration {
            local_name: self.local_name.clone(),
            source: self.source.to_type_reference(),
        })
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SourceGenerics {
    entries: Vec<SourceGenericDefinition>,
}

impl SourceGenerics {
    pub fn entries(&self) -> &[SourceGenericDefinition] {
        &self.entries
    }

    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::Brace, "source generics")?;
        if body.root_objects().len() % 2 != 0 {
            return Err(SchemaError::ExpectedEvenMapEntries {
                found: body.root_objects().len(),
            });
        }
        let mut entries = Vec::new();
        for pair in body.root_objects().chunks_exact(2) {
            entries.push(SourceGenericDefinition::from_blocks(&pair[0], &pair[1])?);
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

    fn to_definitions(
        &self,
        resolver: &SourceTypeResolver,
    ) -> Result<Vec<GenericDefinition>, SchemaError> {
        self.entries
            .iter()
            .map(|entry| entry.to_definition(resolver))
            .collect()
    }

    fn definition_named(&self, name: &Name) -> Option<&SourceGenericDefinition> {
        self.entries.iter().find(|entry| entry.name() == name)
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SourceGenericDefinition {
    name: Name,
    builtin: SourceGenericBuiltin,
}

impl SourceGenericDefinition {
    fn from_blocks(name: &Block, builtin: &Block) -> Result<Self, SchemaError> {
        Ok(Self {
            name: SourceAtom::from_block(name)?.into_name(),
            builtin: SourceGenericBuiltin::from_block(builtin)?,
        })
    }

    pub fn name(&self) -> &Name {
        &self.name
    }

    fn builtin(&self) -> &SourceGenericBuiltin {
        &self.builtin
    }

    fn to_schema_text(&self) -> String {
        format!("{} {}", self.name.to_nota(), self.builtin.to_schema_text())
    }

    fn to_definition(
        &self,
        resolver: &SourceTypeResolver,
    ) -> Result<GenericDefinition, SchemaError> {
        Ok(GenericDefinition::new(
            self.name.clone(),
            self.builtin.to_builtin(resolver)?,
        ))
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum SourceGenericBuiltin {
    Vector,
    Optional,
    ScopeOf,
    Map,
    FixedBytes,
    Frame {
        parameters: Vec<Name>,
        body: SourceEnumBody,
    },
}

impl SourceGenericBuiltin {
    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        if let Some(name) = block.demote_to_string() {
            return match name {
                "Vector" => Ok(Self::Vector),
                "Optional" => Ok(Self::Optional),
                "ScopeOf" => Ok(Self::ScopeOf),
                "Map" => Ok(Self::Map),
                "FixedBytes" => Ok(Self::FixedBytes),
                other => Err(SchemaError::ExpectedSyntaxDeclaration {
                    found: format!("generic builtin {other}"),
                }),
            };
        }
        let body = NotaBody::from_delimited(block, Delimiter::Parenthesis, "generic frame")?;
        let objects = body.root_objects();
        let [head, parameters, variants] = objects else {
            return Err(SchemaError::ExpectedRootObjectCount {
                expected: "Frame, parameter vector, and variant vector",
                found: objects.len(),
            });
        };
        if head.demote_to_string() != Some("Frame") {
            return Err(SchemaError::ExpectedSyntaxDeclaration {
                found: block.reemit_fallback(),
            });
        }
        Ok(Self::Frame {
            parameters: Self::parameters_from_block(parameters)?,
            body: SourceEnumBody::from_block(variants)?,
        })
    }

    fn parameters_from_block(block: &Block) -> Result<Vec<Name>, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::SquareBracket, "generic parameters")?;
        body.root_objects()
            .iter()
            .map(|block| block.schema_name())
            .collect()
    }

    fn parameter_count(&self) -> usize {
        match self {
            Self::Vector | Self::Optional | Self::ScopeOf | Self::FixedBytes => 1,
            Self::Map => 2,
            Self::Frame { parameters, .. } => parameters.len(),
        }
    }

    fn to_schema_text(&self) -> String {
        match self {
            Self::Vector => "Vector".to_owned(),
            Self::Optional => "Optional".to_owned(),
            Self::ScopeOf => "ScopeOf".to_owned(),
            Self::Map => "Map".to_owned(),
            Self::FixedBytes => "FixedBytes".to_owned(),
            Self::Frame { parameters, body } => Delimiter::Parenthesis.wrap([
                "Frame".to_owned(),
                Delimiter::SquareBracket.wrap(parameters.iter().map(Name::to_nota)),
                body.to_schema_text(),
            ]),
        }
    }

    fn to_builtin(&self, resolver: &SourceTypeResolver) -> Result<GenericBuiltin, SchemaError> {
        match self {
            Self::Vector => Ok(GenericBuiltin::Vector),
            Self::Optional => Ok(GenericBuiltin::Optional),
            Self::ScopeOf => Ok(GenericBuiltin::ScopeOf),
            Self::Map => Ok(GenericBuiltin::Map),
            Self::FixedBytes => Ok(GenericBuiltin::FixedBytes),
            Self::Frame { parameters, body } => {
                let variant_resolver = ExplicitSourceVariantResolver::new(resolver);
                Ok(GenericBuiltin::Frame(GenericFrame::new(
                    parameters.clone(),
                    body.to_schema_enum(Name::new("Frame"), &variant_resolver, None)?
                        .variants,
                )))
            }
        }
    }
}

/// A root Input/Output position in the source codec. The body is a typed
/// sum mirroring the semantic [`Root`]: the enum-body form `[Variant …]`
/// or the application form `(Head Arg …)`. The name is the position name
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

    fn from_blocks_at(
        name: Name,
        blocks: &[Block],
        index: &mut usize,
    ) -> Result<Self, SchemaError> {
        Ok(Self {
            name,
            body: SourceRootBody::from_blocks_at(blocks, index)?,
        })
    }

    fn public_inline_declarations(
        &self,
        resolver: &SourceTypeResolver,
    ) -> Result<Vec<Declaration>, SchemaError> {
        self.body.public_inline_declarations(resolver)
    }

    fn to_root(
        &self,
        namespace: &SourceLoweredNamespace,
        resolver: &SourceTypeResolver,
    ) -> Result<Root, SchemaError> {
        self.body.to_root(self.name.clone(), namespace, resolver)
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

    fn from_blocks_at(blocks: &[Block], index: &mut usize) -> Result<Self, SchemaError> {
        let Some(block) = blocks.get(*index) else {
            return Err(SchemaError::ExpectedRootObjectCount {
                expected: "root body",
                found: blocks.len(),
            });
        };
        if matches!(block, Block::Atom(_)) {
            let reference = SourceReference::from_blocks_at(blocks, index)?;
            if matches!(reference, SourceReference::Application { .. }) {
                return Ok(Self::Application(reference));
            }
            return Err(SchemaError::ExpectedRootApplication {
                position: "root",
                found: reference.to_schema_text(),
            });
        }
        if block.is_parenthesis() {
            return Err(SchemaError::ExpectedRootApplication {
                position: "root",
                found: block.reemit_fallback(),
            });
        }
        *index += 1;
        Ok(Self::Enum(SourceEnumBody::from_block(block)?))
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

    fn to_root(
        &self,
        name: Name,
        namespace: &SourceLoweredNamespace,
        resolver: &SourceTypeResolver,
    ) -> Result<Root, SchemaError> {
        match self {
            Self::Enum(body) => {
                let variant_resolver = SourceRootVariantResolver::new(namespace, resolver);
                body.to_schema_enum(name, &variant_resolver, None)
                    .map(Root::Enum)
            }
            Self::Application(reference) => {
                let TypeReference::Application { head, arguments } =
                    resolver.resolve_reference(None, reference)
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

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SourceNamespace {
    entries: Vec<SourceNamespaceEntry>,
}

impl SourceNamespace {
    pub fn entries(&self) -> &[SourceNamespaceEntry] {
        &self.entries
    }

    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::Brace, "source namespace")?;
        let mut entries = Vec::new();
        let mut walk = SourceNamespaceWalk::new(body.root_objects());
        while let Some(entry) = walk.next_entry()? {
            entries.push(entry);
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

    fn stream_declarations(&self) -> Result<Vec<StreamDeclaration>, SchemaError> {
        self.stream_declarations_in_namespace(None)
    }

    fn stream_declarations_in_namespace(
        &self,
        namespace: Option<&Name>,
    ) -> Result<Vec<StreamDeclaration>, SchemaError> {
        let mut streams = Vec::new();
        for entry in &self.entries {
            let entry_streams = entry.stream_declarations(namespace)?;
            for stream in &entry_streams {
                if streams
                    .iter()
                    .any(|existing: &StreamDeclaration| existing.name == stream.name)
                {
                    return Err(SchemaError::DuplicateSourceDeclaration {
                        name: stream.name.as_str().to_owned(),
                    });
                }
            }
            streams.extend(entry_streams);
        }
        Ok(streams)
    }

    fn family_declarations(&self) -> Result<Vec<FamilyDeclaration>, SchemaError> {
        self.family_declarations_in_namespace(None)
    }

    fn family_declarations_in_namespace(
        &self,
        namespace: Option<&Name>,
    ) -> Result<Vec<FamilyDeclaration>, SchemaError> {
        let mut families: Vec<FamilyDeclaration> = Vec::new();
        for entry in &self.entries {
            let entry_families = entry.family_declarations(namespace)?;
            for family in &entry_families {
                if families.iter().any(|existing| existing.name == family.name) {
                    return Err(SchemaError::DuplicateFamilyName {
                        name: family.name.as_str().to_owned(),
                    });
                }
                if families
                    .iter()
                    .any(|existing| existing.table == family.table)
                {
                    return Err(SchemaError::DuplicateFamilyTable {
                        table: family.table.as_str().to_owned(),
                    });
                }
            }
            families.extend(entry_families);
        }
        Ok(families)
    }

    fn type_declaration_names(&self) -> Vec<Name> {
        self.type_declaration_names_in_namespace(None)
    }

    fn type_declaration_names_in_namespace(&self, namespace: Option<&Name>) -> Vec<Name> {
        self.entries
            .iter()
            .flat_map(|entry| entry.type_declaration_names(namespace))
            .collect()
    }
}

/// A cursor over a namespace body's root objects that segments them into
/// entries. Each entry is a head (declaration-head block), an optional inline
/// body (any non-pipe-brace block), and an optional trailing `{| … |}` impl
/// block. The trailing pipe-brace is a *separate* root object — it never
/// nests inside the body — so the classic `chunks_exact(2)` map-pairing
/// cannot see it; this stateful walk is what replaces it. The same grammar is
/// mirrored on the engine/macro path by [`crate::engine`]'s entry walk.
struct SourceNamespaceWalk<'block> {
    objects: &'block [Block],
    cursor: usize,
}

impl<'block> SourceNamespaceWalk<'block> {
    fn new(objects: &'block [Block]) -> Self {
        Self { objects, cursor: 0 }
    }

    /// Read the next entry, or `None` at the end of the body. An entry head
    /// is always present; a pipe-brace head is illegal (an impl block must
    /// trail a type name). After the head, an inline body is taken when the
    /// next object is a non-pipe-brace, then a trailing pipe-brace impl block
    /// is taken when present. At least one of body/impl-block is guaranteed
    /// because a lone head with neither is a missing value.
    fn next_entry(&mut self) -> Result<Option<SourceNamespaceEntry>, SchemaError> {
        let Some(head) = self.objects.get(self.cursor) else {
            return Ok(None);
        };
        if head.is_pipe_brace() {
            return Err(SchemaError::ExpectedSyntaxDeclaration {
                found: format!(
                    "leading impl block {}; a {{| … |}} block must trail a type name",
                    head.reemit_fallback()
                ),
            });
        }
        self.cursor += 1;
        let (name, parameters) = DeclarationHead::from_block(head)?.into_parts();

        let body = match self.objects.get(self.cursor) {
            Some(next) if !next.is_pipe_brace() => Some(self.next_body()?),
            _ => None,
        };

        let impls = match self.objects.get(self.cursor) {
            Some(next) if next.is_pipe_brace() => {
                self.cursor += 1;
                Some(next)
            }
            _ => None,
        };

        if body.is_none() && impls.is_none() {
            return Err(SchemaError::ExpectedSyntaxDeclaration {
                found: format!(
                    "namespace entry {} with neither a body nor a {{| … |}} impl block",
                    name.to_nota()
                ),
            });
        }

        SourceNamespaceEntry::from_parts(name, parameters, body, impls).map(Some)
    }

    fn next_body(&mut self) -> Result<SourceNamespaceEntryBody<'block>, SchemaError> {
        let Some(block) = self.objects.get(self.cursor) else {
            return Err(SchemaError::ExpectedSyntaxDeclaration {
                found: "missing namespace entry body".to_owned(),
            });
        };
        if matches!(block, Block::Atom(_)) {
            let mut reference_cursor = self.cursor;
            let reference = SourceReference::from_blocks_at(self.objects, &mut reference_cursor)?;
            self.cursor = reference_cursor;
            return Ok(SourceNamespaceEntryBody::Reference(reference));
        }
        self.cursor += 1;
        Ok(SourceNamespaceEntryBody::Block(block))
    }
}

#[derive(Clone, Debug)]
enum SourceNamespaceEntryBody<'block> {
    Block(&'block Block),
    Reference(SourceReference),
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
pub struct SourceNamespaceEntry {
    name: Name,
    parameters: Vec<Name>,
    #[rkyv(omit_bounds)]
    value: SourceNamespaceEntryValue,
    #[rkyv(omit_bounds)]
    impls: SourceImplCatalog,
}

impl SourceNamespaceEntry {
    /// Build an entry from its parsed parts. `body` is the optional inline
    /// body block (`String`, `{ … }`, `[ … ]`, …); `impls` is the optional
    /// trailing `{| … |}` block. At least one must be present — the
    /// stateful namespace walk guarantees that. A body-optional entry
    /// (`TypeName {| … |}`, no inline body) carries only impls and
    /// references the type declared elsewhere by name.
    fn from_parts(
        name: Name,
        parameters: Vec<Name>,
        body: Option<SourceNamespaceEntryBody<'_>>,
        impls: Option<&Block>,
    ) -> Result<Self, SchemaError> {
        let value = match body {
            Some(body) => Self::value_from_body(&name, &parameters, body)?,
            None => SourceNamespaceEntryValue::ImplsOnly,
        };
        let impls = match impls {
            Some(block) => SourceImplCatalog::from_block(block)?,
            None => SourceImplCatalog::empty(),
        };
        Ok(Self {
            name,
            parameters,
            value,
            impls,
        })
    }

    fn value_from_body(
        name: &Name,
        parameters: &[Name],
        body: SourceNamespaceEntryBody<'_>,
    ) -> Result<SourceNamespaceEntryValue, SchemaError> {
        match body {
            SourceNamespaceEntryBody::Reference(reference) => {
                Ok(SourceNamespaceEntryValue::Declaration(
                    SourceDeclarationValue::Reference(reference),
                ))
            }
            SourceNamespaceEntryBody::Block(body)
                if parameters.is_empty()
                    && SourceIdentifierCase::new(name).is_namespace()
                    && body.is_brace() =>
            {
                Ok(SourceNamespaceEntryValue::Namespace(
                    SourceNamespace::from_block(body)?,
                ))
            }
            SourceNamespaceEntryBody::Block(body) => Ok(SourceNamespaceEntryValue::Declaration(
                SourceDeclarationValue::from_block(body)?,
            )),
        }
    }

    pub fn impls(&self) -> &SourceImplCatalog {
        &self.impls
    }

    pub fn name(&self) -> &Name {
        &self.name
    }

    /// The declared type parameters from a parameterized entry head
    /// `(| Name Param … |)`. Empty for a bare-name entry.
    pub fn parameters(&self) -> &[Name] {
        &self.parameters
    }

    pub fn value(&self) -> Option<&SourceDeclarationValue> {
        self.value.as_declaration()
    }

    pub fn namespace(&self) -> Option<&SourceNamespace> {
        self.value.as_namespace()
    }

    fn namespace_name(&self, parent: Option<&Name>) -> Name {
        self.name.qualified_under(parent)
    }

    fn declaration_name(&self, namespace: Option<&Name>) -> Name {
        self.name.qualified_under(namespace)
    }

    fn to_schema_text(&self) -> String {
        let mut parts = vec![self.head_schema_text()];
        if let Some(body) = self.value.to_schema_text() {
            parts.push(body);
        }
        if !self.impls.is_empty() {
            parts.push(self.impls.to_schema_text());
        }
        parts.join(" ")
    }

    /// Project the entry's key position back to source text: a bare name,
    /// or a parameterized head `(| Name Param … |)` re-emitting each binder.
    fn head_schema_text(&self) -> String {
        if self.parameters.is_empty() {
            return self.name.to_nota();
        }
        let mut items = Vec::with_capacity(self.parameters.len() + 1);
        items.push(self.name.to_nota());
        items.extend(self.parameters.iter().map(Name::to_nota));
        Delimiter::PipeParenthesis.wrap(items)
    }

    fn to_declaration_group(
        &self,
        resolver: &SourceTypeResolver,
        namespace: Option<&Name>,
    ) -> Result<SourceDeclarationGroup, SchemaError> {
        self.value
            .to_namespace_declaration_group(self.declaration_name(namespace), resolver, namespace)
            .map(|group| {
                group
                    .with_parameters(self.parameters.clone())
                    .with_impls(self.lower_impls(resolver, namespace))
            })
    }

    /// Lower this entry's trailing `{| … |}` catalog to the enumerable
    /// schema-side [`ImplCatalog`]. Method references resolve under the
    /// entry's namespace like every other reference.
    fn lower_impls(&self, resolver: &SourceTypeResolver, namespace: Option<&Name>) -> ImplCatalog {
        self.impls.lower(resolver, namespace)
    }

    /// A body-optional `TypeName {| … |}` entry has no inline body — it
    /// references a type declared elsewhere. Lower it to a standalone
    /// [`ImplBlock`] keyed by that type name; the schema-wide manifest
    /// unions it with the fused catalogs. Entries with an inline body, or an
    /// empty trailing catalog, contribute no standalone block.
    fn to_impl_block(
        &self,
        resolver: &SourceTypeResolver,
        namespace: Option<&Name>,
    ) -> Option<ImplBlock> {
        if !matches!(self.value, SourceNamespaceEntryValue::ImplsOnly) || self.impls.is_empty() {
            return None;
        }
        Some(ImplBlock::new(
            self.declaration_name(namespace),
            self.lower_impls(resolver, namespace),
        ))
    }

    fn stream_declarations(
        &self,
        namespace: Option<&Name>,
    ) -> Result<Vec<StreamDeclaration>, SchemaError> {
        self.value.stream_declarations(self, namespace)
    }

    fn family_declarations(
        &self,
        namespace: Option<&Name>,
    ) -> Result<Vec<FamilyDeclaration>, SchemaError> {
        self.value.family_declarations(self, namespace)
    }

    fn type_declaration_names(&self, namespace: Option<&Name>) -> Vec<Name> {
        self.value.type_declaration_names(self, namespace)
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
pub enum SourceNamespaceEntryValue {
    Declaration(#[rkyv(omit_bounds)] SourceDeclarationValue),
    Namespace(#[rkyv(omit_bounds)] SourceNamespace),
    /// A body-optional entry `TypeName {| … |}`: no inline body, only a
    /// trailing impl catalog. The named type is declared elsewhere; this
    /// entry references it and carries impls. It mints no type declaration.
    ImplsOnly,
}

impl SourceNamespaceEntryValue {
    fn as_declaration(&self) -> Option<&SourceDeclarationValue> {
        match self {
            Self::Declaration(value) => Some(value),
            Self::Namespace(_) | Self::ImplsOnly => None,
        }
    }

    fn as_namespace(&self) -> Option<&SourceNamespace> {
        match self {
            Self::Namespace(namespace) => Some(namespace),
            Self::Declaration(_) | Self::ImplsOnly => None,
        }
    }

    fn to_schema_text(&self) -> Option<String> {
        match self {
            Self::Declaration(value) => Some(value.to_schema_text()),
            Self::Namespace(namespace) => Some(namespace.to_schema_text()),
            Self::ImplsOnly => None,
        }
    }

    fn to_namespace_declaration_group(
        &self,
        name: Name,
        resolver: &SourceTypeResolver,
        namespace: Option<&Name>,
    ) -> Result<SourceDeclarationGroup, SchemaError> {
        match self {
            Self::Declaration(value) => {
                value.to_namespace_declaration_group(name, resolver, namespace)
            }
            Self::Namespace(_) | Self::ImplsOnly => Ok(SourceDeclarationGroup::empty()),
        }
    }

    fn stream_declarations(
        &self,
        entry: &SourceNamespaceEntry,
        namespace: Option<&Name>,
    ) -> Result<Vec<StreamDeclaration>, SchemaError> {
        match self {
            Self::Declaration(value) => Ok(value
                .to_stream_declaration(entry.declaration_name(namespace))
                .into_iter()
                .collect()),
            Self::Namespace(nested) => {
                let nested_namespace = entry.namespace_name(namespace);
                nested.stream_declarations_in_namespace(Some(&nested_namespace))
            }
            Self::ImplsOnly => Ok(Vec::new()),
        }
    }

    fn family_declarations(
        &self,
        entry: &SourceNamespaceEntry,
        namespace: Option<&Name>,
    ) -> Result<Vec<FamilyDeclaration>, SchemaError> {
        match self {
            Self::Declaration(value) => Ok(value
                .to_family_declaration(entry.declaration_name(namespace))
                .into_iter()
                .collect()),
            Self::Namespace(nested) => {
                let nested_namespace = entry.namespace_name(namespace);
                nested.family_declarations_in_namespace(Some(&nested_namespace))
            }
            Self::ImplsOnly => Ok(Vec::new()),
        }
    }

    fn type_declaration_names(
        &self,
        entry: &SourceNamespaceEntry,
        namespace: Option<&Name>,
    ) -> Vec<Name> {
        match self {
            Self::Declaration(value) if value.is_type_declaration() => {
                vec![entry.declaration_name(namespace)]
            }
            Self::Declaration(_) | Self::ImplsOnly => Vec::new(),
            Self::Namespace(nested) => {
                let nested_namespace = entry.namespace_name(namespace);
                nested.type_declaration_names_in_namespace(Some(&nested_namespace))
            }
        }
    }
}

/// The decoded `{| … |}` pipe-brace impl block that trails a type
/// declaration. It is a *catalog* of impl references, not a generated body:
/// each entry names an impl/trait/method that already exists on the Rust
/// side. An empty catalog is the absence of a trailing block — a plain
/// declaration carries `SourceImplCatalog::empty()`.
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
    fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn entries(&self) -> &[SourceImplEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Decode a `Block::Delimited { delimiter: PipeBrace, .. }`. Each root
    /// object inside the pipe-brace is exactly one impl entry — a bare trait
    /// atom (marker), a trait atom followed by a `[ method-sigs ]` vector,
    /// or a bare `(name { params } Return)` inherent method signature.
    /// Unlike a namespace body, entries are NOT paired: the walk reads one
    /// object, then peeks the next to decide whether it is the trait's
    /// `[ method-sigs ]` partner.
    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::PipeBrace, "impl catalog")?;
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
            let trait_name = SourceAtom::from_block(head)?.into_name();
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
            Delimiter::PipeBrace,
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

/// One entry inside a `{| … |}` impl catalog.
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
        if objects.len() < 3 {
            return Err(SchemaError::ExpectedSyntaxReferenceArity {
                form: "method signature (name { params } Return)",
                expected: "a name, a brace parameter block, and a return reference",
                found: objects.len(),
            });
        }
        let name_block = &objects[0];
        let params_block = &objects[1];
        let name = SourceAtom::from_block(name_block)?.into_name();
        if !SourceIdentifierCase::new(&name).is_method() {
            return Err(SchemaError::ExpectedSyntaxReference {
                found: format!("method name {}", name.to_nota()),
            });
        }
        let parameters = SourceMethodParameter::from_brace_block(params_block)?;
        let return_reference = SourceReference::from_blocks(&objects[2..])?;
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
        if let Some(named) = SourceNamedBlock::from_blocks_if_trailing_dot(blocks, index)? {
            let name = named.name;
            Self::validate_name(&name)?;
            let mut value_index = *index - 1;
            let reference = SourceReference::from_blocks_at(blocks, &mut value_index)?;
            *index = value_index;
            return Ok(Self { name, reference });
        }
        let block = &blocks[*index];
        let atom = SourceAtom::from_block(block)?;
        let Some((param_name, type_name)) = atom.0.split_once('.') else {
            return Err(SchemaError::ExpectedSyntaxReference {
                found: format!("method parameter {}", atom.0),
            });
        };
        let name = Name::new(param_name);
        Self::validate_name(&name)?;
        *index += 1;
        let reference = SourceReference::from_atom_with_following(type_name, blocks, index)?;
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
pub struct SourceRelations {
    entries: Vec<SourceRelation>,
}

impl SourceRelations {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn entries(&self) -> &[SourceRelation] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::SquareBracket, "source relations")?;
        let mut entries = Vec::new();
        for object in body.root_objects() {
            entries.push(SourceRelation::from_block(object)?);
        }
        Ok(Self { entries })
    }

    fn to_schema_text(&self) -> String {
        Delimiter::SquareBracket.wrap(self.entries.iter().map(SourceRelation::to_schema_text))
    }

    fn to_schema_relations(&self) -> Vec<RelationDeclaration> {
        self.entries
            .iter()
            .map(SourceRelation::to_schema_relation)
            .collect()
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum SourceRelation {
    Equivalence(SourceEquivalenceRelation),
}

impl SourceRelation {
    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::Parenthesis, "source relation")?;
        let objects = body.root_objects();
        if objects.len() != 2 {
            return Err(SchemaError::ExpectedSyntaxReferenceArity {
                form: "relation declaration",
                expected: "relation name plus value vector",
                found: objects.len(),
            });
        }
        let head = SourceAtom::from_block(&objects[0])?;
        match head.0 {
            "Equivalence" => Ok(Self::Equivalence(SourceEquivalenceRelation::from_block(
                &objects[1],
            )?)),
            other => Err(SchemaError::ExpectedSyntaxDeclaration {
                found: format!("relation {other}"),
            }),
        }
    }

    fn to_schema_text(&self) -> String {
        match self {
            Self::Equivalence(relation) => {
                Delimiter::Parenthesis.wrap(["Equivalence".to_owned(), relation.to_schema_text()])
            }
        }
    }

    fn to_schema_relation(&self) -> RelationDeclaration {
        match self {
            Self::Equivalence(relation) => {
                RelationDeclaration::Equivalence(relation.to_relation_values())
            }
        }
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SourceEquivalenceRelation {
    values: Vec<SourceRelationValue>,
}

impl SourceEquivalenceRelation {
    pub fn values(&self) -> &[SourceRelationValue] {
        &self.values
    }

    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::SquareBracket, "equivalence values")?;
        let mut values = Vec::new();
        for object in body.root_objects() {
            values.push(SourceRelationValue::from_block(object)?);
        }
        Ok(Self { values })
    }

    fn to_schema_text(&self) -> String {
        Delimiter::SquareBracket.wrap(self.values.iter().map(SourceRelationValue::to_schema_text))
    }

    fn to_relation_values(&self) -> Vec<RelationValue> {
        self.values
            .iter()
            .map(SourceRelationValue::to_relation_value)
            .collect()
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SourceRelationValue {
    path: Vec<Name>,
}

impl SourceRelationValue {
    pub fn path(&self) -> &[Name] {
        &self.path
    }

    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        match block {
            Block::Atom(_) => Ok(Self {
                path: vec![block.schema_name()?],
            }),
            Block::Delimited {
                delimiter: Delimiter::Parenthesis,
                root_objects,
                ..
            } => {
                let mut path = Vec::new();
                for object in root_objects {
                    path.extend(Self::from_block(object)?.path);
                }
                Ok(Self { path })
            }
            Block::Delimited { .. } | Block::PipeText(_) => Err(SchemaError::ExpectedSymbol {
                found: block.reemit_fallback(),
            }),
        }
    }

    fn to_schema_text(&self) -> String {
        match self.path.as_slice() {
            [] => Delimiter::Parenthesis.wrap(Vec::<String>::new()),
            [name] => name.to_nota(),
            names => Delimiter::Parenthesis.wrap(names.iter().map(Name::to_nota)),
        }
    }

    fn to_relation_value(&self) -> RelationValue {
        RelationValue::new(self.path.clone())
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
    Stream(#[rkyv(omit_bounds)] SourceStreamBody),
    Family(#[rkyv(omit_bounds)] SourceFamilyBody),
}

impl SourceDeclarationValue {
    pub fn from_schema_text(source: &str) -> Result<Self, SchemaError> {
        let document = Document::parse(source)?;
        Self::from_blocks(document.root_objects())
    }

    fn from_blocks(blocks: &[Block]) -> Result<Self, SchemaError> {
        match blocks {
            [block] => Self::from_block(block),
            _ => Ok(Self::Reference(SourceReference::from_blocks(blocks)?)),
        }
    }

    /// Decode a single declaration body block into the typed value, the
    /// inverse of [`Self::to_schema_text`]. The body's delimiter is the
    /// discriminant — `{ }` is a struct, `[ ]` an enum, a bare atom or
    /// application a reference — so this is the schema declaration decoder a
    /// re-headed help declaration round-trips through, with no parallel codec.
    pub fn from_block(block: &Block) -> Result<Self, SchemaError> {
        match block {
            Block::Atom(_) => Ok(Self::Reference(SourceReference::from_block(block)?)),
            Block::Delimited {
                delimiter: Delimiter::Parenthesis,
                ..
            } => match Self::from_metadata_block(block)? {
                Some(value) => Ok(value),
                None => Ok(Self::Reference(SourceReference::from_block(block)?)),
            },
            Block::PipeText(text) => Ok(Self::Text(text.text.clone())),
            Block::Delimited {
                delimiter: Delimiter::Brace,
                ..
            } => Ok(Self::Struct(SourceStructBody::from_block(block)?)),
            Block::Delimited {
                delimiter: Delimiter::SquareBracket,
                ..
            } => Ok(Self::Enum(SourceEnumBody::from_block(block)?)),
            // A pipe-brace at a value position is consumed by the namespace
            // entry walk (`SourceNamespaceWalk`) as a trailing impl block, so
            // it never reaches the value path. If one does, the head it
            // should have trailed is missing its type body.
            Block::Delimited {
                delimiter: Delimiter::PipeBrace,
                ..
            } => Err(SchemaError::ExpectedSyntaxDeclaration {
                found: format!(
                    "stray impl block {} at a value position",
                    block.reemit_fallback()
                ),
            }),
            // A pipe-parenthesis declares type-parameter binders at a head
            // position, never a value; still rejected here.
            Block::Delimited {
                delimiter: Delimiter::PipeParenthesis,
                ..
            } => Err(SchemaError::ExpectedSyntaxDeclaration {
                found: block.reemit_fallback(),
            }),
        }
    }

    fn from_metadata_block(block: &Block) -> Result<Option<Self>, SchemaError> {
        if let Some(stream) = SourceStreamBody::from_block(block)? {
            return Ok(Some(Self::Stream(stream)));
        }
        SourceFamilyBody::from_block(block).map(|body| body.map(Self::Family))
    }

    pub fn to_schema_text(&self) -> String {
        match self {
            Self::Reference(reference) => reference.to_schema_text(),
            Self::Text(text) => NotaString::new(text).format(),
            Self::Struct(body) => body.to_schema_text(),
            Self::Enum(body) => body.to_schema_text(),
            Self::Stream(body) => body.to_schema_text(),
            Self::Family(body) => body.to_schema_text(),
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
            Self::Stream(_) | Self::Family(_) => Ok(SourceDeclarationGroup::empty()),
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
            Self::Reference(_)
            | Self::Text(_)
            | Self::Struct(_)
            | Self::Stream(_)
            | Self::Family(_) => self.to_declaration_group(name, resolver, namespace),
        }
    }

    fn to_stream_declaration(&self, name: Name) -> Option<StreamDeclaration> {
        match self {
            Self::Stream(body) => Some(body.to_stream_declaration(name)),
            Self::Reference(_)
            | Self::Text(_)
            | Self::Struct(_)
            | Self::Enum(_)
            | Self::Family(_) => None,
        }
    }

    fn to_family_declaration(&self, name: Name) -> Option<FamilyDeclaration> {
        match self {
            Self::Family(body) => Some(body.to_family_declaration(name)),
            Self::Reference(_)
            | Self::Text(_)
            | Self::Struct(_)
            | Self::Enum(_)
            | Self::Stream(_) => None,
        }
    }

    fn is_type_declaration(&self) -> bool {
        !matches!(self, Self::Stream(_) | Self::Family(_))
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SourceStreamBody {
    token: SourceReference,
    opened: SourceReference,
    event: SourceReference,
    close: SourceReference,
}

impl SourceStreamBody {
    pub fn new(
        token: SourceReference,
        opened: SourceReference,
        event: SourceReference,
        close: SourceReference,
    ) -> Self {
        Self {
            token,
            opened,
            event,
            close,
        }
    }

    pub fn token(&self) -> &SourceReference {
        &self.token
    }

    pub fn opened(&self) -> &SourceReference {
        &self.opened
    }

    pub fn event(&self) -> &SourceReference {
        &self.event
    }

    pub fn close(&self) -> &SourceReference {
        &self.close
    }

    fn from_block(block: &Block) -> Result<Option<Self>, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::Parenthesis, "source stream body")?;
        let objects = body.root_objects();
        let Some(head) = objects.first().and_then(Block::demote_to_string) else {
            return Ok(None);
        };
        if head != "Stream" {
            return Ok(None);
        }
        if objects.len() != 2 {
            return Err(SchemaError::ExpectedSyntaxReferenceArity {
                form: "stream declaration",
                expected: "Stream plus one brace payload",
                found: objects.len(),
            });
        }
        let fields = SourceStreamFields::from_block(&objects[1])?;
        Ok(Some(fields.into_stream_body()?))
    }

    fn to_schema_text(&self) -> String {
        Delimiter::Parenthesis.wrap([
            "Stream".to_owned(),
            SourceDelimitedText::new(
                Delimiter::Brace,
                vec![
                    format!("token.{}", self.token.to_schema_text()),
                    format!("opened.{}", self.opened.to_schema_text()),
                    format!("event.{}", self.event.to_schema_text()),
                    format!("close.{}", self.close.to_schema_text()),
                ],
            )
            .inline(),
        ])
    }

    fn to_stream_declaration(&self, name: Name) -> StreamDeclaration {
        StreamDeclaration::new(
            name,
            self.token.to_type_reference(),
            self.opened.to_type_reference(),
            self.event.to_type_reference(),
            self.close.to_type_reference(),
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceStreamFields {
    token: Option<SourceReference>,
    opened: Option<SourceReference>,
    event: Option<SourceReference>,
    close: Option<SourceReference>,
}

impl SourceStreamFields {
    fn empty() -> Self {
        Self {
            token: None,
            opened: None,
            event: None,
            close: None,
        }
    }

    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::Brace, "stream declaration fields")?;
        let mut fields = Self::empty();
        let mut index = 0;
        let objects = body.root_objects();
        while index < objects.len() {
            if let Some(named) = SourceNamedBlock::from_blocks_if_trailing_dot(objects, &mut index)?
            {
                let mut value_index = index - 1;
                let reference = SourceReference::from_blocks_at(objects, &mut value_index)?;
                index = value_index;
                fields.insert(named.name.as_str(), reference)?;
                continue;
            }
            let atom = SourceAtom::from_block(&objects[index])?;
            let Some((field, reference)) = atom.0.split_once('.') else {
                return Err(SchemaError::ExpectedSyntaxDeclaration {
                    found: format!("stream field {}", atom.0),
                });
            };
            index += 1;
            fields.insert(
                field,
                SourceReference::from_atom_with_following(reference, objects, &mut index)?,
            )?;
        }
        Ok(fields)
    }

    fn insert(&mut self, field: &str, reference: SourceReference) -> Result<(), SchemaError> {
        match field {
            "token" => self.token = Some(reference),
            "opened" => self.opened = Some(reference),
            "event" => self.event = Some(reference),
            "close" => self.close = Some(reference),
            other => {
                return Err(SchemaError::ExpectedSyntaxDeclaration {
                    found: format!("stream field {other}"),
                });
            }
        }
        Ok(())
    }

    fn into_stream_body(self) -> Result<SourceStreamBody, SchemaError> {
        Ok(SourceStreamBody {
            token: Self::required_field(self.token, "token")?,
            opened: Self::required_field(self.opened, "opened")?,
            event: Self::required_field(self.event, "event")?,
            close: Self::required_field(self.close, "close")?,
        })
    }

    fn required_field(
        field: Option<SourceReference>,
        field_name: &'static str,
    ) -> Result<SourceReference, SchemaError> {
        field.ok_or_else(|| SchemaError::ExpectedSyntaxDeclaration {
            found: format!("stream missing {field_name} field"),
        })
    }
}

/// The authored body of a family declaration: `(Family { record
/// <TypeName> table <table-name> key <Domain|Identified> })` inside the
/// namespace map, on the stream-declaration precedent. The record name
/// must resolve to a declared or imported type when the source lowers.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SourceFamilyBody {
    record: Name,
    table: TableName,
    key: FamilyKey,
}

impl SourceFamilyBody {
    pub fn new(record: Name, table: TableName, key: FamilyKey) -> Self {
        Self { record, table, key }
    }

    pub fn record(&self) -> &Name {
        &self.record
    }

    pub fn table(&self) -> &TableName {
        &self.table
    }

    pub fn key(&self) -> FamilyKey {
        self.key
    }

    fn from_block(block: &Block) -> Result<Option<Self>, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::Parenthesis, "source family body")?;
        let objects = body.root_objects();
        let Some(head) = objects.first().and_then(Block::demote_to_string) else {
            return Ok(None);
        };
        if head != "Family" {
            return Ok(None);
        }
        if objects.len() != 2 {
            return Err(SchemaError::ExpectedSyntaxReferenceArity {
                form: "family declaration",
                expected: "Family plus one brace payload",
                found: objects.len(),
            });
        }
        let fields = SourceFamilyFields::from_block(&objects[1])?;
        Ok(Some(fields.into_family_body()?))
    }

    fn to_schema_text(&self) -> String {
        Delimiter::Parenthesis.wrap([
            "Family".to_owned(),
            SourceDelimitedText::new(
                Delimiter::Brace,
                vec![
                    format!("record.{}", self.record.to_nota()),
                    format!("table.{}", self.table.to_nota()),
                    format!("key.{}", self.key.to_structural_nota()),
                ],
            )
            .inline(),
        ])
    }

    fn to_family_declaration(&self, name: Name) -> FamilyDeclaration {
        FamilyDeclaration::new(name, self.record.clone(), self.table.clone(), self.key)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceFamilyFields {
    record: Option<Name>,
    table: Option<TableName>,
    key: Option<FamilyKey>,
}

impl SourceFamilyFields {
    fn empty() -> Self {
        Self {
            record: None,
            table: None,
            key: None,
        }
    }

    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::Brace, "family declaration fields")?;
        let mut fields = Self::empty();
        let mut index = 0;
        let objects = body.root_objects();
        while index < objects.len() {
            if let Some(named) = SourceNamedBlock::from_blocks_if_trailing_dot(objects, &mut index)?
            {
                fields.insert_block(named.name.as_str(), named.value)?;
                continue;
            }
            let atom = SourceAtom::from_block(&objects[index])?;
            index += 1;
            let Some((field, value)) = atom.0.split_once('.') else {
                return Err(SchemaError::ExpectedSyntaxDeclaration {
                    found: format!("family field {}", atom.0),
                });
            };
            fields.insert_atom(field, value)?;
        }
        Ok(fields)
    }

    fn insert_block(&mut self, field: &str, value: &Block) -> Result<(), SchemaError> {
        match field {
            "record" => self.record = Some(SourceAtom::from_block(value)?.into_name()),
            "table" => self.table = Some(TableName::new(SourceAtom::from_block(value)?.0)),
            "key" => {
                self.key = Some(FamilyKey::from_structural_block(value).map_err(SchemaError::from)?)
            }
            other => {
                return Err(SchemaError::ExpectedSyntaxDeclaration {
                    found: format!("family field {other}"),
                });
            }
        }
        Ok(())
    }

    fn insert_atom(&mut self, field: &str, value: &str) -> Result<(), SchemaError> {
        match field {
            "record" => self.record = Some(Name::new(value)),
            "table" => self.table = Some(TableName::new(value)),
            "key" => {
                self.key = Some(match value {
                    "Domain" => FamilyKey::Domain,
                    "Identified" => FamilyKey::Identified,
                    other => {
                        return Err(SchemaError::ExpectedSyntaxDeclaration {
                            found: format!("family key {other}"),
                        });
                    }
                })
            }
            other => {
                return Err(SchemaError::ExpectedSyntaxDeclaration {
                    found: format!("family field {other}"),
                });
            }
        }
        Ok(())
    }

    fn into_family_body(self) -> Result<SourceFamilyBody, SchemaError> {
        Ok(SourceFamilyBody {
            record: self.record.ok_or_else(|| Self::missing_field("record"))?,
            table: self.table.ok_or_else(|| Self::missing_field("table"))?,
            key: self.key.ok_or_else(|| Self::missing_field("key"))?,
        })
    }

    fn missing_field(field_name: &'static str) -> SchemaError {
        SchemaError::ExpectedSyntaxDeclaration {
            found: format!("family missing {field_name} field"),
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

#[derive(Clone, Debug)]
struct SourceNamedBlock<'source> {
    name: Name,
    value: &'source Block,
}

impl<'source> SourceNamedBlock<'source> {
    fn from_blocks_if_trailing_dot(
        blocks: &'source [Block],
        index: &mut usize,
    ) -> Result<Option<Self>, SchemaError> {
        let Some(Block::Atom(atom)) = blocks.get(*index) else {
            return Ok(None);
        };
        let Some(name_text) = atom.text().strip_suffix('.') else {
            return Ok(None);
        };
        if name_text.is_empty() {
            return Err(SchemaError::RetiredStructFieldSyntax {
                found: atom.text().to_owned(),
            });
        }
        if name_text.contains('.') || SourceIdentifierCase::new(&Name::new(name_text)).is_type() {
            return Ok(None);
        }
        let value = blocks
            .get(*index + 1)
            .ok_or(SchemaError::ExpectedSyntaxReferenceArity {
                form: "named schema field",
                expected: "a trailing-dot field name and a following value",
                found: 1,
            })?;
        *index += 2;
        Ok(Some(Self {
            name: Name::new(name_text),
            value,
        }))
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
        if let Some(named) = SourceNamedBlock::from_blocks_if_trailing_dot(blocks, index)? {
            let mut value_index = *index - 1;
            let reference = SourceReference::from_blocks_at(blocks, &mut value_index)?;
            *index = value_index;
            return Self::from_explicit_reference(named.name, reference);
        }
        let block = &blocks[*index];
        if block.is_parenthesis() {
            *index += 1;
            return Self::from_positional_block(block);
        }
        let atom = SourceAtom::from_block(block)?;
        if atom.0 == "*" {
            *index += 1;
            return Err(SchemaError::RetiredStructFieldSyntax {
                found: atom.0.to_owned(),
            });
        }
        if let Some((field_name, type_name)) = atom.0.split_once('.') {
            let name = Name::new(field_name);
            if !field_name.is_empty() && !SourceIdentifierCase::new(&name).is_type() {
                *index += 1;
                let reference =
                    SourceReference::from_atom_with_following(type_name, blocks, index)?;
                return Self::from_explicit_reference(name, reference);
            }
        }
        let mut reference_index = *index;
        let reference = SourceReference::from_blocks_at(blocks, &mut reference_index)?;
        *index = reference_index;
        match &reference {
            SourceReference::Plain(name) if SourceIdentifierCase::new(name).is_type() => Ok(Self {
                name: name.clone(),
                value: SourceFieldValue::Derived,
                identity: SourceFieldIdentity::Implicit,
            }),
            SourceReference::Plain(name) => Err(SchemaError::RetiredStructFieldSyntax {
                found: name.to_nota(),
            }),
            reference => Ok(Self {
                name: reference.derived_field_name(),
                value: SourceFieldValue::Reference(reference.clone()),
                identity: SourceFieldIdentity::Implicit,
            }),
        }
    }

    fn from_positional_block(block: &Block) -> Result<Self, SchemaError> {
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
        if let Some((field_name, type_name)) = atom.0.split_once('.') {
            return Self::from_explicit_field_reference(field_name, type_name);
        }
        let name = atom.into_name();
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
        if objects.len() != 2 {
            return Ok(false);
        }
        let name = SourceAtom::from_block(&objects[0])?.into_name();
        if crate::ReferenceHead::classify(name.as_str()).is_some() {
            return Ok(false);
        }
        Ok(SourceIdentifierCase::new(&name).is_type() && !Self::is_reserved_scalar_name(&name))
    }

    fn from_explicit_field_reference(
        field_name: &str,
        type_name: &str,
    ) -> Result<Self, SchemaError> {
        let name = Name::new(field_name);
        let reference = Name::new(type_name);
        if field_name.is_empty()
            || type_name.is_empty()
            || field_name.contains('.')
            || type_name.contains('.')
            || !name.qualifies_as_symbol_name()
            || !reference.qualifies_as_symbol_name()
            || !SourceIdentifierCase::new(&reference).is_type()
        {
            return Err(SchemaError::RetiredStructFieldSyntax {
                found: format!("{field_name}.{type_name}"),
            });
        }
        if name.field_name() == reference.field_name() && !Self::is_reserved_scalar_name(&reference)
        {
            return Err(SchemaError::RedundantExplicitFieldRole {
                found: format!("{field_name}.{type_name}"),
                type_name: reference.to_nota(),
            });
        }
        Ok(Self {
            name,
            value: SourceFieldValue::Reference(SourceReference::Plain(reference)),
            identity: SourceFieldIdentity::Explicit,
        })
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

    fn is_reserved_scalar_name(name: &Name) -> bool {
        matches!(
            name.as_str(),
            "String" | "Integer" | "Boolean" | "Path" | "Bytes"
        )
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
            SourceFieldValue::Reference(reference) => {
                let reference = resolver.resolve_reference(namespace, reference);
                let name = match self.identity {
                    SourceFieldIdentity::Implicit => reference.derived_field_name(),
                    SourceFieldIdentity::Explicit => Name::new(self.name.field_name()),
                };
                Ok(SourceLoweredField::new(
                    Vec::new(),
                    Vec::new(),
                    FieldDeclaration { name, reference },
                ))
            }
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
        let mut variants = Vec::new();
        for block in blocks {
            variants.push(SourceVariantSignature::from_block(block)?);
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
        let variant_resolver = ExplicitSourceVariantResolver::new(resolver);
        Ok(SourceDeclarationGroup::new(
            Vec::new(),
            private,
            TypeDeclaration::Enum(self.to_schema_enum(name, &variant_resolver, namespace)?),
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
        let variant_resolver = ExplicitSourceVariantResolver::new(resolver);
        Ok(SourceDeclarationGroup::new(
            public,
            Vec::new(),
            TypeDeclaration::Enum(self.to_schema_enum(name, &variant_resolver, namespace)?),
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

#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    nota::StructuralMacroNode,
    Clone,
    Debug,
    Eq,
    PartialEq,
)]
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
    #[shape(pascal_atom)]
    Unit(SourceVariantName),
    #[shape(pascal_head, arity = 1)]
    SelfTagged(SourceVariantName),
    #[shape(pascal_head, arity = 2)]
    Data(SourceVariantName, #[rkyv(omit_bounds)] SourceVariantPayload),
    #[shape(pascal_head, arity = 4)]
    Streaming(
        SourceVariantName,
        #[rkyv(omit_bounds)] SourceVariantPayload,
        StreamRelationKeyword,
        SourceVariantName,
    ),
}

impl SourceVariantSignature {
    fn from_block(block: &Block) -> Result<Self, SchemaError> {
        match block {
            Block::Atom(_) => Ok(Self::Unit(
                SourceVariantName::from_structural_block(block).map_err(SchemaError::from)?,
            )),
            Block::Delimited {
                delimiter: Delimiter::Parenthesis,
                ..
            } => Self::from_parenthesized_block(block),
            _ => Err(SchemaError::ExpectedSyntaxEnumVariant {
                found: block.reemit_fallback(),
            }),
        }
    }

    fn from_parenthesized_block(block: &Block) -> Result<Self, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::Parenthesis, "variant signature")?;
        let objects = body.root_objects();
        let Some((head, tail)) = objects.split_first() else {
            return Err(SchemaError::ExpectedSyntaxEnumVariant {
                found: block.reemit_fallback(),
            });
        };
        let name = SourceVariantName::from_structural_block(head).map_err(SchemaError::from)?;
        if tail.is_empty() {
            return Ok(Self::SelfTagged(name));
        }
        if tail.len() >= 3
            && let Some(keyword) =
                StreamRelationKeyword::from_block_if_keyword(&tail[tail.len() - 2])
        {
            let stream_name =
                SourceVariantName::from_structural_block(tail.last().expect("length checked"))
                    .map_err(SchemaError::from)?;
            let payload = SourceVariantPayload::from_blocks(&tail[..tail.len() - 2])?;
            return Ok(Self::Streaming(name, payload, keyword, stream_name));
        }
        Ok(Self::Data(name, SourceVariantPayload::from_blocks(tail)?))
    }

    pub fn from_name(name: Name) -> Self {
        Self::Unit(SourceVariantName::new(name))
    }

    pub fn from_payload(name: Name, payload: SourceVariantPayload) -> Self {
        Self::Data(SourceVariantName::new(name), payload)
    }

    pub fn from_self_tagged(name: Name) -> Self {
        Self::SelfTagged(SourceVariantName::new(name))
    }

    pub fn from_projected(
        name: Name,
        payload: Option<SourceVariantPayload>,
        stream_relation: Option<&StreamRelation>,
    ) -> Self {
        if stream_relation.is_none()
            && matches!(
                &payload,
                Some(SourceVariantPayload::Reference(SourceReference::Plain(payload_name)))
                    if payload_name == &name
            )
        {
            return Self::from_self_tagged(name);
        }
        match (payload, stream_relation) {
            (Some(payload), Some(relation)) => Self::Streaming(
                SourceVariantName::new(name),
                payload,
                StreamRelationKeyword::from(relation),
                SourceVariantName::new(relation.stream_name().clone()),
            ),
            (Some(payload), None) => Self::from_payload(name, payload),
            (None, Some(_)) | (None, None) => Self::from_name(name),
        }
    }

    pub fn name(&self) -> &Name {
        match self {
            Self::Unit(name)
            | Self::SelfTagged(name)
            | Self::Data(name, _)
            | Self::Streaming(name, ..) => name.name(),
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

    pub fn stream_relation(&self) -> Option<StreamRelation> {
        match self {
            Self::Streaming(_, _, keyword, stream_name) => {
                Some(keyword.into_stream_relation(stream_name.name().clone()))
            }
            Self::Unit(_) | Self::SelfTagged(_) | Self::Data(_, _) => None,
        }
    }

    fn payload_value(&self) -> Option<&SourceVariantPayload> {
        match self {
            Self::Data(_, payload) | Self::Streaming(_, payload, _, _) => Some(payload),
            Self::Unit(_) | Self::SelfTagged(_) => None,
        }
    }

    fn to_enum_variant(
        &self,
        resolver: &impl SourceVariantResolver,
        namespace: Option<&Name>,
    ) -> Result<EnumVariant, SchemaError> {
        let name = self.name().clone();
        let payload = match self {
            Self::SelfTagged(_) => Some(resolver.resolve_name(namespace, &name)),
            Self::Data(_, SourceVariantPayload::Reference(reference))
            | Self::Streaming(_, SourceVariantPayload::Reference(reference), _, _) => {
                Some(resolver.resolve_reference(namespace, reference))
            }
            Self::Data(_, SourceVariantPayload::Declaration(_))
            | Self::Streaming(_, SourceVariantPayload::Declaration(_), _, _) => {
                Some(resolver.resolve_name(namespace, &name))
            }
            Self::Unit(_) if resolver.resolves_variant_payload(&name) => {
                Some(resolver.resolve_name(namespace, &name))
            }
            Self::Unit(_) => None,
        };
        let variant = EnumVariant::new(name, payload);
        Ok(match self.stream_relation() {
            Some(relation) => variant.with_stream_relation(relation),
            None => variant,
        })
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

/// A PascalCase schema symbol at a variant-name or stream-name position. It owns
/// the lowered `Name` and decodes itself from a bare PascalCase atom, so the
/// `SourceVariantSignature` derive can recurse into each name field.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SourceVariantName(Name);

impl SourceVariantName {
    pub fn new(name: Name) -> Self {
        Self(name)
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
    fn from_blocks(blocks: &[Block]) -> Result<Self, SchemaError> {
        if let Ok(reference) = SourceReference::from_blocks(blocks) {
            return Ok(Self::Reference(reference));
        }
        match blocks {
            [block] => SourceDeclarationValue::from_block(block).map(Self::Declaration),
            _ => Err(SchemaError::ExpectedSyntaxReference {
                found: blocks
                    .iter()
                    .map(Block::reemit_fallback)
                    .collect::<Vec<_>>()
                    .join(" "),
            }),
        }
    }

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
        Self::from_blocks(std::slice::from_ref(block)).map_err(StructuralMacroError::MatchedNode)
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

/// The `opens` / `belongs` discriminator that precedes a stream name in a
/// streaming variant signature. It is a keyword structural macro node so the
/// `SourceVariantSignature` derive decodes the marker recursively rather than
/// matching a literal string by hand.
#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    nota::StructuralMacroNode,
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
)]
pub enum StreamRelationKeyword {
    #[shape(keyword = "opens")]
    Opens,
    #[shape(keyword = "belongs")]
    Belongs,
}

impl StreamRelationKeyword {
    fn from_block_if_keyword(block: &Block) -> Option<Self> {
        match block.demote_to_string()? {
            "opens" => Some(Self::Opens),
            "belongs" => Some(Self::Belongs),
            _ => None,
        }
    }

    fn into_stream_relation(self, stream_name: Name) -> StreamRelation {
        match self {
            Self::Opens => StreamRelation::Opens(stream_name),
            Self::Belongs => StreamRelation::Belongs(stream_name),
        }
    }
}

impl From<&StreamRelation> for StreamRelationKeyword {
    fn from(relation: &StreamRelation) -> Self {
        match relation {
            StreamRelation::Opens(_) => Self::Opens,
            StreamRelation::Belongs(_) => Self::Belongs,
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
pub enum SourceReference {
    Plain(Name),
    FixedBytes(u64),
    Vector(#[rkyv(omit_bounds)] Box<SourceReference>),
    Optional(#[rkyv(omit_bounds)] Box<SourceReference>),
    ScopeOf(#[rkyv(omit_bounds)] Box<SourceReference>),
    Map(
        #[rkyv(omit_bounds)] Box<SourceReference>,
        #[rkyv(omit_bounds)] Box<SourceReference>,
    ),
    Application {
        head: Name,
        #[rkyv(omit_bounds)]
        arguments: Vec<SourceReference>,
    },
}

impl SourceReference {
    pub fn from_block(block: &Block) -> Result<Self, SchemaError> {
        Self::from_blocks(std::slice::from_ref(block))
    }

    fn from_blocks(blocks: &[Block]) -> Result<Self, SchemaError> {
        let mut reader = SourceReferenceReader::new(blocks);
        let reference = reader.read_reference()?;
        reader.expect_finished()?;
        Ok(reference)
    }

    fn from_blocks_at(blocks: &[Block], index: &mut usize) -> Result<Self, SchemaError> {
        let mut reader = SourceReferenceReader::new_at(blocks, *index);
        let reference = reader.read_reference()?;
        *index = reader.cursor();
        Ok(reference)
    }

    fn from_atom_with_following(
        text: &str,
        blocks: &[Block],
        index: &mut usize,
    ) -> Result<Self, SchemaError> {
        let mut reader = SourceReferenceReader::new_atom_at(text, blocks, *index);
        let reference = reader.read_reference()?;
        *index = reader.cursor();
        Ok(reference)
    }

    pub fn from_type_reference(reference: &TypeReference) -> Self {
        match reference {
            TypeReference::String => Self::Plain(Name::new("String")),
            TypeReference::Integer => Self::Plain(Name::new("Integer")),
            TypeReference::Boolean => Self::Plain(Name::new("Boolean")),
            TypeReference::Path => Self::Plain(Name::new("Path")),
            TypeReference::Bytes => Self::Plain(Name::new("Bytes")),
            TypeReference::FixedBytes(width) => Self::FixedBytes(*width),
            TypeReference::Plain(name) => Self::Plain(name.clone()),
            TypeReference::Vector(reference) => {
                Self::Vector(Box::new(Self::from_type_reference(reference)))
            }
            TypeReference::Map(key, value) => Self::Map(
                Box::new(Self::from_type_reference(key)),
                Box::new(Self::from_type_reference(value)),
            ),
            TypeReference::Optional(reference) => {
                Self::Optional(Box::new(Self::from_type_reference(reference)))
            }
            TypeReference::ScopeOf(reference) => {
                Self::ScopeOf(Box::new(Self::from_type_reference(reference)))
            }
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
            Self::FixedBytes(_)
            | Self::Vector(_)
            | Self::Optional(_)
            | Self::ScopeOf(_)
            | Self::Map(..)
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
        match reference {
            nota::TypeReference::Named(name) => Self::Plain(Name::new(*name)),
            nota::TypeReference::Vector(element) => {
                Self::Vector(Box::new(Self::from_instance_reference(element)))
            }
            nota::TypeReference::Optional(inner) => {
                Self::Optional(Box::new(Self::from_instance_reference(inner)))
            }
            nota::TypeReference::Map(key, value) => Self::Map(
                Box::new(Self::from_instance_reference(key)),
                Box::new(Self::from_instance_reference(value)),
            ),
            nota::TypeReference::FixedBytes(width) => Self::FixedBytes(*width as u64),
        }
    }

    pub fn to_schema_text(&self) -> String {
        match self {
            Self::Plain(name) => name.to_nota(),
            Self::FixedBytes(width) => Self::invocation_schema_text(
                &Name::new("Bytes"),
                &[&Self::Plain(Name::new(width.to_string()))],
            ),
            Self::Vector(reference) => {
                Self::invocation_schema_text(&Name::new("Vector"), &[reference.as_ref()])
            }
            Self::Optional(reference) => {
                Self::invocation_schema_text(&Name::new("Optional"), &[reference.as_ref()])
            }
            Self::ScopeOf(reference) => {
                Self::invocation_schema_text(&Name::new("ScopeOf"), &[reference.as_ref()])
            }
            Self::Map(key, value) => {
                Self::invocation_schema_text(&Name::new("Map"), &[key.as_ref(), value.as_ref()])
            }
            Self::Application { head, arguments } => {
                Self::invocation_schema_text(head, &arguments.iter().collect::<Vec<_>>())
            }
        }
    }

    fn invocation_schema_text(head: &Name, arguments: &[&SourceReference]) -> String {
        match arguments {
            [argument] if !argument.needs_group_as_single_argument() => {
                format!("{}.{}", head.to_nota(), argument.to_schema_text())
            }
            [argument] => format!("{}.({})", head.to_nota(), argument.to_schema_text()),
            _ => {
                let argument_text = arguments
                    .iter()
                    .map(|argument| argument.to_schema_text())
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("{}.({argument_text})", head.to_nota())
            }
        }
    }

    fn needs_group_as_single_argument(&self) -> bool {
        match self {
            Self::Map(..) => true,
            Self::Application { arguments, .. } => arguments.len() != 1,
            Self::Plain(_)
            | Self::FixedBytes(_)
            | Self::Vector(_)
            | Self::Optional(_)
            | Self::ScopeOf(_) => false,
        }
    }

    fn derived_field_name(&self) -> Name {
        match self {
            Self::Plain(name) => match name.as_str() {
                "String" => Name::new("string"),
                "Integer" => Name::new("integer"),
                "Boolean" => Name::new("boolean"),
                "Path" => Name::new("path"),
                "Bytes" => Name::new("bytes"),
                _ => Name::new(name.field_name()),
            },
            Self::FixedBytes(_) => Name::new("bytes"),
            Self::Vector(reference) => {
                Name::new(format!("{}_vector", reference.derived_field_name()))
            }
            Self::Optional(reference) => {
                Name::new(format!("optional_{}", reference.derived_field_name()))
            }
            Self::ScopeOf(reference) => {
                Name::new(format!("{}_scope", reference.derived_field_name()))
            }
            Self::Map(key, value) => Name::new(format!(
                "{}_by_{}",
                value.derived_field_name(),
                key.derived_field_name()
            )),
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

    fn to_type_reference(&self) -> TypeReference {
        match self {
            Self::Plain(name) => TypeReference::from_name(name.clone()),
            Self::FixedBytes(width) => TypeReference::FixedBytes(*width),
            Self::Vector(reference) => {
                TypeReference::Vector(Box::new(reference.to_type_reference()))
            }
            Self::Optional(reference) => {
                TypeReference::Optional(Box::new(reference.to_type_reference()))
            }
            Self::ScopeOf(reference) => {
                TypeReference::ScopeOf(Box::new(reference.to_type_reference()))
            }
            Self::Map(key, value) => TypeReference::Map(
                Box::new(key.to_type_reference()),
                Box::new(value.to_type_reference()),
            ),
            Self::Application { head, arguments } => TypeReference::Application {
                head: crate::ApplicationHead::Local(head.clone()),
                arguments: arguments.iter().map(Self::to_type_reference).collect(),
            },
        }
    }
}

struct SourceReferenceReader<'block> {
    blocks: &'block [Block],
    cursor: usize,
    pending_atom: Option<String>,
}

impl<'block> SourceReferenceReader<'block> {
    fn new(blocks: &'block [Block]) -> Self {
        Self {
            blocks,
            cursor: 0,
            pending_atom: None,
        }
    }

    fn new_at(blocks: &'block [Block], index: usize) -> Self {
        Self {
            blocks,
            cursor: index,
            pending_atom: None,
        }
    }

    fn new_atom_at(text: &str, blocks: &'block [Block], index: usize) -> Self {
        Self {
            blocks,
            cursor: index,
            pending_atom: Some(text.to_owned()),
        }
    }

    fn cursor(&self) -> usize {
        self.cursor
    }

    fn read_reference(&mut self) -> Result<SourceReference, SchemaError> {
        if let Some(text) = self.pending_atom.take() {
            return self.read_atom_text(&text);
        }
        let Some(block) = self.blocks.get(self.cursor) else {
            return Err(SchemaError::ExpectedSyntaxReferenceArity {
                form: "generic reference",
                expected: "a reference atom or dotted argument group",
                found: 0,
            });
        };
        match block {
            Block::Atom(atom) => {
                self.cursor += 1;
                self.read_atom_text(atom.text())
            }
            _ => Err(SchemaError::ExpectedSyntaxReference {
                found: block.reemit_fallback(),
            }),
        }
    }

    fn read_atom_text(&mut self, text: &str) -> Result<SourceReference, SchemaError> {
        let Some(prefix) = text.strip_suffix('.') else {
            return SourceDottedReference::new(text).without_group();
        };
        let Some(arguments_block) = self.blocks.get(self.cursor) else {
            return Err(SchemaError::ExpectedSyntaxReferenceArity {
                form: "dotted generic invocation",
                expected: "a parenthesized argument group after the trailing dot",
                found: 1,
            });
        };
        self.cursor += 1;
        let arguments = Self::arguments_from_block(arguments_block)?;
        SourceDottedReference::new(prefix).with_group(arguments)
    }

    fn arguments_from_block(block: &'block Block) -> Result<Vec<SourceReference>, SchemaError> {
        let body = NotaBody::from_delimited(block, Delimiter::Parenthesis, "generic arguments")?;
        let mut reader = Self::new(body.root_objects());
        let mut arguments = Vec::new();
        while !reader.is_finished() {
            arguments.push(reader.read_reference()?);
        }
        Ok(arguments)
    }

    fn is_finished(&self) -> bool {
        self.pending_atom.is_none() && self.cursor >= self.blocks.len()
    }

    fn expect_finished(&self) -> Result<(), SchemaError> {
        if self.is_finished() {
            return Ok(());
        }
        Err(SchemaError::ExpectedSyntaxReferenceArity {
            form: "dotted generic reference",
            expected: "one complete reference",
            found: self.blocks.len() - self.cursor + usize::from(self.pending_atom.is_some()),
        })
    }
}

struct SourceDottedReference<'text> {
    text: &'text str,
}

impl<'text> SourceDottedReference<'text> {
    fn new(text: &'text str) -> Self {
        Self { text }
    }

    fn without_group(&self) -> Result<SourceReference, SchemaError> {
        let segments = self.segments()?;
        Self::nest_unary(&segments)
    }

    fn with_group(&self, arguments: Vec<SourceReference>) -> Result<SourceReference, SchemaError> {
        let segments = self.segments()?;
        Self::nest_grouped(&segments, arguments)
    }

    fn segments(&self) -> Result<Vec<Name>, SchemaError> {
        if self.text.is_empty() {
            return Err(SchemaError::ExpectedSyntaxReference {
                found: "empty dotted generic head".to_owned(),
            });
        }
        let mut segments = Vec::new();
        for segment in self.text.split('.') {
            if segment.is_empty() {
                return Err(SchemaError::ExpectedSyntaxReference {
                    found: self.text.to_owned(),
                });
            }
            segments.push(Name::new(segment));
        }
        Ok(segments)
    }

    fn nest_unary(segments: &[Name]) -> Result<SourceReference, SchemaError> {
        let Some((head, tail)) = segments.split_first() else {
            return Err(SchemaError::ExpectedSyntaxReference {
                found: "empty dotted generic head".to_owned(),
            });
        };
        if tail.is_empty() {
            return Ok(SourceReference::Plain(head.clone()));
        }
        Ok(SourceReference::Application {
            head: head.clone(),
            arguments: vec![Self::nest_unary(tail)?],
        })
    }

    fn nest_grouped(
        segments: &[Name],
        arguments: Vec<SourceReference>,
    ) -> Result<SourceReference, SchemaError> {
        let Some((head, tail)) = segments.split_first() else {
            return Err(SchemaError::ExpectedSyntaxReference {
                found: "empty dotted generic head".to_owned(),
            });
        };
        if tail.is_empty() {
            return Ok(SourceReference::Application {
                head: head.clone(),
                arguments,
            });
        }
        Ok(SourceReference::Application {
            head: head.clone(),
            arguments: vec![Self::nest_grouped(tail, arguments)?],
        })
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
            SourceReference::FixedBytes(width) => TypeReference::FixedBytes(*width),
            SourceReference::Vector(reference) => {
                TypeReference::Vector(Box::new(self.resolve_reference(namespace, reference)))
            }
            SourceReference::Optional(reference) => {
                TypeReference::Optional(Box::new(self.resolve_reference(namespace, reference)))
            }
            SourceReference::ScopeOf(reference) => {
                TypeReference::ScopeOf(Box::new(self.resolve_reference(namespace, reference)))
            }
            SourceReference::Map(key, value) => TypeReference::Map(
                Box::new(self.resolve_reference(namespace, key)),
                Box::new(self.resolve_reference(namespace, value)),
            ),
            SourceReference::Application { head, arguments } => {
                let visible_head = self.visible_name(namespace, head);
                let resolved_arguments = arguments
                    .iter()
                    .map(|argument| self.resolve_reference(namespace, argument))
                    .collect::<Vec<_>>();
                if let Some(definition) = self.generic_definition_named(&visible_head)
                    && definition.builtin().parameter_count() == resolved_arguments.len()
                {
                    if matches!(definition.builtin(), SourceGenericBuiltin::Frame { .. }) {
                        return TypeReference::Application {
                            head: crate::ApplicationHead::Local(visible_head),
                            arguments: resolved_arguments,
                        };
                    }
                    return self.resolve_generic_application(
                        visible_head,
                        definition.builtin(),
                        resolved_arguments,
                    );
                }
                TypeReference::Application {
                    head: crate::ApplicationHead::Local(visible_head),
                    arguments: resolved_arguments,
                }
            }
        }
    }

    fn generic_definition_named(&self, _name: &Name) -> Option<&SourceGenericDefinition> {
        None
    }

    fn resolve_generic_application(
        &self,
        visible_head: Name,
        builtin: &SourceGenericBuiltin,
        arguments: Vec<TypeReference>,
    ) -> TypeReference {
        match builtin {
            SourceGenericBuiltin::Vector => TypeReference::Vector(Box::new(
                arguments.into_iter().next().expect("arity checked"),
            )),
            SourceGenericBuiltin::Optional => TypeReference::Optional(Box::new(
                arguments.into_iter().next().expect("arity checked"),
            )),
            SourceGenericBuiltin::ScopeOf => TypeReference::ScopeOf(Box::new(
                arguments.into_iter().next().expect("arity checked"),
            )),
            SourceGenericBuiltin::Map => {
                let mut arguments = arguments.into_iter();
                TypeReference::Map(
                    Box::new(arguments.next().expect("arity checked")),
                    Box::new(arguments.next().expect("arity checked")),
                )
            }
            SourceGenericBuiltin::FixedBytes => match arguments.as_slice() {
                [TypeReference::Plain(width)] => width
                    .as_str()
                    .parse::<u64>()
                    .map(TypeReference::FixedBytes)
                    .unwrap_or_else(|_| TypeReference::Application {
                        head: crate::ApplicationHead::Local(visible_head),
                        arguments,
                    }),
                _ => TypeReference::Application {
                    head: crate::ApplicationHead::Local(visible_head),
                    arguments,
                },
            },
            SourceGenericBuiltin::Frame { .. } => TypeReference::Application {
                head: crate::ApplicationHead::Local(visible_head),
                arguments,
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
struct ExplicitSourceVariantResolver<'resolver> {
    resolver: &'resolver SourceTypeResolver,
}

impl<'resolver> ExplicitSourceVariantResolver<'resolver> {
    fn new(resolver: &'resolver SourceTypeResolver) -> Self {
        Self { resolver }
    }
}

impl SourceVariantResolver for ExplicitSourceVariantResolver<'_> {
    fn generic_definition_named(&self, name: &Name) -> Option<&SourceGenericDefinition> {
        self.resolver.generic_definition_named(name)
    }

    fn resolves_variant_payload(&self, _name: &Name) -> bool {
        false
    }

    fn resolves_type_name(&self, name: &Name) -> bool {
        self.resolver.resolves_type_name(name)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceRootVariantResolver<'resolver> {
    namespace: &'resolver SourceLoweredNamespace,
    resolver: &'resolver SourceTypeResolver,
}

impl<'resolver> SourceRootVariantResolver<'resolver> {
    fn new(
        namespace: &'resolver SourceLoweredNamespace,
        resolver: &'resolver SourceTypeResolver,
    ) -> Self {
        Self {
            namespace,
            resolver,
        }
    }
}

impl SourceVariantResolver for SourceRootVariantResolver<'_> {
    fn generic_definition_named(&self, name: &Name) -> Option<&SourceGenericDefinition> {
        self.resolver.generic_definition_named(name)
    }

    fn resolves_variant_payload(&self, name: &Name) -> bool {
        self.namespace.resolves_variant_payload(name)
    }

    fn resolves_type_name(&self, name: &Name) -> bool {
        self.namespace.resolves_type_name(name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceTypeResolver {
    names: Vec<Name>,
    generics: SourceGenerics,
}

impl SourceTypeResolver {
    fn from_source(source: &SchemaSource) -> Self {
        let mut names = source.namespace().type_declaration_names();
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
        Self {
            names,
            generics: source.generics().clone(),
        }
    }

    fn contains(&self, name: &Name) -> bool {
        self.names.iter().any(|candidate| candidate == name)
    }
}

impl SourceVariantResolver for SourceTypeResolver {
    fn generic_definition_named(&self, name: &Name) -> Option<&SourceGenericDefinition> {
        self.generics.definition_named(name)
    }

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
    /// Standalone impl blocks lowered from body-optional `TypeName {| … |}`
    /// entries. They mint no type declaration; they attach a catalog to a
    /// type declared elsewhere, surfaced through `TrueSchema::impl_blocks`.
    impl_blocks: Vec<ImplBlock>,
}

impl SourceLoweredNamespace {
    fn from_source(
        source: &SourceNamespace,
        resolver: &SourceTypeResolver,
    ) -> Result<Self, SchemaError> {
        let mut namespace = Self {
            declarations: Vec::new(),
            impl_blocks: Vec::new(),
        };
        namespace.push_source_namespace(source, resolver, None)?;
        Ok(namespace)
    }

    fn push_source_namespace(
        &mut self,
        source: &SourceNamespace,
        resolver: &SourceTypeResolver,
        namespace: Option<&Name>,
    ) -> Result<(), SchemaError> {
        for entry in source.entries() {
            match entry.namespace() {
                Some(nested) => {
                    let nested_namespace = entry.namespace_name(namespace);
                    self.push_source_namespace(nested, resolver, Some(&nested_namespace))?;
                }
                None => {
                    // A reserved scalar name (`String`, `Integer`, …) cannot be
                    // user-declared at the namespace declaration position. The
                    // field-position machinery already gates these names; this
                    // is the matching declaration-position gate, so the single
                    // lowering path rejects `{ String Integer }` the same way
                    // the retired second engine did.
                    if SourceField::is_reserved_scalar_name(entry.name()) {
                        return Err(SchemaError::ReservedScalarTypeName {
                            name: entry.name().as_str().to_owned(),
                        });
                    }
                    if let Some(block) = entry.to_impl_block(resolver, namespace) {
                        self.impl_blocks.push(block);
                    }
                    self.push_public_group(entry.to_declaration_group(resolver, namespace)?)?;
                }
            }
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
    /// Declared type parameters carried from a parameterized entry head.
    /// They attach to the group's primary declaration; the inline helper
    /// declarations (public / private) are not parameterized.
    parameters: Vec<Name>,
    /// The lowered trailing `{| … |}` catalog. It attaches to the group's
    /// primary declaration, beside the parameters. Empty for an entry with
    /// no trailing impl block.
    impls: ImplCatalog,
}

impl SourceDeclarationGroup {
    fn empty() -> Self {
        Self {
            public: Vec::new(),
            private: Vec::new(),
            primary: None,
            parameters: Vec::new(),
            impls: ImplCatalog::empty(),
        }
    }

    fn primary(primary: TypeDeclaration) -> Self {
        Self {
            public: Vec::new(),
            private: Vec::new(),
            primary: Some(primary),
            parameters: Vec::new(),
            impls: ImplCatalog::empty(),
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
            impls: ImplCatalog::empty(),
        }
    }

    /// Attach declared type parameters to the group's primary
    /// declaration. The binders belong to the named declaration the entry
    /// head introduced, not to its inline helpers.
    fn with_parameters(mut self, parameters: Vec<Name>) -> Self {
        self.parameters = parameters;
        self
    }

    /// Attach the lowered impl catalog to the group's primary declaration.
    /// Like parameters, the catalog belongs to the named declaration the
    /// entry head introduced, not to its inline helpers.
    fn with_impls(mut self, impls: ImplCatalog) -> Self {
        self.impls = impls;
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
            declarations.push(
                Declaration::public(primary)
                    .with_parameters(self.parameters)
                    .with_impls(self.impls),
            );
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

    fn into_name(self) -> Name {
        Name::new(self.0)
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
            Block::Atom(atom) => format!("atom {}", atom.text()),
        }
    }
}
