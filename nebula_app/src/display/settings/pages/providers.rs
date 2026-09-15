// Settings UI: Providers page (AI provider management).

use crate::display::color::Rgb;
use crate::display::ui::surface;
use crate::display::ui::text_field;
use crate::display::ui::theme::Skin;
use crate::display::ui::widgets;
use crate::display::ui::tokens;
use crate::display::{SettingsHit, SizeInfo, UiLanguage};
use crate::display::settings::geometry::SettingsGeometry;
use crate::display::settings::view::SettingsView;
use crate::display::settings::settings_toggle_slot;
use crate::renderer::ui::{Rgba, UiQuad};
use crate::renderer::{GlyphCache, Renderer};
use unicode_width::UnicodeWidthChar;

pub(crate) fn push_providers_quads(
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

    for index in 0..geometry.provider_row_count {
        let row = (
            geometry.provider_row0.0,
            geometry.provider_row0.1 + index as f32 * geometry.provider_row_h,
            geometry.provider_row0.2,
            geometry.provider_row_h,
        );
        let active = view
            .providers
            .get(index)
            .is_some_and(|provider| provider.id == view.active_provider_id);
        let hovered = view.hover == SettingsHit::ProviderRow(index)
            || view.hover == SettingsHit::ProviderEnableToggle(index);
        clip(
            quads,
            UiQuad::solid(
                row.0,
                row.1,
                row.2,
                row.3,
                tokens::radius::CONTROL * scale,
                if active {
                    sk.accent_soft
                } else if hovered {
                    sk.hover
                } else {
                    sk.surface
                },
            ),
        );
    }
    for (index, field) in geometry.provider_fields.iter().enumerate() {
        let input_rect = super::super::sync_input_rect(*field, scale);
        let mut input = Vec::new();
        surface::push_input(
            &mut input,
            input_rect,
            scale,
            &sk,
            view.density,
            view.provider_focus == Some(index),
        );
        if view.provider_focus == Some(index) {
                    let max_cols = (((input_rect.2 - s(24.0)) / size.cell_width()) as usize).max(1);
                    let (display, placeholder, hidden) =
                        super::super::provider_input_display(view, index, max_cols);
                    text_field::push_cursor(
                        &mut input,
                        input_rect.1,
                        input_rect.3,
                        input_rect.0 + s(12.0),
                        if placeholder { "" } else { &display },
                        &view.provider_cursors[index].shifted(hidden),
                        size.cell_width(),
                        scale,
                        &sk,
                    );
                }
                for quad in input {
                    clip(quads, quad);
                }
            }
            let current =
                view.providers.iter().find(|provider| provider.id == view.active_provider_id);
            for (row, hit, on) in [
                (
                    geometry.provider_codex_goals,
                    SettingsHit::ProviderCodexGoalsToggle,
                    current.is_some_and(|provider| provider.codex_goals),
                ),
                (
                    geometry.provider_codex_remote,
                    SettingsHit::ProviderCodexRemoteToggle,
                    current.is_some_and(|provider| provider.codex_remote_compaction),
                ),
            ] {
                toggle(quads, &mut staged, row, hit, on, view.hover == hit, view.pressed == hit);
            }
            action_button(
                quads,
                geometry.provider_codex_apply,
                148.0,
                view.hover == SettingsHit::ProviderApplyCodex,
            );
            for (hit, rect) in [
                (SettingsHit::ProviderAdd, geometry.provider_add),
                (SettingsHit::ProviderSave, geometry.provider_save),
                (SettingsHit::ProviderTest, geometry.provider_test),
                (SettingsHit::ProviderDelete, geometry.provider_delete),
            ] {
                let mut button = Vec::new();
                widgets::push_outline_button(&mut button, rect, scale, &sk, view.hover == hit);
                for quad in button {
                    clip(quads, quad);
                }
            }
            quads.extend(staged.drain(..));
        }
        
        pub(crate) fn draw_providers_text(
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
            let (list_x, list_y, _, _) = geometry.provider_row0;
            if visible(group_y(list_y), title_h) {
                super::super::render::section_title(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    list_x,
                    group_y(list_y),
                    language.pick("AI 供应商", "AI providers"),
                );
            }
            if visible(geometry.provider_add.1, geometry.provider_add.3) {
                super::super::render::draw_button_label(
                    r,
                    gc,
                    size,
                    geometry.provider_add,
                    language.pick("+ 添加", "+ Add"),
                    if view.hover == SettingsHit::ProviderAdd { sk.accent } else { sk.ink },
                );
            }
            for (index, provider) in view.providers.iter().enumerate() {
                let row = (
                    geometry.provider_row0.0,
                    geometry.provider_row0.1 + index as f32 * geometry.provider_row_h,
                    geometry.provider_row0.2,
                    geometry.provider_row_h,
                );
                if !visible(row.1, row.3) {
                    continue;
                }
                let active = provider.id == view.active_provider_id;
                let value = if provider.model.is_empty() {
                    provider.kind.label()
                } else {
                    provider.model.as_str()
                };
                super::super::render::row_label_with_right_inset(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    row,
                    &provider.name,
                    value,
                    if active { sk.accent } else { sk.ink_dim },
                    s(76.0),
                );
                let enabled = provider.enabled;
                let state = if enabled {
                    language.pick("启用", "On")
                } else {
                    language.pick("关闭", "Off")
                };
                let cols = state.chars().map(|ch| ch.width().unwrap_or(1)).sum::<usize>();
                r.draw_chrome_text(
                    size,
                    row.0 + row.2 - s(16.0) - cols as f32 * cell_w,
                    widgets::centered_y(row.1, row.3, cell_h),
                    if enabled { sk.accent } else { sk.ink_dim },
                    state,
                    gc,
                );
            }

            for (index, row) in geometry.provider_fields.iter().enumerate() {
                if !visible(row.1, row.3) {
                    continue;
                }
                let label = match index {
                    0 => language.pick("供应商名称", "Provider name"),
                    1 => language.pick("备注", "Note"),
                    2 => language.pick("官网链接", "Website"),
                    3 => language.pick("API 请求地址", "API endpoint"),
                    4 => language.pick("默认模型", "Default model"),
                    _ => "API Key",
                };
                super::super::render::row_label(r, gc, size, scale, &sk, *row, label, "", sk.ink);
                let input = super::super::sync_input_rect(*row, scale);
                let max_cols = ((input.2 - s(24.0)).max(cell_w) / cell_w).floor() as usize;
                let (value, placeholder, _) = super::super::provider_input_display(view, index, max_cols.max(1));
                r.draw_chrome_text(
                    size,
                    input.0 + s(12.0),
                    widgets::centered_y(input.1, input.3, cell_h),
                    if placeholder { sk.ink_dim } else { sk.ink },
                    &value,
                    gc,
                );
            }
            for (row, label, detail) in [
                (
                    geometry.provider_codex_goals,
                    "Codex Goal mode",
                    language.pick("写入 features.goals", "Writes features.goals"),
                ),
                (
                    geometry.provider_codex_remote,
                    language.pick("Codex 远程压缩", "Codex remote compaction"),
                    language.pick(
                        "写入 features.remote_compaction_v2",
                        "Writes features.remote_compaction_v2",
                    ),
                ),
            ] {
                if visible(row.1, row.3) {
                    super::super::render::row_label(r, gc, size, scale, &sk, row, label, detail, sk.ink_dim);
                }
            }
            if visible(geometry.provider_codex_apply.1, geometry.provider_codex_apply.3) {
                super::super::render::row_label_with_right_inset(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    geometry.provider_codex_apply,
                    language.pick("Codex 配置", "Codex configuration"),
                    language.pick("写入 auth.json / config.toml", "W rite auth.json / config.toml"),
                    sk.ink_dim,
                    s(164.0),
                );
                super::super::render::draw_button_label(
                    r,
                    gc,
                    size,
                    super::super::row_action_rect(geometry.provider_codex_apply, scale, 148.0),
                    language.pick("应用到 Codex", "Apply to Codex"),
                    if view.hover == SettingsHit::ProviderApplyCodex { sk.accent } else { sk.ink },
                );
            }
            for (hit, rect, label) in [
                (
                    SettingsHit::ProviderDelete,
                    geometry.provider_delete,
                    language.pick("删除", "Delete"),
                ),
                (
                    SettingsHit::ProviderTest,
                    geometry.provider_test,
                    language.pick("测试连接", "Test"),
                ),
                (SettingsHit::ProviderSave, geometry.provider_save, language.pick("保存", "Save")),
            ] {
                if visible(rect.1, rect.3) {
                    super::super::render::draw_button_label(
                        r,
                        gc,
                        size,
                        rect,
                        label,
                        if view.hover == hit { sk.accent } else { sk.ink },
                    );
                }
            }
            if let Some((message, is_error)) = &view.provider_status {
                let y = geometry.provider_save.1 + geometry.provider_save.3 + s(8.0);
                if visible(y, cell_h) {
                    let max_cols = (geometry.provider_row0.2 / cell_w).floor().max(1.0) as usize;
                    let message = super::super::truncate_tab_label(message, max_cols);
                    r.draw_chrome_text(
                        size,
                        geometry.provider_delete.0,
                        y,
                        if *is_error {
                            Rgb::new(sk.danger.r, sk.danger.g, sk.danger.b)
                        } else {
                            sk.accent
                        },
                        &message,
                        gc,
                    );
                }
            }
        }