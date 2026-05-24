//! X11 + wgpu implementation of [`OverlayRenderer`](power_shimmer_core::ports::OverlayRenderer).

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use power_shimmer_core::ports::OverlayRenderer;
use power_shimmer_core::{OverlayError, ShimmerRequest};
use tokio::sync::oneshot;
use tracing::error;
use winit::event_loop::EventLoopProxy;

use super::render_loop::{require_x11_session, OverlayApp, OverlayUserEvent};
use super::session::{SessionController, SessionId};

const PLAY_TIMEOUT: Duration = Duration::from_secs(15);
const TEARDOWN_GRACE: Duration = Duration::from_millis(500);

fn build_overlay_event_loop() -> winit::event_loop::EventLoop<OverlayUserEvent> {
    use winit::event_loop::EventLoop;

    let mut builder = EventLoop::<OverlayUserEvent>::with_user_event();
    #[cfg(target_os = "linux")]
    enable_event_loop_on_worker_thread(&mut builder);
    builder.build().expect("overlay event loop")
}

#[cfg(target_os = "linux")]
fn enable_event_loop_on_worker_thread(
    builder: &mut winit::event_loop::EventLoopBuilder<OverlayUserEvent>,
) {
    use winit::platform::x11::EventLoopBuilderExtX11;
    EventLoopBuilderExtX11::with_any_thread(builder, true);
}

/// Linux overlay renderer using winit + wgpu on X11.
pub struct WgpuShimmerRenderer {
    controller: Arc<SessionController>,
    proxy: EventLoopProxy<OverlayUserEvent>,
}

impl WgpuShimmerRenderer {
    /// Spawns the dedicated overlay thread and returns a renderer handle.
    ///
    /// # Panics
    ///
    /// Panics if the overlay thread fails to start or signal readiness.
    #[must_use]
    pub fn new() -> Self {
        let controller = Arc::new(SessionController::new());
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<EventLoopProxy<OverlayUserEvent>>(1);
        let controller_thread = Arc::clone(&controller);

        thread::Builder::new()
            .name("power-shimmer-overlay".into())
            .spawn(move || {
                let event_loop = build_overlay_event_loop();
                let proxy = event_loop.create_proxy();
                ready_tx
                    .send(proxy.clone())
                    .expect("overlay ready signal");
                let mut app = OverlayApp::new(controller_thread, proxy);
                event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
                if let Err(err) = event_loop.run_app(&mut app) {
                    error!(%err, "overlay event loop exited with error");
                }
            })
            .expect("spawn overlay thread");

        let proxy = ready_rx.recv().expect("overlay thread ready");
        Self { controller, proxy }
    }

    /// Shared session controller (tests).
    #[must_use]
    pub fn session_controller(&self) -> Arc<SessionController> {
        Arc::clone(&self.controller)
    }
}

impl Default for WgpuShimmerRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WgpuShimmerRenderer {
    fn drop(&mut self) {
        let _ = self
            .proxy
            .send_event(OverlayUserEvent::Shutdown);
    }
}

impl OverlayRenderer for WgpuShimmerRenderer {
    async fn play(&self, request: ShimmerRequest) -> Result<(), OverlayError> {
        require_x11_session()?;

        let wait_deadline = std::time::Instant::now() + TEARDOWN_GRACE;
        while self.controller.is_playing() && std::time::Instant::now() < wait_deadline {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        if self.controller.is_playing() {
            return Err(OverlayError::RenderFailed(
                "overlay still busy after teardown grace".to_string(),
            ));
        }

        let session_id = self.controller.begin_session();
        let (done_tx, done_rx) = oneshot::channel();

        if self
            .proxy
            .send_event(OverlayUserEvent::Play {
                request,
                session_id,
                done: done_tx,
            })
            .is_err()
        {
            self.controller.finish_session();
            return Err(OverlayError::RenderFailed(
                "overlay event loop not running".to_string(),
            ));
        }

        match tokio::time::timeout(PLAY_TIMEOUT, done_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                self.controller.finish_session();
                Err(OverlayError::RenderFailed(
                    "overlay result channel closed".to_string(),
                ))
            }
            Err(_) => {
                self.cancel();
                let _ = tokio::time::timeout(TEARDOWN_GRACE, async {
                    while self.controller.is_playing() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await;
                Err(OverlayError::RenderFailed("overlay play timed out".to_string()))
            }
        }
    }

    fn is_playing(&self) -> bool {
        self.controller.is_playing()
    }

    fn cancel(&self) {
        let id = self.controller.active_session_id();
        if id == 0 {
            return;
        }
        self.controller.request_cancel_active();
        let _ = self.proxy.send_event(OverlayUserEvent::Cancel {
            session_id: SessionId::from_raw(id),
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use power_shimmer_core::{OverlayError, ShimmerConfig, ShimmerRequest, ShimmerTrigger};
    use super::*;
    use crate::overlay::render_loop::require_x11_session;

    fn sample_request() -> ShimmerRequest {
        ShimmerRequest {
            config: ShimmerConfig {
                duration_ms: 50,
                ..ShimmerConfig::default()
            },
            trigger: ShimmerTrigger::Manual,
        }
    }

    #[test]
    fn cancel_when_idle_is_no_op() {
        let renderer = WgpuShimmerRenderer::new();
        renderer.cancel();
        assert!(!renderer.is_playing());
    }

    /// Exercises port semantics when an X11 display is available.
    #[tokio::test]
    #[ignore = "requires X11 DISPLAY and GPU"]
    async fn play_sets_is_playing_until_complete() {
        if require_x11_session().is_err() {
            return;
        }

        let renderer = Arc::new(WgpuShimmerRenderer::new());
        let overlay = Arc::clone(&renderer);
        let request = sample_request();
        let handle = tokio::spawn(async move { overlay.play(request).await });

        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(renderer.is_playing());

        let result = handle.await.expect("join");
        result.expect("play ok");
        assert!(!renderer.is_playing());
    }

    #[tokio::test]
    #[ignore = "requires X11 DISPLAY and GPU"]
    async fn cancel_during_play_returns_cancelled() {
        if require_x11_session().is_err() {
            return;
        }

        let renderer = Arc::new(WgpuShimmerRenderer::new());
        let mut config = ShimmerConfig::default();
        config.duration_ms = 5_000;
        let request = ShimmerRequest {
            config,
            trigger: ShimmerTrigger::Manual,
        };

        let overlay = Arc::clone(&renderer);
        let handle = tokio::spawn(async move { overlay.play(request).await });

        tokio::time::sleep(Duration::from_millis(80)).await;
        renderer.cancel();

        let result = handle.await.expect("join");
        assert!(matches!(result, Err(OverlayError::Cancelled)));
        assert!(!renderer.is_playing());
    }
}
