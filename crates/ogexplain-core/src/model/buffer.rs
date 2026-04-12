use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct BufferStats {
    #[serde(skip_serializing_if = "is_zero")]
    pub shared_hit: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub shared_read: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub shared_dirtied: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub shared_written: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub local_hit: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub local_read: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub local_dirtied: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub local_written: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub temp_read: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub temp_written: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub io_read_time_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub io_write_time_ms: Option<f64>,
}

fn is_zero(v: &i64) -> bool {
    *v == 0
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NodeProperty {
    pub label: String,
    pub value: String,
}
