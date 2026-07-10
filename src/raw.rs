use std::path::Path;

use nota::{Block, Delimiter, Document, NotaDecodeError};

use crate::{Name, SchemaError, macros::SchemaBlockExt};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawSchemaFile {
    root_name: Name,
    datatypes: RawDatatypeMap,
}

impl RawSchemaFile {
    pub fn from_path_and_source(path: impl AsRef<Path>, source: &str) -> Result<Self, SchemaError> {
        let root_name = RawSchemaFileName::from_path(path.as_ref())?.to_name();
        let document = Document::parse(source)?;
        RawSchemaDocument::new(&document).read(root_name)
    }

    pub fn root_name(&self) -> &Name {
        &self.root_name
    }

    pub fn datatypes(&self) -> &RawDatatypeMap {
        &self.datatypes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawDatatypeMap {
    entries: Vec<RawDatatypeEntry>,
}

impl RawDatatypeMap {
    /// Read the datatype map as a sequence of dotted `key.datatype` entries,
    /// walking by how many blocks each entry consumes rather than pairing by a
    /// fixed stride. This is the raw, pre-semantic reflection layer: its keys are
    /// arbitrary NOTA atoms — capitalized type names at the top level, lowercase
    /// field roles inside a record — so the split is decided by the first
    /// top-level dot alone, through the shared NOTA split primitive, without the
    /// case gate that the two semantic dotted expectations apply.
    pub fn from_blocks(objects: &[Block]) -> Result<Self, SchemaError> {
        let mut entries = Vec::new();
        let mut index = 0;
        while index < objects.len() {
            let atom = objects[index].atom().ok_or_else(|| {
                SchemaError::from(NotaDecodeError::ExpectedDottedEntry {
                    expectation: Self::ENTRY_EXPECTATION,
                })
            })?;
            let (prefix, remainder) = atom.split_at_first_dot().ok_or_else(|| {
                SchemaError::from(NotaDecodeError::ExpectedDottedEntry {
                    expectation: Self::ENTRY_EXPECTATION,
                })
            })?;
            let name = Block::Atom(prefix).schema_name()?;
            let (value, consumed) = match remainder {
                Some(value_atom) => (Block::Atom(value_atom), 1),
                None => {
                    let value = objects.get(index + 1).cloned().ok_or_else(|| {
                        SchemaError::from(NotaDecodeError::DottedEntryMissingValue {
                            expectation: Self::ENTRY_EXPECTATION,
                        })
                    })?;
                    (value, 2)
                }
            };
            entries.push(RawDatatypeEntry {
                name,
                datatype: RawNotaDatatype::from_block(&value)?,
            });
            index += consumed;
        }
        Ok(Self { entries })
    }

    const ENTRY_EXPECTATION: &'static str = "raw datatype map entry";

    pub fn entries(&self) -> &[RawDatatypeEntry] {
        &self.entries
    }

    pub fn datatype_named(&self, name: &str) -> Option<&RawNotaDatatype> {
        self.entries
            .iter()
            .find(|entry| entry.name.as_str() == name)
            .map(RawDatatypeEntry::datatype)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawDatatypeEntry {
    name: Name,
    datatype: RawNotaDatatype,
}

impl RawDatatypeEntry {
    pub fn name(&self) -> &Name {
        &self.name
    }

    pub fn datatype(&self) -> &RawNotaDatatype {
        &self.datatype
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RawNotaDatatype {
    Atom(String),
    Text(String),
    Record(RawNotaSequence),
    Vector(RawNotaSequence),
    KeyValue(RawDatatypeMap),
}

impl RawNotaDatatype {
    pub fn from_block(block: &Block) -> Result<Self, SchemaError> {
        match block {
            Block::Atom(atom) => Ok(Self::Atom(atom.text().to_owned())),
            Block::PipeText(text) => Ok(Self::Text(text.text.clone())),
            Block::Delimited {
                delimiter: Delimiter::Parenthesis,
                root_objects,
                ..
            } => Ok(Self::Record(RawNotaSequence::from_blocks(root_objects)?)),
            Block::Delimited {
                delimiter: Delimiter::SquareBracket,
                root_objects,
                ..
            } => Ok(Self::Vector(RawNotaSequence::from_blocks(root_objects)?)),
            Block::Delimited {
                delimiter: Delimiter::Brace,
                root_objects,
                ..
            } => Ok(Self::KeyValue(RawDatatypeMap::from_blocks(root_objects)?)),
        }
    }

    pub fn as_atom(&self) -> Option<&str> {
        match self {
            Self::Atom(text) => Some(text),
            Self::Text(_) | Self::Record(_) | Self::Vector(_) | Self::KeyValue(_) => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Atom(_) | Self::Record(_) | Self::Vector(_) | Self::KeyValue(_) => None,
        }
    }

    pub fn as_record(&self) -> Option<&RawNotaSequence> {
        match self {
            Self::Record(sequence) => Some(sequence),
            Self::Atom(_) | Self::Text(_) | Self::Vector(_) | Self::KeyValue(_) => None,
        }
    }

    pub fn as_vector(&self) -> Option<&RawNotaSequence> {
        match self {
            Self::Vector(sequence) => Some(sequence),
            Self::Atom(_) | Self::Text(_) | Self::Record(_) | Self::KeyValue(_) => None,
        }
    }

    pub fn as_key_value(&self) -> Option<&RawDatatypeMap> {
        match self {
            Self::KeyValue(map) => Some(map),
            Self::Atom(_) | Self::Text(_) | Self::Record(_) | Self::Vector(_) => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawNotaSequence {
    items: Vec<RawNotaDatatype>,
}

impl RawNotaSequence {
    pub fn from_blocks(objects: &[Block]) -> Result<Self, SchemaError> {
        let mut items = Vec::new();
        for object in objects {
            items.push(RawNotaDatatype::from_block(object)?);
        }
        Ok(Self { items })
    }

    pub fn items(&self) -> &[RawNotaDatatype] {
        &self.items
    }
}

#[derive(Clone, Debug)]
struct RawSchemaDocument<'document> {
    document: &'document Document,
}

impl<'document> RawSchemaDocument<'document> {
    fn new(document: &'document Document) -> Self {
        Self { document }
    }

    fn read(&self, root_name: Name) -> Result<RawSchemaFile, SchemaError> {
        if self.document.holds_root_objects() != 1 {
            return Err(SchemaError::ExpectedRootObjectCount {
                expected: "one root key-value datatype map",
                found: self.document.holds_root_objects(),
            });
        }
        let root = self.document.root_object_at(0).expect("root count checked");
        let Block::Delimited {
            delimiter: Delimiter::Brace,
            root_objects,
            ..
        } = root
        else {
            return Err(SchemaError::ExpectedDelimiter {
                expected: "root key-value datatype map",
            });
        };
        Ok(RawSchemaFile {
            root_name,
            datatypes: RawDatatypeMap::from_blocks(root_objects)?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RawSchemaFileName {
    stem: String,
}

impl RawSchemaFileName {
    fn from_path(path: &Path) -> Result<Self, SchemaError> {
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| SchemaError::MalformedSchemaPath {
                path: path.display().to_string(),
            })?;
        Ok(Self {
            stem: stem.to_owned(),
        })
    }

    fn to_name(&self) -> Name {
        let mut output = String::new();
        let mut upper_next = true;
        for character in self.stem.chars() {
            if character.is_ascii_alphanumeric() {
                if upper_next {
                    output.push(character.to_ascii_uppercase());
                    upper_next = false;
                } else {
                    output.push(character);
                }
            } else {
                upper_next = true;
            }
        }
        Name::new(output)
    }
}
