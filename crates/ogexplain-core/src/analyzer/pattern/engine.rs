//! Anti-pattern matching engine.
//!
//! Provides the [`AntiPatternDef`] trait for defining individual anti-patterns
//! and the [`PatternEngine`] struct that orchestrates DFS traversal of the
//! plan tree, running all registered anti-patterns at each node.

use super::types::MatchResult;
use crate::analyzer::report::{DiagnosticCategory, Severity};
use crate::model::{ExplainPlan, PlanNode};

/// Trait for defining a single anti-pattern's matching logic.
///
/// Each anti-pattern provides metadata (ID, name, severity, category), one or
/// more related classic rule IDs for deduplication, template strings for
/// diagnostic messages, and most importantly the [`try_match`](AntiPatternDef::try_match)
/// method that attempts to match at a given subtree root.
///
/// Implementations must be [`Send`] + [`Sync`] so that the engine can run
/// patterns in parallel if needed.
pub trait AntiPatternDef: Send + Sync {
    /// Unique identifier for this anti-pattern (e.g., `"ANTI-005"`).
    fn id(&self) -> &str;

    /// Human-readable name for this anti-pattern.
    fn name(&self) -> &str;

    /// Severity level of findings produced by this anti-pattern.
    fn severity(&self) -> Severity;

    /// Category under which findings from this anti-pattern are grouped.
    fn category(&self) -> DiagnosticCategory;

    /// IDs of classic diagnostic rules that overlap with this anti-pattern.
    ///
    /// Used for future deduplication — when an anti-pattern and a classic rule
    /// both fire on the same node, the finding with the lower severity may be
    /// suppressed.
    fn related_classic_rules(&self) -> Vec<String>;

    /// Attempt to match this anti-pattern at the given subtree root.
    ///
    /// `root` is the current node being visited. `ancestors` contains the
    /// chain of parent nodes from the plan root down to (but not including)
    /// `root`. It is empty when `root` is the plan root.
    ///
    /// Returns `Some(MatchResult)` on success, `None` if the pattern does not
    /// match at this node.
    fn try_match<'a>(
        &self,
        root: &'a PlanNode,
        ancestors: &[&'a PlanNode],
    ) -> Option<MatchResult<'a>>;

    /// Detail message template supporting `{capture.property}` placeholders.
    ///
    /// Rendered by [`render_template`](super::templates::render_template) during
    /// finding generation.
    fn detail_template(&self) -> String;

    /// Suggestion message template supporting `{capture.property}` placeholders.
    ///
    /// Rendered by [`render_template`](super::templates::render_template) during
    /// finding generation.
    fn suggestion_template(&self) -> String;
}

/// DFS-based engine that walks a plan tree and runs all registered anti-patterns.
///
/// For each node, every registered [`AntiPatternDef`] is given a chance to match.
/// Matching always starts at the current node — patterns are responsible for
/// descending into children as needed (structural matching).
pub struct PatternEngine {
    patterns: Vec<Box<dyn AntiPatternDef>>,
}

impl PatternEngine {
    /// Create a new engine with the given list of anti-pattern definitions.
    pub fn new(patterns: Vec<Box<dyn AntiPatternDef>>) -> Self {
        Self { patterns }
    }

    /// Run all registered anti-patterns against the entire plan tree.
    ///
    /// Returns every successful match across all nodes.
    pub fn match_plan<'a>(&self, plan: &'a ExplainPlan) -> Vec<MatchResult<'a>> {
        let mut results = Vec::new();
        self.walk(&plan.root, &[], &mut results);
        results
    }

    /// Find a registered anti-pattern by its ID.
    ///
    /// Returns the pattern definition if found. Used by
    /// [`AntiPatternRule`](super::AntiPatternRule) during finding rendering.
    pub fn find_pattern(&self, id: &str) -> &dyn AntiPatternDef {
        self.patterns
            .iter()
            .find_map(|p| if p.id() == id { Some(p.as_ref()) } else { None })
            .expect("pattern must be registered")
    }

    /// Recursive DFS walk.
    fn walk<'a>(
        &self,
        node: &'a PlanNode,
        ancestors: &[&'a PlanNode],
        results: &mut Vec<MatchResult<'a>>,
    ) {
        for pattern in &self.patterns {
            if let Some(result) = pattern.try_match(node, ancestors) {
                results.push(result);
            }
        }
        for child in &node.children {
            let mut extended: Vec<&'a PlanNode> = ancestors.to_vec();
            extended.push(node);
            self.walk(child, &extended, results);
        }
    }
}
