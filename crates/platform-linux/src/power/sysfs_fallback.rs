//! sysfs `power_supply` fallback listener.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use power_shimmer_core::PowerSource;
use tracing::debug;

use super::backend::{source_from_online_option, PowerSourceBackend};

const DEFAULT_SUPPLY_ROOT: &str = "/sys/class/power_supply";
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Reads AC online state from `/sys/class/power_supply` when `UPower` is unavailable.
pub struct SysfsFallbackBackend {
    state: Arc<SharedState>,
    _monitor: JoinHandle<()>,
}

struct SharedState {
    online: Mutex<Option<bool>>,
    change_tx: Sender<()>,
    change_rx: Mutex<Receiver<()>>,
    shutdown: AtomicBool,
}

impl SysfsFallbackBackend {
    /// Creates a backend that polls the default sysfs power supply directory.
    #[must_use]
    pub fn new() -> Self {
        Self::with_supply_root(PathBuf::from(DEFAULT_SUPPLY_ROOT))
    }

    /// Creates a backend that polls a custom power supply root (for tests).
    #[must_use]
    pub fn with_supply_root(supply_root: PathBuf) -> Self {
        let initial_online = read_ac_online(&supply_root);
        let (change_tx, change_rx) = mpsc::channel();

        let state = Arc::new(SharedState {
            online: Mutex::new(initial_online),
            change_tx,
            change_rx: Mutex::new(change_rx),
            shutdown: AtomicBool::new(false),
        });

        let monitor_state = Arc::clone(&state);
        let monitor = thread::spawn(move || monitor_loop(&supply_root, monitor_state.as_ref()));

        Self {
            state,
            _monitor: monitor,
        }
    }
}

impl Default for SysfsFallbackBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerSourceBackend for SysfsFallbackBackend {
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

impl Drop for SysfsFallbackBackend {
    fn drop(&mut self) {
        self.state.shutdown.store(true, Ordering::SeqCst);
        let _ = self.state.change_tx.send(());
    }
}

fn monitor_loop(supply_root: &Path, state: &SharedState) {
    let mut last = *state.online.lock().expect("online mutex poisoned");

    while !state.shutdown.load(Ordering::SeqCst) {
        thread::sleep(POLL_INTERVAL);

        if state.shutdown.load(Ordering::SeqCst) {
            break;
        }

        let current = read_ac_online(supply_root);
        if current != last {
            debug!(?last, ?current, "sysfs AC online state changed");
            *state.online.lock().expect("online mutex poisoned") = current;
            let _ = state.change_tx.send(());
            last = current;
        }
    }
}

/// Returns aggregate AC online state from sysfs power supply entries.
fn read_ac_online(root: &Path) -> Option<bool> {
    let entries = fs::read_dir(root).ok()?;
    let mut found_mains = false;
    let mut any_online = false;

    for entry in entries.flatten() {
        let path = entry.path();
        if !is_mains_supply(&path) {
            continue;
        }

        found_mains = true;
        if read_supply_online(&path).unwrap_or(false) {
            any_online = true;
        }
    }

    if found_mains {
        Some(any_online)
    } else {
        None
    }
}

fn is_mains_supply(path: &Path) -> bool {
    if let Ok(supply_type) = fs::read_to_string(path.join("type")) {
        if supply_type.trim() == "Mains" {
            return true;
        }
    }

    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with("AC") || name.starts_with("ADP") || name.starts_with("ac")
        })
}

fn read_supply_online(path: &Path) -> Option<bool> {
    let online_path = path.join("online");
    if online_path.exists() {
        return parse_online_value(&fs::read_to_string(online_path).ok()?);
    }

    let status_path = path.join("status");
    if status_path.exists() {
        let status = fs::read_to_string(status_path).ok()?;
        return match status.trim() {
            "Discharging" => Some(false),
            "Charging" | "Full" | "Not charging" => Some(true),
            _ => None,
        };
    }

    None
}

fn parse_online_value(raw: &str) -> Option<bool> {
    match raw.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use super::*;

    fn write_supply(root: &Path, name: &str, supply_type: &str, online: &str) {
        let supply_path = root.join(name);
        fs::create_dir_all(&supply_path).expect("create supply dir");
        fs::write(supply_path.join("type"), supply_type).expect("write type");
        fs::write(supply_path.join("online"), online).expect("write online");
    }

    #[test]
    fn read_ac_online_detects_plugged_mains_supply() {
        let root = std::env::temp_dir().join(format!("power-shimmer-sysfs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);

        write_supply(&root, "AC0", "Mains", "1");
        write_supply(&root, "BAT0", "Battery", "1");

        assert_eq!(read_ac_online(&root), Some(true));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn read_ac_online_detects_unplugged_mains_supply() {
        let root = std::env::temp_dir().join(format!(
            "power-shimmer-sysfs-unplugged-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);

        write_supply(&root, "ACAD", "Mains", "0");
        write_supply(&root, "BAT0", "Battery", "1");

        assert_eq!(read_ac_online(&root), Some(false));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn backend_notifies_on_sysfs_change() {
        let root =
            std::env::temp_dir().join(format!("power-shimmer-sysfs-notify-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);

        write_supply(&root, "AC0", "Mains", "0");

        let backend = SysfsFallbackBackend::with_supply_root(root.clone());
        assert_eq!(backend.initial_source(), PowerSource::Battery);

        fs::write(root.join("AC0/online"), "1").expect("write online");

        assert!(
            backend.wait_online_change().is_some(),
            "expected change notification after sysfs update"
        );
        assert_eq!(backend.read_online(), Some(true));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn backend_poll_detects_change_within_interval() {
        let root =
            std::env::temp_dir().join(format!("power-shimmer-sysfs-poll-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);

        write_supply(&root, "AC0", "Mains", "0");

        let backend = SysfsFallbackBackend::with_supply_root(root.clone());

        fs::write(root.join("AC0/online"), "1").expect("write online");

        let deadline = Duration::from_secs(2);
        let start = std::time::Instant::now();
        while start.elapsed() < deadline {
            if backend.read_online() == Some(true) {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }

        panic!("sysfs poll did not observe online change within 2s");
    }
}
