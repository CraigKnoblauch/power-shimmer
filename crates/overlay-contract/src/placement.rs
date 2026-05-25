//! Full-screen overlay geometry and monitor selection (SPEC visual contract).

use power_shimmer_core::MonitorTarget;

/// Maximum per-axis difference (physical pixels) between window and monitor size.
pub const SIZE_TOLERANCE_PX: u32 = 1;

/// Recorded window vs monitor sizes after overlay window creation (verification hook).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayPlacement {
    /// Overlay window physical size `(width, height)`.
    pub window_size: (u32, u32),
    /// Target monitor physical size `(width, height)`.
    pub monitor_size: (u32, u32),
    /// Monitor target from the shimmer request.
    pub monitor_target: MonitorTarget,
}

/// Returns true when `window` matches `monitor` within [`SIZE_TOLERANCE_PX`] on each axis.
#[must_use]
pub fn window_covers_monitor(window: (u32, u32), monitor: (u32, u32)) -> bool {
    window.0.abs_diff(monitor.0) <= SIZE_TOLERANCE_PX
        && window.1.abs_diff(monitor.1) <= SIZE_TOLERANCE_PX
}

/// Panics when placement does not satisfy [`window_covers_monitor`].
#[must_use]
pub fn assert_placement_covers_monitor(placement: OverlayPlacement) -> OverlayPlacement {
    assert!(
        window_covers_monitor(placement.window_size, placement.monitor_size),
        "overlay window {:?} does not cover monitor {:?} (target {:?})",
        placement.window_size,
        placement.monitor_size,
        placement.monitor_target
    );
    placement
}

/// v1 monitor selection policy: primary if reported, else first available monitor.
///
/// `primary_index` is the index of the primary monitor in `0..monitor_count`, if known.
#[must_use]
pub fn select_monitor_index_for_target(
    primary_index: Option<usize>,
    monitor_count: usize,
    target: MonitorTarget,
) -> Option<usize> {
    match target {
        MonitorTarget::Primary => {
            if let Some(idx) = primary_index {
                if idx < monitor_count {
                    return Some(idx);
                }
            }
            if monitor_count > 0 {
                Some(0)
            } else {
                None
            }
        }
    }
}

/// Returns an error message when `target` is not supported in v1.
///
/// # Errors
///
/// Returns `Err` when `target` is not implemented by the overlay adapter.
pub fn validate_monitor_target(target: MonitorTarget) -> Result<(), String> {
    match target {
        MonitorTarget::Primary => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_covers_monitor_exact_match() {
        assert!(window_covers_monitor((1920, 1080), (1920, 1080)));
    }

    #[test]
    fn window_covers_monitor_rejects_mismatch() {
        assert!(!window_covers_monitor((1920, 1080), (1280, 720)));
    }

    #[test]
    fn window_covers_monitor_allows_one_pixel_tolerance() {
        assert!(window_covers_monitor((1921, 1080), (1920, 1080)));
        assert!(window_covers_monitor((1920, 1081), (1920, 1080)));
        assert!(!window_covers_monitor((1922, 1080), (1920, 1080)));
    }

    #[test]
    fn select_primary_when_reported() {
        assert_eq!(
            select_monitor_index_for_target(Some(0), 2, MonitorTarget::Primary),
            Some(0)
        );
        assert_eq!(
            select_monitor_index_for_target(Some(1), 2, MonitorTarget::Primary),
            Some(1)
        );
    }

    #[test]
    fn select_first_when_primary_missing() {
        assert_eq!(
            select_monitor_index_for_target(None, 2, MonitorTarget::Primary),
            Some(0)
        );
    }

    #[test]
    fn select_none_when_no_monitors() {
        assert_eq!(
            select_monitor_index_for_target(None, 0, MonitorTarget::Primary),
            None
        );
        assert_eq!(
            select_monitor_index_for_target(Some(0), 0, MonitorTarget::Primary),
            None
        );
    }

    #[test]
    fn select_falls_back_when_primary_index_out_of_range() {
        assert_eq!(
            select_monitor_index_for_target(Some(5), 2, MonitorTarget::Primary),
            Some(0)
        );
    }

    #[test]
    fn validate_monitor_target_accepts_primary() {
        assert!(validate_monitor_target(MonitorTarget::Primary).is_ok());
    }

    #[test]
    fn assert_placement_covers_monitor_passes_valid() {
        let p = OverlayPlacement {
            window_size: (1920, 1080),
            monitor_size: (1920, 1080),
            monitor_target: MonitorTarget::Primary,
        };
        assert_eq!(assert_placement_covers_monitor(p), p);
    }
}
