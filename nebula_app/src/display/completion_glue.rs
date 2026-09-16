//! Completion glue: config reload, highlighted-hint refresh, the commit-line
//! and directory-recording hooks that feed suggestions, the suggestion
//! recompute, and the inline completion popup paint.

use unicode_width::UnicodeWidthChar;
use winit::keyboard::ModifiersState;
use winit::window::CursorIcon;

use nebula_terminal::grid::Dimensions;
use nebula_terminal::index::{Column, Point};
use nebula_terminal::selection::Selection;
use nebula_terminal::term::cell::Flags;
use nebula_terminal::term::{Term, TermMode};

use crate::config::UiConfig;
use crate::display::color::{List, Rgb};
use crate::event::Mouse;
use crate::renderer::ui::{Rgba, UiQuad};

use super::hint;
use super::input_state::nebula_clear_line;
use super::suggest_engine;
use super::ui;
use super::{
    Display, NebulaCompletionItem, NebulaCompletionKind, NebulaPaneState, SizeInfo,
    nebula_debug_log, nebula_pad_to_cells,
};

impl Display {
    /// Update to a new configuration.
    pub fn update_config(&mut self, config: &UiConfig) {
        self.nebula_config_paths.clone_from(&config.config_paths);
        self.nebula_profiles.clone_from(&config.profiles);
        self.damage_tracker.debug = config.debug.highlight_damage;
        self.visual_bell.update_config(&config.bell);
        // Refresh the base scheme, then re-apply the active theme's restyle.
        self.nebula_default_colors = List::from(&config.colors);
        let defaults = self.nebula_default_colors;
        self.nebula_theme.apply_term_colors(&mut self.colors, &defaults);
    }

    /// Update the mouse/vi mode cursor hint highlighting.
    ///
    /// This will return whether the highlighted hints changed.
    pub fn update_highlighted_hints<T>(
        &mut self,
        term: &Term<T>,
        config: &UiConfig,
        mouse: &Mouse,
        point: Point,
        modifiers: ModifiersState,
    ) -> bool {
        // Update vi mode cursor hint.
        let vi_highlighted_hint = if term.mode().contains(TermMode::VI) {
            let mods = ModifiersState::all();
            let point = term.vi_mode_cursor.point;
            hint::highlighted_at(term, config, point, mods)
        } else {
            None
        };
        let mut dirty = vi_highlighted_hint != self.vi_highlighted_hint;
        self.vi_highlighted_hint = vi_highlighted_hint;
        self.vi_highlighted_hint_age = 0;

        // Force full redraw if the vi mode highlight was cleared.
        if dirty {
            self.damage_tracker.frame().mark_fully_damaged();
        }

        // Abort if mouse highlighting conditions are not met.
        if !self.window.mouse_visible()
            || !mouse.inside_text_area
            || !term.selection.as_ref().is_none_or(Selection::is_empty)
        {
            if self.highlighted_hint.take().is_some() {
                self.damage_tracker.frame().mark_fully_damaged();
                dirty = true;
            }
            return dirty;
        }

        // `point` has already passed through the focused pane's math projection,
        // so hover and click resolve the same immutable source cell.
        let highlighted_hint = hint::highlighted_at(term, config, point, modifiers);

        // Update cursor shape.
        if highlighted_hint.is_some() {
            // If mouse changed the line, we should update the hyperlink preview, since the
            // highlighted hint could be disrupted by the old preview.
            dirty = self.hint_mouse_point.is_some_and(|p| p.line != point.line);
            self.hint_mouse_point = Some(point);
            self.window.set_mouse_cursor(CursorIcon::Pointer);
        } else if self.highlighted_hint.is_some() {
            self.hint_mouse_point = None;
            if term.mode().intersects(TermMode::MOUSE_MODE) && !term.mode().contains(TermMode::VI) {
                self.window.set_mouse_cursor(CursorIcon::Default);
            } else {
                // Nebula: normal arrow over the terminal area (no I-beam).
                self.window.set_mouse_cursor(CursorIcon::Default);
            }
        }

        let mouse_highlight_dirty = self.highlighted_hint != highlighted_hint;
        dirty |= mouse_highlight_dirty;
        self.highlighted_hint = highlighted_hint;
        self.highlighted_hint_age = 0;

        // Force full redraw if the mouse cursor highlight was changed.
        if mouse_highlight_dirty {
            self.damage_tracker.frame().mark_fully_damaged();
        }

        dirty
    }

    /// Commit the current line to history (on Enter) and reset the buffer.
    ///
    /// `screen_line` (the input read off the grid, i.e. what the shell's own
    /// editor really contained) wins over the keystroke-reconstructed
    /// `line_buf`: the latter desyncs on cursor motion / completion / history
    /// recall and used to commit spliced garbage like "laudeclaude", which the
    /// hint would then resurface as a command the user never typed.
    pub fn nebula_commit_line(&mut self, state: &mut NebulaPaneState) {
        // On Windows the grid read is the only source that sees tab
        // completions; when it failed (no prompt arrow — cmd/ssh/REPL — or a
        // mid-line edit) the keystroke buffer likely holds spliced garbage,
        // and recording that would resurface it forever as a bogus ghost hint
        // (truncated CJK paths were the visible symptom). Better no history
        // entry than a corrupted one.
        #[cfg(windows)]
        let line = state.screen_line.trim().to_owned();
        #[cfg(not(windows))]
        let line = if state.screen_line.trim().is_empty() {
            state.line_buf.trim()
        } else {
            state.screen_line.trim()
        }
        .to_owned();
        nebula_debug_log(format!(
            "input_commit cwd={:?} line={line:?} line_buf={:?} screen_line={:?}",
            state.cwd, state.line_buf, state.screen_line
        ));
        self.nebula_history.record(&state.suggest_env.history_scope(), &line, &state.cwd);
        // Kept for CommandStart (OSC 133;C): by the time it arrives from the
        // PTY these buffers are already cleared, so the program identity for
        // the tab icon has to be captured here. Fall back to the keystroke
        // buffer so the icon still resolves when the grid read failed. Agent
        // parsing also understands package runners such as npx/uvx.
        state.last_committed =
            if line.is_empty() { state.line_buf.trim().to_owned() } else { line };
        if let Some(agent) = crate::ai_agents::AgentKind::parse_command(&state.last_committed) {
            state.running_program = Some(agent.slug().to_owned());
            state.command_started = Some(std::time::Instant::now());
            state.agent_status = crate::ai_agents::AgentStatus::Working;
            state.agent_status_source = crate::ai_agents::AgentStatusSource::Process;
            state.agent_status_rule = None;
            state.agent_hook_seen = false;
            state.idle_screen_streak = 0;
            state.awaiting_input = false;
            state.finished_unseen = false;
            state.needs_attention = false;
        }
        nebula_clear_line(state);
    }

    /// Feed the shared directory model from an authoritative shell report.
    pub fn nebula_record_directory(&self, cwd: &str) {
        self.directory_history.record(cwd);
    }

    /// Recompute the inline ghost-text suggestion. `line_override` carries the
    /// grid-read input on Windows (the authoritative screen truth); when `None`
    /// the keystroke-tracked `line_buf` is used (other platforms). A whole
    /// previous command sharing the prefix wins (fish-style history hint);
    /// otherwise the final token gets path completion against the shell-reported
    /// cwd. Cached on `cwd\0buffer` so disk is only touched when the line
    /// changes — not every frame.
    pub(super) fn nebula_update_suggestion(
        &mut self,
        state: &mut NebulaPaneState,
        line_override: Option<String>,
    ) {
        suggest_engine::suggest_update(
            &suggest_engine::SuggestSources {
                history: &self.nebula_history,
                directories: &self.directory_history,
                commands: &self.nebula_commands,
                enabled: self.nebula_ghost_enabled,
                style: self.nebula_completion_style,
            },
            state,
            line_override,
        );
    }

    /// Render the popup completion list on the terminal cell grid: one padded
    /// row per candidate directly below the cursor (above when the prompt sits
    /// near the bottom), the selected row on the theme accent. Cell-grid
    /// `draw_string` keeps this inside the pane projection — no chrome quads,
    /// so splits and scrolled panes behave like the ghost text does.
    pub(super) fn draw_completion_popup(
        &mut self,
        items: &[NebulaCompletionItem],
        selected: Option<usize>,
        anchor: Point<usize>,
        size_info: &SizeInfo,
        term_bg: Rgb,
    ) {
        let columns = size_info.columns();
        let screen_lines = size_info.screen_lines();
        if columns < 12 || screen_lines < 2 {
            return;
        }

        // Rows: prefer the space below the cursor, else above; clamp count.
        let below = screen_lines.saturating_sub(anchor.line + 1);
        let above = anchor.line;
        let want = items.len().min(8);
        let (rows, start_line) = if below >= want || below >= above {
            (want.min(below), anchor.line + 1)
        } else {
            (want.min(above), anchor.line - want.min(above))
        };
        if rows == 0 {
            return;
        }
        // When the list is cut short keep an explicitly selected row visible.
        let selected = selected.filter(|index| *index < items.len());
        let offset =
            selected.filter(|index| *index >= rows).map(|index| index + 1 - rows).unwrap_or(0);

        let language = self.nebula_language;
        let tag = |kind: NebulaCompletionKind| -> &'static str {
            match kind {
                NebulaCompletionKind::History => language.pick("历史", "hist"),
                NebulaCompletionKind::Command => language.pick("命令", "cmd"),
                NebulaCompletionKind::Dir => language.pick("目录", "dir"),
                NebulaCompletionKind::File => language.pick("文件", "file"),
            }
        };
        let icon = |kind: NebulaCompletionKind| -> char {
            match kind {
                NebulaCompletionKind::History => '↶',
                NebulaCompletionKind::Command => '›',
                NebulaCompletionKind::Dir => '/',
                NebulaCompletionKind::File => '·',
            }
        };
        let cell_width =
            |text: &str| -> usize { text.chars().map(|c| c.width().unwrap_or(0)).sum() };

        let visible = &items[offset..(offset + rows).min(items.len())];
        let tag_w = visible.iter().map(|item| cell_width(tag(item.kind))).max().unwrap_or(0);
        let label_w_max = visible.iter().map(|item| cell_width(&item.label)).max().unwrap_or(0);

        // ` label  tag ` — 1 cell padding each side, 2 cells between.
        let mut start_col = anchor.column.0;
        let mut avail = columns - start_col;
        let full_w = label_w_max + tag_w + 4;
        if full_w > avail {
            // Slide left rather than shrink first; narrow panes then clamp.
            let slide = (full_w - avail).min(start_col);
            start_col -= slide;
            avail += slide;
        }
        let width = full_w.min(avail);
        let label_w = width.saturating_sub(tag_w + 4);
        if label_w == 0 {
            return;
        }

        let sk = self.nebula_theme.skin();
        let opaque = |c: Rgb| Rgba::new(c.r, c.g, c.b, 255);
        let rgb = |c: Rgba| Rgb::new(c.r, c.g, c.b);
        let row_bg = rgb(ui::icons::blend_over(opaque(term_bg), sk.panel));
        let scale = self.window.scale_factor as f32;
        let cell_w = size_info.cell_width();
        let cell_h = size_info.cell_height();
        let content_x = size_info.padding_x() + start_col as f32 * cell_w;
        let content_y = size_info.padding_y() + start_line as f32 * cell_h;
        let content_w = width as f32 * cell_w;
        let content_h = rows as f32 * cell_h;
        let panel_pad = (4.0 * scale).round();
        let panel = (
            content_x - panel_pad,
            content_y - panel_pad,
            content_w + panel_pad * 2.0,
            content_h + panel_pad * 2.0,
        );
        let mut quads = Vec::with_capacity(4);
        ui::surface::push_surface_with_radius(
            &mut quads,
            panel,
            (0.0, 0.0, size_info.width(), size_info.height()),
            0.0,
            scale,
            &sk,
            ui::surface::Elevation::Menu,
            1.0,
            8.0,
        );
        if let Some(selected) = selected.filter(|index| *index >= offset && *index < offset + rows)
        {
            let row = selected - offset;
            quads.push(UiQuad::solid(
                content_x,
                content_y + row as f32 * cell_h,
                content_w,
                cell_h,
                6.0 * scale,
                sk.accent_soft,
            ));
        }
        self.renderer.draw_ui(size_info, &quads);

        for (row, item) in visible.iter().enumerate() {
            let line = start_line + row;
            if line >= screen_lines {
                break;
            }
            let is_selected = Some(offset + row) == selected;
            let (label_fg, tag_fg, style) = if is_selected {
                (sk.ink_strong, sk.ink_strong, Flags::BOLD)
            } else {
                (sk.ink, sk.ink_faint, Flags::empty())
            };
            let label =
                format!("{} {}", icon(item.kind), nebula_pad_to_cells(&item.label, label_w + 1));
            let tag_text = format!("{} ", nebula_pad_to_cells(tag(item.kind), tag_w));
            let glyph_cache = &mut self.glyph_cache;
            self.renderer.draw_string_styled(
                Point::new(line, Column(start_col)),
                label_fg,
                row_bg,
                label.chars(),
                style,
                0.0,
                size_info,
                glyph_cache,
            );
            // Fits by construction (start_col + width <= columns); the
            // renderer clips at the grid edge regardless.
            let tag_col = start_col + 1 + label_w + 2;
            let glyph_cache = &mut self.glyph_cache;
            self.renderer.draw_string_styled(
                Point::new(line, Column(tag_col)),
                tag_fg,
                row_bg,
                tag_text.chars(),
                Flags::empty(),
                0.0,
                size_info,
                glyph_cache,
            );
        }
    }
}
