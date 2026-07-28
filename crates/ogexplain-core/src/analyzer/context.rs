use serde::Serialize;

use crate::model::ExplainPlan;

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct GlobalStats {
    pub max_node_time_ms: f64,
    pub max_node_rows: f64,
    pub total_nodes: usize,
    pub max_depth: usize,
}

pub struct PlanContext<'a> {
    pub plan: &'a ExplainPlan,
    pub global_stats: &'a GlobalStats,
}

impl GlobalStats {
    pub fn compute(plan: &ExplainPlan) -> Self {
        let mut stats = Self {
            max_node_time_ms: 0.0,
            max_node_rows: 0.0,
            total_nodes: 0,
            max_depth: 0,
        };
        stats.walk_node(&plan.root, 1);
        stats
    }

    fn walk_node(&mut self, node: &crate::model::PlanNode, depth: usize) {
        self.total_nodes += 1;
        if depth > self.max_depth {
            self.max_depth = depth;
        }
        if let Some(actual) = &node.actual {
            if actual.total_time_ms > self.max_node_time_ms {
                self.max_node_time_ms = actual.total_time_ms;
            }
            if actual.rows > self.max_node_rows {
                self.max_node_rows = actual.rows;
            }
        }
        for child in &node.children {
            self.walk_node(child, depth + 1);
        }
    }
}
