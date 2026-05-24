//! winit event loop + wgpu frame loop for one overlay session.

use std::sync::Arc;
use std::time::{Duration, Instant};

use power_shimmer_core::{OverlayError, ShimmerRequest};
use wgpu::{Instance, Surface};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::{Fullscreen, Window, WindowAttributes, WindowId, WindowLevel};

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
    pub fn new(controller: Arc<SessionController>, proxy: EventLoopProxy<OverlayUserEvent>) -> Self {
        Self {
            controller,
            proxy,
            gpu: None,
        }
    }
}

impl OverlayApp {
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
            let _ = done.send(Err(OverlayError::RenderFailed(
                "overlay busy".to_string(),
            )));
            return;
        }

        let Some(monitor) = event_loop.primary_monitor() else {
            self.controller.finish_session();
            let _ = done.send(Err(OverlayError::WindowCreationFailed(
                "no primary monitor".to_string(),
            )));
            return;
        };

        let attrs = WindowAttributes::default()
            .with_title("Power Shimmer")
            .with_transparent(true)
            .with_decorations(false)
            .with_active(false)
            .with_visible(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_fullscreen(Some(Fullscreen::Borderless(Some(monitor))));

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.controller.finish_session();
                let _ = done.send(Err(OverlayError::WindowCreationFailed(e.to_string())));
                return;
            }
        };

        x11_click_through::apply_x11_overlay_hints_best_effort(&window);

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

        let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })) else {
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

        let size = window.inner_size();
        let mut config = shader::surface_config(size.width, size.height, format);
        if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied) {
            config.alpha_mode = wgpu::CompositeAlphaMode::PreMultiplied;
        } else if let Some(&mode) = caps.alpha_modes.first() {
            config.alpha_mode = mode;
        }
        surface.configure(&device, &config);

        let pipeline = ShimmerPipeline::new(&device, format, shader::SHADER_SOURCE);

        window.set_visible(true);
        window.request_redraw();

        self.gpu = Some(GpuSession {
            window,
            surface,
            config,
            device,
            queue,
            pipeline,
            request,
            session_id,
            started: Instant::now(),
            done: Some(done),
        });
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

        let size = session.window.inner_size();
        if size.width > 0 && size.height > 0
            && (session.config.width != size.width || session.config.height != size.height)
        {
            session.config.width = size.width;
            session.config.height = size.height;
            session.surface.configure(&session.device, &session.config);
        }

        let params = ShimmerParams::from_config(&session.request.config, elapsed.as_secs_f32());
        session.pipeline.write_uniforms(&session.queue, &params);

        let frame = match session.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                session
                    .surface
                    .configure(&session.device, &session.config);
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
        let mut encoder =
            session
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
