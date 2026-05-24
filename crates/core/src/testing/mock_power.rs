//! Test double for [`PowerEventListener`](crate::ports::PowerEventListener).

use std::sync::mpsc;
use std::thread;

use crate::domain::{PowerEvent, PowerListenerError};
use crate::ports::{PowerEventListener, PowerEventStream};

/// Injects a predetermined sequence of power events for orchestrator tests.
#[derive(Debug)]
pub struct MockPowerEventListener {
    events: Vec<PowerEvent>,
}

impl MockPowerEventListener {
    /// Creates a listener that emits `events` in order, then closes the stream.
    #[must_use]
    pub fn new(events: Vec<PowerEvent>) -> Self {
        Self { events }
    }
}

impl PowerEventListener for MockPowerEventListener {
    fn subscribe(&self) -> Result<PowerEventStream, PowerListenerError> {
        let (tx, rx) = mpsc::channel();
        let events = self.events.clone();

        let worker = thread::spawn(move || {
            for event in events {
                if tx.send(Ok(event)).is_err() {
                    break;
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
