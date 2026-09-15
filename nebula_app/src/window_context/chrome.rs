//! Chrome drawing: frame rendering, tab label generation, and tab bar sync.

use super::*;

impl WindowContext {
    /// Draw the window.
    pub fn draw(&mut self, scheduler: &mut Scheduler) {
        self.display.window.requested_redraw = false;
        self.sync_chrome_tabs();
        // The drawer follows the focused pane: its VIEW routes to SFTP only
        // while an SSH pane with the matching destination is focused, and the
        // directory tree follows the focused pane's cwd (throttled inside).
        let focused_ssh =
            self.pane(self.focused_pane_id()).and_then(|pane| pane.ssh_destination.clone());
        self.display.route_side_panel(focused_ssh.as_deref());
        let panel_cwd = self.focused_cwd().or_else(|| self.focused_wsl_cwd());
        // 命令面板的「工作目录」组也认这个值（WSL 路径已映射成 `\\wsl$\…`，
        // 复制出去和丢给资源管理器都能用）。抽屉是节流的，这里不能顺手复用
        // 它的内部状态——面板要的是**当前**目录，不是抽屉上次同步到的那个。
        self.display.nebula_focused_cwd = panel_cwd.clone();
        self.display.side_panel_sync(panel_cwd);

        // Chrome clock: unfocused/idle windows keep only the 1 Hz state watchdog;
        // visible animations use 12.5 fps. Re-arm whenever the cadence class changes.
        let clock_timer = TimerId::new(Topic::NebulaClock, self.display.window.id());
        let interval = chrome_clock_interval(
            self.display.window.has_focus(),
            self.display.any_tab_running()
                || self.display.ssh_test_running()
                || self.display.any_tab_flashing(),
            self.display.chrome_editor_active(),
            self.display.chrome_animating(),
        );
        if self.clock_interval != interval {
            scheduler.unschedule(clock_timer);
            self.clock_interval = interval;
        }
        if !scheduler.scheduled(clock_timer) {
            let event = Event::new(EventType::NebulaTick, self.display.window.id());
            scheduler.schedule(event, interval, true, clock_timer);
        }

        if self.occluded {
            return;
        }
        self.dirty = false;

        // Force the display to process any pending display update.
        self.display.process_renderer_update();

        // Request immediate re-draw if visual bell animation is not finished yet.
        if !self.display.visual_bell.completed() {
            // We can get an OS redraw which bypasses nebula's frame throttling, thus
            // marking the window as dirty when we don't have frame yet.
            if self.display.window.has_frame {
                self.display.window.request_redraw();
            } else {
                self.dirty = true;
            }
        }

        // Chrome sidebar/drawer transitions need display-rate frames until settled.
        if self.display.chrome_animating() {
            if self.display.window.has_frame {
                self.display.window.request_redraw();
            } else {
                self.dirty = true;
            }
        }

        // Redraw the window: walk the active tab's layout tree and draw each
        // pane in its rectangle. A single-pane tab uses the simple full-window
        // path; multi-pane tabs draw every leaf then overlay dividers + dimming.
        let pane_rects = self.layout_geometry(false).0;
        let divider_rects = self.layout_geometry(true).1;
        let focused = self.focused_pane_id();
        // 助手建议条（spec 001）跟随焦点 pane：绘制层只认 Display 自己的
        // 快照字段（SSH 撤销条同款模式），此处每帧同步一次。
        self.display.nebula_ai_fix_bar =
            self.pane_index(focused).and_then(|idx| self.panes[idx].nebula_state.ai_fix.clone());

        // Settings is rendered inside the normal tab content card; it is not
        // a modal and therefore keeps the tab/sidebar chrome fully usable.
        if self.tabs.get(self.active_tab).is_some_and(|tab| tab.settings) {
            self.display.begin_pane_frame(&self.config);
            self.display.draw_settings_frame(scheduler);
            return;
        }

        // Document-viewer tab: no pane, no grid. Draw the doc into the tab's
        // content rect; `present_frame` inside lays the normal chrome on top.
        if let Some(image) = self.tabs.get(self.active_tab).and_then(|tab| tab.image.clone()) {
            let view = pane_rects.first().map(|(_, view)| *view).unwrap_or(self.display.size_info);
            self.display.begin_pane_frame(&self.config);
            self.display.draw_image_frame(&image, view, scheduler);
            return;
        }

        if let Some(doc) = self.tabs.get_mut(self.active_tab).and_then(|tab| tab.doc.as_mut()) {
            let view = pane_rects.first().map(|(_, view)| *view).unwrap_or(self.display.size_info);
            self.display.begin_pane_frame(&self.config);
            self.display.draw_doc_frame(doc, view, scheduler);
            return;
        }

        // 连接卡片只画在聚焦 pane 里，display 侧只有几何、没有身份。
        self.display.set_focused_pane(focused);

        // 焦点 pane 变了：blink 定时器还按旧终端的样式在跑（或没跑）。给
        // 新聚焦终端补发一次 CursorBlinkingChange，让它按自己的样式起表。
        if self.blink_focus_pane != Some(focused) {
            self.blink_focus_pane = Some(focused);
            if let Some(idx) = self.pane_index(focused) {
                let pane = &self.panes[idx];
                EventProxy::new_tab(self.proxy.clone(), pane.window_route.clone(), pane.id)
                    .send_event(TerminalEvent::CursorBlinkingChange.into());
            }
        }

        if pane_rects.len() <= 1 {
            let id = pane_rects.first().map(|(id, _)| *id).unwrap_or(focused);
            if let Some(idx) = self.pane_index(id) {
                let pane = &mut self.panes[idx];
                let terminal_arc = pane.terminal.clone();
                let terminal = terminal_arc.lock();
                self.display.draw(
                    terminal,
                    scheduler,
                    &self.message_buffer,
                    &self.config,
                    &mut pane.search_state,
                    &mut pane.nebula_state,
                );
            }
        } else {
            self.display.begin_pane_frame(&self.config);
            let mut dim_rects = Vec::new();
            // The whole-window clear must not be tied to pane_rects[0]: a
            // layout leaf whose pane is gone (or a doc sentinel) is skipped
            // below, and skipping the clearing pane would leave every later
            // frame compositing over stale buffer contents (ghost frames).
            let mut cleared = false;
            // Pane focus AND window focus together decide the cursor's
            // focused look — a focused pane in an unfocused window must show
            // the hollow unfocused cursor, exactly like the single-pane path.
            let window_focused = self.display.window.has_focus();
            for (id, view) in pane_rects.iter() {
                let Some(idx) = self.pane_index(*id) else { continue };
                let is_focused = *id == focused;
                if !is_focused {
                    dim_rects.push((
                        view.padding_x(),
                        view.padding_y(),
                        // Split views use asymmetric padding: the sidebar is
                        // included on the left while the right keeps only the
                        // normal content margin. Using `2 * padding_x` here
                        // dropped the entire asymmetric difference from the
                        // dim veil, leaving a bright uncovered strip.
                        view.width() - view.padding_x() - view.padding_right(),
                        view.height() - view.padding_y() - view.padding_bottom(),
                    ));
                }
                let pane = &mut self.panes[idx];
                let terminal_arc = pane.terminal.clone();
                let terminal = terminal_arc.lock();
                self.display.draw_pane_view(
                    terminal,
                    &self.message_buffer,
                    &self.config,
                    &mut pane.search_state,
                    &mut pane.nebula_state,
                    *view,
                    is_focused && window_focused,
                    !cleared,
                );
                cleared = true;
            }
            if !cleared {
                crate::display::nebula_debug_log(format!(
                    "render_clear_missing active_tab={} layout_panes={} live_panes={} focused={focused}",
                    self.active_tab,
                    pane_rects.len(),
                    self.panes.len(),
                ));
            }
            self.display.draw_split_overlays(&dim_rects, &divider_rects);
            self.display.finish_pane_frame(scheduler);
        }

        // Startup profiling: the process-wide first completed frame.
        {
            use std::sync::atomic::AtomicBool;
            static FIRST_FRAME: AtomicBool = AtomicBool::new(false);
            if !FIRST_FRAME.swap(true, Ordering::Relaxed) {
                crate::boot_trace("first frame drawn");
            }
        }
    }

    /// Reorder the tab bar by moving the tab at index `from` to index `to`.
    /// With the pane pool the bar always lists every tab in storage order
    /// (displayed == storage index), so this is unconditional.
    pub(super) fn move_tab(&mut self, from: usize, to: usize) {
        let len = self.tabs.len();
        if from >= len || to >= len || from == to {
            return;
        }
        let entry = self.tabs.remove(from);
        self.tabs.insert(to, entry);
        // Keep the same tab focused: remap the active index through the move.
        self.active_tab = Self::shifted_index(self.active_tab, from, to);
        self.sync_chrome_tabs();
        self.dirty = true;
    }

    /// New position of `idx` after the element at `from` is removed and
    /// re-inserted at `to` (a single-element move within the vector).
    fn shifted_index(idx: usize, from: usize, to: usize) -> usize {
        if idx == from {
            to
        } else if from < to && idx > from && idx <= to {
            idx - 1
        } else if from > to && idx >= to && idx < from {
            idx + 1
        } else {
            idx
        }
    }

    pub(super) fn sync_chrome_tabs(&mut self) {
        let special = self
            .tabs
            .get(self.active_tab)
            .is_some_and(|tab| tab.doc.is_some() || tab.image.is_some() || tab.settings);
        self.display.set_special_tab_active(special);
        self.display.set_settings_tab_active(
            self.tabs.get(self.active_tab).is_some_and(|tab| tab.settings),
        );
        // The visible tab's activity is seen by definition — consume its
        // flag before it can render (dots are for background tabs only).
        if let Some(id) = self.tabs.get(self.active_tab).map(|t| t.active_pane) {
            if let Some(i) = self.pane_index(id) {
                self.panes[i].nebula_state.finished_unseen = false;
                self.panes[i].nebula_state.needs_attention = false;
                self.panes[i].nebula_state.failed_unseen = false;
            }
        }

        let mut labels = Vec::with_capacity(self.tabs.len());
        let mut colors = Vec::with_capacity(self.tabs.len());
        let mut dots = Vec::with_capacity(self.tabs.len());
        let mut running = Vec::with_capacity(self.tabs.len());
        let mut attention = Vec::with_capacity(self.tabs.len());
        let mut failed = Vec::with_capacity(self.tabs.len());
        let mut flashing = Vec::with_capacity(self.tabs.len());
        let mut logos = Vec::with_capacity(self.tabs.len());
        let mut shells = Vec::with_capacity(self.tabs.len());
        let mut ai_fork = Vec::with_capacity(self.tabs.len());
        // 静默行右侧的 shell 短标；Default 启动的 tab 用当前默认 shell 的。
        let default_tag = self.display.default_shell_tag();
        let ui_language = self.display.ui_language();
        for tab in &self.tabs {
            let pane = self.pane(tab.active_pane);
            let state = pane.map(|p| &p.nebula_state);
            // Use custom name if set, otherwise derive from cwd/title
            let mut label = if tab.settings {
                format!("\u{eb51} {}", ui_language.pick("设置", "Settings"))
            } else if let Some(custom) = &tab.custom_name {
                custom.clone()
            } else {
                pane.map(Self::chrome_tab_label).unwrap_or_default()
            };
            // Program icon (Nerd Font) in front of the label while a command
            // runs — the sidebar shows WHAT each tab is busy with. AI clients
            // with a real brand logo skip the glyph: the
            // display layer textures the actual mark into the icon slot.
            let logo =
                state.and_then(|s| s.running_program.as_deref()).and_then(crate::display::ai_logo);
            if let Some(program) = state.and_then(|s| s.running_program.as_deref()) {
                if logo.is_none() {
                    label = format!("{} {label}", crate::display::program_icon(program));
                }
            }
            logos.push(logo);
            labels.push(label);
            colors.push(tab.custom_color);
            shells.push(match &tab.launch {
                TabLaunch::Default => default_tag.clone(),
                TabLaunch::Shell { name, .. } => crate::shell_detect::shell_short_tag(name),
                // SSH 行的身份是目标主机（标签本身就写着），短标只说环境。
                TabLaunch::Ssh(_) => "ssh".to_owned(),
                TabLaunch::Profile(_)
                | TabLaunch::Document(_)
                | TabLaunch::Image(_)
                | TabLaunch::Settings => String::new(),
            });
            ai_fork.push(
                matches!(&tab.launch, TabLaunch::Default | TabLaunch::Shell { .. })
                    && state
                        .and_then(|state| state.ai_session.as_ref())
                        .and_then(|identity| {
                            crate::ai_agents::AgentKind::parse(&identity.source)
                                .and_then(|agent| agent.fork_command(&identity.session_id))
                        })
                        .is_some(),
            );
            // Unseen-result dot: bell in a background tab, a tracked command
            // that finished unseen, or a tracked program parked at "waiting
            // for input" (claude between turns). The ring collapsing into a
            // dot IS the "turn finished, your move" signal — also on the
            // visible tab, where a merely-paused ring still read as busy.
            dots.push(
                tab.has_bell
                    || state.is_some_and(|s| {
                        s.finished_unseen || (s.command_started.is_some() && s.awaiting_input)
                    }),
            );
            // Spinner only while the command actually works; once it rang BEL
            // and waits for input the dot above takes over.
            running.push(state.is_some_and(|s| s.command_started.is_some() && !s.awaiting_input));
            attention.push(state.is_some_and(|s| s.needs_attention));
            failed.push(state.is_some_and(|s| s.failed_unseen));
            // 对勾只在成功收尾后的一小段里亮着，随后落回圆点。
            flashing.push(state.is_some_and(|s| {
                s.finished_at.is_some_and(|at| at.elapsed() < crate::display::BADGE_FLASH)
            }));
        }
        let active = self.active_tab.min(labels.len().saturating_sub(1));
        // displayed == storage index always holds now, so the bar is reorderable.
        self.display.set_chrome_tabs(
            labels, colors, dots, running, attention, failed, flashing, logos, shells, ai_fork,
            active, true,
        );
    }

    pub(super) fn chrome_tab_label(pane: &Pane) -> String {
        let cwd = pane.nebula_state.cwd.trim();
        if !cwd.is_empty() {
            // Just the directory's own name: a full path wall-to-walls the
            // sidebar row and kills the design's breathing room. The last
            // meaningful component is what identifies the workspace anyway.
            let name = cwd
                .trim_end_matches(['/', '\\'])
                .rsplit(['/', '\\'])
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or(cwd);
            return name.to_owned();
        }

        if pane.title != "shell" && !pane.title.trim().is_empty() {
            return pane.title.clone();
        }

        std::env::current_dir()
            .ok()
            .and_then(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| ".".to_owned())
    }
}