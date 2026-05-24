//! Overlay session lifecycle state (GPU-agnostic).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// Monotonic session identifier for cancel routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionId(u64);

impl SessionId {
    /// Allocates the next session id.
    pub fn next(counter: &AtomicU64) -> Self {
        Self(counter.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// Wraps an already-allocated session counter value.
    #[must_use]
    pub const fn from_raw(id: u64) -> Self {
        Self(id)
    }
}

/// High-level overlay session phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPhase {
    /// No active shimmer.
    Idle,
    /// Window/GPU setup or animation frames.
    Running,
    /// Releasing GPU/window (still counts as playing per SPEC).
    TearingDown,
}

/// Thread-safe playing flag + cancel routing for one session.
#[derive(Debug)]
pub struct SessionController {
    next_id: AtomicU64,
    active_id: AtomicU64,
    playing: Arc<AtomicBool>,
    cancel_requested: Arc<AtomicBool>,
    phase: Arc<std::sync::Mutex<SessionPhase>>,
}

impl SessionController {
    /// Creates a new controller with no active session.
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(0),
            active_id: AtomicU64::new(0),
            playing: Arc::new(AtomicBool::new(false)),
            cancel_requested: Arc::new(AtomicBool::new(false)),
            phase: Arc::new(std::sync::Mutex::new(SessionPhase::Idle)),
        }
    }

    /// Shared atomic used for [`OverlayRenderer::is_playing`](power_shimmer_core::ports::OverlayRenderer::is_playing).
    #[must_use]
    pub fn playing_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.playing)
    }

    /// Returns whether a shimmer is active (running or tearing down).
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::SeqCst)
    }

    /// Current lifecycle phase.
    #[must_use]
    pub fn phase(&self) -> SessionPhase {
        *self.phase.lock().expect("session phase mutex poisoned")
    }

    fn set_phase(&self, phase: SessionPhase) {
        *self.phase.lock().expect("session phase mutex poisoned") = phase;
    }

    /// Begins a new session; returns its id.
    pub fn begin_session(&self) -> SessionId {
        let id = SessionId::next(&self.next_id);
        self.active_id.store(id.0, Ordering::SeqCst);
        self.cancel_requested.store(false, Ordering::SeqCst);
        self.playing.store(true, Ordering::SeqCst);
        self.set_phase(SessionPhase::Running);
        id
    }

    /// Marks teardown; `is_playing` stays true until [`Self::finish_session`].
    pub fn begin_teardown(&self) {
        self.set_phase(SessionPhase::TearingDown);
    }

    /// Ends the session and clears playing.
    pub fn finish_session(&self) {
        self.set_phase(SessionPhase::Idle);
        self.active_id.store(0, Ordering::SeqCst);
        self.cancel_requested.store(false, Ordering::SeqCst);
        self.playing.store(false, Ordering::SeqCst);
    }

    /// Active session id, or `0` when idle.
    #[must_use]
    pub fn active_session_id(&self) -> u64 {
        self.active_id.load(Ordering::SeqCst)
    }

    /// Requests cancel for the given session (no-op if stale).
    pub fn request_cancel(&self, id: SessionId) -> bool {
        if self.active_id.load(Ordering::SeqCst) != id.0 {
            return false;
        }
        self.cancel_requested.store(true, Ordering::SeqCst);
        true
    }

    /// Cancel for the currently active session (if any).
    pub fn request_cancel_active(&self) -> bool {
        let id = self.active_session_id();
        if id == 0 {
            return false;
        }
        self.cancel_requested.store(true, Ordering::SeqCst);
        true
    }

    /// True when cancel was requested for `id`.
    #[must_use]
    pub fn is_cancelled(&self, id: SessionId) -> bool {
        self.active_id.load(Ordering::SeqCst) == id.0
            && self.cancel_requested.load(Ordering::SeqCst)
    }
}

impl Default for SessionController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_and_finish_clears_playing() {
        let ctrl = SessionController::new();
        assert!(!ctrl.is_playing());
        let id = ctrl.begin_session();
        assert!(ctrl.is_playing());
        assert_eq!(ctrl.phase(), SessionPhase::Running);
        assert!(ctrl.request_cancel(id));
        assert!(ctrl.is_cancelled(id));
        ctrl.begin_teardown();
        assert_eq!(ctrl.phase(), SessionPhase::TearingDown);
        assert!(ctrl.is_playing());
        ctrl.finish_session();
        assert!(!ctrl.is_playing());
        assert_eq!(ctrl.phase(), SessionPhase::Idle);
    }

    #[test]
    fn cancel_stale_session_is_no_op() {
        let ctrl = SessionController::new();
        let first = ctrl.begin_session();
        ctrl.finish_session();
        assert!(!ctrl.request_cancel(first));
    }

    #[test]
    fn cancel_when_idle_is_no_op() {
        let ctrl = SessionController::new();
        assert!(!ctrl.request_cancel_active());
        assert!(!ctrl.is_playing());
    }
}
