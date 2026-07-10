use std::{
    fs,
    path::{Path, PathBuf},
};

use nota::{
    Block, Delimiter, Document, MacroCandidate, NotaEncode, NotaSource, PositionPredicate,
    StructuralMacroError, StructuralMacroNode, StructuralVariant,
};

use crate::{
    EnumDeclaration, EnumVariant, FieldDeclaration, MacroContext, MacroObject, MacroOutput,
    MacroPair, MacroPosition, MacroRegistry, Name, NewtypeDeclaration, SchemaError,
    SchemaMacroHandler, StructDeclaration, TypeDeclaration, TypeReference,
    macros::SchemaBlockExt,
};

#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    nota::NotaDecode,
    nota::NotaEncode,
    Clone,
    Debug,
    Eq,
    PartialEq,
)]
pub struct MacroLibrary {
    source_entries: Vec<MacroLibrarySourceEntry>,
}

impl MacroLibrary {
    pub fn new(source_entries: Vec<MacroLibrarySourceEntry>) -> Self {
        Self { source_entries }
    }

    pub fn builtin() -> Result<Self, SchemaError> {
        Self::from_nota_source(include_str!("../schemas/builtin-macros.macro-library"))
    }

    pub fn builtin_source() -> Result<Self, SchemaError> {
        Self::from_source(include_str!("../schemas/builtin-macros.schema"))
    }

    /// Read the hand-authored bootstrap notation. The document body is a
    /// vector of typed source entries; every root object decodes through the
    /// structural macro node codec, never through positional hand parsing.
    pub fn from_source(source: &str) -> Result<Self, SchemaError> {
        let document = Document::parse(source)?;
        let entries =
            Vec::<MacroLibrarySourceEntry>::from_structural_candidate(MacroCandidate::new(
                MacroLibrarySourceEntry::structural_position(),
                document.root_objects().iter().collect(),
            ))?;
        Ok(Self::new(entries))
    }

    /// Write the same bootstrap notation back out, one entry per line.
    pub fn to_source(&self) -> String {
        self.source_entries
            .iter()
            .map(StructuralMacroNode::to_structural_nota)
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn source_entries(&self) -> &[MacroLibrarySourceEntry] {
        &self.source_entries
    }

    pub fn definitions(&self) -> Vec<&SchemaMacro> {
        self.source_entries
            .iter()
            .map(MacroLibrarySourceEntry::definition)
            .collect()
    }

    pub fn into_macros(self) -> Vec<Box<dyn SchemaMacroHandler>> {
        self.source_entries
            .into_iter()
            .map(MacroLibrarySourceEntry::into_schema_macro)
            .collect()
    }

    pub fn from_nota_source(source: &str) -> Result<Self, SchemaError> {
        NotaSource::new(source).parse::<Self>().map_err(Into::into)
    }

    pub fn to_nota_source(&self) -> String {
        NotaEncode::to_nota(self)
    }

    pub fn from_binary_bytes(bytes: &[u8]) -> Result<Self, SchemaError> {
        rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes).map_err(|_| SchemaError::ArchiveDecode)
    }

    pub fn to_binary_bytes(&self) -> Result<Vec<u8>, SchemaError> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .map(|bytes| bytes.to_vec())
            .map_err(|_| SchemaError::ArchiveEncode)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MacroLibraryArtifact {
    library: MacroLibrary,
}

impl MacroLibraryArtifact {
    pub fn new(library: MacroLibrary) -> Self {
        Self { library }
    }

    pub fn library(&self) -> &MacroLibrary {
        &self.library
    }

    pub fn into_library(self) -> MacroLibrary {
        self.library
    }

    pub fn from_nota_source(source: &str) -> Result<Self, SchemaError> {
        MacroLibrary::from_nota_source(source).map(Self::new)
    }

    pub fn to_nota_source(&self) -> String {
        self.library.to_nota_source()
    }

    pub fn from_binary_bytes(bytes: &[u8]) -> Result<Self, SchemaError> {
        MacroLibrary::from_binary_bytes(bytes).map(Self::new)
    }

    pub fn to_binary_bytes(&self) -> Result<Vec<u8>, SchemaError> {
        self.library.to_binary_bytes()
    }

    pub fn read_nota_file(path: impl AsRef<Path>) -> Result<Self, SchemaError> {
        let artifact_path = MacroLibraryArtifactPath::new(path.as_ref());
        let source = fs::read_to_string(artifact_path.path())
            .map_err(|error| artifact_path.io_error(error))?;
        Self::from_nota_source(&source)
    }

    pub fn write_nota_file(&self, path: impl AsRef<Path>) -> Result<(), SchemaError> {
        let artifact_path = MacroLibraryArtifactPath::new(path.as_ref());
        fs::write(artifact_path.path(), self.to_nota_source())
            .map_err(|error| artifact_path.io_error(error))
    }

    pub fn read_binary_file(path: impl AsRef<Path>) -> Result<Self, SchemaError> {
        let artifact_path = MacroLibraryArtifactPath::new(path.as_ref());
        let bytes =
            fs::read(artifact_path.path()).map_err(|error| artifact_path.io_error(error))?;
        Self::from_binary_bytes(&bytes)
    }

    pub fn write_binary_file(&self, path: impl AsRef<Path>) -> Result<(), SchemaError> {
        let artifact_path = MacroLibraryArtifactPath::new(path.as_ref());
        let bytes = self.to_binary_bytes()?;
        fs::write(artifact_path.path(), bytes).map_err(|error| artifact_path.io_error(error))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MacroLibraryArtifactPath {
    path: PathBuf,
}

impl MacroLibraryArtifactPath {
    fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn io_error(&self, error: std::io::Error) -> SchemaError {
        SchemaError::Io {
            path: self.path.display().to_string(),
            reason: error.to_string(),
        }
    }
}

#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    nota::NotaDecode,
    nota::NotaEncode,
    Clone,
    Debug,
    Eq,
    PartialEq,
)]
pub struct SchemaMacro {
    macro_name: Name,
    macro_position: MacroPosition,
    macro_pattern: MacroPattern,
    macro_template: MacroTemplate,
}

impl SchemaMacro {
    pub fn new(
        macro_name: Name,
        macro_position: MacroPosition,
        macro_pattern: MacroPattern,
        macro_template: MacroTemplate,
    ) -> Self {
        Self {
            macro_name,
            macro_position,
            macro_pattern,
            macro_template,
        }
    }

    pub fn name(&self) -> &Name {
        &self.macro_name
    }

    pub fn position(&self) -> MacroPosition {
        self.macro_position
    }

    pub fn pattern(&self) -> &MacroPattern {
        &self.macro_pattern
    }

    pub fn template(&self) -> &MacroTemplate {
        &self.macro_template
    }

    pub fn capture_names(&self) -> Vec<String> {
        self.macro_pattern.capture_names()
    }

    fn into_executable_definition(self) -> ExecutableMacroDefinition {
        ExecutableMacroDefinition {
            name: self.macro_name,
            position: self.macro_position,
            pattern: self.macro_pattern,
            template: self.macro_template,
        }
    }
}

/// The bootstrap body of one macro definition: name, position, pattern, and
/// template as four ordered objects after the `SchemaMacro` head. Each field
/// decodes through its own typed node; the slice pattern carries the arity.
impl StructuralMacroNode for SchemaMacro {
    type Error = SchemaError;

    fn structural_position() -> PositionPredicate {
        PositionPredicate::named("macro definition body")
    }

    fn structural_variants() -> Vec<StructuralVariant> {
        Vec::new()
    }

    fn from_structural_candidate(
        candidate: MacroCandidate<'_>,
    ) -> Result<Self, StructuralMacroError<Self::Error>> {
        match candidate.blocks() {
            [name, position, pattern, template] => Ok(Self {
                macro_name: name
                    .schema_name()
                    .map_err(StructuralMacroError::MatchedNode)?,
                macro_position: MacroPosition::from_structural_block(position)
                    .map_err(|error| StructuralMacroError::MatchedNode(SchemaError::from(error)))?,
                macro_pattern: MacroPattern::from_structural_block(pattern)
                    .map_err(|error| StructuralMacroError::MatchedNode(SchemaError::from(error)))?,
                macro_template: MacroTemplate::from_structural_block(template)
                    .map_err(|error| StructuralMacroError::MatchedNode(SchemaError::from(error)))?,
            }),
            blocks => Err(StructuralMacroError::MatchedNode(
                SchemaError::ExpectedMacroDefinition {
                    found: format!("macro definition body with {} objects", blocks.len()),
                },
            )),
        }
    }

    fn to_structural_nota(&self) -> String {
        format!(
            "{} {} {} {}",
            self.macro_name.to_nota(),
            self.macro_position.to_structural_nota(),
            self.macro_pattern.to_structural_nota(),
            self.macro_template.to_structural_nota(),
        )
    }
}

#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    nota::NotaDecode,
    nota::NotaEncode,
    Clone,
    Debug,
    Eq,
    PartialEq,
)]
pub struct MacroPattern {
    object: MacroPatternObject,
}

impl MacroPattern {
    pub fn new(object: MacroPatternObject) -> Self {
        Self { object }
    }

    pub fn object(&self) -> &MacroPatternObject {
        &self.object
    }

    fn from_block(object: &Block) -> Result<Self, SchemaError> {
        Ok(Self {
            object: MacroPatternObject::from_block(object)?,
        })
    }

    fn captures(&self, object: MacroObject<'_>) -> Result<Option<MacroBindings>, SchemaError> {
        let mut bindings = MacroBindings::default();
        let matched = match object {
            MacroObject::Block(block) => self.object.matches_block(block, &mut bindings)?,
            MacroObject::Pair(pair) => self.object.matches_pair(pair, &mut bindings)?,
        };
        if matched {
            Ok(Some(bindings))
        } else {
            Ok(None)
        }
    }

    fn capture_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        self.object.push_capture_names(&mut names);
        names
    }
}

/// The pattern position of a bootstrap macro definition. The pattern object
/// is a structural mirror of one NOTA object with `$name` / `$*name` capture
/// atoms, so the leaf codec accepts any delimiter shape and encodes the same
/// sigil notation back out.
impl StructuralMacroNode for MacroPattern {
    type Error = SchemaError;

    fn structural_position() -> PositionPredicate {
        PositionPredicate::named("macro pattern")
    }

    fn structural_variants() -> Vec<StructuralVariant> {
        Vec::new()
    }

    fn from_structural_block(block: &Block) -> Result<Self, StructuralMacroError<Self::Error>> {
        Self::from_block(block).map_err(StructuralMacroError::MatchedNode)
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
        self.object.to_source_notation()
    }
}

#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    nota::NotaDecode,
    nota::NotaEncode,
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
pub enum MacroPatternObject {
    Capture(String),
    RestCapture(String),
    Atom(String),
    Delimited(#[rkyv(omit_bounds)] Box<MacroPatternDelimited>),
}

impl MacroPatternObject {
    fn from_block(object: &Block) -> Result<Self, SchemaError> {
        if let Some(text) = object.demote_to_string() {
            if let Some(capture) = CaptureName::from_token(text)? {
                if capture.rest {
                    return Ok(Self::RestCapture(capture.name));
                }
                return Ok(Self::Capture(capture.name));
            }
            return Ok(Self::Atom(text.to_owned()));
        }
        match object {
            Block::Delimited {
                delimiter,
                root_objects,
                ..
            } => {
                let mut children = Vec::new();
                for child in root_objects {
                    children.push(Self::from_block(child)?);
                }
                Ok(Self::Delimited(Box::new(MacroPatternDelimited::new(
                    MacroDelimiter::from_nota(*delimiter),
                    children,
                ))))
            }
            Block::PipeText(_) => Ok(Self::Atom(NotationBlock::new(object).compact_notation())),
            Block::Atom(_) => unreachable!("atoms are handled by demote_to_string"),
        }
    }

    fn matches_pair(
        &self,
        pair: MacroPair<'_>,
        bindings: &mut MacroBindings,
    ) -> Result<bool, SchemaError> {
        let Self::Delimited(data) = self else {
            return Ok(false);
        };
        if data.delimiter() != MacroDelimiter::Parenthesis || data.children.len() != 2 {
            return Ok(false);
        }
        Ok(data.children[0].matches_block(pair.name, bindings)?
            && data.children[1].matches_block(pair.definition, bindings)?)
    }

    fn matches_block(
        &self,
        object: &Block,
        bindings: &mut MacroBindings,
    ) -> Result<bool, SchemaError> {
        match self {
            Self::Capture(name) => bindings.bind_single(name, object),
            Self::RestCapture(_) => Ok(false),
            Self::Atom(expected) => Ok(object.demote_to_string() == Some(expected.as_str())),
            Self::Delimited(data) => match object {
                Block::Delimited {
                    delimiter,
                    root_objects,
                    ..
                } if *delimiter == data.delimiter().into_nota() => {
                    PatternChildren::new(data.children()).matches(root_objects, bindings)
                }
                _ => Ok(false),
            },
        }
    }

    fn push_capture_names(&self, names: &mut Vec<String>) {
        match self {
            Self::Capture(name) => names.push(format!("${name}")),
            Self::RestCapture(name) => names.push(format!("$*{name}")),
            Self::Delimited(data) => {
                for child in data.children() {
                    child.push_capture_names(names);
                }
            }
            Self::Atom(_) => {}
        }
    }

    fn as_rest_capture_name(&self) -> Option<&str> {
        match self {
            Self::RestCapture(name) => Some(name),
            Self::Capture(_) | Self::Atom(_) | Self::Delimited(_) => None,
        }
    }

    fn to_source_notation(&self) -> String {
        match self {
            Self::Capture(name) => format!("${name}"),
            Self::RestCapture(name) => format!("$*{name}"),
            Self::Atom(text) => text.clone(),
            Self::Delimited(data) => DelimitedNotation::new(data.delimiter().into_nota())
                .wrap_children(
                    &data
                        .children()
                        .iter()
                        .map(Self::to_source_notation)
                        .collect::<Vec<_>>(),
                ),
        }
    }
}

#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    nota::NotaDecode,
    nota::NotaEncode,
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
pub struct MacroPatternDelimited {
    delimiter: MacroDelimiter,
    #[rkyv(omit_bounds)]
    children: Vec<MacroPatternObject>,
}

impl MacroPatternDelimited {
    pub fn new(delimiter: MacroDelimiter, children: Vec<MacroPatternObject>) -> Self {
        Self {
            delimiter,
            children,
        }
    }

    pub fn delimiter(&self) -> MacroDelimiter {
        self.delimiter
    }

    pub fn children(&self) -> &[MacroPatternObject] {
        &self.children
    }
}

/// The expansion template of a macro definition, typed by output kind. The
/// head names what the macro produces, so registry consumers know the output
/// before any expansion runs, and an unknown head is rejected at decode time.
#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    nota::NotaDecode,
    nota::NotaEncode,
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
pub enum MacroTemplate {
    #[shape(head = "Type", arity = 2)]
    Type(#[rkyv(omit_bounds)] TypeTemplate),
    #[shape(head = "Fields", body)]
    Fields(#[rkyv(omit_bounds)] Vec<MacroTemplateObject>),
    #[shape(head = "Variants", body)]
    Variants(#[rkyv(omit_bounds)] Vec<MacroTemplateObject>),
    #[shape(head = "Reference", body)]
    Reference(#[rkyv(omit_bounds)] Vec<MacroTemplateObject>),
}

impl MacroTemplate {
    fn expand_output(
        &self,
        macro_name: &str,
        bindings: &MacroBindings,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<MacroOutput, SchemaError> {
        match self {
            Self::Type(template) => template
                .expand_declaration(macro_name, bindings, registry, context)
                .map(MacroOutput::Type),
            Self::Fields(objects) => {
                let mut expanded = Vec::new();
                for object in objects {
                    expanded.extend(object.expand_objects(bindings)?);
                }
                context.remember_expanded_template(
                    macro_name,
                    ExpandedNotation::headed("Fields", &expanded).text(),
                );
                MacroExpansionFields::from_objects(
                    expanded.iter().map(ObjectView::Expanded).collect(),
                )
                .lower(registry, context)
                .map(MacroOutput::Fields)
            }
            Self::Variants(objects) => {
                let mut expanded = Vec::new();
                for object in objects {
                    expanded.extend(object.expand_objects(bindings)?);
                }
                context.remember_expanded_template(
                    macro_name,
                    ExpandedNotation::headed("Variants", &expanded).text(),
                );
                MacroExpansionVariants::from_objects(
                    expanded.iter().map(ObjectView::Expanded).collect(),
                )
                .lower(registry, context)
                .map(MacroOutput::Variants)
            }
            Self::Reference(objects) => {
                let mut expanded = Vec::new();
                for object in objects {
                    expanded.extend(object.expand_objects(bindings)?);
                }
                context.remember_expanded_template(
                    macro_name,
                    ExpandedNotation::headed("Reference", &expanded).text(),
                );
                MacroExpansionReference::from_objects(
                    expanded.iter().map(ObjectView::Expanded).collect(),
                )
                .lower(registry, context)
                .map(MacroOutput::Reference)
            }
        }
    }
}

/// The payload of a `(Type ...)` template: the declaration kind is part of
/// the template's structure, so struct, enum, and newtype expansion dispatch
/// on this typed node instead of an extracted head string.
#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    nota::NotaDecode,
    nota::NotaEncode,
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
pub enum TypeTemplate {
    #[shape(head = "Struct", arity = 3)]
    Struct(
        #[rkyv(omit_bounds)] MacroTemplateObject,
        #[rkyv(omit_bounds)] MacroTemplateObject,
    ),
    #[shape(head = "Enum", arity = 3)]
    Enum(
        #[rkyv(omit_bounds)] MacroTemplateObject,
        #[rkyv(omit_bounds)] MacroTemplateObject,
    ),
    #[shape(head = "Newtype", arity = 3)]
    Newtype(
        #[rkyv(omit_bounds)] MacroTemplateObject,
        #[rkyv(omit_bounds)] MacroTemplateObject,
    ),
}

impl TypeTemplate {
    fn expand_declaration(
        &self,
        macro_name: &str,
        bindings: &MacroBindings,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<TypeDeclaration, SchemaError> {
        match self {
            Self::Struct(name, body) => {
                let name = name.expand_schema_name(bindings, "Struct name")?;
                let body = body.expand_single(bindings, "Struct body")?;
                context.remember_expanded_template(
                    macro_name,
                    format!(
                        "(Type (Struct {} {}))",
                        name.as_str(),
                        body.compact_notation()
                    ),
                );
                let body_view = ObjectView::Expanded(&body);
                MacroExpansionStructBody::new(name, body_view.root_objects())
                    .lower_type(registry, context)
            }
            Self::Enum(name, body) => {
                let name = name.expand_schema_name(bindings, "Enum name")?;
                let body = body.expand_single(bindings, "Enum body")?;
                context.remember_expanded_template(
                    macro_name,
                    format!(
                        "(Type (Enum {} {}))",
                        name.as_str(),
                        body.compact_notation()
                    ),
                );
                let body_view = ObjectView::Expanded(&body);
                let variants = MacroExpansionVariants::from_objects(body_view.root_objects())
                    .lower(registry, context)?;
                Ok(TypeDeclaration::Enum(EnumDeclaration::new(name, variants)))
            }
            Self::Newtype(name, reference) => {
                let name = name.expand_schema_name(bindings, "Newtype name")?;
                let reference = reference.expand_single(bindings, "Newtype reference")?;
                context.remember_expanded_template(
                    macro_name,
                    format!(
                        "(Type (Newtype {} {}))",
                        name.as_str(),
                        reference.compact_notation()
                    ),
                );
                let reference =
                    ObjectView::Expanded(&reference).type_reference(registry, context)?;
                Ok(TypeDeclaration::Newtype(NewtypeDeclaration::new(
                    name, reference,
                )))
            }
        }
    }
}

/// Diagnostic notation of an expanded template: the template head plus the
/// expanded payload objects, kept only as a `MacroContext` trace string.
#[derive(Clone, Debug)]
struct ExpandedNotation {
    text: String,
}

impl ExpandedNotation {
    fn headed(head: &str, expanded: &[ExpandedObject]) -> Self {
        let mut parts = vec![head.to_owned()];
        parts.extend(expanded.iter().map(ExpandedObject::compact_notation));
        Self {
            text: DelimitedNotation::new(Delimiter::Parenthesis).wrap_children(&parts),
        }
    }

    fn text(&self) -> String {
        self.text.clone()
    }
}

#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    nota::NotaDecode,
    nota::NotaEncode,
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
pub enum MacroTemplateObject {
    Capture(String),
    RestCapture(String),
    Atom(String),
    Delimited(#[rkyv(omit_bounds)] Box<MacroTemplateDelimited>),
}

impl MacroTemplateObject {
    fn from_block(object: &Block) -> Result<Self, SchemaError> {
        if let Some(text) = object.demote_to_string() {
            if let Some(capture) = CaptureName::from_token(text)? {
                if capture.rest {
                    return Ok(Self::RestCapture(capture.name));
                }
                return Ok(Self::Capture(capture.name));
            }
            return Ok(Self::Atom(text.to_owned()));
        }
        match object {
            Block::Delimited {
                delimiter,
                root_objects,
                ..
            } => {
                let mut children = Vec::new();
                for child in root_objects {
                    children.push(Self::from_block(child)?);
                }
                Ok(Self::Delimited(Box::new(MacroTemplateDelimited::new(
                    MacroDelimiter::from_nota(*delimiter),
                    children,
                ))))
            }
            Block::PipeText(_) => Ok(Self::Atom(NotationBlock::new(object).compact_notation())),
            Block::Atom(_) => unreachable!("atoms are handled by demote_to_string"),
        }
    }

    fn expand_objects(&self, bindings: &MacroBindings) -> Result<Vec<ExpandedObject>, SchemaError> {
        match self {
            Self::Capture(name) => Ok(vec![ExpandedObject::Captured(
                bindings.single(name)?.clone(),
            )]),
            Self::RestCapture(name) => Ok(bindings
                .repeated(name)?
                .iter()
                .cloned()
                .map(ExpandedObject::Captured)
                .collect()),
            Self::Atom(text) => Ok(vec![ExpandedObject::Atom(text.clone())]),
            Self::Delimited(data) => {
                let mut expanded_children = Vec::new();
                for child in data.children() {
                    expanded_children.extend(child.expand_objects(bindings)?);
                }
                Ok(vec![ExpandedObject::Delimited {
                    delimiter: data.delimiter().into_nota(),
                    children: expanded_children,
                }])
            }
        }
    }

    fn expand_single(
        &self,
        bindings: &MacroBindings,
        position: &'static str,
    ) -> Result<ExpandedObject, SchemaError> {
        let mut objects = self.expand_objects(bindings)?;
        if objects.len() != 1 {
            return Err(SchemaError::ExpectedTemplateObjectCount {
                position,
                expected: 1,
                found: objects.len(),
            });
        }
        Ok(objects.pop().expect("length checked"))
    }

    fn expand_schema_name(
        &self,
        bindings: &MacroBindings,
        position: &'static str,
    ) -> Result<Name, SchemaError> {
        self.expand_single(bindings, position)?.schema_name()
    }

    fn to_source_notation(&self) -> String {
        match self {
            Self::Capture(name) => format!("${name}"),
            Self::RestCapture(name) => format!("$*{name}"),
            Self::Atom(text) => text.clone(),
            Self::Delimited(data) => DelimitedNotation::new(data.delimiter().into_nota())
                .wrap_children(
                    &data
                        .children()
                        .iter()
                        .map(Self::to_source_notation)
                        .collect::<Vec<_>>(),
                ),
        }
    }
}

/// A leaf object inside a bootstrap template: a capture atom, a literal atom,
/// or a delimited tree of further template objects. Any structural shape is
/// legal here, so the leaf codec mirrors the object instead of dispatching on
/// variant shapes.
impl StructuralMacroNode for MacroTemplateObject {
    type Error = SchemaError;

    fn structural_position() -> PositionPredicate {
        PositionPredicate::named("macro template object")
    }

    fn structural_variants() -> Vec<StructuralVariant> {
        Vec::new()
    }

    fn from_structural_block(block: &Block) -> Result<Self, StructuralMacroError<Self::Error>> {
        Self::from_block(block).map_err(StructuralMacroError::MatchedNode)
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
        self.to_source_notation()
    }
}

#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    nota::NotaDecode,
    nota::NotaEncode,
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
pub struct MacroTemplateDelimited {
    delimiter: MacroDelimiter,
    #[rkyv(omit_bounds)]
    children: Vec<MacroTemplateObject>,
}

impl MacroTemplateDelimited {
    pub fn new(delimiter: MacroDelimiter, children: Vec<MacroTemplateObject>) -> Self {
        Self {
            delimiter,
            children,
        }
    }

    pub fn delimiter(&self) -> MacroDelimiter {
        self.delimiter
    }

    pub fn children(&self) -> &[MacroTemplateObject] {
        &self.children
    }
}

#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    nota::NotaDecode,
    nota::NotaEncode,
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
)]
pub enum MacroDelimiter {
    Parenthesis,
    SquareBracket,
    Brace,
    PipeParenthesis,
    PipeBrace,
}

impl MacroDelimiter {
    fn from_nota(delimiter: Delimiter) -> Self {
        match delimiter {
            Delimiter::Parenthesis => Self::Parenthesis,
            Delimiter::SquareBracket => Self::SquareBracket,
            Delimiter::Brace => Self::Brace,
            Delimiter::PipeParenthesis => Self::PipeParenthesis,
            Delimiter::PipeBrace => Self::PipeBrace,
        }
    }

    fn into_nota(self) -> Delimiter {
        match self {
            Self::Parenthesis => Delimiter::Parenthesis,
            Self::SquareBracket => Delimiter::SquareBracket,
            Self::Brace => Delimiter::Brace,
            Self::PipeParenthesis => Delimiter::PipeParenthesis,
            Self::PipeBrace => Delimiter::PipeBrace,
        }
    }
}

/// One entry of the hand-authored bootstrap source. The `SchemaMacro` head is
/// a structural variant shape, and the headed tail decodes as the definition
/// body — the same typed read path the serialized artifact uses, with no
/// positional wrapper and no variant-name string comparison.
#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    nota::NotaDecode,
    nota::NotaEncode,
    nota::StructuralMacroNode,
    Clone,
    Debug,
    Eq,
    PartialEq,
)]
pub enum MacroLibrarySourceEntry {
    #[shape(head = "SchemaMacro", body)]
    SchemaMacro(SchemaMacro),
}

impl MacroLibrarySourceEntry {
    pub fn definition(&self) -> &SchemaMacro {
        match self {
            Self::SchemaMacro(definition) => definition,
        }
    }

    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::SchemaMacro(_) => "SchemaMacro",
        }
    }

    fn into_schema_macro(self) -> Box<dyn SchemaMacroHandler> {
        match self {
            Self::SchemaMacro(definition) => Box::new(DeclarativeSchemaMacro {
                definition: definition.into_executable_definition(),
            }) as Box<dyn SchemaMacroHandler>,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExecutableMacroDefinition {
    name: Name,
    position: MacroPosition,
    pattern: MacroPattern,
    template: MacroTemplate,
}

#[derive(Clone, Copy, Debug)]
struct PatternChildren<'pattern> {
    children: &'pattern [MacroPatternObject],
}

impl<'pattern> PatternChildren<'pattern> {
    fn new(children: &'pattern [MacroPatternObject]) -> Self {
        Self { children }
    }

    fn matches(
        &self,
        objects: &[Block],
        bindings: &mut MacroBindings,
    ) -> Result<bool, SchemaError> {
        if let Some(rest_index) = self.rest_capture_index() {
            return self.matches_with_rest_capture(rest_index, objects, bindings);
        }
        if self.children.len() != objects.len() {
            return Ok(false);
        }
        for (pattern, object) in self.children.iter().zip(objects) {
            if !pattern.matches_block(object, bindings)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn rest_capture_index(&self) -> Option<usize> {
        self.children
            .iter()
            .position(|child| child.as_rest_capture_name().is_some())
    }

    fn matches_with_rest_capture(
        &self,
        rest_index: usize,
        objects: &[Block],
        bindings: &mut MacroBindings,
    ) -> Result<bool, SchemaError> {
        let before = rest_index;
        let after = self.children.len() - rest_index - 1;
        if objects.len() < before + after {
            return Ok(false);
        }
        for (pattern, object) in self.children.iter().zip(objects).take(before) {
            if !pattern.matches_block(object, bindings)? {
                return Ok(false);
            }
        }
        let repeated_end = objects.len() - after;
        let capture_name = self.children[rest_index]
            .as_rest_capture_name()
            .expect("rest index came from rest capture");
        bindings.bind_repeated(capture_name, &objects[before..repeated_end])?;
        for index in 0..after {
            let pattern_index = rest_index + 1 + index;
            let object_index = repeated_end + index;
            if !self.children[pattern_index].matches_block(&objects[object_index], bindings)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CaptureName {
    name: String,
    rest: bool,
}

impl CaptureName {
    fn from_token(token: &str) -> Result<Option<Self>, SchemaError> {
        if !token.starts_with('$') {
            return Ok(None);
        }
        let (rest, name) = token
            .strip_prefix("$*")
            .map(|name| (true, name))
            .or_else(|| token.strip_prefix('$').map(|name| (false, name)))
            .expect("starts with dollar");
        if name.is_empty() {
            return Err(SchemaError::InvalidMacroCapture {
                found: token.to_owned(),
            });
        }
        Ok(Some(Self {
            name: name.to_owned(),
            rest,
        }))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct MacroBindings {
    singles: Vec<SingleMacroBinding>,
    repeated: Vec<RepeatedMacroBinding>,
}

impl MacroBindings {
    fn bind_single(&mut self, name: &str, value: &Block) -> Result<bool, SchemaError> {
        if let Some(existing) = self.singles.iter().find(|binding| binding.name == name) {
            return Ok(existing.value == *value);
        }
        self.singles.push(SingleMacroBinding {
            name: name.to_owned(),
            value: value.clone(),
        });
        Ok(true)
    }

    fn bind_repeated(&mut self, name: &str, values: &[Block]) -> Result<(), SchemaError> {
        if let Some(existing) = self.repeated.iter().find(|binding| binding.name == name) {
            if existing.values == values {
                return Ok(());
            }
            return Err(SchemaError::ConflictingMacroBinding {
                name: name.to_owned(),
            });
        }
        self.repeated.push(RepeatedMacroBinding {
            name: name.to_owned(),
            values: values.to_vec(),
        });
        Ok(())
    }

    fn single(&self, name: &str) -> Result<&Block, SchemaError> {
        self.singles
            .iter()
            .find(|binding| binding.name == name)
            .map(|binding| &binding.value)
            .ok_or_else(|| SchemaError::MissingMacroBinding {
                name: name.to_owned(),
            })
    }

    fn repeated(&self, name: &str) -> Result<&[Block], SchemaError> {
        self.repeated
            .iter()
            .find(|binding| binding.name == name)
            .map(|binding| binding.values.as_slice())
            .ok_or_else(|| SchemaError::MissingMacroBinding {
                name: name.to_owned(),
            })
    }

    fn remember(&self, macro_name: &str, context: &mut MacroContext) {
        for binding in &self.singles {
            context.remember_binding(macro_name, &binding.name);
        }
        for binding in &self.repeated {
            context.remember_binding(macro_name, format!("*{}", binding.name));
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SingleMacroBinding {
    name: String,
    value: Block,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RepeatedMacroBinding {
    name: String,
    values: Vec<Block>,
}

#[derive(Clone, Debug)]
struct DeclarativeSchemaMacro {
    definition: ExecutableMacroDefinition,
}

impl SchemaMacroHandler for DeclarativeSchemaMacro {
    fn name(&self) -> &str {
        self.definition.name.as_str()
    }

    fn matches(&self, object: MacroObject<'_>, position: MacroPosition) -> bool {
        position == self.definition.position
            && self
                .definition
                .pattern
                .captures(object)
                .is_ok_and(|captures| captures.is_some())
    }

    fn lower(
        &self,
        object: MacroObject<'_>,
        position: MacroPosition,
        context: &mut MacroContext,
        registry: &MacroRegistry,
    ) -> Result<MacroOutput, SchemaError> {
        if position != self.definition.position {
            return Err(SchemaError::MacroDidNotMatch {
                macro_name: self.name().to_owned(),
            });
        }
        let bindings = self.definition.pattern.captures(object)?.ok_or_else(|| {
            SchemaError::MacroDidNotMatch {
                macro_name: self.name().to_owned(),
            }
        })?;
        context.remember_macro(self.name());
        context.remember_position(position);
        bindings.remember(self.name(), context);
        self.definition
            .template
            .expand_output(self.name(), &bindings, registry, context)
    }
}

#[derive(Clone, Copy, Debug)]
enum ObjectView<'object> {
    Block(&'object Block),
    Expanded(&'object ExpandedObject),
}

impl<'object> ObjectView<'object> {
    fn demote_to_string(&self) -> Option<&'object str> {
        match self {
            Self::Block(block) => block.demote_to_string(),
            Self::Expanded(object) => object.demote_to_string(),
        }
    }

    fn schema_name(&self) -> Result<Name, SchemaError> {
        match self {
            Self::Block(block) => block.schema_name(),
            Self::Expanded(object) => object.schema_name(),
        }
    }

    fn is_parenthesis(&self) -> bool {
        self.delimited_children(Delimiter::Parenthesis).is_some()
    }

    fn holds_root_objects(&self) -> usize {
        match self {
            Self::Block(block) => block.holds_root_objects(),
            Self::Expanded(object) => object.holds_root_objects(),
        }
    }

    fn root_object_at(&self, index: usize) -> Option<Self> {
        match self {
            Self::Block(block) => block.root_object_at(index).map(Self::Block),
            Self::Expanded(ExpandedObject::Captured(block)) => {
                block.root_object_at(index).map(Self::Block)
            }
            Self::Expanded(object) => object.root_object_at(index).map(Self::Expanded),
        }
    }

    fn root_objects(&self) -> Vec<Self> {
        match self {
            Self::Block(block) => block.root_objects().iter().map(Self::Block).collect(),
            Self::Expanded(ExpandedObject::Captured(block)) => {
                block.root_objects().iter().map(Self::Block).collect()
            }
            Self::Expanded(object) => object.root_objects().iter().map(Self::Expanded).collect(),
        }
    }

    fn delimited_children(&self, expected: Delimiter) -> Option<Vec<Self>> {
        match self {
            Self::Block(Block::Delimited {
                delimiter,
                root_objects,
                ..
            }) if *delimiter == expected => Some(root_objects.iter().map(Self::Block).collect()),
            Self::Expanded(ExpandedObject::Delimited {
                delimiter,
                children,
            }) if *delimiter == expected => Some(children.iter().map(Self::Expanded).collect()),
            Self::Expanded(ExpandedObject::Captured(block)) => {
                ObjectView::Block(block).delimited_children(expected)
            }
            Self::Block(_) | Self::Expanded(_) => None,
        }
    }

    fn qualifies_as_pascal_case_symbol(&self) -> bool {
        match self {
            Self::Block(block) => block.qualifies_as_pascal_case_symbol(),
            Self::Expanded(object) => object.qualifies_as_pascal_case_symbol(),
        }
    }

    fn compact_notation(&self) -> String {
        match self {
            Self::Block(block) => NotationBlock::new(block).compact_notation(),
            Self::Expanded(object) => object.compact_notation(),
        }
    }

    fn type_reference(
        &self,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<TypeReference, SchemaError> {
        match self {
            Self::Block(block) => TypeReference::from_block(block),
            Self::Expanded(object) => object.type_reference(registry, context),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ExpandedObject {
    Captured(Block),
    Atom(String),
    Delimited {
        delimiter: Delimiter,
        children: Vec<ExpandedObject>,
    },
}

impl ExpandedObject {
    fn compact_notation(&self) -> String {
        match self {
            Self::Captured(block) => NotationBlock::new(block).compact_notation(),
            Self::Atom(text) => text.clone(),
            Self::Delimited {
                delimiter,
                children,
            } => DelimitedNotation::new(*delimiter).wrap_children(
                &children
                    .iter()
                    .map(Self::compact_notation)
                    .collect::<Vec<_>>(),
            ),
        }
    }

    fn demote_to_string(&self) -> Option<&str> {
        match self {
            Self::Captured(block) => block.demote_to_string(),
            Self::Atom(text) => Some(text.as_str()),
            Self::Delimited { .. } => None,
        }
    }

    fn schema_name(&self) -> Result<Name, SchemaError> {
        match self {
            Self::Captured(block) => block.schema_name(),
            Self::Atom(text) => {
                let name = Name::new(text);
                if name.qualifies_as_symbol_name() {
                    Ok(name)
                } else {
                    Err(SchemaError::ExpectedSymbol {
                        found: text.clone(),
                    })
                }
            }
            Self::Delimited { .. } => Err(SchemaError::ExpectedSymbol {
                found: self.compact_notation(),
            }),
        }
    }

    fn holds_root_objects(&self) -> usize {
        match self {
            Self::Captured(block) => block.holds_root_objects(),
            Self::Delimited { children, .. } => children.len(),
            Self::Atom(_) => 0,
        }
    }

    fn root_object_at(&self, index: usize) -> Option<&ExpandedObject> {
        match self {
            Self::Delimited { children, .. } => children.get(index),
            Self::Captured(_) | Self::Atom(_) => None,
        }
    }

    fn root_objects(&self) -> &[ExpandedObject] {
        match self {
            Self::Delimited { children, .. } => children,
            Self::Captured(_) | Self::Atom(_) => &[],
        }
    }

    fn qualifies_as_pascal_case_symbol(&self) -> bool {
        match self {
            Self::Captured(block) => block.qualifies_as_pascal_case_symbol(),
            Self::Atom(text) => {
                Name::new(text.as_str()).qualifies_as_symbol_name()
                    && text
                        .chars()
                        .next()
                        .is_some_and(|character| character.is_ascii_uppercase())
                    && !text.contains('-')
            }
            Self::Delimited { .. } => false,
        }
    }

    fn type_reference(
        &self,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<TypeReference, SchemaError> {
        match self {
            Self::Captured(block) => TypeReference::from_block(block),
            Self::Atom(_) => ExpandedReference::new(std::slice::from_ref(self)).type_reference(),
            Self::Delimited {
                delimiter: Delimiter::Parenthesis,
                children,
            } => ExpandedReference::new(children).type_reference(),
            Self::Delimited {
                delimiter: Delimiter::SquareBracket,
                children,
            } => Err(SchemaError::UnknownTypeReferenceForm {
                head: "SquareBracket".to_owned(),
                argument_count: children.len(),
            }),
            Self::Delimited {
                delimiter: Delimiter::Brace,
                children,
            } => Err(SchemaError::UnknownTypeReferenceForm {
                head: "Brace".to_owned(),
                argument_count: children.len(),
            }),
            Self::Delimited {
                delimiter: Delimiter::PipeBrace,
                children,
            } => ExpandedReference::new(children).inline_struct(registry, context),
            Self::Delimited {
                delimiter: Delimiter::PipeParenthesis,
                children,
            } => ExpandedReference::new(children).inline_enum(registry, context),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ExpandedReference<'object> {
    children: &'object [ExpandedObject],
}

impl<'object> ExpandedReference<'object> {
    fn new(children: &'object [ExpandedObject]) -> Self {
        Self { children }
    }

    /// Lower a grouped post-expansion source-reference sequence through the
    /// same dotted reader used by authored schema source. The grouping exists
    /// only so a macro template can keep a trailing-dot head and its capture
    /// payload together; `(Vector $Type)` remains invalid because the sequence
    /// is parsed as ordinary dotted source, not as a parenthesized resolver.
    fn type_reference(&self) -> Result<TypeReference, SchemaError> {
        MacroExpansionReference::from_objects(
            self.children.iter().map(ObjectView::Expanded).collect(),
        )
        .lower_source()
    }

    fn inline_struct(
        &self,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<TypeReference, SchemaError> {
        let name = self.inline_declaration_name("inline struct declaration")?;
        let fields = MacroExpansionFields::from_objects(
            self.children[1..]
                .iter()
                .map(ObjectView::Expanded)
                .collect(),
        )
        .lower(registry, context)?;
        if fields.len() == 1 {
            let reference = fields.into_iter().next().expect("length checked").reference;
            context.remember_inline_declaration(crate::Declaration::private(
                TypeDeclaration::Newtype(NewtypeDeclaration::new(name.clone(), reference)),
            ));
        } else {
            context.remember_inline_declaration(crate::Declaration::private(
                TypeDeclaration::Struct(StructDeclaration::new(name.clone(), fields)),
            ));
        }
        Ok(TypeReference::Plain(name))
    }

    fn inline_enum(
        &self,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<TypeReference, SchemaError> {
        let name = self.inline_declaration_name("inline enum declaration")?;
        let variants = MacroExpansionVariants::from_objects(
            self.children[1..]
                .iter()
                .map(ObjectView::Expanded)
                .collect(),
        )
        .lower(registry, context)?;
        context.remember_inline_declaration(crate::Declaration::private(TypeDeclaration::Enum(
            EnumDeclaration::new(name.clone(), variants),
        )));
        Ok(TypeReference::Plain(name))
    }

    fn inline_declaration_name(&self, form: &'static str) -> Result<Name, SchemaError> {
        let Some(name) = self.children.first() else {
            return Err(SchemaError::ExpectedSyntaxReferenceArity {
                form,
                expected: "declaration name plus body",
                found: 0,
            });
        };
        ObjectView::Expanded(name).schema_name()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MacroExpansionFields<'template> {
    objects: Vec<ObjectView<'template>>,
}

#[derive(Clone, Debug)]
pub(crate) struct MacroExpansionStructBody<'template> {
    name: Name,
    objects: Vec<ObjectView<'template>>,
}

impl<'template> MacroExpansionStructBody<'template> {
    fn new(name: Name, objects: Vec<ObjectView<'template>>) -> Self {
        Self { name, objects }
    }

    pub(crate) fn from_blocks(name: Name, objects: &'template [Block]) -> Self {
        Self {
            name,
            objects: objects.iter().map(ObjectView::Block).collect(),
        }
    }

    pub(crate) fn lower_type(
        &self,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<TypeDeclaration, SchemaError> {
        let fields =
            MacroExpansionFields::from_objects(self.objects.clone()).lower(registry, context)?;
        if fields.len() == 1 {
            let reference = fields.into_iter().next().expect("length checked").reference;
            Ok(TypeDeclaration::Newtype(NewtypeDeclaration::new(
                self.name.clone(),
                reference,
            )))
        } else {
            Ok(TypeDeclaration::Struct(StructDeclaration::new(
                self.name.clone(),
                fields,
            )))
        }
    }
}

impl<'template> MacroExpansionFields<'template> {
    fn from_objects(objects: Vec<ObjectView<'template>>) -> Self {
        Self { objects }
    }

    pub(crate) fn lower(
        &self,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<Vec<FieldDeclaration>, SchemaError> {
        let mut fields = Vec::new();
        let mut index = 0;
        while index < self.objects.len() {
            if let Some(field) = self.explicit_trailing_dot_field(&mut index, registry, context)? {
                fields.push(field);
                continue;
            }
            let object = self.objects[index];
            index += 1;
            fields.push(MacroExpansionField::new(object).lower(registry, context)?);
        }
        Ok(fields)
    }

    fn explicit_trailing_dot_field(
        &self,
        index: &mut usize,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<Option<FieldDeclaration>, SchemaError> {
        let object = self.objects[*index];
        let Some(text) = object.demote_to_string() else {
            return Ok(None);
        };
        let Some(field_name) = text.strip_suffix('.') else {
            return Ok(None);
        };
        if field_name.is_empty() {
            return Err(SchemaError::RetiredStructFieldSyntax {
                found: text.to_owned(),
            });
        }
        let Some(reference_object) = self.objects.get(*index + 1).copied() else {
            return Err(SchemaError::ExpectedSyntaxReferenceArity {
                form: "explicit composite field",
                expected: "a trailing-dot field name and a following type reference",
                found: 1,
            });
        };
        *index += 2;
        MacroExpansionField::explicit_reference_field(
            field_name,
            reference_object.type_reference(registry, context)?,
        )
        .map(Some)
    }
}

/// One field inside a struct body.
///
/// Strict struct bodies are positional lists. A bare PascalCase type
/// derives the field name from the referenced type. `field.Type` is the
/// explicit differentiator when a field role should not be derived from
/// the referenced type. Composite references use the same dotted source reader
/// as authored schema: `Vector.Topic`, `Map.(Topic RecordIdentifier)`, and
/// `Optional.Topic` lower to vector, map, and optional references with names
/// derived from the reference shape.
#[derive(Clone, Copy, Debug)]
struct MacroExpansionField<'template> {
    object: ObjectView<'template>,
}

impl<'template> MacroExpansionField<'template> {
    fn new(object: ObjectView<'template>) -> Self {
        Self { object }
    }

    fn lower(
        &self,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<FieldDeclaration, SchemaError> {
        if self.is_explicit_field_pair() {
            return Err(SchemaError::RetiredStructFieldSyntax {
                found: self
                    .object
                    .demote_to_string()
                    .unwrap_or("parenthesized field pair")
                    .to_owned(),
            });
        }
        if let Some(field) = self.explicit_structural_field(registry, context)? {
            return Ok(field);
        }
        if self.object.demote_to_string().is_none() {
            let reference = self.object.type_reference(registry, context)?;
            return Ok(FieldDeclaration {
                name: reference.derived_field_name(),
                reference,
            });
        }
        let text = self.object.demote_to_string().expect("checked");
        if let Some((field_name, type_name)) = text.split_once('.') {
            return self.explicit_dot_field(field_name, type_name);
        }
        if text == "*"
            || text
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_lowercase())
        {
            return Err(SchemaError::RetiredStructFieldSyntax {
                found: text.to_owned(),
            });
        }
        let name = self.object.schema_name()?;
        if TypeReference::is_reserved_scalar_name(&name) {
            return Err(SchemaError::RetiredStructFieldSyntax {
                found: name.to_nota(),
            });
        }
        Ok(FieldDeclaration {
            name: Name::new(name.field_name()),
            reference: TypeReference::from_name(name),
        })
    }

    fn explicit_dot_field(
        &self,
        field_name: &str,
        type_name: &str,
    ) -> Result<FieldDeclaration, SchemaError> {
        let name = Name::new(field_name);
        let reference = Name::new(type_name);
        if field_name.is_empty()
            || type_name.is_empty()
            || field_name.contains('.')
            || type_name.contains('.')
            || !name.qualifies_as_symbol_name()
            || !reference.qualifies_as_symbol_name()
            || !reference.qualifies_as_pascal_case()
        {
            return Err(SchemaError::RetiredStructFieldSyntax {
                found: format!("{field_name}.{type_name}"),
            });
        }
        if name.field_name() == reference.field_name()
            && !TypeReference::is_reserved_scalar_name(&reference)
        {
            return Err(SchemaError::RedundantExplicitFieldRole {
                found: format!("{field_name}.{type_name}"),
                type_name: reference.to_nota(),
            });
        }
        Ok(FieldDeclaration {
            name: Name::new(name.field_name()),
            reference: TypeReference::from_name(reference),
        })
    }

    fn explicit_reference_field(
        field_name: &str,
        reference: TypeReference,
    ) -> Result<FieldDeclaration, SchemaError> {
        let name = Name::new(field_name);
        if field_name.is_empty() || field_name.contains('.') || !name.qualifies_as_symbol_name() {
            return Err(SchemaError::RetiredStructFieldSyntax {
                found: format!("{field_name}.<reference>"),
            });
        }
        let derived = reference.derived_field_name();
        if name.field_name() == derived.as_str() {
            return Err(SchemaError::RedundantExplicitFieldRole {
                found: format!("{field_name}.<reference>"),
                type_name: derived.to_nota(),
            });
        }
        Ok(FieldDeclaration {
            name: Name::new(name.field_name()),
            reference,
        })
    }

    fn explicit_structural_field(
        &self,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<Option<FieldDeclaration>, SchemaError> {
        if !self.object.is_parenthesis() || self.object.holds_root_objects() != 2 {
            return Ok(None);
        }
        let Some(name_object) = self.object.root_object_at(0) else {
            return Ok(None);
        };
        let Some(reference_object) = self.object.root_object_at(1) else {
            return Ok(None);
        };
        if !name_object.qualifies_as_pascal_case_symbol()
            || reference_object.demote_to_string().is_some()
        {
            return Ok(None);
        }
        let _ = (registry, context, reference_object);
        let _name = name_object.schema_name()?;
        Err(SchemaError::RetiredStructFieldSyntax {
            found: self
                .object
                .demote_to_string()
                .unwrap_or("parenthesized structural field")
                .to_owned(),
        })
    }

    fn is_explicit_field_pair(&self) -> bool {
        self.object.is_parenthesis()
            && self.object.holds_root_objects() == 2
            && self
                .object
                .root_object_at(0)
                .and_then(|object| object.demote_to_string())
                .is_some_and(|name| {
                    name.chars()
                        .next()
                        .is_some_and(|character| character.is_ascii_lowercase())
                })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MacroExpansionVariants<'template> {
    objects: Vec<ObjectView<'template>>,
}

impl<'template> MacroExpansionVariants<'template> {
    pub(crate) fn new(objects: &'template [Block]) -> Self {
        Self {
            objects: objects.iter().map(ObjectView::Block).collect(),
        }
    }

    fn from_objects(objects: Vec<ObjectView<'template>>) -> Self {
        Self { objects }
    }

    pub(crate) fn lower(
        &self,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<Vec<EnumVariant>, SchemaError> {
        self.objects
            .iter()
            .map(|object| MacroExpansionVariant::new(*object).lower(registry, context))
            .collect()
    }
}

#[derive(Clone, Copy, Debug)]
struct MacroExpansionVariant<'template> {
    object: ObjectView<'template>,
}

impl<'template> MacroExpansionVariant<'template> {
    fn new(object: ObjectView<'template>) -> Self {
        Self { object }
    }

    fn lower(
        &self,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<EnumVariant, SchemaError> {
        if self.object.is_parenthesis() {
            self.lower_parenthesis(registry, context)
        } else if self.object.qualifies_as_pascal_case_symbol() {
            Ok(EnumVariant::new(self.object.schema_name()?, None))
        } else {
            Err(SchemaError::ExpectedEnumVariant)
        }
    }

    fn lower_parenthesis(
        &self,
        registry: &MacroRegistry,
        context: &mut MacroContext,
    ) -> Result<EnumVariant, SchemaError> {
        match self.object.holds_root_objects() {
            1 => {
                let name = self
                    .object
                    .root_object_at(0)
                    .expect("count checked")
                    .schema_name()?;
                Ok(EnumVariant::new(
                    name.clone(),
                    Some(TypeReference::from_name(name)),
                ))
            }
            2 => Ok(EnumVariant::new(
                self.object
                    .root_object_at(0)
                    .expect("count checked")
                    .schema_name()?,
                Some(
                    self.object
                        .root_object_at(1)
                        .expect("count checked")
                        .type_reference(registry, context)?,
                ),
            )),
            _ => Err(SchemaError::ExpectedEnumVariant),
        }
    }
}

#[derive(Clone, Debug)]
struct MacroExpansionReference<'template> {
    objects: Vec<ObjectView<'template>>,
}

impl<'template> MacroExpansionReference<'template> {
    fn from_objects(objects: Vec<ObjectView<'template>>) -> Self {
        Self { objects }
    }

    fn lower(
        &self,
        _registry: &MacroRegistry,
        _context: &mut MacroContext,
    ) -> Result<TypeReference, SchemaError> {
        self.lower_source()
    }

    fn lower_source(&self) -> Result<TypeReference, SchemaError> {
        let document = Document::parse(self.source_text())?;
        let mut cursor = 0;
        let reference =
            crate::SourceReference::from_blocks_at(document.root_objects(), &mut cursor)?;
        if cursor == document.root_objects().len() {
            Ok(reference.to_type_reference())
        } else {
            Err(SchemaError::ExpectedRootObjectCount {
                expected: "one dotted type-reference template",
                found: document.root_objects().len(),
            })
        }
    }

    fn source_text(&self) -> String {
        self.objects
            .iter()
            .map(ObjectView::compact_notation)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Clone, Copy, Debug)]
struct NotationBlock<'block> {
    block: &'block Block,
}

impl<'block> NotationBlock<'block> {
    fn new(block: &'block Block) -> Self {
        Self { block }
    }

    fn compact_notation(&self) -> String {
        match self.block {
            Block::Delimited {
                delimiter,
                root_objects,
                ..
            } => DelimitedNotation::new(*delimiter).wrap_children(
                &root_objects
                    .iter()
                    .map(|object| NotationBlock::new(object).compact_notation())
                    .collect::<Vec<_>>(),
            ),
            Block::PipeText(pipe_text) => format!("[|{}|]", pipe_text.text),
            Block::Atom(atom) => atom.text().to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct DelimitedNotation {
    delimiter: Delimiter,
}

impl DelimitedNotation {
    fn new(delimiter: Delimiter) -> Self {
        Self { delimiter }
    }

    fn wrap_children(&self, children: &[String]) -> String {
        if children.is_empty() {
            return format!("{}{}", self.opening(), self.closing());
        }
        format!("{}{}{}", self.opening(), children.join(" "), self.closing())
    }

    fn opening(&self) -> &'static str {
        match self.delimiter {
            Delimiter::Parenthesis => "(",
            Delimiter::SquareBracket => "[",
            Delimiter::Brace => "{",
            Delimiter::PipeParenthesis => "(|",
            Delimiter::PipeBrace => "{|",
        }
    }

    fn closing(&self) -> &'static str {
        match self.delimiter {
            Delimiter::Parenthesis => ")",
            Delimiter::SquareBracket => "]",
            Delimiter::Brace => "}",
            Delimiter::PipeParenthesis => "|)",
            Delimiter::PipeBrace => "|}",
        }
    }
}
