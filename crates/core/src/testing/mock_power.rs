//! Test double for [`PowerEventListener`](crate::ports::PowerEventListener).

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::domain::{PowerEvent, PowerListenerError};
use crate::ports::{PowerEventListener, PowerEventStream};

/// Injects a predetermined sequence of power events for orchestrator tests.
#[derive(Debug, Clone)]
pub struct MockPowerEventListener {
    events: Vec<PowerEvent>,
    keep_alive: bool,
}

impl MockPowerEventListener {
    /// Creates a listener that emits `events` in order, then closes the stream.
    #[must_use]
    pub fn new(events: Vec<PowerEvent>) -> Self {
        Self {
            events,
            keep_alive: false,
        }
    }

    /// Keeps the subscription open after emitting `events` until the stream is dropped.
    #[must_use]
    pub fn keep_alive_after_events(events: Vec<PowerEvent>) -> Self {
        Self {
            events,
            keep_alive: true,
        }
    }
}

impl PowerEventListener for MockPowerEventListener {
    fn subscribe(&self) -> Result<PowerEventStream, PowerListenerError> {
        let (tx, rx) = mpsc::channel();
        let events = self.events.clone();
        let keep_alive = self.keep_alive;

        let worker = thread::spawn(move || {
            for event in events {
                if tx.send(Ok(event)).is_err() {
                    return;
                }
            }

            if keep_alive {
                loop {
                    thread::sleep(Duration::from_secs(3600));
                }
            }
        });

        Ok(PowerEventStream::new(rx, worker))
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::{PowerEvent, PowerSource};

    use super::*;

    #[test]
    fn emits_injected_events_in_order() {
        let listener = MockPowerEventListener::new(vec![
            PowerEvent::InitialState {
                source: PowerSource::Battery,
            },
            PowerEvent::Transition {
                from: PowerSource::Battery,
                to: PowerSource::Ac,
            },
        ]);

        let stream = listener.subscribe().expect("subscribe should succeed");

        let first = stream
            .recv()
            .expect("stream should yield first event")
            .expect("first event should be ok");
        assert_eq!(
            first,
            PowerEvent::InitialState {
                source: PowerSource::Battery,
            }
        );

        let second = stream
            .recv()
            .expect("stream should yield second event")
            .expect("second event should be ok");
        assert!(second.is_battery_to_ac());

        assert!(
            stream.recv().is_none(),
            "stream should end after injected events"
        );
    }
}
