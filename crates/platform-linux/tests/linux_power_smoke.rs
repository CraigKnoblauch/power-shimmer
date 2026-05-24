//! Integration smoke test for Linux power listener — requires UPower (v1).

use std::time::Duration;

use power_shimmer_core::{PowerEvent, PowerEventListener, PowerSource, StreamRecvResult};
use power_shimmer_platform_linux::power::{LinuxPowerBackend, LinuxPowerListener};

#[test]
#[ignore = "requires UPower and hardware; enable when adapter is implemented"]
fn linux_power_smoke_receives_initial_state() {
    let backend = LinuxPowerBackend::select();
    let listener = LinuxPowerListener::new(backend);
    let stream = listener.subscribe().expect("subscribe should succeed");

    let event = match stream.recv_timeout(Duration::from_secs(2)) {
        StreamRecvResult::Message(Ok(event)) => event,
        StreamRecvResult::Message(Err(error)) => panic!("stream error: {error}"),
        StreamRecvResult::Timeout => panic!("timed out waiting for InitialState"),
        StreamRecvResult::Disconnected => panic!("stream disconnected before InitialState"),
    };

    match event {
        PowerEvent::InitialState { source } => {
            assert!(
                matches!(
                    source,
                    PowerSource::Ac | PowerSource::Battery | PowerSource::Unknown
                ),
                "unexpected initial source: {source:?}"
            );
        }
        other => panic!("expected InitialState first, got {other:?}"),
    }
}
