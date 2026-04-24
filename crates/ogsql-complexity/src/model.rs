use serde::Serialize;

/// GaussDB-specific weight constants for complexity scoring.
///
/// These integer weights are used by the GaussDB scoring formula
/// and differ from the float-based `WeightProfile` weights.
pub mod gauss_weights {
    pub const TABLE: i64 = 10;
    pub const JOIN: i64 = 15;
    pub const WHERE_CONDITION: i64 = 5;
    pub const SUBQUERY: i64 = 20;
    pub const AGGREGATE_FUNCTION: i64 = 10;
    pub const CASE_EXPRESSION: i64 = 5;
    pub const SET_OPERATION: i64 = 15;
    pub const GROUP_BY: i64 = 5;
    pub const ORDER_BY: i64 = 5;
    pub const LOOP: i64 = 15;
    pub const NESTED_LOOP: i64 = 20;
    pub const CUSTOM_FUNCTION: i64 = 10;
    pub const HIGH_WEIGHT_TABLE: i64 = 20;
    pub const HIGH_WEIGHT_PROCEDURE: i64 = 20;
    pub const NESTED_PROCEDURE: i64 = 15;
    pub const HINT: i64 = 3;
    pub const CURSOR_DECLARATION: i64 = 10;
    pub const CURSOR_OPERATION: i64 = 5;
    pub const NESTED_CURSOR: i64 = 15;
    pub const DYNAMIC_SQL: i64 = 15;
    pub const PARAMETER_BINDING: i64 = 5;
    pub const NESTED_DYNAMIC_SQL: i64 = 25;
    pub const TRANSACTION_CONTROL: i64 = 10;
    pub const AUTONOMOUS_TRANSACTION: i64 = 15;
    pub const NESTED_TRANSACTION: i64 = 20;
    pub const JAVA_PROCEDURE: i64 = 25;
    pub const TYPE_CONVERSION: i64 = 5;
    pub const TABLE_WEIGHT: i64 = 10;
    pub const COLUMN: i64 = 2;
    pub const COMPUTED_COLUMN: i64 = 15;
    pub const CHECK_CONSTRAINT: i64 = 10;
}

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

#[derive(Debug, Clone, Default, Serialize)]
pub struct ComplexityConfig {
    pub custom_functions: Vec<String>,
    pub high_weight_tables: Vec<String>,
    pub high_weight_procedures: Vec<String>,
    pub builtin_functions: Vec<String>,
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

    // SQL statement level additions
    pub hint_count: usize,

    // Stored procedure level metrics
    pub loop_count: usize,
    pub max_loop_nesting_level: usize,
    pub cursor_count: usize,
    pub cursor_operation_count: usize,
    pub max_cursor_nesting_level: usize,
    pub dynamic_sql_count: usize,
    pub param_binding_count: usize,
    pub nested_dynamic_sql_count: usize,
    pub transaction_control_count: usize,
    pub transaction_nesting_level: usize,
    pub uses_autonomous_transactions: bool,
    pub subtransaction_count: usize,
    pub max_subtransaction_nesting_level: usize,

    // Counted from user config matching
    pub custom_function_count: usize,
    pub high_weight_table_count: usize,
    pub nested_procedure_count: usize,
    pub high_weight_procedure_count: usize,

    // Java stored procedure metrics
    pub java_stored_procedure_count: usize,
    pub java_type_conversion_count: usize,

    // CREATE TABLE metrics
    pub column_count: usize,
    pub computed_column_count: usize,
    pub check_constraint_count: usize,

    // Source metrics
    pub line_count: usize,

    // Package metrics
    pub package_procedure_count: usize,
    pub package_variable_count: usize,
    pub package_has_java: bool,
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

#[derive(Debug, Clone, Default, Serialize)]
pub struct PackageMetrics {
    pub total_procedures: usize,
    pub package_level_variables: usize,
    pub contains_java_procedures: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    SqlStatement,
    StoredProcedure,
    AnonymousBlock,
}

/// Broad SQL statement category for statistical grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlCategory {
    Query,
    DML,
    DDL,
    DCL,
    PLBlock,
    Package,
}

impl SqlCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Query => "Query",
            Self::DML => "DML",
            Self::DDL => "DDL",
            Self::DCL => "DCL",
            Self::PLBlock => "PL",
            Self::Package => "Pkg",
        }
    }
}

/// Risk tag derived from complexity metrics for filtering and alerting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComplexityTag {
    HighTableCount,
    DeepNesting,
    DynamicSql,
    CursorHeavy,
    TransactionComplex,
    JavaProcedure,
    LargeJoin,
}

impl ComplexityTag {
    pub fn icon(self) -> &'static str {
        match self {
            Self::HighTableCount => "📊",
            Self::DeepNesting => "🔗",
            Self::DynamicSql => "⚡",
            Self::CursorHeavy => "🎯",
            Self::TransactionComplex => "🔒",
            Self::JavaProcedure => "☕",
            Self::LargeJoin => "🔀",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::HighTableCount => "多表",
            Self::DeepNesting => "深嵌套",
            Self::DynamicSql => "动态SQL",
            Self::CursorHeavy => "重游标",
            Self::TransactionComplex => "复杂事务",
            Self::JavaProcedure => "Java过程",
            Self::LargeJoin => "多表连接",
        }
    }
}

/// Score grouped by dimension for statistical analysis.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ScoreDimensions {
    pub sql_structure: i64,
    pub pl_logic: i64,
    pub advanced_feature: i64,
    pub extension: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GaussDbComplexityReport {
    pub input_kind: InputKind,
    pub sql_category: SqlCategory,
    pub sql_sub_type: String,
    pub overall_score: i64,
    pub level: ComplexityLevel,
    pub dimensions: ScoreDimensions,
    pub tags: Vec<ComplexityTag>,
    pub score_breakdown: GaussDbScoreBreakdown,
    pub sql_statement_scores: Vec<i64>,
    pub pl_metrics: ComplexityMetrics,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct GaussDbScoreBreakdown {
    pub sql_statements_sum: i64,
    pub loop_complexity: i64,
    pub custom_function_complexity: i64,
    pub high_weight_table_complexity: i64,
    pub nested_procedure_complexity: i64,
    pub high_weight_procedure_complexity: i64,
    pub cursor_complexity: i64,
    pub enhanced_complexity: i64,
    pub dynamic_sql_complexity: i64,
    pub param_binding_complexity: i64,
    pub nested_dynamic_sql_complexity: i64,
    pub transaction_complexity: i64,
    pub autonomous_transaction_bonus: i64,
    pub java_procedure_complexity: i64,
    pub java_type_conversion_complexity: i64,
    pub hint_complexity: i64,
    pub package_complexity: i64,
}
