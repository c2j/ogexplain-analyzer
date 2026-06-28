//! Convergence detection for closed-loop optimization.
//!
//! Compares two [`MetricsSnapshot`]s across iterations and decides whether
//! to continue the rewrite→verify→re-evaluate loop or stop.
//!
//! See `.sisyphus/plans/2026-06-28-closed-loop-pilot.md` Phase 2 and
//! Heptadecagon `docs/closed-loop-optimization-design.md` §7 for context.

use serde::Serialize;

/// Snapshot of plan metrics relevant to convergence detection.
///
/// Subset of [`crate::summary::SummaryRow`] that:
/// (a) is sufficient for convergence decisions,
/// (b) derives `PartialEq` safely (no f64 NaN risk on real plan data).
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct MetricsSnapshot {
    pub total_cost: Option<f64>,
    pub total_time_ms: Option<f64>,
    pub critical_count: usize,
    pub warning_count: usize,
    pub spill_kb: Option<f64>,
    pub peak_memory_kb: Option<f64>,
    pub worst_est_ratio: Option<f64>,
}

impl MetricsSnapshot {
    /// Extract convergence-relevant fields from a full SummaryRow.
    pub fn from_summary(s: &crate::summary::SummaryRow) -> Self {
        Self {
            total_cost: Some(s.total_cost),
            total_time_ms: Some(s.total_time_ms),
            critical_count: s.critical_count,
            warning_count: s.warning_count,
            spill_kb: s.spill_kb,
            peak_memory_kb: s.peak_memory_kb,
            worst_est_ratio: s.worst_est_ratio,
        }
    }
}

/// Loop configuration. All thresholds are inclusive (>=).
#[derive(Debug, Clone, Serialize)]
pub struct LoopConfig {
    /// Maximum iterations before forced stop. Default 10.
    pub max_iterations: usize,
    /// Minimum cost improvement fraction to count as progress. Default 0.05 (5%).
    pub min_improvement_pct: f64,
    /// Consecutive non-improving iterations before plateau stop. Default 3.
    pub max_plateau_count: usize,
    /// Cost increase fraction that triggers regression rollback. Default 0.10 (10%).
    pub regression_threshold_pct: f64,
    /// Whether to require QED/VeriEQL equivalence proof before accepting a rewrite.
    /// Week 1 pilot may set this to false (rely on metamorphosis Conditional safety).
    pub require_equivalence_proof: bool,
    /// Whether to auto-run ANALYZE when stale statistics detected (Phase 0).
    /// Week 1 pilot sets this to false (manual ANALYZE required).
    pub auto_run_analyze: bool,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            min_improvement_pct: 0.05,
            max_plateau_count: 3,
            regression_threshold_pct: 0.10,
            require_equivalence_proof: true,
            auto_run_analyze: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum LoopDecision {
    Continue,
    Stop(StopReason),
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum StopReason {
    /// `critical_count == 0` — all critical findings resolved.
    Success,
    /// Cost improvement < `min_improvement_pct` for `max_plateau_count` iterations.
    Plateau,
    /// Cost increased > `regression_threshold_pct` — rollback and stop.
    Regression,
    /// Reached `max_iterations`.
    MaxIterations,
    /// Remaining findings have no rewrite mapping (DDL/Config/Log only).
    NoRewritableFindings,
    /// Rewritten SQL equals previously-seen SQL (oscillation or fixed-point).
    FixedPoint,
}

/// Decide whether to continue the optimization loop.
///
/// Evaluation order (first match wins):
/// 1. FixedPoint — `sql_unchanged == true`
/// 2. Success — `curr.critical_count == 0`
/// 3. Regression — cost increased beyond threshold
/// 4. MaxIterations — `iteration >= config.max_iterations`
/// 5. Plateau — non-improving for `max_plateau_count`
/// 6. NoRewritableFindings — no rewritable findings remain
/// 7. otherwise → Continue
pub fn should_continue(
    prev: &MetricsSnapshot,
    curr: &MetricsSnapshot,
    config: &LoopConfig,
    iteration: usize,
    plateau_count: usize,
    has_rewritable: bool,
    sql_unchanged: bool,
) -> LoopDecision {
    if sql_unchanged {
        return LoopDecision::Stop(StopReason::FixedPoint);
    }
    if curr.critical_count == 0 {
        return LoopDecision::Stop(StopReason::Success);
    }
    if let (Some(p), Some(c)) = (prev.total_cost, curr.total_cost) {
        if c > p * (1.0 + config.regression_threshold_pct) {
            return LoopDecision::Stop(StopReason::Regression);
        }
    }
    if iteration >= config.max_iterations {
        return LoopDecision::Stop(StopReason::MaxIterations);
    }
    if let (Some(p), Some(c)) = (prev.total_cost, curr.total_cost) {
        if p > 0.0 {
            let improvement = (p - c) / p;
            if improvement < config.min_improvement_pct
                && plateau_count >= config.max_plateau_count
            {
                return LoopDecision::Stop(StopReason::Plateau);
            }
        }
    }
    if !has_rewritable {
        return LoopDecision::Stop(StopReason::NoRewritableFindings);
    }
    LoopDecision::Continue
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(cost: f64, critical: usize) -> MetricsSnapshot {
        MetricsSnapshot {
            total_cost: Some(cost),
            critical_count: critical,
            ..Default::default()
        }
    }

    #[test]
    fn metrics_snapshot_partial_eq_works() {
        let a = snap(100.0, 2);
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn stop_when_critical_zero() {
        let prev = snap(100.0, 2);
        let curr = snap(80.0, 0);
        let decision = should_continue(&prev, &curr, &LoopConfig::default(), 1, 0, true, false);
        assert!(matches!(decision, LoopDecision::Stop(StopReason::Success)));
    }

    #[test]
    fn stop_on_regression() {
        let prev = snap(100.0, 2);
        let curr = snap(120.0, 2);
        let decision = should_continue(&prev, &curr, &LoopConfig::default(), 1, 0, true, false);
        assert!(matches!(decision, LoopDecision::Stop(StopReason::Regression)));
    }

    #[test]
    fn stop_on_max_iterations() {
        let prev = snap(100.0, 2);
        let curr = snap(95.0, 2);
        let cfg = LoopConfig {
            max_iterations: 5,
            ..Default::default()
        };
        let decision = should_continue(&prev, &curr, &cfg, 5, 0, true, false);
        assert!(matches!(
            decision,
            LoopDecision::Stop(StopReason::MaxIterations)
        ));
    }

    #[test]
    fn stop_on_plateau() {
        let prev = snap(100.0, 2);
        let curr = snap(99.0, 2);
        let cfg = LoopConfig {
            max_plateau_count: 3,
            ..Default::default()
        };
        let decision = should_continue(&prev, &curr, &cfg, 1, 3, true, false);
        assert!(matches!(decision, LoopDecision::Stop(StopReason::Plateau)));
    }

    #[test]
    fn stop_on_no_rewritable() {
        let prev = snap(100.0, 2);
        let curr = snap(95.0, 2);
        let decision = should_continue(&prev, &curr, &LoopConfig::default(), 1, 0, false, false);
        assert!(matches!(
            decision,
            LoopDecision::Stop(StopReason::NoRewritableFindings)
        ));
    }

    #[test]
    fn stop_on_fixed_point() {
        let prev = snap(100.0, 2);
        let curr = snap(95.0, 2);
        let decision = should_continue(&prev, &curr, &LoopConfig::default(), 1, 0, true, true);
        assert!(matches!(decision, LoopDecision::Stop(StopReason::FixedPoint)));
    }

    #[test]
    fn continue_on_progress() {
        let prev = snap(100.0, 2);
        let curr = snap(80.0, 1);
        let decision = should_continue(&prev, &curr, &LoopConfig::default(), 1, 0, true, false);
        assert!(matches!(decision, LoopDecision::Continue));
    }
}
