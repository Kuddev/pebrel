//! Wayland 全局键由桌面 portal 授权；X11 继续使用现有注册器。
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use futures::StreamExt as _;
use std::sync::mpsc;
use std::time::Duration;
use tokio::sync::watch;

pub(crate) struct Portal {
    change: Option<watch::Sender<String>>,
    pressed: mpsc::Receiver<()>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Portal {
    pub(crate) fn start(combo: String) -> std::io::Result<Self> {
        let (change, mut changes) = watch::channel(combo);
        let (pressed, receive) = mpsc::sync_channel(1);
        let worker =
            std::thread::Builder::new().name("pebrel-shortcut-portal".into()).spawn(move || {
                let Ok(runtime) =
                    tokio::runtime::Builder::new_current_thread().enable_all().build()
                else {
                    return;
                };
                runtime.block_on(async move {
                    loop {
                        let combo = changes.borrow_and_update().clone();
                        if !combo.trim().is_empty() {
                            match register(&combo, &pressed, &mut changes).await {
                                Ok(true) => continue,
                                Ok(false) => return,
                                Err(error) => {
                                    log::warn!("Global shortcut portal unavailable: {error}")
                                },
                            }
                        }
                        // 失败后只在设置变化时重试，不能反复弹出系统授权窗口。
                        if changes.changed().await.is_err() {
                            break;
                        }
                    }
                });
            })?;
        Ok(Self { change: Some(change), pressed: receive, worker: Some(worker) })
    }

    pub(crate) fn set_combo(&self, combo: String) {
        if let Some(change) = &self.change {
            change.send_if_modified(|current| {
                if *current == combo {
                    return false;
                }
                *current = combo;
                true
            });
        }
    }

    pub(crate) fn take_pressed(&self) -> bool {
        let mut pressed = false;
        while self.pressed.try_recv().is_ok() {
            pressed = true;
        }
        pressed
    }
}

impl Drop for Portal {
    fn drop(&mut self) {
        self.change.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

async fn register(
    combo: &str,
    pressed: &mpsc::SyncSender<()>,
    changes: &mut watch::Receiver<String>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let setup = async {
        let portal = GlobalShortcuts::new().await?;
        let session = portal.create_session(Default::default()).await?;
        Ok::<_, ashpd::Error>((portal, session))
    };
    let (portal, session) = tokio::time::timeout(Duration::from_secs(5), setup).await??;
    // Session 的公共序列化合同就是对象路径；不依赖库内私有 path()。
    let session_handle = serde_json::to_value(&session)?;
    let result: Result<bool, Box<dyn std::error::Error>> = async {
        let mut events = portal.receive_activated().await?;
        let trigger = preferred_trigger(combo);
        let shortcut = NewShortcut::new("quick-terminal", "Pebrel quick terminal").preferred_trigger(trigger.as_deref());
        let shortcuts = [shortcut];
        tokio::select! {
            result = portal.bind_shortcuts(&session, &shortcuts, None, Default::default()) => { result?.response()?; },
            changed = changes.changed() => return Ok(changed.is_ok()),
        }
        loop {
            tokio::select! {
                event = events.next() => {
                    let Some(event) = event else { return Ok(false) };
                    if event.shortcut_id() == "quick-terminal" && Some(event.session_handle().as_str()) == session_handle.as_str() {
                        let _ = pressed.try_send(());
                    }
                },
                changed = changes.changed() => return Ok(changed.is_ok()),
            }
        }
    }.await;
    // 改键、取消和退出都显式关闭旧 session，不能遗留全局热键注册。
    let _ = tokio::time::timeout(Duration::from_secs(2), session.close()).await;
    result
}

fn preferred_trigger(combo: &str) -> Option<String> {
    use global_hotkey::hotkey::{HotKey, Modifiers};
    let key = combo.parse::<HotKey>().ok()?;
    let mut parts = Vec::new();
    for (modifier, name) in [
        (Modifiers::CONTROL, "CTRL"),
        (Modifiers::ALT, "ALT"),
        (Modifiers::SHIFT, "SHIFT"),
        (Modifiers::SUPER, "LOGO"),
    ] {
        if key.mods.contains(modifier) {
            parts.push(name.to_owned());
        }
    }
    let name = key.key.to_string();
    let key = match name.as_str() {
        "Space" => "space",
        "Enter" => "Return",
        "Backquote" => "grave",
        "ArrowUp" => "Up",
        "ArrowDown" => "Down",
        "ArrowLeft" => "Left",
        "ArrowRight" => "Right",
        value => value.strip_prefix("Key").or_else(|| value.strip_prefix("Digit")).unwrap_or(value),
    };
    parts.push(key.to_owned());
    Some(parts.join("+"))
}
