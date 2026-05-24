//! Power Shimmer binary entry point.

use std::process::ExitCode;

use clap::Parser;
use power_shimmer_app::cli::{Cli, EffectiveConfig};
use power_shimmer_app::config;
use power_shimmer_app::error::AppError;
use power_shimmer_app::logging;
use tracing::{error, info};

#[cfg(feature = "linux")]
use power_shimmer_app::wiring;

#[cfg(feature = "linux")]
#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    logging::init();

    let cli = Cli::parse();
    let loaded = match config::load_config_file() {
        Ok(loaded) => loaded,
        Err(error) => {
            error!(%error, "failed to load configuration");
            return ExitCode::from(AppError::Config(error).exit_code());
        }
    };

    if let Some(ref path) = loaded.path {
        info!(path = %path.display(), "loaded configuration file");
    }

    let effective = match EffectiveConfig::from_cli_and_file(&cli, loaded) {
        Ok(effective) => effective,
        Err(error) => {
            error!(%error, "invalid configuration");
            return ExitCode::from(AppError::Config(error).exit_code());
        }
    };

    info!(
        ?effective.entry,
        dry_run = effective.orchestrator.dry_run,
        auto_enabled = effective.orchestrator.auto_enabled,
        "starting power-shimmer"
    );

    if let Err(error) = wiring::run(effective.entry, effective.orchestrator).await {
        error!(%error, "power-shimmer exited with error");
        return ExitCode::from(error.exit_code());
    }

    info!("power-shimmer exited");
    ExitCode::SUCCESS
}

#[cfg(not(feature = "linux"))]
fn main() -> ExitCode {
    eprintln!("power-shimmer: no platform adapter enabled for this build");
    eprintln!("Rebuild with `--features linux` (default on Linux).");
    ExitCode::FAILURE
}
