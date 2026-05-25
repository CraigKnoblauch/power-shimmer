//! Shared overlay placement contract — SPEC Module 3 visual requirements.
//!
//! Platform adapters implement [`OverlayPlacementProbe`] / [`OverlayWindowPlacementProbe`]
//! and run contract tests from [`integration`] and [`window_probe`].

pub mod integration;
pub mod placement;
pub mod probe;
pub mod window_probe;

pub use integration::{ContractConfig, ContractError, run_primary_fullscreen_contract};
pub use placement::{
    OverlayPlacement, SIZE_TOLERANCE_PX, assert_placement_covers_monitor,
    select_monitor_index_for_target, validate_monitor_target, window_covers_monitor,
};
pub use probe::OverlayPlacementProbe;
pub use window_probe::{
    OverlayWindowPlacementProbe, run_primary_window_placement_contract,
    run_primary_window_placement_contract_from_overlay_probe,
};
