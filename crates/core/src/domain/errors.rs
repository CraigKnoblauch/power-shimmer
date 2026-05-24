//! Domain error types — see SPEC.md Module 1.

/// Errors from the power event listener port.
#[derive(Debug, thiserror::Error)]
pub enum PowerListenerError {
    /// Failed to begin OS power subscription.
    #[error("failed to subscribe to power events: {0}")]
    SubscribeFailed(String),
    /// Event stream ended before the consumer shut down.
    #[error("power event stream ended unexpectedly")]
    StreamEnded,
}

/// Errors from the overlay renderer port.
#[derive(Debug, thiserror::Error)]
pub enum OverlayError {
    #[error("overlay already disposed")]
    Disposed,
    #[error("failed to create overlay window: {0}")]
    WindowCreationFailed(String),
    #[error("render error: {0}")]
    RenderFailed(String),
    #[error("play cancelled")]
    Cancelled,
}

/// Errors surfaced by the shimmer orchestrator.
#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    #[error("power listener error: {0}")]
    Power(#[from] PowerListenerError),
    #[error("overlay error: {0}")]
    Overlay(#[from] OverlayError),
}
