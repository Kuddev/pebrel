//! GPUI 壳的 OSC 8 / 正则 hint 接线：虚线下划线、悬停预览、Ctrl+点击打开。
//!
//! 匹配与动作全部复用旧壳 `display::hint` + `file_uri` + `daemon`，这里只做
//! 视口坐标、GPUI 修饰键和打开入口。

use std::collections::HashSet;
use std::sync::Arc;

use gpui::{ClipboardItem, Context, Window};
use nebula_terminal::event::EventListener;
use nebula_terminal::index::Point;
use nebula_terminal::term::{Term, point_to_viewport_from};
use unicode_width::UnicodeWidthChar;
use winit::keyboard::ModifiersState;

use crate::config::UiConfig;
use crate::config::ui_config::{HintAction, HintInternalAction};
use crate::display::hint::{self, HintMatch};

/// 悬停目标：旧壳 `highlighted_hint` + 已经解码好的预览文案。
#[derive(Clone)]
pub(super) struct LinkHover {
    pub hint: HintMatch,
    pub preview: String,
    pub anchor_row: u16,
    pub anchor_col: u16,
}

pub(super) fn hint_config() -> Arc<UiConfig> {
    Arc::new(UiConfig::default())
}

pub(super) fn winit_mouse_mods(mods: &gpui::Modifiers) -> ModifiersState {
    let mut state = ModifiersState::empty();
    if mods.shift {
        state |= ModifiersState::SHIFT;
    }
    if mods.control {
        state |= ModifiersState::CONTROL;
    }
    if mods.alt {
        state |= ModifiersState::ALT;
    }
    if mods.platform {
        state |= ModifiersState::SUPER;
    }
    state
}

/// 可见可点范围（OSC 8 + 正则 URL）映射到当前视口格子，供虚线下划线使用。
pub(super) fn dashed_cells<T: EventListener>(
    term: &Term<T>,
    config: &UiConfig,
    rows: usize,
    cols: usize,
) -> HashSet<(u16, u16)> {
    let matches = hint::visible_clickable_matches(term, config);
    if matches.is_empty() {
        return HashSet::new();
    }
    let origin = term.viewport_origin_for(rows);
    let mut cells = HashSet::new();
    for indexed in term.grid().display_iter() {
        if !matches.iter().any(|bounds| bounds.contains(&indexed.point)) {
            continue;
        }
        let Some(vp) = point_to_viewport_from(origin, indexed.point) else { continue };
        if vp.line < rows && vp.column.0 < cols {
            cells.insert((vp.line as u16, vp.column.0 as u16));
        }
    }
    cells
}

pub(super) fn highlighted_at<T: EventListener>(
    term: &Term<T>,
    config: &UiConfig,
    point: Point,
    mods: &gpui::Modifiers,
) -> Option<HintMatch> {
    hint::highlighted_at(term, config, point, winit_mouse_mods(mods))
}

pub(super) fn hover_from_hint<T: EventListener>(
    term: &Term<T>,
    hint: HintMatch,
    rows: usize,
    cols: usize,
) -> Option<LinkHover> {
    let raw = hint
        .hyperlink()
        .map(|link| link.uri().to_owned())
        .or_else(|| hint.text(term).map(|text| text.into_owned()))?;
    let uri = crate::file_uri::extract_link_target(&raw);
    let origin = term.viewport_origin_for(rows);
    let start = *hint.bounds().start();
    let vp =
        point_to_viewport_from(origin, start).filter(|vp| vp.line < rows && vp.column.0 < cols);
    let (anchor_row, anchor_col) =
        vp.map(|vp| (vp.line as u16, vp.column.0 as u16)).unwrap_or((0, 0));
    const HINT: &str = " · Ctrl+点击";
    let width = |s: &str| -> usize { s.chars().map(|c| c.width().unwrap_or(0)).sum() };
    let target = crate::display::strip_file_scheme(uri);
    let budget = cols.saturating_sub(width(HINT) + 1);
    let target = crate::display::fit_tail(&target, budget);
    Some(LinkHover { hint, preview: format!("{target}{HINT}"), anchor_row, anchor_col })
}

pub(super) fn open_hint_match(
    hint: &HintMatch,
    text: &str,
    cwd: Option<&std::path::Path>,
    window: &mut Window,
    cx: &mut Context<super::view::TerminalView>,
) {
    let language = crate::gpui_shell::config::ui_language(cx);
    if let Some(result) = crate::file_uri::try_open_local_link_with_cwd(text, cwd) {
        if let Err(err) = result {
            crate::gpui_shell::toast::toast(
                window,
                cx,
                crate::display::ToastKind::Warning,
                err.localized_message(language),
            );
        }
        return;
    }
    let target = crate::file_uri::extract_link_target(text);
    if hint.hyperlink().is_some() || crate::file_uri::is_web_or_protocol_uri(target) {
        match hint.action() {
            HintAction::Command(command) => {
                let mut args = command.args().to_vec();
                args.push(target.to_string());
                if let Err(err) = crate::daemon::spawn_detached(command.program(), &args) {
                    let err_str = err.to_string();
                    crate::gpui_shell::toast::toast(
                        window,
                        cx,
                        crate::display::ToastKind::Warning,
                        language.format(
                            crate::i18n::Message::CommonLinkOpenUrlFailed,
                            &[("error", &err_str)],
                        ),
                    );
                }
            },
            HintAction::Action(HintInternalAction::Copy) => {
                cx.write_to_clipboard(ClipboardItem::new_string(target.to_string()));
            },
            HintAction::Action(
                HintInternalAction::Paste
                | HintInternalAction::Select
                | HintInternalAction::MoveViModeCursor,
            ) => {},
        }
    } else {
        crate::gpui_shell::toast::toast(
            window,
            cx,
            crate::display::ToastKind::Warning,
            language.format(
                crate::i18n::Message::CommonLinkUnrecognized,
                &[("target", target)],
            ),
        );
    }
}
