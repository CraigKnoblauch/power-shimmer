//! winit-only window placement contract (no GPU).

use power_shimmer_core::MonitorTarget;

use crate::placement::{self, OverlayPlacement};
use crate::probe::OverlayPlacementProbe;

/// Adapter hook for window-only placement verification (no [`OverlayRenderer::play`]).
pub trait OverlayWindowPlacementProbe: Send + Sync {
    /// Creates a primary-target overlay window and returns its placement snapshot without GPU init.
    ///
    /// # Errors
    ///
    /// Returns a human-readable message on session or window setup failure.
    fn probe_primary_window_placement(&self) -> Result<OverlayPlacement, String>;

    /// Platform session precondition (e.g. X11 `DISPLAY` available).
    ///
    /// # Errors
    ///
    /// Returns a human-readable message when the overlay session cannot run.
    fn require_overlay_session(&self) -> Result<(), String>;
}

/// Runs the primary-monitor window placement contract (no GPU / no full `play`).
///
/// # Errors
///
/// Returns [`super::integration::ContractError`] on session, geometry, or target mismatch.
pub fn run_primary_window_placement_contract<P>(probe: P) -> Result<(), super::integration::ContractError>
where
    P: OverlayWindowPlacementProbe,
{
    probe
        .require_overlay_session()
        .map_err(super::integration::ContractError::SessionUnavailable)?;

    let placement = probe
        .probe_primary_window_placement()
        .map_err(super::integration::ContractError::SessionUnavailable)?;

    if placement.monitor_target != MonitorTarget::Primary {
        return Err(super::integration::ContractError::WrongMonitorTarget {
            expected: MonitorTarget::Primary,
            got: placement.monitor_target,
        });
    }

    if !placement::window_covers_monitor(placement.window_size, placement.monitor_size) {
        return Err(super::integration::ContractError::PlacementMismatch {
            window: placement.window_size,
            monitor: placement.monitor_size,
        });
    }

    Ok(())
}

/// Convenience when the adapter also implements [`OverlayPlacementProbe`] for session checks.
pub fn run_primary_window_placement_contract_from_overlay_probe<P>(probe: P) -> Result<(), super::integration::ContractError>
where
    P: OverlayPlacementProbe + OverlayWindowPlacementProbe,
{
    run_primary_window_placement_contract(probe)
}
