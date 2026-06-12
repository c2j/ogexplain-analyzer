//! Resource waterfall types.
//!
//! All types are independent of PlanNode — computed from immutable plan references.
//! Phase 1 covers CPU Time and Memory; IO and Network are deferred.

use serde::Serialize;

/// Resource dimension consumed by a plan node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[non_exhaustive]
pub enum ResourceDimension {
    /// CPU time (actual.total_time_ms × loops)
    CpuTime,
    /// Memory usage (peak_memory_kb, sort/hash spill)
    Memory,
    /// IO operations (Buffer reads/writes) — Phase 2
    #[serde(skip)]
    Io,
    /// Network transfer (Streaming node data volume) — Phase 2
    #[serde(skip)]
    Network,
}

/// Per-node resource consumption metrics.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct NodeResourceMetrics {
    /// EXPLAIN output line number (unique identifier within the plan).
    pub line_number: usize,
    /// Human-readable node type name (e.g., "Seq Scan", "Hash Join").
    pub node_type: String,
    /// Associated table or relation name (present on scan nodes).
    pub relation: Option<String>,

    // --- CPU dimension ---
    /// Actual CPU time (ms) = total_time_ms × loops.
    /// None when no EXPLAIN ANALYZE data.
    pub cpu_time_ms: Option<f64>,

    // --- Memory dimension ---
    /// Peak memory usage (KB) from structured_props.peak_memory_kb.
    pub peak_memory_kb: Option<f64>,
    /// Sort spill size (KB) from structured_props.sort_disk.
    pub sort_spill_kb: Option<f64>,
    /// Hash spill batches (> 1 indicates spill), from structured_props.hash_batches.
    pub hash_spill_batches: Option<i64>,
    /// Hash memory usage (raw string, e.g. "48kB").
    pub hash_memory_usage: Option<String>,
    /// Whether memory spill was detected (sort or hash spill).
    pub has_memory_spill: bool,

    // --- Subtree reduction ---
    /// Total CPU time (ms) across entire subtree (self + all descendants).
    pub subtree_cpu_time_ms: f64,
    /// Peak memory (KB) across entire subtree (max of self + all descendants).
    pub subtree_peak_memory_kb: f64,
}

/// A single node's resource waterfall entry.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct WaterfallEntry {
    /// Per-node resource metrics.
    pub metrics: NodeResourceMetrics,
    /// Which resource dimensions this node type primarily consumes.
    pub dimensions: Vec<ResourceDimension>,
    /// CPU time as percentage of total plan CPU time.
    pub cpu_percent: f64,
    /// Memory as percentage of total plan peak memory.
    pub memory_percent: f64,
    /// Whether this node is a bottleneck (CPU or Memory exceeds threshold).
    pub is_bottleneck: bool,
    /// Which dimensions triggered the bottleneck.
    pub bottleneck_dimensions: Vec<ResourceDimension>,
    /// Depth in the plan tree (root = 0).
    pub depth: usize,
}

/// Summary of bottleneck nodes found in the plan.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct BottleneckSummary {
    /// CPU bottleneck nodes (line numbers), sorted by CPU time descending (top 5).
    pub cpu_bottlenecks: Vec<usize>,
    /// Memory bottleneck nodes (line numbers), sorted by peak memory descending (top 5).
    pub memory_bottlenecks: Vec<usize>,
    /// Total CPU time (ms) across all nodes.
    pub total_cpu_time_ms: f64,
    /// Maximum single-node peak memory (KB).
    pub max_peak_memory_kb: f64,
    /// Number of nodes with memory spill (sort or hash).
    pub spill_node_count: usize,
}

/// Complete resource waterfall for an execution plan.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct PlanWaterfall {
    /// All nodes' waterfall entries (DFS post-order traversal order).
    pub entries: Vec<WaterfallEntry>,
    /// Bottleneck summary.
    pub bottlenecks: BottleneckSummary,
    /// Total number of nodes in the plan tree.
    pub total_nodes: usize,
    /// Number of nodes with EXPLAIN ANALYZE statistics.
    pub nodes_with_stats: usize,
}
