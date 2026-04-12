use serde::Serialize;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", content = "variant")]
pub enum NodeType {
    SeqScan,
    PartitionedSeqScan,
    SampleScan,
    IndexScan,
    PartitionedIndexScan,
    IndexOnlyScan,
    PartitionedIndexOnlyScan,
    BitmapIndexScan,
    PartitionedBitmapIndexScan,
    BitmapHeapScan,
    PartitionedBitmapHeapScan,
    TidScan,
    TidRangeScan,
    SubqueryScan,
    FunctionScan,
    ValuesScan,
    CteScan,
    WorkTableScan,
    ForeignScan,
    PartitionedForeignScan,
    CStoreScan,
    PartitionedCStoreScan,
    TsStoreScan,
    AnnIndexScan,
    CStoreIndexScan,
    CStoreIndexCtidScan,
    CStoreIndexHeapScan,
    ImCStoreScan,
    VectorSubqueryScan,
    VectorForeignScan,
    NestedLoop,
    HashJoin,
    MergeJoin,
    VectorNestLoop,
    VectorHashJoin,
    VectorSonicHashJoin,
    VectorMergeJoin,
    VectorAsofJoin,
    Aggregate,
    GroupAggregate,
    HashAggregate,
    VectorAggregate,
    VectorHashAggregate,
    VectorSonicHashAggregate,
    VectorSortAggregate,
    Sort,
    GroupSort,
    VectorSort,
    Group,
    VectorGroup,
    WindowAgg,
    VectorWindowAgg,
    Unique,
    VectorUnique,
    SetOp,
    HashSetOp,
    VectorSetOp,
    VectorHashSetOp,
    Append,
    VectorAppend,
    MergeAppend,
    RecursiveUnion,
    BitmapAnd,
    BitmapOr,
    CStoreIndexAnd,
    CStoreIndexOr,
    Insert,
    Update,
    Delete,
    Merge,
    Replace,
    VectorInsert,
    VectorUpdate,
    VectorDelete,
    VectorMerge,
    Result,
    VectorResult,
    ProjectSet,
    Hash,
    Materialize,
    VectorMaterialize,
    Limit,
    VectorLimit,
    LockRows,
    PartitionIterator,
    VectorPartitionIterator,
    RowAdapter,
    VectorAdapter,
    StartWithOp,
    Streaming(super::StreamingType),
    VectorStreaming(super::StreamingType),
    DataNodeScan,
    DummyHashAggregate,
    RemoteSubplanScan,
    ModifyTable,
    Gather,
    GatherMerge,
    Unknown(String),
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub enum NodeTypeCategory {
    Scan,
    Join,
    Aggregate,
    Sort,
    Dml,
    SetOp,
    Auxiliary,
    Streaming,
    Other,
}

impl NodeType {
    pub fn category(&self) -> NodeTypeCategory {
        match self {
            Self::SeqScan
            | Self::PartitionedSeqScan
            | Self::SampleScan
            | Self::IndexScan
            | Self::PartitionedIndexScan
            | Self::IndexOnlyScan
            | Self::PartitionedIndexOnlyScan
            | Self::BitmapIndexScan
            | Self::PartitionedBitmapIndexScan
            | Self::BitmapHeapScan
            | Self::PartitionedBitmapHeapScan
            | Self::TidScan
            | Self::TidRangeScan
            | Self::SubqueryScan
            | Self::FunctionScan
            | Self::ValuesScan
            | Self::CteScan
            | Self::WorkTableScan
            | Self::ForeignScan
            | Self::PartitionedForeignScan
            | Self::CStoreScan
            | Self::PartitionedCStoreScan
            | Self::TsStoreScan
            | Self::AnnIndexScan
            | Self::CStoreIndexScan
            | Self::CStoreIndexCtidScan
            | Self::CStoreIndexHeapScan
            | Self::ImCStoreScan
            | Self::VectorSubqueryScan
            | Self::VectorForeignScan
            | Self::DataNodeScan => NodeTypeCategory::Scan,

            Self::NestedLoop
            | Self::HashJoin
            | Self::MergeJoin
            | Self::VectorNestLoop
            | Self::VectorHashJoin
            | Self::VectorSonicHashJoin
            | Self::VectorMergeJoin
            | Self::VectorAsofJoin => NodeTypeCategory::Join,

            Self::Aggregate
            | Self::GroupAggregate
            | Self::HashAggregate
            | Self::DummyHashAggregate
            | Self::VectorAggregate
            | Self::VectorHashAggregate
            | Self::VectorSonicHashAggregate
            | Self::VectorSortAggregate
            | Self::Group
            | Self::VectorGroup => NodeTypeCategory::Aggregate,

            Self::Sort | Self::GroupSort | Self::VectorSort => NodeTypeCategory::Sort,

            Self::Insert
            | Self::Update
            | Self::Delete
            | Self::Merge
            | Self::Replace
            | Self::VectorInsert
            | Self::VectorUpdate
            | Self::VectorDelete
            | Self::VectorMerge => NodeTypeCategory::Dml,

            Self::SetOp
            | Self::HashSetOp
            | Self::VectorSetOp
            | Self::VectorHashSetOp
            | Self::Append
            | Self::VectorAppend
            | Self::MergeAppend
            | Self::RecursiveUnion
            | Self::BitmapAnd
            | Self::BitmapOr
            | Self::CStoreIndexAnd
            | Self::CStoreIndexOr => NodeTypeCategory::SetOp,

            Self::Streaming(_) | Self::VectorStreaming(_) => NodeTypeCategory::Streaming,

            _ => NodeTypeCategory::Other,
        }
    }
}

impl fmt::Display for NodeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Streaming(st) => write!(f, "Streaming(type: {})", st),
            Self::VectorStreaming(st) => write!(f, "Vector Streaming(type: {})", st),
            Self::Unknown(name) => write!(f, "{}", name),
            other => write!(f, "{:?}", other),
        }
    }
}

impl FromStr for NodeType {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(parse_node_type(s))
    }
}

fn parse_node_type(s: &str) -> NodeType {
    let s = s.trim();

    if let Some(rest) = s.strip_prefix("Streaming") {
        if let Some(stype) = extract_streaming_type(rest) {
            return NodeType::Streaming(stype);
        }
    }
    if let Some(rest) = s.strip_prefix("Vector Streaming") {
        if let Some(stype) = extract_streaming_type(rest) {
            return NodeType::VectorStreaming(stype);
        }
    }

    match s {
        "Seq Scan" => NodeType::SeqScan,
        "Partitioned Seq Scan" => NodeType::PartitionedSeqScan,
        "Sample Scan" => NodeType::SampleScan,
        "Index Scan" => NodeType::IndexScan,
        "Partitioned Index Scan" => NodeType::PartitionedIndexScan,
        "Index Only Scan" => NodeType::IndexOnlyScan,
        "Partitioned Index Only Scan" => NodeType::PartitionedIndexOnlyScan,
        "Bitmap Index Scan" => NodeType::BitmapIndexScan,
        "Partitioned Bitmap Index Scan" => NodeType::PartitionedBitmapIndexScan,
        "Bitmap Heap Scan" => NodeType::BitmapHeapScan,
        "Partitioned Bitmap Heap Scan" => NodeType::PartitionedBitmapHeapScan,
        "Tid Scan" => NodeType::TidScan,
        "Tid Range Scan" => NodeType::TidRangeScan,
        "Subquery Scan" => NodeType::SubqueryScan,
        "Function Scan" => NodeType::FunctionScan,
        "Values Scan" => NodeType::ValuesScan,
        "CTE Scan" => NodeType::CteScan,
        "WorkTable Scan" => NodeType::WorkTableScan,
        "Foreign Scan" => NodeType::ForeignScan,
        "Partitioned Foreign Scan" => NodeType::PartitionedForeignScan,
        "CStore Scan" => NodeType::CStoreScan,
        "Partitioned CStore Scan" => NodeType::PartitionedCStoreScan,
        "TsStore Scan" => NodeType::TsStoreScan,
        "ANN Index Scan" => NodeType::AnnIndexScan,
        "CStore Index Scan" => NodeType::CStoreIndexScan,
        "CStore Index Ctid Scan" => NodeType::CStoreIndexCtidScan,
        "CStore Index Heap Scan" => NodeType::CStoreIndexHeapScan,
        "ImCStore Scan" => NodeType::ImCStoreScan,
        "Vector Subquery Scan" => NodeType::VectorSubqueryScan,
        "Vector Foreign Scan" => NodeType::VectorForeignScan,
        "Nested Loop" => NodeType::NestedLoop,
        "Hash Join" => NodeType::HashJoin,
        "Merge Join" => NodeType::MergeJoin,
        "Vector Nest Loop" => NodeType::VectorNestLoop,
        "Vector Hash Join" => NodeType::VectorHashJoin,
        "Vector Sonic Hash Join" => NodeType::VectorSonicHashJoin,
        "Vector Merge Join" => NodeType::VectorMergeJoin,
        "Vector Asof Join" => NodeType::VectorAsofJoin,
        "Aggregate" => NodeType::Aggregate,
        "Group Aggregate" => NodeType::GroupAggregate,
        "Hash Aggregate" | "HashAggregate" => NodeType::HashAggregate,
        "Dummy HashAggregate" => NodeType::DummyHashAggregate,
        "Vector Aggregate" => NodeType::VectorAggregate,
        "Vector Hash Aggregate" => NodeType::VectorHashAggregate,
        "Vector Sonic Hash Aggregate" => NodeType::VectorSonicHashAggregate,
        "Vector Sort Aggregate" => NodeType::VectorSortAggregate,
        "Sort" => NodeType::Sort,
        "Group Sort" => NodeType::GroupSort,
        "Vector Sort" => NodeType::VectorSort,
        "Group" => NodeType::Group,
        "Vector Group" => NodeType::VectorGroup,
        "WindowAgg" => NodeType::WindowAgg,
        "Vector WindowAgg" => NodeType::VectorWindowAgg,
        "Unique" => NodeType::Unique,
        "Vector Unique" => NodeType::VectorUnique,
        "SetOp" => NodeType::SetOp,
        "Hash SetOp" | "HashSetOp" => NodeType::HashSetOp,
        "Vector SetOp" => NodeType::VectorSetOp,
        "Vector Hash SetOp" | "Vector HashSetOp" => NodeType::VectorHashSetOp,
        "Append" => NodeType::Append,
        "Vector Append" => NodeType::VectorAppend,
        "Merge Append" => NodeType::MergeAppend,
        "Recursive Union" => NodeType::RecursiveUnion,
        "BitmapAnd" => NodeType::BitmapAnd,
        "BitmapOr" => NodeType::BitmapOr,
        "CStore Index And" => NodeType::CStoreIndexAnd,
        "CStore Index Or" => NodeType::CStoreIndexOr,
        "Insert" => NodeType::Insert,
        "Update" => NodeType::Update,
        "Delete" => NodeType::Delete,
        "Merge" => NodeType::Merge,
        "Replace" => NodeType::Replace,
        "Vector Insert" => NodeType::VectorInsert,
        "Vector Update" => NodeType::VectorUpdate,
        "Vector Delete" => NodeType::VectorDelete,
        "Vector Merge" => NodeType::VectorMerge,
        "Result" => NodeType::Result,
        "Vector Result" => NodeType::VectorResult,
        "ProjectSet" => NodeType::ProjectSet,
        "Hash" => NodeType::Hash,
        "Materialize" => NodeType::Materialize,
        "Vector Materialize" => NodeType::VectorMaterialize,
        "Limit" => NodeType::Limit,
        "Vector Limit" => NodeType::VectorLimit,
        "LockRows" => NodeType::LockRows,
        "Partition Iterator" => NodeType::PartitionIterator,
        "Vector Partition Iterator" => NodeType::VectorPartitionIterator,
        "Row Adapter" => NodeType::RowAdapter,
        "Vector Adapter" => NodeType::VectorAdapter,
        "StartWith Op" => NodeType::StartWithOp,
        "Data Node Scan" => NodeType::DataNodeScan,
        "Remote Subplan Scan" => NodeType::RemoteSubplanScan,
        "ModifyTable" => NodeType::ModifyTable,
        "Gather" => NodeType::Gather,
        "Gather Merge" => NodeType::GatherMerge,
        _ => NodeType::Unknown(s.to_string()),
    }
}

fn extract_streaming_type(s: &str) -> Option<super::StreamingType> {
    let s = s.trim();
    if !s.starts_with('(') {
        return None;
    }
    let end = s.find(')')?;
    let inner = &s[1..end];
    let type_str = inner
        .strip_prefix("type: ")
        .or_else(|| inner.strip_prefix("type:"))?;
    let cleaned = type_str
        .split("DOP:")
        .next()
        .unwrap_or(type_str)
        .split("ng:")
        .next()
        .unwrap_or(type_str)
        .trim();
    cleaned.parse().ok()
}
