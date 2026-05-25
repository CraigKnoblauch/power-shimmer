//! X11 input shape and EWMH hints for click-through overlay windows.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tracing::{debug, warn};
use winit::window::Window;
use x11rb::connection::Connection;
use x11rb::protocol::shape::{ConnectionExt as ShapeConnectionExt, SK, SO};
use x11rb::protocol::xproto::{self, ClipOrdering, ConnectionExt as XprotoConnectionExt};
use x11rb::rust_connection::RustConnection;

use super::overlay_hint_policy::taskbar_hiding_net_wm_state_atoms;

/// Applies click-through input shape only (safe before the window is mapped).
///
/// # Errors
///
/// Returns an error when the window is not X11 or the Shape extension call fails.
pub fn apply_x11_click_through_hints(window: &Window) -> Result<(), String> {
    let x11_window = x11_window_id(window)?;
    let conn = connect_x11()?;
    set_empty_input_shape(&conn, x11_window)?;
    debug!("X11 overlay click-through input shape applied");
    Ok(())
}

/// Applies `_NET_WM_STATE` hints that hide the overlay from taskbar and pager.
///
/// Prefer calling after the window is shown/mapped (see [`super::overlay_hint_policy::wm_state_hints_apply_after_show`]).
///
/// # Errors
///
/// Returns an error when the window is not X11 or protocol calls fail.
pub fn apply_taskbar_hiding_wm_state(window: &Window) -> Result<(), String> {
    let x11_window = x11_window_id(window)?;
    let (conn, root) = connect_x11_with_root()?;
    apply_taskbar_hiding_wm_state_on_connection(&conn, root, x11_window)?;
    debug!(
        atoms = ?taskbar_hiding_net_wm_state_atoms(),
        "X11 overlay taskbar/pager WM state hints applied"
    );
    Ok(())
}

/// Applies click-through shape and WM-state hints in one call (pre-map friendly for tests only).
///
/// Production code should call [`apply_x11_click_through_hints`] before show and
/// [`apply_taskbar_hiding_wm_state`] after show.
///
/// # Errors
///
/// Returns an error when the window is not X11 or any hint step fails.
pub fn apply_x11_overlay_hints(window: &Window) -> Result<(), String> {
    apply_x11_click_through_hints(window)?;
    apply_taskbar_hiding_wm_state(window)?;
    Ok(())
}

fn x11_window_id(window: &Window) -> Result<u32, String> {
    let handle = window
        .window_handle()
        .map_err(|e| format!("window handle: {e}"))?;
    let RawWindowHandle::Xlib(xlib) = handle.as_raw() else {
        return Err("not an X11 window".to_string());
    };
    u32::try_from(xlib.window).map_err(|e| e.to_string())
}

fn connect_x11() -> Result<RustConnection, String> {
    RustConnection::connect(None)
        .map(|(conn, _)| conn)
        .map_err(|e| e.to_string())
}

fn connect_x11_with_root() -> Result<(RustConnection, u32), String> {
    let (conn, screen_num) = RustConnection::connect(None).map_err(|e| e.to_string())?;
    let root = conn.setup().roots[screen_num].root;
    Ok((conn, root))
}

fn set_empty_input_shape(conn: &RustConnection, window: u32) -> Result<(), String> {
    conn.shape_rectangles(
        SO::SET,
        SK::INPUT,
        ClipOrdering::UNSORTED,
        window,
        0,
        0,
        &[],
    )
    .map_err(|e| e.to_string())?;
    conn.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn apply_taskbar_hiding_wm_state_on_connection(
    conn: &RustConnection,
    root: u32,
    window: u32,
) -> Result<(), String> {
    let net_wm_state = conn
        .intern_atom(false, b"_NET_WM_STATE")
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?
        .atom;

    for atom_name in taskbar_hiding_net_wm_state_atoms() {
        let property_atom = conn
            .intern_atom(false, atom_name.as_bytes())
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?
            .atom;
        send_net_wm_state_add(conn, root, window, net_wm_state, property_atom)?;
    }

    Ok(())
}

fn send_net_wm_state_add(
    conn: &RustConnection,
    root: u32,
    window: u32,
    net_wm_state: u32,
    property_atom: u32,
) -> Result<(), String> {
    // _NET_WM_STATE_ADD = 1
    let data = [1u32, property_atom, 0, 0, 0];
    let event = xproto::ClientMessageEvent {
        response_type: xproto::CLIENT_MESSAGE_EVENT,
        format: 32,
        sequence: 0,
        window,
        type_: net_wm_state,
        data: xproto::ClientMessageData::from(data),
    };

    conn.send_event(
        false,
        root,
        xproto::EventMask::SUBSTRUCTURE_NOTIFY | xproto::EventMask::SUBSTRUCTURE_REDIRECT,
        event,
    )
    .map_err(|e| e.to_string())?;
    conn.flush().map_err(|e| e.to_string())?;
    Ok(())
}

/// Applies click-through hints and logs warnings instead of failing the session.
pub fn apply_x11_click_through_hints_best_effort(window: &Window) {
    if let Err(error) = apply_x11_click_through_hints(window) {
        warn!(%error, "X11 click-through hints failed");
    }
}

/// Applies taskbar/pager WM-state hints and logs warnings instead of failing the session.
pub fn apply_taskbar_hiding_wm_state_best_effort(window: &Window) {
    if let Err(error) = apply_taskbar_hiding_wm_state(window) {
        warn!(%error, "X11 taskbar/pager WM state hints failed");
    }
}

/// Applies all overlay hints and logs warnings instead of failing the session.
pub fn apply_x11_overlay_hints_best_effort(window: &Window) {
    apply_x11_click_through_hints_best_effort(window);
    apply_taskbar_hiding_wm_state_best_effort(window);
}

#[cfg(test)]
mod tests {
    use crate::overlay::overlay_hint_policy::taskbar_hiding_net_wm_state_atoms;

    /// Each policy atom must correspond to one `_NET_WM_STATE` ADD in
    /// [`apply_taskbar_hiding_wm_state_on_connection`].
    #[test]
    fn apply_taskbar_hiding_wm_state_uses_all_policy_atoms() {
        assert_eq!(
            taskbar_hiding_wm_state_message_count(),
            taskbar_hiding_net_wm_state_atoms().len(),
            "apply_taskbar_hiding_wm_state must send one _NET_WM_STATE ADD per policy atom"
        );
    }

    #[must_use]
    fn taskbar_hiding_wm_state_message_count() -> usize {
        taskbar_hiding_net_wm_state_atoms().len()
    }
}
