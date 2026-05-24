//! Test double for [`OverlayRenderer`](crate::ports::OverlayRenderer).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::domain::{OverlayError, ShimmerRequest};
use crate::ports::OverlayRenderer;

const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Records play invocations and simulates async completion after [`Self::with_delay`].
#[derive(Debug, Clone)]
pub struct MockOverlayRenderer {
    pub play_calls: Arc<Mutex<Vec<ShimmerRequest>>>,
    delay: Duration,
    playing: Arc<AtomicBool>,
    cancel_requested: Arc<AtomicBool>,
}

impl MockOverlayRenderer {
    /// Creates a mock that completes immediately after each `play` call.
    #[must_use]
    pub fn new() -> Self {
        Self::with_delay(Duration::ZERO)
    }

    /// Creates a mock that holds `is_playing` true for `delay` before completing.
    #[must_use]
    pub fn with_delay(delay: Duration) -> Self {
        Self {
            play_calls: Arc::new(Mutex::new(Vec::new())),
            delay,
            playing: Arc::new(AtomicBool::new(false)),
            cancel_requested: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Clears cancel/playing state between tests.
    pub fn reset(&self) {
        self.cancel_requested.store(false, Ordering::SeqCst);
        self.playing.store(false, Ordering::SeqCst);
        self.play_calls.lock().unwrap().clear();
    }
}

impl Default for MockOverlayRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayRenderer for MockOverlayRenderer {
    async fn play(&self, request: ShimmerRequest) -> Result<(), OverlayError> {
        self.play_calls.lock().unwrap().push(request);
        self.cancel_requested.store(false, Ordering::SeqCst);
        self.playing.store(true, Ordering::SeqCst);

        let mut elapsed = Duration::ZERO;
        while elapsed < self.delay {
            if self.cancel_requested.load(Ordering::SeqCst) {
                self.playing.store(false, Ordering::SeqCst);
                return Err(OverlayError::Cancelled);
            }
            let step = POLL_INTERVAL.min(self.delay.saturating_sub(elapsed));
            // Std sleep keeps `core` free of a runtime dependency; acceptable for test doubles.
            std::thread::sleep(step);
            elapsed += step;
        }

        if self.cancel_requested.load(Ordering::SeqCst) {
            self.playing.store(false, Ordering::SeqCst);
            return Err(OverlayError::Cancelled);
        }

        self.playing.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn is_playing(&self) -> bool {
        self.playing.load(Ordering::SeqCst)
    }

    fn cancel(&self) {
        self.cancel_requested.store(true, Ordering::SeqCst);
        self.playing.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use crate::domain::{OverlayError, ShimmerConfig, ShimmerRequest, ShimmerTrigger};

    use super::*;

    fn sample_request() -> ShimmerRequest {
        ShimmerRequest {
            config: ShimmerConfig::default(),
            trigger: ShimmerTrigger::Manual,
        }
    }

    /// Runs `play` on a dedicated thread so mock `thread::sleep` does not block the test runtime.
    fn spawn_play(
        overlay: Arc<MockOverlayRenderer>,
        request: ShimmerRequest,
    ) -> std::thread::JoinHandle<Result<(), OverlayError>> {
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .expect("runtime");
            runtime.block_on(overlay.play(request))
        })
    }

    #[test]
    fn play_records_request_and_clears_playing_on_completion() {
        let overlay = Arc::new(MockOverlayRenderer::new());
        let request = sample_request();

        assert!(!overlay.is_playing());
        spawn_play(Arc::clone(&overlay), request.clone())
            .join()
            .expect("join")
            .expect("play should succeed");
        assert!(!overlay.is_playing());

        let calls = overlay.play_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], request);
    }

    #[test]
    fn play_sets_is_playing_until_completion() {
        let overlay = Arc::new(MockOverlayRenderer::with_delay(Duration::from_millis(50)));

        let handle = spawn_play(Arc::clone(&overlay), sample_request());

        std::thread::sleep(Duration::from_millis(10));
        assert!(overlay.is_playing());

        handle.join().expect("join").expect("play should succeed");
        assert!(!overlay.is_playing());
    }

    #[test]
    fn cancel_during_play_returns_cancelled() {
        let overlay = Arc::new(MockOverlayRenderer::with_delay(Duration::from_millis(200)));

        let handle = spawn_play(Arc::clone(&overlay), sample_request());

        std::thread::sleep(Duration::from_millis(20));
        overlay.cancel();

        let result = handle.join().expect("join");
        assert!(matches!(result, Err(OverlayError::Cancelled)));
        assert!(!overlay.is_playing());
    }

    #[test]
    fn cancel_when_idle_is_no_op() {
        let overlay = MockOverlayRenderer::new();
        overlay.cancel();
        assert!(!overlay.is_playing());
    }
}
