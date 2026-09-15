//! Concrete `input::Processor` for the legacy shell: the top-level event
//! dispatch that routes winit events through input handling and terminal state.

use super::*;
use crate::input;

impl input::Processor<EventProxy, ActionContext<'_, Notifier, EventProxy>> {
    /// 助手错误恢复（spec 001 阶段一）触发点：便宜的门（冷却、开关、规则
    /// 表）都过了才抓输出、开线程。任何一门不过都安静返回——这条路径跑在
    /// 每次命令失败上，不能吵。
    fn maybe_request_ai_fix(&mut self, exit_code: i32, program: Option<&str>) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(1);

        if let Some(last) = self.ctx.nebula_state.ai_fix_cooldown {
            if last.elapsed() < crate::ai_assistant::COOLDOWN {
                return;
            }
        }
        let cfg = crate::ai_assistant::AssistantConfig::load();
        if !cfg.enabled {
            return;
        }
        let command = self.ctx.nebula_state.last_committed.clone();
        if !crate::ai_assistant::should_suggest(
            exit_code,
            &command,
            program,
            &cfg.ignored_exit_codes,
        ) {
            return;
        }
        self.ctx.nebula_state.ai_fix_cooldown = Some(Instant::now());
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let request = crate::ai_assistant::FixRequest {
            pane: self.ctx.pane_id,
            seq,
            command,
            exit_code,
            cwd: self.ctx.nebula_state.cwd.clone(),
            branch: self.ctx.nebula_state.branch.clone(),
            output_tail: crate::ai_assistant::redact_secrets(&self.grid_output_tail(24, 2000)),
        };
        self.ctx.nebula_state.ai_fix = Some(crate::ai_assistant::AiFixState::Pending { seq });
        crate::ai_assistant::spawn_fix_request(self.ctx.event_proxy.clone(), cfg, request);
        self.ctx.mark_dirty();
    }

    /// The failed command's on-screen output: up to `max_lines` rows ending at
    /// the cursor (OSC 133;D arrives before the next prompt paints, so the
    /// cursor still sits at the end of the output), tail-capped at `max_chars`
    /// — the newest lines carry the actual error. Shell-side integrations cannot
    /// see this rendered context, so the terminal grid remains the authoritative source.
    fn grid_output_tail(&self, max_lines: usize, max_chars: usize) -> String {
        use nebula_terminal::index::{Column, Line};
        use nebula_terminal::term::cell::Flags as CellFlags;

        let grid = self.ctx.terminal.grid();
        let cursor_line = grid.cursor.point.line.0;
        let columns = self.ctx.terminal.columns();
        let first = (cursor_line + 1 - max_lines as i32).max(0);
        let mut lines: Vec<String> = Vec::new();
        for l in first..=cursor_line {
            let row = &grid[Line(l)];
            let mut text = String::with_capacity(columns);
            for c in 0..columns {
                let cell = &row[Column(c)];
                // Wide chars own two cells; the spacer half would double every
                // CJK glyph as a stray space.
                if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                    continue;
                }
                text.push(cell.c);
            }
            lines.push(text.trim_end().to_owned());
        }
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        while lines.first().is_some_and(String::is_empty) {
            lines.remove(0);
        }
        let text = lines.join("\n");
        let overflow = text.chars().count().saturating_sub(max_chars);
        if overflow > 0 { text.chars().skip(overflow).collect() } else { text }
    }

    /// Handle events from winit.
    pub fn handle_event(&mut self, event: WinitEvent<Event>) {
        match event {
            WinitEvent::UserEvent(Event { payload, tab_id, .. }) => match payload {
                EventType::SearchNext => self.ctx.goto_match(None),
                // Tab requests are handled at the window-context level.
                EventType::NebulaTab(_) => (),
                // Clock ticks are handled at the window-context level.
                EventType::NebulaTick | EventType::NebulaAttach | EventType::RuntimeControl(_) => {
                    ()
                },
                // Resize settling is handled at the window-context level.
                EventType::NebulaResizeSettled
                | EventType::SshDeleteUndoExpired
                | EventType::QuickTerminalHotkeyChanged { .. }
                | EventType::ProxyTestDone { .. }
                | EventType::ProviderTestDone { .. }
                | EventType::SshTestDone { .. }
                | EventType::SshConnect(_)
                | EventType::SftpUpdated => (),
                // AI hook events are handled at the Processor level (they may
                // target any window's pane); FocusWindow, fix results and
                // WebDAV sync likewise.
                EventType::AiHook(_)
                | EventType::AiFixReady { .. }
                | EventType::NebulaSync { .. }
                | EventType::NebulaSyncDone { .. }
                | EventType::NebulaBackupRemote { .. }
                | EventType::NebulaBackupRemoteDone { .. }
                | EventType::LocalProxyScan
                | EventType::LocalProxyScanDone(_)
                | EventType::FocusWindow { .. } => (),
                EventType::Scroll(scroll) => self.ctx.scroll(scroll),
                EventType::BlinkCursor => {
                    // 切 tab / 切 pane 后 timer 可能还按旧终端的口味在跑；
                    // 每个 tick 都对照当前聚焦终端自检，不该闪就地停表并把
                    // 光标恢复常亮——"残留闪烁"没有活过一个周期的机会。
                    if !self.ctx.cursor_should_blink() {
                        self.ctx.update_cursor_blinking();
                    } else if !*self.ctx.cursor_blink_timed_out {
                        // Only change state when timeout isn't reached, since we could get
                        // BlinkCursor and BlinkCursorTimeout events at the same time.
                        self.ctx.display.cursor_hidden ^= true;
                        *self.ctx.dirty = true;
                    }
                },
                EventType::BlinkCursorTimeout => {
                    // Disable blinking after timeout reached.
                    let timer_id = TimerId::new(Topic::BlinkCursor, self.ctx.display.window.id());
                    self.ctx.scheduler.unschedule(timer_id);
                    *self.ctx.cursor_blink_timed_out = true;
                    self.ctx.display.cursor_hidden = false;
                    *self.ctx.dirty = true;
                },
                // Add message only if it's not already queued.
                EventType::Message(message) if !self.ctx.message_buffer.is_queued(&message) => {
                    self.ctx.message_buffer.push(message);
                    self.ctx.display.pending_update.dirty = true;
                },
                EventType::Terminal(event) => match event {
                    // OSC 9;4：程序自报任务进度。旧壳一个窗口只投一次，不像
                    // GPUI 壳那样先判「这个 pane 是不是正被看着」——旧壳的多
                    // pane 场景里最后一次更新赢，读数不完美但不会互相打断。
                    TerminalEvent::Progress { state, value } => {
                        let progress = crate::taskbar::TaskProgress::from_osc(state, value);
                        if let Some(hwnd) = self.ctx.display.window.native_window_handle_id() {
                            crate::taskbar::apply(hwnd as isize, progress);
                        }
                    },
                    TerminalEvent::Title(title) => {
                        // Nebula encodes cwd/branch in a `NEBULA|cwd|branch` title
                        // for the glass powerline instead of the window title. A
                        // remote `nebula ssh` shell appends a 4th `program` field
                        // (`NEBULA|cwd|branch|program`): the local screen-scrape
                        // that normally feeds `running_program` can't see through
                        // the SSH pipe, so the remote reports the program identity
                        // here instead — empty at the prompt, the command name
                        // while one runs. A local shell sends only 3 fields, so
                        // the 4th is absent and `running_program` is left to the
                        // existing OSC-133;C/last_committed path untouched.
                        if let Some(rest) = title.strip_prefix("NEBULA|") {
                            let mut parts = rest.splitn(3, '|');
                            let cwd = parts.next().unwrap_or("").to_owned();
                            if self.ctx.nebula_state.cwd != cwd {
                                self.ctx.nebula_state.cwd.clone_from(&cwd);
                                self.ctx.display.nebula_record_directory(&cwd);
                            }
                            self.ctx.nebula_state.branch = parts.next().unwrap_or("").to_owned();
                            if let Some(program) = parts.next() {
                                self.ctx.nebula_state.running_program = if program.is_empty() {
                                    None
                                } else {
                                    Some(program.to_owned())
                                };
                                // A 4-field title only ever comes from the
                                // remote `nebula ssh` integration, so the
                                // typed ssh login is confirmed connected:
                                // save its destination to the sidebar now
                                // instead of waiting out SAVE_MIN_SESSION.
                                if let Some(host) = self.ctx.nebula_state.pending_ssh_host.take() {
                                    self.ctx.display.nebula_save_ssh_host(&host);
                                }
                            }
                            *self.ctx.dirty = true;
                        } else {
                            // A non-NEBULA title while a command is in flight
                            // can only come from the program on the PTY — the
                            // local shell integration only retitles at its
                            // prompt. For a typed `ssh` login that means the
                            // remote shell is up (Ubuntu-style PS1 retitles on
                            // login): confirm and save the destination without
                            // waiting for the session to end.
                            if self.ctx.nebula_state.command_started.is_some() {
                                if let Some(host) = self.ctx.nebula_state.pending_ssh_host.take() {
                                    self.ctx.display.nebula_save_ssh_host(&host);
                                    *self.ctx.dirty = true;
                                }
                            }
                            if !self.ctx.preserve_title && self.ctx.config.window.dynamic_title {
                                self.ctx.window().set_title(title);
                            }
                        }
                    },
                    TerminalEvent::ResetTitle => {
                        let window_config = &self.ctx.config.window;
                        if !self.ctx.preserve_title && window_config.dynamic_title {
                            self.ctx.display.window.set_title(window_config.identity.title.clone());
                        }
                    },
                    TerminalEvent::CwdReport(cwd) => {
                        // Standard OSC 7 / 9;9 directory report. Update cwd only,
                        // leaving any branch captured from a `NEBULA|cwd|branch`
                        // title intact, so the two channels coexist.
                        if self.ctx.nebula_state.cwd != cwd {
                            self.ctx.nebula_state.cwd.clone_from(&cwd);
                            self.ctx.display.nebula_record_directory(&cwd);
                            *self.ctx.dirty = true;
                        }
                    },
                    TerminalEvent::InlineImage { data, abs_line, width, height } => {
                        // Decode off the PTY thread (here, on the UI loop) and
                        // anchor the pixels to the pane. Textures upload lazily
                        // on first draw.
                        match crate::renderer::image::decode_png_bytes(&data) {
                            Ok((px_w, px_h, rgba)) => {
                                use std::sync::atomic::{AtomicU64, Ordering};
                                static NEXT_INLINE_IMAGE_ID: AtomicU64 = AtomicU64::new(1);
                                let id = NEXT_INLINE_IMAGE_ID.fetch_add(1, Ordering::Relaxed);
                                let images = &mut self.ctx.nebula_state.inline_images;
                                images.push(crate::display::NebulaInlineImage {
                                    id,
                                    abs_line,
                                    width,
                                    height,
                                    rgba: std::sync::Arc::new(rgba),
                                    px_w,
                                    px_h,
                                });
                                // VRAM/heap guard against imgcat runaway loops.
                                if images.len() > 16 {
                                    images.remove(0);
                                }
                                *self.ctx.dirty = true;
                            },
                            Err(err) => {
                                warn!("inline image decode failed: {err}");
                            },
                        }
                    },
                    TerminalEvent::CommandStart => {
                        // 首个词就是交互式 shell（`cmd`、`wsl`、裸 `bash`）时不
                        // 算「命令在跑」：那个 shell 接管终端后 133;D 永远不会
                        // 来，`command_started` 会一直留着，侧栏就一直转圈。
                        //
                        // GPUI 壳另有进程树复核（`runtime::reconcile_shell_activity`
                        // 会双向纠正），旧壳只做这一次启动判定——判据函数是同一个。
                        if !crate::process_tree::is_interactive_shell_command(
                            &self.ctx.nebula_state.last_committed,
                        ) {
                            self.ctx.nebula_state.command_started = Some(Instant::now());
                        }
                        // Program identity for the sidebar tab icon, from the
                        // line captured at Enter (buffers are cleared by now).
                        self.ctx.nebula_state.running_program =
                            crate::ai_agents::AgentKind::parse_command(
                                &self.ctx.nebula_state.last_committed,
                            )
                            .map(|agent| agent.slug().to_owned())
                            .or_else(|| {
                                crate::display::extract_program(
                                    &self.ctx.nebula_state.last_committed,
                                )
                            });
                        self.ctx.nebula_state.agent_hook_seen = false;
                        self.ctx.nebula_state.agent_status_rule = None;
                        self.ctx.nebula_state.agent_status_source =
                            crate::ai_agents::AgentStatusSource::Process;
                        self.ctx.nebula_state.agent_status = if self
                            .ctx
                            .nebula_state
                            .running_program
                            .as_deref()
                            .and_then(crate::ai_agents::AgentKind::parse)
                            .is_some()
                        {
                            crate::ai_agents::AgentStatus::Working
                        } else {
                            crate::ai_agents::AgentStatus::Unknown
                        };
                        // Arm the ssh host auto-save: when this command is an
                        // interactive ssh login, hold its destination until a
                        // remote NEBULA| title or a long-enough session
                        // (CommandDone) confirms the connection was real.
                        self.ctx.nebula_state.pending_ssh_host =
                            crate::ssh::ssh_destination(&self.ctx.nebula_state.last_committed);
                        self.ctx.nebula_state.awaiting_input = false;
                        if let Some(run) = &mut self.ctx.nebula_state.active_run
                            && run.phase == crate::runtime_api::RuntimeRunPhase::Submitted
                        {
                            run.phase = crate::runtime_api::RuntimeRunPhase::Started;
                        }
                    },
                    TerminalEvent::CommandDone { exit_code } => {
                        // 新 PTY 初始化提示符也可能先发一个 CommandDone。Runtime
                        // 提交 barrier 尚未冲刷时，它不能结束当前请求。
                        if self.ctx.nebula_state.runtime_submit_barrier.is_some() {
                            return;
                        }
                        // Take (not just clear) the program: the toast below
                        // names it, and reading the field after the reset used
                        // to hand the toast a permanent `None`.
                        let program = self.ctx.nebula_state.running_program.take();
                        // CLI 退回提示符，对话不再是这个 pane 的前台事实；
                        // 留着它，快照会把一个已经退出的会话当活的接续。
                        self.ctx.nebula_state.ai_session = None;
                        self.ctx.nebula_state.agent_hook_seen = false;
                        self.ctx.nebula_state.agent_status = crate::ai_agents::AgentStatus::Unknown;
                        self.ctx.nebula_state.agent_status_source =
                            crate::ai_agents::AgentStatusSource::Unknown;
                        self.ctx.nebula_state.agent_status_rule = None;
                        self.ctx.nebula_state.pending_command_prompt = None;
                        self.ctx.nebula_state.agent_runtime_submit_pending = false;
                        self.ctx.nebula_state.runtime_submit_barrier = None;
                        self.ctx.nebula_state.idle_screen_streak = 0;
                        let pending_ssh = self.ctx.nebula_state.pending_ssh_host.take();
                        self.ctx.nebula_state.awaiting_input = false;
                        if let Some(run) = self.ctx.nebula_state.active_run.take() {
                            self.ctx.nebula_state.last_run = Some(
                                crate::runtime_api::RuntimeRunOutcome::command_done(run, exit_code),
                            );
                        }
                        // 助手错误恢复（spec 001）：Nebula 集成上报的退出码
                        // 走触发判定；裸 133;D（第三方集成）码为 None，静默。
                        if let Some(code) = exit_code {
                            self.maybe_request_ai_fix(code, program.as_deref());
                        }
                        // Long commands (npm/cargo builds...) notify when the
                        // window is in the background; quick ones stay silent.
                        if let Some(started) = self.ctx.nebula_state.command_started.take() {
                            let duration = started.elapsed();
                            // An ssh session that lived this long was a real
                            // connection even without the remote integration's
                            // NEBULA| title: save the host to the sidebar.
                            if duration >= crate::ssh::SAVE_MIN_SESSION {
                                if let Some(host) = pending_ssh {
                                    self.ctx.display.nebula_save_ssh_host(&host);
                                }
                            }
                            if duration >= crate::notify::COMMAND_NOTIFY_MIN {
                                // Sidebar dot until the tab gets looked at
                                // (cleared instantly for the visible tab).
                                self.ctx.nebula_state.finished_unseen = true;
                                // 成败分流：非零码走警示三角，零码走"刚完成"
                                // 的对勾闪现。裸 133;D 没带码（第三方集成），
                                // 那种情况只当作完成，不敢报错。
                                match exit_code {
                                    Some(code) if code != 0 => {
                                        self.ctx.nebula_state.failed_unseen = true;
                                    },
                                    _ => {
                                        self.ctx.nebula_state.finished_at =
                                            Some(std::time::Instant::now());
                                    },
                                }
                                if !self.ctx.display.window.has_focus() {
                                    crate::notify::deliver(
                                        &self.ctx.display.window,
                                        &crate::notify::Notification::CommandDone {
                                            duration,
                                            program,
                                        },
                                        tab_id,
                                    );
                                }
                            }
                        }
                    },
                    TerminalEvent::UserVar { name, value } => {
                        // `nebula_ai_query`（`#` 自然语言转命令）是阶段二的
                        // 消费者；通道先贯通，其余变量目前无人认领。
                        if name == "nebula_ai_query" {
                            info!(
                                "assistant: query channel received ({} chars)",
                                value.chars().count()
                            );
                        }
                    },
                    TerminalEvent::Notify(body) => {
                        // Program-initiated (OSC 9) notifications only matter
                        // when the user isn't already looking at the pane.
                        if !self.ctx.display.window.has_focus() {
                            crate::notify::deliver(
                                &self.ctx.display.window,
                                &crate::notify::Notification::Text {
                                    body,
                                    program: self.ctx.nebula_state.running_program.clone(),
                                },
                                tab_id,
                            );
                        }
                    },
                    TerminalEvent::AiHookEnvelope(_) => (),
                    TerminalEvent::Bell => {
                        // Claude Code / Codex ring BEL when a turn finishes, so
                        // an unfocused bell is the primary "AI task done"
                        // signal: always request attention + sound, without
                        // gating on the (rarely set) URGENCY_HINTS mode.
                        //
                        // CRITICAL: Query the window's CURRENT focus state directly
                        // via winit, not the cached terminal.is_focused flag. The
                        // cached flag is updated by WindowEvent::Focused, which may
                        // arrive AFTER the BEL if the user switches windows quickly.
                        if !self.ctx.display.window.has_focus() {
                            crate::notify::deliver(
                                &self.ctx.display.window,
                                &crate::notify::Notification::Bell {
                                    program: self.ctx.nebula_state.running_program.clone(),
                                },
                                tab_id,
                            );
                        }

                        // A bell from a tracked program (claude finishing a
                        // turn) means it now waits for input: pause the
                        // sidebar spinner until the user types again.
                        if self.ctx.nebula_state.running_program.is_some() {
                            self.ctx.nebula_state.awaiting_input = true;
                        }

                        // Ring visual bell.
                        self.ctx.display.visual_bell.ring();

                        // Audible bell. AI CLIs ring BEL when a turn ends or
                        // they need input, so this is the "needs you" sound
                        // even with Nebula focused on another tab — the toast
                        // above only fires when the window is unfocused, and
                        // the visual bell is invisible from another tab.
                        // Plays regardless of focus; `platform::beep` throttles
                        // so a looping BEL cannot machine-gun it.
                        if self.ctx.config.bell.audible {
                            crate::platform::beep();
                        }

                        // Execute bell command.
                        if let Some(bell_command) = &self.ctx.config.bell.command {
                            if self
                                .ctx
                                .prev_bell_cmd
                                .is_none_or(|i| i.elapsed() >= BELL_CMD_COOLDOWN)
                            {
                                self.ctx.spawn_daemon(bell_command.program(), bell_command.args());

                                *self.ctx.prev_bell_cmd = Some(Instant::now());
                            }
                        }
                    },
                    TerminalEvent::ClipboardStore(clipboard_type, content) => {
                        if self.ctx.terminal.is_focused {
                            self.ctx.clipboard.store(clipboard_type, content);
                        }
                    },
                    TerminalEvent::ClipboardLoad(clipboard_type, format) => {
                        if self.ctx.terminal.is_focused {
                            let text = format(self.ctx.clipboard.load(clipboard_type).as_str());
                            self.ctx.write_to_pty(text.into_bytes());
                        }
                    },
                    TerminalEvent::ColorRequest(index, format) => {
                        if crate::display::replays_untrusted_terminal_output(
                            &self.ctx.nebula_state.last_committed,
                        ) {
                            return;
                        }
                        let color = match self.ctx.terminal().colors()[index] {
                            Some(color) => Rgb(color),
                            // Ignore cursor color requests unless it was changed.
                            None if index == NamedColor::Cursor as usize => return,
                            None => self.ctx.display.colors[index],
                        };
                        self.ctx.write_to_pty(format(color.0).into_bytes());
                    },
                    TerminalEvent::TextAreaSizeRequest(format) => {
                        let text = format(self.ctx.size_info().into());
                        self.ctx.write_to_pty(text.into_bytes());
                    },
                    TerminalEvent::PtyWrite(text) => self.ctx.write_to_pty(text.into_bytes()),
                    TerminalEvent::MouseCursorDirty => self.reset_mouse_cursor(),
                    TerminalEvent::CursorBlinkingChange => self.ctx.update_cursor_blinking(),
                    TerminalEvent::PtyFailure(reason) => {
                        // 宿主/管道异常死(shell 未退出)。三层裁定:用户有待办
                        // 动作(重开会话)→ 消息栏;toast 不承载唯一副本,必落 log。
                        // 随后到来的 `Exit` 走既有的 tab 关闭路径。
                        crate::display::nebula_debug_log(format!("pty failure: {reason}"));
                        self.ctx.message_buffer.push(Message::new(
                            format!("终端会话异常终止(宿主或管道故障):{reason}"),
                            MessageType::Error,
                        ));
                        self.ctx.display.pending_update.dirty = true;
                    },
                    TerminalEvent::Exit | TerminalEvent::ChildExit(_) | TerminalEvent::Wakeup => (),
                },
                #[cfg(unix)]
                EventType::IpcConfig(_) | EventType::IpcGetConfig(..) | EventType::Shutdown => (),
                EventType::Message(_)
                | EventType::ConfigReload(_)
                | EventType::ConfigReloadReady
                | EventType::TerminalProfilesChanged
                | EventType::CreateWindow(_)
                | EventType::Frame => (),
            },
            WinitEvent::WindowEvent { event, .. } => {
                match event {
                    WindowEvent::CloseRequested => {
                        // User asked to close the window, so no need to hold it.
                        // This is a window-level action: close every tab/pane at once,
                        // not only the currently focused PTY.
                        self.ctx.window().hold = false;
                        self.ctx.nebula_tab(TabRequest::CloseWindow);
                    },
                    WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                        if self.ctx.window().native_live_move() {
                            // During a mixed-DPI drag, Windows can emit several
                            // transient factors before the window settles. Keep
                            // only the newest one and defer glyph/UI work.
                            crate::display::nebula_debug_log(format!(
                                "winmove scale_factor_changed {scale_factor} deferred"
                            ));
                            self.ctx.window().defer_scale_factor(scale_factor);
                        } else {
                            let start = std::time::Instant::now();
                            self.ctx
                                .display
                                .apply_scale_factor_change(scale_factor, self.ctx.config);
                            crate::display::nebula_debug_log(format!(
                                "winmove scale_factor_changed {scale_factor} applied in {:?}",
                                start.elapsed()
                            ));
                        }
                    },
                    WindowEvent::Resized(size) => {
                        // Ignore unreasonably small resizes. A borderless window on
                        // Windows reports a tiny size (~237x39) when minimized instead
                        // of 0x0; honoring it would collapse the terminal grid to a
                        // single row and lose the visible content on restore.
                        if size.width < 100 || size.height < 100 {
                            return;
                        }

                        let defer_native_resize = {
                            let window = self.ctx.window();
                            window.native_live_move() && window.has_pending_scale_factor()
                        };
                        crate::display::nebula_debug_log(format!(
                            "winmove resized {}x{} defer={defer_native_resize}",
                            size.width, size.height
                        ));
                        if defer_native_resize {
                            // A DPI transition is followed by a synthetic
                            // resize on Windows. Keep its physical size until
                            // the native move exits, while ordinary edge
                            // resizing remains fully live.
                            self.ctx.window().defer_inner_size(size);
                            return;
                        }

                        if self.ctx.display.window.allows_drag_resize() {
                            *self.ctx.windowed_size =
                                size.to_logical(self.ctx.display.window.scale_factor);
                        }

                        self.ctx.display.pending_update.set_dimensions(size);
                    },
                    WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                        // mouse-hide-while-typing: hide the cursor on any key
                        // press; any mouse movement/click/wheel below shows it
                        // again. Hide the pointer while typing.
                        if self.ctx.config.mouse.hide_when_typing
                            && event.state == ElementState::Pressed
                        {
                            self.ctx.window().set_mouse_visible(false);
                        }
                        self.key_input(event);
                    },
                    WindowEvent::ModifiersChanged(modifiers) => self.modifiers_input(modifiers),
                    WindowEvent::MouseInput { state, button, .. } => {
                        self.ctx.window().set_mouse_visible(true);
                        self.mouse_input(state, button);
                    },
                    WindowEvent::CursorMoved { position, .. } => {
                        self.ctx.window().set_mouse_visible(true);
                        self.mouse_moved(position);
                    },
                    WindowEvent::MouseWheel { delta, phase, .. } => {
                        self.ctx.window().set_mouse_visible(true);
                        self.mouse_wheel_input(delta, phase);
                    },
                    WindowEvent::Touch(touch) => self.touch(touch),
                    WindowEvent::Focused(is_focused) => {
                        log::info!("WindowEvent::Focused({})", is_focused);
                        self.ctx.terminal.is_focused = is_focused;
                        // 焦点切换会让输入法宿主重置窗口状态；IME 位置缓存
                        // 必须跟着作废，否则回焦后第一次组合可能拿到陈旧的
                        // 候选窗位置（见 window.rs push_ime_cursor_area）。
                        self.ctx.display.window.reset_ime_cursor_area_cache();

                        // Losing window focus ends any chrome text editing —
                        // a rename box left open under another window reads
                        // as a hang (its caret froze), and stray keystrokes
                        // later would edit a name the user forgot about.
                        if !is_focused {
                            if self.ctx.display.nebula_tab_rename.take().is_some() {
                                self.ctx.display.nebula_tab_rename_select_all = false;
                            }
                            let panel = &mut self.ctx.display.nebula_side_panel;
                            panel.search_unfocus(false);
                            panel.commit_unfocus();
                            if let Some(panel) = self.ctx.display.nebula_sftp_panel.as_mut() {
                                panel.editor_unfocus();
                            }
                        }

                        // Nebula: always redraw on focus change, and clear the
                        // occluded flag when refocused. On Windows `Occluded(false)`
                        // is unreliable, so without this the draw path stays gated
                        // off and terminal content vanishes after backgrounding.
                        *self.ctx.dirty = true;
                        if is_focused {
                            *self.ctx.occluded = false;
                            // Bypass frame throttling and force an immediate
                            // repaint; otherwise content stays blank after the
                            // window returns from the background on Windows.
                            self.ctx.window().has_frame = true;
                            self.ctx.window().request_redraw();
                            self.ctx.window().set_urgent(false);
                        }

                        self.ctx.update_cursor_blinking();
                        self.on_focus_change(is_focused);

                        // Ensure IME is disabled while unfocused.
                        self.ctx.window().set_ime_inhibitor(ImeInhibitor::FOCUS, !is_focused);
                    },
                    WindowEvent::Occluded(occluded) => {
                        // Windows 的遮挡事件不可靠：启动早期 / DWM 合成切换
                        // 会误发 `Occluded(true)`，而配对的 `false` 可能永远
                        // 不来。标志被误置后整条 draw 路径熄火——窗口"点什
                        // 么都没反应"，直到最小化再复原靠 Focused(true) 的
                        // 补丁解锁（issue #21）。窗口明明没最小化就发来的
                        // true 一律不信；false 永远接受。
                        let minimized = self.ctx.display.window.is_minimized().unwrap_or(false);
                        if !occluded || minimized {
                            *self.ctx.occluded = occluded;
                        }

                        // Force a full redraw when the window becomes visible again.
                        if !occluded {
                            *self.ctx.dirty = true;
                        }
                    },
                    WindowEvent::DroppedFile(path) => {
                        let over_sftp = if self.ctx.display().nebula_sftp_panel.is_some() {
                            let (x, y, width, height) =
                                self.ctx.display().side_panel_layout().panel;
                            let px = self.ctx.mouse.x as f32;
                            let py = self.ctx.mouse.y as f32;
                            px >= x && px < x + width && py >= y && py < y + height
                        } else {
                            false
                        };
                        if over_sftp {
                            self.ctx.display().sftp_upload_dropped_paths(vec![path]);
                        } else {
                            let path: String = path.to_string_lossy().into();
                            self.ctx.paste(&(path + " "), true);
                        }
                    },
                    WindowEvent::CursorLeft { .. } => {
                        self.ctx.mouse.inside_text_area = false;
                        self.ctx.display().set_chrome_hover(
                            crate::display::ChromeHit::None,
                            crate::display::SettingsHit::None,
                        );

                        if self.ctx.display().highlighted_hint.is_some() {
                            *self.ctx.dirty = true;
                        }
                    },
                    WindowEvent::Ime(ime) => {
                        match ime {
                            Ime::Commit(text) => {
                                *self.ctx.dirty = true;
                                // 设置页的自绘输入框也必须在 IME 提交阶段消费文字。
                                // Windows 中文输入法不会经过 `KeyboardInput` 的字符分支；
                                // 若这里漏掉某个字段，拼音确认后就会穿透到终端。
                                if self.ctx.display().settings_open()
                                    && self.ctx.display().nebula_settings_dropdown
                                        == Some(crate::display::SettingsDropdown::Font)
                                {
                                    self.ctx.display().font_query_edit(Some(&text));
                                } else if self.ctx.display().settings_open()
                                    && self.ctx.display().keymap_search_active()
                                {
                                    self.ctx.display().keymap_search_edit(&text);
                                } else if self.ctx.display().settings_open()
                                    && self.ctx.display().nebula_ssh_proxy_focus.is_some()
                                {
                                    self.ctx.display().ssh_proxy_field_paste(&text);
                                } else if self.ctx.display().settings_open()
                                    && self.ctx.display().nebula_sync_focus.is_some()
                                {
                                    self.ctx.display().sync_field_paste(&text);
                                } else if self.ctx.display().nebula_tab_rename.is_some() {
                                    // Tab rename owns committed text while editing: on
                                    // Windows (and any IME), printable characters are
                                    // delivered here, NOT through key_input — so the
                                    // rename buffer must consume them here or typing
                                    // silently pastes into the shell behind the box.
                                    // Caret-aware insert (type-to-overwrite on a
                                    // pending select-all) — same code path as the
                                    // non-IME keyboard fallback.
                                    self.ctx.display.tab_rename_insert(&text);
                                } else if self.ctx.display.nebula_sftp_panel.as_ref().is_some_and(
                                    crate::display::sftp_panel::SftpPanel::editor_active,
                                ) {
                                    if let Some(panel) = self.ctx.display.nebula_sftp_panel.as_mut()
                                    {
                                        panel.editor_insert(&text);
                                    }
                                } else if self.ctx.display.nebula_side_panel.search_focus {
                                    // Side-panel filter box: same IME contract as
                                    // tab rename — committed text must land in the
                                    // box, not paste into the shell behind it.
                                    self.ctx.display.nebula_side_panel.search_input(&text);
                                } else if self.ctx.display.nebula_side_panel.commit_focus {
                                    self.ctx.display.nebula_side_panel.commit_input(&text);
                                } else if self.ctx.display.nebula_ssh_editor.is_some() {
                                    self.ctx.display.ssh_editor_insert(&text);
                                } else if self.ctx.display.command_palette_open() {
                                    self.ctx.display.palette_input_text(&text);
                                } else {
                                    // Don't use bracketed paste for single char input.
                                    self.ctx.paste(&text, text.chars().count() > 1);
                                }
                                self.ctx.display().update_settings_ime_cursor();
                                self.ctx.update_cursor_blinking();
                            },
                            Ime::Preedit(text, cursor_offset) => {
                                let preedit =
                                    (!text.is_empty()).then(|| Preedit::new(text, cursor_offset));

                                if self.ctx.display.ime.preedit() != preedit.as_ref() {
                                    self.ctx.display.ime.set_preedit(preedit);
                                    self.ctx.display.update_settings_ime_cursor();
                                    self.ctx.update_cursor_blinking();
                                    *self.ctx.dirty = true;
                                }
                            },
                            Ime::Enabled => {
                                self.ctx.display.ime.set_enabled(true);
                                // 输入法启用/切换：位置状态从零开始，下一帧
                                // 必须重推（值相同也要推）。
                                self.ctx.display.window.reset_ime_cursor_area_cache();
                                self.ctx.display.update_settings_ime_cursor();
                                *self.ctx.dirty = true;
                            },
                            Ime::Disabled => {
                                self.ctx.display.ime.set_enabled(false);
                                *self.ctx.dirty = true;
                            },
                        }
                    },
                    WindowEvent::ThemeChanged(theme) => {
                        self.ctx.display.system_theme_changed(theme);
                        *self.ctx.dirty = true;
                    },
                    WindowEvent::KeyboardInput { is_synthetic: true, .. }
                    | WindowEvent::ActivationTokenDone { .. }
                    | WindowEvent::DoubleTapGesture { .. }
                    | WindowEvent::TouchpadPressure { .. }
                    | WindowEvent::RotationGesture { .. }
                    | WindowEvent::CursorEntered { .. }
                    | WindowEvent::PinchGesture { .. }
                    | WindowEvent::AxisMotion { .. }
                    | WindowEvent::PanGesture { .. }
                    | WindowEvent::HoveredFileCancelled
                    | WindowEvent::Destroyed
                    | WindowEvent::HoveredFile(_)
                    | WindowEvent::RedrawRequested
                    | WindowEvent::Moved(_) => (),
                }
            },
            WinitEvent::Suspended
            | WinitEvent::NewEvents { .. }
            | WinitEvent::DeviceEvent { .. }
            | WinitEvent::LoopExiting
            | WinitEvent::Resumed
            | WinitEvent::MemoryWarning
            | WinitEvent::AboutToWait => (),
        }
    }
}
