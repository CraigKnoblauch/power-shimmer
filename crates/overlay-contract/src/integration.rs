//! Shared async integration runner for primary-monitor fullscreen placement.

use std::time::Duration;

use power_shimmer_core::ports::OverlayRenderer;
use power_shimmer_core::{MonitorTarget, ShimmerConfig, ShimmerRequest, ShimmerTrigger};

use crate::placement::window_covers_monitor;
use crate::probe::OverlayPlacementProbe;

/// Configuration for [`run_primary_fullscreen_contract`].
#[derive(Debug, Clone)]
pub struct ContractConfig {
    /// Shimmer duration used during the contract play.
    pub duration_ms: u32,
    /// Maximum wait for a placement snapshot after `play` starts.
    pub poll_timeout: Duration,
    /// Interval between `last_placement` polls.
    pub poll_interval: Duration,
}

impl Default for ContractConfig {
    fn default() -> Self {
        Self {
            duration_ms: 800,
            poll_timeout: Duration::from_secs(15),
            poll_interval: Duration::from_millis(20),
        }
    }
}

/// Contract test failure.
#[derive(Debug)]
pub enum ContractError {
    /// Session precondition failed (no display, wrong platform, etc.).
    SessionUnavailable(String),
    /// Placement snapshot not recorded before timeout.
    PlacementTimeout,
    /// Window size does not cover target monitor per contract policy.
    PlacementMismatch {
        /// Overlay window physical size.
        window: (u32, u32),
        /// Target monitor physical size.
        monitor: (u32, u32),
    },
    /// Unexpected monitor target in snapshot.
    WrongMonitorTarget {
        /// Expected monitor target from the contract request.
        expected: MonitorTarget,
        /// Monitor target recorded in the placement snapshot.
        got: MonitorTarget,
    },
    /// `play` returned an error.
    PlayFailed(String),
    /// Renderer still marked playing after `play` completed.
    StillPlaying,
}

impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SessionUnavailable(msg) => write!(f, "overlay session unavailable: {msg}"),
            Self::PlacementTimeout => {
                write!(f, "timed out waiting for overlay placement snapshot")
            }
            Self::PlacementMismatch { window, monitor } => write!(
                f,
                "overlay window {window:?} does not cover monitor {monitor:?}"
            ),
            Self::WrongMonitorTarget { expected, got } => write!(
                f,
                "expected monitor target {expected:?}, got {got:?}"
            ),
            Self::PlayFailed(msg) => write!(f, "overlay play failed: {msg}"),
            Self::StillPlaying => write!(f, "overlay still playing after play completed"),
        }
    }
}

impl std::error::Error for ContractError {}

/// Runs the primary-monitor fullscreen placement contract against a real renderer.
///
/// # Errors
///
/// Returns [`ContractError`] when session, placement, or play semantics fail.
pub async fn run_primary_fullscreen_contract<R>(
    renderer: R,
    config: ContractConfig,
) -> Result<(), ContractError>
where
    R: OverlayRenderer + OverlayPlacementProbe,
{
    renderer
        .require_overlay_session()
        .map_err(ContractError::SessionUnavailable)?;

    let request = ShimmerRequest {
        config: ShimmerConfig {
            duration_ms: config.duration_ms,
            monitor: MonitorTarget::Primary,
            ..ShimmerConfig::default()
        },
        trigger: ShimmerTrigger::Manual,
    };

    let play = renderer.play(request);
    let deadline = tokio::time::Instant::now() + config.poll_timeout;
    let placement = loop {
        if let Some(p) = renderer.last_placement() {
            if !window_covers_monitor(p.window_size, p.monitor_size) {
                return Err(ContractError::PlacementMismatch {
                    window: p.window_size,
                    monitor: p.monitor_size,
                });
            }
            if p.monitor_target != MonitorTarget::Primary {
                return Err(ContractError::WrongMonitorTarget {
                    expected: MonitorTarget::Primary,
                    got: p.monitor_target,
                });
            }
            break p;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(ContractError::PlacementTimeout);
        }
        tokio::time::sleep(config.poll_interval).await;
    };

    let _placement = placement;

    play.await
        .map_err(|e| ContractError::PlayFailed(e.to_string()))?;

    if renderer.is_playing() {
        return Err(ContractError::StillPlaying);
    }

    Ok(())
}
