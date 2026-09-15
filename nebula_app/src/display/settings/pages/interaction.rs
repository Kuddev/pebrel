// Settings UI: Interaction page (clipboard, tab reveal, panel resize, CJK bold).

use crate::display::color::Rgb;
use crate::display::ui::theme::Skin;
use crate::display::ui::widgets;
use crate::display::{SettingsHit, SettingsDropdown, SizeInfo, UiLanguage};
use crate::display::settings::geometry::SettingsGeometry;
use crate::display::settings::view::SettingsView;
use crate::display::settings::settings_toggle_slot;
use crate::renderer::ui::{Rgba, UiQuad};
use crate::renderer::{GlyphCache, Renderer};
use unicode_width::UnicodeWidthChar;

pub(crate) fn push_interaction_quads(
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

    group_frame(quads, geometry.copy_on_select, 4);
    row_hover(
        quads,
        geometry.copy_on_select,
        view.hover == SettingsHit::CopyOnSelectToggle,
    );
    toggle(
        quads,
        &mut staged,
        geometry.copy_on_select,
        SettingsHit::CopyOnSelectToggle,
        view.copy_on_select,
        view.hover == SettingsHit::CopyOnSelectToggle,
        view.pressed == SettingsHit::CopyOnSelectToggle,
    );
    row_hover(quads, geometry.tab_reveal, view.hover == SettingsHit::TabRevealDropdown);
    combobox(
        quads,
        &mut staged,
        geometry.tab_reveal,
        view.hover == SettingsHit::TabRevealDropdown,
        view.dropdown == Some(SettingsDropdown::TabReveal),
    );
    row_hover(
        quads,
        geometry.new_tab_position,
        view.hover == SettingsHit::NewTabPositionDropdown,
    );
    combobox(
        quads,
        &mut staged,
        geometry.new_tab_position,
        view.hover == SettingsHit::NewTabPositionDropdown,
        view.dropdown == Some(SettingsDropdown::NewTabPosition),
    );
    row_hover(quads, geometry.panel_resize, view.hover == SettingsHit::PanelResizeToggle);
    toggle(
        quads,
        &mut staged,
        geometry.panel_resize,
        SettingsHit::PanelResizeToggle,
        view.panel_resize,
        view.hover == SettingsHit::PanelResizeToggle,
        view.pressed == SettingsHit::PanelResizeToggle,
    );
    group_frame(quads, geometry.cjk_bold, 1);
    row_hover(quads, geometry.cjk_bold, view.hover == SettingsHit::CjkBoldToggle);
    toggle(
        quads,
        &mut staged,
        geometry.cjk_bold,
        SettingsHit::CjkBoldToggle,
        view.cjk_bold_regular,
        view.hover == SettingsHit::CjkBoldToggle,
        view.pressed == SettingsHit::CjkBoldToggle,
    );
    quads.extend(staged.drain(..));
}

pub(crate) fn draw_interaction_text(
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
    _icon_draws: &mut Vec<(String, (f32, f32, f32, f32))>,
    _content_x: f32,
    _px: f32,
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
    let (ix, iy, _, ih) = geometry.copy_on_select;
    if visible(group_y(iy), title_h) {
        super::super::render::section_title(
            r,
            gc,
            size,
            scale,
            &sk,
            ix,
            group_y(iy),
            language.pick("剪贴板", "Clipboard"),
        );
    }
    if visible(iy, ih) {
        // The switch (drawn in `push_quads`) carries the state; the
        // label spells out the OFF fallback so both modes are clear.
        super::super::render::row_label(
            r,
            gc,
            size,
            scale,
            &sk,
            geometry.copy_on_select,
            language.pick(
                "自动将所选内容复制到剪贴板（关闭时右键复制 / 粘贴）",
                "Copy selection to clipboard (off: right-click copies / pastes)",
            ),
            "",
            sk.ink,
        );
    }
    if visible(geometry.tab_reveal.1, geometry.tab_reveal.3) {
        super::super::render::row_label(
            r,
            gc,
            size,
            scale,
            &sk,
            geometry.tab_reveal,
            language.pick("标签展开", "Tab reveal"),
            "",
            sk.ink,
        );
        combobox_value(
            r,
            gc,
            geometry.tab_reveal,
            super::super::tab_reveal_label(view.tab_reveal, language),
            sk.accent,
        );
    }
    if visible(geometry.new_tab_position.1, geometry.new_tab_position.3) {
        super::super::render::row_label(
            r,
            gc,
            size,
            scale,
            &sk,
            geometry.new_tab_position,
            language.pick("新标签位置", "New tab position"),
            "",
            sk.ink,
        );
        combobox_value(
            r,
            gc,
            geometry.new_tab_position,
            super::super::new_tab_position_label(view.new_tab_position, language),
            sk.accent,
        );
    }
    if visible(geometry.panel_resize.1, geometry.panel_resize.3) {
        // 开关本体在 quads pass；开启前的性能告知走确认框，这里的
        // label 只说清楚它管哪三条分界线。
        super::super::render::row_label(
            r,
            gc,
            size,
            scale,
            &sk,
            geometry.panel_resize,
            language.pick(
                "拖拽调节侧栏宽度（左侧栏 / 右抽屉；SSH 分界高度无需开关）",
                "Drag to resize panel widths (sidebar / drawer)",
            ),
            "",
            sk.ink,
        );
    }
    let (bx, by, _, bh) = geometry.cjk_bold;
    if visible(group_y(by), title_h) {
        super::super::render::section_title(
            r,
            gc,
            size,
            scale,
            &sk,
            bx,
            group_y(by),
            language.pick("文本渲染", "Text rendering"),
        );
    }
    if visible(by, bh) {
        super::super::render::row_label(
            r,
            gc,
            size,
            scale,
            &sk,
            geometry.cjk_bold,
            language.pick(
                "中文粗体只提亮不加粗（避免小字号下笔画发闷）",
                "Render CJK bold with regular glyphs (avoids muddy strokes)",
            ),
            "",
            sk.ink,
        );
    }
}