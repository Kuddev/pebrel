//! Install/repair scheduling. Provider file edits remain in their installers.
use super::{
    ManagedSkillInstall, claude_config_dir, codex_config_dir, ensure_claude_hooks,
    ensure_codex_hooks, ensure_codex_notify, ensure_opencode_plugin, ensure_pi_extension,
    ensure_runtime_skills, kimi, opencode_config_dir, pi_agent_dir,
};
use std::time::Duration;

// ─── settings self-heal ─────────────────────────────────────────────────

/// Boot entrypoint: install now, then keep installed (see module docs).
pub fn spawn_config_guard() {
    // `setup-ai --remove` 落下的持久开关：用户明确断开过就不再自动
    // 装回（#38 的自愈复发面 / #8 卸载后仍在 hook）。重新启用走
    // `nebula setup-ai`。
    if hooks_disabled() {
        log::info!("ai_hook: ai_hooks=0 (setup-ai --remove); auto-install disabled");
        return;
    }
    if let Err(err) = std::thread::Builder::new().name("pebrel-ai-setup".into()).spawn(config_guard)
    {
        log::warn!("ai_hook: failed to spawn settings guard: {err}");
    }
}

/// `nebula_settings.txt` 里 `ai_hooks=0`（由 `setup-ai --remove` 写入）。
fn hooks_disabled() -> bool {
    nebula_settings::RawSettings::load().bool_on("ai_hooks") == Some(false)
}

/// 一轮完整自愈。每轮都重读开关：`setup-ai --remove` 可能发生在本进程
/// 存活期间，它触发的 config 变更事件会立刻打回这里——不重读就会在
/// 400ms 内把刚移除的接线原样装回（#38 实测的自愈复发路径）。
fn heal_all() {
    if hooks_disabled() {
        return;
    }
    ensure_claude_hooks();
    ensure_codex_notify();
    ensure_codex_hooks();
    kimi::ensure_kimi_hooks();
    ensure_opencode_plugin();
    ensure_pi_extension();
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

    // Neither CLI installed (yet): re-check occasionally instead of
    // watching directories that do not exist.
    let (claude_dir, codex_dir, kimi_dir) = loop {
        let claude = claude_config_dir().filter(|d| d.exists());
        let codex = codex_config_dir().filter(|d| d.exists());
        let kimi = kimi::kimi_config_dir().filter(|d| d.exists());
        if claude.is_some()
            || codex.is_some()
            || kimi.is_some()
            || opencode_config_dir().is_some_and(|d| d.exists())
            || pi_agent_dir().is_some_and(|d| d.exists())
        {
            break (claude, codex, kimi);
        }
        std::thread::sleep(Duration::from_secs(300));
    };

    heal_all();

    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = match notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    }) {
        Ok(watcher) => watcher,
        Err(err) => {
            log::warn!("ai_hook: settings watcher unavailable ({err}); polling instead");
            poll_guard()
        },
    };
    for dir in [&claude_dir, &codex_dir, &kimi_dir].into_iter().flatten() {
        if let Err(err) = watcher.watch(dir, RecursiveMode::NonRecursive) {
            log::warn!("ai_hook: cannot watch {}: {err}; polling instead", dir.display());
            poll_guard();
        }
    }

    loop {
        match rx.recv() {
            Ok(event) => {
                // Only the two config files matter — ~/.codex especially
                // is a busy directory (sessions, sqlite WALs) that would
                // otherwise trigger constant re-checks.
                let relevant = match &event {
                    Ok(ev) => {
                        ev.paths.is_empty()
                            || ev.paths.iter().any(|p| {
                                p.file_name().is_some_and(|n| {
                                    n == "settings.json" || n == "config.toml" || n == "hooks.json"
                                })
                            })
                    },
                    Err(_) => true,
                };
                if !relevant {
                    continue;
                }
                // Debounce the writer's burst, then heal. Our own atomic
                // rename lands here once and heals to a no-op.
                while rx.recv_timeout(Duration::from_millis(400)).is_ok() {}
                heal_all();
            },
            Err(_) => return, // channel closed: shutting down
        }
    }
}

/// Degraded guard when file watching is unavailable: heal every 5 min.
fn poll_guard() -> ! {
    loop {
        std::thread::sleep(Duration::from_secs(300));
        heal_all();
    }
}
