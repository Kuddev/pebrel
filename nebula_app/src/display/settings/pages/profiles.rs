// Settings UI: Profiles page (shell, font, completion, config file).

use crate::display::color::Rgb;
use crate::display::ui::theme::Skin;
use crate::display::ui::widgets;
use crate::display::{AcceptKey, CompletionStyle, SettingsHit, SettingsDropdown, SizeInfo, UiLanguage};
use crate::display::settings::geometry::SettingsGeometry;
use crate::display::settings::view::SettingsView;
use crate::display::settings::{STANDARD_ROW_ACTION_W, settings_toggle_slot};
use crate::renderer::ui::{Rgba, UiQuad};
use crate::renderer::{GlyphCache, Renderer};
use unicode_width::UnicodeWidthChar;

pub(crate) fn push_profiles_quads(
    view: &SettingsView,
    quads: &mut Vec<UiQuad>,
    size: &SizeInfo,
    scale: f32,
    geometry: &SettingsGeometry,
    sk: &Skin,
    clip_top: f32,
    clip_bot: f32,
) {
    let s = |v: f32| v * scale;
    let clip = |quads: &mut Vec<UiQuad>, quad: UiQuad| {
        if let Some(quad) = quad.clip_y(clip_top, clip_bot) {
            quads.push(quad);
        }
    };
    let mut staged: Vec<UiQuad> = Vec::new();
    let group_frame = |_quads: &mut Vec<UiQuad>, _first_row, _rows: usize| {};
    let row_hover = |_quads: &mut Vec<UiQuad>, _rect, _hovered: bool| {};
    let action_button = |quads: &mut Vec<UiQuad>, row, logical_w: f32, hovered: bool| {
        let rect = super::super::row_action_rect(row, scale, logical_w);
        clip(
            quads,
            UiQuad::solid(
                rect.0,
                rect.1,
                rect.2,
                rect.3,
                rect.3 * 0.5,
                if hovered { sk.hover } else { sk.surface },
            ),
        );
    };
    let combobox = |quads: &mut Vec<UiQuad>,
                    staged: &mut Vec<UiQuad>,
                    row,
                    hot: bool,
                    open: bool| {
        widgets::push_combobox(staged, widgets::combobox_rect(row, scale), scale, &sk, hot, open);
        for quad in staged.drain(..) {
            clip(quads, quad);
        }
    };
    let toggle = |quads: &mut Vec<UiQuad>,
                  staged: &mut Vec<UiQuad>,
                  row,
                  hit: SettingsHit,
                  on: bool,
                  _hot: bool,
                  _pressed: bool| {
        let motion = settings_toggle_slot(hit)
            .map_or_else(|| widgets::ToggleMotion::settled(on), |index| view.toggle_motion[index]);
        widgets::push_toggle(staged, row, scale, &sk, motion);
        for quad in staged.drain(..) {
            clip(quads, quad);
        }
    };

    // 终端组：Shell / 启动目录 / 字体。下拉列表是浮层，行的
    // hairline 分组保持固定，不再被展开的列表推开。
    group_frame(quads, geometry.shell, 2);
    group_frame(quads, geometry.startup_directory, 1);
    group_frame(quads, geometry.font, 1);
    group_frame(quads, geometry.ghost, 3);
    group_frame(quads, geometry.open_config_file, 1);
    for (hit, rect) in [
        (SettingsHit::ShellCycle, geometry.shell),
        (SettingsHit::ImportTerminal, geometry.terminal_import),
        (SettingsHit::StartupDirectory, geometry.startup_directory),
        (SettingsHit::FontCycle, geometry.font),
        (SettingsHit::GhostToggle, geometry.ghost),
        (SettingsHit::AcceptCycle, geometry.accept),
        (SettingsHit::CompletionStyleCycle, geometry.completion_style),
        (SettingsHit::OpenConfigFile, geometry.open_config_file),
    ] {
        row_hover(quads, rect, view.hover == hit);
    }
    if view.startup_directory.is_some() {
        row_hover(
            quads,
            geometry.startup_directory_clear,
            view.hover == SettingsHit::StartupDirectoryClear,
        );
    }
    action_button(
        quads,
        geometry.terminal_import,
        STANDARD_ROW_ACTION_W,
        view.hover == SettingsHit::ImportTerminal,
    );
    action_button(
        quads,
        geometry.open_config_file,
        STANDARD_ROW_ACTION_W,
        view.hover == SettingsHit::OpenConfigFile,
    );
    combobox(
        quads,
        &mut staged,
        geometry.shell,
        view.hover == SettingsHit::ShellCycle,
        view.dropdown == Some(SettingsDropdown::Shell),
    );
    combobox(
        quads,
        &mut staged,
        geometry.font,
        view.hover == SettingsHit::FontCycle,
        view.dropdown == Some(SettingsDropdown::Font),
    );
    combobox(
        quads,
        &mut staged,
        geometry.accept,
        view.hover == SettingsHit::AcceptCycle,
        view.dropdown == Some(SettingsDropdown::Accept),
    );
    combobox(
        quads,
        &mut staged,
        geometry.completion_style,
        view.hover == SettingsHit::CompletionStyleCycle,
        view.dropdown == Some(SettingsDropdown::CompletionStyle),
    );
    // Boolean rows render a real switch instead of an "On/Off" string.
    for (rect, on) in [(geometry.ghost, view.ghost)] {
        toggle(
            quads,
            &mut staged,
            rect,
            SettingsHit::GhostToggle,
            on,
            view.hover == SettingsHit::GhostToggle,
            view.pressed == SettingsHit::GhostToggle,
        );
    }
    quads.extend(staged.drain(..));
}

pub(crate) fn draw_profiles_text(
    view: &SettingsView,
    r: &mut Renderer,
    gc: &mut GlyphCache,
    size: &SizeInfo,
    scale: f32,
    geometry: &SettingsGeometry,
    sk: &Skin,
    language: UiLanguage,
    cell_w: f32,
    cell_h: f32,
    icon_draws: &mut Vec<(String, (f32, f32, f32, f32))>,
    content_x: f32,
    px: f32,
    clip_top: f32,
    clip_bot: f32,
    title_h: f32,
) {
    let s = |v: f32| v * scale;
    let visible = |ry: f32, rh: f32| ry >= clip_top && ry + rh <= clip_bot;
    let row_text_y = |ry: f32, rh: f32| {
        if geometry.stacked_rows { ry + s(9.0) } else { ry + (rh - cell_h) / 2.0 }
    };
    let combobox_value = |r: &mut Renderer,
                          gc: &mut GlyphCache,
                          row: (f32, f32, f32, f32),
                          value: &str,
                          ink: Rgb| {
        let rect = widgets::combobox_rect(row, scale);
        let tx = widgets::combobox_text_x(rect, scale);
        let right = widgets::combobox_text_right(rect, scale);
        let max_chars = ((right - tx).max(cell_w) / cell_w).floor().max(1.0) as usize;
        let value = super::super::truncate_tab_label(value, max_chars);
        r.draw_chrome_text(size, tx, rect.1 + (rect.3 - cell_h) / 2.0, ink, &value, gc);
    };
    let group_y = |row_y: f32| row_y - s(42.0);
    // Rows carry single, self-explanatory Chinese labels — the old
    // second-line descriptions overflowed the 44px rows and collided
    // with the next group's title.
    let (sh_x, sh_y, _, sh_h) = geometry.shell;
    if visible(group_y(sh_y), title_h) {
        super::super::render::section_title(
            r,
            gc,
            size,
            scale,
            &sk,
            sh_x,
            group_y(sh_y),
            language.pick("终端", "Terminal"),
        );
    }
    if visible(sh_y, sh_h) {
        super::super::render::row_label(
            r,
            gc,
            size,
            scale,
            &sk,
            geometry.shell,
            language.pick("默认 Shell", "Default shell"),
            "",
            sk.ink,
        );
        combobox_value(r, gc, geometry.shell, &view.shell_label, sk.accent);
    }
    let (_ix, iy, _, ih) = geometry.terminal_import;
    if visible(iy, ih) {
        super::super::render::row_label(
            r,
            gc,
            size,
            scale,
            &sk,
            geometry.terminal_import,
            language.pick("导入终端目录", "Import terminal directory"),
            "",
            sk.ink,
        );
        super::super::render::draw_button_label(
            r,
            gc,
            size,
            super::super::row_action_rect(geometry.terminal_import, scale, super::super::STANDARD_ROW_ACTION_W),
            language.pick("导入", "Import"),
            if view.hover == SettingsHit::ImportTerminal { sk.accent } else { sk.ink },
        );
    }
    if visible(geometry.startup_directory.1, geometry.startup_directory.3) {
        super::super::render::row_label(
            r,
            gc,
            size,
            scale,
            &sk,
            geometry.startup_directory,
            language.pick("启动目录", "Startup directory"),
            "",
            sk.ink,
        );

        let (dx, dy, dw, dh) = geometry.startup_directory;
        // 与"默认 Shell / 终端字体"同一右对齐基线；有清除按钮时向左避让。
        let stacked = dh >= s(56.0);
        let value_left = if stacked { dx + s(16.0) } else { dx + dw * 0.42 };
        let value_right = if view.startup_directory.is_some() {
            geometry.startup_directory_clear.0 - s(12.0)
        } else {
            dx + dw - s(16.0)
        };
        let max_chars = ((value_right - value_left).max(cell_w) / cell_w).floor() as usize;
        let value = view
            .startup_directory
            .as_deref()
            .unwrap_or_else(|| language.pick("继承当前目录", "Inherit current directory"));
        let value = super::super::truncate_tab_label(value, max_chars.max(1));
        let value_cols: usize = value.chars().map(|c| c.width().unwrap_or(0)).sum();
        let value_x = (value_right - value_cols as f32 * cell_w).max(value_left);

        let value_y = if stacked { dy + s(9.0) + cell_h } else { dy + (dh - cell_h) / 2.0 };
        r.draw_chrome_text(
            size,
            if stacked { value_left } else { value_x },
            value_y,
            if view.startup_directory.is_some() { sk.accent } else { sk.ink_dim },
            &value,
            gc,
        );

        if view.startup_directory.is_some() {
            let (cx, cy, cw, ch) = geometry.startup_directory_clear;
            let clear = language.pick("清除", "Clear");
            let clear_cols: usize =
                clear.chars().map(|character| character.width().unwrap_or(0)).sum();
            r.draw_chrome_text(
                size,
                cx + (cw - clear_cols as f32 * cell_w) / 2.0,
                cy + (ch - cell_h) / 2.0,
                sk.accent,
                clear,
                gc,
            );
        }
    }
    if visible(geometry.font.1, geometry.font.3) {
        // 查询串现在长在弹层顶部的搜索框里，触发器只管报当前字体。
        // 加载失败的告警优先。
        let font_value = match view.font_notice.as_deref() {
            Some(notice) => notice,
            None => &view.font_family,
        };
        super::super::render::row_label(
            r,
            gc,
            size,
            scale,
            &sk,
            geometry.font,
            language.pick("终端字体", "Terminal font"),
            "",
            sk.ink,
        );
        combobox_value(
            r,
            gc,
            geometry.font,
            font_value,
            if view.font_notice.is_some() { sk.ink_dim } else { sk.accent },
        );
    }
    let (gh_x, gh_y, _, gh_h) = geometry.ghost;
    if visible(group_y(gh_y), title_h) {
        super::super::render::section_title(
            r,
            gc,
            size,
            scale,
            &sk,
            gh_x,
            group_y(gh_y),
            language.pick("补全", "Completion"),
        );
    }
    if visible(gh_y, gh_h) {
        super::super::render::row_label(
            r,
            gc,
            size,
            scale,
            &sk,
            geometry.ghost,
            language.pick("历史补全灰字", "History ghost text"),
            "",
            sk.ink,
        );
    }
    if visible(geometry.accept.1, geometry.accept.3) {
        super::super::render::row_label(
            r,
            gc,
            size,
            scale,
            &sk,
            geometry.accept,
            language.pick("补全接受键", "Completion accept key"),
            "",
            sk.ink,
        );
        combobox_value(
            r,
            gc,
            geometry.accept,
            super::super::accept_label(view.accept, language),
            sk.accent,
        );
    }
    if visible(geometry.completion_style.1, geometry.completion_style.3) {
        super::super::render::row_label(
            r,
            gc,
            size,
            scale,
            &sk,
            geometry.completion_style,
            language.pick("补全样式", "Completion style"),
            "",
            sk.ink,
        );
        combobox_value(
            r,
            gc,
            geometry.completion_style,
            super::super::completion_style_label(view.completion_style, language),
            sk.accent,
        );
    }

    let (ocx, ocy, _ocw, och) = geometry.open_config_file;
    if visible(group_y(ocy), title_h) {
        super::super::render::section_title(
            r,
            gc,
            size,
            scale,
            &sk,
            ocx,
            group_y(ocy),
            language.pick("配置文件", "Configuration"),
        );
    }
    if visible(ocy, och) {
        super::super::render::row_label(
            r,
            gc,
            size,
            scale,
            &sk,
            geometry.open_config_file,
            language.pick("打开配置文件", "Open configuration file"),
            "",
            sk.ink,
        );
        super::super::render::draw_button_label(
            r,
            gc,
            size,
            super::super::row_action_rect(geometry.open_config_file, scale, super::super::STANDARD_ROW_ACTION_W),
            language.pick("打开", "Open"),
            if view.hover == SettingsHit::OpenConfigFile { sk.accent } else { sk.ink },
        );
    }
}