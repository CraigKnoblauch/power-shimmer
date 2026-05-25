//! Testable wiring contract between [`super::render_loop`] and [`super::overlay_hint_policy`].
//!
//! See `notes/issues/overlay-quarter-screen-regression.md`.

use super::overlay_hint_policy;

/// Whether `start_session` configures the wgpu surface only before `set_visible(true)`.
///
/// Returns `false` when the surface is also configured/reconfigured after show per
/// [`overlay_hint_policy::surface_configure_after_show`].
#[must_use]
#[allow(dead_code)] // asserted in unit tests; render_loop follows `overlay_hint_policy` directly
pub const fn surface_configure_only_before_show() -> bool {
    !overlay_hint_policy::surface_configure_after_show()
}

/// Whether `OverlayApp::window_event` handles [`winit::event::WindowEvent::Resized`].
#[must_use]
#[allow(dead_code)]
pub const fn handles_resized_window_event() -> bool {
    overlay_hint_policy::surface_reconfigure_on_resized_event()
}

#[cfg(test)]
mod tests {
    use super::{handles_resized_window_event, surface_configure_only_before_show};
    use crate::overlay::overlay_hint_policy;

    #[test]
    fn start_session_configures_surface_after_show_when_policy_requires() {
        assert!(
            overlay_hint_policy::surface_configure_after_show(),
            "policy requires post-show surface configure"
        );
        assert!(
            !surface_configure_only_before_show(),
            "render_loop must not leave surface at pre-map size (800x600 / quarter-screen)"
        );
    }

    #[test]
    fn window_event_handles_resized_when_policy_requires() {
        assert!(overlay_hint_policy::surface_reconfigure_on_resized_event());
        assert!(
            handles_resized_window_event(),
            "render_loop must handle WindowEvent::Resized"
        );
    }
}
