//! Orchestrator integration tests — SPEC.md "Unit Test Requirements" table.

use std::sync::Arc;
use std::time::Duration;

use power_shimmer_core::{
    MockOverlayRenderer, MockPowerEventListener, OrchestratorConfig, OrchestratorError,
    OverlapPolicy, OverlayRenderer, PowerEvent, PowerListenerError, PowerSource,
    ShimmerOrchestrator, ShimmerTrigger,
};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("runtime")
        .block_on(future)
}

async fn run_to_stream_end(
    events: Vec<PowerEvent>,
    config: OrchestratorConfig,
    overlay: MockOverlayRenderer,
) -> (Result<(), OrchestratorError>, usize) {
    let orchestrator =
        ShimmerOrchestrator::new(MockPowerEventListener::new(events), overlay.clone(), config);
    let result = orchestrator.run().await;
    let play_count = overlay.play_calls.lock().unwrap().len();
    (result, play_count)
}

#[test]
fn initial_state_ac_plays_nothing() {
    let (result, play_count) = block_on(run_to_stream_end(
        vec![PowerEvent::InitialState {
            source: PowerSource::Ac,
        }],
        OrchestratorConfig::default(),
        MockOverlayRenderer::new(),
    ));

    assert_eq!(play_count, 0);
    assert!(matches!(
        result,
        Err(OrchestratorError::Power(PowerListenerError::StreamEnded))
    ));
}

#[test]
fn battery_boot_then_plug_ac_plays_once() {
    let overlay = MockOverlayRenderer::new();
    let (result, play_count) = block_on(run_to_stream_end(
        vec![
            PowerEvent::InitialState {
                source: PowerSource::Battery,
            },
            PowerEvent::Transition {
                from: PowerSource::Battery,
                to: PowerSource::Ac,
            },
        ],
        OrchestratorConfig::default(),
        overlay.clone(),
    ));

    assert_eq!(play_count, 1);
    assert_eq!(
        overlay.play_calls.lock().unwrap()[0].trigger,
        ShimmerTrigger::PowerTransition
    );
    assert!(matches!(
        result,
        Err(OrchestratorError::Power(PowerListenerError::StreamEnded))
    ));
}

#[test]
fn transition_ac_to_battery_plays_nothing() {
    let (result, play_count) = block_on(run_to_stream_end(
        vec![
            PowerEvent::InitialState {
                source: PowerSource::Ac,
            },
            PowerEvent::Transition {
                from: PowerSource::Ac,
                to: PowerSource::Battery,
            },
        ],
        OrchestratorConfig::default(),
        MockOverlayRenderer::new(),
    ));

    assert_eq!(play_count, 0);
    assert!(matches!(
        result,
        Err(OrchestratorError::Power(PowerListenerError::StreamEnded))
    ));
}

#[test]
fn auto_disabled_suppresses_battery_to_ac_play() {
    let config = OrchestratorConfig {
        auto_enabled: false,
        ..OrchestratorConfig::default()
    };

    let (result, play_count) = block_on(run_to_stream_end(
        vec![PowerEvent::Transition {
            from: PowerSource::Battery,
            to: PowerSource::Ac,
        }],
        config,
        MockOverlayRenderer::new(),
    ));

    assert_eq!(play_count, 0);
    assert!(matches!(
        result,
        Err(OrchestratorError::Power(PowerListenerError::StreamEnded))
    ));
}

#[test]
fn dry_run_suppresses_battery_to_ac_play() {
    let config = OrchestratorConfig {
        dry_run: true,
        ..OrchestratorConfig::default()
    };

    let (result, play_count) = block_on(run_to_stream_end(
        vec![PowerEvent::Transition {
            from: PowerSource::Battery,
            to: PowerSource::Ac,
        }],
        config,
        MockOverlayRenderer::new(),
    ));

    assert_eq!(play_count, 0);
    assert!(matches!(
        result,
        Err(OrchestratorError::Power(PowerListenerError::StreamEnded))
    ));
}

#[test]
fn trigger_manual_plays_on_battery() {
    let overlay = MockOverlayRenderer::new();
    let orchestrator = ShimmerOrchestrator::new(
        MockPowerEventListener::new(vec![]),
        overlay.clone(),
        OrchestratorConfig::default(),
    );

    block_on(orchestrator.trigger_manual()).expect("manual trigger should succeed");

    let calls = overlay.play_calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].trigger, ShimmerTrigger::Manual);
}

#[test]
fn overlap_skip_second_trigger_while_playing() {
    let overlay = MockOverlayRenderer::with_delay(Duration::from_millis(200));
    let orchestrator = Arc::new(ShimmerOrchestrator::new(
        MockPowerEventListener::new(vec![
            PowerEvent::InitialState {
                source: PowerSource::Battery,
            },
            PowerEvent::Transition {
                from: PowerSource::Battery,
                to: PowerSource::Ac,
            },
        ]),
        overlay.clone(),
        OrchestratorConfig::default(),
    ));

    let run_orchestrator = Arc::clone(&orchestrator);
    let run_handle =
        std::thread::spawn(move || block_on(async move { run_orchestrator.run().await }));

    while !overlay.is_playing() {
        std::thread::sleep(Duration::from_millis(5));
    }

    block_on({
        let orchestrator = Arc::clone(&orchestrator);
        async move { orchestrator.trigger_manual().await }
    })
    .expect("manual trigger should succeed while playing");

    let _run_result = run_handle.join().expect("run thread should finish");

    assert_eq!(overlay.play_calls.lock().unwrap().len(), 1);
}

#[test]
fn overlap_restart_second_trigger_while_playing() {
    let config = OrchestratorConfig {
        overlap_policy: OverlapPolicy::Restart,
        ..OrchestratorConfig::default()
    };

    let overlay = MockOverlayRenderer::with_delay(Duration::from_millis(200));
    let orchestrator = Arc::new(ShimmerOrchestrator::new(
        MockPowerEventListener::new(vec![
            PowerEvent::InitialState {
                source: PowerSource::Battery,
            },
            PowerEvent::Transition {
                from: PowerSource::Battery,
                to: PowerSource::Ac,
            },
        ]),
        overlay.clone(),
        config,
    ));

    let run_orchestrator = Arc::clone(&orchestrator);
    let run_handle =
        std::thread::spawn(move || block_on(async move { run_orchestrator.run().await }));

    while !overlay.is_playing() {
        std::thread::sleep(Duration::from_millis(5));
    }

    block_on({
        let orchestrator = Arc::clone(&orchestrator);
        async move { orchestrator.trigger_manual().await }
    })
    .expect("manual trigger should succeed while playing");

    let _run_result = run_handle.join().expect("run thread should finish");

    let calls = overlay.play_calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].trigger, ShimmerTrigger::PowerTransition);
    assert_eq!(calls[1].trigger, ShimmerTrigger::Manual);
}

#[test]
fn shutdown_ends_run_cleanly() {
    let overlay = MockOverlayRenderer::with_delay(Duration::from_millis(500));
    let orchestrator = Arc::new(ShimmerOrchestrator::new(
        MockPowerEventListener::keep_alive_after_events(vec![PowerEvent::InitialState {
            source: PowerSource::Ac,
        }]),
        overlay.clone(),
        OrchestratorConfig::default(),
    ));

    let run_orchestrator = Arc::clone(&orchestrator);
    let run_handle =
        std::thread::spawn(move || block_on(async move { run_orchestrator.run().await }));

    std::thread::sleep(Duration::from_millis(20));
    {
        let orchestrator = Arc::clone(&orchestrator);
        orchestrator.shutdown();
    }

    let result = run_handle.join().expect("run thread should finish");
    assert!(matches!(result, Ok(())));
}
