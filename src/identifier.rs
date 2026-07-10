//! Minted nominal identifiers and their name table — the stringless
//! identity substrate the target `CoreSchema` is built on.
//!
//! Every declaration — type, field, variant, and generic — is addressed by a
//! [`NominalIdentifier`], a minted 128-bit value that stays fixed across all
//! edits including rename. The human names live apart in a [`NameTable`] mapping
//! identifier to current name, so structure and names move independently. See
//! the "Core and True schema" section of `ARCHITECTURE.md`.
//!
//! # PROVISIONAL — this minting mechanism is possibly unreliable
//!
//! The minting scheme here is the provisional one sanctioned by the OPEN
//! section "deterministic identifier and NameTable creation" of
//! `ARCHITECTURE.md`. It is NOT a solution to that open problem, and nothing in
//! this module should be read as claiming otherwise.
//!
//! What works: an identifier is minted as a deterministic hash of
//! `(declaration kind, fully-qualified name at introduction)`, so minting is
//! order-independent by construction — the same declarations in any source
//! order yield identical identifiers and, after canonical sorting, identical
//! `NameTable` bytes. On load, a declaration whose current name is already in a
//! prior `NameTable` reuses that identifier; a miss mints fresh. A rename
//! performed through the table keeps the identifier and only updates the name,
//! so a renamed declaration stays reachable by its current name.
//!
//! What is unsolved, and why this is possibly unreliable:
//!
//! - Selecting which persisted `NameTable` applies to a source being loaded is
//!   itself a lineage question, and lineage is answered by the core hash, which
//!   cannot be computed until identifiers are assigned. That bootstrap
//!   circularity is not resolved here.
//! - An out-of-band rename — editing the name directly in `.schema` source
//!   rather than through [`NameTable::rename`] — is indistinguishable from a
//!   delete-plus-add: the old current name is gone from the table, so
//!   re-association misses and mints a FRESH identifier, breaking lineage.
//!
//! Both hazards stay OPEN in `ARCHITECTURE.md`. The real answer may only emerge
//! after the system is implemented and used; until then this substrate must be
//! treated as possibly unreliable, and it is deliberately not yet wired into
//! `TrueSchema` or the hashing domains.

use nota::{Block, Delimiter, NotaBlock, NotaDecode, NotaDecodeError, NotaEncode};

use crate::{SchemaError, schema::Name};

/// The declaration kind a nominal identifier addresses. The kind is folded into
/// the minted hash so two declarations of different kinds that happen to share a
/// fully-qualified name never collide, and it is carried on the identifier so
/// the kind stays inspectable and orderable.
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
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
pub enum DeclarationKind {
    Type,
    Field,
    Variant,
    Generic,
}

impl DeclarationKind {
    /// The stable per-kind tag folded into the minted hash. These values are
    /// part of the minting domain: changing one re-mints every identifier of
    /// that kind, so they are fixed.
    fn mint_tag(self) -> u8 {
        match self {
            Self::Type => 0,
            Self::Field => 1,
            Self::Variant => 2,
            Self::Generic => 3,
        }
    }
}

/// A minted 128-bit nominal identifier for one declaration.
///
/// The identifier orders first by [`DeclarationKind`] and then by its 16-byte
/// digest, giving a total order independent of construction order. It is minted
/// once at introduction through [`NominalIdentifier::mint`] and preserved across
/// every edit, including rename.
#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
)]
pub struct NominalIdentifier {
    kind: DeclarationKind,
    digest: [u8; 16],
}

impl NominalIdentifier {
    /// The blake3 `derive_key` context that domain-separates nominal-identifier
    /// minting from the content-identity hash domains.
    const MINT_CONTEXT: &'static str = "schema 2026-07-10 provisional nominal identifier";

    /// Mint the identifier of a declaration as a deterministic 128-bit hash of
    /// its kind and fully-qualified name at introduction. Minting is a pure
    /// function of its inputs — it never reads source order — so the same
    /// declaration always mints the same identifier.
    pub fn mint(kind: DeclarationKind, fully_qualified_name: &str) -> Self {
        let mut hasher = blake3::Hasher::new_derive_key(Self::MINT_CONTEXT);
        hasher.update(&[kind.mint_tag()]);
        hasher.update(fully_qualified_name.as_bytes());
        let hash = hasher.finalize();
        let mut digest = [0u8; 16];
        digest.copy_from_slice(&hash.as_bytes()[..16]);
        Self { kind, digest }
    }

    pub fn kind(&self) -> DeclarationKind {
        self.kind
    }

    /// The 32-character lowercase-hex projection of the 128-bit digest, used as
    /// the identifier's NOTA leaf and human-facing address.
    pub fn to_hex(&self) -> String {
        self.digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// Parse a 32-character lowercase-hex digest back into 16 bytes. Any other
    /// length or a non-hex character yields `None`.
    fn digest_from_hex(hex: &str) -> Option<[u8; 16]> {
        if hex.len() != 32 {
            return None;
        }
        let mut digest = [0u8; 16];
        for (index, slot) in digest.iter_mut().enumerate() {
            *slot = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok()?;
        }
        Some(digest)
    }
}

/// A `NominalIdentifier` projects to the positional NOTA record
/// `(<Kind> <hex-digest>)`: the kind as its bare enum atom and the digest as a
/// 32-character lowercase-hex leaf.
impl NotaDecode for NominalIdentifier {
    fn from_nota_block(block: &Block) -> Result<Self, NotaDecodeError> {
        let children = NotaBlock::new(block).expect_children(
            Delimiter::Parenthesis,
            "NominalIdentifier",
            2,
        )?;
        let kind = DeclarationKind::from_nota_block(&children[0])?;
        let hex = NotaBlock::new(&children[1]).parse_string()?;
        let digest = Self::digest_from_hex(&hex).ok_or_else(|| NotaDecodeError::InvalidValue {
            type_name: "NominalIdentifier",
            value: hex,
            reason: "expected 32 lowercase hexadecimal digits".to_owned(),
        })?;
        Ok(Self { kind, digest })
    }
}

impl NotaEncode for NominalIdentifier {
    fn to_nota(&self) -> String {
        format!("({} {})", self.kind.to_nota(), self.to_hex())
    }
}

/// One row of a [`NameTable`]: a minted identifier paired with the declaration's
/// current human name.
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
pub struct NameEntry {
    identifier: NominalIdentifier,
    name: Name,
}

impl NameEntry {
    pub fn identifier(&self) -> NominalIdentifier {
        self.identifier
    }

    pub fn name(&self) -> &Name {
        &self.name
    }
}

/// A mapping from [`NominalIdentifier`] to current human [`Name`].
///
/// Entries are held sorted by identifier, so a table's canonical rkyv bytes
/// depend only on its contents and never on the order declarations were added.
/// Two tables with the same identifier/name pairs therefore serialize to
/// identical bytes regardless of construction order.
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
pub struct NameTable {
    entries: Vec<NameEntry>,
}

impl NameTable {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn entries(&self) -> &[NameEntry] {
        &self.entries
    }

    /// The current name of an identifier, if the table holds it.
    pub fn name_of(&self, identifier: &NominalIdentifier) -> Option<&Name> {
        self.entries
            .iter()
            .find(|entry| &entry.identifier == identifier)
            .map(|entry| &entry.name)
    }

    /// The identifier currently bound to a name of the given kind, if any. This
    /// is the reachable-by-current-name lookup: after a rename through the
    /// table, only the new name resolves, and the old name resolves to nothing.
    pub fn identifier_of(&self, kind: DeclarationKind, name: &Name) -> Option<NominalIdentifier> {
        self.entries
            .iter()
            .find(|entry| entry.identifier.kind == kind && &entry.name == name)
            .map(|entry| entry.identifier)
    }

    /// The identifier a declaration should carry: reuse the one already bound to
    /// its current name and kind, or mint a fresh one on a miss. This is the
    /// provisional re-association step — see the module doc for why an
    /// out-of-band rename defeats it.
    pub fn associate(
        &self,
        kind: DeclarationKind,
        fully_qualified_name: &Name,
    ) -> NominalIdentifier {
        self.identifier_of(kind, fully_qualified_name)
            .unwrap_or_else(|| NominalIdentifier::mint(kind, fully_qualified_name.as_str()))
    }

    /// Build a table for a set of declarations, re-associating each against a
    /// prior table (use [`NameTable::empty`] when there is none). Unchanged
    /// names keep their identifiers, renamed-through-table names keep theirs,
    /// and genuinely new names mint fresh. The result is canonicalized by
    /// sorting on identifier, so declaration order does not affect the bytes.
    pub fn build(
        prior: &NameTable,
        declarations: impl IntoIterator<Item = (DeclarationKind, Name)>,
    ) -> Self {
        let mut entries: Vec<NameEntry> = declarations
            .into_iter()
            .map(|(kind, name)| {
                let identifier = prior.associate(kind, &name);
                NameEntry { identifier, name }
            })
            .collect();
        entries.sort_by_key(|entry| entry.identifier);
        Self { entries }
    }

    /// Rename a declaration through the table: keep its identifier and replace
    /// only the name. The identifier is unchanged, so the entry stays in
    /// canonical position and lineage is preserved. A rename of an absent
    /// identifier is a typed error.
    pub fn rename(
        &mut self,
        identifier: &NominalIdentifier,
        new_name: Name,
    ) -> Result<(), SchemaError> {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| &entry.identifier == identifier)
            .ok_or_else(|| SchemaError::NameTableIdentifierAbsent {
                identifier: identifier.to_hex(),
            })?;
        entry.name = new_name;
        Ok(())
    }

    /// The table's canonical rkyv bytes. Equal tables produce equal bytes
    /// because entries are held sorted by identifier.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, SchemaError> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .map(|bytes| bytes.to_vec())
            .map_err(|_| SchemaError::ArchiveEncode)
    }
}
