//! CLI parsing and merge with file configuration.

use clap::Parser;
use power_shimmer_core::OrchestratorConfig;

use crate::config::{self, ConfigError, LoadedConfig};

/// Command-line interface (SPEC App-Layer Integration).
#[derive(Debug, Parser, Clone)]
#[command(
    name = "power-shimmer",
    version,
    about = "Rainbow shimmer overlay when battery transitions to AC power"
)]
pub struct Cli {
    /// Play shimmer once via manual trigger, then exit (no tray, no power loop).
    #[arg(long)]
    pub trigger: bool,

    /// Log trigger intent without calling the overlay.
    #[arg(long)]
    pub dry_run: bool,

    /// Run the daemon without a system tray icon.
    #[arg(long)]
    pub no_tray: bool,

    /// Override shimmer duration in milliseconds.
    #[arg(long)]
    pub duration_ms: Option<u32>,

    /// Override peak shimmer opacity in `[0.0, 1.0]`.
    #[arg(long)]
    pub opacity: Option<f32>,
}

/// Daemon vs one-shot CLI entry path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryMode {
    /// `--trigger`: manual play and exit.
    Trigger,
    /// Default: background daemon with optional tray.
    Daemon {
        /// When false, `--no-tray` was passed.
        tray: bool,
    },
}

/// Fully resolved runtime settings for the composition root.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveConfig {
    /// Orchestrator policy after file + CLI merge.
    pub orchestrator: OrchestratorConfig,
    /// How `main` should enter the application.
    pub entry: EntryMode,
}

impl Cli {
    /// Resolves daemon vs trigger entry mode.
    #[must_use]
    pub fn entry_mode(&self) -> EntryMode {
        if self.trigger {
            EntryMode::Trigger
        } else {
            EntryMode::Daemon {
                tray: !self.no_tray,
            }
        }
    }
}

impl EffectiveConfig {
    /// Loads optional config file, merges CLI overrides, validates shimmer fields.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when the config file or merged values are invalid.
    pub fn from_cli_and_file(cli: &Cli, loaded: LoadedConfig) -> Result<Self, ConfigError> {
        let mut orchestrator = config::orchestrator_from_file(loaded.file)?;

        if cli.dry_run {
            orchestrator.dry_run = true;
        }
        if let Some(duration_ms) = cli.duration_ms {
            orchestrator.shimmer.duration_ms = duration_ms;
        }
        if let Some(opacity) = cli.opacity {
            orchestrator.shimmer.opacity = opacity;
        }

        config::validate_shimmer(&orchestrator.shimmer)?;

        Ok(Self {
            orchestrator,
            entry: cli.entry_mode(),
        })
    }

    /// Convenience: parse CLI args and load config from disk.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when configuration is invalid.
    pub fn load() -> Result<(Self, Option<std::path::PathBuf>), ConfigError> {
        let cli = Cli::parse();
        let loaded = config::load_config_file()?;
        let path = loaded.path.clone();
        let effective = Self::from_cli_and_file(&cli, loaded)?;
        Ok((effective, path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_entry_mode() {
        let cli = Cli {
            trigger: true,
            dry_run: false,
            no_tray: false,
            duration_ms: None,
            opacity: None,
        };
        assert_eq!(cli.entry_mode(), EntryMode::Trigger);
    }

    #[test]
    fn daemon_respects_no_tray() {
        let cli = Cli {
            trigger: false,
            dry_run: false,
            no_tray: true,
            duration_ms: None,
            opacity: None,
        };
        assert_eq!(cli.entry_mode(), EntryMode::Daemon { tray: false });
    }

    #[test]
    fn cli_overrides_duration() {
        let cli = Cli {
            trigger: false,
            dry_run: false,
            no_tray: false,
            duration_ms: Some(3_000),
            opacity: None,
        };
        let loaded = LoadedConfig {
            path: None,
            file: None,
        };
        let effective = EffectiveConfig::from_cli_and_file(&cli, loaded).expect("merge");
        assert_eq!(effective.orchestrator.shimmer.duration_ms, 3_000);
    }
}
