//! X11 input shape and EWMH hints for click-through overlay windows.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tracing::warn;
use winit::window::Window;
use x11rb::connection::Connection;
use x11rb::protocol::shape::{ConnectionExt as ShapeConnectionExt, SK, SO};
use x11rb::protocol::xproto::{self, ClipOrdering, ConnectionExt as XprotoConnectionExt};
use x11rb::rust_connection::RustConnection;

/// Applies click-through input shape and skip-taskbar hints (best effort).
///
/// # Errors
///
/// Returns an error when the window is not X11 or the X11 connection/protocol calls fail.
pub fn apply_x11_overlay_hints(window: &Window) -> Result<(), String> {
    let handle = window
        .window_handle()
        .map_err(|e| format!("window handle: {e}"))?;
    let RawWindowHandle::Xlib(xlib) = handle.as_raw() else {
        return Err("not an X11 window".to_string());
    };
    let x11_window = u32::try_from(xlib.window).map_err(|e| e.to_string())?;

    let (conn, screen_num) = RustConnection::connect(None).map_err(|e| e.to_string())?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    set_empty_input_shape(&conn, x11_window)?;
    set_skip_taskbar(&conn, root, x11_window)?;

    Ok(())
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

fn set_skip_taskbar(conn: &RustConnection, root: u32, window: u32) -> Result<(), String> {
    let net_wm_state = conn
        .intern_atom(false, b"_NET_WM_STATE")
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?
        .atom;
    let skip_taskbar = conn
        .intern_atom(false, b"_NET_WM_STATE_SKIP_TASKBAR")
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?
        .atom;

    // _NET_WM_STATE_ADD = 1
    let data = [1u32, skip_taskbar, 0, 0, 0];
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

/// Applies hints and logs warnings instead of failing the session.
pub fn apply_x11_overlay_hints_best_effort(window: &Window) {
    if let Err(error) = apply_x11_overlay_hints(window) {
        warn!(%error, "X11 overlay hints failed (click-through may be incomplete)");
    }
}
