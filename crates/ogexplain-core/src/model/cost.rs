use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EstimatedCost {
    pub startup_cost: f64,
    pub total_cost: f64,
    pub plan_rows: f64,
    pub plan_width: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pred_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pred_rows: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distinct: Option<(f64, f64)>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ActualStats {
    pub startup_time_ms: f64,
    pub total_time_ms: f64,
    pub rows: f64,
    pub loops: f64,
    pub executed: bool,
}
