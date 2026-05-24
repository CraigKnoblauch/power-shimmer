//! Composition root: production adapters and orchestrator entry paths.

use std::sync::Arc;
use std::time::Duration;

use power_shimmer_core::{OrchestratorConfig, OrchestratorError, ShimmerOrchestrator};
use power_shimmer_platform_linux::power::{LinuxPowerBackend, LinuxPowerListener};
use power_shimmer_platform_linux::WgpuShimmerRenderer;
use tracing::{error, info, warn};

use crate::cli::EntryMode;
use crate::error::AppError;

/// Production orchestrator type for Linux v1.
pub type LinuxOrchestrator =
    ShimmerOrchestrator<LinuxPowerListener<LinuxPowerBackend>, WgpuShimmerRenderer>;

const TEARDOWN_GRACE: Duration = Duration::from_millis(600);

/// Builds UPower/sysfs power listener, X11 overlay, and orchestrator.
pub fn build_orchestrator(config: OrchestratorConfig) -> Arc<LinuxOrchestrator> {
    info!("initializing overlay renderer");
    let overlay = WgpuShimmerRenderer::new();

    let backend = LinuxPowerBackend::select();
    let power = LinuxPowerListener::new(backend);

    info!(
        auto_enabled = config.auto_enabled,
        dry_run = config.dry_run,
        duration_ms = config.shimmer.duration_ms,
        opacity = config.shimmer.opacity,
        "initializing orchestrator"
    );

    Arc::new(ShimmerOrchestrator::new(power, overlay, config))
}

/// Plays one manual shimmer and exits (`--trigger`).
///
/// # Errors
///
/// Returns [`AppError`] when overlay playback fails.
pub async fn run_trigger(config: OrchestratorConfig) -> Result<(), AppError> {
    info!(dry_run = config.dry_run, "entry path: CLI trigger");

    if config.dry_run {
        info!("dry_run: skipping manual shimmer (CLI --trigger)");
        return Ok(());
    }

    let orchestrator = build_orchestrator(config);

    match orchestrator.trigger_manual().await {
        Ok(()) => info!("manual shimmer completed"),
        Err(error) => {
            error!(%error, "manual shimmer failed");
            return Err(error.into());
        }
    }

    tokio::time::sleep(TEARDOWN_GRACE).await;
    Ok(())
}

/// Runs the background daemon: power loop, optional tray, until shutdown.
///
/// # Panics
///
/// Panics if Ctrl+C listener registration fails in headless (`--no-tray`) mode.
///
/// # Errors
///
/// Returns [`AppError`] on tray setup failure.
pub async fn run_daemon(config: OrchestratorConfig, tray: bool) -> Result<(), AppError> {
    info!(tray, dry_run = config.dry_run, "entry path: daemon");

    let auto_enabled_initial = config.auto_enabled;
    let orchestrator = build_orchestrator(config);
    let auto_enabled = Arc::new(std::sync::atomic::AtomicBool::new(auto_enabled_initial));

    let orchestrator_run = Arc::clone(&orchestrator);
    let run_handle = tokio::spawn(async move {
        match orchestrator_run.run().await {
            Ok(()) => info!("orchestrator power loop exited"),
            Err(OrchestratorError::Power(power_shimmer_core::PowerListenerError::StreamEnded)) => {
                warn!("power event stream ended");
            }
            Err(error) => {
                error!(%error, "orchestrator power loop failed");
            }
        }
    });

    if tray {
        run_tray_or_error(&orchestrator, &auto_enabled)?;
    } else {
        info!("headless daemon; press Ctrl+C to quit");
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for Ctrl+C");
        info!("Ctrl+C received");
        orchestrator.shutdown();
    }

    let _ = run_handle.await;
    info!("daemon shutdown complete");
    Ok(())
}

/// Dispatches the approved entry mode.
///
/// # Errors
///
/// Returns [`AppError`] when the selected entry path fails.
pub async fn run(entry: EntryMode, config: OrchestratorConfig) -> Result<(), AppError> {
    match entry {
        EntryMode::Trigger => run_trigger(config).await,
        EntryMode::Daemon { tray } => run_daemon(config, tray).await,
    }
}

fn run_tray_or_error(
    orchestrator: &Arc<LinuxOrchestrator>,
    auto_enabled: &Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), AppError> {
    #[cfg(all(feature = "linux", feature = "tray"))]
    {
        crate::tray::run_tray(orchestrator, auto_enabled)
    }

    #[cfg(all(feature = "linux", not(feature = "tray")))]
    {
        let _ = (orchestrator, auto_enabled);
        Err(AppError::Tray(
            "tray support not enabled; rebuild with `--features tray`".to_string(),
        ))
    }

    #[cfg(not(feature = "linux"))]
    {
        let _ = (orchestrator, auto_enabled);
        Err(AppError::Tray("linux platform not enabled".to_string()))
    }
}
