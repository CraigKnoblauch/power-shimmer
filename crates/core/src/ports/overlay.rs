//! Overlay renderer port — see SPEC.md Module 3.

#![allow(async_fn_in_trait)]

use crate::domain::{OverlayError, ShimmerRequest};

/// Visual overlay port. Implemented by platform adapters only.
///
/// The renderer is stateless with respect to power — it only responds to
/// explicit `play` / `cancel` invocations from the orchestrator.
pub trait OverlayRenderer: Send + Sync {
    /// Play one shimmer animation.
    ///
    /// Contract:
    /// - Creates window + GPU resources lazily if not present.
    /// - Covers `request.config.monitor` (v1: primary display bounds).
    /// - Window is click-through, does not take focus, hidden from taskbar (best effort).
    /// - Animates for `request.config.duration_ms`, then hides window.
    /// - Returns `Ok(())` on normal completion.
    /// - Returns `Err(OverlayError::Cancelled)` if [`cancel`](Self::cancel) was called mid-play.
    ///
    /// Must be safe to call from a dedicated async task (orchestrator-owned).
    async fn play(&self, request: ShimmerRequest) -> Result<(), OverlayError>;

    /// Returns `true` while [`play`](Self::play) is in progress (including fade-out teardown).
    fn is_playing(&self) -> bool;

    /// Abort the current animation immediately and release the overlay window.
    ///
    /// No-op if idle. After cancel, [`is_playing`](Self::is_playing) becomes `false`.
    fn cancel(&self);
}
