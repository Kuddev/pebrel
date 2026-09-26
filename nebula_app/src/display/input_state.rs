//! Shared prompt input mirror and terminal-grid reconciliation.
//!
//! Shells still own their line editor. Nebula mirrors plain input only for
//! suggestions and reconciles it with the terminal grid whenever screen truth
//! is available. No window or renderer state belongs in this module.

use nebula_terminal::event::EventListener;
use nebula_terminal::grid::Dimensions;
use nebula_terminal::index::{Column, Line, Point};
use nebula_terminal::term::Term;
use nebula_terminal::term::cell::{Cell, Flags};

use super::{NebulaPaneState, SuggestEnv, nebula_debug_log};

const NEBULA_PROMPT_ARROW: char = '\u{276F}';

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PromptLineSnapshot {
    pub(crate) prompt: String,
    pub(crate) input: String,
}

#[inline(never)]
pub(crate) fn nebula_input_char(state: &mut NebulaPaneState, c: char) {
    state.line_buf.push(c);
    state.touched = true;
    state.clear_completion_hints();
    nebula_debug_log(format!("input_char c={c:?} line_buf={:?}", state.line_buf));
}

pub(crate) fn nebula_input_backspace(state: &mut NebulaPaneState) {
    state.line_buf.pop();
    state.touched = true;
    state.clear_completion_hints();
    nebula_debug_log(format!("input_backspace line_buf={:?}", state.line_buf));
}

pub(crate) fn nebula_input_delete_word(state: &mut NebulaPaneState) {
    state.touched = true;
    while state.line_buf.ends_with(char::is_whitespace) {
        state.line_buf.pop();
    }
    while state.line_buf.chars().last().is_some_and(|c| !c.is_whitespace()) {
        state.line_buf.pop();
    }
    state.clear_completion_hints();
    nebula_debug_log(format!("input_delete_word line_buf={:?}", state.line_buf));
}

/// Merge pasted or IME-committed literal text into the prompt mirror. Text
/// that can execute commands or move the cursor invalidates the mirror.
pub(crate) fn nebula_input_text(state: &mut NebulaPaneState, text: &str) {
    if text.contains(['\r', '\n']) || text.chars().any(|c| c.is_control() && c != '\t') {
        nebula_debug_log(format!("input_text_clear text={text:?}"));
        nebula_clear_line(state);
        return;
    }

    state.line_buf.push_str(text);
    state.touched = true;
    state.clear_completion_hints();
    nebula_debug_log(format!("input_text text={text:?} line_buf={:?}", state.line_buf));
}

pub(crate) fn nebula_clear_line(state: &mut NebulaPaneState) {
    if !state.line_buf.is_empty() || !state.suggestion.is_empty() {
        nebula_debug_log(format!(
            "input_clear line_buf={:?} suggestion={:?}",
            state.line_buf, state.suggestion
        ));
    }
    state.line_buf.clear();
    state.screen_line.clear();
    state.completion_suppressed_line = None;
    state.clear_completion_hints();
}

#[cfg(windows)]
pub(crate) fn nebula_raw_grid_row_preview<T: EventListener>(
    terminal: &Term<T>,
    cursor: Point,
) -> String {
    let grid = terminal.grid();
    let topmost = grid.topmost_line();
    let bottommost = grid.bottommost_line();
    if !raw_grid_line_is_readable(cursor.line, topmost, bottommost) {
        return format!("line={} outside_grid={}..={}", cursor.line.0, topmost.0, bottommost.0);
    }
    let columns = grid.columns();
    let mut text = String::with_capacity(columns);
    let mut arrow_cols = Vec::new();

    for col in 0..columns {
        let cell: &Cell = &grid[cursor.line][Column(col)];
        if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            continue;
        }
        if cell.c == NEBULA_PROMPT_ARROW {
            arrow_cols.push(col);
        }
        text.push(cell.c);
    }

    while text.ends_with(' ') {
        text.pop();
    }

    format!("line={} col={} arrows={arrow_cols:?} text={text:?}", cursor.line.0, cursor.column.0)
}

fn raw_grid_line_is_readable(line: Line, topmost: Line, bottommost: Line) -> bool {
    line >= topmost && line <= bottommost
}

/// Read the real echoed input from the terminal row. This is authoritative for
/// cursor motion, shell completion and history recall that a keystroke mirror
/// cannot reconstruct.
#[cfg(windows)]
pub(crate) fn nebula_input_from_raw_grid<T: EventListener>(
    terminal: &Term<T>,
    cursor: Point,
    typed_tail: &str,
    env: &SuggestEnv,
) -> Option<String> {
    nebula_prompt_line_from_raw_grid(terminal, cursor, typed_tail, env).map(|line| line.input)
}

pub(crate) fn nebula_prompt_line_from_raw_grid<T: EventListener>(
    terminal: &Term<T>,
    cursor: Point,
    typed_tail: &str,
    env: &SuggestEnv,
) -> Option<PromptLineSnapshot> {
    let input_start = terminal.nebula_prompt_input_point();
    let (text, boundary) = raw_grid_line_with_boundary(terminal, cursor, input_start)?;
    if let Some(boundary) = boundary {
        let (prompt, input) = text.split_at(boundary);
        return Some(PromptLineSnapshot { prompt: prompt.to_owned(), input: input.to_owned() });
    }
    if input_start.is_some() {
        return None;
    }
    prompt_line_snapshot(&text, typed_tail, env, terminal.nebula_prompt_active())
}

/// Cold startup has no typed input mirror yet. Readiness is distinct from
/// reconciling user keystrokes, which must continue to reject an empty mirror.
pub(crate) fn nebula_shell_ready_from_raw_grid<T: EventListener>(
    terminal: &Term<T>,
    env: &SuggestEnv,
) -> bool {
    if let Some(line) =
        nebula_prompt_line_from_raw_grid(terminal, terminal.grid().cursor.point, "", env)
    {
        return line.input.trim().is_empty();
    }
    let Some(text) = raw_grid_logical_line(terminal, terminal.grid().cursor.point) else {
        return false;
    };
    let prompt = text.trim_end();
    let Some(marker) = prompt.chars().next_back() else { return false };
    safe_shell_prompt_marker(prompt, marker, env)
        && (terminal.nebula_prompt_active() || likely_prompt(prompt, marker, env))
}

pub(crate) fn nebula_shell_prompt_restored_from_raw_grid<T: EventListener>(
    terminal: &Term<T>,
    expected_prompt: &str,
    env: &SuggestEnv,
) -> bool {
    if terminal.mode().intersects(nebula_terminal::term::TermMode::ALT_SCREEN) {
        return false;
    }
    let cursor = terminal.grid().cursor.point;
    raw_grid_logical_line(terminal, cursor)
        .is_some_and(|line| shell_prompt_restored(expected_prompt, &line, env))
}

fn raw_grid_logical_line<T: EventListener>(terminal: &Term<T>, cursor: Point) -> Option<String> {
    raw_grid_line_with_boundary(terminal, cursor, None).map(|(text, _)| text)
}

fn raw_grid_line_with_boundary<T: EventListener>(
    terminal: &Term<T>,
    cursor: Point,
    input_start: Option<Point>,
) -> Option<(String, Option<usize>)> {
    let grid = terminal.grid();
    if !raw_grid_line_is_readable(cursor.line, grid.topmost_line(), grid.bottommost_line()) {
        return None;
    }
    let columns = grid.columns();
    if columns == 0 {
        return None;
    }
    // A filled last cell leaves the cursor on that cell until another printable
    // character wraps. Its contents already belong to the echoed input.
    let cursor_col = if cursor == grid.cursor.point && grid.cursor.input_needs_wrap {
        columns
    } else {
        cursor.column.0.min(columns)
    };

    if grid[cursor.line][Column(columns - 1)].flags.contains(Flags::WRAPLINE) {
        return None;
    }

    for col in cursor_col..columns {
        let cell: &Cell = &grid[cursor.line][Column(col)];
        if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            continue;
        }
        if !cell.c.is_whitespace() {
            return None;
        }
    }

    let topmost = grid.topmost_line().0;
    let mut first_row = cursor.line.0;
    while first_row > topmost
        && cursor.line.0 - first_row < 64
        && grid[Line(first_row - 1)][Column(columns - 1)].flags.contains(Flags::WRAPLINE)
    {
        first_row -= 1;
    }

    let mut text = String::with_capacity(columns);
    let mut boundary = None;
    for row in first_row..=cursor.line.0 {
        let row_end = if row == cursor.line.0 { cursor_col } else { columns };
        for col in 0..row_end {
            if input_start == Some(Point::new(Line(row), Column(col))) {
                boundary = Some(text.len());
            }
            let cell: &Cell = &grid[Line(row)][Column(col)];
            if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            text.push(cell.c);
        }
    }

    if input_start == Some(Point::new(cursor.line, Column(cursor_col))) {
        boundary = Some(text.len());
    }
    Some((text, boundary))
}

fn prompt_line_snapshot(
    text: &str,
    typed_tail: &str,
    env: &SuggestEnv,
    semantic_prompt: bool,
) -> Option<PromptLineSnapshot> {
    if let Some(arrow_pos) = text.rfind(NEBULA_PROMPT_ARROW) {
        let prompt_end = arrow_pos + NEBULA_PROMPT_ARROW.len_utf8();
        let input = &text[prompt_end..];
        return Some(PromptLineSnapshot {
            prompt: text[..prompt_end].trim_end().to_owned(),
            input: input.strip_prefix(' ').unwrap_or(input).to_owned(),
        });
    }
    // CMD history recall and completion need not update the keystroke mirror.
    if matches!(env, SuggestEnv::Local)
        && let Some((head, input)) = text.split_once('>')
        && cmd_path_prompt(head)
    {
        return Some(PromptLineSnapshot { prompt: format!("{head}>"), input: input.to_owned() });
    }
    if typed_tail.is_empty() || !text.ends_with(typed_tail) {
        return None;
    }
    let prefix = &text[..text.len() - typed_tail.len()];
    let prompt = prefix.trim_end();
    let marker = prompt.chars().next_back()?;
    if marker != NEBULA_PROMPT_ARROW && !semantic_prompt && !likely_prompt(prompt, marker, env) {
        return None;
    }
    Some(PromptLineSnapshot { prompt: prompt.to_owned(), input: typed_tail.to_owned() })
}

fn shell_prompt_restored(expected_prompt: &str, current_line: &str, env: &SuggestEnv) -> bool {
    let expected = expected_prompt.trim_end();
    let current = current_line.trim_end();
    let Some(marker) = expected.chars().next_back() else { return false };
    if !safe_shell_prompt_marker(expected, marker, env) {
        return false;
    }
    if expected == current {
        return true;
    }

    if matches!(env, SuggestEnv::Local)
        && expected.strip_suffix('>').is_some_and(cmd_path_prompt)
        && current.strip_suffix('>').is_some_and(cmd_path_prompt)
    {
        return true;
    }

    !env.is_this_machine()
        && remote_prompt_anchor(expected, marker)
            .zip(remote_prompt_anchor(current, marker))
            .is_some_and(|(expected, current)| expected == current)
}

fn cmd_path_prompt(head: &str) -> bool {
    let bytes = head.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && bytes[2] == b'\\'
        && !head.contains(['<', '>', '|', '\r', '\n'])
}

fn safe_shell_prompt_marker(prompt: &str, marker: char, env: &SuggestEnv) -> bool {
    if matches!(marker, '$' | '#' | '%') {
        return true;
    }
    marker == '>' && likely_prompt(prompt, marker, env)
}

fn remote_prompt_anchor(prompt: &str, expected_marker: char) -> Option<&str> {
    let normalized = prompt.trim_end();
    if normalized.chars().next_back()? != expected_marker {
        return None;
    }
    let head = normalized[..normalized.len() - expected_marker.len_utf8()].trim_end();
    let colon = head.find(':')?;
    let suffix = head[colon + 1..].trim_start();
    if !matches!(suffix.chars().next(), Some('~' | '/')) {
        return None;
    }
    let anchor = head[..colon].trim();
    (!anchor.is_empty() && !anchor.chars().any(char::is_whitespace)).then_some(anchor)
}

fn input_after_prompt(
    text: &str,
    typed_tail: &str,
    env: &SuggestEnv,
    semantic_prompt: bool,
) -> Option<String> {
    prompt_line_snapshot(text, typed_tail, env, semantic_prompt).map(|line| line.input)
}

fn likely_prompt(prompt: &str, marker: char, env: &SuggestEnv) -> bool {
    const THEMED: &[char] = &['❯', '❮', '→', '➜', '➤', '⟩', '»', '›'];
    if THEMED.contains(&marker) {
        return true;
    }
    if matches!(marker, '$' | '#' | '%') {
        return !prompt[..prompt.len() - marker.len_utf8()].trim().is_empty()
            || !env.is_this_machine();
    }
    if marker != '>' {
        return false;
    }

    let head = prompt[..prompt.len() - 1].trim();
    match env {
        SuggestEnv::Local => {
            head.contains(":\\")
                || head.starts_with("\\\\")
                || head.get(1..2) == Some(":")
                || head.starts_with("PS ")
        },
        SuggestEnv::Wsl { .. } | SuggestEnv::Ssh { .. } | SuggestEnv::Shell { .. } => {
            head.contains(['@', ':', '/', '~', ']', ')'])
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_mirror_tracks_plain_edits_and_invalidates_control_text() {
        let mut state = NebulaPaneState::default();
        nebula_input_text(&mut state, "cargo test");
        nebula_input_delete_word(&mut state);
        assert_eq!(state.line_buf, "cargo ");
        nebula_input_backspace(&mut state);
        nebula_input_char(&mut state, 'b');
        assert_eq!(state.line_buf, "cargob");
        nebula_input_text(&mut state, "\rnext");
        assert!(state.line_buf.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn raw_grid_reads_reject_cursor_lines_outside_storage_bounds() {
        let topmost = Line(-1498);
        let bottommost = Line(29);
        assert!(raw_grid_line_is_readable(Line(-1498), topmost, bottommost));
        assert!(raw_grid_line_is_readable(Line(29), topmost, bottommost));
        assert!(!raw_grid_line_is_readable(Line(-1500), topmost, bottommost));
        assert!(!raw_grid_line_is_readable(Line(30), topmost, bottommost));
    }

    #[test]
    fn shell_text_after_cursor_yields_completion_until_accepted() {
        use nebula_terminal::event::VoidListener;
        use nebula_terminal::grid::Grid;

        let size = Grid::<Cell>::new(2, 80, 0);
        let mut terminal = Term::new(Default::default(), &size, VoidListener);
        for (column, c) in "❯ echo native_hint".chars().enumerate() {
            terminal.grid_mut()[Line(0)][Column(column)].c = c;
        }
        // The shell owns the visible suffix, so Pebrel must not overlay it or
        // treat it as an already accepted command. Color is user-configurable.
        let cursor = Point::new(Line(0), Column(7));
        assert_eq!(raw_grid_logical_line(&terminal, cursor), None);
        let accepted = Point::new(Line(0), Column(18));
        assert_eq!(
            raw_grid_logical_line(&terminal, accepted).as_deref(),
            Some("❯ echo native_hint")
        );
    }

    #[test]
    fn semantic_prompt_input_handles_pending_wrap_and_bottom_row_scroll() {
        use nebula_terminal::event::VoidListener;
        use nebula_terminal::event_loop::StreamProcessor;
        use nebula_terminal::grid::Grid;

        for start_row in [1, 2] {
            for prompt_len in [0, 75, 79, 80, 81, 160] {
                for input in ["", "pause"] {
                    let size = Grid::<Cell>::new(2, 80, 0);
                    let mut terminal = Term::new(Default::default(), &size, VoidListener);
                    let prompt = "x".repeat(prompt_len);
                    let bytes =
                        format!("\x1b[{start_row};1H\x1b]133;A\x07{prompt}\x1b]133;B\x07{input}");
                    StreamProcessor::default().feed(&mut terminal, &VoidListener, bytes.as_bytes());
                    let snapshot = nebula_prompt_line_from_raw_grid(
                        &terminal,
                        terminal.grid().cursor.point,
                        "",
                        &SuggestEnv::Local,
                    )
                    .unwrap_or_else(|| {
                        panic!("row {start_row}, prompt {prompt_len}, input {input:?}")
                    });
                    assert_eq!(snapshot.prompt, prompt);
                    assert_eq!(snapshot.input, input);
                    assert_eq!(
                        nebula_shell_ready_from_raw_grid(&terminal, &SuggestEnv::Local),
                        input.is_empty()
                    );
                }
            }
        }
    }

    #[test]
    fn cmd_prompt_reconciles_the_typed_tail() {
        let env = SuggestEnv::Local;
        assert_eq!(
            input_after_prompt(r"D:\temp_build\nebula>git st", "git st", &env, false),
            Some("git st".to_owned())
        );
        assert_eq!(input_after_prompt("Password: secret", "secret", &env, false), None);
    }

    #[test]
    fn remote_prompts_reconcile_without_nebula_arrow() {
        for env in [
            SuggestEnv::Wsl { distro: "Ubuntu".to_owned() },
            SuggestEnv::Ssh { destination: "dev@example.com".to_owned() },
        ] {
            assert_eq!(
                input_after_prompt("dev@box:/work$ cargo t", "cargo t", &env, false),
                Some("cargo t".to_owned())
            );
            assert_eq!(
                input_after_prompt("root@box:/srv# systemc", "systemc", &env, false),
                Some("systemc".to_owned())
            );
        }
    }

    #[test]
    fn reconciliation_requires_screen_and_keystrokes_to_agree() {
        let env = SuggestEnv::Wsl { distro: "Ubuntu".to_owned() };
        assert_eq!(input_after_prompt("dev@box:~$ git status", "git stat", &env, false), None);
        assert_eq!(input_after_prompt("dev@box:~$ ", "", &env, false), None);
        assert_eq!(
            input_after_prompt("custom prompt :: cargo t", "cargo t", &env, true),
            Some("cargo t".to_owned())
        );
    }

    #[test]
    fn nebula_arrow_snapshot_keeps_grid_truth_when_the_mirror_is_stale() {
        let env = SuggestEnv::Local;
        assert_eq!(
            prompt_line_snapshot(r"PS D:\work ❯ git status", "git stat", &env, false),
            Some(PromptLineSnapshot {
                prompt: r"PS D:\work ❯".to_owned(),
                input: "git status".to_owned(),
            })
        );
    }

    #[test]
    fn submitted_remote_command_captures_its_shell_prompt() {
        let env = SuggestEnv::Ssh { destination: "root@box".to_owned() };
        assert_eq!(
            prompt_line_snapshot("u944-pdsgd9gu:~# codex", "codex", &env, false),
            Some(PromptLineSnapshot {
                prompt: "u944-pdsgd9gu:~#".to_owned(),
                input: "codex".to_owned(),
            })
        );
    }

    #[test]
    fn empty_remote_prompt_restores_after_interrupt_or_startup_failure() {
        let ssh = SuggestEnv::Ssh { destination: "root@box".to_owned() };
        assert!(shell_prompt_restored("u944-pdsgd9gu:~#", "u944-pdsgd9gu:~# ", &ssh));

        let wsl = SuggestEnv::Wsl { distro: "Ubuntu".to_owned() };
        assert!(shell_prompt_restored("dev@box:~$", "dev@box:~$ ", &wsl));
    }

    #[test]
    fn prompt_restore_rejects_commands_agent_prompts_and_questions() {
        let env = SuggestEnv::Ssh { destination: "root@box".to_owned() };
        assert!(!shell_prompt_restored("u944-pdsgd9gu:~#", "u944-pdsgd9gu:~# codex", &env));
        assert!(!shell_prompt_restored("u944-pdsgd9gu:~#", "›", &env));
        assert!(!shell_prompt_restored("›", "› ", &env));
        assert!(!shell_prompt_restored("❯", "❯ ", &env));
        assert!(!shell_prompt_restored("Password:", "Password: ", &env));
        assert!(!shell_prompt_restored("u944-pdsgd9gu:~#", "build output#", &env));
    }

    #[test]
    fn remote_prompt_restore_allows_only_same_host_dynamic_cwd() {
        for env in [
            SuggestEnv::Wsl { distro: "Ubuntu".to_owned() },
            SuggestEnv::Shell {
                scope: crate::nebula_history::HistoryScope::Ssh("typed:test".into()),
            },
        ] {
            assert!(shell_prompt_restored("dev@box:~$", "dev@box:/work$ ", &env));
            assert!(!shell_prompt_restored("dev@box:~$", "dev@other:/work$ ", &env));
            assert!(!shell_prompt_restored("dev@box:~$", "build:/work$ ", &env));
        }
    }
}
