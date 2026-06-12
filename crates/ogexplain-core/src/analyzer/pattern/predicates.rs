//! Unified field accessor for anti-pattern predicates.
//!
//! Provides a consistent way to extract typed fields from [`PlanNode`](crate::model::PlanNode)
//! instances without exposing internal model structure to pattern definitions.
//! Used by anti-pattern `try_match` implementations for predicate evaluation.

use crate::model::{NodeType, PlanNode, StreamingType};

/// A path identifying a field on a [`PlanNode`](crate::model::PlanNode) that can be queried.
///
/// Variants may be numeric (`field_f64`), string (`field_str`), or compound
/// (`Property` by label, `StreamingType` by node type inspection).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum FieldPath {
    /// Actual rows (`node.actual.rows`)
    ActualRows,
    /// Actual loop count (`node.actual.loops`)
    ActualLoops,
    /// Actual total time in ms (`node.actual.total_time_ms`)
    ActualTimeMs,
    /// Estimated plan rows (`node.estimated.plan_rows`)
    EstimatedRows,
    /// Estimated total cost (`node.estimated.total_cost`)
    EstimatedTotalCost,
    /// Relation (table) name (`node.relation`)
    Relation,
    /// Peak memory usage in kB (`node.structured_props.peak_memory_kb`)
    PeakMemoryKb,
    /// Rows removed by filter (`node.structured_props.rows_removed_by_filter`)
    RowsRemovedByFilter,
    /// Number of hash batches (`node.structured_props.hash_batches`)
    HashBatches,
    /// Sort method string (`node.structured_props.sort_method`)
    SortMethod,
    /// Selected partition info (`node.structured_props.selected_partitions`)
    SelectedPartitions,
    /// Arbitrary property by label, looked up via [`get_property_value`]
    Property(String),
    /// Streaming sub-type (only valid for `Streaming` / `VectorStreaming` nodes)
    StreamingType,
    /// Number of child nodes
    ChildCount,
}

/// Stateless accessor for extracting fields from plan nodes.
///
/// Methods are organized by return type:
/// - [`field_f64`](FieldAccessor::field_f64) for numeric fields
/// - [`field_str`](FieldAccessor::field_str) for string fields
/// - [`streaming_type`](FieldAccessor::streaming_type) for the streaming sub-type
pub struct FieldAccessor;

impl FieldAccessor {
    /// Extract a numeric field value from `node` at the given `path`.
    ///
    /// Returns `None` if the field is absent or the path does not correspond
    /// to a numeric field.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let rows = FieldAccessor::field_f64(node, &FieldPath::ActualRows);
    /// ```
    pub fn field_f64(node: &PlanNode, path: &FieldPath) -> Option<f64> {
        match path {
            FieldPath::ActualRows => node.actual.as_ref().map(|a| a.rows),
            FieldPath::ActualLoops => node.actual.as_ref().map(|a| a.loops),
            FieldPath::ActualTimeMs => node.actual.as_ref().map(|a| a.total_time_ms),
            FieldPath::EstimatedRows => node.estimated.as_ref().map(|e| e.plan_rows),
            FieldPath::EstimatedTotalCost => node.estimated.as_ref().map(|e| e.total_cost),
            FieldPath::PeakMemoryKb => node
                .structured_props
                .as_ref()
                .and_then(|p| p.peak_memory_kb),
            FieldPath::RowsRemovedByFilter => node
                .structured_props
                .as_ref()
                .and_then(|p| p.rows_removed_by_filter),
            FieldPath::HashBatches => node
                .structured_props
                .as_ref()
                .and_then(|p| p.hash_batches.map(|b| b as f64)),
            FieldPath::ChildCount => {
                let count = node.children.len();
                Some(count as f64)
            }
            _ => None,
        }
    }

    /// Extract a string field value from `node` at the given `path`.
    ///
    /// Returns `None` if the field is absent or the path does not correspond
    /// to a string field.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let rel = FieldAccessor::field_str(node, &FieldPath::Relation);
    /// ```
    pub fn field_str<'a>(node: &'a PlanNode, path: &FieldPath) -> Option<&'a str> {
        match path {
            FieldPath::Relation => node.relation.as_deref(),
            FieldPath::SortMethod => node
                .structured_props
                .as_ref()
                .and_then(|p| p.sort_method.as_deref()),
            FieldPath::SelectedPartitions => node
                .structured_props
                .as_ref()
                .and_then(|p| p.selected_partitions.as_deref()),
            FieldPath::Property(label) => {
                crate::analyzer::rules::utils::get_property_value(node, label)
            }
            _ => None,
        }
    }

    /// Extract the [`StreamingType`] from a node, if the node is a streaming node.
    ///
    /// Returns `None` for non-streaming node types.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// if let Some(st) = FieldAccessor::streaming_type(node) {
    ///     // node is a streaming node
    /// }
    /// ```
    pub fn streaming_type(node: &PlanNode) -> Option<&StreamingType> {
        match &node.node_type {
            NodeType::Streaming(st) | NodeType::VectorStreaming(st) => Some(st),
            _ => None,
        }
    }
}
