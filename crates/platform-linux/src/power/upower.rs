//! `UPower` D-Bus power listener.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use futures_lite::stream::StreamExt;
use power_shimmer_core::PowerSource;
use tracing::{debug, warn};
use zbus::fdo::PropertiesProxy;
use zbus::names::InterfaceName;
use zbus::Connection;

use super::backend::{source_from_online_option, PowerSourceBackend};

const UPOWER_SERVICE: &str = "org.freedesktop.UPower";
const UPOWER_PATH: &str = "/org/freedesktop/UPower";
const UPOWER_INTERFACE: &str = "org.freedesktop.UPower";

const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(5);

/// Reads AC online state from `UPower` over the system D-Bus.
pub struct UpowerBackend {
    state: Arc<SharedState>,
    _monitor: JoinHandle<()>,
}

struct SharedState {
    online: Mutex<Option<bool>>,
    connected: AtomicBool,
    change_tx: Sender<()>,
    change_rx: Mutex<Receiver<()>>,
    shutdown: AtomicBool,
}

impl Default for UpowerBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl UpowerBackend {
    /// Creates a backend that connects to `UPower` or retries in the background.
    #[must_use]
    pub fn new() -> Self {
        let (change_tx, change_rx) = mpsc::channel();
        let state = Arc::new(SharedState {
            online: Mutex::new(None),
            connected: AtomicBool::new(false),
            change_tx,
            change_rx: Mutex::new(change_rx),
            shutdown: AtomicBool::new(false),
        });

        let monitor_state = Arc::clone(&state);
        let monitor = thread::spawn(move || monitor_loop(monitor_state.as_ref()));

        Self {
            state,
            _monitor: monitor,
        }
    }

    /// Returns true when `UPower` is connected and an online reading is available.
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.state.connected.load(Ordering::SeqCst) && self.read_online().is_some()
    }
}

impl PowerSourceBackend for UpowerBackend {
    fn initial_source(&self) -> PowerSource {
        source_from_online_option(self.read_online())
    }

    fn wait_online_change(&self) -> Option<()> {
        self.state
            .change_rx
            .lock()
            .expect("change_rx mutex poisoned")
            .recv()
            .ok()
    }

    fn read_online(&self) -> Option<bool> {
        *self.state.online.lock().expect("online mutex poisoned")
    }

    fn try_wait_online_change(&self, timeout: Duration) -> Option<()> {
        self.state
            .change_rx
            .lock()
            .expect("change_rx mutex poisoned")
            .recv_timeout(timeout)
            .ok()
    }
}

impl Drop for UpowerBackend {
    fn drop(&mut self) {
        self.state.shutdown.store(true, Ordering::SeqCst);
        let _ = self.state.change_tx.send(());
    }
}

fn monitor_loop(state: &SharedState) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .expect("build UPower monitor tokio runtime");

    let mut retry_delay = INITIAL_RETRY_DELAY;

    while !state.shutdown.load(Ordering::SeqCst) {
        match runtime.block_on(run_upower_session(state)) {
            Ok(()) => debug!("UPower monitor session ended"),
            Err(error) => {
                warn!(%error, "UPower monitor session failed");
                state.connected.store(false, Ordering::SeqCst);
                *state.online.lock().expect("online mutex poisoned") = None;
            }
        }

        if state.shutdown.load(Ordering::SeqCst) {
            break;
        }

        thread::sleep(retry_delay);
        retry_delay = retry_delay.saturating_mul(2).min(MAX_RETRY_DELAY);
    }
}

async fn run_upower_session(state: &SharedState) -> Result<(), String> {
    let connection = Connection::system()
        .await
        .map_err(|error| error.to_string())?;

    let properties = PropertiesProxy::builder(&connection)
        .destination(UPOWER_SERVICE)
        .map_err(|error| error.to_string())?
        .path(UPOWER_PATH)
        .map_err(|error| error.to_string())?
        .build()
        .await
        .map_err(|error| error.to_string())?;

    let initial = read_upower_online(&properties).await?;
    update_online(state, initial);

    let mut changes = properties
        .receive_properties_changed()
        .await
        .map_err(|error| error.to_string())?;

    while !state.shutdown.load(Ordering::SeqCst) {
        if changes.next().await.is_none() {
            return Err("UPower properties stream ended".to_string());
        }

        let online = read_upower_online(&properties).await?;
        update_online(state, online);
    }

    Ok(())
}

async fn read_upower_online(properties: &PropertiesProxy<'_>) -> Result<Option<bool>, String> {
    let interface =
        InterfaceName::try_from(UPOWER_INTERFACE).map_err(|error| error.to_string())?;

    match properties.get(interface, "OnLine").await {
        Ok(value) => {
            let online = bool::try_from(value).map_err(|error| error.to_string())?;
            Ok(Some(online))
        }
        Err(error) => {
            warn!(%error, "failed to read UPower OnLine property");
            Ok(None)
        }
    }
}

fn update_online(state: &SharedState, online: Option<bool>) {
    let mut guard = state.online.lock().expect("online mutex poisoned");
    let changed = *guard != online;
    *guard = online;

    if online.is_some() {
        state.connected.store(true, Ordering::SeqCst);
    }

    if changed {
        debug!(?online, "UPower online state updated");
        let _ = state.change_tx.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_backend_starts_without_panic() {
        let backend = UpowerBackend::new();
        let _ = backend.initial_source();
    }
}
