//! Integration tests for TOML configuration merge.

use std::io::Write;

use power_shimmer_app::config::{merge_file, parse_file_at};
use power_shimmer_core::{OrchestratorConfig, OverlapPolicy};
use tempfile::NamedTempFile;

#[test]
fn parses_default_toml_shape() {
    let contents = include_str!("../../../config/default.toml");
    let root = parse_file_at(std::path::Path::new("default.toml"), contents).expect("parse");
    let config = merge_file(OrchestratorConfig::default(), root).expect("merge");
    assert!(config.auto_enabled);
    assert!(!config.dry_run);
    assert_eq!(config.overlap_policy, OverlapPolicy::Skip);
    assert_eq!(config.shimmer.duration_ms, 2_000);
}

#[test]
fn tempfile_override_restart_policy() {
    let mut file = NamedTempFile::new().expect("tempfile");
    writeln!(
        file,
        r#"
[orchestrator]
overlap_policy = "restart"
"#
    )
    .expect("write");

    let contents = std::fs::read_to_string(file.path()).expect("read");
    let root = parse_file_at(file.path(), &contents).expect("parse");
    let config = merge_file(OrchestratorConfig::default(), root).expect("merge");
    assert_eq!(config.overlap_policy, OverlapPolicy::Restart);
}
