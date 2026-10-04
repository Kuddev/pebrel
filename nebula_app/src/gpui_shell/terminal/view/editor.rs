//! Explicit editor snapshots distinguish real input from native prediction text.

use std::collections::VecDeque;

use super::*;

#[derive(Clone, Debug)]
pub(super) struct Snapshot {
    pub line: String,
    pub cursor: usize,
    pub syntax: pebrel_completions::command_context::ShellSyntax,
    epoch: u64,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Action {
    Accept,
    List,
}

#[derive(Default, Debug)]
pub(super) struct Editor {
    owner: Option<String>,
    basic_unicode: bool,
    #[cfg(test)]
    last_report: Option<String>,
    revision: u64,
    waiting: VecDeque<u64>,
    pending: Option<(u64, u64, Action)>,
    queued: Option<u64>,
    action: Option<(u64, Action)>,
    snapshot: Option<Snapshot>,
}

impl Editor {
    #[cfg(test)]
    pub(super) fn ready_for_test(&self) -> bool {
        self.owner.is_some()
    }

    #[cfg(test)]
    pub(super) fn clear_report_for_test(&mut self) {
        self.last_report = None;
    }

    #[cfg(test)]
    pub(super) fn reported_line_for_test(&self) -> Option<&str> {
        self.last_report.as_deref()?.splitn(4, '\n').nth(3)
    }

    pub(super) fn permits_insert(&self, text: &str) -> bool {
        !self.basic_unicode || text.chars().all(|ch| u32::from(ch) <= 0xffff)
    }

    fn advertise(&mut self, value: &str) -> bool {
        let (owner, capability) = value.split_once('\n').unwrap_or((value, "unicode"));
        self.basic_unicode = capability == "basic";
        if self.owner.as_deref() == Some(owner) {
            return false;
        }
        self.owner = Some(owner.to_owned());
        self.waiting.clear();
        self.invalidate();
        true
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn invalidate(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.pending = None;
        self.queued = None;
        self.action = None;
        self.snapshot = None;
    }

    pub(super) fn snapshot(&self, epoch: u64) -> Option<&Snapshot> {
        self.snapshot.as_ref().filter(|snapshot| snapshot.epoch == epoch)
    }

    pub(super) fn is_querying(&self) -> bool {
        self.pending.is_some() || self.queued.is_some() || self.action.is_some()
    }

    pub(super) fn take_action(&mut self, revision: u64) -> Option<Action> {
        let (expected, action) = self.action?;
        if expected != revision {
            return None;
        }
        self.action = None;
        Some(action)
    }

    fn begin(&mut self, epoch: u64, action: Action) -> bool {
        if self.owner.is_none() || self.waiting.len() >= 4 {
            return false;
        }
        if self.pending.is_some() {
            return false;
        }
        self.waiting.push_back(self.revision);
        self.pending = Some((self.revision, epoch, action));
        true
    }

    fn report(&mut self, value: &str) -> Option<Snapshot> {
        let revision = self.waiting.pop_front()?;
        let (expected, epoch, action) = self.pending?;
        if revision != expected || revision != self.revision || value.len() > 4352 {
            return None;
        }
        let mut fields = value.splitn(4, '\n');
        if Some(fields.next()?) != self.owner.as_deref() {
            return None;
        }
        let unit = fields.next()?;
        let position: usize = fields.next()?.trim().parse().ok()?;
        let line = fields.next()?;
        if line.len() > 4096 || line.chars().any(|c| c.is_control() && c != '\t') {
            return None;
        }
        let cursor = match unit {
            "utf8" if line.is_char_boundary(position) => position,
            "utf16" => {
                let mut units = 0;
                let mut cursor = 0;
                for ch in line.chars() {
                    if units == position {
                        break;
                    }
                    units += ch.len_utf16();
                    cursor += ch.len_utf8();
                }
                if units != position {
                    return None;
                }
                cursor
            },
            _ => return None,
        };
        self.pending = None;
        self.action = Some((revision, action));
        let syntax = if unit == "utf16" {
            pebrel_completions::command_context::ShellSyntax::PowerShell
        } else {
            pebrel_completions::command_context::ShellSyntax::Posix
        };
        let snapshot = Snapshot { line: line.to_owned(), cursor, syntax, epoch };
        self.snapshot = Some(snapshot.clone());
        Some(snapshot)
    }
}

impl TerminalView {
    pub(super) fn finish_editor_action(&mut self, cx: &mut Context<Self>) {
        let revision = self.completion_editor.revision();
        let Some(action) = self.completion_editor.take_action(revision) else { return };
        if self.suggest.completion_popup_requested && !self.suggest.completion_items.is_empty() {
            self.suggest.completion_selected = Some(0);
        }
        let accepted = match action {
            Action::Accept => self
                .suggest
                .suggestion_edit
                .take()
                .is_some_and(|item| self.accept_completion_item(item, cx)),
            Action::List => !self.suggest.completion_items.is_empty(),
        };
        if !accepted {
            self.fallback_completion_tab(cx);
        }
    }

    pub(super) fn fallback_completion_tab(&mut self, cx: &mut Context<Self>) {
        self.suggestion_task = None;
        crate::display::nebula_clear_line(&mut self.suggest);
        let key = gpui::Keystroke::parse("tab").unwrap();
        let bytes =
            super::super::keymap::encode(&key, &self.term_mode()).unwrap_or_else(|| vec![b'\t']);
        self.write_user_key(key, bytes, cx);
    }

    pub(super) fn handle_completion_editor_report(
        &mut self,
        name: &str,
        value: &str,
        cx: &mut Context<Self>,
    ) {
        if name == "pebrel_editor_ready"
            && self
                .suggest
                .completion_context
                .owns_current(value.split('\n').next().unwrap_or_default())
        {
            let queued = self.completion_editor.queued.take();
            // A ready event may reach the UI after input has already been sent
            // to that prompt. The same shell must retain its queued Tab intent.
            if self.completion_editor.advertise(value) {
                self.completion_session.invalidate();
                self.suggestion_task = None;
                self.suggest.clear_completion_hints();
            }
            if queued == Some(self.prompt_input_epoch) {
                self.query_completion_editor(cx);
            }
        } else if name == "pebrel_editor" {
            let owner = value.split('\n').next().unwrap_or_default();
            if !self.suggest.completion_context.owns_current(owner) {
                return;
            }
            #[cfg(test)]
            if value.len() <= 4352 {
                self.completion_editor.last_report = Some(value.to_owned());
            }
            if let Some(snapshot) = self.completion_editor.report(value) {
                self.editor_query_task = None;
                self.refresh_suggestion_from_snapshot(Some(snapshot.line), self.suggest_anchor, cx);
            }
        }
    }

    pub(super) fn query_completion_editor(&mut self, cx: &mut Context<Self>) -> bool {
        if self.completion_editor.is_querying() {
            return true;
        }
        let prompt = self.session.as_ref().is_some_and(|session| {
            let term = session.term.lock();
            term.nebula_prompt_active()
                && !term.mode().intersects(TermMode::ALT_SCREEN | TermMode::VI)
        });
        let action = if self.completion_style == crate::display::CompletionStyle::Inline {
            Action::Accept
        } else {
            Action::List
        };
        if !prompt {
            return false;
        }
        let send = self.completion_editor.owner.is_some();
        if send {
            if !self.completion_editor.begin(self.prompt_input_epoch, action) {
                return false;
            }
        } else {
            // Prompt pixels can arrive before their queued capability report.
            self.completion_editor.queued = Some(self.prompt_input_epoch);
        }
        if matches!(action, Action::List) {
            // A native caret move invalidates the grid's prefix. The verified
            // prompt and queued editor request still own this explicit list intent.
            self.suggest.completion_popup_requested = true;
        }
        self.suggest.completion_suppressed_line = None;
        self.suggest.clear_completion_hints();
        if send {
            let powershell = self
                .completion_editor
                .owner
                .as_deref()
                .is_some_and(|owner| owner.starts_with("pwsh:"));
            let key =
                gpui::Keystroke::parse(if powershell { "ctrl-shift-f12" } else { "f24" }).unwrap();
            let bytes =
                super::super::keymap::encode(&key, &self.term_mode()).unwrap_or_else(|| {
                    if powershell { b"\x1b[24;6~".to_vec() } else { b"\x1b[45~".to_vec() }
                });
            self.write_bytes(bytes);
        }
        let revision = self.completion_editor.revision;
        self.editor_query_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_secs(1)).await;
            let _ = this.update(cx, |view, cx| {
                if view.completion_editor.revision == revision
                    && (view
                        .completion_editor
                        .pending
                        .is_some_and(|(pending, _, _)| pending == revision)
                        || view.completion_editor.queued.is_some())
                {
                    view.completion_editor.owner = None;
                    view.completion_editor.waiting.clear();
                    view.completion_editor.invalidate();
                    view.fallback_completion_tab(cx);
                    cx.notify();
                }
            });
        }));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn directory_miss_keeps_manual_tab_until_the_remote_cache_is_ready(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::display::{CompletionStyle, SuggestEnv};
        let (view, window, receiver) = super::startup_tests::open(cx);
        let env = SuggestEnv::Wsl { distro: "completion-delayed-directory-qa".into() };
        assert!(crate::remote_dirs::begin_fetch(&env, "/project"));
        view.update(window, |view, cx| {
            view.suggest.suggest_env = env.clone();
            view.suggest.cwd = "/project".into();
            view.completion_style = CompletionStyle::Hybrid;
            view.ghost_enabled = true;
            view.suggest
                .completion_shell_report(crate::completion_context::SHELL_VAR, "bash:fixture");
            view.completion_editor.advertise("bash:fixture");
            view.session.as_ref().unwrap().term.lock().nebula_add_prompt_mark();
            assert!(view.query_completion_editor(cx));
            assert!(
                view.suggest.completion_popup_requested,
                "caret editing has no readable grid prefix yet"
            );
            view.refresh_suggestion_from_snapshot(None, None, cx);
            assert!(
                view.suggest.completion_popup_requested,
                "a paint before the native reply keeps the list intent"
            );
            view.handle_completion_editor_report(
                "pebrel_editor",
                "bash:fixture\nutf8\n10\ncat readme",
                cx,
            );
        });
        window.run_until_parked();
        let queries: Vec<_> = receiver
            .try_iter()
            .filter_map(|msg| match msg {
                Msg::Input(bytes) => Some(bytes.into_owned()),
                _ => None,
            })
            .collect();
        assert_eq!(
            queries,
            [b"\x1b[45~".to_vec()],
            "the private editor query is the only shell input"
        );
        view.update(window, |view, cx| {
            assert!(view.completion_editor.is_querying());
            assert!(view.suggest.completion_items.is_empty());
            assert!(view.handle_completion_key("enter", cx));
        });
        assert!(
            receiver.try_iter().all(|msg| !matches!(msg, Msg::Input(_))),
            "cache misses must not fall back early"
        );
        crate::remote_dirs::finish_fetch(
            &env,
            "/project",
            Some(vec![crate::remote_dirs::RemoteEntry {
                name: "readme.txt".into(),
                is_dir: false,
            }]),
        );
        view.update(window, |view, cx| {
            view.refresh_suggestion_from_snapshot(None, Some((0, 10)), cx)
        });
        window.run_until_parked();
        view.update(window, |view, _| {
            assert!(!view.completion_editor.is_querying());
            assert_eq!(view.suggest.completion_items[0].insert, ".txt");
        });
        assert!(
            receiver.try_iter().all(|msg| !matches!(msg, Msg::Input(_))),
            "manual lists still only select edits"
        );
    }

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn grid_result_refinement_completes_a_later_native_snapshot(cx: &mut gpui::TestAppContext) {
        use crate::display::{CompletionStyle, SuggestEnv};
        let (view, window, receiver) = super::startup_tests::open(cx);
        let env = SuggestEnv::Wsl { distro: "completion-ready-result-qa".into() };
        crate::remote_dirs::finish_fetch(
            &env,
            "/project",
            Some(vec![crate::remote_dirs::RemoteEntry {
                name: "readme.txt".into(),
                is_dir: false,
            }]),
        );
        view.update(window, |view, cx| {
            view.suggest.suggest_env = env;
            view.suggest.cwd = "/project".into();
            view.completion_style = CompletionStyle::Hybrid;
            view.ghost_enabled = true;
            view.suggest
                .completion_shell_report(crate::completion_context::SHELL_VAR, "bash:cached");
            view.completion_editor.advertise("bash:cached");
            view.session.as_ref().unwrap().term.lock().nebula_add_prompt_mark();
            assert!(view.query_completion_editor(cx));
            // The echoed grid can finish calculating before the private reply.
            view.refresh_suggestion_from_snapshot(Some("cat readme".into()), Some((0, 10)), cx);
        });
        window.run_until_parked();
        view.update(window, |view, cx| {
            assert!(view.completion_editor.is_querying());
            assert_eq!(view.suggest.completion_items[0].insert, ".txt");
            view.handle_completion_editor_report(
                "pebrel_editor",
                "bash:cached\nutf8\n10\ncat readme",
                cx,
            );
        });
        window.run_until_parked();
        view.update(window, |view, _| {
            assert!(
                !view.completion_editor.is_querying(),
                "native refinement must consume the list intent"
            );
            assert_eq!(view.suggest.completion_selected, Some(0));
        });
        let inputs: Vec<_> = receiver
            .try_iter()
            .filter_map(|msg| match msg {
                Msg::Input(bytes) => Some(bytes.into_owned()),
                _ => None,
            })
            .collect();
        assert_eq!(
            inputs,
            [b"\x1b[45~".to_vec()],
            "cached list delivery never types or executes a command"
        );
    }

    #[test]
    fn legacy_editor_capability_declines_lossy_non_bmp_insertion() {
        let mut editor = Editor::default();
        assert!(editor.advertise("pwsh:fixture\nbasic"));
        assert!(editor.permits_insert("中文"));
        assert!(!editor.permits_insert("中文😀"));
        assert!(editor.advertise("pwsh:modern\nunicode"));
        assert!(editor.permits_insert("中文😀"));
    }

    #[test]
    fn repeated_ready_for_the_same_shell_keeps_an_immediate_tab_request() {
        let mut editor = Editor::default();
        assert!(editor.advertise("fixture"));
        assert!(editor.begin(1, Action::Accept));
        let revision = editor.revision();
        assert!(!editor.advertise("fixture"));
        assert_eq!(editor.report("fixture\nutf8\n2\nhi").unwrap().line, "hi");
        assert!(matches!(editor.take_action(revision), Some(Action::Accept)));
        assert!(editor.begin(2, Action::List));
        assert!(editor.advertise("child"));
        assert!(editor.report("fixture\nutf8\n2\nhi").is_none());
        assert!(!editor.is_querying());
    }

    #[test]
    fn native_snapshots_reject_cancelled_queries_and_partial_utf16_positions() {
        let mut editor = Editor { owner: Some("fixture".into()), ..Default::default() };
        assert!(editor.begin(1, Action::List));
        editor.invalidate();
        assert!(editor.begin(1, Action::List));
        assert!(editor.report("fixture\nutf16\n1\na").is_none());
        let snapshot = editor.report("fixture\nutf16\n2\n😀x").unwrap();
        assert_eq!(snapshot.cursor, 4);
        editor.invalidate();
        assert!(editor.begin(2, Action::Accept));
        assert!(editor.report("fixture\nutf16\n1\n😀x").is_none());
    }
}
