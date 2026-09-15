//! Winit event dispatching for the legacy shell window context.

use super::*;

impl WindowContext {
    pub fn apply_pending_native_transition(&mut self) {
        if self.display.window.native_live_move() {
            return;
        }
        if let Some(scale_factor) = self.display.window.take_pending_scale_factor() {
            let start = Instant::now();
            self.display.apply_scale_factor_change(scale_factor, &self.config);
            crate::display::nebula_debug_log(format!(
                "winmove pending_scale {scale_factor} applied in {:?}",
                start.elapsed()
            ));
            self.dirty = true;
        }
        if let Some(size) = self.display.window.take_pending_inner_size() {
            crate::display::nebula_debug_log(format!(
                "winmove pending_size {}x{} applied",
                size.width, size.height
            ));
            if self.display.window.allows_drag_resize() {
                self.windowed_size = size.to_logical(self.display.window.scale_factor);
            }
            self.display.pending_update.set_dimensions(size);
            self.dirty = true;
        }
    }

    pub fn handle_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        event_proxy: &EventLoopProxy<Event>,
        clipboard: &mut Clipboard,
        scheduler: &mut Scheduler,
        event: WinitEvent<Event>,
    ) {
        self.display.sync_system_theme(event_loop.system_theme());
        match event {
            WinitEvent::AboutToWait
            | WinitEvent::WindowEvent { event: WindowEvent::RedrawRequested, .. } => {
                if self.event_queue.is_empty() && !self.display.pending_update.dirty {
                    return;
                }
            },
            event => { self.event_queue.push(event); return; },
        }
        self.preprocess_split_mouse();
        let bell_panes: Vec<u64> = self.event_queue.iter()
            .filter_map(|e| match e { WinitEvent::UserEvent(ev) => ev.terminal_bell_pane(), _ => None })
            .collect();
        for pane_id in bell_panes { self.mark_pane_bell(pane_id); }
        let key_pressed = self.event_queue.iter().any(|e| matches!(e, WinitEvent::WindowEvent { event: WindowEvent::KeyboardInput { event: key, .. }, .. } if key.state == ElementState::Pressed));
        if key_pressed {
            let focused = self.focused_pane_id();
            if let Some(i) = self.pane_index(focused) { self.panes[i].nebula_state.awaiting_input = false; self.panes[i].nebula_state.needs_attention = false; }
        }
        if self.display.nebula_confirm.is_none() && !matches!(self.active_layout(), Layout::Leaf(_)) {
            let ffm = self.config.mouse.focus_follows_mouse;
            let latest_pos = self.event_queue.iter().rev().find_map(|e| match e { WinitEvent::WindowEvent { event: WindowEvent::CursorMoved { position, .. }, .. } => Some((position.x as f32, position.y as f32)), _ => None });
            let clicked = self.event_queue.iter().any(|e| matches!(e, WinitEvent::WindowEvent { event: WindowEvent::MouseInput { state: ElementState::Pressed, button, .. }, .. } if pane_focus_button(button)));
            let target = if clicked { latest_pos.or(Some((self.mouse.x as f32, self.mouse.y as f32))) } else if ffm { latest_pos } else { None };
            if let Some((px, py)) = target { if let Some(id) = self.pane_at_position(px, py) { if self.tabs[self.active_tab].active_pane != id { self.tabs[self.active_tab].active_pane = id; self.dirty = true; } } }
        }
        let normal_focus = self.focused_pane_id();
        let focused_id = routed_input_pane(self.display.nebula_confirm.as_ref(), normal_focus, |pane_id| self.pane_index(pane_id).is_some());
        let special_tab = self.tabs.get(self.active_tab).is_some_and(|tab| tab.doc.is_some() || tab.image.is_some() || tab.settings);
        let focused = match self.pane_index(focused_id) { Some(index) => Some(index), None if special_tab => None, None => return };
        let pane_rects = self.layout_geometry(false).0;
        let pane_view = if pane_rects.len() > 1 { pane_rects.iter().find(|(id, _)| *id == focused_id).map(|(_, v)| *v) } else { None };
        self.display.nebula_pane_view = pane_view;
        let old_is_searching = focused.is_some_and(|index| self.panes[index].search_state.history_index.is_some());
        let target_of = |event: &WinitEvent<Event>| match event { WinitEvent::UserEvent(event) => event.terminal_tab_id().unwrap_or(focused_id), _ => focused_id };
        let mut events = mem::take(&mut self.event_queue).into_iter().peekable();
        while let Some(event) = events.next() {
            let target_id = target_of(&event);
            let (pane, doc, image) = match self.pane_index(target_id) {
                Some(pane_idx) => (&mut self.panes[pane_idx], None, None),
                None if target_id == DOC_PANE_ID && special_tab => {
                    let tab = &mut self.tabs[self.active_tab];
                    (&mut self.doc_pane, tab.doc.as_mut(), tab.image.as_mut())
                },
                None => { while events.next_if(|event| target_of(event) == target_id).is_some() {} continue; },
            };
            let terminal_arc = pane.terminal.clone();
            let mut terminal = terminal_arc.lock();
            let context = ActionContext {
                pane_id: pane.id, cursor_blink_timed_out: &mut self.cursor_blink_timed_out,
                prev_bell_cmd: &mut self.prev_bell_cmd, message_buffer: &mut self.message_buffer,
                inline_search_state: &mut pane.inline_search_state, search_state: &mut pane.search_state,
                nebula_state: &mut pane.nebula_state, ssh_destination: pane.ssh_destination.as_deref(),
                doc, image, modifiers: &mut self.modifiers, notifier: &mut pane.notifier,
                display: &mut self.display, windowed_size: &mut self.windowed_size,
                mouse: &mut self.mouse, touch: &mut self.touch,
                dirty: &mut self.dirty, occluded: &mut self.occluded,
                terminal: &mut terminal,
                #[cfg(not(windows))] master_fd: pane.master_fd,
                #[cfg(not(windows))] shell_pid: pane.shell_pid,
                preserve_title: self.preserve_title, config: &self.config,
                event_proxy,
                #[cfg(target_os = "macos")] event_loop,
                clipboard, scheduler,
            };
            let mut processor = input::Processor::new(context);
            processor.handle_event(event);
            while let Some(event) = events.next_if(|event| target_of(event) == target_id) { processor.handle_event(event); }
        }
        if self.display.pending_update.terminal_colors_dirty() {
            let dark = { let bg = self.display.colors[nebula_terminal::vte::ansi::NamedColor::Background]; nebula_terminal::term::background_is_dark(bg.r, bg.g, bg.b) };
            for pane in &self.panes { let mut terminal = pane.terminal.lock(); terminal.reset_dynamic_colors(); terminal.set_color_scheme(dark); }
            let mut doc = self.doc_pane.terminal.lock(); doc.reset_dynamic_colors(); doc.set_color_scheme(dark); drop(doc);
            self.dirty = true;
        }
        let terminal_arc = match focused { Some(index) => self.panes[index].terminal.clone(), None => self.doc_pane.terminal.clone() };
        let mut terminal = terminal_arc.lock();
        if self.display.pending_update.dirty {
            let update_start = Instant::now();
            let pane = match focused { Some(index) => &mut self.panes[index], None => &mut self.doc_pane };
            Self::submit_display_update(&mut terminal, &mut self.display, &mut pane.notifier, &self.message_buffer, &mut pane.search_state, old_is_searching, &self.config);
            crate::display::nebula_debug_log(format!("winmove display_update in {:?}", update_start.elapsed()));
            self.dirty = true;
            if self.display.nebula_pty_resize_pending {
                let now = Instant::now();
                let dragging = self.last_pty_resize.is_some_and(|t| now.duration_since(t) < Duration::from_millis(300));
                if dragging {
                    let timer = TimerId::new(Topic::NebulaResizeSettle, self.display.window.id());
                    scheduler.unschedule(timer);
                    let event = Event::new(EventType::NebulaResizeSettled, self.display.window.id());
                    scheduler.schedule(event, Duration::from_millis(150), false, timer);
                } else {
                    self.display.nebula_pty_resize_pending = false;
                    self.last_pty_resize = Some(now);
                    drop(terminal);
                    self.resize_active_layout();
                    terminal = terminal_arc.lock();
                }
            }
        }
        if self.dirty || self.mouse.hint_highlight_dirty {
            let view = self.display.pane_view();
            let visual_point = self.mouse.point(&view, &*terminal);
            let pane = match focused { Some(index) => &self.panes[index], None => &self.doc_pane };
            let hint_point = pane.nebula_state.terminal_math_source_point(visual_point, self.mouse.cell_side, terminal.viewport_origin_for(view.screen_lines())).0;
            self.dirty |= self.display.update_highlighted_hints(&terminal, &self.config, &self.mouse, hint_point, self.modifiers.state());
            self.mouse.hint_highlight_dirty = false;
        }
        if self.dirty && self.display.window.has_frame && !self.occluded && !matches!(event, WinitEvent::WindowEvent { event: WindowEvent::RedrawRequested, .. }) {
            self.display.window.request_redraw();
        }
    }

    pub fn id(&self) -> WindowId { self.display.window.id() }

    pub fn write_ref_test_results(&self) {
        let focused = self.focused_pane_id();
        let mut grid = self.pane(focused).expect("focused pane exists").terminal.lock().grid().clone();
        grid.initialize_all(); grid.truncate();
        let serialized_grid = json::to_string(&grid).expect("serialize grid");
        let size_info = &self.display.size_info;
        let size = TermSize::new(size_info.columns(), size_info.screen_lines());
        let serialized_size = json::to_string(&size).expect("serialize size");
        let serialized_config = format!("{{\"history_size\":{}}}", grid.history_size());
        File::create("./grid.json").and_then(|mut f| f.write_all(serialized_grid.as_bytes())).expect("write grid.json");
        File::create("./size.json").and_then(|mut f| f.write_all(serialized_size.as_bytes())).expect("write size.json");
        File::create("./config.json").and_then(|mut f| f.write_all(serialized_config.as_bytes())).expect("write config.json");
    }

    pub fn apply_settled_pty_resize(&mut self) {
        if !mem::take(&mut self.display.nebula_pty_resize_pending) { return; }
        self.last_pty_resize = Some(Instant::now());
        self.resize_active_layout();
    }

    fn submit_display_update(
        terminal: &mut Term<EventProxy>,
        display: &mut Display,
        notifier: &mut Notifier,
        message_buffer: &MessageBuffer,
        search_state: &mut SearchState,
        old_is_searching: bool,
        config: &UiConfig,
    ) {
        let num_lines = terminal.screen_lines();
        let cursor_at_bottom = terminal.grid().cursor.point.line + 1 == num_lines;
        let origin_at_bottom = if terminal.mode().contains(TermMode::VI) {
            terminal.vi_mode_cursor.point.line == num_lines - 1
        } else { search_state.direction == Direction::Left };
        display.handle_update(terminal, notifier, message_buffer, search_state, config);
        let new_is_searching = search_state.history_index.is_some();
        if !old_is_searching && new_is_searching {
            let display_offset = terminal.grid().display_offset();
            if display_offset == 0 && cursor_at_bottom && !origin_at_bottom {
                terminal.scroll_display(Scroll::Delta(1));
            } else if display_offset != 0 && origin_at_bottom {
                terminal.scroll_display(Scroll::Delta(-1));
            }
        }
    }
}