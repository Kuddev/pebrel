//! GPUI 壳的 OSC 8 / 正则 hint 接线：虚线下划线、悬停预览、平台修饰键+点击打开。
//!
//! 匹配与动作全部复用旧壳 `display::hint` + `file_uri` + `daemon`，这里只做
//! 视口坐标、GPUI 修饰键和打开入口。

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{App, ClipboardItem, Window};
use nebula_terminal::event::EventListener;
use nebula_terminal::index::Point;
use nebula_terminal::term::cell::{Cell, Flags};
use nebula_terminal::term::{Term, point_to_viewport_from};
use nebula_terminal::tty::REMOTE_CLAUDE_CHIP_GLYPH;
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

/// 注入提示符里 "ssh" 标签用的内部链接 scheme：
/// `pebrel-ssh://<base64url(本机目录 UTF-8)>`。
///
/// 载荷由 `nebula_terminal` 的 PowerShell 提示符生成（目录可能含空格与
/// 非 ASCII，所以走 base64url 而不是百分号转义）；只有 Pebrel 自己消费它，
/// 别的终端里它只是普通文字，不会触发任何外部打开动作。
pub(super) const REMOTE_CLAUDE_SCHEME: &str = "pebrel-ssh://";

/// 解析 `pebrel-ssh://` 链接里的本机目录；不是该 scheme 或载荷坏了返回 `None`。
pub(super) fn remote_claude_target(uri: &str) -> Option<String> {
    use base64::Engine as _;

    let payload = uri.strip_prefix(REMOTE_CLAUDE_SCHEME)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).ok()?;
    let target = String::from_utf8(bytes).ok()?;
    (!target.trim().is_empty()).then_some(target)
}

/// 鼠标下的 `pebrel-ssh://` 链接指向的本机目录（不需要任何修饰键）。
pub(super) fn remote_claude_at<T: EventListener>(term: &Term<T>, point: Point) -> Option<String> {
    let (hyperlink, _) = hint::hyperlink_at(term, point)?;
    remote_claude_target(hyperlink.uri())
}

/// 悬停状态：内部链接不走"修饰键 + 正则命中"那套判定，普通悬停即可预览。
pub(super) fn remote_claude_hint_at<T: EventListener>(
    term: &Term<T>,
    config: &UiConfig,
    point: Point,
) -> Option<HintMatch> {
    let (hyperlink, bounds) = hint::hyperlink_at(term, point)?;
    remote_claude_target(hyperlink.uri())?;
    let trigger = config.hints.enabled.first()?.clone();
    Some(HintMatch::for_hyperlink(bounds, hyperlink, trigger))
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

/// 提示符的两类按格装饰：外部链接的虚线下划线格子，以及 ssh 标签里那格要换成
/// Claude 品牌图的图标。一次网格遍历同时取回，绘制帧不做第二遍。
#[derive(Default)]
pub(super) struct LinkDecorations {
    pub dashed: LinkCells,
    /// (视口行, 视口列)：宿主在这格上画品牌图（见 `claude_chip`）。
    pub chip_icons: Vec<(u16, u16)>,
}

/// 目标格的颜色（含 INVERSE 交换），虚线装饰与图标格共用同一份判定。
fn link_cell(cell: &Cell) -> LinkCell {
    let (mut fg, mut bg) = (cell.fg, cell.bg);
    if cell.flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    LinkCell { fg, bg, bold: cell.flags.contains(Flags::BOLD) }
}

pub(super) fn link_decorations<T: EventListener>(
    term: &Term<T>,
    config: &UiConfig,
    rows: usize,
    cols: usize,
) -> LinkDecorations {
    let matches = hint::visible_clickable_matches(term, config);
    if matches.is_empty() {
        return LinkDecorations::default();
    }
    let origin = term.viewport_origin_for(rows);
    let mut decorations = LinkDecorations::default();
    for indexed in term.grid().display_iter() {
        if indexed.flags.intersects(Flags::HIDDEN | Flags::LEADING_WIDE_CHAR_SPACER)
            || !matches.iter().any(|bounds| bounds.contains(&indexed.point))
        {
            continue;
        }
        // 提示符里的 ssh 标签自带图标与 "ssh" 文字：它不是外部链接，不该再叠
        // 一条虚线下划线（那条装饰是"Ctrl+点击打开外部目标"的信号）；图标那一格
        // 交给宿主画品牌图，其余格子保持文字。
        if indexed.hyperlink().is_some_and(|link| remote_claude_target(link.uri()).is_some()) {
            if indexed.cell.c == REMOTE_CLAUDE_CHIP_GLYPH
                && let Some(vp) = point_to_viewport_from(origin, indexed.point)
                && vp.line < rows
                && vp.column.0 < cols
            {
                decorations.chip_icons.push((vp.line as u16, vp.column.0 as u16));
            }
            continue;
        }
        let Some(vp) = point_to_viewport_from(origin, indexed.point) else { continue };
        if vp.line < rows && vp.column.0 < cols {
            decorations
                .dashed
                .insert((vp.line as u16, vp.column.0 as u16), link_cell(indexed.cell));
        }
    }
    decorations
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
    let origin = term.viewport_origin_for(rows);
    let start = *hint.bounds().start();
    let vp =
        point_to_viewport_from(origin, start).filter(|vp| vp.line < rows && vp.column.0 < cols);
    let (anchor_row, anchor_col) =
        vp.map(|vp| (vp.line as u16, vp.column.0 as u16)).unwrap_or((0, 0));
    // 内部链接没有可跳转的地址：预览只说明点下去会发生什么（单击，不需要
    // Ctrl/Command——它不是"外部打开"类手势）。
    if remote_claude_target(&raw).is_some() {
        return Some(LinkHover {
            hint,
            preview: language.text(Message::RemoteClaudePromptHint).to_owned(),
            anchor_row,
            anchor_col,
        });
    }
    let uri = crate::file_uri::extract_link_target(&raw);
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
    fn remote_claude_links_decode_the_local_directory() {
        use base64::Engine as _;

        let cwd = r"E:\work\proj 带空格";
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(cwd);
        let uri = format!("{REMOTE_CLAUDE_SCHEME}{payload}");
        assert_eq!(remote_claude_target(&uri).as_deref(), Some(cwd));
        // 其它 scheme、坏载荷与空目录都不是内部链接。
        assert!(remote_claude_target("https://example.com").is_none());
        assert!(remote_claude_target("pebrel-ssh://%%%").is_none());
        assert!(remote_claude_target(REMOTE_CLAUDE_SCHEME).is_none());
    }

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
