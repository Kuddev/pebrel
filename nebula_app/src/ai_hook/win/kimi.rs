//! Kimi Code CLI hooks（`~/.kimi-code/config.toml` 的 `[[hooks]]` 数组）。
//!
//! kimi 的 hook 条目只允许 `event`/`matcher`/`command`/`timeout` 四个字段，
//! 多写一个整个 config.toml 就加载失败（官方文档硬约束），所以每条只写
//! event/command/timeout，省略 matcher。与 claude 的 exec 形式不同，kimi
//! 的 `command` 是 shell 命令字符串，helper 路径必须双引号包裹，否则含
//! 空格的安装路径会被 shell 切开（#80 的同源教训）。事件 JSON 经 stdin
//! 传给 helper；kimi 侧 fail-open，helper 任何路径都 exit 0。
//!
//! 与 claude/codex 同一套纪律：幂等合并、只认 `command` 里含 helper 标记
//! 的条目（`contains_helper`）、过期路径就地自愈、首次改动留
//! `*.pebrel-bak`、解析失败或形状意外拒写、目录不存在不 scaffold。

use std::path::{Path, PathBuf};

use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, Value};

use crate::ai_hook::contains_helper;

/// 订阅的 kimi 事件，每个一条 `[[hooks]]`。其余事件（SessionHeartbeat、
/// Notification、PreToolUse 等）不订阅。
const KIMI_EVENTS: [&str; 8] = [
    "SessionStart",
    "UserPromptSubmit",
    "Stop",
    "Interrupt",
    "StopFailure",
    "PermissionRequest",
    "PermissionResult",
    "SessionEnd",
];

/// kimi 文档允许 1–600 秒；与 claude hook 的 timeout 取同一值。
const KIMI_HOOK_TIMEOUT: i64 = 10;

/// Kimi 的配置目录：`$KIMI_CODE_HOME`，否则 `~/.kimi-code`。
pub(super) fn kimi_config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("KIMI_CODE_HOME") {
        return Some(PathBuf::from(dir));
    }
    Some(PathBuf::from(std::env::var_os("USERPROFILE")?).join(".kimi-code"))
}

/// hook 条目的 command：`"<helper 绝对路径>" kimi`。路径用正斜杠，与
/// claude/codex 条目的写法一致，也避开 TOML 基本字符串里的反斜杠转义。
fn hook_command(helper: &Path) -> String {
    format!("\"{}\" kimi", helper.display().to_string().replace('\\', "/"))
}

/// 「自己的条目」= 订阅事件 ∩ command 含 helper 标记。用户把 pebrel-hook
/// 手工挂到未订阅事件上的条目不在移除范围内（与 claude remove_hooks
/// 只遍历 CLAUDE_EVENTS 同理）。
fn is_our_entry(event: Option<&str>, command: Option<&str>) -> bool {
    event.is_some_and(|event| KIMI_EVENTS.contains(&event)) && command.is_some_and(contains_helper)
}

/// 把文档的 `hooks` 键归一化成数组表。`hooks = []`（空内联数组）与空的
/// `[[hooks]]` 语义相同，就地替换；非空内联数组或其它类型属于意外形状，
/// 返回 `None` 拒写（与 claude install_into 的 unexpected-shape 一致）。
fn hooks_array_of_tables(doc: &mut DocumentMut) -> Option<&mut ArrayOfTables> {
    let replace = match doc.get("hooks") {
        None => true,
        Some(item) if item.as_array_of_tables().is_some() => false,
        Some(item) => match item.as_array() {
            Some(array) if array.is_empty() => true,
            _ => return None,
        },
    };
    if replace {
        doc["hooks"] = Item::ArrayOfTables(ArrayOfTables::new());
    }
    doc.get_mut("hooks").and_then(Item::as_array_of_tables_mut)
}

/// 纯 TOML 合并：每个订阅事件恰好一条 helper 条目，过期路径就地修好。
/// `None` = hooks 形状意外，拒写。
fn install_into_doc(doc: &mut DocumentMut, command: &str) -> Option<bool> {
    let hooks = hooks_array_of_tables(doc)?;
    let mut changed = false;
    for event in KIMI_EVENTS {
        let mut found = false;
        for entry in hooks.iter_mut() {
            let ours = entry.get("event").and_then(Item::as_str) == Some(event)
                && entry.get("command").and_then(Item::as_str).is_some_and(contains_helper);
            if !ours {
                continue;
            }
            found = true;
            // 只修路径：timeout/matcher 可能被用户调过，不动（claude 自愈
            // 同样只碰 command/args）。
            if entry.get("command").and_then(Item::as_str) != Some(command) {
                entry["command"] = toml_edit::value(command);
                changed = true;
            }
        }
        if !found {
            let mut entry = Table::new();
            entry["event"] = toml_edit::value(event);
            entry["command"] = toml_edit::value(command);
            entry["timeout"] = toml_edit::value(KIMI_HOOK_TIMEOUT);
            hooks.push(entry);
            changed = true;
        }
    }
    Some(changed)
}

/// 从文档中删掉我们的条目；清空后留下的空数组不再清理。返回是否有改动。
fn remove_from_doc(doc: &mut DocumentMut) -> bool {
    match doc.get_mut("hooks") {
        Some(Item::ArrayOfTables(hooks)) => {
            let doomed: Vec<usize> = hooks
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    is_our_entry(
                        entry.get("event").and_then(Item::as_str),
                        entry.get("command").and_then(Item::as_str),
                    )
                })
                .map(|(index, _)| index)
                .collect();
            let changed = !doomed.is_empty();
            for index in doomed.into_iter().rev() {
                hooks.remove(index);
            }
            changed
        },
        // 卸载器尽最大努力：配置切换器可能把 [[hooks]] 归一化成内联数组，
        // 里面指向 helper 的条目同样要清（#8：卸载后 hook 不能指向已删的 exe）。
        Some(Item::Value(Value::Array(array))) => {
            let before = array.len();
            array.retain(|value| {
                !value.as_inline_table().is_some_and(|entry| {
                    is_our_entry(
                        entry.get("event").and_then(Value::as_str),
                        entry.get("command").and_then(Value::as_str),
                    )
                })
            });
            array.len() != before
        },
        _ => false,
    }
}

/// 在 `path`（config.toml）上执行安装/自愈。返回是否写了文件。
fn ensure_kimi_hooks_in(path: &Path, helper: &Path) -> bool {
    let raw = std::fs::read_to_string(path).unwrap_or_default(); // 文件不存在 → 空文档
    let Ok(mut doc) = raw.parse::<DocumentMut>() else {
        log::warn!("ai_hook: {} is not valid TOML; left alone", path.display());
        return false;
    };
    let command = hook_command(helper);
    let Some(changed) = install_into_doc(&mut doc, &command) else {
        log::warn!("ai_hook: {} has an unexpected hooks shape; left alone", path.display());
        return false;
    };
    if !changed {
        return false;
    }
    // 首次改动留一份原始备份；新建文件没有可备份的原文，跳过。
    if path.exists() {
        let bak = path.with_extension("toml.pebrel-bak");
        if !bak.exists()
            && let Err(err) = std::fs::copy(path, &bak)
        {
            log::warn!("ai_hook: backup failed ({err}); not touching {}", path.display());
            return false;
        }
    }
    match super::write_atomic(path, &doc.to_string()) {
        Ok(()) => {
            log::info!("ai_hook: kimi hooks installed into {}", path.display());
            true
        },
        Err(err) => {
            log::warn!("ai_hook: failed to write {}: {err}", path.display());
            false
        },
    }
}

/// 目录不存在 = kimi 未安装，不为它 scaffold 配置树（与 pi/opencode 一致）。
fn ensure_kimi_hooks_in_dir(dir: &Path, helper: &Path) -> bool {
    if !dir.exists() {
        return false;
    }
    ensure_kimi_hooks_in(&dir.join("config.toml"), helper)
}

/// 自愈/安装入口。返回是否写了文件。
pub(super) fn ensure_kimi_hooks() -> bool {
    let Some(dir) = kimi_config_dir() else { return false };
    let Some(helper) = super::helper_path() else { return false };
    let changed = ensure_kimi_hooks_in_dir(&dir, &helper);
    if changed {
        super::announce();
    }
    changed
}

fn remove_kimi_hooks_in(path: &Path) -> std::io::Result<bool> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return Ok(false),
    };
    let mut doc =
        raw.parse::<DocumentMut>().map_err(|err| std::io::Error::other(err.to_string()))?;
    if !remove_from_doc(&mut doc) {
        return Ok(false);
    }
    super::write_atomic(path, &doc.to_string())?;
    Ok(true)
}

/// `setup-ai --remove` 的 kimi 分支。
pub(super) fn remove_kimi_hooks() -> std::io::Result<bool> {
    let Some(dir) = kimi_config_dir() else { return Ok(false) };
    remove_kimi_hooks_in(&dir.join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::{
        KIMI_EVENTS, KIMI_HOOK_TIMEOUT, ensure_kimi_hooks_in, ensure_kimi_hooks_in_dir,
        hook_command, remove_kimi_hooks_in,
    };

    const HELPER: &str = "C:/Program Files/Pebrel/runtime/pebrel-hook.exe";

    fn helper() -> &'static std::path::Path {
        std::path::Path::new(HELPER)
    }

    fn expected_command() -> String {
        format!("\"{HELPER}\" kimi")
    }

    fn parse(path: &std::path::Path) -> toml_edit::DocumentMut {
        std::fs::read_to_string(path).unwrap().parse().unwrap()
    }

    #[test]
    fn hook_command_quotes_the_helper_path_for_the_shell() {
        // #80：kimi 的 command 走 shell，含空格路径必须双引号包裹；
        // 反斜杠归一成正斜杠，避免 TOML/shell 双层转义。
        let command =
            hook_command(std::path::Path::new("D:\\Program Files\\Pebrel\\pebrel-hook.exe"));
        assert_eq!(command, "\"D:/Program Files/Pebrel/pebrel-hook.exe\" kimi");
    }

    #[test]
    fn installs_all_eight_events_with_only_the_allowed_fields() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");

        assert!(ensure_kimi_hooks_in(&path, helper()));
        let doc = parse(&path);
        let entries = doc["hooks"].as_array_of_tables().expect("hooks 是数组表");
        assert_eq!(entries.len(), KIMI_EVENTS.len());
        let events: std::collections::BTreeSet<_> =
            entries.iter().map(|entry| entry["event"].as_str().unwrap()).collect();
        assert_eq!(events, KIMI_EVENTS.iter().copied().collect());
        for entry in entries.iter() {
            let keys: Vec<_> = entry.iter().map(|(key, _)| key).collect();
            assert_eq!(keys, ["event", "command", "timeout"], "四字段以内且不写 matcher");
            assert_eq!(entry["command"].as_str(), Some(expected_command().as_str()));
            assert_eq!(entry["timeout"].as_integer(), Some(KIMI_HOOK_TIMEOUT));
        }
        // 新建文件没有可备份的原文，不落 .pebrel-bak。
        assert!(!path.with_extension("toml.pebrel-bak").exists());
    }

    #[test]
    fn merge_is_idempotent_down_to_the_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        assert!(ensure_kimi_hooks_in(&path, helper()));
        let first = std::fs::read_to_string(&path).unwrap();
        assert!(!ensure_kimi_hooks_in(&path, helper()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
    }

    #[test]
    fn stale_helper_paths_heal_in_place_without_duplicates() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"[[hooks]]
event = "Stop"
command = "\"D:/old/Nebula/runtime/nebula-hook.exe\" kimi"
timeout = 30
"#,
        )
        .unwrap();

        assert!(ensure_kimi_hooks_in(&path, helper()));
        let doc = parse(&path);
        let entries = doc["hooks"].as_array_of_tables().unwrap();
        assert_eq!(entries.len(), KIMI_EVENTS.len(), "自愈就地修复，不为同一事件追加第二条");
        let stop: Vec<_> =
            entries.iter().filter(|entry| entry["event"].as_str() == Some("Stop")).collect();
        assert_eq!(stop.len(), 1);
        assert_eq!(stop[0]["command"].as_str(), Some(expected_command().as_str()));
        assert_eq!(stop[0]["timeout"].as_integer(), Some(30), "用户调过的字段不自愈");
        // 既有文件的首次改动留下了原始备份。
        let backup = std::fs::read_to_string(path.with_extension("toml.pebrel-bak")).unwrap();
        assert!(backup.contains("D:/old/Nebula"));
        assert!(!ensure_kimi_hooks_in(&path, helper()));
    }

    #[test]
    fn removal_strips_only_our_own_entries() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"[[hooks]]
event = "Notification"
command = "notify-send done"

[[hooks]]
event = "PreToolUse"
command = "\"D:/tools/pebrel-hook.exe\" kimi"
"#,
        )
        .unwrap();
        assert!(ensure_kimi_hooks_in(&path, helper()));

        assert!(remove_kimi_hooks_in(&path).unwrap());
        let doc = parse(&path);
        let kept: Vec<(&str, &str)> = doc["hooks"]
            .as_array_of_tables()
            .unwrap()
            .iter()
            .map(|entry| (entry["event"].as_str().unwrap(), entry["command"].as_str().unwrap()))
            .collect();
        assert_eq!(
            kept,
            [
                ("Notification", "notify-send done"),
                ("PreToolUse", "\"D:/tools/pebrel-hook.exe\" kimi"),
            ],
            "别人的条目与手工挂在未订阅事件上的 helper 条目都要留下"
        );
        assert!(!remove_kimi_hooks_in(&path).unwrap(), "已经删净，第二遍无改动");
    }

    #[test]
    fn removal_also_covers_inline_array_form_left_by_config_switchers() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            "hooks = [{ event = \"Stop\", command = \"\\\"D:/x/pebrel-hook.exe\\\" kimi\" }, \
             { event = \"Notification\", command = \"notify-send done\" }]\n",
        )
        .unwrap();

        assert!(remove_kimi_hooks_in(&path).unwrap());
        let doc = parse(&path);
        let kept: Vec<&str> = doc["hooks"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value.as_inline_table()?.get("command")?.as_str())
            .collect();
        assert_eq!(kept, ["notify-send done"]);
    }

    #[test]
    fn broken_toml_is_left_untouched() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(&path, "[[hooks]\nevent = ").unwrap();

        assert!(!ensure_kimi_hooks_in(&path, helper()));
        assert!(remove_kimi_hooks_in(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[[hooks]\nevent = ");
    }

    #[test]
    fn unexpected_hooks_shape_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let raw = "hooks = [{ event = \"Stop\", command = \"other-tool\" }]\n";
        std::fs::write(&path, raw).unwrap();

        assert!(!ensure_kimi_hooks_in(&path, helper()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
    }

    #[test]
    fn empty_inline_array_is_normalized_and_user_keys_survive() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(&path, "model = \"k2\"\nhooks = []\n").unwrap();

        assert!(ensure_kimi_hooks_in(&path, helper()));
        let doc = parse(&path);
        assert_eq!(doc["model"].as_str(), Some("k2"));
        assert_eq!(doc["hooks"].as_array_of_tables().unwrap().len(), KIMI_EVENTS.len());
    }

    #[test]
    fn missing_directory_is_not_scaffolded() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("no-such-dir");
        assert!(!ensure_kimi_hooks_in_dir(&dir, helper()));
        assert!(!dir.exists());
    }

    #[test]
    fn removal_leaves_a_parseable_config_behind() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        assert!(ensure_kimi_hooks_in(&path, helper()));
        assert!(remove_kimi_hooks_in(&path).unwrap());

        let doc = parse(&path);
        let empty = doc.get("hooks").is_none_or(|hooks| {
            hooks.as_array_of_tables().is_some_and(|entries| entries.is_empty())
        });
        assert!(empty, "我们的条目删净后只留下空 hooks（可留）");
    }
}
