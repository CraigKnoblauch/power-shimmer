//! Testable X11 overlay policy (taskbar hiding + full-screen geometry).
//!
//! Production overlay code must satisfy the contracts asserted in [`tests`].
//!
//! - `notes/issues/overlay-taskbar-icon-during-animation.md`
//! - `notes/issues/overlay-quarter-screen-regression.md`

use winit::platform::x11::WindowType;

/// EWMH `_NET_WM_STATE` property names applied to hide the overlay from taskbar and pager.
///
/// `x11_click_through::apply_taskbar_hiding_wm_state` sends one `_NET_WM_STATE` ADD
/// [`ClientMessage`](x11rb::protocol::xproto::ClientMessageEvent) per entry.
#[must_use]
pub fn taskbar_hiding_net_wm_state_atoms() -> &'static [&'static str] {
    &["_NET_WM_STATE_SKIP_TASKBAR", "_NET_WM_STATE_SKIP_PAGER"]
}

/// When `true`, WM-state hints (taskbar/pager) are applied only after the overlay window is shown/mapped.
#[must_use]
pub const fn wm_state_hints_apply_after_show() -> bool {
    true
}

/// `_NET_WM_WINDOW_TYPE` for the transient overlay (`WindowAttributesExtX11`).
///
/// Must **not** include [`WindowType::Notification`] — WMs size notification clients as
/// small bubbles, which regressed full-monitor coverage (see quarter-screen issue note).
#[must_use]
pub fn overlay_x11_window_types() -> Vec<WindowType> {
    vec![WindowType::Normal]
}

/// When `true`, the wgpu surface is configured (or reconfigured) after `set_visible(true)` so
/// `inner_size` reflects borderless fullscreen on the primary monitor.
#[must_use]
pub const fn surface_configure_after_show() -> bool {
    true
}

/// When `true`, `OverlayApp` handles [`winit::event::WindowEvent::Resized`] and reconfigures the surface.
#[must_use]
pub const fn surface_reconfigure_on_resized_event() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::{
        overlay_x11_window_types, surface_configure_after_show,
        surface_reconfigure_on_resized_event, taskbar_hiding_net_wm_state_atoms,
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
    fn fullscreen_overlay_uses_normal_window_type() {
        let types = overlay_x11_window_types();
        assert!(
            !types.contains(&WindowType::Notification),
            "Notification window type causes quarter-screen draw on laptops; use Normal + SKIP_TASKBAR"
        );
        assert_eq!(
            types,
            vec![WindowType::Normal],
            "full-screen overlay must use Normal so WM honors borderless fullscreen sizing"
        );
    }

    #[test]
    fn surface_configure_after_show_for_fullscreen() {
        assert!(
            surface_configure_after_show(),
            "wgpu surface must be configured after set_visible so inner_size matches the monitor"
        );
    }

    #[test]
    fn surface_reconfigures_on_resized_event() {
        assert!(
            surface_reconfigure_on_resized_event(),
            "OverlayApp must handle Resized to pick up post-fullscreen geometry"
        );
    }
}
