//! winit-only overlay window placement (no wgpu) — for contract tests and shared setup.

use std::sync::Arc;

use power_shimmer_core::MonitorTarget;
use power_shimmer_overlay_contract::OverlayPlacement;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::monitor::MonitorHandle;
use winit::window::{Fullscreen, Window, WindowAttributes, WindowId, WindowLevel};

use super::placement;
use power_shimmer_core::OverlayError;

/// Picks the monitor for a full-screen overlay.
///
/// `primary_monitor()` is often `None` on `XWayland` even when displays exist; fall back
/// to the first reported monitor in that case.
fn select_target_monitor(
    event_loop: &ActiveEventLoop,
    target: MonitorTarget,
) -> Option<MonitorHandle> {
    let monitors: Vec<MonitorHandle> = event_loop.available_monitors().collect();
    let monitor_count = monitors.len();
    let primary_index = event_loop.primary_monitor().and_then(|primary| {
        monitors
            .iter()
            .position(|m| m.name() == primary.name() && m.size() == primary.size())
    });
    let index = placement::select_monitor_index_for_target(primary_index, monitor_count, target)?;
    monitors.into_iter().nth(index)
}

/// Returns true when an X11 session is required and appears available.
///
/// # Errors
///
/// Returns [`OverlayError::WindowCreationFailed`] when only Wayland is available or `DISPLAY` is unset.
pub fn require_x11_session() -> Result<(), OverlayError> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_none() {
        return Err(OverlayError::WindowCreationFailed(
            "Wayland session detected; v1 overlay requires X11 (set DISPLAY or use XWayland)"
                .to_string(),
        ));
    }
    if std::env::var_os("DISPLAY").is_none() {
        return Err(OverlayError::WindowCreationFailed(
            "DISPLAY is not set; cannot create X11 overlay".to_string(),
        ));
    }
    Ok(())
}

/// Builds production overlay [`WindowAttributes`] for borderless fullscreen on `monitor`.
#[must_use]
pub fn overlay_window_attributes(monitor: MonitorHandle) -> WindowAttributes {
    WindowAttributes::default()
        .with_title("Power Shimmer")
        .with_transparent(true)
        .with_decorations(false)
        .with_active(false)
        .with_visible(false)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_fullscreen(Some(Fullscreen::Borderless(Some(monitor))))
}

fn placement_from_window(
    window: &Window,
    monitor_size: (u32, u32),
    monitor_target: MonitorTarget,
) -> OverlayPlacement {
    let window_size = window.inner_size();
    OverlayPlacement {
        window_size: (window_size.width, window_size.height),
        monitor_size,
        monitor_target,
    }
}

/// Creates a borderless-fullscreen overlay window (no placement check).
fn create_overlay_window(
    event_loop: &ActiveEventLoop,
    monitor_target: MonitorTarget,
) -> Result<(Arc<Window>, (u32, u32), MonitorTarget), OverlayError> {
    placement::validate_monitor_target(monitor_target).map_err(OverlayError::WindowCreationFailed)?;

    let monitor = select_target_monitor(event_loop, monitor_target).ok_or_else(|| {
        OverlayError::WindowCreationFailed("no monitors available".to_string())
    })?;

    let monitor_size = monitor.size();
    let monitor_dims = (monitor_size.width, monitor_size.height);
    let window = event_loop
        .create_window(overlay_window_attributes(monitor))
        .map_err(|e| OverlayError::WindowCreationFailed(e.to_string()))?;

    Ok((Arc::new(window), monitor_dims, monitor_target))
}

/// Creates a fullscreen overlay window and returns its placement snapshot (no GPU init).
///
/// # Errors
///
/// Returns [`OverlayError::WindowCreationFailed`] when monitor selection, window creation,
/// or placement geometry checks fail.
pub fn prepare_overlay_window(
    event_loop: &ActiveEventLoop,
    monitor_target: MonitorTarget,
) -> Result<(Arc<Window>, OverlayPlacement), OverlayError> {
    let (window, monitor_dims, target) = create_overlay_window(event_loop, monitor_target)?;
    let placement = placement_from_window(&window, monitor_dims, target);

    if !placement::window_covers_monitor(placement.window_size, placement.monitor_size) {
        return Err(OverlayError::WindowCreationFailed(format!(
            "overlay window {:?} does not cover monitor {:?}",
            placement.window_size, placement.monitor_size
        )));
    }

    Ok((window, placement))
}

fn build_probe_event_loop() -> EventLoop<()> {
    let mut builder = EventLoop::<()>::with_user_event();
    #[cfg(target_os = "linux")]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
    }
    builder.build().expect("placement probe event loop")
}

struct PlacementProbeApp {
    result: Option<Result<OverlayPlacement, OverlayError>>,
    pending: Option<PendingProbe>,
}

struct PendingProbe {
    window: Arc<Window>,
    monitor_target: MonitorTarget,
    monitor_size: (u32, u32),
    attempts: u32,
}

const PROBE_MAX_ATTEMPTS: u32 = 50;

impl PlacementProbeApp {
    fn try_finish(&mut self, event_loop: &ActiveEventLoop) {
        let Some(pending) = &self.pending else {
            return;
        };
        if pending.attempts >= PROBE_MAX_ATTEMPTS {
            self.result = Some(Err(OverlayError::WindowCreationFailed(
                "timed out waiting for fullscreen window size".to_string(),
            )));
            self.pending = None;
            event_loop.exit();
            return;
        }

        let placement = placement_from_window(
            &pending.window,
            pending.monitor_size,
            pending.monitor_target,
        );

        if placement::window_covers_monitor(placement.window_size, placement.monitor_size) {
            self.result = Some(Ok(placement));
            self.pending = None;
            event_loop.exit();
        }
    }
}

impl ApplicationHandler for PlacementProbeApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.result.is_some() || self.pending.is_some() {
            return;
        }

        match create_overlay_window(event_loop, MonitorTarget::Primary) {
            Ok((window, monitor_size, monitor_target)) => {
                self.pending = Some(PendingProbe {
                    window,
                    monitor_target,
                    monitor_size,
                    attempts: 0,
                });
            }
            Err(e) => {
                self.result = Some(Err(e));
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        if matches!(event, WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }) {
            if let Some(pending) = &mut self.pending {
                pending.attempts += 1;
            }
            self.try_finish(event_loop);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(pending) = &mut self.pending {
            pending.attempts += 1;
            self.try_finish(event_loop);
        }
    }
}

/// Probes primary-monitor window placement using winit only (no wgpu / GPU adapter).
///
/// Polls until the window reports a size that covers the target monitor, so fullscreen
/// geometry applied asynchronously by the WM is observed.
///
/// # Errors
///
/// Same as [`require_x11_session`] and [`prepare_overlay_window`].
pub fn probe_primary_window_placement() -> Result<OverlayPlacement, OverlayError> {
    require_x11_session()?;
    let event_loop = build_probe_event_loop();
    let mut app = PlacementProbeApp {
        result: None,
        pending: None,
    };
    if let Err(err) = event_loop.run_app(&mut app) {
        return Err(OverlayError::WindowCreationFailed(err.to_string()));
    }
    app.result.unwrap_or_else(|| {
        Err(OverlayError::WindowCreationFailed(
            "placement probe did not run".to_string(),
        ))
    })
}

/// Linux adapter for [`power_shimmer_overlay_contract::OverlayWindowPlacementProbe`].
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxWindowPlacementProbe;

impl power_shimmer_overlay_contract::OverlayWindowPlacementProbe for LinuxWindowPlacementProbe {
    fn probe_primary_window_placement(&self) -> Result<OverlayPlacement, String> {
        probe_primary_window_placement().map_err(|e| e.to_string())
    }

    fn require_overlay_session(&self) -> Result<(), String> {
        require_x11_session().map_err(|e| e.to_string())
    }
}
