use super::bridges::{OPENCODE_PLUGIN_JS, PI_EXTENSION_TS};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};

use super::{CLAUDE_EVENTS, HELPER_ARGS, contains_helper};

mod codex_hooks;
mod codex_notify;
mod config_guard;
mod kimi;
mod managed_files;
mod runtime_skills;
mod transport;
use codex_hooks::{ensure_codex_hooks, remove_codex_hooks};
use codex_notify::{codex_config_dir, ensure_codex_notify, remove_codex_notify};
pub use config_guard::spawn_config_guard;
use runtime_skills::{
    ManagedSkillInstall, ManagedSkillRemoval, ensure_runtime_skills, remove_runtime_skill,
    runtime_skill_candidates,
};
pub use transport::spawn_gpui_server;
#[cfg(feature = "legacy-shell")]
pub use transport::spawn_server;

static ANNOUNCED: AtomicBool = AtomicBool::new(false);

fn announce() {
    if ANNOUNCED.swap(true, Ordering::Relaxed) {
        return;
    }
    match claim_setup_announcement(&nebula_settings::settings_dir()) {
        Ok(true) => crate::notify::toast(
            "Pebrel",
            crate::i18n::LanguagePreference::from(
                nebula_settings::RuntimeSettings::load().language,
            )
            .resolved()
            .text(crate::i18n::Message::AiHooksAnnounced),
        ),
        Ok(false) => {},
        Err(error) => log::debug!("ai_hook: could not persist setup announcement: {error}"),
    }
}

fn claim_setup_announcement(directory: &Path) -> std::io::Result<bool> {
    std::fs::create_dir_all(directory)?;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("ai-hooks-announced"))
    {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
    }
}

// ─── opencode plugin (~/.config/opencode/plugins/nebula.js) ─────────────

/// The Nebula↔opencode bridge, auto-dropped into opencode's global plugin
/// dir. opencode is a Bun app that auto-loads `{plugin,plugins}/*.js`; this
/// plugin subscribes to its event bus and shells out to nebula-hook.exe
/// (path in `NEBULA_HOOK_EXE`, pipe in the inherited `NEBULA_NOTIFY_PIPE`),
/// normalizing events into the small payload `parse_envelope` reads. The
/// send chain serializes delivery: Bun waits for one helper to close before
/// starting the next, so a delayed processing edge cannot overtake idle.
/// A 3 s watchdog releases the chain if a helper hangs — otherwise one stuck
/// process would swallow every later event, including the final idle.

// Pi 官方扩展 API 在 agent_start/agent_end 提供稳定的回合边界。扩展只做
// fire-and-forget 转发，且 NEBULA_HOOK_EXE 不存在时完全静默，因此全局安装
// 不会影响从其他终端启动的 Pi。

/// Idempotently install/heal our hook entries in claude's settings.json.
/// Returns whether the file was modified.
pub fn ensure_claude_hooks() -> bool {
    let Some(dir) = claude_config_dir() else { return false };
    if !dir.exists() {
        return false; // no claude footprint → nothing to install into
    }
    let Some(command) = helper_command() else { return false };

    let path = dir.join("settings.json");
    let mut root: Value = match std::fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str(&raw) {
            Ok(json) => json,
            Err(err) => {
                // Mid-rewrite by a concurrent writer, or genuinely broken:
                // never "repair" by clobbering. The watcher retries on the
                // next change, the boot pass on the next start.
                log::warn!("ai_hook: {} is not valid JSON ({err}); left alone", path.display());
                return false;
            },
        },
        Err(_) => json!({}),
    };

    let Some(changed) = install_into(&mut root, &command) else {
        log::warn!("ai_hook: {} has an unexpected shape; left alone", path.display());
        return false;
    };
    if !changed {
        return false;
    }

    // First modification keeps a pristine copy next to the original.
    if path.exists() {
        let bak = path.with_extension("json.pebrel-bak");
        if !bak.exists() {
            if let Err(err) = std::fs::copy(&path, &bak) {
                log::warn!("ai_hook: backup failed ({err}); not touching {}", path.display());
                return false;
            }
        }
    }
    let Ok(raw) = serde_json::to_string_pretty(&root) else { return false };
    match write_atomic(&path, &raw) {
        Ok(()) => {
            log::info!("ai_hook: claude hooks installed into {}", path.display());
            announce();
            true
        },
        Err(err) => {
            log::warn!("ai_hook: failed to write {}: {err}", path.display());
            false
        },
    }
}

/// Pure JSON surgery: ensure each subscribed event carries exactly one
/// nebula-hook command, healing a stale absolute path in place. `None`
/// means the document's shape is not what claude documents — refuse.
fn install_into(root: &mut Value, command: &str) -> Option<bool> {
    let obj = root.as_object_mut()?;
    let hooks = obj.entry("hooks").or_insert_with(|| json!({})).as_object_mut()?;
    let mut changed = false;
    for event in CLAUDE_EVENTS {
        let matchers = hooks.entry(event).or_insert_with(|| json!([])).as_array_mut()?;
        let mut found = false;
        for matcher in matchers.iter_mut() {
            let Some(cmds) = matcher.get_mut("hooks").and_then(Value::as_array_mut) else {
                continue;
            };
            for cmd in cmds {
                let ours = cmd.get("command").and_then(Value::as_str).is_some_and(contains_helper);
                if !ours {
                    continue;
                }
                found = true;
                let Some(entry) = cmd.as_object_mut() else { continue };
                if entry.get("command").and_then(Value::as_str) != Some(command) {
                    entry.insert("command".into(), json!(command));
                    changed = true;
                }
                // 1.4.0 及更早写的是 shell 形式（引号路径 + 拼在字符串里的
                // 子命令）。healing 必须补上 args：只改 command 会留下一条
                // 没有 argv 的裸路径，claude 仍旧交给 shell 解析（#80）。
                if entry.get("args") != Some(&json!(HELPER_ARGS)) {
                    entry.insert("args".into(), json!(HELPER_ARGS));
                    changed = true;
                }
            }
        }
        if !found {
            matchers.push(json!({
                "hooks": [{
                    "type": "command",
                    "command": command,
                    "args": HELPER_ARGS,
                    "timeout": 10,
                }]
            }));
            changed = true;
        }
    }
    Some(changed)
}

/// Strip every nebula-hook entry (and matchers left empty by that).
fn remove_hooks() -> std::io::Result<bool> {
    let Some(dir) = claude_config_dir() else { return Ok(false) };
    let path = dir.join("settings.json");
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(_) => return Ok(false),
    };
    let mut root: Value =
        serde_json::from_str(&raw).map_err(|e| std::io::Error::other(e.to_string()))?;
    let mut changed = false;
    if let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) {
        for event in CLAUDE_EVENTS {
            let Some(matchers) = hooks.get_mut(event).and_then(Value::as_array_mut) else {
                continue;
            };
            for matcher in matchers.iter_mut() {
                if let Some(cmds) = matcher.get_mut("hooks").and_then(Value::as_array_mut) {
                    let before = cmds.len();
                    cmds.retain(|c| {
                        !c.get("command").and_then(Value::as_str).is_some_and(contains_helper)
                    });
                    changed |= cmds.len() != before;
                }
            }
            let before = matchers.len();
            matchers
                .retain(|m| m.get("hooks").and_then(Value::as_array).is_none_or(|c| !c.is_empty()));
            changed |= matchers.len() != before;
        }
    }
    if changed {
        write_atomic(&path, &serde_json::to_string_pretty(&root)?)?;
    }
    Ok(changed)
}

/// `nebula setup-ai [--remove]` entrypoint (console attached in `main`).
pub fn setup_ai_cli(remove: bool) -> i32 {
    let Some(dir) = claude_config_dir() else {
        eprintln!("找不到用户目录（USERPROFILE / CLAUDE_CONFIG_DIR）。");
        return 1;
    };
    let path = dir.join("settings.json");
    if remove {
        let mut failed = false;
        match remove_hooks() {
            Ok(true) => println!("claude: 已从 {} 移除 hooks。", path.display()),
            Ok(false) => println!("claude: {} 中没有 Pebrel 的 hooks。", path.display()),
            Err(err) => {
                eprintln!("claude: 移除失败：{err}");
                failed = true;
            },
        }
        if let Err(error) = remove_codex_hooks() {
            eprintln!("codex: 原生 hook 移除失败，已保留配置：{error}");
            failed = true;
        }
        match remove_codex_notify() {
            Ok(true) => println!("codex: 已还原 config.toml 的 notify。"),
            Ok(false) => println!("codex: notify 不是 Pebrel 接管的，未改动。"),
            Err(err) => {
                eprintln!("codex: 还原失败：{err}");
                failed = true;
            },
        }
        match kimi::remove_kimi_hooks() {
            Ok(true) => println!("kimi: 已从 config.toml 移除 hooks。"),
            Ok(false) => println!("kimi: config.toml 中没有 Pebrel 的 hooks。"),
            Err(err) => {
                eprintln!("kimi: 移除失败：{err}");
                failed = true;
            },
        }
        match remove_opencode_plugin() {
            Ok(true) => println!("opencode: 已删除 Pebrel 管理的插件。"),
            Ok(false) => println!("opencode: 没有 Pebrel 的插件，未改动。"),
            Err(err) => {
                eprintln!("opencode: 删除失败：{err}");
                failed = true;
            },
        }
        match remove_pi_extension() {
            Ok(true) => println!("pi: 已删除 Pebrel 管理的扩展。"),
            Ok(false) => println!("pi: 没有 Pebrel 的扩展，未改动。"),
            Err(err) => {
                eprintln!("pi: 删除失败：{err}");
                failed = true;
            },
        }
        for (agent, path) in runtime_skill_candidates() {
            match remove_runtime_skill(&path) {
                Ok(ManagedSkillRemoval::Removed) => {
                    println!("{agent}: 已移除 Pebrel Runtime Skill（{}）。", path.display())
                },
                Ok(ManagedSkillRemoval::Absent) => {
                    println!("{agent}: 没有 Pebrel 管理的 Runtime Skill，未改动。")
                },
                Ok(ManagedSkillRemoval::Conflict) => {
                    eprintln!(
                        "{agent}: {} 已被用户修改，保留该 Skill；如需删除请手动确认内容。",
                        path.display()
                    );
                    failed = true;
                },
                Err(error) => {
                    eprintln!("{agent}: 移除 Runtime Skill 失败：{error}");
                    failed = true;
                },
            }
        }
        // 持久开关：不写它，下次 Nebula 启动（含开机自启）会把上面
        // 刚清掉的四处原样装回——移除必须比自愈活得久（#8、#38）。
        match nebula_settings::persist_keys(&[("ai_hooks", "0".to_owned())]) {
            Ok(()) => println!(
                "已写入 ai_hooks=0：Pebrel 启动时不再自动接线（重新启用：pebrel setup-ai）。"
            ),
            Err(err) => {
                eprintln!("警告：无法写入 ai_hooks=0（{err}），下次启动仍会自动装回。");
                failed = true;
            },
        }
        // 卸载器必须尽最大努力清理所有集成，不能因一个损坏的用户配置
        // 提前返回而让其他 Hook 永久指向即将被删除的程序目录。
        return i32::from(failed);
    }
    match helper_command() {
        Some(command) => {
            println!("hook 命令：{command} {}（exec 形式，不经 shell 解析）", HELPER_ARGS[0])
        },
        None => {
            eprintln!("runtime/ 和 pebrel.exe 同目录中均未找到 pebrel-hook.exe，无法安装。");
            return 1;
        },
    }
    let mut setup_failed = false;
    // 显式安装即重新授权：清掉 --remove 落下的持久开关，守护线程下次
    // 启动恢复自愈。
    if let Err(err) = nebula_settings::persist_keys(&[("ai_hooks", "1".to_owned())]) {
        eprintln!(
            "警告：无法写入 ai_hooks=1（{err}）；若之前执行过 --remove，自动接线仍是关闭状态。"
        );
    }
    if dir.exists() {
        if ensure_claude_hooks() {
            println!("claude: 已写入 {}（首次改动备份 *.pebrel-bak）。", path.display());
        } else {
            println!("claude: {} 已是最新。", path.display());
        }
    } else {
        println!("claude: 未检测到（{} 不存在），跳过。", dir.display());
    }
    match codex_config_dir().map(|d| d.join("config.toml")) {
        Some(cfg) if cfg.exists() => {
            if ensure_codex_hooks() {
                println!(
                    "codex: 已写入原生生命周期 hook；请在 Codex /hooks 中审阅后启用。旧 notify 保留兼容。"
                );
            }
            if ensure_codex_notify() {
                println!("codex: 已接管 notify（原 notifier 经 --chain 保留）。");
            } else {
                println!("codex: {} 已是最新。", cfg.display());
            }
        },
        _ => println!("codex: 未检测到 config.toml，跳过。"),
    }
    match kimi::kimi_config_dir() {
        Some(cfg_dir) if cfg_dir.exists() => {
            let cfg = cfg_dir.join("config.toml");
            if kimi::ensure_kimi_hooks() {
                println!("kimi: 已写入 {}（首次改动备份 *.pebrel-bak）。", cfg.display());
            } else {
                println!("kimi: {} 已是最新。", cfg.display());
            }
        },
        _ => println!("kimi: 未检测到（~/.kimi-code 不存在），跳过。"),
    }
    match opencode_config_dir() {
        Some(cfg) if cfg.exists() => {
            let dir = cfg.join("plugins");
            setup_failed |= report_cli_bridge_install("opencode", &dir, Bridge::Opencode);
        },
        _ => println!("opencode: 未检测到（~/.config/opencode 不存在），跳过。"),
    }
    match pi_agent_dir() {
        Some(agent) if agent.exists() => {
            let dir = agent.join("extensions");
            setup_failed |= report_cli_bridge_install("pi", &dir, Bridge::Pi);
        },
        _ => println!("pi: 未检测到（~/.pi/agent 不存在），跳过。"),
    }
    for (agent, path, result) in ensure_runtime_skills() {
        match result {
            Ok(ManagedSkillInstall::Installed) => {
                println!("{agent}: 已安装 Pebrel Runtime Skill 到 {}。", path.display())
            },
            Ok(ManagedSkillInstall::Current) => {
                println!("{agent}: Pebrel Runtime Skill 已是最新。")
            },
            Ok(ManagedSkillInstall::Conflict) => {
                eprintln!(
                    "{agent}: {} 或旧目录存在非 Pebrel 管理或被编辑的 Skill，未覆盖。",
                    path.display()
                );
                setup_failed = true;
            },
            Err(error) => {
                eprintln!("{agent}: 安装 Runtime Skill 失败：{error}");
                setup_failed = true;
            },
        }
    }
    println!("对新启动的会话生效；正在运行的会话保持原快照。");
    i32::from(setup_failed)
}

fn report_cli_bridge_install(agent: &str, directory: &Path, bridge: Bridge) -> bool {
    let path = directory.join(bridge.files().0);
    match install_bridge(directory, bridge) {
        Ok(managed_files::Install::Installed) => {
            println!("{agent}: 已安装 {}。", path.display());
            announce();
            false
        },
        Ok(managed_files::Install::Current) => {
            println!("{agent}: {} 已是最新。", path.display());
            false
        },
        Ok(managed_files::Install::Conflict) => {
            eprintln!("{agent}: {} 或旧文件已被编辑或属于用户，未覆盖。", path.display());
            true
        },
        Err(error) => {
            eprintln!("{agent}: 安装 {} 失败：{error}", path.display());
            true
        },
    }
}

fn claude_config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    Some(PathBuf::from(std::env::var_os("USERPROFILE")?).join(".claude"))
}

// ─── opencode plugin (~/.config/opencode/plugins/nebula.js) ─────────────

/// opencode's global config dir. It uses `xdg-basedir`, which on Windows
/// resolves `$XDG_CONFIG_HOME` else `~/.config` (NOT %APPDATA%), so mirror
/// that exactly or the plugin lands where opencode never looks.
fn opencode_config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(dir).join("opencode"));
    }
    Some(PathBuf::from(std::env::var_os("USERPROFILE")?).join(".config").join("opencode"))
}

/// Drop our event-forwarding plugin into opencode's global plugin dir.
/// Unlike claude/codex, opencode never rewrites files under its own plugin
/// dir, so no self-heal watcher is needed — a write-if-changed on boot
/// suffices (and heals a stale copy after a Nebula upgrade). Only writes
/// when opencode is actually installed. Returns whether it wrote.
pub fn ensure_opencode_plugin() -> bool {
    // Only act when opencode exists — don't scaffold its config tree.
    let Some(cfg) = opencode_config_dir().filter(|d| d.exists()) else { return false };
    let dir = cfg.join("plugins");
    report_bridge_install(&dir.join("pebrel.js"), install_bridge(&dir, Bridge::Opencode))
}

/// Undo [`ensure_opencode_plugin`]: delete the plugin file if it is ours.
fn remove_opencode_plugin() -> std::io::Result<bool> {
    let Some(cfg) = opencode_config_dir() else { return Ok(false) };
    remove_bridge(&cfg.join("plugins"), Bridge::Opencode)
}

// ─── Pi extension (~/.pi/agent/extensions/nebula.ts) ───────────────────

fn pi_agent_dir() -> Option<PathBuf> {
    if let Some(directory) = std::env::var_os("PI_CODING_AGENT_DIR") {
        return Some(PathBuf::from(directory));
    }
    Some(PathBuf::from(std::env::var_os("USERPROFILE")?).join(".pi").join("agent"))
}

/// Install the bridge only when Pi already has a global agent directory;
/// Nebula must not create a fake Pi footprint for users who do not use it.
pub fn ensure_pi_extension() -> bool {
    let Some(agent) = pi_agent_dir().filter(|dir| dir.exists()) else { return false };
    let dir = agent.join("extensions");
    report_bridge_install(&dir.join("pebrel.ts"), install_bridge(&dir, Bridge::Pi))
}

fn remove_pi_extension() -> std::io::Result<bool> {
    let Some(agent) = pi_agent_dir() else { return Ok(false) };
    remove_bridge(&agent.join("extensions"), Bridge::Pi)
}

#[derive(Clone, Copy)]
enum Bridge {
    Opencode,
    Pi,
}

impl Bridge {
    fn files(self) -> (&'static str, &'static str, &'static str, &'static [&'static str]) {
        // Exact embedded payloads verified from v1.0.0 through v1.5.0.
        match self {
            Self::Opencode => (
                "pebrel.js",
                "nebula.js",
                OPENCODE_PLUGIN_JS,
                &[
                    "f42225dac77b7f9e577b6a025309c44f8b35a830a60dee475eefc68f00c30190",
                    "e81481ed990d205911f22096a34aff5bbbda3d220450a3b39230b121ca158075",
                    "5f155e7330a9ef51c5ad1a048e27bedf6f48ade94e0bbe0624d76e632b545a06",
                ],
            ),
            Self::Pi => (
                "pebrel.ts",
                "nebula.ts",
                PI_EXTENSION_TS,
                &[
                    "52a13a3a39114a9ca1ddb1e224712449124a532a21e9a57e6627a89d2ae02302",
                    "496680cbec44d1f4b60f2138ec86b8fe453a74e974507867cb72736c0ac00766",
                    "50e81b910107150fd4c7e47064ab2b78e4fc6dfca4484d8ad3d64f17a0a5fb7e",
                ],
            ),
        }
    }
}

fn install_bridge(dir: &Path, bridge: Bridge) -> std::io::Result<managed_files::Install> {
    let (name, legacy, content, hashes) = bridge.files();
    managed_files::install(&dir.join(name), &dir.join(legacy), content, hashes)
}

fn remove_bridge(dir: &Path, bridge: Bridge) -> std::io::Result<bool> {
    let (name, legacy, content, hashes) = bridge.files();
    managed_files::remove(&dir.join(name), &dir.join(legacy), content, hashes)
}

fn report_bridge_install(path: &Path, result: std::io::Result<managed_files::Install>) -> bool {
    match result {
        Ok(managed_files::Install::Installed) => {
            log::info!("ai_hook: installed bridge at {}", path.display());
            announce();
            true
        },
        Ok(managed_files::Install::Current) => false,
        Ok(managed_files::Install::Conflict) => {
            log::warn!("ai_hook: preserving edited or unmanaged bridge near {}", path.display());
            false
        },
        Err(error) => {
            log::warn!("ai_hook: failed to install bridge at {}: {error}", path.display());
            false
        },
    }
}

/// Absolute path of the bridge exe.
///
/// The helper is an optional runtime asset, so a development checkout or
/// an incomplete standalone directory is a valid state. `helper_path()`
/// is called from several self-healing paths and from the pipe bootstrap;
/// logging on every probe turns that state into an apparent infinite
/// warning loop when a config watcher is busy. Keep the state transition
/// noisy once, but make repeated probes silent until the helper is found
/// again (or removed later).
static HELPER_MISSING_ANNOUNCED: AtomicBool = AtomicBool::new(false);

fn helper_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let helper = helper_path_from_exe(&exe);
    match helper {
        Some(path) => {
            // Permit a later installation/removal to be reported once on
            // the next state transition rather than caching a stale path.
            HELPER_MISSING_ANNOUNCED.store(false, Ordering::Relaxed);
            Some(path)
        },
        None => {
            if !HELPER_MISSING_ANNOUNCED.swap(true, Ordering::Relaxed) {
                log::warn!(
                    "ai_hook: pebrel-hook.exe missing from runtime/ and executable directory; AI integrations not installed"
                );
            }
            None
        },
    }
}

fn helper_path_from_exe(exe: &Path) -> Option<PathBuf> {
    let exe_dir = exe.parent()?;
    // 新包优先使用分类目录，旧同目录位置仅用于开发构建和兼容历史包。
    [
        exe_dir.join("runtime").join("pebrel-hook.exe"),
        exe_dir.join("pebrel-hook.exe"),
        exe_dir.join("runtime").join("nebula-hook.exe"),
        exe_dir.join("nebula-hook.exe"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

/// The hook entry's `command`: nothing but the helper's absolute path.
///
/// Claude runs a hook in *exec form* whenever the entry carries `args` —
/// it spawns the executable directly, so no shell ever re-parses the path.
/// That is the only shape that holds on Windows: claude routes shell-form
/// hooks through PowerShell (or Git Bash / cmd, depending on version and
/// per-hook `shell`), and PowerShell parses a leading quoted token as a
/// *string expression* — `"C:/…/nebula-hook.exe" claude` therefore dies
/// with `UnexpectedToken: claude` (#80). The garbled text next to that
/// error is the same failure: PowerShell writes its localized parser
/// message in the console codepage and claude reads it back as UTF-8.
///
/// Forward slashes stay: `CreateProcess` accepts them and they keep the
/// entry readable when the user opens `settings.json`.
fn helper_command() -> Option<String> {
    Some(helper_path()?.display().to_string().replace('\\', "/"))
}

/// Write via tmp + rename (MoveFileEx REPLACE_EXISTING under the hood):
/// readers never observe a torn file, a crash leaves the original intact.
fn write_atomic(path: &Path, data: &str) -> std::io::Result<()> {
    // 临时文件名带上进程号。多个 Nebula 实例各自守着同一份
    // `settings.json` 自愈，共用一个固定的 tmp 名就会互相踩：A 写 tmp、
    // B 覆盖同一个 tmp、A `rename` 把它搬走，B 的 `rename` 于是报
    // `ERROR_FILE_NOT_FOUND(2)`——一句"系统找不到指定的文件"，指的却是
    // 那个临时文件，读起来像 settings.json 不见了。
    //
    // 内容本身是幂等的（装的是同一套 hook 条目），所以最后谁赢都行，
    // 要防的只是这个假报错。
    let tmp = path.with_extension(format!("pebrel-tmp-{}", std::process::id()));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod generated_hook_tests {
    use serde_json::json;

    use super::{CLAUDE_EVENTS, OPENCODE_PLUGIN_JS, PI_EXTENSION_TS, install_into};

    const HELPER: &str = "C:/Program Files/Pebrel/runtime/pebrel-hook.exe";

    #[test]
    fn claude_install_includes_permission_requests_and_remains_idempotent() {
        let mut root = json!({});
        assert_eq!(install_into(&mut root, HELPER), Some(true));
        assert!(CLAUDE_EVENTS.contains(&"PermissionRequest"));
        for event in CLAUDE_EVENTS {
            assert_eq!(root["hooks"][event].as_array().map(Vec::len), Some(1));
        }
        assert_eq!(install_into(&mut root, HELPER), Some(false));
    }

    /// #80: the command string used to be `"<path>" claude`, and claude
    /// hands hook strings to a shell. PowerShell reads the leading quoted
    /// token as a string expression, so `claude` became an unexpected
    /// token and every hook failed. Exec form (`command` + `args`) is
    /// spawned directly, so no shell parses the path at all.
    #[test]
    fn claude_hooks_use_exec_form_so_no_shell_parses_the_path() {
        let mut root = json!({});
        assert_eq!(install_into(&mut root, HELPER), Some(true));
        for event in CLAUDE_EVENTS {
            let entry = &root["hooks"][event][0]["hooks"][0];
            assert_eq!(entry["type"], json!("command"));
            assert_eq!(entry["command"], json!(HELPER), "command 必须是可直接 spawn 的路径");
            assert_eq!(entry["args"], json!(["claude"]), "子命令必须走 argv");
            let command = entry["command"].as_str().expect("command is a string");
            assert!(!command.contains('"'), "exec form 不能带引号：{command}");
            assert!(!command.contains(" claude"), "子命令不能拼进命令字符串：{command}");
        }
    }

    /// 1.4.0 装出去的坏条目必须被就地修好，而不是再追加一条——两条 hook
    /// 会让每个事件上报两次。
    #[test]
    fn legacy_shell_form_entries_are_healed_in_place() {
        let mut root = json!({
            "hooks": {
                "SessionStart": [{
                    "hooks": [{
                        "type": "command",
                        "command": "\"D:/old/Nebula/runtime/nebula-hook.exe\" claude",
                        "timeout": 10,
                    }]
                }]
            }
        });
        assert_eq!(install_into(&mut root, HELPER), Some(true));
        let start = root["hooks"]["SessionStart"].as_array().expect("matchers");
        assert_eq!(start.len(), 1, "不得为同一事件追加第二条 hook");
        let entry = &start[0]["hooks"][0];
        assert_eq!(entry["command"], json!(HELPER));
        assert_eq!(entry["args"], json!(["claude"]));
        assert_eq!(entry["timeout"], json!(10), "既有字段不能被 healing 丢掉");
        assert_eq!(install_into(&mut root, HELPER), Some(false));
    }

    #[test]
    fn generated_plugins_carry_ordering_metadata() {
        assert!(OPENCODE_PLUGIN_JS.contains("let sendChain = Promise.resolve()"));
        assert!(OPENCODE_PLUGIN_JS.contains("const sequenceEpoch = BigInt(Date.now())"));
        assert!(OPENCODE_PLUGIN_JS.contains("\"permission.ask\": async (input)"));
        assert!(OPENCODE_PLUGIN_JS.contains("reportPermission(input)"));
        assert!(PI_EXTENSION_TS.contains("getSessionFile"));
        assert!(PI_EXTENSION_TS.contains("const sequenceEpoch = BigInt(Date.now())"));
        assert!(PI_EXTENSION_TS.contains("event_id"));
        for source in [OPENCODE_PLUGIN_JS, PI_EXTENSION_TS] {
            assert!(source.contains("process.env.PEBREL_HOOK_EXE ?? process.env.NEBULA_HOOK_EXE"));
        }
    }

    #[test]
    fn verified_legacy_plugins_migrate_to_a_single_current_bridge() {
        use super::{Bridge, install_bridge, managed_files};

        let temp = tempfile::tempdir().unwrap();
        // Fixed payloads from v1.5.0: deriving an "old" plugin from today's
        // implementation invents bytes that no released version ever owned.
        for (bridge, legacy) in [
            (
                Bridge::Opencode,
                include_str!("../../../scripts/tests/fixtures/ai-hooks-v1.5.0/opencode.js"),
            ),
            (Bridge::Pi, include_str!("../../../scripts/tests/fixtures/ai-hooks-v1.5.0/pi.ts")),
        ] {
            let (name, legacy_name, source, _) = bridge.files();
            std::fs::write(temp.path().join(legacy_name), legacy).unwrap();
            assert_eq!(
                install_bridge(temp.path(), bridge).unwrap(),
                managed_files::Install::Installed
            );
            assert!(!temp.path().join(legacy_name).exists());
            assert_eq!(std::fs::read_to_string(temp.path().join(name)).unwrap(), source);
            assert_eq!(
                install_bridge(temp.path(), bridge).unwrap(),
                managed_files::Install::Current
            );
        }
    }
}

#[cfg(test)]
mod setup_announcement_tests {
    use super::claim_setup_announcement;

    #[test]
    fn setup_announcement_survives_restarts_and_repairs() {
        let directory = tempfile::tempdir().unwrap();
        let settings = directory.path().join("settings");
        assert!(claim_setup_announcement(&settings).unwrap());
        for _ in 0..4 {
            assert!(!claim_setup_announcement(&settings).unwrap());
        }
    }

    #[test]
    fn concurrent_windows_only_claim_one_setup_announcement() {
        let directory = tempfile::tempdir().unwrap();
        let announced = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..8)
                .map(|_| scope.spawn(|| claim_setup_announcement(directory.path()).unwrap()))
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .filter(|claimed| *claimed)
                .count()
        });
        assert_eq!(announced, 1);
    }

    #[test]
    fn setup_announcement_does_not_ignore_storage_errors() {
        let directory = tempfile::tempdir().unwrap();
        let not_a_directory = directory.path().join("file");
        std::fs::write(&not_a_directory, b"").unwrap();
        assert!(claim_setup_announcement(&not_a_directory).is_err());
    }
}

#[cfg(test)]
mod runtime_asset_tests {
    use super::helper_path_from_exe;

    #[test]
    fn hook_helper_prefers_runtime_directory() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("pebrel.exe");
        let runtime = dir.path().join("runtime");
        std::fs::create_dir(&runtime).unwrap();
        std::fs::write(dir.path().join("nebula-hook.exe"), b"legacy").unwrap();
        std::fs::write(runtime.join("nebula-hook.exe"), b"structured").unwrap();
        std::fs::write(runtime.join("pebrel-hook.exe"), b"current").unwrap();

        assert_eq!(helper_path_from_exe(&exe), Some(runtime.join("pebrel-hook.exe")));
    }

    #[test]
    fn hook_helper_falls_back_to_legacy_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("nebula.exe");
        let legacy = dir.path().join("nebula-hook.exe");
        std::fs::write(&legacy, b"legacy").unwrap();

        assert_eq!(helper_path_from_exe(&exe), Some(legacy));
    }
}
