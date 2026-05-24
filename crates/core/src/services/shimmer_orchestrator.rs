//! Shimmer orchestrator — see SPEC.md Module 4.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use crate::domain::{
    OrchestratorConfig, OrchestratorError, OverlapPolicy, OverlayError, PowerEvent,
    PowerListenerError, PowerSource, ShimmerRequest, ShimmerTrigger,
};
use crate::ports::{OverlayRenderer, PowerEventListener, PowerEventStream, StreamRecvResult};

use super::policy::should_auto_play;

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Tracks the last known power source for automatic trigger policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OrchestratorState {
    current_power: PowerSource,
    power_initialized: bool,
}

impl Default for OrchestratorState {
    fn default() -> Self {
        Self {
            current_power: PowerSource::Unknown,
            power_initialized: false,
        }
    }
}

/// Coordinates power events and overlay playback (the only module that uses both ports).
pub struct ShimmerOrchestrator<P, O> {
    power: P,
    overlay: O,
    config: Mutex<OrchestratorConfig>,
    state: Mutex<OrchestratorState>,
    stream: Mutex<Option<PowerEventStream>>,
    shutdown_requested: AtomicBool,
}

impl<P, O> ShimmerOrchestrator<P, O>
where
    P: PowerEventListener,
    O: OverlayRenderer,
{
    /// Creates an orchestrator with the given ports and runtime policy.
    pub fn new(power: P, overlay: O, config: OrchestratorConfig) -> Self {
        Self {
            power,
            overlay,
            config: Mutex::new(config),
            state: Mutex::new(OrchestratorState::default()),
            stream: Mutex::new(None),
            shutdown_requested: AtomicBool::new(false),
        }
    }

    /// Processes power events until [`shutdown`](Self::shutdown) is called or the stream ends.
    ///
    /// # Errors
    ///
    /// Returns [`OrchestratorError::Power`] when the event stream ends unexpectedly or
    /// the adapter reports a subscription error. Returns [`OrchestratorError::Overlay`]
    /// when overlay playback fails.
    pub async fn run(&self) -> Result<(), OrchestratorError> {
        self.shutdown_requested.store(false, Ordering::SeqCst);

        let stream = self.power.subscribe()?;
        *self.lock_stream() = Some(stream);

        loop {
            if self.shutdown_requested.load(Ordering::SeqCst) {
                break;
            }

            let recv_result = {
                let stream_guard = self.lock_stream();
                let Some(stream) = stream_guard.as_ref() else {
                    break;
                };
                stream.recv_timeout(EVENT_POLL_INTERVAL)
            };

            match recv_result {
                StreamRecvResult::Message(Ok(event)) => {
                    self.handle_power_event(event).await?;
                }
                StreamRecvResult::Message(Err(error)) => {
                    return Err(error.into());
                }
                StreamRecvResult::Timeout => {}
                StreamRecvResult::Disconnected => {
                    return Err(PowerListenerError::StreamEnded.into());
                }
            }
        }

        Ok(())
    }

    /// Manually triggers a shimmer (tray "Play now", CLI `--trigger`).
    ///
    /// # Errors
    ///
    /// Returns [`OrchestratorError::Overlay`] when overlay playback fails.
    pub async fn trigger_manual(&self) -> Result<(), OrchestratorError> {
        if self.lock_config().dry_run {
            return Ok(());
        }
        self.play_shimmer(ShimmerTrigger::Manual).await
    }

    /// Replaces runtime orchestrator policy.
    pub fn update_config(&self, config: OrchestratorConfig) {
        *self.lock_config() = config;
    }

    /// Enables or disables automatic Battery→AC triggers without stopping the run loop.
    pub fn set_auto_enabled(&self, enabled: bool) {
        self.lock_config().auto_enabled = enabled;
    }

    /// Cancels any in-flight shimmer and signals [`run`](Self::run) to exit cleanly.
    pub fn shutdown(&self) {
        self.shutdown_requested.store(true, Ordering::SeqCst);
        if self.overlay.is_playing() {
            self.overlay.cancel();
        }
        *self.lock_stream() = None;
    }

    async fn handle_power_event(&self, event: PowerEvent) -> Result<(), OrchestratorError> {
        match event {
            PowerEvent::InitialState { source } => {
                let mut state = self.lock_state();
                state.current_power = source;
                state.power_initialized = true;
            }
            PowerEvent::Transition { to, .. } => {
                {
                    let mut state = self.lock_state();
                    state.current_power = to;
                }

                let config = self.lock_config().clone();
                if should_auto_play(&event, &config) {
                    self.play_shimmer(ShimmerTrigger::PowerTransition).await?;
                }
            }
        }
        Ok(())
    }

    async fn play_shimmer(&self, trigger: ShimmerTrigger) -> Result<(), OrchestratorError> {
        let request = ShimmerRequest {
            config: self.lock_config().shimmer.clone(),
            trigger,
        };

        if self.overlay.is_playing() {
            let overlap_policy = self.lock_config().overlap_policy;
            match overlap_policy {
                OverlapPolicy::Skip => return Ok(()),
                OverlapPolicy::Restart => self.overlay.cancel(),
            }
        }

        match self.overlay.play(request).await {
            Ok(()) | Err(OverlayError::Cancelled) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn lock_config(&self) -> MutexGuard<'_, OrchestratorConfig> {
        self.config
            .lock()
            .expect("orchestrator config mutex poisoned")
    }

    fn lock_state(&self) -> MutexGuard<'_, OrchestratorState> {
        self.state
            .lock()
            .expect("orchestrator state mutex poisoned")
    }

    fn lock_stream(&self) -> MutexGuard<'_, Option<PowerEventStream>> {
        self.stream
            .lock()
            .expect("orchestrator stream mutex poisoned")
    }
}
