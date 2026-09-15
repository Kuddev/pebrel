//! Action context for the legacy shell: the bridge between input handling
//! and terminal state that wires up the `input::Processor` to concrete
//! window, display, clipboard and scheduler instances.

use super::*;
use crate::input;

pub struct ActionContext<'a, N, T> {
    pub pane_id: u64,
    pub notifier: &'a mut N,
    pub terminal: &'a mut Term<T>,
    pub clipboard: &'a mut Clipboard,
    pub mouse: &'a mut Mouse,
    pub touch: &'a mut TouchPurpose,
    pub modifiers: &'a mut Modifiers,
    pub display: &'a mut Display,
    pub windowed_size: &'a mut LogicalSize<u32>,
    pub nebula_state: &'a mut NebulaPaneState,
    pub ssh_destination: Option<&'a str>,
    /// Document shown by the active tab, when it is a viewer tab — wheel and
    /// navigation keys scroll this instead of a grid. `None` on pane tabs.
    pub doc: Option<&'a mut crate::display::markdown_view::DocView>,
    /// Standalone image shown by the active tab. Pointer wheel/drag events
    /// update this state instead of reaching the document stub terminal.
    pub image: Option<&'a mut crate::display::image_viewer::ImageView>,
    pub message_buffer: &'a mut MessageBuffer,
    pub config: &'a UiConfig,
    pub cursor_blink_timed_out: &'a mut bool,
    pub prev_bell_cmd: &'a mut Option<Instant>,
    #[cfg(target_os = "macos")]
    pub event_loop: &'a ActiveEventLoop,
    pub event_proxy: &'a EventLoopProxy<Event>,
    pub scheduler: &'a mut Scheduler,
    pub search_state: &'a mut SearchState,
    pub inline_search_state: &'a mut InlineSearchState,
    pub dirty: &'a mut bool,
    pub occluded: &'a mut bool,
    pub preserve_title: bool,
    #[cfg(not(windows))]
    pub master_fd: RawFd,
    #[cfg(not(windows))]
    pub shell_pid: u32,
}

impl<'a, N: Notify + 'a, T: EventListener> input::ActionContext<T> for ActionContext<'a, N, T> {
    #[inline]
    fn pane_id(&self) -> u64 {
        self.pane_id
    }

    #[inline]
    fn nebula_special_tab_active(&self) -> bool {
        self.display.nebula_special_tab_active
    }

    #[inline]
    fn write_to_pty<B: Into<Cow<'static, [u8]>>>(&self, val: B) {
        self.notifier.notify(val);
    }

    #[inline]
    fn doc_view(&mut self) -> Option<&mut crate::display::markdown_view::DocView> {
        self.doc.as_deref_mut()
    }

    #[inline]
    fn image_view(&mut self) -> Option<&mut crate::display::image_viewer::ImageView> {
        self.image.as_deref_mut()
    }

    /// Request a redraw.
    #[inline]
    fn mark_dirty(&mut self) {
        *self.dirty = true;
    }

    #[inline]
    fn size_info(&self) -> SizeInfo {
        // In split mode this is the focused pane's view, so mouse/selection
        // coordinates map into the focused grid rather than the full window.
        self.display.pane_view()
    }

    fn terminal_math_source_point(&self, point: Point, side: Side) -> (Point, Side) {
        // Formula projection spans live in rendered-viewport coordinates,
        // which can be a crop of the grid while a resize commit is pending.
        self.nebula_state.terminal_math_source_point(
            point,
            side,
            self.terminal.viewport_origin_for(self.size_info().screen_lines()),
        )
    }

    fn scroll(&mut self, scroll: Scroll) {
        let old_offset = self.terminal.grid().display_offset() as i32;

        let old_vi_cursor = self.terminal.vi_mode_cursor;
        self.terminal.scroll_display(scroll);

        let lines_changed = old_offset - self.terminal.grid().display_offset() as i32;

        // Keep track of manual display offset changes during search.
        if self.search_active() {
            self.search_state.display_offset_delta += lines_changed;
        }

        let vi_mode = self.terminal.mode().contains(TermMode::VI);

        // Update selection.
        if vi_mode && self.terminal.selection.as_ref().is_some_and(|s| !s.is_empty()) {
            self.update_selection(self.terminal.vi_mode_cursor.point, Side::Right);
        } else if self.mouse.left_button_state == ElementState::Pressed
            || self.mouse.right_button_state == ElementState::Pressed
        {
            let point = self.mouse.point(&self.size_info(), &*self.terminal);
            let (point, side) = self.terminal_math_source_point(point, self.mouse.cell_side);
            self.update_selection(point, side);
        }

        // Scrolling inside Vi mode moves the cursor, so start typing.
        if vi_mode {
            self.on_typing_start();
        }

        // Update dirty if actually scrolled or moved Vi cursor in Vi mode.
        *self.dirty |=
            lines_changed != 0 || (vi_mode && old_vi_cursor != self.terminal.vi_mode_cursor);
    }

    // Copy text selection.
    fn copy_selection(&mut self, ty: ClipboardType) {
        let text = match self.terminal.selection_to_string().filter(|s| !s.is_empty()) {
            Some(text) => text,
            None => return,
        };

        // 交互设置「选中即复制」：开启时任何选区立即进系统剪贴板；
        // 关闭时选择只保留在终端内，复制交给右键（复制/粘贴）路径。
        if ty == ClipboardType::Selection && self.display.nebula_copy_on_select {
            self.clipboard.store(ClipboardType::Clipboard, text.clone());
        }
        self.clipboard.store(ty, text.clone());
        // Explicit clipboard copies are user actions worth acknowledging.
        // Selection storage is intentionally silent: with copy-on-select it
        // can fire for every mouse-motion update and would spam the toast rail.
        if ty == ClipboardType::Clipboard {
            self.notify_copy(&text);
        }
    }

    /// Show the copy confirmation in the current UI language. Keeping this in
    /// the event layer lets keyboard, context-menu and right-click copies share
    /// exactly one notification path while paste remains silent.
    fn notify_copy(&mut self, text: &str) {
        let lines = text.lines().count().max(1);
        let language = self.display.ui_language();
        let message = match language {
            UiLanguage::ZhCn => format!("已复制 {lines} 行到剪贴板"),
            _ => format!("Copied {lines} lines to clipboard"),
        };
        self.display.push_toast(message, ToastKind::Info);
    }

    fn selection_is_empty(&self) -> bool {
        self.terminal.selection.as_ref().is_none_or(Selection::is_empty)
    }

    fn clear_selection(&mut self) {
        // Clear the selection on the terminal.
        let selection = self.terminal.selection.take();
        if selection.is_some() {
            crate::display::nebula_debug_log(format!(
                "pointer_selection_clear id={} non_empty={}",
                self.mouse.debug_press_id,
                selection.as_ref().is_some_and(|selection| !selection.is_empty())
            ));
        }
        // Mark the terminal as dirty when selection wasn't empty.
        *self.dirty |= selection.is_some_and(|s| !s.is_empty());
    }

    fn update_selection(&mut self, mut point: Point, side: Side) {
        let mut selection = match self.terminal.selection.take() {
            Some(selection) => selection,
            None => {
                crate::display::nebula_debug_log(format!(
                    "pointer_selection_update_ignored id={} point={point:?} side={side:?} reason=no-selection",
                    self.mouse.debug_press_id
                ));
                return;
            },
        };

        // Treat motion over message bar like motion over the last line.
        point.line = min(point.line, self.terminal.bottommost_line());

        // Update selection.
        selection.update(point, side);
        self.mouse.debug_selection_updates = self.mouse.debug_selection_updates.saturating_add(1);
        let update = self.mouse.debug_selection_updates;
        if update <= 3 || update % 10 == 0 {
            crate::display::nebula_debug_log(format!(
                "pointer_selection_update id={} update={} point={point:?} side={side:?} type={:?}",
                self.mouse.debug_press_id, update, selection.ty
            ));
        }

        // Move vi cursor and expand selection.
        if self.terminal.mode().contains(TermMode::VI) && !self.search_active() {
            self.terminal.vi_mode_cursor.point = point;
            selection.include_all();
        }

        self.terminal.selection = Some(selection);
        *self.dirty = true;
    }

    fn start_selection(&mut self, ty: SelectionType, point: Point, side: Side) {
        crate::display::nebula_debug_log(format!(
            "pointer_selection_start id={} type={ty:?} point={point:?} side={side:?} xy=({}, {})",
            self.mouse.debug_press_id, self.mouse.x, self.mouse.y
        ));
        self.terminal.selection = Some(Selection::new(ty, point, side));
        *self.dirty = true;

        self.copy_selection(ClipboardType::Selection);
    }

    fn toggle_selection(&mut self, ty: SelectionType, point: Point, side: Side) {
        match &mut self.terminal.selection {
            Some(selection) if selection.ty == ty && !selection.is_empty() => {
                self.clear_selection();
            },
            Some(selection) if !selection.is_empty() => {
                selection.ty = ty;
                *self.dirty = true;

                self.copy_selection(ClipboardType::Selection);
            },
            _ => self.start_selection(ty, point, side),
        }
    }

    #[inline]
    fn mouse_mode(&self) -> bool {
        self.terminal.mode().intersects(TermMode::MOUSE_MODE)
            && !self.terminal.mode().contains(TermMode::VI)
    }

    #[inline]
    fn mouse_mut(&mut self) -> &mut Mouse {
        self.mouse
    }

    #[inline]
    fn mouse(&self) -> &Mouse {
        self.mouse
    }

    #[inline]
    fn touch_purpose(&mut self) -> &mut TouchPurpose {
        self.touch
    }

    #[inline]
    fn modifiers(&mut self) -> &mut Modifiers {
        self.modifiers
    }

    #[inline]
    fn window(&mut self) -> &mut Window {
        &mut self.display.window
    }

    #[inline]
    fn display(&mut self) -> &mut Display {
        self.display
    }

    #[inline]
    fn terminal(&self) -> &Term<T> {
        self.terminal
    }

    #[inline]
    fn terminal_mut(&mut self) -> &mut Term<T> {
        self.terminal
    }

    #[inline]
    fn nebula_accept(&self) -> crate::display::AcceptKey {
        self.display.nebula_accept
    }

    #[inline]
    fn nebula_take_suggestion(&mut self) -> String {
        mem::take(&mut self.nebula_state.suggestion)
    }

    #[inline]
    fn nebula_completion_popup_active(&self) -> bool {
        !self.nebula_state.completion_items.is_empty()
    }

    fn nebula_completion_popup_move(&mut self, delta: isize) {
        let len = self.nebula_state.completion_items.len();
        if len == 0 {
            return;
        }
        self.nebula_state.completion_selected = Some(match self.nebula_state.completion_selected {
            Some(current) => (current as isize + delta).rem_euclid(len as isize) as usize,
            None => 0,
        });
        *self.dirty = true;
    }

    fn nebula_completion_popup_take(&mut self) -> Option<crate::display::NebulaCompletionItem> {
        let state = &mut self.nebula_state;
        let index = state.completion_selected?;
        let insert = state.completion_items.get(index)?.clone();
        state.completion_items.clear();
        state.completion_selected = None;
        *self.dirty = true;
        Some(insert)
    }

    fn nebula_completion_popup_dismiss(&mut self) -> bool {
        let state = &mut self.nebula_state;
        if state.completion_items.is_empty() {
            return false;
        }
        // Items go, the recompute key stays: the cache guard in
        // `nebula_update_suggestion` then keeps the list closed until the
        // line itself changes.
        state.completion_items.clear();
        state.completion_selected = None;
        *self.dirty = true;
        true
    }

    fn nebula_take_ai_fix(&mut self) -> Option<String> {
        use crate::ai_assistant::AiFixState;
        match self.nebula_state.ai_fix.take() {
            Some(AiFixState::Ready { fix, .. }) => Some(fix.command),
            other => {
                // Pending 放回去：分析中的请求不因误按 Ctrl+. 而丢。
                self.nebula_state.ai_fix = other;
                None
            },
        }
    }

    fn nebula_dismiss_ai_fix(&mut self) -> bool {
        self.nebula_state.ai_fix.take().is_some()
    }

    #[inline]
    fn nebula_input_char(&mut self, c: char) {
        crate::display::nebula_input_char(self.nebula_state, c);
    }

    #[inline]
    fn nebula_input_text(&mut self, text: &str) {
        crate::display::nebula_input_text(self.nebula_state, text);
    }

    #[inline]
    fn nebula_input_backspace(&mut self) {
        crate::display::nebula_input_backspace(self.nebula_state);
    }

    #[inline]
    fn nebula_delete_word(&mut self) {
        crate::display::nebula_input_delete_word(self.nebula_state);
    }

    #[inline]
    fn nebula_commit_line(&mut self) {
        // Snapshot the input straight off the grid at Enter time: the shell
        // hasn't processed the newline yet, so the row still shows the full
        // line, while the cached `screen_line` is one draw behind and commits
        // a truncated command on type-fast-then-Enter.
        let agent_already_active = self
            .nebula_state
            .running_program
            .as_deref()
            .and_then(crate::ai_agents::AgentKind::parse)
            .is_some();
        if !agent_already_active {
            self.nebula_state.pending_command_prompt = None;
        }
        #[cfg(windows)]
        if !agent_already_active
            && !self.terminal.mode().intersects(TermMode::ALT_SCREEN | TermMode::VI)
            && self.search_state.regex().is_none()
        {
            let cursor = self.terminal.grid().cursor.point;
            match crate::display::nebula_prompt_line_from_raw_grid(
                self.terminal,
                cursor,
                &self.nebula_state.line_buf,
                &self.nebula_state.suggest_env,
            ) {
                Some(line) => {
                    self.nebula_state.screen_line = line.input;
                    self.nebula_state.pending_command_prompt = Some(line.prompt);
                },
                // A failed read means the cached copy is stale too — an
                // earlier partial line must not get recorded as this command.
                None => {
                    self.nebula_state.screen_line.clear();
                    self.nebula_state.pending_command_prompt = None;
                },
            }
        }
        self.display.nebula_commit_line(self.nebula_state);
    }

    #[inline]
    fn nebula_clear_line(&mut self) {
        crate::display::nebula_clear_line(self.nebula_state);
    }

    fn nebula_tab(&self, request: TabRequest) {
        let _ = self.event_proxy.send_event(Event {
            window_id: Some(self.display.window.id()),
            tab_id: None,
            payload: EventType::NebulaTab(request),
        });
    }

    fn refresh_terminal_profiles(&mut self) {
        let _ = self.event_proxy.send_event(Event {
            window_id: None,
            tab_id: None,
            payload: EventType::TerminalProfilesChanged,
        });
    }

    fn nebula_sync(&self, push: bool) {
        let _ = self.event_proxy.send_event(Event {
            window_id: Some(self.display.window.id()),
            tab_id: None,
            payload: EventType::NebulaSync { push },
        });
    }

    fn nebula_backup_remote(&self, request: crate::display::RemoteBackupRequest) {
        let _ = self.event_proxy.send_event(Event {
            window_id: Some(self.display.window.id()),
            tab_id: None,
            payload: EventType::NebulaBackupRemote {
                upload: request.upload,
                passphrase: request.passphrase,
                selection: request.selection,
            },
        });
    }

    fn nebula_local_proxy_scan(&mut self) {
        if !self.display.take_local_proxy_scan_request() {
            return;
        }
        let _ = self
            .event_proxy
            .send_event(Event::new(EventType::LocalProxyScan, self.display.window.id()));
    }

    fn nebula_proxy_test(&mut self) {
        let Some(request_id) = self.display.take_proxy_test_request() else { return };
        if let Err(err) = crate::ssh_session::spawn_proxy_test(
            request_id,
            self.event_proxy.clone(),
            self.display.window.id(),
        ) {
            self.display.proxy_test_done(
                request_id,
                crate::proxy_test::ProxyTestOutcome::Failed(
                    crate::proxy_test::ProxyTestFailure::Start(err.to_string()),
                ),
                0,
            );
        }
    }

    fn nebula_provider_test(&mut self) {
        let Some(request) = self.display.take_provider_test_request() else { return };
        let request_id = request.request_id;
        let provider_id = request.provider.id.clone();
        if let Err(err) = crate::ai_providers::spawn_test(
            request,
            self.event_proxy.clone(),
            self.display.window.id(),
        ) {
            self.display.provider_test_done(
                request_id,
                &provider_id,
                &crate::provider_test::ProviderTestOutcome::StartFailed { error: err.to_string() },
                0,
            );
        }
    }

    fn nebula_quick_hotkey_changed(&mut self) {
        let Some(hotkey) = self.display.take_quick_hotkey_request() else { return };
        let _ = self.event_proxy.send_event(Event::new(
            EventType::QuickTerminalHotkeyChanged { hotkey },
            self.display.window.id(),
        ));
    }

    /// SSH 编辑器「测试连接」：display 侧点击时暂存的请求在这里被取走，
    /// 交给共享 SSH runtime 执行；结果以 [`EventType::SshTestDone`] 回流。
    fn nebula_ssh_test(&mut self) {
        let Some(request) = self.display.take_ssh_test_request() else { return };
        let request_id = request.request_id;
        let destination = request.destination.clone();
        if let Err(err) = crate::ssh_session::spawn_test(
            request,
            self.event_proxy.clone(),
            self.display.window.id(),
        ) {
            self.display.ssh_test_done(
                request_id,
                &destination,
                false,
                &format!("无法启动测试任务：{err}"),
                0,
            );
        }
    }

    fn nebula_open_sftp(&mut self, destination: String) {
        if let Err(err) = self.display.open_sftp_panel(destination, self.event_proxy.clone()) {
            log::error!("{err}");
        }
    }

    fn nebula_ssh_destination(&self) -> Option<&str> {
        self.ssh_destination
    }

    fn spawn_new_instance(&mut self) {
        let mut env_args = env::args();
        let nebula = env_args.next().unwrap();

        let mut args: Vec<String> = Vec::new();

        // Reuse the arguments passed to Nebula for the new instance.
        #[allow(clippy::while_let_on_iterator)]
        while let Some(arg) = env_args.next() {
            // New instances shouldn't inherit command.
            if arg == "-e" || arg == "--command" {
                break;
            }

            // On unix, the working directory of the foreground shell is used by `start_daemon`.
            #[cfg(not(windows))]
            if arg == "--working-directory" {
                let _ = env_args.next();
                continue;
            }

            args.push(arg);
        }

        self.spawn_daemon(&nebula, &args);
    }

    #[cfg(not(windows))]
    fn create_new_window(&mut self, #[cfg(target_os = "macos")] tabbing_id: Option<String>) {
        let mut options = WindowOptions::default();
        options.terminal_options.working_directory =
            foreground_process_path(self.master_fd, self.shell_pid).ok();

        #[cfg(target_os = "macos")]
        {
            options.window_tabbing_id = tabbing_id;
        }

        let _ = self.event_proxy.send_event(Event::new(EventType::CreateWindow(options), None));
    }

    #[cfg(windows)]
    fn create_new_window(&mut self) {
        let _ = self
            .event_proxy
            .send_event(Event::new(EventType::CreateWindow(WindowOptions::default()), None));
    }

    fn spawn_daemon<I, S>(&self, program: &str, args: I)
    where
        I: IntoIterator<Item = S> + Debug + Copy,
        S: AsRef<OsStr>,
    {
        #[cfg(not(windows))]
        let result = spawn_daemon(program, args, self.master_fd, self.shell_pid);
        #[cfg(windows)]
        let result = spawn_daemon(program, args);

        match result {
            Ok(_) => debug!("Launched {program} with args {args:?}"),
            Err(err) => warn!("Unable to launch {program} with args {args:?}: {err}"),
        }
    }

    fn change_font_size(&mut self, delta: f32) {
        let scale = self.display.window.scale_factor as f32;
        // Hard bounds keep runaway zooms recoverable. Without them a stuck
        // modifier or trackpad burst can scroll the terminal to 180 px+,
        // where a ±1-step notch changes the size by under 1 % and zooming
        // back out reads as "broken". Logical 4–64 px covers everything from
        // dense logs to presentations.
        let (min_px, max_px) = (4.0 * scale, 64.0 * scale);
        // Round to pick integral px steps, since fonts look better on them.
        let new_size = (self.display.font_size.as_px().round() + delta).clamp(min_px, max_px);
        self.display.font_size = FontSize::from_px(new_size);
        let font = self.display.effective_font(&self.config.font).with_size(self.display.font_size);
        self.display.pending_update.set_font(font);
    }

    fn reset_font_size(&mut self) {
        let scale_factor = self.display.window.scale_factor as f32;
        self.display.font_size = self.config.font.size().scale(scale_factor);
        let font = self.display.effective_font(&self.config.font).with_size(self.display.font_size);
        self.display.pending_update.set_font(font);
    }

    fn apply_default_cursor_style(&mut self) {
        // 用户在设置页显式选了光标样式：先清掉 shell 早前用 DECSCUSR 钉住
        // 的覆盖（PSReadLine/starship 启动时常发），否则新默认被旧覆盖压
        // 住、"改了不生效"；之后 vim 等程序再发 DECSCUSR 仍可正常覆盖。
        self.terminal.reset_cursor_style_override();
        self.update_cursor_blinking();
    }

    #[inline]
    fn pop_message(&mut self) {
        if !self.message_buffer.is_empty() {
            self.display.pending_update.dirty = true;
            self.message_buffer.pop();
        }
    }

    #[inline]
    fn start_search(&mut self, direction: Direction) {
        // Only create new history entry if the previous regex wasn't empty.
        if self.search_state.history.front().is_none_or(|regex| !regex.is_empty()) {
            self.search_state.history.push_front(String::new());
            self.search_state.history.truncate(MAX_SEARCH_HISTORY_SIZE);
        }

        self.search_state.history_index = Some(0);
        self.search_state.direction = direction;
        self.search_state.focused_match = None;

        // Store original search position as origin and reset location.
        if self.terminal.mode().contains(TermMode::VI) {
            self.search_state.origin = self.terminal.vi_mode_cursor.point;
            self.search_state.display_offset_delta = 0;

            // Adjust origin for content moving upward on search start.
            if self.terminal.grid().cursor.point.line + 1 == self.terminal.screen_lines() {
                self.search_state.origin.line -= 1;
            }
        } else {
            let viewport_top = Line(-(self.terminal.grid().display_offset() as i32)) - 1;
            let viewport_bottom = viewport_top + self.terminal.bottommost_line();
            let last_column = self.terminal.last_column();
            self.search_state.origin = match direction {
                Direction::Right => Point::new(viewport_top, Column(0)),
                Direction::Left => Point::new(viewport_bottom, last_column),
            };
        }

        // Remove vi mode IME inhibitor, so the user can input the target character.
        self.window().set_ime_inhibitor(ImeInhibitor::VI, false);

        self.display.damage_tracker.frame().mark_fully_damaged();
        self.display.pending_update.dirty = true;
    }

    #[inline]
    fn start_seeded_search(&mut self, direction: Direction, text: String) {
        let origin = self.terminal.vi_mode_cursor.point;

        // Start new search.
        self.clear_selection();
        self.start_search(direction);

        // Enter initial selection text.
        for c in text.chars() {
            if let '$' | '('..='+' | '?' | '['..='^' | '{'..='}' = c {
                self.search_input('\\');
            }
            self.search_input(c);
        }

        // Leave search mode.
        self.confirm_search();

        if !self.terminal.mode().contains(TermMode::VI) {
            return;
        }

        // Find the target vi cursor point by going to the next match to the right of the origin,
        // then jump to the next search match in the target direction.
        let target = self.search_next(origin, Direction::Right, Side::Right).and_then(|rm| {
            let regex_match = match direction {
                Direction::Right => {
                    let origin = rm.end().add(self.terminal, Boundary::None, 1);
                    self.search_next(origin, Direction::Right, Side::Left)?
                },
                Direction::Left => {
                    let origin = rm.start().sub(self.terminal, Boundary::None, 1);
                    self.search_next(origin, Direction::Left, Side::Left)?
                },
            };
            Some(*regex_match.start())
        });

        // Move the vi cursor to the target position.
        if let Some(target) = target {
            self.terminal_mut().vi_goto_point(target);
            self.mark_dirty();
        }
    }

    #[inline]
    fn confirm_search(&mut self) {
        // Just cancel search when not in vi mode.
        if !self.terminal.mode().contains(TermMode::VI) {
            self.cancel_search();
            return;
        }

        // Force unlimited search if the previous one was interrupted.
        let timer_id = TimerId::new(Topic::DelayedSearch, self.display.window.id());
        if self.scheduler.scheduled(timer_id) {
            self.goto_match(None);
        }

        self.exit_search();
    }

    #[inline]
    fn cancel_search(&mut self) {
        if self.terminal.mode().contains(TermMode::VI) {
            // Recover pre-search state in vi mode.
            self.search_reset_state();
        } else if let Some(focused_match) = &self.search_state.focused_match {
            // Create a selection for the focused match.
            let start = *focused_match.start();
            let end = *focused_match.end();
            self.start_selection(SelectionType::Simple, start, Side::Left);
            self.update_selection(end, Side::Right);
            self.copy_selection(ClipboardType::Selection);
        }

        self.search_state.dfas = None;

        self.exit_search();
    }

    #[inline]
    fn search_input(&mut self, c: char) {
        match self.search_state.history_index {
            Some(0) => (),
            // When currently in history, replace active regex with history on change.
            Some(index) => {
                self.search_state.history[0] = self.search_state.history[index].clone();
                self.search_state.history_index = Some(0);
            },
            None => return,
        }
        let regex = &mut self.search_state.history[0];

        match c {
            // Handle backspace/ctrl+h.
            '\x08' | '\x7f' => {
                let _ = regex.pop();
            },
            // Add ascii and unicode text.
            ' '..='~' | '\u{a0}'..='\u{10ffff}' => regex.push(c),
            // Ignore non-printable characters.
            _ => return,
        }

        if !self.terminal.mode().contains(TermMode::VI) {
            // Clear selection so we do not obstruct any matches.
            self.terminal.selection = None;
        }

        self.update_search();
    }

    #[inline]
    fn search_pop_word(&mut self) {
        if let Some(regex) = self.search_state.regex_mut() {
            *regex = regex.trim_end().to_owned();
            regex.truncate(regex.rfind(' ').map_or(0, |i| i + 1));
            self.update_search();
        }
    }

    /// Go to the previous regex in the search history.
    #[inline]
    fn search_history_previous(&mut self) {
        let index = match &mut self.search_state.history_index {
            None => return,
            Some(index) if *index + 1 >= self.search_state.history.len() => return,
            Some(index) => index,
        };

        *index += 1;
        self.update_search();
    }

    /// Go to the previous regex in the search history.
    #[inline]
    fn search_history_next(&mut self) {
        let index = match &mut self.search_state.history_index {
            Some(0) | None => return,
            Some(index) => index,
        };

        *index -= 1;
        self.update_search();
    }

    #[inline]
    fn advance_search_origin(&mut self, direction: Direction) {
        // Use focused match as new search origin if available.
        if let Some(focused_match) = &self.search_state.focused_match {
            let new_origin = match direction {
                Direction::Right => focused_match.end().add(self.terminal, Boundary::None, 1),
                Direction::Left => focused_match.start().sub(self.terminal, Boundary::None, 1),
            };

            self.terminal.scroll_to_point(new_origin);

            self.search_state.display_offset_delta = 0;
            self.search_state.origin = new_origin;
        }

        // Search for the next match using the supplied direction.
        let search_direction = mem::replace(&mut self.search_state.direction, direction);
        self.goto_match(None);
        self.search_state.direction = search_direction;

        // If we found a match, we set the search origin right in front of it to make sure that
        // after modifications to the regex the search is started without moving the focused match
        // around.
        let focused_match = match &self.search_state.focused_match {
            Some(focused_match) => focused_match,
            None => return,
        };

        // Set new origin to the left/right of the match, depending on search direction.
        let new_origin = match self.search_state.direction {
            Direction::Right => *focused_match.start(),
            Direction::Left => *focused_match.end(),
        };

        // Store the search origin with display offset by checking how far we need to scroll to it.
        let old_display_offset = self.terminal.grid().display_offset() as i32;
        self.terminal.scroll_to_point(new_origin);
        let new_display_offset = self.terminal.grid().display_offset() as i32;
        self.search_state.display_offset_delta = new_display_offset - old_display_offset;

        // Store origin and scroll back to the match.
        self.terminal.scroll_display(Scroll::Delta(-self.search_state.display_offset_delta));
        self.search_state.origin = new_origin;
    }

    /// Find the next search match.
    fn search_next(&mut self, origin: Point, direction: Direction, side: Side) -> Option<Match> {
        self.search_state
            .dfas
            .as_mut()
            .and_then(|dfas| self.terminal.search_next(dfas, origin, direction, side, None))
    }

    #[inline]
    fn search_direction(&self) -> Direction {
        self.search_state.direction
    }

    #[inline]
    fn search_active(&self) -> bool {
        self.search_state.history_index.is_some()
    }

    /// Handle keyboard typing start.
    ///
    /// This will temporarily disable some features like terminal cursor blinking or the mouse
    /// cursor.
    ///
    /// All features are re-enabled again automatically.
    #[inline]
    fn on_typing_start(&mut self) {
        // Disable cursor blinking.
        let timer_id = TimerId::new(Topic::BlinkCursor, self.display.window.id());
        if self.scheduler.unschedule(timer_id).is_some() {
            self.schedule_blinking();

            // Mark the cursor as visible and queue redraw if the cursor was hidden.
            if mem::take(&mut self.display.cursor_hidden) {
                *self.dirty = true;
            }
        } else if *self.cursor_blink_timed_out {
            self.update_cursor_blinking();
        }

        // Hide mouse cursor.
        if self.config.mouse.hide_when_typing && self.display.window.mouse_visible() {
            self.display.window.set_mouse_visible(false);

            // Request hint highlights update, since the mouse may have been hovering a hint.
            self.mouse.hint_highlight_dirty = true
        }
    }

    /// Process a new character for keyboard hints.
    fn hint_input(&mut self, c: char) {
        if let Some(hint) = self.display.hint_state.keyboard_input(self.terminal, c) {
            self.mouse.block_hint_launcher = false;
            self.trigger_hint(&hint);
        }
        *self.dirty = true;
    }

    /// Open a filesystem path with the system default handler (the drawer's
    /// double-click). `explorer.exe` handles files AND folders, and sidesteps
    /// `cmd /c start` mangling spaces/unicode (same as file:// hints).
    fn open_path(&mut self, path: &std::path::Path) {
        #[cfg(windows)]
        self.spawn_daemon("explorer.exe", &[path.as_os_str()]);
        #[cfg(not(windows))]
        self.spawn_daemon("xdg-open", &[path.as_os_str()]);
    }

    /// 资源管理器里定位到条目本身（文件树右键「在资源管理器中显示」）。
    /// `/select,` 与路径必须是同一个参数，逗号后直接拼路径。
    fn reveal_in_file_manager(&mut self, path: &std::path::Path) {
        #[cfg(windows)]
        {
            let mut arg = std::ffi::OsString::from("/select,");
            arg.push(path.as_os_str());
            self.spawn_daemon("explorer.exe", &[arg.as_os_str()]);
        }
        #[cfg(not(windows))]
        if let Some(parent) = path.parent() {
            self.spawn_daemon("xdg-open", &[parent.as_os_str()]);
        }
    }

    /// Trigger a hint action.
    fn trigger_hint(&mut self, hint: &HintMatch) {
        crate::display::nebula_link_log(format!(
            "trigger_hint block={} hyperlink={}",
            self.mouse.block_hint_launcher,
            hint.hyperlink().is_some()
        ));
        if self.mouse.block_hint_launcher {
            return;
        }

        let hint_bounds = hint.bounds();
        let text = match hint.text(self.terminal) {
            Some(text) => text,
            None => return,
        };

        match &hint.action() {
            // Launch an external program.
            HintAction::Command(command) => {
                // On Windows, a `file://` OSC 8 link (our clickable `ls`) is
                // opened via `explorer.exe` with a translated native path. This
                // sidesteps `cmd /c start` mangling spaces/unicode and lets
                // WSL/MSYS posix paths (`/mnt/c/…`, `/d/…`) actually resolve.
                #[cfg(windows)]
                if let Some(path) = crate::file_uri::file_uri_to_local_path(&text) {
                    crate::display::nebula_link_log(format!(
                        "trigger_hint file-uri explorer path={path:?} (from {text:?})"
                    ));
                    self.spawn_daemon("explorer.exe", &[path.as_os_str()]);
                    return;
                }

                let mut args = command.args().to_vec();
                args.push(text.into());
                crate::display::nebula_link_log(format!(
                    "trigger_hint spawn program={:?} args={args:?}",
                    command.program()
                ));
                self.spawn_daemon(command.program(), &args);
            },
            // Copy the text to the clipboard.
            HintAction::Action(HintInternalAction::Copy) => {
                self.clipboard.store(ClipboardType::Clipboard, text.clone());
                self.notify_copy(&text);
            },
            // Write the text to the PTY/search.
            HintAction::Action(HintInternalAction::Paste) => self.paste(&text, true),
            // Select the text.
            HintAction::Action(HintInternalAction::Select) => {
                self.start_selection(SelectionType::Simple, *hint_bounds.start(), Side::Left);
                self.update_selection(*hint_bounds.end(), Side::Right);
                self.copy_selection(ClipboardType::Selection);
            },
            // Move the vi mode cursor.
            HintAction::Action(HintInternalAction::MoveViModeCursor) => {
                // Enter vi mode if we're not in it already.
                if !self.terminal.mode().contains(TermMode::VI) {
                    self.terminal.toggle_vi_mode();
                }

                self.terminal.vi_goto_point(*hint_bounds.start());
                self.mark_dirty();
            },
        }
    }

    /// Expand the selection to the current mouse cursor position.
    #[inline]
    fn expand_selection(&mut self) {
        let control = self.modifiers().state().control_key();
        let selection_type = match self.mouse().click_state {
            ClickState::None => return,
            _ if control => SelectionType::Block,
            ClickState::Click => SelectionType::Simple,
            ClickState::DoubleClick => SelectionType::Semantic,
            ClickState::TripleClick => SelectionType::Lines,
        };

        // Load mouse point, treating message bar and padding as the closest cell.
        let point = self.mouse().point(&self.size_info(), self.terminal());
        let (point, cell_side) = self.terminal_math_source_point(point, self.mouse().cell_side);

        let selection = match &mut self.terminal_mut().selection {
            Some(selection) => selection,
            None => return,
        };

        selection.ty = selection_type;
        self.update_selection(point, cell_side);

        // Move vi mode cursor to mouse click position.
        if self.terminal().mode().contains(TermMode::VI) && !self.search_active() {
            self.terminal_mut().vi_mode_cursor.point = point;
        }
    }

    /// Get the semantic word at the specified point.
    fn semantic_word(&self, point: Point) -> String {
        let terminal = self.terminal();
        let grid = terminal.grid();

        // Find the next semantic word boundary to the right.
        let mut end = terminal.semantic_search_right(point);

        // Get point at which skipping over semantic characters has led us back to the
        // original character.
        let start_cell = &grid[point];
        let search_end = if start_cell.flags.intersects(Flags::LEADING_WIDE_CHAR_SPACER) {
            point.add(terminal, Boundary::None, 2)
        } else if start_cell.flags.intersects(Flags::WIDE_CHAR) {
            point.add(terminal, Boundary::None, 1)
        } else {
            point
        };

        // Keep moving until we're not on top of a semantic escape character.
        let semantic_chars = terminal.semantic_escape_chars();
        loop {
            let cell = &grid[end];

            // Get cell's character, taking wide characters into account.
            let c = if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                grid[end.sub(terminal, Boundary::None, 1)].c
            } else {
                cell.c
            };

            if !semantic_chars.contains(c) {
                break;
            }

            end = terminal.semantic_search_right(end.add(terminal, Boundary::None, 1));

            // Stop if the entire grid is only semantic escape characters.
            if end == search_end {
                return String::new();
            }
        }

        // Find the beginning of the semantic word.
        let start = terminal.semantic_search_left(end);

        terminal.bounds_to_string(start, end)
    }

    /// Handle beginning of terminal text input.
    fn on_terminal_input_start(&mut self) {
        self.on_typing_start();
        self.clear_selection();

        if self.terminal().grid().display_offset() != 0 {
            self.scroll(Scroll::Bottom);
        }
    }

    /// 剪贴板截图转路径粘贴（无文本时的回退）。本地 pane 同步落盘临时 PNG
    /// 直接粘；SSH pane 交给 SFTP 后台上传，路径经 runtime Prompt 通道回粘
    /// ——期间不阻塞任何输入。
    fn paste_clipboard_image(&mut self) -> bool {
        let Some(png) = crate::clipboard::clipboard_image_png() else {
            return false;
        };
        if let Some(destination) = self.ssh_destination {
            let proxy = self.event_proxy.clone();
            let pane_id = self.pane_id;
            crate::ssh_sftp::upload_clipboard_image(destination.to_owned(), png, move |remote| {
                crate::runtime_api::dispatch_prompt(&proxy, pane_id, remote);
            });
            return true;
        }
        let Ok(path) = crate::clipboard::stage_image_png(&png) else { return false };
        let Ok(path) = path.keep() else { return false };
        self.paste(&path.display().to_string(), true);
        true
    }

    /// Paste a text into the terminal.
    fn paste(&mut self, text: &str, bracketed: bool) {
        // Multi-line paste confirmation (#18): a newline heading to a bare
        // shell starts executing the moment it lands. But an app in
        // bracketed-paste mode — codex, vim, a REPL, modern PSReadLine —
        // receives the whole paste as one chunk (wrapped below in
        // `\x1b[200~`…`\x1b[201~`) and decides what to do with the newlines
        // itself, so it is *not* executed line by line and the warning both
        // misleads and gets in the way (#35). Confirm only for the genuinely
        // dangerous case: newlines going to a shell that is not bracketing.
        // Search and pending-char inputs are exempt (they consume text
        // locally).
        let goes_to_pty = !self.search_active() && !self.inline_search_state.char_pending;
        let bracketing = bracketed && self.terminal().mode().contains(TermMode::BRACKETED_PASTE);
        if goes_to_pty
            && !bracketing
            && self.display.nebula_confirm.is_none()
            && (text.contains('\n') || text.contains('\r'))
        {
            let lines = text.lines().count().max(2);
            self.display.nebula_confirm = Some(crate::display::NebulaConfirm::Paste {
                pane_id: self.pane_id,
                text: text.to_owned(),
                bracketed,
                lines,
            });
            *self.dirty = true;
            return;
        }
        self.paste_now(text, bracketed);
    }

    fn paste_now(&mut self, text: &str, bracketed: bool) {
        if self.search_active() {
            for c in text.chars() {
                self.search_input(c);
            }
        } else if self.inline_search_state.char_pending {
            self.inline_search_input(text);
        } else if bracketed && self.terminal().mode().contains(TermMode::BRACKETED_PASTE) {
            self.on_terminal_input_start();

            self.write_to_pty(&b"\x1b[200~"[..]);

            // Write filtered escape sequences.
            //
            // We remove `\x1b` to ensure it's impossible for the pasted text to write the bracketed
            // paste end escape `\x1b[201~` and `\x03` since some shells incorrectly terminate
            // bracketed paste when they receive it.
            let filtered = text.replace(['\x1b', '\x03'], "");
            self.nebula_input_text(&filtered);
            self.write_to_pty(filtered.into_bytes());

            self.write_to_pty(&b"\x1b[201~"[..]);
        } else {
            self.on_terminal_input_start();

            let payload = if bracketed {
                // In non-bracketed (ie: normal) mode, terminal applications cannot distinguish
                // pasted data from keystrokes.
                //
                // In theory, we should construct the keystrokes needed to produce the data we are
                // pasting... since that's neither practical nor sensible (and probably an
                // impossible task to solve in a general way), we'll just replace line breaks
                // (windows and unix style) with a single carriage return (\r, which is what the
                // Enter key produces).
                text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
            } else {
                // When we explicitly disable bracketed paste don't manipulate with the input,
                // so we pass user input as is.
                text.to_owned().into_bytes()
            };

            if bracketed {
                if let Ok(text) = std::str::from_utf8(&payload) {
                    self.nebula_input_text(text);
                } else {
                    self.nebula_clear_line();
                }
            }
            self.write_to_pty(payload);
        }
    }

    /// Toggle the vi mode status.
    #[inline]
    fn toggle_vi_mode(&mut self) {
        let was_in_vi_mode = self.terminal.mode().contains(TermMode::VI);
        if was_in_vi_mode {
            // If we had search running when leaving Vi mode we should mark terminal fully damaged
            // to cleanup highlighted results.
            if self.search_state.dfas.take().is_some() {
                self.display.damage_tracker.frame().mark_fully_damaged();
            }
        } else {
            self.clear_selection();
        }

        if self.search_active() {
            self.cancel_search();
        }

        // We don't want IME in Vi mode.
        self.window().set_ime_inhibitor(ImeInhibitor::VI, !was_in_vi_mode);

        self.terminal.toggle_vi_mode();

        *self.dirty = true;
    }

    /// Get vi inline search state.
    fn inline_search_state(&mut self) -> &mut InlineSearchState {
        self.inline_search_state
    }

    /// Start vi mode inline search.
    fn start_inline_search(&mut self, direction: Direction, stop_short: bool) {
        self.inline_search_state.stop_short = stop_short;
        self.inline_search_state.direction = direction;
        self.inline_search_state.char_pending = true;
        self.inline_search_state.character = None;
    }

    /// Jump to the next matching character in the line.
    fn inline_search_next(&mut self) {
        let direction = self.inline_search_state.direction;
        self.inline_search(direction);
    }

    /// Jump to the next matching character in the line.
    fn inline_search_previous(&mut self) {
        let direction = self.inline_search_state.direction.opposite();
        self.inline_search(direction);
    }

    /// Process input during inline search.
    fn inline_search_input(&mut self, text: &str) {
        // Ignore input with empty text, like modifier keys.
        let c = match text.chars().next() {
            Some(c) => c,
            None => return,
        };

        self.inline_search_state.char_pending = false;
        self.inline_search_state.character = Some(c);
        self.window().set_ime_inhibitor(ImeInhibitor::VI, true);

        // Immediately move to the captured character.
        self.inline_search_next();
    }

    fn message(&self) -> Option<&Message> {
        self.message_buffer.message()
    }

    fn config(&self) -> &UiConfig {
        self.config
    }

    #[cfg(target_os = "macos")]
    fn event_loop(&self) -> &ActiveEventLoop {
        self.event_loop
    }

    fn clipboard_mut(&mut self) -> &mut Clipboard {
        self.clipboard
    }

    fn scheduler_mut(&mut self) -> &mut Scheduler {
        self.scheduler
    }
}

impl<'a, N: Notify + 'a, T: EventListener> ActionContext<'a, N, T> {
    fn update_search(&mut self) {
        let regex = match self.search_state.regex() {
            Some(regex) => regex,
            None => return,
        };

        // Hide cursor while typing into the search bar.
        if self.config.mouse.hide_when_typing {
            self.display.window.set_mouse_visible(false);
        }

        if regex.is_empty() {
            // Stop search if there's nothing to search for.
            self.search_reset_state();
            self.search_state.dfas = None;
        } else {
            // Create search dfas for the new regex string.
            self.search_state.dfas = RegexSearch::new(regex).ok();

            // Update search highlighting.
            self.goto_match(MAX_SEARCH_WHILE_TYPING);
        }

        *self.dirty = true;
    }

    /// Reset terminal to the state before search was started.
    fn search_reset_state(&mut self) {
        // Unschedule pending timers.
        let timer_id = TimerId::new(Topic::DelayedSearch, self.display.window.id());
        self.scheduler.unschedule(timer_id);

        // Clear focused match.
        self.search_state.focused_match = None;

        // The viewport reset logic is only needed for vi mode, since without it our origin is
        // always at the current display offset instead of at the vi cursor position which we need
        // to recover to.
        if !self.terminal.mode().contains(TermMode::VI) {
            return;
        }

        // Reset display offset and cursor position.
        self.terminal.vi_mode_cursor.point = self.search_state.origin;
        self.terminal.scroll_display(Scroll::Delta(self.search_state.display_offset_delta));
        self.search_state.display_offset_delta = 0;

        *self.dirty = true;
    }

    /// Jump to the first regex match from the search origin.
    pub(super) fn goto_match(&mut self, mut limit: Option<usize>) {
        let dfas = match &mut self.search_state.dfas {
            Some(dfas) => dfas,
            None => return,
        };

        // Limit search only when enough lines are available to run into the limit.
        limit = limit.filter(|&limit| limit <= self.terminal.total_lines());

        // Jump to the next match.
        let direction = self.search_state.direction;
        let clamped_origin = self.search_state.origin.grid_clamp(self.terminal, Boundary::Grid);
        match self.terminal.search_next(dfas, clamped_origin, direction, Side::Left, limit) {
            Some(regex_match) => {
                let old_offset = self.terminal.grid().display_offset() as i32;

                if self.terminal.mode().contains(TermMode::VI) {
                    // Move vi cursor to the start of the match.
                    self.terminal.vi_goto_point(*regex_match.start());
                } else {
                    // Select the match when vi mode is not active.
                    self.terminal.scroll_to_point(*regex_match.start());
                }

                // Update the focused match.
                self.search_state.focused_match = Some(regex_match);

                // Store number of lines the viewport had to be moved.
                let display_offset = self.terminal.grid().display_offset();
                self.search_state.display_offset_delta += old_offset - display_offset as i32;

                // Since we found a result, we require no delayed re-search.
                let timer_id = TimerId::new(Topic::DelayedSearch, self.display.window.id());
                self.scheduler.unschedule(timer_id);
            },
            // Reset viewport only when we know there is no match, to prevent unnecessary jumping.
            None if limit.is_none() => self.search_reset_state(),
            None => {
                // Schedule delayed search if we ran into our search limit.
                let timer_id = TimerId::new(Topic::DelayedSearch, self.display.window.id());
                if !self.scheduler.scheduled(timer_id) {
                    let event = Event::new(EventType::SearchNext, self.display.window.id());
                    self.scheduler.schedule(event, TYPING_SEARCH_DELAY, false, timer_id);
                }

                // Clear focused match.
                self.search_state.focused_match = None;
            },
        }

        *self.dirty = true;
    }

    /// Cleanup the search state.
    fn exit_search(&mut self) {
        let vi_mode = self.terminal.mode().contains(TermMode::VI);
        self.window().set_ime_inhibitor(ImeInhibitor::VI, vi_mode);

        self.display.damage_tracker.frame().mark_fully_damaged();
        self.display.pending_update.dirty = true;
        self.search_state.history_index = None;

        // Clear focused match.
        self.search_state.focused_match = None;
    }

    /// Update the cursor blinking state.
    /// 当前聚焦终端此刻是否应该闪烁光标——blink 定时与每个 tick 的自检共
    /// 用这一个判定,两处口径不可能分叉。
    pub(super) fn cursor_should_blink(&mut self) -> bool {
        // Push the settings default (shape + blink) into the terminal first:
        // `Term::cursor_style()` falls back to it whenever no DECSCUSR escape
        // has overridden the style, so vim's mode cursor keeps working while
        // plain shells follow the user's choice immediately.
        self.terminal.set_default_cursor_style(self.display.nebula_default_cursor_style());
        // Get config cursor style.
        let mut cursor_style = self.config.cursor.style;
        let vi_mode = self.terminal.mode().contains(TermMode::VI);
        if vi_mode {
            cursor_style = self.config.cursor.vi_mode_style.unwrap_or(cursor_style);
        }

        // Check terminal cursor style.
        let terminal_blinking = self.terminal.cursor_style().blinking;
        let mut blinking = cursor_style.blinking_override().unwrap_or(terminal_blinking);
        blinking &= (vi_mode || self.terminal().mode().contains(TermMode::SHOW_CURSOR))
            && self.display().ime.preedit().is_none();
        // 用 winit 的实时焦点而不是 `terminal.is_focused` 缓存:切 pane / 切
        // tab 后新 Term 的缓存可能从未见过 Focused 事件,残留 false 会让
        // 闪烁"有时不闪";残留 true 则让失焦窗口继续闪。
        blinking && self.display.window.has_focus()
    }

    pub(super) fn update_cursor_blinking(&mut self) {
        let blinking = self.cursor_should_blink();

        // Update cursor blinking state.
        let window_id = self.display.window.id();
        self.scheduler.unschedule(TimerId::new(Topic::BlinkCursor, window_id));
        self.scheduler.unschedule(TimerId::new(Topic::BlinkTimeout, window_id));

        // Reset blinking timeout.
        *self.cursor_blink_timed_out = false;

        if blinking {
            self.schedule_blinking();
            self.schedule_blinking_timeout();
        } else {
            self.display.cursor_hidden = false;
            *self.dirty = true;
        }
    }

    fn schedule_blinking(&mut self) {
        let window_id = self.display.window.id();
        let timer_id = TimerId::new(Topic::BlinkCursor, window_id);
        let event = Event::new(EventType::BlinkCursor, window_id);
        let blinking_interval = Duration::from_millis(self.config.cursor.blink_interval());
        self.scheduler.schedule(event, blinking_interval, true, timer_id);
    }

    fn schedule_blinking_timeout(&mut self) {
        let blinking_timeout = self.config.cursor.blink_timeout();
        if blinking_timeout == Duration::ZERO {
            return;
        }

        let window_id = self.display.window.id();
        let event = Event::new(EventType::BlinkCursorTimeout, window_id);
        let timer_id = TimerId::new(Topic::BlinkTimeout, window_id);

        self.scheduler.schedule(event, blinking_timeout, false, timer_id);
    }

    /// Perform vi mode inline search in the specified direction.
    fn inline_search(&mut self, direction: Direction) {
        let c = match self.inline_search_state.character {
            Some(c) => c,
            None => return,
        };
        let mut buf = [0; 4];
        let search_character = c.encode_utf8(&mut buf);

        // Find next match in this line.
        let vi_point = self.terminal.vi_mode_cursor.point;
        let point = match direction {
            Direction::Right => self.terminal.inline_search_right(vi_point, search_character),
            Direction::Left => self.terminal.inline_search_left(vi_point, search_character),
        };

        // Jump to point if there's a match.
        if let Ok(mut point) = point {
            if self.inline_search_state.stop_short {
                let grid = self.terminal.grid();
                point = match direction {
                    Direction::Right => {
                        grid.iter_from(point).prev().map_or(point, |cell| cell.point)
                    },
                    Direction::Left => {
                        grid.iter_from(point).next().map_or(point, |cell| cell.point)
                    },
                };
            }

            self.terminal.vi_goto_point(point);
            self.mark_dirty();
        }
    }
}
