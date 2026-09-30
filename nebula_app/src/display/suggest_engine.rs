//! 补全/建议引擎：ghost 余量与弹窗候选的计算核心。
//!
//! 从 `Display` 的方法下沉为自由函数：winit 壳把 `Display` 字段借成
//! [`SuggestSources`]，GPUI 壳借应用层单例（`completion`）。
//! 数据源与排序规则两个壳共用，避免第二套平行实现（与 `ssh_session` 的
//! `SshEventHost` 泛型下沉同一手法）。

use nebula_completions::command_context::ShellSyntax;
use nebula_completions::{SemanticSuggestion, SuggestionKind};

use super::state::{CompletionStyle, NebulaCompletionItem, NebulaCompletionKind, NebulaPaneState};
use super::{
    NEBULA_GHOST_MAX, nebula_command_hint, nebula_command_hints, nebula_debug_log,
    nebula_is_command_position,
};

/// 借用的共享数据源与运行时开关。生命周期只覆盖一次重算调用。
pub(crate) struct SuggestSources<'a> {
    pub history: HistorySource<'a>,
    pub directories: &'a crate::directory_history::DirectoryHistory,
    pub commands: &'a std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    pub enabled: bool,
    pub style: CompletionStyle,
}

/// 两个界面拥有历史的方式不同；只在取出少量匹配项时借用历史，不能让文件扫描占锁。
pub(crate) enum HistorySource<'a> {
    Borrowed(&'a crate::nebula_history::NebulaHistory),
    Shared(&'a std::sync::Mutex<crate::nebula_history::NebulaHistory>),
}

impl HistorySource<'_> {
    fn read<R>(&self, query: impl FnOnce(&crate::nebula_history::NebulaHistory) -> R) -> R {
        match self {
            Self::Borrowed(history) => query(history),
            Self::Shared(history) => {
                query(&history.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
            },
        }
    }

    fn hint(
        &self,
        scope: &crate::nebula_history::HistoryScope,
        line: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<String> {
        self.read(|history| history.hint_with_cancel(scope, line, cancelled).map(clamp_ghost))
    }

    fn search(
        &self,
        scope: &crate::nebula_history::HistoryScope,
        line: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Vec<String> {
        self.read(|history| {
            history
                .search_with_cancel(scope, line, 8, cancelled)
                .into_iter()
                .map(str::to_owned)
                .collect()
        })
    }
}

/// WSL / SSH pane 在命令位置能补的东西。
///
/// 本进程的 PATH 描述的是 Windows，里面一个 `notepad.exe` 在来宾或远端都不存在，
/// 拿它去补 tab 只会给出跑不起来的命令。真实的远端 PATH 要一次往返才问得到
/// （见 [`SuggestEnv`] 的文档），在那之前这张表是诚实的兜底：全是 POSIX shell
/// 内置与几乎每台 Linux 都装了的工具。
///
/// 排序无所谓——[`nebula_command_hints`] 会自己按长度和字典序收敛。
const POSIX_COMMANDS: &[&str] = &[
    // shell 内置
    "alias",
    "bg",
    "cd",
    "declare",
    "echo",
    "eval",
    "exec",
    "exit",
    "export",
    "false",
    "fg",
    "history",
    "jobs",
    "kill",
    "local",
    "printf",
    "pwd",
    "read",
    "return",
    "set",
    "shift",
    "source",
    "test",
    "trap",
    "true",
    "type",
    "umask",
    "unalias",
    "unset",
    "wait",
    "which",
    // coreutils 与文件操作
    "basename",
    "cat",
    "chgrp",
    "chmod",
    "chown",
    "cp",
    "cut",
    "df",
    "dirname",
    "du",
    "find",
    "grep",
    "head",
    "less",
    "ln",
    "ls",
    "mkdir",
    "more",
    "mv",
    "readlink",
    "realpath",
    "rm",
    "rmdir",
    "sed",
    "sort",
    "stat",
    "tail",
    "tee",
    "touch",
    "tr",
    "uniq",
    "wc",
    "xargs",
    // 进程 / 系统
    "chsh",
    "env",
    "free",
    "htop",
    "id",
    "journalctl",
    "man",
    "mount",
    "nohup",
    "ps",
    "service",
    "su",
    "sudo",
    "systemctl",
    "top",
    "uname",
    "uptime",
    "whoami",
    // 网络与传输
    "curl",
    "dig",
    "ip",
    "netstat",
    "ping",
    "rsync",
    "scp",
    "ss",
    "ssh",
    "wget",
    // 归档
    "gunzip",
    "gzip",
    "tar",
    "unzip",
    "xz",
    "zip",
    // 包管理
    "apt",
    "apt-get",
    "dnf",
    "dpkg",
    "pacman",
    "snap",
    "yum",
    // 开发
    "awk",
    "cargo",
    "cmake",
    "docker",
    "g++",
    "gcc",
    "gdb",
    "git",
    "go",
    "make",
    "nano",
    "node",
    "npm",
    "pip",
    "pip3",
    "pnpm",
    "python",
    "python3",
    "rustc",
    "rustup",
    "vim",
    "yarn",
];

/// [`POSIX_COMMANDS`] 的 `Vec<String>` 视图。
///
/// `nebula_command_hint`/`_hints` 吃的是 `&[String]`（本机那份来自 PATH 探针）。
/// 转换只做一次；等远端 PATH 真的探到了，替换的就是这个容器本身。
fn posix_commands() -> &'static [String] {
    static CACHE: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| POSIX_COMMANDS.iter().map(|name| (*name).to_owned()).collect())
}

/// 补齐面对的是**哪台机器**的文件系统与命令集。
///
/// 这是补齐正确性的根，不只是个功能开关。同一条 `ls /te<Tab>`：本地 tab 该查
/// `D:\te*`，WSL tab 该查来宾的 `/te*`，SSH tab 该问远端。走错机器不是"补不
/// 出来"这么轻——它会**补出另一台机器上的路径**，而那条路径在当前 shell 里
/// 根本不存在。
///
/// 这不是假想的失败模式。宿主把 `/temp_build` 解析成"当前盘符下的
/// `\temp_build`"，而这台机器的 D 盘恰好就有 `D:\temp_build`，于是
/// [`crate::directory_history::DirectoryHistory::hint`] 的 `is_dir` 把关会当成
/// 命中，把宿主的目录补进 WSL 的命令行里。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SuggestEnv {
    /// 普通本地 tab：`std::fs` 与本进程 PATH 指向同一台机器。
    #[default]
    Local,
    /// WSL tab：cwd 是来宾的 Linux 绝对路径。宿主的 `std::fs` 读不到它——本机
    /// 的 9P 重定向不可用，`\\wsl.localhost\` 三条路实测全部失败，见
    /// [`crate::shell_detect::wsl_unc_cwd`] 的记录。要列来宾目录只能
    /// `wsl.exe -d <发行版> -- find`，那是子进程，冷启动可达数秒，绝不能挂在
    /// 按键路径上。
    Wsl { distro: String },
    /// SSH tab：文件系统只能经 SFTP 往返，命令集是远端的。`destination` 进
    /// 缓存键——同一条 `/home/kud` 在两台远端上是两个不同的目录。
    Ssh { destination: String },
    /// A shell reached through a typed command. History has its own route;
    /// there is no verified local WSL/SFTP channel for filesystem queries.
    Shell { scope: crate::nebula_history::HistoryScope },
}

impl SuggestEnv {
    /// 本进程的 `std::fs` 与 PATH 是否描述这个 pane 所在的机器。
    pub fn is_this_machine(&self) -> bool {
        matches!(self, Self::Local)
    }

    pub(crate) fn can_query_remote_paths(&self) -> bool {
        matches!(self, Self::Ssh { .. })
            || matches!(self, Self::Wsl { distro } if !distro.is_empty())
    }

    pub(crate) fn history_scope(&self) -> crate::nebula_history::HistoryScope {
        match self {
            Self::Local => crate::nebula_history::HistoryScope::Local,
            Self::Wsl { distro } => {
                crate::nebula_history::HistoryScope::Wsl(if distro.is_empty() {
                    "<default>".to_owned()
                } else {
                    distro.clone()
                })
            },
            Self::Ssh { destination } => {
                crate::nebula_history::HistoryScope::Ssh(destination.clone())
            },
            Self::Shell { scope } => scope.clone(),
        }
    }
}

/// Refresh the inline ghost remainder or the popup candidate list for the
/// current prompt line. Cached on `cwd + line`; dismissal keeps the key so a
/// closed popup stays closed until the line itself changes.
pub(crate) fn suggest_update(
    sources: &SuggestSources<'_>,
    state: &mut NebulaPaneState,
    line_override: Option<String>,
) {
    suggest_update_with_cancel(sources, state, line_override, &|| false);
}

pub(crate) fn suggest_update_with_cancel(
    sources: &SuggestSources<'_>,
    state: &mut NebulaPaneState,
    line_override: Option<String>,
    cancelled: &dyn Fn() -> bool,
) {
    if cancelled() {
        return;
    }
    if let Some(line) = line_override.as_deref()
        && !state.completion_echo_ready(line)
    {
        return;
    }
    let line = line_override.unwrap_or_else(|| state.line_buf.clone());
    if !sources.enabled || line.is_empty() {
        state.completion_popup_requested = false;
        state.clear_completion_hints();
        nebula_debug_log(format!(
            "suggest_skip enabled={} cwd={:?} line={:?} line_buf={:?}",
            sources.enabled, state.cwd, line, state.line_buf
        ));
        return;
    }

    // 命令目录由后台 PowerShell 探针填充；目录从 0 变为完整集合时，即使
    // 用户没有继续输入，也必须让上一帧的“无候选”缓存失效。
    let command_generation = sources.commands.lock().map(|commands| commands.len()).unwrap_or(0);
    // 远端目录是异步拉回来的，那一刻 cwd 与行都没变——少了这个代际，拉到的
    // 条目要等用户再多打一个字符才会显形。
    let key = suggestion_key(
        &Input { cwd: &state.cwd, env: &state.suggest_env, line: &line },
        sources.style,
        command_generation,
    );
    if state.completion_suppressed_line.as_deref() == Some(line.as_str()) {
        state.suggestion_key = key;
        state.suggestion.clear();
        state.completion_items.clear();
        state.completion_selected = None;
        return;
    }
    state.completion_suppressed_line = None;
    if key == state.suggestion_key {
        // Cache hit also protects an Esc-dismissed popup: dismissal clears
        // the items but keeps the key, so nothing reopens until the line
        // actually changes.
        return;
    }
    state.suggestion_key = key;
    state.suggestion.clear();
    state.suggestion_edit = None;
    state.completion_items.clear();
    state.completion_selected = None;

    let result = calculate(
        sources,
        &Input { cwd: &state.cwd, env: &state.suggest_env, line: &line },
        if state.suggest_env.is_this_machine() {
            ShellSyntax::for_program(&crate::platform::shell::default_shell_id())
        } else {
            ShellSyntax::Literal
        },
        cancelled,
    );
    state.suggestion = result.suggestion;
    state.suggestion_edit = result.suggestion_edit;
    state.completion_items = result.completion_items;
    state.pending_remote_dir = result.pending_remote_dir;
}

/// 计算仅借用输入快照，不能访问或修改窗口、网格及命令运行状态。
pub(crate) struct Input<'a> {
    pub cwd: &'a str,
    pub env: &'a SuggestEnv,
    pub line: &'a str,
}

#[derive(Default)]
pub(crate) struct Candidates {
    pub suggestion: String,
    pub suggestion_edit: Option<NebulaCompletionItem>,
    pub completion_items: Vec<NebulaCompletionItem>,
    pub pending_remote_dir: Option<String>,
}

pub(crate) fn calculate(
    sources: &SuggestSources<'_>,
    input: &Input<'_>,
    syntax: ShellSyntax,
    cancelled: &dyn Fn() -> bool,
) -> Candidates {
    let mut result = Candidates::default();
    let line = input.line;
    if cancelled() || !sources.enabled || line.is_empty() {
        return result;
    }
    nebula_debug_log(format!("suggest_begin cwd={:?} line={:?}", input.cwd, line));

    // Popup style computes a multi-candidate list instead of the single
    // ghost remainder; the two are mutually exclusive per pane.
    if sources.style == CompletionStyle::Popup {
        suggest_collect(sources, input, syntax, &mut result, cancelled);
        return result;
    }

    // History first: newest command that extends the whole line (indexed
    // prefix lookup — scales with matches, not history size).
    if let Some(rem) = sources.history.hint(&input.env.history_scope(), line, cancelled) {
        result.suggestion = rem;
        nebula_debug_log(format!("suggest_result kind=history rem={:?}", result.suggestion));
        return result;
    }

    if cancelled() {
        return result;
    }

    // First token with no path separators is a command position. Reuse the
    // process PATH inherited by the shell so typing `ca` can ghost `rgo`
    // even before that command has appeared in Nebula/Nushell history.
    // WSL/SSH 的 shell 没继承这个 PATH，那边走 [`POSIX_COMMANDS`]。
    if nebula_is_command_position(line) {
        let hinted = if input.env.is_this_machine() {
            sources.commands.lock().ok().and_then(|commands| {
                nebula_command_hint(commands.as_slice(), line).map(str::to_owned)
            })
        } else {
            nebula_command_hint(posix_commands(), line).map(str::to_owned)
        };
        if let Some(rem) = hinted {
            result.suggestion = clamp_ghost(&rem);
            nebula_debug_log(format!("suggest_result kind=command rem={:?}", result.suggestion));
            return result;
        }
    }

    let (paths, pending) = crate::completion::paths::complete(
        input,
        syntax,
        sources.directories,
        sources.style,
        None,
        cancelled,
    );
    result = semantic_candidates(line, sources.style, paths);
    result.pending_remote_dir = pending;
    result
}

/// 输入、环境和后台数据代际共同标识候选；异步适配器也用它拒绝过期结果。
pub(crate) fn suggestion_key(
    input: &Input<'_>,
    style: CompletionStyle,
    command_generation: usize,
) -> String {
    format!(
        "{:?}\0{}\0{}\0{style:?}\0{command_generation}\0{}",
        input.env,
        input.cwd,
        input.line,
        crate::remote_dirs::generation()
    )
}

/// Cap ghost length so a long path/command can't spill into the chrome.
fn clamp_ghost(rem: &str) -> String {
    rem.chars().take(NEBULA_GHOST_MAX).collect()
}

/// Elide long popup labels from the LEFT (paths keep their informative
/// tail; the head the user already typed is the expendable part).
pub(crate) fn elide_left(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_owned();
    }
    let tail: String = text.chars().skip(count + 1 - max_chars).collect();
    format!("…{tail}")
}

/// Preserve the candidate's spelling when a case-insensitive/fuzzy match
/// changes already-typed text. Only a literal prefix can be appended safely.
fn popup_edit(line: &str, candidate: &str) -> (usize, String) {
    match candidate.strip_prefix(line) {
        Some(suffix) => (0, suffix.to_owned()),
        None => (line.chars().count(), candidate.to_owned()),
    }
}

/// Project semantic byte spans into the terminal's end-of-line edit contract.
pub(crate) fn semantic_candidates(
    line: &str,
    style: CompletionStyle,
    candidates: Vec<SemanticSuggestion>,
) -> Candidates {
    let mut result = Candidates::default();
    let items: Vec<_> = candidates
        .into_iter()
        .filter_map(|semantic| {
            let candidate = semantic.suggestion;
            if candidate.span.end != line.len() {
                return None;
            }
            let token = line.get(candidate.span.start..candidate.span.end)?;
            // 只替换分歧后的尾部；已闭合引号也能接受，不删除整条命令或 UTF-8 半字符。
            let common: usize = token
                .chars()
                .zip(candidate.value.chars())
                .take_while(|(a, b)| a == b)
                .map(|(ch, _)| ch.len_utf8())
                .sum();
            Some(NebulaCompletionItem {
                label: elide_left(candidate.display_value(), 44),
                insert: candidate.value[common..].to_owned(),
                replace_chars: token[common..].chars().count(),
                kind: match semantic.kind {
                    Some(SuggestionKind::Directory) => NebulaCompletionKind::Dir,
                    Some(SuggestionKind::File) => NebulaCompletionKind::File,
                    _ => NebulaCompletionKind::Command,
                },
            })
        })
        .filter(|item| item.replace_chars != 0 || !item.insert.is_empty())
        .take(256)
        .collect();
    if style == CompletionStyle::Popup {
        result.completion_items = items;
    } else if let Some(item) = items.into_iter().find(|item| !item.insert.is_empty()) {
        result.suggestion = clamp_ghost(&item.insert);
        result.suggestion_edit = Some(item);
    }
    result
}

/// Fill `result.completion_items` for the popup style: the same sources as
/// the ghost hint (history → directory history → PATH commands → file
/// system), but keeping several candidates each instead of the first hit.
fn suggest_collect(
    sources: &SuggestSources<'_>,
    input: &Input<'_>,
    syntax: ShellSyntax,
    result: &mut Candidates,
    cancelled: &dyn Fn() -> bool,
) {
    // 8 是视口行数，不是数据上限。旧实现把两者混成一个常量，候选在收集
    // 阶段就被截断，因而既画不出滚动条，Tab 也永远走不到第九项以后。
    let line = input.line;
    const POPUP_LIMIT: usize = 256;
    const LABEL_MAX: usize = 44;

    let mut items: Vec<NebulaCompletionItem> = Vec::new();
    let push = |items: &mut Vec<NebulaCompletionItem>, item: NebulaCompletionItem| {
        if item.insert.is_empty() {
            return;
        }
        if items.iter().any(|seen| seen.insert == item.insert && seen.kind == item.kind) {
            return;
        }
        if items.len() < POPUP_LIMIT {
            items.push(item);
        }
    };

    // Whole-line history matches, newest first.
    for full in sources.history.search(&input.env.history_scope(), line, cancelled) {
        let (replace_chars, insert) = popup_edit(line, &full);
        push(
            &mut items,
            NebulaCompletionItem {
                replace_chars,
                label: elide_left(&full, LABEL_MAX),
                insert,
                kind: NebulaCompletionKind::History,
            },
        );
    }

    if cancelled() {
        return;
    }

    // PATH executables while the first token is being typed. 本机用 shell 继承
    // 的进程 PATH；WSL/SSH 的 shell 没继承它，走 [`POSIX_COMMANDS`]。
    if nebula_is_command_position(line) {
        let local = input.env.is_this_machine();
        let guard = local.then(|| sources.commands.lock().ok()).flatten();
        let commands: &[String] = match guard.as_deref() {
            Some(commands) => commands,
            None if local => &[],
            None => posix_commands(),
        };
        for command in nebula_command_hints(commands, line, POPUP_LIMIT) {
            // 精确命令也必须成为可接受项；补一个空格既给用户明确反馈，
            // 又让 Enter 只完成选择而不立刻执行命令。
            let (replace_chars, mut insert) = popup_edit(line, command);
            if command.eq_ignore_ascii_case(line) {
                insert.push(' ');
            }
            push(
                &mut items,
                NebulaCompletionItem {
                    replace_chars,
                    label: elide_left(command, LABEL_MAX),
                    insert,
                    kind: NebulaCompletionKind::Command,
                },
            );
        }
    }

    let (paths, pending) = crate::completion::paths::complete(
        input,
        syntax,
        sources.directories,
        sources.style,
        None,
        cancelled,
    );
    result.pending_remote_dir = pending;
    for item in semantic_candidates(line, CompletionStyle::Popup, paths).completion_items {
        push(&mut items, item);
    }

    nebula_debug_log(format!("suggest_result kind=popup line={:?} items={}", line, items.len()));
    result.completion_items = items;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directory_history::DirectoryHistory;
    use crate::nebula_history::NebulaHistory;

    #[test]
    fn filesystem_completion_does_not_hold_the_shared_history_lock() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("candidate.txt"), b"").unwrap();
        let history = std::sync::Mutex::new(NebulaHistory::default());
        let fixture = Fixture::new();
        for style in [CompletionStyle::Inline, CompletionStyle::Popup] {
            let sources = SuggestSources {
                history: HistorySource::Shared(&history),
                ..fixture.sources(style)
            };
            let mut state = NebulaPaneState {
                cwd: temp.path().to_string_lossy().into_owned(),
                ..Default::default()
            };
            let checks = std::cell::Cell::new(0);
            suggest_update_with_cancel(&sources, &mut state, Some("cat ca".into()), &|| {
                checks.set(checks.get() + 1);
                assert!(
                    history.try_lock().is_ok(),
                    "directory work cannot block history submission"
                );
                false
            });
            assert!(checks.get() > 3, "the real filesystem calculation must run");
            assert!(!state.suggestion.is_empty() || !state.completion_items.is_empty());
        }
    }

    #[test]
    fn issue_353_current_directory_candidates_work_in_both_styles() {
        let temp = tempfile::tempdir().unwrap();
        for name in ["apple", "banana", "orange"] {
            std::fs::create_dir(temp.path().join(name)).unwrap();
        }
        for name in ["x.exe", "y.txt", "z.docx", "a.txt"] {
            std::fs::write(temp.path().join(name), b"").unwrap();
        }
        let cwd = temp.path().to_str().unwrap();
        let fixture = Fixture::new();
        for (input, expected) in [
            ("cd a", format!("pple{}", std::path::MAIN_SEPARATOR)),
            ("cd b", format!("anana{}", std::path::MAIN_SEPARATOR)),
            ("x", ".exe".into()),
            ("y", ".txt".into()),
        ] {
            assert_eq!(fixture.ghost(SuggestEnv::Local, cwd, input), expected, "{input}");
            assert!(fixture.popup(SuggestEnv::Local, cwd, input).contains(&expected), "{input}");
        }
        assert!(!fixture.popup(SuggestEnv::Local, cwd, "cd a").contains(&".txt".into()));
    }

    #[test]
    fn remote_cd_candidates_exclude_files_in_both_styles() {
        let env = SuggestEnv::Ssh { destination: "issue-353-cd-filter".into() };
        crate::remote_dirs::finish_fetch(
            &env,
            "/issue-353",
            Some(vec![
                crate::remote_dirs::RemoteEntry { name: "a.txt".into(), is_dir: false },
                crate::remote_dirs::RemoteEntry { name: "apple".into(), is_dir: true },
            ]),
        );
        let fixture = Fixture::new();
        assert_eq!(fixture.ghost(env.clone(), "/issue-353", "cd a"), "pple/");
        assert_eq!(fixture.popup(env.clone(), "/issue-353", "cd a"), ["pple/"]);
        assert!(fixture.popup(env, "/issue-353", "cat a").contains(&".txt".into()));
    }

    #[cfg(not(windows))]
    #[test]
    fn local_posix_paths_preserve_case() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("Apple.txt"), b"").unwrap();
        let fixture = Fixture::new();
        let cwd = temp.path().to_str().unwrap();
        assert!(fixture.ghost(SuggestEnv::Local, cwd, "cat a").is_empty());
        assert!(fixture.popup(SuggestEnv::Local, cwd, "cat a").is_empty());
        assert_eq!(fixture.ghost(SuggestEnv::Local, cwd, "cat A"), "pple.txt");
    }

    #[test]
    fn changing_environment_invalidates_an_identical_input_cache() {
        let fixture = Fixture::new();
        let mut state = NebulaPaneState::default();
        suggest_update(
            &fixture.sources(CompletionStyle::Inline),
            &mut state,
            Some("notepad".into()),
        );
        assert_eq!(state.suggestion, ".exe");
        state.suggest_env = SuggestEnv::Ssh { destination: "cache-test".into() };
        suggest_update(
            &fixture.sources(CompletionStyle::Inline),
            &mut state,
            Some("notepad".into()),
        );
        assert!(state.suggestion.is_empty());
    }

    #[test]
    fn late_outer_directory_results_cannot_complete_an_inner_shell() {
        let fixture = Fixture::new();
        let outer = SuggestEnv::Ssh { destination: "late-directory-test".into() };
        let mut state = NebulaPaneState {
            suggest_env: outer.clone(),
            cwd: "/scope-test".into(),
            ..Default::default()
        };
        state.completion_shell_report(crate::completion_context::SHELL_VAR, "outer");
        suggest_update(
            &fixture.sources(CompletionStyle::Inline),
            &mut state,
            Some("cat ./outer-".into()),
        );
        let dir = state.pending_remote_dir.clone().expect("missing outer directory");
        assert!(crate::remote_dirs::begin_fetch(&outer, &dir));
        state.completion_submitted("ssh inner");
        state.completion_shell_report(crate::completion_context::SHELL_VAR, "inner");
        crate::remote_dirs::finish_fetch(
            &outer,
            &dir,
            Some(vec![crate::remote_dirs::RemoteEntry {
                name: "outer-only-file".into(),
                is_dir: false,
            }]),
        );
        for style in [CompletionStyle::Inline, CompletionStyle::Popup] {
            suggest_update(&fixture.sources(style), &mut state, Some("cat ./outer-".into()));
            assert!(state.suggestion.is_empty());
            assert!(state.completion_items.is_empty());
            assert!(state.pending_remote_dir.is_none());
        }
        state.completion_shell_report(crate::completion_context::SHELL_VAR, "outer");
        suggest_update(
            &fixture.sources(CompletionStyle::Inline),
            &mut state,
            Some("cat ./outer-".into()),
        );
        assert_eq!(state.suggestion, "only-file");
    }

    #[test]
    fn unnamed_wsl_keeps_guest_commands_and_never_queries_the_host_filesystem() {
        let fixture = Fixture::new();
        let env = SuggestEnv::Wsl { distro: String::new() };
        assert!(fixture.ghost(env.clone(), "", "notepad").is_empty());
        assert!(!fixture.ghost(env.clone(), "", "gre").is_empty());
        assert!(!crate::remote_dirs::begin_fetch(&env, "/"));
        assert_ne!(env.history_scope(), crate::nebula_history::HistoryScope::Local);
    }

    struct Fixture {
        history: NebulaHistory,
        directories: DirectoryHistory,
        commands: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl Fixture {
        /// 历史与 frecency 都留空，断言才只反映被测的那条分支。`commands` 装的
        /// 是**这台机器**的 PATH 探针结果，所以放一个只有 Windows 才有的名字。
        fn new() -> Self {
            Self {
                history: NebulaHistory::default(),
                directories: DirectoryHistory::empty(),
                commands: std::sync::Arc::new(std::sync::Mutex::new(vec![
                    "notepad.exe".to_owned(),
                    "grepwin.exe".to_owned(),
                ])),
            }
        }

        fn sources(&self, style: CompletionStyle) -> SuggestSources<'_> {
            SuggestSources {
                history: HistorySource::Borrowed(&self.history),
                directories: &self.directories,
                commands: &self.commands,
                enabled: true,
                style,
            }
        }

        /// 在给定环境下算一次 ghost，返回补出的余量。
        fn ghost(&self, env: SuggestEnv, cwd: &str, line: &str) -> String {
            let mut state =
                NebulaPaneState { cwd: cwd.to_owned(), suggest_env: env, ..Default::default() };
            suggest_update(
                &self.sources(CompletionStyle::Inline),
                &mut state,
                Some(line.to_owned()),
            );
            state.suggestion
        }

        /// 同上但走弹窗，返回候选的插入余量。
        fn popup(&self, env: SuggestEnv, cwd: &str, line: &str) -> Vec<String> {
            let mut state =
                NebulaPaneState { cwd: cwd.to_owned(), suggest_env: env, ..Default::default() };
            suggest_update(
                &self.sources(CompletionStyle::Popup),
                &mut state,
                Some(line.to_owned()),
            );
            state.completion_items.into_iter().map(|item| item.insert).collect()
        }
    }

    /// 走错机器不是"补不出来"这么轻——它会把**宿主**的目录补进 WSL 的命令行。
    ///
    /// 同一条命令、同一个 cwd，只有环境不同：本机该补出来，WSL 必须一无所获。
    /// 断言用的是同一份真实存在的宿主目录，所以"WSL 补不出"只可能来自环境
    /// 分流，不可能是路径碰巧不存在。
    #[test]
    fn a_foreign_pane_never_completes_a_path_off_this_machine() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("host-only-dir")).unwrap();
        let cwd = temp.path().to_string_lossy().into_owned();
        let fixture = Fixture::new();

        assert_eq!(
            fixture.ghost(SuggestEnv::Local, &cwd, "ls host-only-"),
            format!("dir{}", std::path::MAIN_SEPARATOR),
            "本机 tab 的 std::fs 就是这个 pane 的文件系统（补到目录带尾分隔符）"
        );
        for env in [
            SuggestEnv::Wsl { distro: "Debian".to_owned() },
            SuggestEnv::Ssh { destination: "kud@box".to_owned() },
        ] {
            assert_eq!(
                fixture.ghost(env.clone(), &cwd, "ls host-only-"),
                "",
                "{env:?} 的文件系统这个进程碰不到，补出宿主路径比补不出来更糟"
            );
            assert!(
                fixture.popup(env.clone(), &cwd, "ls host-only-").is_empty(),
                "弹窗与 ghost 必须同一套分流：{env:?}"
            );
        }
    }

    /// 进程 PATH 描述的是 Windows。把它当成来宾/远端的命令集，补出的
    /// `notepad.exe` 在那边根本不存在。
    #[test]
    fn a_foreign_pane_completes_posix_commands_instead_of_this_machines_binaries() {
        let fixture = Fixture::new();
        assert_eq!(fixture.ghost(SuggestEnv::Local, "", "notepad"), ".exe");

        let wsl = SuggestEnv::Wsl { distro: "Debian".to_owned() };
        assert_eq!(fixture.ghost(wsl.clone(), "", "notepad"), "", "来宾没有 notepad.exe");
        assert_eq!(fixture.ghost(wsl.clone(), "", "grep"), "", "精确命令不再往更长的邻居补");
        assert_eq!(fixture.ghost(wsl.clone(), "", "systemc"), "tl");
        assert!(fixture.popup(wsl, "", "gre").contains(&"p".to_owned()), "弹窗同样走 POSIX 表");
    }

    /// 这张表是"远端 PATH 还没探到"时的兜底，所以里面只能是那边真有的东西。
    #[test]
    fn the_posix_table_names_nothing_windows_only() {
        assert!(
            POSIX_COMMANDS.iter().all(|name| !name.ends_with(".exe")),
            "Windows 可执行后缀不该出现在 POSIX 表里"
        );
        for expected in ["ls", "grep", "sudo", "systemctl", "apt", "cargo"] {
            assert!(POSIX_COMMANDS.contains(&expected), "缺少常用命令 {expected}");
        }
        assert_eq!(posix_commands().len(), POSIX_COMMANDS.len(), "两个视图必须同源");
    }
}
