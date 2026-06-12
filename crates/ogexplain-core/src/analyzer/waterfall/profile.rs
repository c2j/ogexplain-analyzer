//! NodeType → resource profile mapping.
//!
//! Uses match expression (not HashMap) because NodeType contains variant data
//! (Streaming(StreamingType), Unknown(String)) which cannot derive Hash.

use super::types::ResourceDimension;
use crate::model::node_type::NodeType;

/// Resource profile characterizing what resources a node type primarily consumes.
#[derive(Debug, Clone)]
pub struct ResourceProfile {
    /// The primary resource dimensions consumed by this node type.
    pub primary_dimensions: &'static [ResourceDimension],
    /// Human-readable description of the resource usage characteristics.
    pub description: &'static str,
}

impl ResourceProfile {
    /// Get the resource profile for a given NodeType.
    ///
    /// Uses a match expression to ensure compile-time exhaustiveness
    /// across all 105+ NodeType variants.
    pub fn for_node(node_type: &NodeType) -> &'static ResourceProfile {
        match node_type {
            // ---- Scan nodes: IO-bound + CPU (filter evaluation) ----
            NodeType::SeqScan | NodeType::PartitionedSeqScan | NodeType::SampleScan => {
                &SCAN_IO_BOUND
            }

            NodeType::IndexScan
            | NodeType::PartitionedIndexScan
            | NodeType::IndexOnlyScan
            | NodeType::PartitionedIndexOnlyScan
            | NodeType::AnnIndexScan
            | NodeType::CStoreIndexScan
            | NodeType::CStoreIndexCtidScan
            | NodeType::CStoreIndexHeapScan => &INDEX_IO_CPU,

            NodeType::BitmapIndexScan | NodeType::PartitionedBitmapIndexScan => &BITMAP_CPU,

            NodeType::BitmapHeapScan | NodeType::PartitionedBitmapHeapScan => &BITMAP_HEAP_IO_MEM,

            NodeType::TidScan | NodeType::TidRangeScan => &SCAN_IO_BOUND,

            NodeType::SubqueryScan | NodeType::CteScan | NodeType::VectorSubqueryScan => {
                &SUBQUERY_CPU_MEM
            }

            NodeType::FunctionScan => &FUNCTION_CPU,
            NodeType::ValuesScan => &VALUES_CPU_MEM,
            NodeType::WorkTableScan => &WORKTABLE_MEM,

            NodeType::ForeignScan
            | NodeType::PartitionedForeignScan
            | NodeType::VectorForeignScan => &FOREIGN_NETWORK_IO,

            NodeType::CStoreScan
            | NodeType::PartitionedCStoreScan
            | NodeType::TsStoreScan
            | NodeType::ImCStoreScan => &CSTORE_IO_CPU,

            NodeType::DataNodeScan => &DATANODE_NETWORK,

            // ---- Join nodes: CPU + Memory-intensive ----
            NodeType::NestedLoop | NodeType::VectorNestLoop => &NESTED_LOOP_CPU_IO,

            NodeType::HashJoin | NodeType::VectorHashJoin | NodeType::VectorSonicHashJoin => {
                &HASH_JOIN_CPU_MEM
            }

            NodeType::MergeJoin | NodeType::VectorMergeJoin | NodeType::VectorAsofJoin => {
                &MERGE_JOIN_CPU_MEM
            }

            // ---- Aggregate nodes: Memory + CPU ----
            NodeType::Aggregate
            | NodeType::GroupAggregate
            | NodeType::WindowAgg
            | NodeType::VectorWindowAgg => &GROUP_AGG_CPU_MEM,

            NodeType::HashAggregate
            | NodeType::DummyHashAggregate
            | NodeType::VectorHashAggregate
            | NodeType::VectorSonicHashAggregate => &HASH_AGG_CPU_MEM,

            NodeType::VectorAggregate | NodeType::VectorSortAggregate => &HASH_AGG_CPU_MEM,

            NodeType::Group | NodeType::VectorGroup => &GROUP_AGG_CPU_MEM,

            // ---- Sort nodes: Memory + IO (spill) ----
            NodeType::Sort | NodeType::GroupSort | NodeType::VectorSort => &SORT_MEM_IO,

            // ---- DML nodes: IO (write path) ----
            NodeType::Insert
            | NodeType::Update
            | NodeType::Delete
            | NodeType::Merge
            | NodeType::Replace
            | NodeType::ModifyTable => &DML_IO_CPU,

            NodeType::VectorInsert
            | NodeType::VectorUpdate
            | NodeType::VectorDelete
            | NodeType::VectorMerge => &DML_IO_CPU,

            // ---- SetOp / Append nodes ----
            NodeType::SetOp
            | NodeType::HashSetOp
            | NodeType::VectorSetOp
            | NodeType::VectorHashSetOp
            | NodeType::Unique
            | NodeType::VectorUnique => &SETOP_CPU_MEM,

            NodeType::Append | NodeType::VectorAppend => &APPEND_CPU,

            NodeType::MergeAppend => &MERGE_APPEND_IO_MEM,
            NodeType::RecursiveUnion => &RECURSIVE_MEM_CPU,

            NodeType::BitmapAnd
            | NodeType::BitmapOr
            | NodeType::CStoreIndexAnd
            | NodeType::CStoreIndexOr => &BITMAP_OP_CPU,

            // ---- Streaming nodes: Network-bound ----
            NodeType::Streaming(_) | NodeType::VectorStreaming(_) => &STREAMING_NETWORK,

            // ---- Other nodes: Mixed ----
            NodeType::Result | NodeType::VectorResult | NodeType::Limit | NodeType::VectorLimit => {
                &RESULT_CPU
            }

            NodeType::ProjectSet => &PROJECTSET_CPU_MEM,
            NodeType::Hash => &HASH_MEM,

            NodeType::Materialize | NodeType::VectorMaterialize => &MATERIALIZE_MEM,

            NodeType::LockRows => &LOCKROWS_CPU,

            NodeType::PartitionIterator | NodeType::VectorPartitionIterator => {
                &PARTITION_ITER_IO_CPU
            }

            NodeType::RowAdapter | NodeType::VectorAdapter => &ADAPTER_CPU_MEM,

            NodeType::StartWithOp => &RECURSIVE_MEM_CPU,
            NodeType::RemoteSubplanScan => &DATANODE_NETWORK,

            NodeType::Gather | NodeType::GatherMerge => &GATHER_NETWORK,

            // ---- Unknown ----
            NodeType::Unknown(_) => &UNKNOWN,
        }
    }
}

// ---- Static profile instances ----

static SCAN_IO_BOUND: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Sequential scan — IO-bound, filter evaluation costs CPU",
};

static INDEX_IO_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Index scan — random IO + index cache memory",
};

static BITMAP_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Bitmap index scan — CPU for bitmap construction",
};

static BITMAP_HEAP_IO_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Bitmap heap scan — IO via bitmap + memory for bitmap",
};

static SUBQUERY_CPU_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Subquery scan — CPU for evaluation + memory for result cache",
};

static FUNCTION_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Function scan — CPU for function execution",
};

static VALUES_CPU_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Values scan — CPU + memory for in-memory rows",
};

static WORKTABLE_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::Memory, ResourceDimension::CpuTime],
    description: "Work table scan — memory for recursive query state",
};

static FOREIGN_NETWORK_IO: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Foreign scan — network for external data access",
};

static CSTORE_IO_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "CStore scan — columnar IO + CPU for decompression",
};

static DATANODE_NETWORK: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Data node scan — network for remote data fetch",
};

static NESTED_LOOP_CPU_IO: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Nested loop — O(n×m) CPU, inner index IO",
};

static HASH_JOIN_CPU_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Hash join — memory for hash table, CPU for probing, may spill",
};

static MERGE_JOIN_CPU_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Merge join — CPU for sorted merge, memory for sort state",
};

static GROUP_AGG_CPU_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Group aggregate — CPU for grouping, memory for sort state",
};

static HASH_AGG_CPU_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Hash aggregate — memory for hash table, CPU for hashing, may spill",
};

static SORT_MEM_IO: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::Memory, ResourceDimension::CpuTime],
    description: "Sort — memory-intensive, may spill to disk (IO)",
};

static DML_IO_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "DML — write path IO + WAL logging",
};

static SETOP_CPU_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Set operation — CPU for comparison + memory for dedup",
};

static APPEND_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Append — CPU for concatenation",
};

static MERGE_APPEND_IO_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Merge append — IO + memory for sorted merge",
};

static RECURSIVE_MEM_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::Memory, ResourceDimension::CpuTime],
    description: "Recursive union — memory for CTE state + CPU for recursion",
};

static BITMAP_OP_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Bitmap operation — CPU for bitmap AND/OR",
};

static STREAMING_NETWORK: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Streaming — network data motion between datanodes",
};

static RESULT_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Result/Limit — CPU for projection/filtering",
};

static PROJECTSET_CPU_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "ProjectSet — CPU for SRF evaluation + memory for results",
};

static HASH_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::Memory],
    description: "Hash — memory for hash table construction",
};

static MATERIALIZE_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::Memory],
    description: "Materialize — memory for materialized result cache",
};

static LOCKROWS_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Lock rows — CPU for row-level locking",
};

static PARTITION_ITER_IO_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Partition iterator — IO for partition access + CPU for pruning",
};

static ADAPTER_CPU_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Row/Vector adapter — CPU for format conversion + memory buffer",
};

static GATHER_NETWORK: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Gather — network for result collection",
};

static UNKNOWN: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Unknown node type — resource profile uncertain",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_for_streaming() {
        use crate::model::StreamingType;
        let st = NodeType::Streaming(StreamingType::Gather);
        let profile = ResourceProfile::for_node(&st);
        assert!(profile.description.contains("network"));
    }

    #[test]
    fn test_profile_for_all_scans() {
        let scans = [
            NodeType::SeqScan,
            NodeType::IndexScan,
            NodeType::CStoreScan,
            NodeType::BitmapIndexScan,
            NodeType::ForeignScan,
            NodeType::DataNodeScan,
        ];
        for nt in &scans {
            let profile = ResourceProfile::for_node(nt);
            assert!(
                !profile.primary_dimensions.is_empty(),
                "No profile for {:?}",
                nt
            );
        }
    }

    #[test]
    fn test_profile_for_unknown() {
        let unk = NodeType::Unknown("CustomNode".to_string());
        let profile = ResourceProfile::for_node(&unk);
        assert!(profile.description.contains("Unknown"));
    }

    #[test]
    fn test_profile_for_all_variants() {
        // Verify that every variant mentioned in the exhaustive match
        // returns a valid profile with non-empty dimensions.
        let cases = [
            NodeType::SeqScan,
            NodeType::PartitionedSeqScan,
            NodeType::SampleScan,
            NodeType::IndexScan,
            NodeType::PartitionedIndexScan,
            NodeType::IndexOnlyScan,
            NodeType::PartitionedIndexOnlyScan,
            NodeType::BitmapIndexScan,
            NodeType::PartitionedBitmapIndexScan,
            NodeType::BitmapHeapScan,
            NodeType::PartitionedBitmapHeapScan,
            NodeType::TidScan,
            NodeType::TidRangeScan,
            NodeType::SubqueryScan,
            NodeType::FunctionScan,
            NodeType::ValuesScan,
            NodeType::CteScan,
            NodeType::WorkTableScan,
            NodeType::ForeignScan,
            NodeType::PartitionedForeignScan,
            NodeType::CStoreScan,
            NodeType::PartitionedCStoreScan,
            NodeType::TsStoreScan,
            NodeType::AnnIndexScan,
            NodeType::CStoreIndexScan,
            NodeType::CStoreIndexCtidScan,
            NodeType::CStoreIndexHeapScan,
            NodeType::ImCStoreScan,
            NodeType::VectorSubqueryScan,
            NodeType::VectorForeignScan,
            NodeType::DataNodeScan,
            NodeType::NestedLoop,
            NodeType::VectorNestLoop,
            NodeType::HashJoin,
            NodeType::VectorHashJoin,
            NodeType::VectorSonicHashJoin,
            NodeType::MergeJoin,
            NodeType::VectorMergeJoin,
            NodeType::VectorAsofJoin,
            NodeType::Aggregate,
            NodeType::GroupAggregate,
            NodeType::HashAggregate,
            NodeType::DummyHashAggregate,
            NodeType::VectorAggregate,
            NodeType::VectorHashAggregate,
            NodeType::VectorSonicHashAggregate,
            NodeType::VectorSortAggregate,
            NodeType::Group,
            NodeType::VectorGroup,
            NodeType::WindowAgg,
            NodeType::VectorWindowAgg,
            NodeType::Unique,
            NodeType::VectorUnique,
            NodeType::Sort,
            NodeType::GroupSort,
            NodeType::VectorSort,
            NodeType::SetOp,
            NodeType::HashSetOp,
            NodeType::VectorSetOp,
            NodeType::VectorHashSetOp,
            NodeType::Append,
            NodeType::VectorAppend,
            NodeType::MergeAppend,
            NodeType::RecursiveUnion,
            NodeType::BitmapAnd,
            NodeType::BitmapOr,
            NodeType::CStoreIndexAnd,
            NodeType::CStoreIndexOr,
            NodeType::Insert,
            NodeType::Update,
            NodeType::Delete,
            NodeType::Merge,
            NodeType::Replace,
            NodeType::VectorInsert,
            NodeType::VectorUpdate,
            NodeType::VectorDelete,
            NodeType::VectorMerge,
            NodeType::Result,
            NodeType::VectorResult,
            NodeType::ProjectSet,
            NodeType::Hash,
            NodeType::Materialize,
            NodeType::VectorMaterialize,
            NodeType::Limit,
            NodeType::VectorLimit,
            NodeType::LockRows,
            NodeType::PartitionIterator,
            NodeType::VectorPartitionIterator,
            NodeType::RowAdapter,
            NodeType::VectorAdapter,
            NodeType::StartWithOp,
            NodeType::ModifyTable,
            NodeType::RemoteSubplanScan,
            NodeType::Gather,
            NodeType::GatherMerge,
        ];
        for nt in &cases {
            let profile = ResourceProfile::for_node(nt);
            assert!(
                !profile.primary_dimensions.is_empty(),
                "No profile for {:?}",
                nt
            );
        }
    }
}
