use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum SuggestionCategory {
    IndexOptimization,
    StatisticsUpdate,
    QueryRewrite,
    ConfigurationTuning,
    DistributionOptimization,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Suggestion {
    pub related_rules: Vec<String>,
    pub category: SuggestionCategory,
    pub message: String,
    pub confidence: f64,
}
