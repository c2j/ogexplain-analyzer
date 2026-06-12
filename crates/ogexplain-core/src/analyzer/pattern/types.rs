//! Anti-pattern matching types.
//!
//! Core data structures for anti-pattern detection:
//! - [`MatchResult`]: The result of a successful anti-pattern match on a plan tree
//! - [`Evidence`]: Evidence metadata attached to a [`Finding`](crate::analyzer::report::Finding)
//! - [`MatchedNode`]: A single captured node within a match

use serde::Serialize;
use std::collections::HashMap;

/// The result of a successful anti-pattern match.
///
/// Contains the matched nodes (by capture name), the ancestor chain,
/// and a reference to the root of the matched subtree.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MatchResult<'a> {
    /// Anti-pattern identifier (e.g., `"ANTI-005"`)
    pub pattern_id: String,
    /// Named captures from the pattern match.
    /// Keys are capture names (e.g., `"mat1"`, `"nl"`), values are node references.
    pub captures: HashMap<String, &'a crate::model::PlanNode>,
    /// Ancestor chain from the plan root to the parent of `matched_node`.
    /// The first element is the plan root; the last is the direct parent.
    /// Empty if `matched_node` is the plan root.
    pub ancestors: Vec<&'a crate::model::PlanNode>,
    /// The root node of the matched subtree.
    pub matched_node: &'a crate::model::PlanNode,
}

/// Evidence metadata attached to a finding produced by an anti-pattern match.
///
/// This is an incremental extension to [`Finding`](crate::analyzer::report::Finding):
/// classic diagnostic rules set `evidence` to `None`, while anti-pattern rules
/// populate it with match details.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[non_exhaustive]
pub struct Evidence {
    /// Anti-pattern identifier (e.g., `"ANTI-005"`)
    pub pattern_id: String,
    /// Confidence score in \[0.0, 1.0\].
    /// Structural matches default to `1.0`; partial or heuristic matches may be lower.
    pub confidence: f64,
    /// List of captured nodes in this match.
    pub matched_nodes: Vec<MatchedNode>,
    /// IDs of classic diagnostic rules that this anti-pattern overlaps with.
    /// Used for potential future deduplication.
    pub related_classic_rules: Vec<String>,
}

/// A single node captured during an anti-pattern match.
///
/// Provides enough context to display the matched portion of the plan tree
/// without retaining full node references.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[non_exhaustive]
pub struct MatchedNode {
    /// Capture name used in the pattern definition (e.g., `"mat1"`, `"nl"`)
    pub capture_name: String,
    /// Line number of the node in the original EXPLAIN output
    pub line_number: usize,
    /// String representation of the node type (e.g., `"Materialize"`, `"NestedLoop"`)
    pub node_type: String,
    /// Relation (table) name, if applicable
    pub relation: Option<String>,
}
