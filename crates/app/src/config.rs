//! TOML configuration loading and merge into [`OrchestratorConfig`].

use std::fs;
use std::path::{Path, PathBuf};

use power_shimmer_core::{MonitorTarget, OrchestratorConfig, OverlapPolicy, ShimmerConfig};
use serde::Deserialize;
use thiserror::Error;

/// Configuration load or validation failure.
#[derive(Debug, Error, PartialEq)]
pub enum ConfigError {
    /// Failed to read or parse a config file.
    #[error("failed to read config at {path}: {message}")]
    Read {
        /// Config file path.
        path: PathBuf,
        /// Underlying I/O or parse error.
        message: String,
    },

    /// Invalid field value after merge.
    #[error("invalid {field}: {message}")]
    Invalid {
        /// TOML field name.
        field: &'static str,
        /// Human-readable reason.
        message: String,
    },
}

/// Root TOML document (`config/default.toml` shape).
#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct FileRoot {
    /// Orchestrator policy section.
    pub orchestrator: Option<FileOrchestrator>,
}

/// `[orchestrator]` table.
#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct FileOrchestrator {
    /// Automatic Battery→AC triggers.
    pub auto_enabled: Option<bool>,
    /// Suppress overlay plays.
    pub dry_run: Option<bool>,
    /// `"skip"` or `"restart"`.
    pub overlap_policy: Option<String>,
    /// Visual parameters.
    pub shimmer: Option<FileShimmer>,
}

/// `[orchestrator.shimmer]` table.
#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct FileShimmer {
    /// Total animation length in milliseconds.
    pub duration_ms: Option<u32>,
    /// Peak overlay opacity.
    pub opacity: Option<f32>,
    /// Shimmer scroll speed multiplier.
    pub speed: Option<f32>,
    /// Display target (`"primary"` in v1).
    pub monitor: Option<String>,
}

/// Resolved config file path and optional parsed contents.
#[derive(Debug)]
pub struct LoadedConfig {
    /// Path used when a file was found and read successfully.
    pub path: Option<PathBuf>,
    /// Parsed overlay; `None` when no file exists.
    pub file: Option<FileRoot>,
}

/// Returns SPEC defaults (no file I/O).
#[must_use]
pub fn default_orchestrator_config() -> OrchestratorConfig {
    OrchestratorConfig::default()
}

/// Locates and reads an optional user config file.
///
/// # Errors
///
/// Returns [`ConfigError`] when a config path is resolved but the file cannot be read or parsed.
pub fn load_config_file() -> Result<LoadedConfig, ConfigError> {
    let Some(path) = resolve_config_path() else {
        return Ok(LoadedConfig {
            path: None,
            file: None,
        });
    };

    let contents = fs::read_to_string(&path).map_err(|error| ConfigError::Read {
        path: path.clone(),
        message: error.to_string(),
    })?;

    let file: FileRoot = toml::from_str(&contents).map_err(|error| ConfigError::Read {
        path: path.clone(),
        message: error.to_string(),
    })?;

    Ok(LoadedConfig {
        path: Some(path),
        file: Some(file),
    })
}

/// Merges optional file settings onto `base`, then validates shimmer fields.
///
/// # Errors
///
/// Returns [`ConfigError`] on invalid enum strings or numeric ranges.
pub fn merge_file(
    base: OrchestratorConfig,
    file: FileRoot,
) -> Result<OrchestratorConfig, ConfigError> {
    let mut config = base;

    if let Some(section) = file.orchestrator {
        if let Some(auto_enabled) = section.auto_enabled {
            config.auto_enabled = auto_enabled;
        }
        if let Some(dry_run) = section.dry_run {
            config.dry_run = dry_run;
        }
        if let Some(policy) = section.overlap_policy {
            config.overlap_policy = parse_overlap_policy(&policy)?;
        }
        if let Some(shimmer) = section.shimmer {
            config.shimmer = merge_shimmer(config.shimmer, shimmer)?;
        }
    }

    validate_shimmer(&config.shimmer)?;
    Ok(config)
}

/// Builds [`OrchestratorConfig`] from defaults plus optional file.
///
/// # Errors
///
/// Returns [`ConfigError`] when the file is present but invalid.
pub fn orchestrator_from_file(file: Option<FileRoot>) -> Result<OrchestratorConfig, ConfigError> {
    match file {
        Some(root) => merge_file(default_orchestrator_config(), root),
        None => Ok(default_orchestrator_config()),
    }
}

fn merge_shimmer(base: ShimmerConfig, file: FileShimmer) -> Result<ShimmerConfig, ConfigError> {
    let mut shimmer = base;

    if let Some(duration_ms) = file.duration_ms {
        shimmer.duration_ms = duration_ms;
    }
    if let Some(opacity) = file.opacity {
        shimmer.opacity = opacity;
    }
    if let Some(speed) = file.speed {
        shimmer.speed = speed;
    }
    if let Some(monitor) = file.monitor {
        shimmer.monitor = parse_monitor(&monitor)?;
    }

    Ok(shimmer)
}

fn parse_overlap_policy(value: &str) -> Result<OverlapPolicy, ConfigError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "skip" => Ok(OverlapPolicy::Skip),
        "restart" => Ok(OverlapPolicy::Restart),
        _ => Err(ConfigError::Invalid {
            field: "overlap_policy",
            message: format!("expected \"skip\" or \"restart\", got \"{value}\""),
        }),
    }
}

fn parse_monitor(value: &str) -> Result<MonitorTarget, ConfigError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "primary" => Ok(MonitorTarget::Primary),
        _ => Err(ConfigError::Invalid {
            field: "monitor",
            message: format!("v1 only supports \"primary\", got \"{value}\""),
        }),
    }
}

/// Validates shimmer numeric ranges per SPEC.
///
/// # Errors
///
/// Returns [`ConfigError`] when a field is out of range.
pub fn validate_shimmer(shimmer: &ShimmerConfig) -> Result<(), ConfigError> {
    if shimmer.duration_ms == 0 {
        return Err(ConfigError::Invalid {
            field: "duration_ms",
            message: "must be greater than 0".to_string(),
        });
    }
    if !(0.0..=1.0).contains(&shimmer.opacity) {
        return Err(ConfigError::Invalid {
            field: "opacity",
            message: "must be in [0.0, 1.0]".to_string(),
        });
    }
    if shimmer.speed <= 0.0 {
        return Err(ConfigError::Invalid {
            field: "speed",
            message: "must be greater than 0".to_string(),
        });
    }
    Ok(())
}

fn resolve_config_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("POWER_SHIMMER_CONFIG") {
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }

    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;

    let path = base.join("power-shimmer").join("config.toml");
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

/// Parses TOML from a path (tests and tooling).
///
/// # Errors
///
/// Returns [`ConfigError`] on I/O or parse failure.
pub fn parse_file_at(path: &Path, contents: &str) -> Result<FileRoot, ConfigError> {
    toml::from_str(contents).map_err(|error| ConfigError::Read {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_without_file_matches_core() {
        let config = orchestrator_from_file(None).expect("defaults");
        assert_eq!(config, OrchestratorConfig::default());
    }

    #[test]
    fn partial_merge_overrides_one_field() {
        let file = FileRoot {
            orchestrator: Some(FileOrchestrator {
                shimmer: Some(FileShimmer {
                    opacity: Some(0.5),
                    ..FileShimmer::default()
                }),
                ..FileOrchestrator::default()
            }),
        };
        let config = merge_file(default_orchestrator_config(), file).expect("merge");
        assert!((config.shimmer.opacity - 0.5).abs() < f32::EPSILON);
        assert_eq!(config.shimmer.duration_ms, 2_000);
        assert!(config.auto_enabled);
    }

    #[test]
    fn invalid_overlap_policy_errors() {
        let file = FileRoot {
            orchestrator: Some(FileOrchestrator {
                overlap_policy: Some("queue".into()),
                ..FileOrchestrator::default()
            }),
        };
        let err = merge_file(default_orchestrator_config(), file).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                field: "overlap_policy",
                ..
            }
        ));
    }

    #[test]
    fn invalid_opacity_errors() {
        let file = FileRoot {
            orchestrator: Some(FileOrchestrator {
                shimmer: Some(FileShimmer {
                    opacity: Some(1.5),
                    ..FileShimmer::default()
                }),
                ..FileOrchestrator::default()
            }),
        };
        let err = merge_file(default_orchestrator_config(), file).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                field: "opacity",
                ..
            }
        ));
    }
}
