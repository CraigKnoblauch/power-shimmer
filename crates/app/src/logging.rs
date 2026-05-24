//! `tracing` subscriber setup for the application binary.

use tracing_subscriber::EnvFilter;

/// Installs the global `tracing` subscriber (idempotent for tests via `try_init`).
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("info,power_shimmer_app=debug,power_shimmer_platform_linux=debug")
    });

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .try_init();
}
