//! Codex legacy notify installation, chaining and reversible removal.
use super::{announce, helper_path, write_atomic};
use crate::ai_hook::installation::{desired_codex_notify, stripped_codex_notify};
use std::path::PathBuf;

// ─── codex notify (config.toml) ─────────────────────────────────────────

/// Codex home: `$CODEX_HOME`, else `~/.codex`.
pub(super) fn codex_config_dir() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("CODEX_HOME") {
        return Some(PathBuf::from(home));
    }
    Some(PathBuf::from(std::env::var_os("USERPROFILE")?).join(".codex"))
}

/// Wire codex's `notify` to nebula-hook. Codex has a SINGLE notify slot
/// which may already be taken (e.g. OpenAI's own computer-use notifier),
/// so an occupied slot is wrapped, not evicted: nebula-hook forwards to
/// the pipe and then invokes the original program via `--chain` with the
/// same payload. toml_edit keeps the file's formatting and comments.
/// Idempotent; heals a moved helper path. Returns whether it wrote.
pub fn ensure_codex_notify() -> bool {
    let Some(path) = codex_config_dir().map(|d| d.join("config.toml")) else { return false };
    if !path.is_file() {
        return false;
    }
    let Ok(Some(_lock)) = crate::atomic_file::try_lock(&path) else { return false };
    let Ok(raw) = std::fs::read_to_string(&path) else { return false }; // no codex → skip
    let Some(helper) = helper_path() else { return false };
    let helper = helper.display().to_string().replace('\\', "/");

    let Ok(mut doc) = raw.parse::<toml_edit::DocumentMut>() else {
        log::warn!("ai_hook: {} is not valid TOML; left alone", path.display());
        return false;
    };

    let current: Vec<String> = doc
        .get("notify")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|i| i.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();

    let Some(desired) = desired_codex_notify(&current, &helper) else {
        return false;
    };

    let mut array = toml_edit::Array::new();
    for arg in &desired {
        array.push(arg.as_str());
    }
    doc["notify"] = toml_edit::value(array);

    let bak = path.with_extension("toml.pebrel-bak");
    if !bak.exists() {
        if let Err(err) = std::fs::copy(&path, &bak) {
            log::warn!("ai_hook: backup failed ({err}); not touching {}", path.display());
            return false;
        }
    }
    match write_atomic(&path, &doc.to_string()) {
        Ok(()) => {
            log::info!("ai_hook: codex notify wired in {}", path.display());
            announce();
            true
        },
        Err(err) => {
            log::warn!("ai_hook: failed to write {}: {err}", path.display());
            false
        },
    }
}

/// Undo [`ensure_codex_notify`]: restore a wrapped notifier from the
/// `--chain` tail, or drop the key entirely when we created it.
pub(super) fn remove_codex_notify() -> std::io::Result<bool> {
    let Some(path) = codex_config_dir().map(|d| d.join("config.toml")) else {
        return Ok(false);
    };
    if !path.is_file() {
        return Ok(false);
    }
    let Some(_lock) = crate::atomic_file::try_lock(&path)? else {
        return Err(std::io::Error::other("Codex configuration is busy"));
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(_) => return Ok(false),
    };
    let mut doc =
        raw.parse::<toml_edit::DocumentMut>().map_err(|e| std::io::Error::other(e.to_string()))?;
    let current: Vec<String> = doc
        .get("notify")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|i| i.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    let Some(restored) = stripped_codex_notify(&current) else { return Ok(false) };
    if restored.is_empty() {
        doc.as_table_mut().remove("notify");
    } else {
        let mut array = toml_edit::Array::new();
        for arg in &restored {
            array.push(arg.as_str());
        }
        doc["notify"] = toml_edit::value(array);
    }
    write_atomic(&path, &doc.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod codex_notify_tests {
    use super::{desired_codex_notify, stripped_codex_notify};

    const HELPER: &str = "C:/Program Files/Pebrel/runtime/pebrel-hook.exe";

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn an_empty_slot_is_claimed_outright() {
        assert_eq!(desired_codex_notify(&[], HELPER), Some(argv(&[HELPER, "codex"])));
    }

    #[test]
    fn a_foreign_notifier_is_wrapped_behind_chain() {
        let current = argv(&["C:/cua/codex-computer-use.exe", "turn-ended"]);
        assert_eq!(
            desired_codex_notify(&current, HELPER),
            Some(argv(&[
                HELPER,
                "codex",
                "--chain",
                "C:/cua/codex-computer-use.exe",
                "turn-ended",
            ]))
        );
    }

    #[test]
    fn our_stale_helper_path_heals_and_keeps_the_chain_tail() {
        let current = argv(&["D:/old/nebula-hook.exe", "codex", "--chain", "C:/cua/cua.exe"]);
        assert_eq!(
            desired_codex_notify(&current, HELPER),
            Some(argv(&[HELPER, "codex", "--chain", "C:/cua/cua.exe"]))
        );
    }

    #[test]
    fn an_up_to_date_wiring_is_left_alone() {
        let current = argv(&[HELPER, "codex"]);
        assert_eq!(desired_codex_notify(&current, HELPER), None);
    }

    #[test]
    fn removal_restores_direct_and_nested_notifiers_without_claiming_similar_names() {
        let original = argv(&["C:/user/notifier.exe", "turn-ended", "路径 with spaces"]);
        let installed = desired_codex_notify(&original, HELPER).unwrap();
        assert_eq!(stripped_codex_notify(&installed), Some(original.clone()));
        assert_eq!(stripped_codex_notify(&argv(&[HELPER, "codex"])), Some(vec![]));
        let nested = argv(&[
            "C:/wrapper/notifier.exe",
            "--previous-notify",
            &serde_json::to_string(&installed).unwrap(),
            "keep",
        ]);
        let removed = stripped_codex_notify(&nested).unwrap();
        assert_eq!(removed[0..2], nested[0..2]);
        assert_eq!(removed[3], "keep");
        assert_eq!(serde_json::from_str::<Vec<String>>(&removed[2]).unwrap(), original);
        assert!(stripped_codex_notify(&removed).is_none());
        for foreign in [
            argv(&["C:/pebrel-hook-logger.exe", "codex"]),
            argv(&["echo", "pebrel-hook.exe"]),
            argv(&[HELPER, "custom"]),
            argv(&["foreign.exe", "--notify", "encoded:nebula-hook.exe:payload"]),
        ] {
            assert!(stripped_codex_notify(&foreign).is_none());
        }
    }

    // #38 的核心形态：codex-computer-use 重新注册时把我们的 chain JSON
    // 编码进 --previous-notify。我们不在最外层，但已在链中——再包一层
    // 就进入互相包装、反斜杠每轮翻倍的指数爆炸。
    #[test]
    fn a_notifier_that_swallowed_us_into_previous_notify_is_migrated_without_wrapping_again() {
        let current = argv(&[
            "C:/cua/codex-computer-use.exe",
            "--previous-notify",
            r#"["C:\\Program Files\\Nebula\\runtime\\nebula-hook.exe", "codex", "--chain", "C:\\cua\\cua.exe", "turn-ended"]"#,
            "turn-ended",
        ]);
        let desired = desired_codex_notify(&current, HELPER).expect("old embedded path migrates");
        assert_eq!(desired.len(), current.len());
        assert_eq!(desired[0], current[0]);
        assert_eq!(desired[1], current[1]);
        assert_eq!(desired[3], current[3]);
        let previous: Vec<String> = serde_json::from_str(&desired[2]).unwrap();
        let old_previous: Vec<String> = serde_json::from_str(&current[2]).unwrap();
        assert_eq!(previous[0], HELPER);
        assert_eq!(previous[1..], old_previous[1..]);
        assert_eq!(desired_codex_notify(&desired, HELPER), None);
    }

    #[test]
    fn an_unknown_wrapper_encoding_never_grows_another_hook_layer() {
        let current = argv(&["foreign.exe", "--notify", "encoded:nebula-hook.exe:payload"]);
        assert_eq!(desired_codex_notify(&current, HELPER), None);
    }

    // 兜底：即使标记检测失手（比如未来某个包装器改了我们的文件名），
    // 病态膨胀也会被字节预算拦住，config.toml 不会被写到 codex 起不来。
    #[test]
    fn an_oversized_result_is_refused() {
        let ballooned = "\\".repeat(64 * 1024);
        let current = argv(&["C:/cua/codex-computer-use.exe", &ballooned]);
        assert_eq!(desired_codex_notify(&current, HELPER), None);
    }
}
