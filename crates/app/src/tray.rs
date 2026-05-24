//! System tray menu (SPEC: Play now, Enable/Disable auto, Quit).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use gtk::glib;
use tracing::{error, info};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon, TrayIconBuilder};

use crate::error::AppError;
use crate::wiring::LinuxOrchestrator;

const ID_PLAY: &str = "play";
const ID_AUTO: &str = "auto";
const ID_QUIT: &str = "quit";
const TRAY_THREAD_NAME: &str = "power-shimmer-tray";
const EVENT_LOOP_SLEEP: Duration = Duration::from_millis(16);

/// Initializes GTK on the current thread so tray menus can be created.
///
/// Must be called on the tray thread before [`prepare_tray_menu`] or [`run_tray`].
///
/// # Errors
///
/// Returns [`AppError::Tray`] when GTK initialization fails.
pub fn init_tray_gtk() -> Result<(), AppError> {
    if gtk::is_initialized() {
        return Ok(());
    }

    gtk::init().map_err(|error| AppError::Tray(error.to_string()))
}

/// Builds the tray context menu after GTK has been initialized.
///
/// # Errors
///
/// Returns [`AppError::Tray`] when menu items cannot be appended.
pub fn prepare_tray_menu(auto_enabled: bool) -> Result<(), AppError> {
    let menu = build_menu(auto_enabled)?;
    drop(menu);
    Ok(())
}

/// Runs the tray event loop on a dedicated GTK thread until Quit.
///
/// # Errors
///
/// Returns [`AppError::Tray`] when the tray thread cannot start, panics, or fails setup.
pub fn run_tray(
    orchestrator: &Arc<LinuxOrchestrator>,
    auto_enabled: &Arc<AtomicBool>,
) -> Result<(), AppError> {
    let orchestrator = Arc::clone(orchestrator);
    let auto_enabled = Arc::clone(auto_enabled);
    let runtime = tokio::runtime::Handle::current();

    let handle: JoinHandle<Result<(), AppError>> = thread::Builder::new()
        .name(TRAY_THREAD_NAME.into())
        .spawn(move || run_tray_on_thread(&orchestrator, &auto_enabled, &runtime))
        .map_err(|error| AppError::Tray(format!("failed to spawn tray thread: {error}")))?;

    handle
        .join()
        .map_err(|_| AppError::Tray("tray thread panicked".to_string()))?
}

fn run_tray_on_thread(
    orchestrator: &Arc<LinuxOrchestrator>,
    auto_enabled: &Arc<AtomicBool>,
    runtime: &tokio::runtime::Handle,
) -> Result<(), AppError> {
    init_tray_gtk()?;

    let icon = tray_icon().map_err(AppError::Tray)?;
    let menu = build_menu(auto_enabled.load(Ordering::Relaxed))?;
    let play_id = MenuId::new(ID_PLAY);
    let auto_id = MenuId::new(ID_AUTO);
    let quit_id = MenuId::new(ID_QUIT);

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Power Shimmer")
        .with_icon(icon)
        .build()
        .map_err(|error| AppError::Tray(error.to_string()))?;

    info!("system tray active");

    loop {
        pump_gtk_events();

        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == quit_id {
                info!("tray: Quit");
                orchestrator.shutdown();
                break;
            }

            if event.id == play_id {
                info!("tray: Play now");
                let orch = Arc::clone(orchestrator);
                runtime.spawn(async move {
                    if let Err(error) = orch.trigger_manual().await {
                        error!(%error, "tray: manual shimmer failed");
                    } else {
                        info!("tray: manual shimmer completed");
                    }
                });
                continue;
            }

            if event.id == auto_id {
                let enabled = !auto_enabled.load(Ordering::Relaxed);
                auto_enabled.store(enabled, Ordering::Relaxed);
                orchestrator.set_auto_enabled(enabled);
                info!(enabled, "tray: auto shimmer toggled");
            }
        }

        thread::sleep(EVENT_LOOP_SLEEP);
    }

    Ok(())
}

fn pump_gtk_events() {
    while glib::MainContext::default().iteration(false) {}
}

fn build_menu(auto_enabled: bool) -> Result<Menu, AppError> {
    let menu = Menu::new();
    let play = MenuItem::with_id(ID_PLAY, "Play now", true, None);
    let auto_label = if auto_enabled {
        "Disable auto shimmer"
    } else {
        "Enable auto shimmer"
    };
    let auto = MenuItem::with_id(ID_AUTO, auto_label, true, None);
    let quit = MenuItem::with_id(ID_QUIT, "Quit", true, None);

    menu.append(&play)
        .map_err(|error| AppError::Tray(format!("menu play item: {error}")))?;
    menu.append(&auto)
        .map_err(|error| AppError::Tray(format!("menu auto item: {error}")))?;
    menu.append(&quit)
        .map_err(|error| AppError::Tray(format!("menu quit item: {error}")))?;
    Ok(menu)
}

fn tray_icon() -> Result<Icon, String> {
    const WIDTH: u32 = 16;
    const HEIGHT: u32 = 16;
    let mut rgba = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for y in 0u8..16 {
        for x in 0u8..16 {
            rgba.extend([x.wrapping_mul(16), y.wrapping_mul(16), 220, 255]);
        }
    }
    Icon::from_rgba(rgba, WIDTH, HEIGHT).map_err(|error| error.to_string())
}
