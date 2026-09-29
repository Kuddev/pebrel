//! GPUI 壳的 OSC 8 / 正则 hint 接线：虚线下划线、悬停预览、平台修饰键+点击打开。
//!
//! 匹配与动作全部复用旧壳 `display::hint` + `file_uri` + `daemon`，这里只做
//! 视口坐标、GPUI 修饰键和打开入口。

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{App, ClipboardItem, Window};
use nebula_terminal::event::EventListener;
use nebula_terminal::index::Point;
use nebula_terminal::term::cell::Flags;
use nebula_terminal::term::{Term, point_to_viewport_from};
use nebula_terminal::vte::ansi::Color;
use unicode_width::UnicodeWidthChar;
use winit::keyboard::ModifiersState;

use crate::config::UiConfig;
use crate::config::ui_config::{HintAction, HintInternalAction, default_hint_command};
use crate::display::hint::{self, HintMatch};
use crate::i18n::{Message, UiLanguage};
use crate::platform::Platform;

pub(super) fn link_modifier(mods: &gpui::Modifiers) -> bool {
    match Platform::current() {
        Platform::MacOS => mods.platform,
        Platform::Windows | Platform::Linux => mods.control,
    }
}

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
pub(super) struct LinkCell {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
}

pub(super) type LinkCells = HashMap<(u16, u16), LinkCell>;

pub(super) fn dashed_cells<T: EventListener>(
    term: &Term<T>,
    config: &UiConfig,
    rows: usize,
    cols: usize,
) -> LinkCells {
    let matches = hint::visible_clickable_matches(term, config);
    if matches.is_empty() {
        return HashMap::new();
    }
    let origin = term.viewport_origin_for(rows);
    let mut cells = HashMap::new();
    for indexed in term.grid().display_iter() {
        if indexed.flags.intersects(Flags::HIDDEN | Flags::LEADING_WIDE_CHAR_SPACER)
            || !matches.iter().any(|bounds| bounds.contains(&indexed.point))
        {
            continue;
        }
        let Some(vp) = point_to_viewport_from(origin, indexed.point) else { continue };
        if vp.line < rows && vp.column.0 < cols {
            let (mut fg, mut bg) = (indexed.fg, indexed.bg);
            if indexed.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            cells.insert(
                (vp.line as u16, vp.column.0 as u16),
                LinkCell { fg, bg, bold: indexed.flags.contains(Flags::BOLD) },
            );
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
    hint::highlighted_at_with_mouse_override(
        term,
        config,
        point,
        winit_mouse_mods(mods),
        link_modifier(mods),
    )
}

pub(super) fn hover_from_hint<T: EventListener>(
    term: &Term<T>,
    hint: HintMatch,
    rows: usize,
    cols: usize,
    language: UiLanguage,
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
    let gesture = language.text(match Platform::current() {
        Platform::MacOS => Message::CommonLinkCommandClick,
        Platform::Windows | Platform::Linux => Message::CommonLinkCtrlClick,
    });
    let width = |s: &str| -> usize { s.chars().map(|c| c.width().unwrap_or(0)).sum() };
    let target = crate::display::strip_file_scheme(uri);
    let budget = cols.saturating_sub(width(gesture) + width(" · ") + 1);
    let target = crate::display::fit_tail(&target, budget);
    Some(LinkHover { hint, preview: format!("{target} · {gesture}"), anchor_row, anchor_col })
}

pub(super) fn open_hint_match(
    hint: &HintMatch,
    text: &str,
    cwd: Option<&std::path::Path>,
    wsl_distro: Option<&str>,
    window: &mut Window,
    cx: &mut App,
) {
    let target = (hint.hyperlink().is_none()
        && hint.action() == &HintAction::Command(default_hint_command()))
        .then(|| wsl_absolute_target(text, wsl_distro))
        .flatten();
    let text = target.as_deref().unwrap_or(text);
    dispatch_hint_action(hint.action(), hint.hyperlink().is_some(), text, cwd, window, cx);
}

/// `wsl_distro` is the pane's spawn-time snapshot, so bare `wsl` and a WSL
/// default shell resolve prompt paths in the distribution they actually run.
fn wsl_absolute_target(text: &str, wsl_distro: Option<&str>) -> Option<String> {
    if !text.starts_with('/') || text.starts_with("//") {
        return None;
    }
    let distro = wsl_distro?;
    Some(crate::shell_detect::wsl_unc_path(distro, text).to_string_lossy().into_owned())
}

/// Base for relative link targets. A WSL pane's guest cwd maps into its
/// distribution like an absolute prompt path; the opener resolves it off the UI
/// thread. Only other panes fall back to the host-visible cwd, since Windows
/// would resolve a guest `/home/x` against the current drive.
pub(super) fn link_base_directory(
    cwd: &str,
    wsl_distro: Option<&str>,
    host_cwd: impl FnOnce() -> Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    match (wsl_distro, crate::shell_detect::wsl_guest_cwd(cwd)) {
        (Some(distro), Some(guest)) => Some(crate::shell_detect::wsl_unc_path(distro, guest)),
        _ => host_cwd(),
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn absolute_prompt_paths_use_the_owning_wsl_distribution() {
        let distro = Some("Debian");
        assert_eq!(
            wsl_absolute_target("/mnt/d/project", distro).as_deref(),
            Some(r"\\wsl.localhost\Debian\mnt\d\project")
        );
        assert!(wsl_absolute_target("//example.com/file", distro).is_none());
        assert!(wsl_absolute_target("https://example.com", distro).is_none());
        // SSH and host panes have no WSL snapshot.
        assert!(wsl_absolute_target("/home/user", None).is_none());
    }

    #[test]
    fn relative_prompt_paths_resolve_in_the_guest_cwd() {
        let host = || Some(std::path::PathBuf::from(r"D:\host"));
        assert_eq!(
            link_base_directory("/home/dev/app", Some("Debian"), || panic!("no host probe")),
            Some(std::path::PathBuf::from(r"\\wsl.localhost\Debian\home\dev\app"))
        );
        // A host-form cwd (before the first guest report) and host panes keep the host path.
        assert_eq!(link_base_directory(r"D:\host", Some("Debian"), host), host());
        assert_eq!(link_base_directory("/home/dev", None, host), host());
    }
}

fn dispatch_hint_action(
    action: &HintAction,
    hyperlink: bool,
    text: &str,
    cwd: Option<&std::path::Path>,
    window: &mut Window,
    cx: &mut App,
) {
    // Internal actions operate on the original match, without filesystem work.
    let command = match action {
        HintAction::Action(HintInternalAction::Copy) => {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
            return;
        },
        HintAction::Action(_) => return,
        HintAction::Command(command) => command.clone(),
    };
    let text = text.to_owned();
    let cwd = cwd.map(std::path::Path::to_path_buf);
    let language = crate::gpui_shell::config::ui_language(cx);
    let task = cx.background_executor().spawn(async move {
        let target = if command == default_hint_command() {
            if let Some(result) =
                crate::file_uri::try_open_local_link_with_cwd(&text, cwd.as_deref())
            {
                return result.map_err(|error| error.localized_message(language));
            }
            let target = crate::file_uri::extract_link_target(&text);
            if !hyperlink && !crate::file_uri::is_web_or_protocol_uri(target) {
                return Err(language
                    .format(crate::i18n::Message::CommonLinkUnrecognized, &[("target", target)]));
            }
            target
        } else {
            &text
        };
        let mut args = command.args().to_vec();
        args.push(target.to_owned());
        crate::daemon::spawn_detached(command.program(), &args).map_err(|error| {
            language.format(
                crate::i18n::Message::CommonLinkOpenUrlFailed,
                &[("error", &error.to_string())],
            )
        })
    });
    window
        .spawn(cx, async move |cx| {
            if let Err(message) = task.await {
                let _ = cx.update(|window, cx| {
                    crate::gpui_shell::toast::toast(
                        window,
                        cx,
                        crate::display::ToastKind::Warning,
                        message,
                    );
                });
            }
        })
        .detach();
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use gpui::{Context, IntoElement, Render, TestAppContext};

    struct Surface;
    impl Render for Surface {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            gpui::div()
        }
    }

    #[gpui::test]
    fn copying_local_markdown_and_custom_matches_never_opens_them(cx: &mut TestAppContext) {
        let (_, window) = cx.add_window_view(|_, _| Surface);
        for text in [
            "[notes](./missing notes.md)",
            "file:///C:/missing-copy-test.md",
            r"\\server\share\copy-only.md",
            "custom match without a URL",
            "[site](https://example.com)",
        ] {
            window.update(|window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string("before".into()));
                dispatch_hint_action(
                    &HintAction::Action(HintInternalAction::Copy),
                    false,
                    text,
                    None,
                    window,
                    cx,
                );
                assert_eq!(
                    cx.read_from_clipboard().and_then(|item| item.text()).as_deref(),
                    Some(text)
                );
            });
        }
        cx.run_until_parked();
    }
}
