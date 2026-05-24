//! Orchestrator trigger policy — see SPEC.md Module 4.

use crate::domain::{OrchestratorConfig, PowerEvent};

/// Returns true when an automatic shimmer should play for `event` under `config`.
#[must_use]
pub fn should_auto_play(event: &PowerEvent, config: &OrchestratorConfig) -> bool {
    event.is_battery_to_ac() && config.auto_enabled && !config.dry_run
}

#[cfg(test)]
mod tests {
    use crate::domain::{OrchestratorConfig, PowerEvent, PowerSource};

    use super::*;

    fn battery_to_ac() -> PowerEvent {
        PowerEvent::Transition {
            from: PowerSource::Battery,
            to: PowerSource::Ac,
        }
    }

    #[test]
    fn battery_to_ac_with_defaults_returns_true() {
        assert!(should_auto_play(
            &battery_to_ac(),
            &OrchestratorConfig::default()
        ));
    }

    #[test]
    fn initial_state_never_auto_plays() {
        let config = OrchestratorConfig::default();
        let event = PowerEvent::InitialState {
            source: PowerSource::Battery,
        };
        assert!(!should_auto_play(&event, &config));
    }

    #[test]
    fn auto_disabled_suppresses_play() {
        let config = OrchestratorConfig {
            auto_enabled: false,
            ..OrchestratorConfig::default()
        };
        assert!(!should_auto_play(&battery_to_ac(), &config));
    }

    #[test]
    fn dry_run_suppresses_play() {
        let config = OrchestratorConfig {
            dry_run: true,
            ..OrchestratorConfig::default()
        };
        assert!(!should_auto_play(&battery_to_ac(), &config));
    }

    #[test]
    fn ac_to_battery_never_auto_plays() {
        let event = PowerEvent::Transition {
            from: PowerSource::Ac,
            to: PowerSource::Battery,
        };
        assert!(!should_auto_play(&event, &OrchestratorConfig::default()));
    }
}
