//! Content identity for semantic schema values.
//!
//! Two blake3 hash domains exist, each domain-separated through its own
//! `derive_key` context so hashes over identical bytes can never collide:
//!
//! - the CORE hash, over the stringless [`crate::CoreSchema`] substrate's
//!   canonical bytes — nominal identifiers plus structure, with
//!   `SchemaIdentity` and every human name outside the hashed bytes. It is the
//!   structural LINEAGE ADDRESS: equal core hash means compatible, shared
//!   ancestry, and a rename never moves it because names live in the
//!   [`crate::NameTable`], not the substrate; and
//! - the TRUE/NAME hash, over the full human-facing view — the projected
//!   name-bearing tree including `SchemaIdentity` and every current name. It is
//!   the per-version human-view address: it MOVES on rename and lives outside
//!   the lineage receipt chain, where the core hash does not.
//!
//! The core hash excludes `SchemaIdentity`; only the true/name hash carries it.

use std::fmt;

use nota::{Block, NotaBlock, NotaDecode, NotaDecodeError, NotaEncode};

use crate::{SchemaError, view::TrueSchema};

/// The hash domains content identity is derived under. Each domain carries its
/// own blake3 `derive_key` context string, so hashes over identical bytes in
/// different domains are structurally distinct values. The core and true/name
/// domains are minted fresh for the Core/True split: the retired whole-schema
/// domain hashed the identity-bearing projected tree AS the lineage address,
/// and that semantics does not survive under a reused context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HashDomain {
    CoreSchema,
    TrueName,
}

impl HashDomain {
    fn context(self) -> &'static str {
        match self {
            Self::CoreSchema => "schema 2026-07-10 core structural lineage address",
            Self::TrueName => "schema 2026-07-10 true-name human view identity",
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

    /// Parse a 64-character lowercase-hex address back into 32 bytes. Any other
    /// length or a non-hex character yields `None`. This is the inverse of
    /// [`ContentHash::to_hex`], so an address survives a NOTA round trip.
    fn from_hex(hex: &str) -> Option<Self> {
        if hex.len() != 64 {
            return None;
        }
        let mut bytes = [0u8; 32];
        for (index, slot) in bytes.iter_mut().enumerate() {
            *slot = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok()?;
        }
        Some(Self(bytes))
    }
}

/// A `ContentHash` projects to its 64-character lowercase-hex address as a
/// single NOTA leaf, so a receipt edge keyed by a hash pair round-trips through
/// the human projection.
impl NotaDecode for ContentHash {
    fn from_nota_block(block: &Block) -> Result<Self, NotaDecodeError> {
        let hex = NotaBlock::new(block).parse_string()?;
        Self::from_hex(&hex).ok_or_else(|| NotaDecodeError::InvalidValue {
            type_name: "ContentHash",
            value: hex,
            reason: "expected 64 lowercase hexadecimal digits".to_owned(),
        })
    }
}

impl NotaEncode for ContentHash {
    fn to_nota(&self) -> String {
        self.to_hex()
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

impl TrueSchema {
    /// The CORE hash: blake3 over the stringless [`crate::CoreSchema`]
    /// substrate's canonical bytes, under the core-schema domain. Structure and
    /// nominal identifiers only — `SchemaIdentity` and every human name are
    /// outside these bytes, so a rename through the [`crate::NameTable`] never
    /// moves it. This is the structural LINEAGE ADDRESS: equal core hash means
    /// compatible, shared ancestry, and lineage receipt edges are keyed by it.
    pub fn core_hash(&self) -> Result<ContentHash, SchemaError> {
        let bytes = self.core().canonical_bytes()?;
        Ok(ContentHash::derive(HashDomain::CoreSchema, &bytes))
    }

    /// The TRUE/NAME hash: blake3 over the full human-facing view — the
    /// projected name-bearing tree including `SchemaIdentity` and every current
    /// name — under the true-name domain. It MOVES on rename and is the
    /// per-version human-view address, distinct from and outside the core
    /// hash's lineage receipt chain.
    pub fn true_name_hash(&self) -> Result<ContentHash, SchemaError> {
        let bytes = self.to_binary_bytes()?;
        Ok(ContentHash::derive(HashDomain::TrueName, &bytes))
    }
}
