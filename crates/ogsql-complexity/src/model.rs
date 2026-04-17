use serde::Serialize;

/// Weight profile for complexity scoring.
///
/// Different database systems warrant different weight profiles because
/// query patterns and cost characteristics differ across platforms.
#[derive(Debug, Clone, Serialize)]
pub struct WeightProfile {
    pub name: String,
    pub table: f64,
    pub join: f64,
    pub where_condition: f64,
    pub subquery: f64,
    pub aggregate_function: f64,
    pub case_expression: f64,
    pub set_operation: f64,
    pub group_by: f64,
    pub order_by: f64,
    pub window_function: f64,
    pub cte: f64,
}

impl WeightProfile {
    /// GaussDB/OpenGauss weight profile — the default.
    pub fn gauss() -> Self {
        Self {
            name: "gauss".into(),
            table: 1.0,
            join: 2.0,
            where_condition: 1.0,
            subquery: 3.0,
            aggregate_function: 1.5,
            case_expression: 1.5,
            set_operation: 2.0,
            group_by: 1.5,
            order_by: 1.0,
            window_function: 2.5,
            cte: 1.5,
        }
    }

    /// Oracle-style weight profile.
    pub fn oracle() -> Self {
        Self {
            name: "oracle".into(),
            table: 1.0,
            join: 2.0,
            where_condition: 1.5,
            subquery: 3.0,
            aggregate_function: 1.0,
            case_expression: 1.0,
            set_operation: 2.0,
            group_by: 1.5,
            order_by: 1.0,
            window_function: 2.5,
            cte: 1.5,
        }
    }

    /// Hive-style weight profile.
    pub fn hive() -> Self {
        Self {
            name: "hive".into(),
            table: 1.0,
            join: 2.0,
            where_condition: 1.5,
            subquery: 3.0,
            aggregate_function: 1.0,
            case_expression: 1.5,
            set_operation: 2.0,
            group_by: 1.5,
            order_by: 1.0,
            window_function: 2.5,
            cte: 1.5,
        }
    }
}

impl Default for WeightProfile {
    fn default() -> Self {
        Self::gauss()
    }
}

/// Statement type multiplier applied to the raw complexity score.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatementTypeMultiplier {
    Select = 0,
    Insert = 1,
    Update = 2,
    Delete = 3,
    Merge = 4,
    Other = 5,
}

impl StatementTypeMultiplier {
    /// Returns the numeric multiplier for this statement type.
    pub fn multiplier(self) -> f64 {
        match self {
            Self::Select => 1.0,
            Self::Insert => 1.0,
            Self::Update => 1.2,
            Self::Delete => 1.1,
            Self::Merge => 1.5,
            Self::Other => 1.0,
        }
    }
}

/// Raw complexity metrics extracted from a SQL statement.
///
/// These are the structural counts that feed into the weighted scoring.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ComplexityMetrics {
    pub table_count: usize,
    pub join_count: usize,
    pub where_condition_count: usize,
    pub subquery_count: usize,
    pub aggregate_function_count: usize,
    pub case_expression_count: usize,
    pub set_operation_count: usize,
    pub cte_count: usize,
    pub window_function_count: usize,
    pub has_group_by: bool,
    pub has_order_by: bool,
    pub has_distinct: bool,
    pub subquery_depth: usize,
}

/// Per-component weighted score breakdown.
#[derive(Debug, Clone, Default, Serialize)]
pub struct WeightedBreakdown {
    pub tables: f64,
    pub joins: f64,
    pub where_conditions: f64,
    pub subqueries: f64,
    pub aggregate_functions: f64,
    pub case_expressions: f64,
    pub set_operations: f64,
    pub group_by: f64,
    pub order_by: f64,
    pub window_functions: f64,
    pub ctes: f64,
}

/// Complexity level derived from the numeric score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComplexityLevel {
    Trivial,
    Simple,
    Moderate,
    Complex,
    VeryComplex,
}

impl ComplexityLevel {
    /// Derive complexity level from a numeric score.
    pub fn from_score(score: f64) -> Self {
        if score < 5.0 {
            Self::Trivial
        } else if score < 15.0 {
            Self::Simple
        } else if score < 30.0 {
            Self::Moderate
        } else if score < 50.0 {
            Self::Complex
        } else {
            Self::VeryComplex
        }
    }

    /// Human-readable label for the complexity level.
    pub fn label(self) -> &'static str {
        match self {
            Self::Trivial => "Trivial",
            Self::Simple => "Simple",
            Self::Moderate => "Moderate",
            Self::Complex => "Complex",
            Self::VeryComplex => "Very Complex",
        }
    }
}

/// Complexity analysis for a single SQL statement.
#[derive(Debug, Clone, Serialize)]
pub struct StatementComplexity {
    pub sql_text: String,
    pub statement_type: StatementTypeMultiplier,
    pub metrics: ComplexityMetrics,
    pub weighted_breakdown: WeightedBreakdown,
    pub raw_score: f64,
    pub adjusted_score: f64,
    pub level: ComplexityLevel,
}

/// Full complexity report covering one or more SQL statements.
#[derive(Debug, Clone, Serialize)]
pub struct ComplexityReport {
    pub statements: Vec<StatementComplexity>,
    pub overall_score: f64,
    pub overall_level: ComplexityLevel,
    pub profile: String,
}
