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
//! What works: minting is a deterministic hash, order-independent by
//! construction — the same declarations in any source order yield identical
//! identifiers and, after canonical sorting, identical `NameTable` bytes. A
//! top-level declaration mints from `(kind, name at introduction)`. A MEMBER —
//! a struct field, an enum variant, or a generic parameter binder — mints from
//! `(kind, owner identifier digest, member local name)` through
//! [`NominalIdentifier::mint_member`]: it is anchored to its OWNER's identifier,
//! never the owner's current name, so renaming the owner leaves every member
//! identifier fixed by construction. On load, a declaration whose kind, owner
//! scope, and current name are already in a prior `NameTable` reuses that
//! identifier; a miss mints fresh. A rename through the table keeps the
//! identifier and only updates the row's name, and a member row carries the
//! owner as a stable identifier plus the member's local name, so an owner rename
//! never leaves a member row carrying a stale owner prefix.
//!
//! What is unsolved, and why this is possibly unreliable:
//!
//! - Selecting which persisted `NameTable` applies to a source being loaded is
//!   itself a lineage question, and lineage is answered by the core hash, which
//!   cannot be computed until identifiers are assigned. That bootstrap
//!   circularity is not resolved here.
//! - An out-of-band rename — editing a name directly in `.schema` source rather
//!   than through [`NameTable::rename`] — is still indistinguishable from a
//!   delete-plus-add: the old current name is gone from the table, so
//!   re-association misses and mints a FRESH identifier, breaking lineage. For a
//!   top-level owner this cascades, because its members are anchored to the
//!   owner identifier that just moved.
//!
//! Both hazards stay OPEN in `ARCHITECTURE.md`. The real answer may only emerge
//! after the system is implemented and used; until then this substrate must be
//! treated as possibly unreliable. This provisional minting IS the current
//! lineage foundation: the core hash rests entirely on the identifier-built
//! canonical bytes, so every hazard above is inherited by lineage identity as it
//! stands today — it is not a dormant mechanism awaiting a later wiring into the
//! hashing domains.

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

    /// Mint the identifier of a member declaration — a field, an enum variant,
    /// or a generic parameter binder — anchored to its OWNER's identifier
    /// rather than the owner's current name. The mint input is the member kind,
    /// the owner's immutable 16-byte digest, and the member's local current
    /// name. Because the owner digest never moves when the owner is renamed,
    /// the member identifier is stable across owner rename by construction, and
    /// two equal member names under different owners still mint distinct
    /// identifiers. This is the anchoring the "allocated once at introduction
    /// and preserved across all edits, including rename" property requires for
    /// members.
    pub fn mint_member(kind: DeclarationKind, owner: &NominalIdentifier, local_name: &str) -> Self {
        let mut hasher = blake3::Hasher::new_derive_key(Self::MINT_CONTEXT);
        hasher.update(&[kind.mint_tag()]);
        hasher.update(&owner.digest);
        hasher.update(local_name.as_bytes());
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

/// One row of a [`NameTable`]: a minted identifier, the identifier of its
/// owner when the declaration is a member, and the declaration's current human
/// name. Member rows (fields, variants, generic binders) carry the owner as a
/// stable identifier and store only the member's LOCAL name, so an owner rename
/// never leaves a member row carrying a stale owner prefix, and re-association
/// scopes a local name to its owner. Top-level rows carry `None` and store the
/// full name.
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
    owner: Option<NominalIdentifier>,
    name: Name,
}

impl NameEntry {
    pub fn identifier(&self) -> NominalIdentifier {
        self.identifier
    }

    /// The owner identifier for a member row, or `None` for a top-level row.
    pub fn owner(&self) -> Option<NominalIdentifier> {
        self.owner
    }

    /// The row's stored name: the LOCAL name for a member, the full name for a
    /// top-level declaration.
    pub fn name(&self) -> &Name {
        &self.name
    }
}

/// One declaration to place in a [`NameTable`] build: its kind, its owner when
/// it is a member, and its stored name (local for members, full for top-level).
/// A `(DeclarationKind, Name)` pair converts to a top-level declaration, so
/// callers that only ever build top-level rows stay terse.
#[derive(Clone, Debug)]
pub struct NameDeclaration {
    kind: DeclarationKind,
    owner: Option<NominalIdentifier>,
    name: Name,
}

impl NameDeclaration {
    pub fn top_level(kind: DeclarationKind, name: Name) -> Self {
        Self {
            kind,
            owner: None,
            name,
        }
    }

    pub fn member(kind: DeclarationKind, owner: NominalIdentifier, local_name: Name) -> Self {
        Self {
            kind,
            owner: Some(owner),
            name: local_name,
        }
    }
}

impl From<(DeclarationKind, Name)> for NameDeclaration {
    fn from((kind, name): (DeclarationKind, Name)) -> Self {
        Self::top_level(kind, name)
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

    /// The current name of an identifier, as a typed projection error when the
    /// table has no entry. Projecting a `CoreSchema` node into its human-facing
    /// form requires every carried identifier to resolve; a miss means the
    /// substrate and the table have diverged.
    pub fn projected_name(&self, identifier: &NominalIdentifier) -> Result<&Name, SchemaError> {
        self.name_of(identifier)
            .ok_or_else(|| SchemaError::CoreProjectionNameAbsent {
                identifier: identifier.to_hex(),
            })
    }

    /// The identifier of a declaration matching the given kind, owner scope, and
    /// stored name, if the table holds it. Top-level lookups pass `owner: None`;
    /// member lookups pass the owner identifier, so a local name resolves only
    /// under its owner and never collides with an equal local name elsewhere.
    fn find(
        &self,
        kind: DeclarationKind,
        owner: Option<&NominalIdentifier>,
        name: &Name,
    ) -> Option<NominalIdentifier> {
        self.entries
            .iter()
            .find(|entry| {
                entry.identifier.kind == kind
                    && entry.owner.as_ref() == owner
                    && &entry.name == name
            })
            .map(|entry| entry.identifier)
    }

    /// The top-level identifier currently bound to a full name of the given
    /// kind, if any. This is the reachable-by-current-name lookup for a
    /// top-level declaration: after a rename through the table, only the new
    /// name resolves, and the old name resolves to nothing.
    pub fn identifier_of(&self, kind: DeclarationKind, name: &Name) -> Option<NominalIdentifier> {
        self.find(kind, None, name)
    }

    /// The member identifier currently bound to a local name under the given
    /// owner and kind, if any. Renaming the owner never moves this binding — the
    /// owner is addressed by its stable identifier, not its current name.
    pub fn member_identifier_of(
        &self,
        kind: DeclarationKind,
        owner: &NominalIdentifier,
        local_name: &Name,
    ) -> Option<NominalIdentifier> {
        self.find(kind, Some(owner), local_name)
    }

    /// The identifier a declaration should carry: reuse the one already bound to
    /// its kind, owner scope, and current name, or mint a fresh one on a miss. A
    /// member mints from its owner's identifier so the mint is stable across an
    /// owner rename. This is the provisional re-association step — see the module
    /// doc for why an out-of-band rename defeats it.
    fn associate(
        &self,
        kind: DeclarationKind,
        owner: Option<&NominalIdentifier>,
        name: &Name,
    ) -> NominalIdentifier {
        self.find(kind, owner, name).unwrap_or_else(|| match owner {
            Some(owner) => NominalIdentifier::mint_member(kind, owner, name.as_str()),
            None => NominalIdentifier::mint(kind, name.as_str()),
        })
    }

    /// Build a table for a set of declarations, re-associating each against a
    /// prior table (use [`NameTable::empty`] when there is none). Unchanged
    /// names keep their identifiers, renamed-through-table names keep theirs,
    /// and genuinely new names mint fresh. The result is canonicalized by
    /// sorting on identifier, so declaration order does not affect the bytes.
    pub fn build<Declared: Into<NameDeclaration>>(
        prior: &NameTable,
        declarations: impl IntoIterator<Item = Declared>,
    ) -> Self {
        let mut entries: Vec<NameEntry> = declarations
            .into_iter()
            .map(|declared| {
                let declared = declared.into();
                let identifier =
                    prior.associate(declared.kind, declared.owner.as_ref(), &declared.name);
                NameEntry {
                    identifier,
                    owner: declared.owner,
                    name: declared.name,
                }
            })
            .collect();
        entries.sort_by_key(|entry| entry.identifier);
        // Collapse identical duplicate rows (same identifier, same name) so
        // multiplicity never leaks into the canonical bytes. Sorting by
        // identifier makes equal rows adjacent, and a row is a pure function of
        // its identifier and name, so `dedup` removes exactly the duplicates
        // and leaves the mapping construction-order-independent.
        entries.dedup();
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
        // Resolve the target row first so the conflict check runs in the row's
        // own owner scope: a member's local name only has to be unique among its
        // siblings, never across the whole table.
        let owner = self
            .entries
            .iter()
            .find(|entry| &entry.identifier == identifier)
            .map(|entry| entry.owner)
            .ok_or_else(|| SchemaError::NameTableIdentifierAbsent {
                identifier: identifier.to_hex(),
            })?;
        // The identifier-to-name mapping is injective per kind within an owner
        // scope: the new name must not already belong to a different identifier
        // of the same kind and owner. Reassigning the same name to the same
        // identifier is a no-op rename and stays legal.
        if let Some(holder) = self.find(identifier.kind, owner.as_ref(), &new_name) {
            if &holder != identifier {
                return Err(SchemaError::NameTableNameConflict {
                    kind: identifier.kind,
                    name: new_name.as_str().to_owned(),
                    holder: holder.to_hex(),
                    requested: identifier.to_hex(),
                });
            }
        }
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| &entry.identifier == identifier)
            .expect("identifier resolved to a row above");
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

    /// Read a table back from its canonical rkyv bytes. This is the inverse of
    /// [`NameTable::canonical_bytes`]: a table encoded and read back is equal to
    /// the original, so the archive is a faithful durable form of the mapping.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, SchemaError> {
        rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes).map_err(|_| SchemaError::ArchiveDecode)
    }
}

/// An in-progress declaration harvest for one `NameTable` build.
///
/// Decomposing a schema into its stringless substrate walks every declaration
/// exactly where it stands, needing the declaration's identifier at the walk
/// site while the full table does not exist yet. The harvest answers each
/// `declare` with the identifier the finished table will hold — re-association
/// against the prior table is a pure per-row function — and `into_table` builds
/// the canonical table from everything declared. Duplicate declarations of the
/// same kind and name collapse to one row, exactly as [`NameTable::build`]
/// guarantees.
pub struct NameHarvest<'prior> {
    prior: &'prior NameTable,
    declarations: Vec<NameDeclaration>,
}

impl<'prior> NameHarvest<'prior> {
    pub fn new(prior: &'prior NameTable) -> Self {
        Self {
            prior,
            declarations: Vec::new(),
        }
    }

    /// Record one top-level declaration and answer the identifier it carries:
    /// the prior table's identifier when the current name is already bound, or
    /// the deterministic fresh mint otherwise. The answer is exactly the
    /// identifier the built table will map to this name.
    pub fn declare(&mut self, kind: DeclarationKind, name: &Name) -> NominalIdentifier {
        let identifier = self.prior.associate(kind, None, name);
        self.declarations
            .push(NameDeclaration::top_level(kind, name.clone()));
        identifier
    }

    /// Record one member declaration under an owner and answer its identifier.
    /// The member mints from the owner's identifier and its local name, so the
    /// answer is stable across an owner rename, and the stored row carries the
    /// owner identifier plus the local name — never a name-qualified prefix that
    /// could go stale.
    pub fn declare_member(
        &mut self,
        kind: DeclarationKind,
        owner: NominalIdentifier,
        local_name: &Name,
    ) -> NominalIdentifier {
        let identifier = self.prior.associate(kind, Some(&owner), local_name);
        self.declarations
            .push(NameDeclaration::member(kind, owner, local_name.clone()));
        identifier
    }

    /// Answer a member's identifier WITHOUT recording a table row. This is the
    /// path for identifier-addressed member positions whose current name is a
    /// pure projection — derived field names — so the table stores only real
    /// name data.
    pub fn associate_member(
        &self,
        kind: DeclarationKind,
        owner: NominalIdentifier,
        local_name: &Name,
    ) -> NominalIdentifier {
        self.prior.associate(kind, Some(&owner), local_name)
    }

    /// Answer a local reference's identifier WITHOUT recording a table row.
    /// Local references — plain type references, application heads, and impl
    /// targets — point AT a declaration that owns its own row, so re-minting
    /// here reuses that declaration's identifier without duplicating its row.
    pub fn associate(&self, kind: DeclarationKind, name: &Name) -> NominalIdentifier {
        self.prior.associate(kind, None, name)
    }

    /// Finish the harvest into the canonical table for everything declared.
    pub fn into_table(self) -> NameTable {
        NameTable::build(self.prior, self.declarations)
    }
}
