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
const DISPLAY_DEVICE_PATH: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const UPOWER_DEVICE_INTERFACE: &str = "org.freedesktop.UPower.Device";

/// UPower device type for AC/mains adapters (`Type` property).
const LINE_POWER_DEVICE_TYPE: u32 = 1;

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

    let initial = read_upower_ac_online(&connection, UPOWER_SERVICE).await?;
    update_online(state, initial);

    let (hint_tx, mut hint_rx) = tokio::sync::mpsc::unbounded_channel();

    subscribe_property_changes(&connection, UPOWER_SERVICE, UPOWER_PATH, hint_tx.clone()).await?;
    subscribe_property_changes(
        &connection,
        UPOWER_SERVICE,
        DISPLAY_DEVICE_PATH,
        hint_tx.clone(),
    )
    .await?;

    for path in line_power_device_paths(&connection, UPOWER_SERVICE).await? {
        subscribe_property_changes(&connection, UPOWER_SERVICE, &path, hint_tx.clone()).await?;
    }

    drop(hint_tx);

    while !state.shutdown.load(Ordering::SeqCst) {
        if hint_rx.recv().await.is_none() {
            return Err("UPower properties stream ended".to_string());
        }

        let online = read_upower_ac_online(&connection, UPOWER_SERVICE).await?;
        update_online(state, online);
    }

    Ok(())
}

async fn subscribe_property_changes(
    connection: &Connection,
    destination: &str,
    path: &str,
    hint_tx: tokio::sync::mpsc::UnboundedSender<()>,
) -> Result<(), String> {
    let properties = properties_proxy(connection, destination, path).await?;
    let mut changes = properties
        .receive_properties_changed()
        .await
        .map_err(|error| error.to_string())?;

    tokio::spawn(async move {
        while changes.next().await.is_some() {
            let _ = hint_tx.send(());
        }
    });

    Ok(())
}

async fn read_upower_ac_online(
    connection: &Connection,
    destination: &str,
) -> Result<Option<bool>, String> {
    if let Some(online) = read_display_device_online(connection, destination).await? {
        return Ok(Some(online));
    }

    if let Some(online) = read_line_power_online(connection, destination).await? {
        return Ok(Some(online));
    }

    read_legacy_root_online(connection, destination).await
}

async fn read_display_device_online(
    connection: &Connection,
    destination: &str,
) -> Result<Option<bool>, String> {
    let properties = match properties_proxy(connection, destination, DISPLAY_DEVICE_PATH).await {
        Ok(proxy) => proxy,
        Err(_) => return Ok(None),
    };
    let interface =
        InterfaceName::try_from(UPOWER_DEVICE_INTERFACE).map_err(|error| error.to_string())?;

    match properties.get(interface, "OnBattery").await {
        Ok(value) => {
            let on_battery = bool::try_from(value).map_err(|error| error.to_string())?;
            Ok(Some(!on_battery))
        }
        Err(_) => Ok(None),
    }
}

async fn read_line_power_online(
    connection: &Connection,
    destination: &str,
) -> Result<Option<bool>, String> {
    let interface =
        InterfaceName::try_from(UPOWER_DEVICE_INTERFACE).map_err(|error| error.to_string())?;
    let mut found = None;

    for path in enumerate_device_paths(connection, destination).await? {
        if path == DISPLAY_DEVICE_PATH {
            continue;
        }

        let properties = match properties_proxy(connection, destination, &path).await {
            Ok(proxy) => proxy,
            Err(_) => continue,
        };

        let device_type = match properties.get(interface.clone(), "Type").await {
            Ok(value) => u32::try_from(value).map_err(|error| error.to_string())?,
            Err(_) => continue,
        };

        if device_type != LINE_POWER_DEVICE_TYPE {
            continue;
        }

        match properties.get(interface.clone(), "Online").await {
            Ok(value) => {
                let online = bool::try_from(value).map_err(|error| error.to_string())?;
                found = Some(found.unwrap_or(false) || online);
            }
            Err(_) => continue,
        }
    }

    Ok(found)
}

async fn read_legacy_root_online(
    connection: &Connection,
    destination: &str,
) -> Result<Option<bool>, String> {
    let properties = match properties_proxy(connection, destination, UPOWER_PATH).await {
        Ok(proxy) => proxy,
        Err(_) => return Ok(None),
    };
    let interface = InterfaceName::try_from(UPOWER_INTERFACE).map_err(|error| error.to_string())?;

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

async fn enumerate_device_paths(
    connection: &Connection,
    destination: &str,
) -> Result<Vec<String>, String> {
    use zbus::zvariant::OwnedObjectPath;

    let reply = match connection
        .call_method(
            Some(destination),
            UPOWER_PATH,
            Some(UPOWER_INTERFACE),
            "EnumerateDevices",
            &(),
        )
        .await
    {
        Ok(reply) => reply,
        Err(_) => return Ok(Vec::new()),
    };

    let paths: Vec<OwnedObjectPath> = reply
        .body()
        .deserialize()
        .map_err(|error| error.to_string())?;

    Ok(paths.into_iter().map(|path| path.to_string()).collect())
}

async fn line_power_device_paths(
    connection: &Connection,
    destination: &str,
) -> Result<Vec<String>, String> {
    let interface =
        InterfaceName::try_from(UPOWER_DEVICE_INTERFACE).map_err(|error| error.to_string())?;
    let mut paths = Vec::new();

    for path in enumerate_device_paths(connection, destination).await? {
        if path == DISPLAY_DEVICE_PATH {
            continue;
        }

        let properties = match properties_proxy(connection, destination, &path).await {
            Ok(proxy) => proxy,
            Err(_) => continue,
        };

        let device_type = match properties.get(interface.clone(), "Type").await {
            Ok(value) => u32::try_from(value).map_err(|error| error.to_string())?,
            Err(_) => continue,
        };

        if device_type == LINE_POWER_DEVICE_TYPE {
            paths.push(path);
        }
    }

    Ok(paths)
}

async fn properties_proxy<'c>(
    connection: &'c Connection,
    destination: &'c str,
    path: &'c str,
) -> Result<PropertiesProxy<'c>, String> {
    PropertiesProxy::builder(connection)
        .destination(destination)
        .map_err(|error| error.to_string())?
        .path(path)
        .map_err(|error| error.to_string())?
        .build()
        .await
        .map_err(|error| error.to_string())
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
    use zbus::connection::Builder;
    use zbus::interface;
    use zbus::names::OwnedUniqueName;
    use zbus::Connection;

    use super::*;

    struct MockUpowerRoot;

    #[interface(name = "org.freedesktop.UPower")]
    impl MockUpowerRoot {}

    struct MockUpowerRootLegacy {
        on_line: bool,
    }

    #[interface(name = "org.freedesktop.UPower")]
    impl MockUpowerRootLegacy {
        #[zbus(property)]
        fn on_line(&self) -> bool {
            self.on_line
        }
    }

    struct MockDisplayDevice {
        on_battery: bool,
    }

    #[interface(name = "org.freedesktop.UPower.Device")]
    impl MockDisplayDevice {
        #[zbus(property)]
        fn on_battery(&self) -> bool {
            self.on_battery
        }
    }

    async fn mock_upower_bus(server: MockUpowerRootLegacy) -> (Connection, OwnedUniqueName) {
        let server = Builder::session()
            .expect("session bus")
            .serve_at(UPOWER_PATH, server)
            .expect("serve root UPower object")
            .build()
            .await
            .expect("mock UPower server connection");

        let destination = server
            .unique_name()
            .expect("mock UPower unique bus name")
            .clone();
        let client = Connection::session()
            .await
            .expect("session bus client connection");

        std::mem::forget(server);
        (client, destination)
    }

    async fn mock_modern_upower_bus() -> (Connection, OwnedUniqueName) {
        let server = Builder::session()
            .expect("session bus")
            .serve_at(UPOWER_PATH, MockUpowerRoot)
            .expect("serve root UPower object")
            .serve_at(DISPLAY_DEVICE_PATH, MockDisplayDevice { on_battery: false })
            .expect("serve DisplayDevice")
            .build()
            .await
            .expect("mock UPower server connection");

        let destination = server
            .unique_name()
            .expect("mock UPower unique bus name")
            .clone();
        let client = Connection::session()
            .await
            .expect("session bus client connection");

        std::mem::forget(server);
        (client, destination)
    }

    #[test]
    fn new_backend_starts_without_panic() {
        let backend = UpowerBackend::new();
        let _ = backend.initial_source();
    }

    /// Modern UPower (≥ 0.99.x) omits root `OnLine` but exposes `OnBattery` on
    /// `DisplayDevice`. AC online should be inferred as `OnBattery == false`.
    #[tokio::test]
    async fn modern_upower_without_root_online_reads_ac_from_display_device() {
        let (connection, destination) = mock_modern_upower_bus().await;
        let bus_name = destination.as_str();

        let root_properties = PropertiesProxy::builder(&connection)
            .destination(bus_name)
            .expect("UPower destination")
            .path(UPOWER_PATH)
            .expect("UPower path")
            .build()
            .await
            .expect("root PropertiesProxy");

        let display_properties = PropertiesProxy::builder(&connection)
            .destination(bus_name)
            .expect("UPower destination")
            .path(DISPLAY_DEVICE_PATH)
            .expect("DisplayDevice path")
            .build()
            .await
            .expect("DisplayDevice PropertiesProxy");

        let root_interface =
            InterfaceName::try_from(UPOWER_INTERFACE).expect("UPower interface name");
        let device_interface =
            InterfaceName::try_from(UPOWER_DEVICE_INTERFACE).expect("UPower device interface");

        assert!(
            root_properties
                .get(root_interface.clone(), "OnLine")
                .await
                .is_err(),
            "mock must omit legacy root OnLine to represent modern UPower"
        );

        let on_battery_value = display_properties
            .get(device_interface, "OnBattery")
            .await
            .expect("DisplayDevice OnBattery");
        let on_battery = bool::try_from(on_battery_value).expect("OnBattery bool");
        assert!(
            !on_battery,
            "mock DisplayDevice must report on AC power (OnBattery=false)"
        );

        let online = read_upower_ac_online(&connection, bus_name)
            .await
            .expect("read should not hard-fail when root OnLine is absent");

        assert_eq!(
            online,
            Some(true),
            "UPower is reachable and DisplayDevice reports AC; read_upower_ac_online must \
             return Some(true) instead of None"
        );
    }

    #[tokio::test]
    async fn legacy_upower_root_online_is_used_when_present() {
        let (connection, destination) =
            mock_upower_bus(MockUpowerRootLegacy { on_line: false }).await;

        let online = read_upower_ac_online(&connection, destination.as_str())
            .await
            .expect("legacy root OnLine read");

        assert_eq!(online, Some(false));
    }
}
