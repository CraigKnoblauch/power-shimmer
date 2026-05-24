//! Power event listener port — see SPEC.md Module 2.

use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::domain::{PowerEvent, PowerListenerError};

/// Async power event source. Implemented by platform adapters only.
pub trait PowerEventListener: Send + Sync {
    /// Begin emitting power events.
    ///
    /// Returns a fallible stream of [`PowerEvent`]. Implementations must:
    /// 1. Query current power source.
    /// 2. Emit [`PowerEvent::InitialState`].
    /// 3. Emit [`PowerEvent::Transition`] on each subsequent change.
    ///
    /// # Errors
    ///
    /// Returns [`PowerListenerError::SubscribeFailed`] when the adapter cannot
    /// begin the OS subscription.
    fn subscribe(&self) -> Result<PowerEventStream, PowerListenerError>;
}

/// Result of a timed receive on [`PowerEventStream`].
#[derive(Debug)]
pub enum StreamRecvResult {
    /// An event (or adapter error) was received.
    Message(Result<PowerEvent, PowerListenerError>),
    /// No event arrived before the timeout elapsed.
    Timeout,
    /// The sender disconnected and the stream will deliver no further events.
    Disconnected,
}

/// Subscription handle; dropping cancels the OS subscription.
pub struct PowerEventStream {
    receiver: mpsc::Receiver<Result<PowerEvent, PowerListenerError>>,
    _worker: Option<JoinHandle<()>>,
}

impl PowerEventStream {
    /// Constructs a stream from an adapter-owned channel and background worker.
    ///
    /// Intended for platform adapters only.
    pub fn new(
        receiver: mpsc::Receiver<Result<PowerEvent, PowerListenerError>>,
        worker: JoinHandle<()>,
    ) -> Self {
        Self {
            receiver,
            _worker: Some(worker),
        }
    }

    /// Receives the next event, blocking until one arrives or the stream ends.
    #[must_use]
    pub fn recv(&self) -> Option<Result<PowerEvent, PowerListenerError>> {
        self.receiver.recv().ok()
    }

    /// Receives the next event, waiting at most `timeout`.
    #[must_use]
    pub fn recv_timeout(&self, timeout: Duration) -> StreamRecvResult {
        match self.receiver.recv_timeout(timeout) {
            Ok(event) => StreamRecvResult::Message(event),
            Err(mpsc::RecvTimeoutError::Timeout) => StreamRecvResult::Timeout,
            Err(mpsc::RecvTimeoutError::Disconnected) => StreamRecvResult::Disconnected,
        }
    }
}
