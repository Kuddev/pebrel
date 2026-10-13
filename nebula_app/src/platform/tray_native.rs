//! macOS 在 GPUI 主线程拥有菜单栏对象；Linux 的 DBus 对象由专用线程拥有。
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::tray::{GpuiTrayCommand, TrayAgent};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

type Callback = Arc<dyn Fn(GpuiTrayCommand) + Send + Sync>;
static CALLBACK: OnceLock<Callback> = OnceLock::new();
static ACTIONS: Mutex<Option<HashMap<MenuId, GpuiTrayCommand>>> = Mutex::new(None);
static STATE: Mutex<Snapshot> =
    Mutex::new(Snapshot { enabled: false, agents: Vec::new(), revision: 0 });

#[derive(Clone)]
struct Snapshot {
    enabled: bool,
    agents: Vec<TrayAgent>,
    revision: u64,
}

struct Native {
    icon: TrayIcon,
    revision: u64,
}
thread_local! { static NATIVE: RefCell<Option<Native>> = const { RefCell::new(None) }; }

pub fn init_gpui(on_command: impl Fn(GpuiTrayCommand) + Send + Sync + 'static) {
    if CALLBACK.set(Arc::new(on_command)).is_err() {
        return;
    }
    MenuEvent::set_event_handler(Some(|event: MenuEvent| {
        let command = ACTIONS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|actions| actions.get(&event.id))
            .copied();
        if let (Some(callback), Some(command)) = (CALLBACK.get(), command) {
            callback(command);
        }
    }));
    tray_icon::TrayIconEvent::set_event_handler(Some(|event| {
        if matches!(
            event,
            tray_icon::TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                button_state: tray_icon::MouseButtonState::Up,
                ..
            }
        ) && let Some(callback) = CALLBACK.get()
        {
            callback(GpuiTrayCommand::Focus(None));
        }
    }));
    #[cfg(target_os = "linux")]
    worker::start();
}

pub fn set_enabled(enabled: bool) {
    {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if state.enabled == enabled {
            return;
        }
        state.enabled = enabled;
        state.revision = state.revision.wrapping_add(1);
    }
    refresh();
}

pub fn update(agents: Vec<TrayAgent>) {
    {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if state.agents == agents {
            return;
        }
        state.agents = agents;
        state.revision = state.revision.wrapping_add(1);
    }
    refresh();
}

pub fn refresh_app_icon() {
    STATE.lock().unwrap_or_else(|e| e.into_inner()).revision += 1;
    refresh();
}

fn refresh() {
    // 工作区可先于原生壳应用设置；没有事件循环所有者时只记录状态，不创建 AppKit 对象。
    if CALLBACK.get().is_none() {
        return;
    }
    #[cfg(target_os = "macos")]
    apply();
    #[cfg(target_os = "linux")]
    worker::refresh();
}

fn apply() {
    let state = STATE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    NATIVE.with_borrow_mut(|native| {
        if !state.enabled {
            *native = None;
            *ACTIONS.lock().unwrap_or_else(|e| e.into_inner()) = None;
            return;
        }
        if native.as_ref().is_some_and(|native| native.revision == state.revision) {
            return;
        }
        match render(&state, native.take()) {
            Ok(updated) => *native = Some(updated),
            Err(error) => log::warn!("System tray unavailable: {error}"),
        }
    });
}

fn render(
    state: &Snapshot,
    previous: Option<Native>,
) -> Result<Native, Box<dyn std::error::Error>> {
    let language =
        crate::i18n::LanguagePreference::from(nebula_settings::RuntimeSettings::load().language)
            .resolved();
    let menu = Menu::new();
    let show = MenuItem::new(language.text(crate::i18n::Message::TrayShow), true, None);
    let quit = MenuItem::new(language.text(crate::i18n::Message::TrayQuit), true, None);
    let mut actions = HashMap::from([
        (show.id().clone(), GpuiTrayCommand::Focus(None)),
        (quit.id().clone(), GpuiTrayCommand::Quit),
    ]);
    menu.append(&show)?;
    if !state.agents.is_empty() {
        menu.append(&PredefinedMenuItem::separator())?;
    }
    for agent in &state.agents {
        let label = if agent.needs_attention {
            format!("● {}", agent.label)
        } else {
            agent.label.clone()
        };
        let item = MenuItem::new(label, true, None);
        actions.insert(item.id().clone(), GpuiTrayCommand::Focus(Some(agent.pane)));
        menu.append(&item)?;
    }
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&quit)?;
    let attention = state.agents.iter().any(|agent| agent.needs_attention);
    let size = 32;
    let mut rgba =
        crate::app_icon::rgba(crate::app_icon::selected(), size).ok_or("Tray icon unavailable")?;
    if attention {
        for y in 23..31 {
            for x in 23..31 {
                rgba.put_pixel(x, y, image::Rgba([255, 160, 32, 255]));
            }
        }
    }
    let icon = Icon::from_rgba(rgba.into_raw(), size, size)?;
    let icon = if let Some(previous) = previous {
        previous.icon.set_icon(Some(icon))?;
        previous.icon.set_menu(Some(Box::new(menu)));
        previous.icon
    } else {
        TrayIconBuilder::new()
            .with_id("pebrel-tray")
            .with_tooltip("Pebrel")
            .with_menu(Box::new(menu))
            .with_icon(icon)
            .build()?
    };
    *ACTIONS.lock().unwrap_or_else(|e| e.into_inner()) = Some(actions);
    Ok(Native { icon, revision: state.revision })
}

pub fn shutdown() {
    #[cfg(target_os = "linux")]
    worker::shutdown();
    #[cfg(target_os = "macos")]
    NATIVE.with_borrow_mut(|native| *native = None);
    *ACTIONS.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

#[cfg(target_os = "linux")]
mod worker {
    use super::*;
    use std::sync::mpsc;
    struct Worker {
        wake: mpsc::SyncSender<()>,
        thread: std::thread::JoinHandle<()>,
    }
    static WORKER: Mutex<Option<Worker>> = Mutex::new(None);

    pub(super) fn start() {
        let (wake, receiver) = mpsc::sync_channel(1);
        match std::thread::Builder::new().name("pebrel-tray".into()).spawn(move || {
            while receiver.recv().is_ok() {
                apply();
            }
            NATIVE.with_borrow_mut(|native| *native = None);
        }) {
            Ok(thread) => {
                *WORKER.lock().unwrap_or_else(|e| e.into_inner()) = Some(Worker { wake, thread })
            },
            Err(error) => log::warn!("Could not start tray worker: {error}"),
        }
    }
    pub(super) fn refresh() {
        if let Some(worker) = WORKER.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = worker.wake.try_send(());
        }
    }
    pub(super) fn shutdown() {
        if let Some(worker) = WORKER.lock().unwrap_or_else(|e| e.into_inner()).take() {
            drop(worker.wake);
            let _ = worker.thread.join();
        }
    }
}
