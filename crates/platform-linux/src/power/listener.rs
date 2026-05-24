//! Linux power listener — normalizes backend signals into [`PowerEvent`] values.

use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

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
            while backend.wait_online_change().is_some() {
                if debounce > Duration::ZERO {
                    let mut quiet_until = Instant::now() + debounce;
                    loop {
                        let now = Instant::now();
                        if now >= quiet_until {
                            break;
                        }
                        let remaining = quiet_until.saturating_duration_since(now);
                        if backend.try_wait_online_change(remaining).is_some() {
                            quiet_until = Instant::now() + debounce;
                        } else {
                            break;
                        }
                    }
                }

                let Some(online) = backend.read_online() else {
                    continue;
                };

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
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use super::*;
    use power_shimmer_core::{PowerEvent, PowerSource, StreamRecvResult};

    struct MockPowerBackend {
        initial: PowerSource,
        online: Arc<Mutex<Option<bool>>>,
        change_rx: Mutex<mpsc::Receiver<()>>,
    }

    impl MockPowerBackend {
        fn on_battery_then_plug_ac() -> Self {
            Self::with_sequence(PowerSource::Battery, Some(false), vec![true])
        }

        fn with_sequence(
            initial: PowerSource,
            start_online: Option<bool>,
            sequence: Vec<bool>,
        ) -> Self {
            let (change_tx, change_rx) = mpsc::channel();
            let online = Arc::new(Mutex::new(start_online));
            let online_for_thread = Arc::clone(&online);

            thread::spawn(move || {
                for value in sequence {
                    *online_for_thread.lock().unwrap() = Some(value);
                    let _ = change_tx.send(());
                }
            });

            Self {
                initial,
                online,
                change_rx: Mutex::new(change_rx),
            }
        }
    }

    impl PowerSourceBackend for MockPowerBackend {
        fn initial_source(&self) -> PowerSource {
            self.initial
        }

        fn wait_online_change(&self) -> Option<()> {
            self.change_rx.lock().unwrap().recv().ok()
        }

        fn read_online(&self) -> Option<bool> {
            *self.online.lock().expect("online mutex poisoned")
        }

        fn try_wait_online_change(&self, timeout: Duration) -> Option<()> {
            self.change_rx.lock().unwrap().recv_timeout(timeout).ok()
        }
    }

    fn recv_event(stream: &PowerEventStream, timeout: Duration, label: &str) -> PowerEvent {
        match stream.recv_timeout(timeout) {
            StreamRecvResult::Message(Ok(event)) => event,
            StreamRecvResult::Message(Err(error)) => panic!("{label} error: {error}"),
            StreamRecvResult::Timeout => panic!("timed out waiting for {label}"),
            StreamRecvResult::Disconnected => panic!("stream ended before {label}"),
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

        handle_event(recv_event(&stream, Duration::from_secs(1), "initial event"));
        handle_event(recv_event(
            &stream,
            Duration::from_secs(1),
            "battery→AC transition",
        ));

        assert_eq!(
            battery_to_ac_handler_count, 1,
            "handler must run exactly once for Battery→AC"
        );
    }

    #[test]
    fn debounce_coalesces_rapid_flicker_into_single_transition() {
        let listener = LinuxPowerListener::new(MockPowerBackend::with_sequence(
            PowerSource::Battery,
            Some(false),
            vec![true, false, true],
        ))
        .with_debounce(Duration::from_millis(50));

        let stream = listener.subscribe().expect("subscribe should succeed");

        let initial = recv_event(&stream, Duration::from_secs(1), "initial event");
        assert_eq!(
            initial,
            PowerEvent::InitialState {
                source: PowerSource::Battery
            }
        );

        let transition = recv_event(&stream, Duration::from_secs(1), "coalesced transition");
        assert_eq!(
            transition,
            PowerEvent::Transition {
                from: PowerSource::Battery,
                to: PowerSource::Ac,
            }
        );

        match stream.recv_timeout(Duration::from_millis(100)) {
            StreamRecvResult::Message(Ok(event)) => {
                panic!("expected one coalesced transition, got extra event: {event:?}")
            }
            StreamRecvResult::Message(Err(error)) => panic!("unexpected stream error: {error}"),
            StreamRecvResult::Timeout | StreamRecvResult::Disconnected => {}
        }
    }

    #[test]
    fn unknown_resolves_via_transition_without_panic() {
        struct UnknownThenBatteryBackend {
            online: Arc<Mutex<Option<bool>>>,
            change_rx: Mutex<mpsc::Receiver<()>>,
        }

        impl PowerSourceBackend for UnknownThenBatteryBackend {
            fn initial_source(&self) -> PowerSource {
                PowerSource::Unknown
            }

            fn wait_online_change(&self) -> Option<()> {
                self.change_rx.lock().unwrap().recv().ok()
            }

            fn read_online(&self) -> Option<bool> {
                *self.online.lock().unwrap()
            }

            fn try_wait_online_change(&self, timeout: Duration) -> Option<()> {
                self.change_rx.lock().unwrap().recv_timeout(timeout).ok()
            }
        }

        let (change_tx, change_rx) = mpsc::channel();
        let online = Arc::new(Mutex::new(None));
        let online_for_thread = Arc::clone(&online);

        thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            *online_for_thread.lock().unwrap() = Some(false);
            let _ = change_tx.send(());
        });

        let listener = LinuxPowerListener::new(UnknownThenBatteryBackend {
            online,
            change_rx: Mutex::new(change_rx),
        })
        .with_debounce(Duration::ZERO);

        let stream = listener.subscribe().expect("subscribe should succeed");

        assert_eq!(
            recv_event(&stream, Duration::from_secs(1), "initial event"),
            PowerEvent::InitialState {
                source: PowerSource::Unknown
            }
        );

        assert_eq!(
            recv_event(&stream, Duration::from_secs(1), "resolution transition"),
            PowerEvent::Transition {
                from: PowerSource::Unknown,
                to: PowerSource::Battery,
            }
        );
    }
}
