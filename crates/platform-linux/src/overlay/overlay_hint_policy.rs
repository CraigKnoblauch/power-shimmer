//! Testable X11 overlay hint policy for taskbar/pager exclusion.
//!
//! Production overlay code must satisfy the contracts asserted in [`tests`].
//! See `notes/issues/overlay-taskbar-icon-during-animation.md`.

use winit::platform::x11::WindowType;

/// EWMH `_NET_WM_STATE` property names applied to hide the overlay from taskbar and pager.
///
/// # Implementation note
///
/// `x11_click_through::set_skip_taskbar` must send one `_NET_WM_STATE` ADD
/// [`ClientMessage`](x11rb::protocol::xproto::ClientMessageEvent) per entry.
#[must_use]
#[allow(dead_code)] // wired by overlay impl in follow-up; tests assert contract
pub fn taskbar_hiding_net_wm_state_atoms() -> &'static [&'static str] {
    &[
        "_NET_WM_STATE_SKIP_TASKBAR",
        "_NET_WM_STATE_SKIP_PAGER",
    ]
}

/// When `true`, WM-state hints (taskbar/pager) are applied only after the overlay window is shown/mapped.
#[must_use]
#[allow(dead_code)]
pub const fn wm_state_hints_apply_after_show() -> bool {
    false
}

/// `_NET_WM_WINDOW_TYPE` values for the transient overlay (via `WindowAttributesExtX11`).
#[must_use]
#[allow(dead_code)]
pub fn overlay_x11_window_types() -> Vec<WindowType> {
    vec![WindowType::Normal]
}

#[cfg(test)]
mod tests {
    use super::{
        overlay_x11_window_types, taskbar_hiding_net_wm_state_atoms,
        wm_state_hints_apply_after_show as policy_wm_state_hints_apply_after_show,
    };
    use winit::platform::x11::WindowType;

    #[test]
    fn taskbar_hiding_includes_skip_taskbar_and_skip_pager() {
        let atoms = taskbar_hiding_net_wm_state_atoms();
        assert_eq!(
            atoms.len(),
            2,
            "overlay must request SKIP_TASKBAR and SKIP_PAGER (SPEC: hidden from taskbar)"
        );
        assert!(atoms.contains(&"_NET_WM_STATE_SKIP_TASKBAR"));
        assert!(atoms.contains(&"_NET_WM_STATE_SKIP_PAGER"));
    }

    #[test]
    fn wm_state_hints_are_applied_after_show() {
        assert!(
            policy_wm_state_hints_apply_after_show(),
            "WM state hints must be applied after set_visible(true), not only before map"
        );
    }

    #[test]
    fn overlay_uses_notification_window_type() {
        assert_eq!(
            overlay_x11_window_types(),
            vec![WindowType::Notification],
            "overlay must use Notification window type to avoid taskbar listing"
        );
    }
}
