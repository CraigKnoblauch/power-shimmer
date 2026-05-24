//! Integration smoke tests for core test doubles (orchestrator tests land in Phase 1.6).

use std::sync::Arc;
use std::time::Duration;

use power_shimmer_core::{
    MockOverlayRenderer, MockPowerEventListener, OrchestratorConfig, OverlayRenderer, PowerEvent,
    PowerEventListener, PowerSource, ShimmerConfig, ShimmerRequest, ShimmerTrigger,
};

#[test]
fn orchestrator_config_default_from_public_api() {
    let config = OrchestratorConfig::default();
    assert!(config.auto_enabled);
    assert!(!config.dry_run);
}

#[test]
fn mock_power_listener_is_reachable_from_integration_tests() {
    let listener = MockPowerEventListener::new(vec![PowerEvent::InitialState {
        source: PowerSource::Ac,
    }]);
    let stream = listener.subscribe().expect("subscribe");
    let event = stream.recv().unwrap().unwrap();
    assert_eq!(
        event,
        PowerEvent::InitialState {
            source: PowerSource::Ac
        }
    );
}

#[test]
fn mock_overlay_renderer_is_reachable_from_integration_tests() {
    let overlay = Arc::new(MockOverlayRenderer::with_delay(Duration::from_millis(30)));
    let request = ShimmerRequest {
        config: ShimmerConfig::default(),
        trigger: ShimmerTrigger::Manual,
    };

    let result = std::thread::spawn({
        let overlay = Arc::clone(&overlay);
        move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .expect("runtime");
            runtime.block_on(overlay.play(request))
        }
    })
    .join()
    .expect("join");

    result.expect("play");
    assert_eq!(overlay.play_calls.lock().unwrap().len(), 1);
}
