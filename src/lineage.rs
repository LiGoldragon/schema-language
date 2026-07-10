//! The in-crate lineage graph: the receipt edges that connect schema versions
//! by their core hashes. See the "Hashing and lineage" part of the "Core and
//! True schema" section of `ARCHITECTURE.md`.
//!
//! Each accepted edit produces a [`SchemaEditReceipt`] keyed by its (parent core
//! hash -> child core hash) pair. A structural edit moves the core hash, so its
//! edge advances the graph; a rename leaves the core hash fixed, so its edge is
//! a self-loop that records a `NameTable` delta on the chain without advancing
//! it. Two questions the version-control layer asks are pure walks over these
//! stored edges:
//!
//! - the historical-to-current CONVERSION CHAIN between two versions is the
//!   composition of the receipts along the path from the older core hash to the
//!   newer one, so a value two versions old is carried to current by applying
//!   each edge's migration in order; and
//! - COMMON-ANCESTOR search between two core hashes is a backward walk to the
//!   nearest core hash from which both descend.
//!
//! This is the typed representation the schema daemon will later persist; the
//! graph invents no storage of its own.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::{ContentHash, SchemaEditReceipt};

/// A lineage graph over schema core hashes: the accepted [`SchemaEditReceipt`]
/// edges, each an edit from a parent core hash to a child core hash. The graph
/// is the receipt store the daemon later persists; navigation over it is a pure
/// walk.
#[derive(
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
    nota::NotaDecode,
    nota::NotaEncode,
    Clone,
    Debug,
    Default,
    Eq,
    PartialEq,
)]
pub struct LineageGraph {
    edges: Vec<SchemaEditReceipt>,
}

impl LineageGraph {
    pub fn new() -> Self {
        Self { edges: Vec::new() }
    }

    /// Build a graph from an existing set of receipt edges, discarding duplicate
    /// edges so the edge set stays hygienic: an edge is fully determined by its
    /// (parent core hash, child core hash, effect) triple, so a second identical
    /// edge carries no lineage information a walk could use.
    pub fn from_edges(edges: impl IntoIterator<Item = SchemaEditReceipt>) -> Self {
        let mut graph = Self::new();
        for edge in edges {
            graph.record(edge);
        }
        graph
    }

    /// Record one accepted edit as an edge on the chain. An edge equal to one
    /// already stored is dropped: walks are visited-guarded, so a duplicate edge
    /// never changes a reachable set, and keeping it would only bloat the store.
    pub fn record(&mut self, receipt: SchemaEditReceipt) {
        if !self.edges.contains(&receipt) {
            self.edges.push(receipt);
        }
    }

    pub fn edges(&self) -> &[SchemaEditReceipt] {
        &self.edges
    }

    /// The receipts along the conversion path from an older core hash `from` to
    /// a newer core hash `to`, in application order, or `None` when no path
    /// connects them. Composing the returned edges' migrations carries a value
    /// authored under `from` to the shape `to` denotes; an empty chain means the
    /// two hashes are already equal. Rename edges are self-loops that never move
    /// the core hash, so they do not lie on any conversion path.
    pub fn conversion_chain(
        &self,
        from: &ContentHash,
        to: &ContentHash,
    ) -> Option<Vec<&SchemaEditReceipt>> {
        if from == to {
            return Some(Vec::new());
        }
        // BFS forward over structural edges, remembering the edge that first
        // reached each core hash so the path can be reconstructed on arrival.
        let mut reached: HashMap<ContentHash, usize> = HashMap::new();
        let mut visited: HashSet<ContentHash> = HashSet::from([*from]);
        let mut frontier: VecDeque<ContentHash> = VecDeque::from([*from]);
        while let Some(node) = frontier.pop_front() {
            for (index, edge) in self.edges.iter().enumerate() {
                if !edge.advances_from(&node) {
                    continue;
                }
                let child = edge.child_core_hash;
                if visited.insert(child) {
                    reached.insert(child, index);
                    if &child == to {
                        return Some(self.reconstruct_chain(from, to, &reached));
                    }
                    frontier.push_back(child);
                }
            }
        }
        None
    }

    /// Reconstruct the ordered edge chain from `from` to `to`, following the
    /// remembered reaching edge backward from `to` and reversing.
    fn reconstruct_chain(
        &self,
        from: &ContentHash,
        to: &ContentHash,
        reached: &HashMap<ContentHash, usize>,
    ) -> Vec<&SchemaEditReceipt> {
        let mut chain = Vec::new();
        let mut node = *to;
        while &node != from {
            let edge = &self.edges[reached[&node]];
            chain.push(edge);
            node = edge.parent_core_hash;
        }
        chain.reverse();
        chain
    }

    /// The nearest core hash from which both `left` and `right` descend, or
    /// `None` when they share no ancestor. The walk runs backward over
    /// structural edges from both sides; a rename self-loop moves neither side.
    pub fn common_ancestor(&self, left: &ContentHash, right: &ContentHash) -> Option<ContentHash> {
        let left_ancestry = self.ancestry(left);
        // Walk `right`'s ancestry nearest-first; the first hash it shares with
        // `left`'s ancestry is the nearest common ancestor.
        let mut visited: HashSet<ContentHash> = HashSet::from([*right]);
        let mut frontier: VecDeque<ContentHash> = VecDeque::from([*right]);
        while let Some(node) = frontier.pop_front() {
            if left_ancestry.contains(&node) {
                return Some(node);
            }
            self.enqueue_parents(&node, &mut visited, &mut frontier);
        }
        None
    }

    /// Every core hash `hash` descends from, `hash` itself included.
    fn ancestry(&self, hash: &ContentHash) -> HashSet<ContentHash> {
        let mut visited: HashSet<ContentHash> = HashSet::from([*hash]);
        let mut frontier: VecDeque<ContentHash> = VecDeque::from([*hash]);
        while let Some(node) = frontier.pop_front() {
            self.enqueue_parents(&node, &mut visited, &mut frontier);
        }
        visited
    }

    /// Enqueue every not-yet-visited structural parent of `node`.
    fn enqueue_parents(
        &self,
        node: &ContentHash,
        visited: &mut HashSet<ContentHash>,
        frontier: &mut VecDeque<ContentHash>,
    ) {
        for edge in &self.edges {
            if !edge.advances_to(node) {
                continue;
            }
            if visited.insert(edge.parent_core_hash) {
                frontier.push_back(edge.parent_core_hash);
            }
        }
    }
}

impl SchemaEditReceipt {
    /// Whether this edge advances the core hash forward out of `node` — a
    /// structural edit whose parent is `node`. A rename self-loop advances
    /// nothing.
    fn advances_from(&self, node: &ContentHash) -> bool {
        &self.parent_core_hash == node && self.parent_core_hash != self.child_core_hash
    }

    /// Whether this edge advances the core hash forward INTO `node` — a
    /// structural edit whose child is `node`.
    fn advances_to(&self, node: &ContentHash) -> bool {
        &self.child_core_hash == node && self.parent_core_hash != self.child_core_hash
    }
}
