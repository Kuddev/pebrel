//! Legacy notify composition shared by Windows and SSH installers.
use crate::ai_hook::{contains_helper, is_helper_executable};

/// notify 序列化后的字节预算。正常接线只有几百字节；超过它的唯一已知
/// 途径是与其他 notify 包装器互相包装的指数膨胀（#38，最终 130 MB 撑爆
/// config.toml 让 codex 起不来）。宁可这一轮不接线，也不把病态值落盘。
const NOTIFY_BYTE_BUDGET: usize = 8 * 1024;

/// 由现有 notify argv 算出应写入的新 argv；`None` = 不动文件。
///
/// 与 codex-computer-use 的共存规则（#38）：它重新注册时若认不出
/// notify\[0\] 是自己，会把整个旧数组 JSON 序列化进自己的
/// `--previous-notify` 参数。此时 nebula-hook 不在最外层，但仍在链中
/// ——事件会沿链回流。若这时再包一层，两个包装器互相包装、转义反斜杠
/// 每轮翻倍。所以：helper 标记出现在**任何位置**（含 JSON 字符串内部）
/// 都算已接线，只有最外层是自己时才做路径自愈。
pub(crate) fn desired_codex_notify(current: &[String], helper: &str) -> Option<Vec<String>> {
    let desired: Vec<String> = match current.first() {
        // Already ours: heal the helper path, keep any chain tail as-is.
        Some(_) if owns_notify(current) => {
            let mut argv = current.to_vec();
            argv[0] = helper.to_owned();
            argv
        },
        // 已在链中但不在最外层：保持现状，绝不再包（见上）。
        Some(_) if current.iter().any(|arg| contains_helper(arg)) => {
            let mut argv = current.to_vec();
            if !heal_nested_codex_notify(&mut argv, helper, 0) {
                return None;
            }
            argv
        },
        // Occupied: wrap the existing notifier behind --chain.
        Some(_) => {
            let mut argv = vec![helper.to_owned(), "codex".to_owned(), "--chain".to_owned()];
            argv.extend(current.iter().cloned());
            argv
        },
        None => vec![helper.to_owned(), "codex".to_owned()],
    };
    if current == desired {
        return None;
    }
    // 长度兜底：对任何形态的膨胀（不止 #38 这一种循环）一律拒写。
    // +4 ≈ 每个元素的引号、逗号与空格开销。
    let bytes: usize = desired.iter().map(|arg| arg.len() + 4).sum();
    if bytes > NOTIFY_BYTE_BUDGET {
        log::warn!(
            "ai_hook: codex notify would serialize to {bytes} bytes (> {NOTIFY_BYTE_BUDGET}); \
                 refusing to write (wrapper loop guard, #38)"
        );
        return None;
    }
    Some(desired)
}

fn heal_nested_codex_notify(argv: &mut [String], helper: &str, depth: usize) -> bool {
    if depth >= 8 || argv.iter().map(String::len).sum::<usize>() > NOTIFY_BYTE_BUDGET {
        return false;
    }
    let mut changed = false;
    if owns_notify(argv) && argv[0] != helper {
        argv[0] = helper.to_owned();
        changed = true;
    }
    for index in 1..argv.len() {
        if argv[index - 1] != "--previous-notify" {
            continue;
        }
        let Ok(mut previous) = serde_json::from_str::<Vec<String>>(&argv[index]) else {
            continue;
        };
        if heal_nested_codex_notify(&mut previous, helper, depth + 1) {
            if let Ok(serialized) = serde_json::to_string(&previous) {
                argv[index] = serialized;
                changed = true;
            }
        }
    }
    changed
}

fn owns_notify(argv: &[String]) -> bool {
    argv.first().is_some_and(|value| is_helper_executable(value))
        && argv.get(1).is_some_and(|source| source == "codex")
        && (argv.len() == 2 || (argv.len() > 3 && argv[2] == "--chain"))
}

/// 只解析已支持的 argv/JSON 包装，不用名字片段把用户文案当成已安装 Hook。
pub(crate) fn has_codex_notify(argv: &[String]) -> bool {
    stripped_codex_notify(argv).is_some()
}

/// 返回移除自身后的原 notifier；空数组表示可以删除 notify 键。
pub(crate) fn stripped_codex_notify(argv: &[String]) -> Option<Vec<String>> {
    let mut result = argv.to_vec();
    strip_nested(&mut result, 0).then_some(result)
}

fn strip_nested(argv: &mut Vec<String>, depth: usize) -> bool {
    if depth >= 8 || argv.iter().map(String::len).sum::<usize>() > NOTIFY_BYTE_BUDGET {
        return false;
    }
    let mut changed = false;
    if owns_notify(argv) {
        // 两层历史包装也逐层解开，但始终保留最初的程序与参数顺序。
        *argv = if argv.len() > 3 { argv[3..].to_vec() } else { Vec::new() };
        strip_nested(argv, depth + 1);
        changed = true;
    }
    for index in 1..argv.len() {
        if argv[index - 1] != "--previous-notify" {
            continue;
        }
        let Ok(mut previous) = serde_json::from_str::<Vec<String>>(&argv[index]) else {
            continue;
        };
        if strip_nested(&mut previous, depth + 1)
            && let Ok(serialized) = serde_json::to_string(&previous)
        {
            argv[index] = serialized;
            changed = true;
        }
    }
    changed
}
