//! Content identity for semantic schema values.
//!
//! A schema's identity is the blake3 hash of its canonical rkyv bytes:
//! any edit to the semantic schema changes the address, and the address
//! is the version the version-control layer consumes. Two hash domains
//! exist — the whole-schema value and a per-family declaration closure —
//! and each is domain-separated through its own blake3 `derive_key`
//! context so the two kinds can never collide.
//!
//! Coverage boundaries the version-control layer must know: a family
//! closure covers what is reachable FROM the declaration — struct
//! fields, variant payloads, alias/newtype targets, collection inner
//! references, and stream relations. Relation declarations point AT
//! declarations rather than being reachable from them, so a relation
//! edit moves only the whole-schema hash, never a family hash. The
//! whole-schema hash covers the full semantic value including
//! `SchemaIdentity` (component name + authored version string) and
//! resolved imports, so it is not a pure-structure address; the family
//! hashes are.

use std::collections::BTreeSet;
use std::fmt;

use crate::{
    SchemaError,
    schema::{
        Declaration, EnumDeclaration, GenericDefinition, ImportDeclaration, Name,
        StreamDeclaration, TrueSchema, TypeDeclaration, TypeReference,
    },
};

/// The hash domains content identity is derived under. Each domain
/// carries its own blake3 `derive_key` context string, so a
/// whole-schema hash and a family-closure hash over identical bytes
/// are structurally distinct values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HashDomain {
    TrueSchema,
    FamilyClosure,
}

impl HashDomain {
    fn context(self) -> &'static str {
        match self {
            Self::TrueSchema => "schema 2026-06-12 whole-schema content identity",
            Self::FamilyClosure => "schema 2026-06-12 family-closure content identity",
        }
    }
}

/// A 32-byte blake3 content address over canonical rkyv bytes.
///
/// The hash is computed over the semantic value's serialized bytes,
/// never over `.schema` source text, so formatting-only source edits
/// (whitespace, comments) do not move the address.
#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    Clone,
    Copy,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    fn derive(domain: HashDomain, bytes: &[u8]) -> Self {
        let mut hasher = blake3::Hasher::new_derive_key(domain.context());
        hasher.update(bytes);
        Self(*hasher.finalize().as_bytes())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

impl fmt::Debug for ContentHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "ContentHash({})", self.to_hex())
    }
}

/// The transitive declaration closure of one named record family.
///
/// The closure holds the family's root name plus every declaration
/// reachable from it through type references — struct fields, enum
/// variant payloads, newtype/alias references, `Vec`/`Map`/`Optional`/
/// `ScopeOf` element references, stream-relation stream declarations —
/// each group sorted canonically by name so the closure's bytes do not
/// depend on walk order. A reachable cross-crate import contributes its
/// stable identity (the local alias plus its `crate:module:Type`
/// source), not the dependency's own declarations.
///
/// When the family root is the application form `(Head Arg …)`, the
/// applied reference is held in `root_application` as well: the reachable
/// declarations and imports alone do not distinguish two application roots
/// whose arguments differ only in scalar leaves (`String` versus
/// `Integer` reach nothing), so the application reference itself is part of
/// the closure's canonical bytes. An enum root leaves `root_application`
/// empty.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct FamilyClosure {
    root: Name,
    root_application: Option<TypeReference>,
    declarations: Vec<Declaration>,
    generics: Vec<GenericDefinition>,
    imports: Vec<ImportDeclaration>,
    streams: Vec<StreamDeclaration>,
}

impl FamilyClosure {
    pub fn root(&self) -> &Name {
        &self.root
    }

    /// The applied reference when the family root is the application form;
    /// `None` for an enum root. It is `Some(TypeReference::Application{..})`
    /// by construction.
    pub fn root_application(&self) -> Option<&TypeReference> {
        self.root_application.as_ref()
    }

    pub fn declarations(&self) -> &[Declaration] {
        &self.declarations
    }

    pub fn generics(&self) -> &[GenericDefinition] {
        &self.generics
    }

    pub fn imports(&self) -> &[ImportDeclaration] {
        &self.imports
    }

    pub fn streams(&self) -> &[StreamDeclaration] {
        &self.streams
    }

    /// The family's content address: blake3 over the closure's
    /// canonical rkyv bytes, under the family-closure hash domain.
    pub fn content_hash(&self) -> Result<ContentHash, SchemaError> {
        let bytes =
            rkyv::to_bytes::<rkyv::rancor::Error>(self).map_err(|_| SchemaError::ArchiveEncode)?;
        Ok(ContentHash::derive(HashDomain::FamilyClosure, &bytes))
    }
}

impl TrueSchema {
    /// The whole-schema content address: blake3 over the semantic
    /// schema value's canonical rkyv bytes, under the whole-schema
    /// hash domain. Any edit to the semantic schema moves this address.
    pub fn content_hash(&self) -> Result<ContentHash, SchemaError> {
        let bytes = self.to_binary_bytes()?;
        Ok(ContentHash::derive(HashDomain::TrueSchema, &bytes))
    }

    /// The declaration closure of the named family. The name must be a
    /// namespace declaration or an input/output root enum of this
    /// schema; every type name reachable from it must resolve to a
    /// namespace declaration, a root enum, or a declared import.
    pub fn family_closure(&self, family_name: &str) -> Result<FamilyClosure, SchemaError> {
        ClosureWalk::new(self, family_name).into_closure()
    }
}

/// The state of one closure walk: the schema being read, the family
/// being closed over, plus the reachable members keyed by name so
/// revisits terminate and the finished closure comes out sorted
/// canonically.
struct ClosureWalk<'schema> {
    schema: &'schema TrueSchema,
    family: &'schema str,
    declarations: Vec<(String, Declaration)>,
    generics: Vec<(String, GenericDefinition)>,
    imports: Vec<(String, ImportDeclaration)>,
    streams: Vec<(String, StreamDeclaration)>,
    /// Type-parameter binders in scope for the declaration currently
    /// being walked. A parameterized declaration head `(| Name Param … |)`
    /// introduces these; a `Plain` reference matching a binder resolves
    /// as a type-parameter rather than a `FamilyReferenceNotFound`. The
    /// scope is per-declaration, so each `visit_declaration` swaps in its
    /// own parameters and restores the caller's on the way out.
    binders: BTreeSet<String>,
}

/// A family's starting point for the closure walk. A declaration root (a
/// namespace declaration or an enum-body root, both wrapped as a
/// `Declaration`) walks through `visit_declaration`; an application root
/// `(Head Arg …)` walks through `visit_reference` on the reference it
/// projects to — the *same* `Application` arm a field-position application
/// takes, so no second hashing path exists. The position name carries the
/// root's identity when the root is an application (an application has no
/// declaration name of its own).
enum FamilyRoot {
    Declaration(Declaration),
    Generic(GenericDefinition),
    Application {
        name: Name,
        reference: TypeReference,
    },
}

impl FamilyRoot {
    fn name(&self) -> &Name {
        match self {
            Self::Declaration(declaration) => declaration.name(),
            Self::Generic(definition) => definition.name(),
            Self::Application { name, .. } => name,
        }
    }
}

impl<'schema> ClosureWalk<'schema> {
    fn new(schema: &'schema TrueSchema, family: &'schema str) -> Self {
        Self {
            schema,
            family,
            declarations: Vec::new(),
            generics: Vec::new(),
            imports: Vec::new(),
            streams: Vec::new(),
            binders: BTreeSet::new(),
        }
    }

    fn into_closure(mut self) -> Result<FamilyClosure, SchemaError> {
        let root =
            self.family_root(self.family)
                .ok_or_else(|| SchemaError::FamilyRootNotFound {
                    name: self.family.to_owned(),
                })?;
        let name = root.name().clone();
        // An application root holds its applied reference in the closure so
        // the content hash incorporates the full argument structure; an
        // enum/namespace root contributes only its reachable declarations.
        let root_application = match &root {
            FamilyRoot::Declaration(declaration) => {
                self.visit_declaration(declaration.clone())?;
                None
            }
            FamilyRoot::Generic(definition) => {
                self.visit_generic_definition(definition.clone())?;
                None
            }
            FamilyRoot::Application { reference, .. } => {
                self.visit_reference(reference)?;
                Some(reference.clone())
            }
        };
        self.declarations
            .sort_by(|left, right| left.0.cmp(&right.0));
        self.generics.sort_by(|left, right| left.0.cmp(&right.0));
        self.imports.sort_by(|left, right| left.0.cmp(&right.0));
        self.streams.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(FamilyClosure {
            root: name,
            root_application,
            declarations: self
                .declarations
                .into_iter()
                .map(|(_, declaration)| declaration)
                .collect(),
            generics: self
                .generics
                .into_iter()
                .map(|(_, generic)| generic)
                .collect(),
            imports: self.imports.into_iter().map(|(_, import)| import).collect(),
            streams: self.streams.into_iter().map(|(_, stream)| stream).collect(),
        })
    }

    /// A family root is a namespace declaration, an enum-body root, or an
    /// application-form root. The first two enter the closure as a public
    /// declaration walked through `visit_declaration`; the application form
    /// enters as the reference it projects to, walked through the shared
    /// `Application` arm of `visit_reference`. The root's input/output
    /// position is the version-control layer's concern, not the closure's.
    fn family_root(&self, family_name: &str) -> Option<FamilyRoot> {
        if let Some(declaration) = self.namespace_declaration(family_name) {
            return Some(FamilyRoot::Declaration(declaration.clone()));
        }
        if let Some(definition) = self.schema.generic_named(family_name) {
            return Some(FamilyRoot::Generic(definition.clone()));
        }
        let root = self.schema.root_named(family_name)?;
        match root.as_application() {
            Some(application) => Some(FamilyRoot::Application {
                name: application.name().clone(),
                reference: TypeReference::from(application),
            }),
            None => root
                .as_enum()
                .cloned()
                .map(TypeDeclaration::Enum)
                .map(Declaration::public)
                .map(FamilyRoot::Declaration),
        }
    }

    fn namespace_declaration(&self, name: &str) -> Option<&'schema Declaration> {
        self.schema
            .namespace()
            .iter()
            .find(|declaration| declaration.name().as_str() == name)
    }

    fn visit_declaration(&mut self, declaration: Declaration) -> Result<(), SchemaError> {
        let name = declaration.name().as_str().to_owned();
        if self
            .declarations
            .iter()
            .any(|(existing, _)| existing == &name)
        {
            return Ok(());
        }
        let value = declaration.value().clone();
        // A declaration is closed over its own type parameters, not the
        // caller's: swap in this declaration's binders for the body walk
        // and restore the caller's scope afterwards. References to a
        // binder resolve as a type-parameter, not a declared type.
        let outer_binders = std::mem::replace(
            &mut self.binders,
            declaration
                .parameters()
                .iter()
                .map(|parameter| parameter.as_str().to_owned())
                .collect(),
        );
        self.declarations.push((name, declaration));
        let result = match value {
            TypeDeclaration::Struct(body) => {
                let mut walked = Ok(());
                for field in body.fields.iter() {
                    walked = self.visit_reference(&field.reference);
                    if walked.is_err() {
                        break;
                    }
                }
                walked
            }
            TypeDeclaration::Newtype(body) => self.visit_reference(&body.reference),
            TypeDeclaration::Enum(body) => self.visit_enum(&body),
        };
        self.binders = outer_binders;
        result
    }

    fn visit_enum(&mut self, declaration: &EnumDeclaration) -> Result<(), SchemaError> {
        for variant in &declaration.variants {
            if let Some(payload) = &variant.payload {
                self.visit_reference(payload)?;
            }
            if let Some(relation) = &variant.stream_relation {
                self.visit_stream(relation.stream_name())?;
            }
        }
        Ok(())
    }

    fn visit_stream(&mut self, stream_name: &Name) -> Result<(), SchemaError> {
        if self
            .streams
            .iter()
            .any(|(existing, _)| existing == stream_name.as_str())
        {
            return Ok(());
        }
        let stream = self
            .schema
            .streams()
            .iter()
            .find(|stream| &stream.name == stream_name)
            .ok_or_else(|| SchemaError::FamilyReferenceNotFound {
                family: self.family.to_owned(),
                name: stream_name.as_str().to_owned(),
            })?
            .clone();
        self.streams
            .push((stream_name.as_str().to_owned(), stream.clone()));
        self.visit_reference(&stream.token)?;
        self.visit_reference(&stream.opened)?;
        self.visit_reference(&stream.event)?;
        self.visit_reference(&stream.close)
    }

    fn visit_reference(&mut self, reference: &TypeReference) -> Result<(), SchemaError> {
        match reference {
            TypeReference::String
            | TypeReference::Integer
            | TypeReference::Boolean
            | TypeReference::Path
            | TypeReference::Bytes
            | TypeReference::FixedBytes(_) => Ok(()),
            TypeReference::Plain(name) => self.visit_name(name),
            TypeReference::Vector(inner)
            | TypeReference::Optional(inner)
            | TypeReference::ScopeOf(inner) => self.visit_reference(inner),
            TypeReference::Map(key, value) => {
                self.visit_reference(key)?;
                self.visit_reference(value)
            }
            // A generic application reaches both its declared generic
            // definition and each argument. A local generic definition is
            // schema data, not a namespace declaration; an imported head still
            // enters through the import path.
            TypeReference::Application { head, arguments } => {
                self.visit_generic_head(head.name())?;
                for argument in arguments {
                    self.visit_reference(argument)?;
                }
                Ok(())
            }
        }
    }

    fn visit_generic_head(&mut self, name: &Name) -> Result<(), SchemaError> {
        if let Some(definition) = self.schema.generic_named(name.as_str()) {
            return self.visit_generic_definition(definition.clone());
        }
        self.visit_name(name)
    }

    fn visit_generic_definition(
        &mut self,
        definition: GenericDefinition,
    ) -> Result<(), SchemaError> {
        let name = definition.name().as_str().to_owned();
        if self.generics.iter().any(|(existing, _)| existing == &name) {
            return Ok(());
        }
        let frame = definition
            .frame_body()
            .map(|(parameters, variants)| (parameters.to_vec(), variants.to_vec()));
        self.generics.push((name, definition));
        if let Some((parameters, variants)) = frame {
            let outer_binders = std::mem::replace(
                &mut self.binders,
                parameters
                    .iter()
                    .map(|parameter| parameter.as_str().to_owned())
                    .collect(),
            );
            let mut walked = Ok(());
            for variant in variants {
                if let Some(payload) = variant.payload {
                    walked = self.visit_reference(&payload);
                    if walked.is_err() {
                        break;
                    }
                }
                if let Some(relation) = variant.stream_relation {
                    walked = self.visit_stream(relation.stream_name());
                    if walked.is_err() {
                        break;
                    }
                }
            }
            self.binders = outer_binders;
            walked?;
        }
        Ok(())
    }

    fn visit_name(&mut self, name: &Name) -> Result<(), SchemaError> {
        // A type parameter in scope is a binder, not a declared type: it
        // resolves here and contributes nothing further to the closure.
        if self.binders.contains(name.as_str()) {
            return Ok(());
        }
        if self
            .declarations
            .iter()
            .any(|(existing, _)| existing == name.as_str())
            || self
                .imports
                .iter()
                .any(|(existing, _)| existing == name.as_str())
        {
            return Ok(());
        }
        if let Some(declaration) = self.namespace_declaration(name.as_str()) {
            return self.visit_declaration(declaration.clone());
        }
        if let Some(root) = self.schema.root_enum_named(name.as_str()) {
            let declaration = Declaration::public(TypeDeclaration::Enum(root.clone()));
            return self.visit_declaration(declaration);
        }
        if let Some(import) = self
            .schema
            .imports()
            .iter()
            .find(|import| &import.local_name == name)
        {
            self.imports
                .push((name.as_str().to_owned(), import.clone()));
            return Ok(());
        }
        Err(SchemaError::FamilyReferenceNotFound {
            family: self.family.to_owned(),
            name: name.as_str().to_owned(),
        })
    }
}
