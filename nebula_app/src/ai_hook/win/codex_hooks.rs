//! Native Codex hooks: bounded capability probe, owned JSON merge and removal.

use std::io::{self, Read, Seek};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{codex_config_dir, helper_path};
use crate::ai_hook::{CodexHookMode, installation};

const MARKER: &str = "hooks.pebrel-managed.json";
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

fn read(path: &Path) -> io::Result<Option<String>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut value = String::new();
    file.take(MAX_CONFIG_BYTES + 1).read_to_string(&mut value)?;
    if value.len() as u64 > MAX_CONFIG_BYTES {
        return Err(io::Error::other("hook configuration exceeds size limit"));
    }
    Ok(Some(value))
}

fn write_changed(path: &Path, value: &str) -> io::Result<bool> {
    if read(path)?.as_deref() == Some(value) {
        return Ok(false);
    }
    crate::atomic_file::write(path, value.as_bytes())?;
    Ok(true)
}

fn probe(args: &[&str]) -> Option<String> {
    use std::os::windows::process::CommandExt as _;
    let mut output = tempfile::tempfile().ok()?;
    let mut child = Command::new("cmd.exe")
        .args(["/d", "/c", "codex"])
        .args(args)
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(output.try_clone().ok()?)
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None)
                if Instant::now() < deadline
                    && output.metadata().is_ok_and(|meta| meta.len() <= 64 * 1024) =>
            {
                std::thread::sleep(Duration::from_millis(10));
            },
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            },
        }
    };
    if !status.success() {
        return None;
    }
    output.rewind().ok()?;
    let mut text = String::new();
    output.take(64 * 1024).read_to_string(&mut text).ok()?;
    Some(text)
}

fn supported_mode() -> Option<CodexHookMode> {
    static CACHE: Mutex<Option<(Instant, Option<CodexHookMode>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|error| error.into_inner());
    if let Some((at, mode)) = *cache
        && at.elapsed() < Duration::from_secs(300)
    {
        return mode;
    }
    let mode = probe(&["--version"])
        .zip(probe(&["features", "list"]))
        .and_then(|(version, features)| installation::codex_mode(&version, &features));
    *cache = Some((Instant::now(), mode));
    mode
}

pub(super) fn ensure_codex_hooks() -> bool {
    let Some(directory) = codex_config_dir().filter(|path| path.is_dir()) else { return false };
    let Some(helper) = helper_path() else { return false };
    let Some(mode) = supported_mode() else { return false };
    let helper = helper.to_string_lossy().replace('\\', "/");
    match install(&directory, &helper, mode) {
        Ok(changed) => {
            if changed {
                log::info!(
                    "ai_hook: Codex native hooks installed; Codex may require review in /hooks"
                );
            }
            changed
        },
        Err(error) => {
            log::warn!("ai_hook: preserving Codex hook configuration: {error}");
            false
        },
    }
}

fn groups(helper: &str, mode: CodexHookMode) -> Value {
    // Codex runs Windows hooks through PowerShell -Command. A quoted path is
    // a string expression until invoked with &, and single quotes keep $, `
    // and other path characters literal. PowerShell escapes ' by doubling it.
    let command = format!("& '{}' codex {}", helper.replace('\'', "''"), mode.argument());
    installation::codex_groups(&command, Some(&command), mode)
}

pub(super) fn installed_at(directory: &Path) -> io::Result<bool> {
    let Some(marker) = read(&directory.join(MARKER))? else { return Ok(false) };
    let marker: Value = serde_json::from_str(&marker)?;
    let previous =
        marker.get("groups").ok_or_else(|| io::Error::other("invalid hook ownership marker"))?;
    let raw = read(&directory.join("hooks.json"))?;
    // 先复用卸载的归属校验，编辑过的自有条目必须在菜单中暴露冲突。
    installation::merge_groups(raw.as_deref(), Some(&previous.to_string()), &json!({}))
        .map_err(io::Error::other)?;
    let current: Value =
        raw.map(|raw| serde_json::from_str(&raw)).transpose()?.unwrap_or_else(|| json!({}));
    Ok(previous.as_object().is_some_and(|events| {
        events.iter().any(|(event, groups)| {
            groups.as_array().is_some_and(|groups| {
                groups.iter().any(|group| {
                    current["hooks"][event]
                        .as_array()
                        .is_some_and(|entries| entries.contains(group))
                })
            })
        })
    }))
}

pub(super) fn current_at(directory: &Path, helper: &str) -> io::Result<bool> {
    current_for_mode(directory, helper, supported_mode())
}

fn current_for_mode(
    directory: &Path,
    helper: &str,
    mode: Option<CodexHookMode>,
) -> io::Result<bool> {
    let Some(mode) = mode else { return Ok(true) };
    let Some(config) = read(&directory.join("config.toml"))? else { return Ok(false) };
    let Some((_, introduced)) =
        installation::enable_codex_feature(&config).map_err(io::Error::other)?
    else {
        // Provider 的显式 opt-out 保持有效，此时 legacy notify 仍可独立工作。
        return Ok(true);
    };
    if introduced {
        return Ok(false);
    }
    let marker: Value = read(&directory.join(MARKER))?
        .map(|raw| serde_json::from_str(&raw))
        .transpose()?
        .unwrap_or_else(|| json!({}));
    let desired = groups(helper, mode);
    let previous = marker.get("groups").map(Value::to_string);
    let raw = read(&directory.join("hooks.json"))?;
    let (updated, _) = installation::merge_groups(raw.as_deref(), previous.as_deref(), &desired)
        .map_err(io::Error::other)?;
    let current: Option<Value> = raw.map(|raw| serde_json::from_str(&raw)).transpose()?;
    Ok(marker.get("groups") == Some(&desired) && current == Some(serde_json::from_str(&updated)?))
}

fn install(directory: &Path, helper: &str, mode: CodexHookMode) -> io::Result<bool> {
    let config_path = directory.join("config.toml");
    let Some(_config_lock) = crate::atomic_file::try_lock(&config_path)? else { return Ok(false) };
    let Some(raw) = read(&config_path)? else { return Ok(false) };
    let Some((config, introduced)) =
        installation::enable_codex_feature(&raw).map_err(io::Error::other)?
    else {
        return Ok(false);
    };
    let hooks_path = directory.join("hooks.json");
    let Some(_lock) = crate::atomic_file::try_lock(&hooks_path)? else { return Ok(false) };
    let marker_path = directory.join(MARKER);
    let previous: Value = read(&marker_path)?
        .map(|raw| serde_json::from_str(&raw))
        .transpose()
        .map_err(io::Error::other)?
        .unwrap_or_else(|| json!({}));
    let groups = groups(helper, mode);
    let previous_groups = previous.get("groups").map(Value::to_string);
    let current_hooks = read(&hooks_path)?;
    let (hooks, _) =
        installation::merge_groups(current_hooks.as_deref(), previous_groups.as_deref(), &groups)
            .map_err(io::Error::other)?;
    let marker = json!({
        "groups":groups,
        "enabled_feature": previous.get("enabled_feature").and_then(Value::as_bool).unwrap_or(false) || introduced,
    });
    let mut changed = write_changed(&hooks_path, &hooks)?;
    changed |= write_changed(&marker_path, &(serde_json::to_string_pretty(&marker)? + "\n"))?;
    changed |= write_changed(&config_path, &config)?;
    Ok(changed)
}

pub(super) fn remove_codex_hooks() -> io::Result<bool> {
    let Some(directory) = codex_config_dir() else { return Ok(false) };
    remove(&directory)
}

fn remove(directory: &Path) -> io::Result<bool> {
    if !directory.join(MARKER).is_file() {
        return Ok(false);
    }
    let Some(_config_lock) = crate::atomic_file::try_lock(&directory.join("config.toml"))? else {
        return Err(io::Error::other("Codex configuration is busy"));
    };
    let marker_path = directory.join(MARKER);
    let Some(marker) = read(&marker_path)? else { return Ok(false) };
    let marker: Value = serde_json::from_str(&marker)?;
    let hooks_path = directory.join("hooks.json");
    let Some(_lock) = crate::atomic_file::try_lock(&hooks_path)? else {
        return Err(io::Error::other("Codex hook configuration is busy"));
    };
    let previous = marker
        .get("groups")
        .ok_or_else(|| io::Error::other("invalid hook ownership marker"))?
        .to_string();
    let (hooks, _) =
        installation::merge_groups(read(&hooks_path)?.as_deref(), Some(&previous), &json!({}))
            .map_err(io::Error::other)?;
    write_changed(&hooks_path, &hooks)?;
    let remaining: Value = serde_json::from_str(&hooks)?;
    if marker.get("enabled_feature").and_then(Value::as_bool) == Some(true)
        && remaining["hooks"].as_object().is_some_and(serde_json::Map::is_empty)
        && let Some(raw) = read(&directory.join("config.toml"))?
    {
        let mut config = raw.parse::<toml_edit::DocumentMut>().map_err(io::Error::other)?;
        // Inline TOML hooks also own the feature, even when hooks.json is empty.
        if config.get("hooks").is_some() {
            std::fs::remove_file(marker_path)?;
            return Ok(true);
        }
        if let Some(features) =
            config.get_mut("features").and_then(toml_edit::Item::as_table_like_mut)
            && features.get("hooks").and_then(toml_edit::Item::as_bool) == Some(true)
        {
            features.remove("hooks");
            write_changed(&directory.join("config.toml"), &config.to_string())?;
        }
    }
    std::fs::remove_file(marker_path)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrades_cmd_style_owned_hooks_without_changing_user_entries() {
        let root = tempfile::tempdir().unwrap();
        let helper = "C:/Program Files/Pebrel/runtime/pebrel-hook.exe";
        let old_command = format!("\"{helper}\" codex --hooks=full");
        let previous =
            installation::codex_groups(&old_command, Some(&old_command), CodexHookMode::Full);
        let foreign = json!({"hooks":[{"type":"command","command":"user-hook"}]});
        let mut hooks = json!({"hooks": previous});
        hooks["hooks"]["Stop"].as_array_mut().unwrap().push(foreign.clone());
        std::fs::write(root.path().join("hooks.json"), hooks.to_string()).unwrap();
        std::fs::write(
            root.path().join(MARKER),
            json!({"groups": previous, "enabled_feature": true}).to_string(),
        )
        .unwrap();
        let config = "notify = ['user-notifier']\n[features]\nhooks = true\n";
        std::fs::write(root.path().join("config.toml"), config).unwrap();

        assert!(!current_for_mode(root.path(), helper, Some(CodexHookMode::Full)).unwrap());
        assert!(install(root.path(), helper, CodexHookMode::Full).unwrap());
        assert!(current_for_mode(root.path(), helper, Some(CodexHookMode::Full)).unwrap());
        assert!(!install(root.path(), helper, CodexHookMode::Full).unwrap());
        let hooks: Value =
            serde_json::from_str(&read(&root.path().join("hooks.json")).unwrap().unwrap()).unwrap();
        let mut expected = json!({"hooks": groups(helper, CodexHookMode::Full)});
        expected["hooks"]["Stop"].as_array_mut().unwrap().insert(0, foreign);
        assert_eq!(hooks, expected);
        assert_eq!(read(&root.path().join("config.toml")).unwrap().unwrap(), config);
    }

    #[test]
    fn powershell_executes_generated_hooks_with_literal_paths_and_stdin() {
        use std::io::Write as _;
        use std::os::windows::process::CommandExt as _;

        let root = tempfile::tempdir().unwrap();
        let helper = root.path().join("Pebrel's $data `hook.ps1");
        std::fs::write(
            &helper,
            "Write-Output ($args -join '|')\r\nWrite-Output ([Console]::In.ReadLine())\r\n",
        )
        .unwrap();
        let powershell = Path::new(&std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        for mode in [CodexHookMode::Turns, CodexHookMode::Full] {
            let groups = groups(&helper.to_string_lossy().replace('\\', "/"), mode);
            let command = groups["Stop"][0]["hooks"][0]["command"].as_str().unwrap();
            let mut child = Command::new(&powershell)
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-Command",
                    command,
                ])
                .creation_flags(0x0800_0000)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(b"{\"session_id\":\"fixture\"}\n").unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert!(stdout.contains(&format!("codex|{}", mode.argument())), "{stdout}");
            assert!(stdout.contains(r#"{"session_id":"fixture"}"#), "{stdout}");
        }
    }

    #[test]
    fn install_upgrade_remove_preserves_notify_and_user_hooks() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config.toml");
        std::fs::write(&config, "# user\nnotify = ['notifier', 'argument']\n").unwrap();
        std::fs::write(
            root.path().join("hooks.json"),
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"user-hook"}]}]}}"#,
        )
        .unwrap();
        assert!(
            install(root.path(), "C:/Program Files/Pebrel/pebrel-hook.exe", CodexHookMode::Turns)
                .unwrap()
        );
        assert!(
            !install(root.path(), "C:/Program Files/Pebrel/pebrel-hook.exe", CodexHookMode::Turns)
                .unwrap()
        );
        assert!(install(root.path(), "D:/new/pebrel-hook.exe", CodexHookMode::Full).unwrap());
        assert!(remove(root.path()).unwrap());
        let hooks: Value =
            serde_json::from_str(&read(&root.path().join("hooks.json")).unwrap().unwrap()).unwrap();
        assert_eq!(hooks["hooks"]["Stop"].as_array().unwrap().len(), 1);
        let config = read(&config).unwrap().unwrap();
        assert!(config.contains("# user") && config.contains("notify = ['notifier', 'argument']"));
        assert!(config.contains("hooks = true"), "another hook still needs the feature");
    }

    #[test]
    fn repeated_toggle_cycles_keep_one_owned_group_and_preserve_the_original_notifier() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config.toml");
        std::fs::write(&config, "# keep\nnotify = ['user-notifier', 'argument']\n").unwrap();
        let foreign = json!({"hooks":[{"type":"command","command":"user-hook"}]});
        std::fs::write(
            root.path().join("hooks.json"),
            json!({"hooks":{"Stop":[foreign]}}).to_string(),
        )
        .unwrap();
        let helper = "C:/Program Files/Pebrel/pebrel-hook.exe";
        for _ in 0..20 {
            assert!(install(root.path(), helper, CodexHookMode::Full).unwrap());
            assert!(installed_at(root.path()).unwrap());
            assert!(current_for_mode(root.path(), helper, Some(CodexHookMode::Full)).unwrap());
            assert!(!install(root.path(), helper, CodexHookMode::Full).unwrap());
            let hooks: Value =
                serde_json::from_str(&read(&root.path().join("hooks.json")).unwrap().unwrap())
                    .unwrap();
            assert_eq!(hooks["hooks"]["Stop"].as_array().unwrap().len(), 2);
            assert!(remove(root.path()).unwrap());
            assert!(!remove(root.path()).unwrap());
            assert!(!installed_at(root.path()).unwrap());
            let hooks: Value =
                serde_json::from_str(&read(&root.path().join("hooks.json")).unwrap().unwrap())
                    .unwrap();
            assert_eq!(hooks, json!({"hooks":{"Stop":[foreign]}}));
            assert!(
                read(&config).unwrap().unwrap().contains("notify = ['user-notifier', 'argument']")
            );
        }
    }

    #[test]
    fn explicit_opt_out_and_edited_hook_survive_automatic_install_and_remove() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("config.toml"), "[features]\nhooks = false\n").unwrap();
        assert!(!install(root.path(), "helper", CodexHookMode::Full).unwrap());
        assert!(!root.path().join("hooks.json").exists());
        std::fs::write(root.path().join("config.toml"), "# original\n").unwrap();
        install(root.path(), "helper", CodexHookMode::Full).unwrap();
        let path = root.path().join("hooks.json");
        let edited = read(&path).unwrap().unwrap().replace("\"timeout\": 3", "\"timeout\": 9");
        std::fs::write(&path, &edited).unwrap();
        assert!(remove(root.path()).is_err());
        assert_eq!(read(&path).unwrap().unwrap(), edited);
    }
}
