//! Re-exports shared overlay placement contract ([`power_shimmer_overlay_contract`]).
pub use power_shimmer_overlay_contract::{
    OverlayPlacement, SIZE_TOLERANCE_PX, assert_placement_covers_monitor,
    select_monitor_index_for_target, validate_monitor_target, window_covers_monitor,
};
