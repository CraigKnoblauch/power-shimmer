//! winit event loop + wgpu frame loop for one overlay session.

use std::sync::Arc;
use std::time::{Duration, Instant};

use power_shimmer_core::{OverlayError, ShimmerRequest};
use tracing::debug;
use wgpu::{Instance, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::monitor::MonitorHandle;
use winit::platform::x11::WindowAttributesExtX11;
use winit::window::{Fullscreen, Window, WindowAttributes, WindowId, WindowLevel};

use super::overlay_hint_policy::{
    overlay_x11_window_types, surface_configure_after_show,
    surface_reconfigure_on_resized_event, wm_state_hints_apply_after_show,
};
use super::session::{SessionController, SessionId};
use super::shader::{self, ShimmerParams, ShimmerPipeline};
use super::x11_click_through;

const FRAME_BUDGET: Duration = Duration::from_millis(16);
/// User events delivered to the overlay thread's winit loop.
pub enum OverlayUserEvent {
    /// Start a shimmer session.
    Play {
        request: ShimmerRequest,
        session_id: SessionId,
        done: tokio::sync::oneshot::Sender<Result<(), OverlayError>>,
    },
    /// Cancel the active session.
    Cancel { session_id: SessionId },
    /// Stop the event loop (tests/shutdown).
    Shutdown,
}

/// GPU + window state for an in-flight session.
struct GpuSession {
    window: Arc<Window>,
    surface: Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: ShimmerPipeline,
    /// Primary monitor physical size — fallback when pre-map `inner_size` is still small.
    monitor_size: PhysicalSize<u32>,
    request: ShimmerRequest,
    session_id: SessionId,
    started: Instant,
    done: Option<tokio::sync::oneshot::Sender<Result<(), OverlayError>>>,
}

/// winit application driving overlay sessions on a dedicated thread.
pub struct OverlayApp {
    controller: Arc<SessionController>,
    #[allow(dead_code)]
    proxy: EventLoopProxy<OverlayUserEvent>,
    gpu: Option<GpuSession>,
}

impl OverlayApp {
    /// Creates the app state used by [`run_overlay_event_loop`].
    #[must_use]
    pub fn new(
        controller: Arc<SessionController>,
        proxy: EventLoopProxy<OverlayUserEvent>,
    ) -> Self {
        Self {
            controller,
            proxy,
            gpu: None,
        }
    }
}

impl OverlayApp {
    fn reconfigure_surface(session: &mut GpuSession) {
        let (width, height) =
            effective_surface_size(&session.window, session.monitor_size);
        if width == 0 || height == 0 {
            return;
        }
        if session.config.width == width && session.config.height == height {
            return;
        }
        session.config.width = width;
        session.config.height = height;
        session
            .surface
            .configure(&session.device, &session.config);
        debug!(
            width,
            height,
            inner_width = session.window.inner_size().width,
            inner_height = session.window.inner_size().height,
            monitor_width = session.monitor_size.width,
            monitor_height = session.monitor_size.height,
            "overlay surface reconfigured"
        );
    }

    fn finish_session(&mut self, result: Result<(), OverlayError>) {
        let Some(mut session) = self.gpu.take() else {
            self.controller.finish_session();
            return;
        };

        session.window.set_visible(false);
        let done = session.done.take();
        self.controller.begin_teardown();
        drop(session);
        self.controller.finish_session();
        if let Some(done) = done {
            let _ = done.send(result);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn start_session(
        &mut self,
        event_loop: &ActiveEventLoop,
        request: ShimmerRequest,
        session_id: SessionId,
        done: tokio::sync::oneshot::Sender<Result<(), OverlayError>>,
    ) {
        if self.gpu.is_some() {
            let _ = done.send(Err(OverlayError::RenderFailed("overlay busy".to_string())));
            return;
        }

        let Some(monitor) = select_target_monitor(event_loop) else {
            self.controller.finish_session();
            let _ = done.send(Err(OverlayError::WindowCreationFailed(
                "no monitors available".to_string(),
            )));
            return;
        };
        let monitor_size = monitor.size();

        let attrs = WindowAttributes::default()
            .with_title("")
            .with_transparent(true)
            .with_decorations(false)
            .with_active(false)
            .with_visible(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_x11_window_type(overlay_x11_window_types())
            .with_fullscreen(Some(Fullscreen::Borderless(Some(monitor))));

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.controller.finish_session();
                let _ = done.send(Err(OverlayError::WindowCreationFailed(e.to_string())));
                return;
            }
        };

        x11_click_through::apply_x11_click_through_hints_best_effort(&window);

        let instance = Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        let surface = match instance.create_surface(Arc::clone(&window)) {
            Ok(s) => s,
            Err(e) => {
                self.controller.finish_session();
                let _ = done.send(Err(OverlayError::WindowCreationFailed(e.to_string())));
                return;
            }
        };

        let Some(adapter) =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            }))
        else {
            self.controller.finish_session();
            let _ = done.send(Err(OverlayError::RenderFailed(
                "no suitable GPU adapter".to_string(),
            )));
            return;
        };

        let (device, queue) = match pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("shimmer_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        )) {
            Ok(pair) => pair,
            Err(e) => {
                self.controller.finish_session();
                let _ = done.send(Err(OverlayError::RenderFailed(e.to_string())));
                return;
            }
        };

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(shader::surface_format());

        let (width, height) = effective_surface_size(&window, monitor_size);
        let mut config = shader::surface_config(width, height, format);
        if caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        {
            config.alpha_mode = wgpu::CompositeAlphaMode::PreMultiplied;
        } else if let Some(&mode) = caps.alpha_modes.first() {
            config.alpha_mode = mode;
        }

        if !surface_configure_after_show() {
            surface.configure(&device, &config);
        }

        let pipeline = ShimmerPipeline::new(&device, format, shader::SHADER_SOURCE);

        window.set_visible(true);
        if wm_state_hints_apply_after_show() {
            x11_click_through::apply_taskbar_hiding_wm_state_best_effort(&window);
        }

        let mut session = GpuSession {
            window,
            surface,
            config,
            device,
            queue,
            pipeline,
            monitor_size,
            request,
            session_id,
            started: Instant::now(),
            done: Some(done),
        };

        if surface_configure_after_show() {
            Self::reconfigure_surface(&mut session);
        }

        session.window.request_redraw();
        self.gpu = Some(session);
    }

    fn render_frame(&mut self, _event_loop: &ActiveEventLoop) {
        let Some(session) = self.gpu.as_mut() else {
            return;
        };

        if self.controller.is_cancelled(session.session_id) {
            self.finish_session(Err(OverlayError::Cancelled));
            return;
        }

        let elapsed = session.started.elapsed();
        let duration = Duration::from_millis(u64::from(session.request.config.duration_ms));
        if elapsed >= duration.saturating_sub(FRAME_BUDGET) {
            self.finish_session(Ok(()));
            return;
        }

        Self::reconfigure_surface(session);

        let params = ShimmerParams::from_config(&session.request.config, elapsed.as_secs_f32());
        session.pipeline.write_uniforms(&session.queue, &params);

        let frame = match session.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                session.surface.configure(&session.device, &session.config);
                return;
            }
            Err(e) => {
                self.finish_session(Err(OverlayError::RenderFailed(e.to_string())));
                return;
            }
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = session
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("shimmer_encoder"),
            });
        session.pipeline.render(&mut encoder, &view);
        session.queue.submit(Some(encoder.finish()));
        frame.present();

        session.window.request_redraw();
    }
}

impl ApplicationHandler<OverlayUserEvent> for OverlayApp {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: OverlayUserEvent) {
        match event {
            OverlayUserEvent::Play {
                request,
                session_id,
                done,
            } => self.start_session(event_loop, request, session_id, done),
            OverlayUserEvent::Cancel { session_id } => {
                if self.controller.request_cancel(session_id)
                    && self
                        .gpu
                        .as_ref()
                        .is_some_and(|s| s.session_id == session_id)
                {
                    self.finish_session(Err(OverlayError::Cancelled));
                }
            }
            OverlayUserEvent::Shutdown => {
                if self.gpu.is_some() {
                    self.finish_session(Err(OverlayError::Cancelled));
                }
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(session) = self.gpu.as_ref() else {
            return;
        };
        if session.window.id() != window_id {
            return;
        }

        match event {
            WindowEvent::RedrawRequested => self.render_frame(event_loop),
            WindowEvent::Resized(size) if surface_reconfigure_on_resized_event() => {
                if let Some(session) = self.gpu.as_mut() {
                    if size.width > 0 && size.height > 0 {
                        session.config.width = size.width;
                        session.config.height = size.height;
                        session
                            .surface
                            .configure(&session.device, &session.config);
                        debug!(
                            width = size.width,
                            height = size.height,
                            "overlay surface reconfigured (Resized)"
                        );
                    }
                }
            }
            WindowEvent::CloseRequested => self.finish_session(Err(OverlayError::Cancelled)),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(session) = self.gpu.as_ref() {
            session.window.request_redraw();
        }
    }
}

/// Picks the monitor for a full-screen overlay.
///
/// `primary_monitor()` is often `None` on XWayland even when displays exist; fall back
/// to the first reported monitor in that case.
fn select_target_monitor(event_loop: &ActiveEventLoop) -> Option<MonitorHandle> {
    event_loop
        .primary_monitor()
        .or_else(|| event_loop.available_monitors().next())
}

/// Picks surface dimensions: max of window `inner_size` and monitor size so pre-map defaults
/// (e.g. winit 800×600) do not leave a quarter-screen buffer after borderless fullscreen.
fn effective_surface_size(window: &Window, monitor_size: PhysicalSize<u32>) -> (u32, u32) {
    let inner = window.inner_size();
    (
        inner.width.max(monitor_size.width),
        inner.height.max(monitor_size.height),
    )
}

#[cfg(test)]
mod tests {
    use winit::dpi::PhysicalSize;

    /// Mirrors [`effective_surface_size`] — pre-map 800×600 must expand to monitor bounds.
    #[test]
    fn effective_surface_size_uses_monitor_when_inner_is_default_small() {
        let inner = PhysicalSize::new(800, 600);
        let monitor = PhysicalSize::new(1920, 1080);
        assert_eq!(
            (
                inner.width.max(monitor.width),
                inner.height.max(monitor.height),
            ),
            (1920, 1080)
        );
    }
}

/// Returns true when an X11 session is required and appears available.
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
