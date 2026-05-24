//! Orchestrator runtime policy — see SPEC.md Module 1.

use crate::domain::ShimmerConfig;

/// Runtime policy for the shimmer orchestrator (distinct from visual [`ShimmerConfig`]).
#[derive(Debug, Clone, PartialEq)]
pub struct OrchestratorConfig {
    /// Visual parameters forwarded to the overlay on each play.
    pub shimmer: ShimmerConfig,
    /// When false, automatic Battery→AC triggers are suppressed.
    /// Manual/CLI triggers still work unless `dry_run` is true.
    pub auto_enabled: bool,
    /// When true, orchestrator logs intent but does not call [`OverlayRenderer`](crate::ports::OverlayRenderer).
    pub dry_run: bool,
    /// Behavior when a new play arrives while one is in flight.
    pub overlap_policy: OverlapPolicy,
}

/// If a shimmer is already playing when a new request arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlapPolicy {
    /// Ignore the new request while a shimmer is playing. v1 default.
    #[default]
    Skip,
    /// Cancel the in-flight shimmer and replay.
    Restart,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            shimmer: ShimmerConfig::default(),
            auto_enabled: true,
            dry_run: false,
            overlap_policy: OverlapPolicy::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::MonitorTarget;

    #[test]
    fn default_matches_spec() {
        let config = OrchestratorConfig::default();
        assert!(config.auto_enabled);
        assert!(!config.dry_run);
        assert_eq!(config.overlap_policy, OverlapPolicy::Skip);
        assert_eq!(config.shimmer.duration_ms, 2_000);
        assert!((config.shimmer.opacity - 0.35).abs() < f32::EPSILON);
        assert!((config.shimmer.speed - 1.0).abs() < f32::EPSILON);
        assert_eq!(config.shimmer.monitor, MonitorTarget::Primary);
    }
}
