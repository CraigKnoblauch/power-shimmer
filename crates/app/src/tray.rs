//! System tray menu (SPEC: Play now, Enable/Disable auto, Quit).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tracing::{error, info};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon, TrayIconBuilder};

use crate::error::AppError;
use crate::wiring::LinuxOrchestrator;

const ID_PLAY: &str = "play";
const ID_AUTO: &str = "auto";
const ID_QUIT: &str = "quit";

/// Runs the tray event loop on the current thread until Quit.
///
/// # Errors
///
/// Returns [`AppError::Tray`] when the tray icon cannot be created.
pub fn run_tray(
    orchestrator: &Arc<LinuxOrchestrator>,
    auto_enabled: &Arc<AtomicBool>,
) -> Result<(), AppError> {
    let icon = tray_icon().map_err(AppError::Tray)?;
    let menu = build_menu(auto_enabled.load(Ordering::Relaxed));
    let play_id = MenuId::new(ID_PLAY);
    let auto_id = MenuId::new(ID_AUTO);
    let quit_id = MenuId::new(ID_QUIT);

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Power Shimmer")
        .with_icon(icon)
        .build()
        .map_err(|error| AppError::Tray(error.to_string()))?;

    let runtime = tokio::runtime::Handle::current();
    info!("system tray active");

    loop {
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

        std::thread::sleep(Duration::from_millis(16));
    }

    Ok(())
}

fn build_menu(auto_enabled: bool) -> Menu {
    let menu = Menu::new();
    let play = MenuItem::with_id(ID_PLAY, "Play now", true, None);
    let auto_label = if auto_enabled {
        "Disable auto shimmer"
    } else {
        "Enable auto shimmer"
    };
    let auto = MenuItem::with_id(ID_AUTO, auto_label, true, None);
    let quit = MenuItem::with_id(ID_QUIT, "Quit", true, None);

    menu.append(&play).expect("menu play");
    menu.append(&auto).expect("menu auto");
    menu.append(&quit).expect("menu quit");
    menu
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
