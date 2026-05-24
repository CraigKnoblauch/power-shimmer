//! Linux power listener — normalizes backend signals into [`PowerEvent`] values.

use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use power_shimmer_core::{PowerEvent, PowerEventListener, PowerEventStream, PowerListenerError};

use super::backend::{source_from_online, PowerSourceBackend};

/// Debounce duration for production (SPEC: 400 ms).
const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(400);

/// Subscribes to a [`PowerSourceBackend`] and emits normalized power events.
pub struct LinuxPowerListener<B> {
    backend: Arc<B>,
    debounce: Duration,
}

impl<B> LinuxPowerListener<B>
where
    B: PowerSourceBackend,
{
    /// Creates a listener with the default 400 ms transition debounce.
    #[must_use]
    pub fn new(backend: B) -> Self {
        Self {
            backend: Arc::new(backend),
            debounce: DEFAULT_DEBOUNCE,
        }
    }

    /// Overrides debounce (use [`Duration::ZERO`] in unit tests).
    #[must_use]
    pub fn with_debounce(mut self, debounce: Duration) -> Self {
        self.debounce = debounce;
        self
    }
}

impl<B> PowerEventListener for LinuxPowerListener<B>
where
    B: PowerSourceBackend + Send + Sync + 'static,
{
    fn subscribe(&self) -> Result<PowerEventStream, PowerListenerError> {
        let (tx, rx) = mpsc::channel();
        let backend = Arc::clone(&self.backend);
        let debounce = self.debounce;

        let worker = thread::spawn(move || {
            let initial = backend.initial_source();
            if tx
                .send(Ok(PowerEvent::InitialState { source: initial }))
                .is_err()
            {
                return;
            }

            let mut current = initial;
            while let Some(online) = backend.wait_online_change() {
                if debounce > Duration::ZERO {
                    thread::sleep(debounce);
                }

                let next = source_from_online(online);
                if next == current {
                    continue;
                }

                let event = PowerEvent::Transition {
                    from: current,
                    to: next,
                };
                current = next;
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
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::*;
    use power_shimmer_core::{PowerEvent, PowerSource};

    struct MockPowerBackend {
        initial: PowerSource,
        changes: Mutex<VecDeque<bool>>,
    }

    impl MockPowerBackend {
        fn on_battery_then_plug_ac() -> Self {
            let mut changes = VecDeque::new();
            changes.push_back(true);
            Self {
                initial: PowerSource::Battery,
                changes: Mutex::new(changes),
            }
        }
    }

    impl PowerSourceBackend for MockPowerBackend {
        fn initial_source(&self) -> PowerSource {
            self.initial
        }

        fn wait_online_change(&self) -> Option<bool> {
            self.changes.lock().unwrap().pop_front()
        }
    }

    #[test]
    fn battery_to_ac_transition_invokes_event_handler() {
        let listener = LinuxPowerListener::new(MockPowerBackend::on_battery_then_plug_ac())
            .with_debounce(Duration::ZERO);

        let stream = listener.subscribe().expect("subscribe should succeed");

        let mut battery_to_ac_handler_count = 0u32;
        let mut handle_event = |event: PowerEvent| {
            if event.is_battery_to_ac() {
                battery_to_ac_handler_count += 1;
            }
        };

        let first = stream
            .recv_timeout(Duration::from_secs(1))
            .expect("timed out waiting for initial event")
            .expect("stream ended")
            .expect("initial event error");
        handle_event(first);

        let second = stream
            .recv_timeout(Duration::from_secs(1))
            .expect("timed out waiting for battery→AC transition")
            .expect("stream ended before transition")
            .expect("transition event error");
        handle_event(second);

        assert_eq!(
            battery_to_ac_handler_count, 1,
            "handler must run exactly once for Battery→AC"
        );
    }
}
