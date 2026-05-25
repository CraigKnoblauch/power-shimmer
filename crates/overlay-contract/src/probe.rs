//! Test-only probe surface for overlay placement verification.

use crate::placement::OverlayPlacement;

/// Adapter hook for contract tests — not part of [`power_shimmer_core::ports::OverlayRenderer`].
pub trait OverlayPlacementProbe: Send + Sync {
    /// Most recent placement recorded during overlay window setup.
    fn last_placement(&self) -> Option<OverlayPlacement>;

    /// Platform session precondition (e.g. X11 `DISPLAY` available).
    ///
    /// # Errors
    ///
    /// Returns a human-readable message when the overlay session cannot run.
    fn require_overlay_session(&self) -> Result<(), String>;
}
