//! Install/repair scheduling. Provider file edits remain in their installers.
use super::{ManagedSkillInstall, ensure_runtime_skills, settings};
use nebula_settings::{AgentHook, RawSettings};
use std::collections::HashSet;
use std::time::Duration;

pub fn spawn_config_guard() {
    if let Err(err) = std::thread::Builder::new().name("pebrel-ai-setup".into()).spawn(config_guard)
    {
        log::warn!("ai_hook: failed to spawn settings guard: {err}");
    }
}

fn heal_all() {
    let Ok(_lock) = settings::lock() else { return };
    // 锁内重读授权，不能把菜单/卸载刚移除的 Hook 按旧快照装回。
    let raw = match RawSettings::try_load() {
        Ok(raw) => raw,
        Err(error) => {
            log::warn!("ai_hook: cannot read hook preferences; installation skipped: {error}");
            return;
        },
    };
    settings::heal_enabled(&raw);
    if raw.bool_on("ai_hooks") == Some(false) {
        return;
    }
    for (agent, path, result) in ensure_runtime_skills() {
        match result {
            Ok(ManagedSkillInstall::Installed) => {
                log::info!("ai_hook: installed {agent} runtime skill at {}", path.display())
            },
            Ok(ManagedSkillInstall::Current) => {},
            Ok(ManagedSkillInstall::Conflict) => log::warn!(
                "ai_hook: preserving unmanaged or edited {agent} skill at {}",
                path.display()
            ),
            Err(error) => log::warn!(
                "ai_hook: failed to install {agent} runtime skill at {}: {error}",
                path.display()
            ),
        }
    }
}

fn config_guard() {
    use notify::{RecursiveMode, Watcher};
    use std::sync::mpsc::{RecvTimeoutError, sync_channel};

    heal_all();
    // 配置事件只代表需要重读；合并成一个信号，避免繁忙目录堆积整批事件。
    let (tx, rx) = sync_channel(1);
    let mut watcher =
        match notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            let relevant = result.as_ref().map_or(true, |event| {
                event.paths.is_empty()
                    || event.paths.iter().any(|path| {
                        path.file_name().is_some_and(|name| {
                            name == "settings.json"
                                || name == "config.toml"
                                || name == "hooks.json"
                                || name == "pebrel.js"
                                || name == "pebrel.ts"
                                || name == "pebrel.json"
                                || name == "pebrel_settings.txt"
                                || name == "nebula_settings.txt"
                        })
                    })
            });
            if relevant {
                let _ = tx.try_send(());
            }
        }) {
            Ok(watcher) => watcher,
            Err(error) => {
                log::warn!("ai_hook: settings watcher unavailable ({error}); polling instead");
                poll_guard()
            },
        };
    let mut watched = HashSet::new();
    loop {
        let dirs = std::iter::once(nebula_settings::settings_dir()).chain(
            AgentHook::ALL.into_iter().filter_map(|agent| {
                settings::configuration(agent)?.0.parent().map(std::path::Path::to_path_buf)
            }),
        );
        for dir in dirs.filter(|dir| dir.is_dir()) {
            if !watched.contains(&dir) {
                match watcher.watch(&dir, RecursiveMode::NonRecursive) {
                    Ok(()) => {
                        watched.insert(dir);
                    },
                    Err(error) => log::debug!("ai_hook: cannot watch {}: {error}", dir.display()),
                }
            }
        }
        match rx.recv_timeout(Duration::from_secs(300)) {
            Ok(()) => while rx.recv_timeout(Duration::from_millis(400)).is_ok() {},
            Err(RecvTimeoutError::Timeout) => {},
            Err(RecvTimeoutError::Disconnected) => return,
        }
        // 定时一轮兼顾启动后新安装的 Agent，以及未能建立 watcher 的目录。
        heal_all();
    }
}

fn poll_guard() -> ! {
    loop {
        std::thread::sleep(Duration::from_secs(300));
        heal_all();
    }
}
