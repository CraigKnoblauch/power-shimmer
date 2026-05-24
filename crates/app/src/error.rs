//! Application-level errors.

/// Errors surfaced by the composition root.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Configuration file or merge failure.
    #[error("configuration error: {0}")]
    Config(#[from] crate::config::ConfigError),

    /// System tray setup or event loop failure.
    #[error("system tray error: {0}")]
    Tray(String),

    /// Orchestrator or adapter failure.
    #[error("{0}")]
    Orchestrator(#[from] power_shimmer_core::OrchestratorError),

    /// Overlay/session prerequisites missing (e.g. no X11 `DISPLAY`).
    #[error("{0}")]
    Overlay(String),
}

impl AppError {
    /// User-facing exit code for `main`.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        1
    }
}
