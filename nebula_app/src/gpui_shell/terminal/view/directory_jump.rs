//! 单击当前提示符上的目录：请宿主弹出目录选择器，选中后在本 pane 里 `cd`。
//!
//! 只认**还在等输入的那一条**提示符：历史输出里的路径保持原有的选区与
//! Ctrl+点击语义，双击选词不受影响。

use std::path::{Path, PathBuf};

use nebula_terminal::term::cell::Flags;
use unicode_width::UnicodeWidthChar as _;

use super::*;

/// 一条提示符（含它上方的路径行）最多占几行；更远的一律当历史输出。
const PROMPT_MAX_ROWS: i32 = 3;

/// 本 pane 的 shell 用哪种 `cd` 写法；认不出的 shell 不提供跳转。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CdSyntax {
    PowerShell,
    Cmd,
    Posix,
}

impl CdSyntax {
    fn from_shell_tag(tag: &str) -> Option<Self> {
        match tag {
            "pwsh" | "ps" => Some(Self::PowerShell),
            "cmd" => Some(Self::Cmd),
            "bash" | "zsh" | "sh" | "fish" => Some(Self::Posix),
            _ => None,
        }
    }

    /// 先认可执行文件名（`...\WindowsApps\pwsh.exe`），再退回设置 id 或菜单
    /// 显示名。`shell_short_tag` 只认短名，喂完整路径会落到兜底分支。
    fn detect(program: &str, fallback: Option<&str>) -> Option<Self> {
        let file = program.trim().trim_matches('"').rsplit(['/', '\\']).next().unwrap_or("");
        let stem = Path::new(file).file_stem().and_then(|stem| stem.to_str()).unwrap_or("");
        let stem = stem.to_ascii_lowercase();
        Self::from_shell_tag(if stem == "powershell" { "ps" } else { &stem })
            .or_else(|| Self::from_shell_tag(&crate::shell_detect::shell_short_tag(program)))
            .or_else(|| {
                fallback.and_then(|name| {
                    Self::from_shell_tag(&crate::shell_detect::shell_short_tag(name))
                })
            })
    }

    pub(crate) fn command(self, path: &Path) -> Option<String> {
        let path = path.to_str()?;
        if path.is_empty() || path.chars().any(char::is_control) {
            return None;
        }
        Some(match self {
            // PowerShell 把弯引号也当单引号，一并加倍；-LiteralPath 不展开 [ ] 通配。
            Self::PowerShell => {
                let mut quoted = String::with_capacity(path.len() + 2);
                for ch in path.chars() {
                    if matches!(ch, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
                        quoted.push(ch);
                    }
                    quoted.push(ch);
                }
                format!("cd -LiteralPath '{quoted}'")
            },
            Self::Cmd => {
                if path.contains('"') {
                    return None;
                }
                format!("cd /d \"{path}\"")
            },
            Self::Posix => format!("cd '{}'", path.replace('\'', r"'\''")),
        })
    }
}

impl TerminalView {
    fn cd_syntax(&self, cx: &App) -> Option<CdSyntax> {
        if self.ssh_destination.is_some() || !self.suggest.suggest_env.is_this_machine() {
            return None;
        }
        match &self.session_launch {
            crate::session::LaunchSession::Shell { program, name, .. } => {
                CdSyntax::detect(program, Some(name))
            },
            crate::session::LaunchSession::Profile { command, shell_id, .. } => {
                CdSyntax::detect(command, shell_id.as_deref())
            },
            crate::session::LaunchSession::Default => CdSyntax::detect(
                &crate::platform::shell::effective_shell_id(
                    cx.try_global::<Settings>().and_then(|settings| settings.shell_id.as_deref()),
                ),
                None,
            ),
            crate::session::LaunchSession::Ssh { .. } => None,
        }
    }

    /// 单击若落在当前提示符的目录上，返回该目录。shell 不在空提示符上时
    /// （命令在跑、已输入一半、全屏程序）一律不接管，单击保持原样。
    pub(super) fn prompt_directory_at(&self, position: Point<Pixels>, cx: &App) -> Option<PathBuf> {
        let cwd = self.local_cwd()?;
        self.cd_syntax(cx)?;
        let (point, _) = self.grid_point(position);
        let session = self.session.as_ref()?;
        let term = session.term.lock();
        if term.mode().intersects(TermMode::ALT_SCREEN | TermMode::VI)
            || !crate::display::nebula_shell_ready_from_raw_grid(&term, &self.suggest.suggest_env)
        {
            return None;
        }
        // 提示符起点之后、光标所在行为止。再限定离光标不超过几行：标记若
        // 一时没跟上（重绘、清屏），也不会把历史输出里的同名路径当成提示符。
        let cursor = term.grid().cursor.point.line;
        let prompt = match term.nebula_live_prompt_line() {
            Some(line) => line,
            // 改尺寸会清空提示符标记（重排后绝对行号对不上），后台恢复的标签
            // 切到前台时就会这样；shell 仍停在这条空提示符上，按光标往上兜底。
            None if term.nebula_prompt_active() => Line(cursor.0 - PROMPT_MAX_ROWS),
            None => return None,
        };
        if point.line < prompt || point.line > cursor || cursor.0 - point.line.0 > PROMPT_MAX_ROWS {
            return None;
        }
        // 先按 shell 上报的 cwd 在这一行里找它的显示形式：带空格、中文的路径
        // 正则 hint 会截断，这条不会。
        let (text, columns) = row_text(&*term, point.line);
        if displayed_paths(&cwd)
            .iter()
            .any(|shown| span_contains(&text, &columns, shown, point.column))
        {
            return Some(cwd);
        }
        // 自定义提示符可能缩写路径：退回到可点路径 hint，只接受本机已有目录。
        let mods = super::super::osc_links::link_modifiers();
        let config = super::super::osc_links::hint_config();
        let hint = super::super::osc_links::highlighted_at(&*term, &config, point, &mods)?;
        let raw = hint.text(&*term)?.into_owned();
        drop(term);
        match crate::file_uri::classify_link_target_with_cwd(&raw, Some(&cwd)) {
            crate::file_uri::LinkTargetKind::LocalExisting(path)
                if !path.to_string_lossy().starts_with(r"\\") && path.is_dir() =>
            {
                Some(path)
            },
            _ => None,
        }
    }

    /// 真手点击常带一两像素抖动，跨过半格就成了单字选区；没离开同一格的
    /// 选区仍算单击。
    pub(super) fn selection_is_click(&self) -> bool {
        self.session.as_ref().is_none_or(|session| {
            let term = session.term.lock();
            term.selection.as_ref().is_none_or(|selection| {
                selection.is_empty()
                    || selection.to_range(&*term).is_some_and(|range| range.start == range.end)
            })
        })
    }

    pub(super) fn clear_selection(&self) {
        if let Some(session) = &self.session {
            session.term.lock().selection = None;
        }
    }

    /// 在本 pane 的提示符上提交 `cd`。沿用启动命令的通道：shell 没回到空
    /// 提示符前只排队，不会把命令打进正在运行的程序。
    pub(crate) fn change_directory(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        let Some(command) = self.cd_syntax(cx).and_then(|syntax| syntax.command(path)) else {
            return false;
        };
        self.run_command(command, cx);
        true
    }
}

/// 一行网格的文字，以及每个字符起始的列（宽字符占两列）。
fn row_text<T>(term: &nebula_terminal::term::Term<T>, line: Line) -> (Vec<char>, Vec<usize>) {
    let row = &term.grid()[line];
    let mut text = Vec::new();
    let mut columns = Vec::new();
    for col in 0..term.columns() {
        let cell = &row[Column(col)];
        if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            continue;
        }
        text.push(cell.c);
        columns.push(col);
    }
    (text, columns)
}

/// 提示符上可能出现的 cwd 写法：原样，以及家目录缩成 `~` 的形式。
fn displayed_paths(cwd: &Path) -> Vec<Vec<char>> {
    let full = cwd.to_string_lossy().into_owned();
    let mut shown = vec![full.chars().collect::<Vec<_>>()];
    if let Some(home) = crate::platform::dirs::home_dir()
        && let Some(rest) = full.strip_prefix(&*home.to_string_lossy())
    {
        shown.push(format!("~{rest}").chars().collect());
    }
    shown
}

fn span_contains(text: &[char], columns: &[usize], needle: &[char], column: Column) -> bool {
    if needle.is_empty() || needle.len() > text.len() {
        return false;
    }
    text.windows(needle.len()).enumerate().any(|(start, window)| {
        let end = start + needle.len() - 1;
        let last = columns[end] + text[end].width().unwrap_or(1).max(1) - 1;
        window == needle && columns[start] <= column.0 && column.0 <= last
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cd_commands_quote_for_each_shell() {
        let path = Path::new(r"C:\Users\me\it's [x]");
        assert_eq!(
            CdSyntax::PowerShell.command(path).as_deref(),
            Some(r"cd -LiteralPath 'C:\Users\me\it''s [x]'")
        );
        assert_eq!(
            CdSyntax::PowerShell.command(Path::new("C:\\a\u{2019}b")).as_deref(),
            Some("cd -LiteralPath 'C:\\a\u{2019}\u{2019}b'")
        );
        assert_eq!(
            CdSyntax::Cmd.command(Path::new(r"C:\Program Files")).as_deref(),
            Some(r#"cd /d "C:\Program Files""#)
        );
        assert_eq!(CdSyntax::Posix.command(path).as_deref(), Some(r"cd 'C:\Users\me\it'\''s [x]'"));
        assert_eq!(CdSyntax::PowerShell.command(Path::new("C:\\a\nb")), None);
    }

    #[test]
    fn full_program_paths_are_recognised_by_file_name() {
        // Microsoft Store 版 PowerShell 7 的启动路径：整条路径里没有 "powershell" 字样。
        assert_eq!(
            CdSyntax::detect(
                r"C:\Users\alice\AppData\Local\Microsoft\WindowsApps\pwsh.exe",
                Some("PowerShell 7")
            ),
            Some(CdSyntax::PowerShell)
        );
        assert_eq!(
            CdSyntax::detect(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe", None),
            Some(CdSyntax::PowerShell)
        );
        assert_eq!(CdSyntax::detect(r"C:\Windows\system32\cmd.exe", None), Some(CdSyntax::Cmd));
        assert_eq!(CdSyntax::detect("/usr/bin/bash", None), Some(CdSyntax::Posix));
        assert_eq!(CdSyntax::detect("pwsh", None), Some(CdSyntax::PowerShell));
        assert_eq!(CdSyntax::detect("wsl.exe", Some("WSL · Ubuntu")), None);
        assert_eq!(CdSyntax::detect("wsl:Ubuntu", None), None);
    }

    #[test]
    fn shell_tags_pick_a_cd_syntax() {
        let tag = crate::shell_detect::shell_short_tag;
        assert_eq!(
            CdSyntax::from_shell_tag(&tag(r"C:\Program Files\PowerShell\7\pwsh.exe")),
            Some(CdSyntax::PowerShell)
        );
        assert_eq!(CdSyntax::from_shell_tag(&tag("powershell")), Some(CdSyntax::PowerShell));
        assert_eq!(
            CdSyntax::from_shell_tag(&tag(r"C:\Windows\system32\cmd.exe")),
            Some(CdSyntax::Cmd)
        );
        assert_eq!(
            CdSyntax::from_shell_tag(&tag(r"C:\Program Files\Git\bin\bash.exe")),
            Some(CdSyntax::Posix)
        );
        assert_eq!(CdSyntax::from_shell_tag(&tag("wsl:Ubuntu")), None);
        assert_eq!(CdSyntax::from_shell_tag(&tag("nu")), None);
    }

    #[test]
    fn displayed_path_span_maps_wide_characters_to_columns() {
        // "  ~\项目\a  "：中文各占两列，列号跳 2。
        let text: Vec<char> = "  ~\\项目\\a  ".chars().collect();
        let columns = vec![0, 1, 2, 3, 4, 6, 8, 9, 10, 11];
        let needle: Vec<char> = "~\\项目\\a".chars().collect();
        assert!(span_contains(&text, &columns, &needle, Column(2)));
        assert!(span_contains(&text, &columns, &needle, Column(7)));
        assert!(span_contains(&text, &columns, &needle, Column(9)));
        assert!(!span_contains(&text, &columns, &needle, Column(10)));
        assert!(!span_contains(&text, &columns, &needle, Column(1)));
        assert!(!span_contains(&text, &columns, &needle, Column(11)));
    }
}
