// ---- rendering ----

use crate::display::color::Rgb;
use crate::display::network_proxy_model::{ManualProxyProtocol, ProxyTestStatus};
use crate::display::ui::text_field;
use crate::display::ui::widgets;
use crate::display::{
    contains_rect, keymap, AcceptKey, CompletionStyle, LanguagePreference, NebulaShell,
    NebulaSettingsSection, NebulaTheme, SettingsHit, SizeInfo, UiLanguage,
};
use crate::encrypted_backup::BackupSelection;
use crate::renderer::image::{BackgroundImageAlignment, BackgroundImageFit};
use crate::renderer::{GlyphCache, Renderer};
use nebula_terminal::vte::ansi::CursorShape;
use unicode_width::UnicodeWidthChar;

use super::{
    CellWidthMode, KeymapPaneState, NewTabPosition, ProxyChoice, ProxyPaneState, SettingsDropdown,
    SettingsOpacityTarget, TabRevealMotion, SETTINGS_TOGGLE_COUNT,
    ACCEPT_OPTIONS, BACKUP_PROTOCOL_OPTIONS, BACKGROUND_ALIGNMENT_OPTIONS,
    BACKGROUND_FIT_OPTIONS, CELL_WIDTH_MODE_OPTIONS, COMPLETION_STYLE_OPTIONS,
    CURSOR_SHAPE_OPTIONS, DENSITY_OPTIONS, LANGUAGE_OPTIONS, MANUAL_PROXY_PROTOCOL_OPTIONS,
    NEW_TAB_POSITION_OPTIONS, SSH_PROXY_MODE_OPTIONS, TAB_REVEAL_OPTIONS, BACKGROUND_SWATCHES,
};
use super::geometry::SettingsGeometry;
// ---- rendering ----

/// Renderer-owned snapshot for one SSH destination. Keeping this small model
/// separate from `Display` means the settings page never reaches into runtime
/// collections while drawing, and the same host ordering can be reused by
/// the sidebar and command palette without UI-specific branching.
pub(crate) struct SshSettingsHost {
    pub(crate) destination: String,
    pub(crate) label: String,
    pub(crate) icon: String,
    pub(crate) pinned: bool,
}

/// A per-frame snapshot of the display state the settings render reads. Owns its
/// data (notably the wallpaper path) so the caller can hand it in by reference
/// while still borrowing `&mut renderer` for [`draw_text`].
pub(crate) struct SettingsView {
    /// The active tab's content card in physical pixels. Settings fills this
    /// area like any other tab instead of inventing a second floating window.
    pub(crate) area: (f32, f32, f32, f32),
    pub(crate) language_preference: LanguagePreference,
    pub(crate) language: UiLanguage,
    pub(crate) section: NebulaSettingsSection,
    pub(crate) hover: SettingsHit,
    /// Settings control currently held by the primary mouse button. This is
    /// separate from hover so toggles can reproduce the HTML reference's
    /// pressed stretch without making every row a click target.
    pub(crate) pressed: SettingsHit,
    /// Independent travel/stretch/color/hover channels for every switch.
    pub(crate) toggle_motion: [widgets::ToggleMotion; SETTINGS_TOGGLE_COUNT],
    pub(crate) theme: NebulaTheme,
    pub(crate) follow_system_theme: bool,
    pub(crate) ghost: bool,
    pub(crate) accept: AcceptKey,
    pub(crate) completion_style: CompletionStyle,
    /// Pre-rendered "默认 Shell" value (icon + name) — resolved by `Display`
    /// from the rich `shell_id` when set, else the 2-value enum label.
    pub(crate) shell_label: String,
    /// Which combobox is expanded, if any (floating option list).
    pub(crate) dropdown: Option<SettingsDropdown>,
    /// Detected shells for the picker (cached once per process).
    pub(crate) shells: Vec<(String, String, String)>, // (id, name, program)
    pub(crate) shell_id: Option<String>,
    pub(crate) startup_directory: Option<String>,
    pub(crate) providers: Vec<crate::ai_providers::AiProvider>,
    pub(crate) active_provider_id: String,
    pub(crate) provider_inputs: [String; 6],
    pub(crate) provider_cursors: [text_field::TextCursor; 6],
    pub(crate) provider_focus: Option<usize>,
    pub(crate) provider_status: Option<(String, bool)>,
    pub(crate) font_family: String,
    /// Current terminal font size in LOGICAL px, for the spinner value box.
    pub(crate) font_size_px: f32,
    /// Private families plus Maple; the import action is rendered separately.
    pub(crate) fonts: Vec<String>,
    pub(crate) font_notice: Option<String>,
    /// 字体目录的「显示全部」临时过滤是否开启。
    pub(crate) font_show_all: bool,
    /// 字体目录搜索串；长在弹层顶部那个搜索框里。
    pub(crate) font_query: String,
    /// 搜索框的光标与选区。下沉到 [`super::ui::text_field`] 的同一套模型，
    /// 新加的输入框直接继承，不必再实现一遍。
    pub(crate) font_query_cursor: text_field::TextCursor,
    /// 字体弹层的候选滚动偏移；搜索框占第 0 行，滚动只移动其余候选。
    pub(crate) font_popup_scroll: usize,
    /// 字体弹层滚动条是否正被拖拽（thumb 高亮用）。
    pub(crate) font_popup_dragging: bool,
    /// 非等宽族的小写名集合；下拉行据此追加比例字体警告。
    pub(crate) font_proportional: std::collections::HashSet<String>,
    /// Persistent soft-deleted destinations. Rows provide a discoverable
    /// recovery path after the short Undo bar has expired.
    pub(crate) hidden_hosts: Vec<String>,
    /// SSH destinations copied from the sidebar's merged, ordered snapshot.
    pub(crate) ssh_hosts: Vec<SshSettingsHost>,
    pub(crate) fetch: bool,
    pub(crate) powerline: bool,
    pub(crate) keep_session: bool,
    /// 高级·会话：启动时恢复上次的标签。
    pub(crate) restore_session: bool,
    /// 高级·会话：冷恢复自动接续 AI 对话。
    pub(crate) resume_ai: bool,
    /// 高级：常驻系统托盘图标。
    pub(crate) tray: bool,
    pub(crate) blur: bool,
    pub(crate) opacity: f32,
    /// Which opacity slider is mid-drag, for thumb-dot grow feedback.
    pub(crate) dragging_opacity: Option<SettingsOpacityTarget>,
    pub(crate) cursor_shape: CursorShape,
    pub(crate) cursor_blink: bool,
    pub(crate) copy_on_select: bool,
    /// 交互·「拖拽调节侧栏」总开关。
    pub(crate) panel_resize: bool,
    pub(crate) cjk_bold_regular: bool,
    pub(crate) tab_reveal: TabRevealMotion,
    pub(crate) density: crate::display::ui::tokens::Density,
    pub(crate) new_tab_position: NewTabPosition,
    pub(crate) cell_width_mode: CellWidthMode,
    /// Live-preview colors: the ACTUAL terminal background/foreground the
    /// grid would use right now (custom background wins over the theme).
    pub(crate) preview_bg: Rgb,
    pub(crate) preview_fg: Rgb,
    pub(crate) background: Option<Rgb>,
    /// 背景色浮层的 16 进制草稿（形如 `#0A0C18`）与输入聚焦态。
    pub(crate) bg_hex_input: String,
    pub(crate) bg_hex_active: bool,
    /// 调色盘草稿 HSV（打开浮层时从生效色初始化；拖动期间是唯一权威，
    /// 灰色/黑白下的色相不会因 RGB 往返而丢失）。
    pub(crate) bg_picker_hsv: (f32, f32, f32),
    pub(crate) background_image: Option<String>,
    pub(crate) background_image_opacity: f32,
    pub(crate) background_image_fit: BackgroundImageFit,
    pub(crate) background_image_alignment: BackgroundImageAlignment,
    pub(crate) background_image_cover_chrome: bool,
    /// Content scroll offset in scaled px (0 = top). Owned by `Display`,
    /// clamped there against [`settings_max_scroll`].
    pub(crate) scroll: f32,
    /// 按键映射: per editable row `(display combo, customized)`; `None` =
    /// unbound. Precomputed by `Display` from the override + default tables.
    pub(crate) keymap: Vec<Option<(String, bool)>>,
    /// 快速终端行使用独立的全局快捷键字符串。
    pub(crate) quick_terminal_hotkey: String,
    pub(crate) quick_hotkey_error: Option<String>,
    /// Row currently capturing a new combo, if any.
    pub(crate) keymap_capture: Option<usize>,
    /// 捕获态按住的修饰键前缀（"Ctrl+"），实时回显。
    pub(crate) keymap_capture_preview: String,
    /// 按键映射页搜索：查询串 + 聚焦态；过滤后可见行的 flat 下标
    /// （编辑组 / 只读组分开），由 Display 用同一过滤谓词预计算。
    pub(crate) keymap_query: String,
    pub(crate) keymap_query_cursor: text_field::TextCursor,
    pub(crate) keymap_search_focus: bool,
    pub(crate) keymap_visible: Vec<usize>,
    pub(crate) keymap_readonly_visible: Vec<usize>,
    /// 与 flat 行对齐的冲突标记 + 预排好的冲突提示句（None = 无冲突）。
    pub(crate) keymap_clash_rows: Vec<bool>,
    pub(crate) keymap_clash_note: Option<String>,
    /// 高级→同步：四个输入草稿（url、用户名、WebDAV 密码、E2E 口令）。
    pub(crate) sync_inputs: [String; 4],
    /// 聚焦的同步输入框下标（0..4）。
    pub(crate) sync_focus: Option<usize>,
    pub(crate) sync_auto_pull: bool,
    /// 凭据管理器里已有 [密码, 口令]，决定密码框占位文案。
    pub(crate) sync_secret_set: [bool; 2],
    /// 最近一次同步动作的结果 `(message, is_error)`。
    pub(crate) sync_status: Option<(String, bool)>,
    pub(crate) sync_busy: bool,
    /// 网络页：[手动地址, 绕过列表, 自定义命令] 输入原文 + 聚焦下标。
    pub(crate) ssh_proxy_mode: crate::ssh_proxy::ProxyMode,
    pub(crate) ssh_proxy_inputs: [String; 3],
    pub(crate) ssh_proxy_cursors: [text_field::TextCursor; 3],
    pub(crate) ssh_proxy_focus: Option<usize>,
    pub(crate) ssh_proxy_protocol: ManualProxyProtocol,
    pub(crate) ssh_proxy_choice: ProxyChoice,
    pub(crate) local_proxies: Vec<crate::ssh_proxy::LocalProxyEndpoint>,
    pub(crate) proxy_scanning: bool,
    /// 「跟随系统」当前读到的代理：`(URL, 来自注册表)`。None = 系统未启用。
    /// Display 在进网络页 / 切模式时刷新缓存；渲染只读，不做系统调用。
    pub(crate) system_proxy_probe: Option<(String, bool)>,
    /// 当前网络设置的最近一次真实出网测试结果。
    pub(crate) proxy_test_status: ProxyTestStatus,
    /// 每主机覆盖行：`(显示名, 摘要, ssh_hosts 下标)`，随视图快照重建。
    pub(crate) ssh_proxy_overrides: Vec<(String, String, usize)>,
    pub(crate) backup_selection: BackupSelection,
    pub(crate) backup_status: Option<(String, bool)>,
    /// 状态行画在触发动作的控件旁：true = 远程组下方，false = 清单下方。
    pub(crate) backup_status_remote: bool,
    /// 远程备份：协议、5 个输入槽草稿、聚焦槽位、密文凭据存在性、动作忙。
    pub(crate) backup_protocol: crate::backup_remote::BackupProtocol,
    pub(crate) backup_remote_inputs: [String; 5],
    pub(crate) backup_remote_focus: Option<usize>,
    pub(crate) backup_remote_secret_set: bool,
    pub(crate) backup_busy: bool,
}

/// 同步行右侧的输入框矩形（quad/text/hit 三处共用）。行左侧留给标签。
pub(crate) fn sync_input_rect((rx, ry, rw, rh): (f32, f32, f32, f32), scale: f32) -> (f32, f32, f32, f32) {
    let s = |v: f32| v * scale;
    if rh >= s(56.0) {
        return (rx + s(16.0), ry + rh - s(38.0), (rw - s(32.0)).max(s(1.0)), s(32.0));
    }
    let w = rw * 0.56;
    let h = rh - s(12.0);
    (rx + rw - s(16.0) - w, ry + (rh - h) / 2.0, w, h)
}

/// 原型的展开组件不带左侧行标签：跳板和命令占满可用宽度；手动填写把
/// 同一行拆成固定协议选择器与自适应地址输入框。
pub(crate) fn ssh_proxy_expand_control(
    (rx, ry, rw, rh): (f32, f32, f32, f32),
    scale: f32,
) -> (f32, f32, f32, f32) {
    let s = |v: f32| v * scale;
    let h = rh - s(12.0);
    (rx, ry + (rh - h) / 2.0, rw, h)
}

/// 网络代理三态文字很短，使用紧凑下拉，避免通用 220px 控件在这一行显得
/// 空旷。绘制、命中、弹层锚点和文字都复用这个矩形。
pub(crate) fn ssh_proxy_mode_control(
    (rx, ry, rw, rh): (f32, f32, f32, f32),
    scale: f32,
) -> (f32, f32, f32, f32) {
    let s = |v: f32| v * scale;
    if rh >= s(56.0) {
        return (rx + s(16.0), ry + rh - s(38.0), (rw - s(32.0)).max(s(1.0)), s(32.0));
    }
    let w = s(156.0).min(rw * 0.38).max(s(132.0));
    let h = s(32.0);
    (rx + rw - s(16.0) - w, ry + (rh - h) * 0.5, w, h)
}

/// 测试横幅右侧动作。横幅整块承载状态，只有这个按钮触发联网，避免用户
/// 点击错误文案时无意重复发起请求。
pub(crate) fn ssh_proxy_test_button(
    (rx, ry, rw, rh): (f32, f32, f32, f32),
    scale: f32,
) -> (f32, f32, f32, f32) {
    let s = |v: f32| v * scale;
    let w = s(108.0).min(rw * 0.34).max(s(88.0));
    let h = s(30.0);
    (rx + rw - s(12.0) - w, ry + (rh - h) * 0.5, w, h)
}

pub(crate) fn ssh_proxy_manual_controls(
    row: (f32, f32, f32, f32),
    scale: f32,
) -> ((f32, f32, f32, f32), (f32, f32, f32, f32)) {
    let s = |v: f32| v * scale;
    let (x, y, w, h) = sync_input_rect(row, scale);
    let gap = s(8.0);
    let protocol_w = s(112.0).min((w - gap) * 0.38);
    let protocol = (x, y, protocol_w, h);
    let address = (x + protocol_w + gap, y, (w - protocol_w - gap).max(s(80.0)), h);
    (protocol, address)
}

/// 同步输入框的展示内容：`(文本, 是否占位, 列数)`。密码/口令显示为
/// 掩码点；超宽时截尾部显示（编辑总发生在末尾）。列数供 caret 定位。
pub(crate) fn sync_input_display(view: &SettingsView, index: usize, max_cols: usize) -> (String, bool, usize) {
    let language = view.language;
    let raw = &view.sync_inputs[index];
    if raw.is_empty() {
        let text = match index {
            0 => language
                .pick("https://dav.example.com/nebula.sync", "https://dav.example.com/nebula.sync"),
            1 => language.pick("WebDAV 用户名", "WebDAV username"),
            2 if view.sync_secret_set[0] => {
                language.pick("已保存（输入以更换）", "Saved (type to replace)")
            },
            3 if view.sync_secret_set[1] => {
                language.pick("已保存（输入以更换）", "Saved (type to replace)")
            },
            _ => language.pick("未设置", "Not set"),
        };
        return (text.to_owned(), true, 0);
    }
    if index >= 2 {
        let dots = raw.chars().count().min(24);
        return ("●".repeat(dots), false, dots);
    }
    // 从尾部收集不超过 max_cols 列的字符（中文占 2 列）。
    let (text, cols) = text_tail(raw, max_cols);
    (text, false, cols)
}

/// 从尾部收集不超过 `max_cols` 列的字符（中文占 2 列）；编辑总发生在
/// 末尾，超宽时截头部。返回 `(展示文本, 实际列数)`，列数供 caret 定位。
pub(crate) fn text_tail(raw: &str, max_cols: usize) -> (String, usize) {
    let mut cols = 0usize;
    let mut chars: Vec<char> = Vec::new();
    for ch in raw.chars().rev() {
        let w = ch.width().unwrap_or(1).max(1);
        if cols + w > max_cols {
            break;
        }
        cols += w;
        chars.push(ch);
    }
    chars.reverse();
    (chars.into_iter().collect(), cols)
}

/// SSH 代理输入框的展示内容：`(文本, 是否占位, 列数)`。与
/// [`sync_input_display`] 同一契约，行矩形也共用 [`sync_input_rect`]。
/// 聚焦时窗口跟随光标：光标退进被截掉的头部时改从光标处向后开窗，
/// 保证 caret 永远落在可见列里。
pub(crate) fn ssh_proxy_input_display(
    view: &SettingsView,
    index: usize,
    max_cols: usize,
) -> (String, bool, usize, usize) {
    let raw = &view.ssh_proxy_inputs[index];
    if raw.is_empty() {
        let text = match index {
            // 无前缀地址就能用（自动按 socks5），placeholder 直接示范最短
            // 形态；System 模式下地址行整个不渲染，无需在此分支。
            0 => "127.0.0.1:7890",
            1 => view.language.pick("例：10.0.0.0, .internal", "e.g. 10.0.0.0, .internal"),
            _ => "corkscrew proxy.corp 8080 %h %p",
        };
        return (text.to_owned(), true, 0, 0);
    }
    if view.ssh_proxy_focus == Some(index) {
        let caret = view.ssh_proxy_cursors[index].caret(raw);
        let total = raw.chars().count();
        let (tail, cols) = text_tail(raw, max_cols);
        let hidden = total - tail.chars().count();
        if caret >= hidden {
            return (tail, false, cols, hidden);
        }
        // 光标在尾窗口之外：从光标处向后开窗（光标贴左缘）。
        let mut cols = 0usize;
        let mut text = String::new();
        for ch in raw.chars().skip(caret) {
            let w = ch.width().unwrap_or(1).max(1);
            if cols + w > max_cols {
                break;
            }
            cols += w;
            text.push(ch);
        }
        return (text, false, cols, caret);
    }
    let (text, cols) = text_tail(raw, max_cols);
    let hidden = raw.chars().count() - text.chars().count();
    (text, false, cols, hidden)
}

pub(crate) fn provider_input_display(
    view: &SettingsView,
    index: usize,
    max_cols: usize,
) -> (String, bool, usize) {
    let raw = &view.provider_inputs[index];
    if raw.is_empty() {
        let placeholder = if index == 5 {
            match view.providers.iter().find(|provider| provider.id == view.active_provider_id) {
                Some(provider) if !provider.kind.requires_api_key() => {
                    view.language.pick("本地服务无需 API Key", "No API key required").to_owned()
                },
                Some(provider) if provider.api_key_set => format!(
                    "{}  {}",
                    provider.api_key_hint,
                    view.language.pick("（输入以更换）", "(type to replace)")
                ),
                _ => view.language.pick("输入 API Key", "Enter API key").to_owned(),
            }
        } else {
            view.language.pick("未设置", "Not set").to_owned()
        };
        return (placeholder, true, 0);
    }

    let source = if index == 5 { "●".repeat(raw.chars().count()) } else { raw.clone() };
    let caret = view.provider_cursors[index].caret(raw);
    let total = source.chars().count();
    let (tail, _) = text_tail(&source, max_cols);
    let hidden = total - tail.chars().count();
    if view.provider_focus != Some(index) || caret >= hidden {
        return (tail, false, hidden);
    }
    let mut cols = 0usize;
    let mut display = String::new();
    for ch in source.chars().skip(caret) {
        let width = ch.width().unwrap_or(1).max(1);
        if cols + width > max_cols {
            break;
        }
        cols += width;
        display.push(ch);
    }
    (display, false, caret)
}

/// 视图 → 网络页几何输入。所有 settings_geometry 调用点共用同一份推导，
/// 防止命中与绘制对模式的理解不一致。
pub(crate) fn proxy_pane_state(view: &SettingsView) -> ProxyPaneState {
    ProxyPaneState {
        mode: view.ssh_proxy_mode,
        choice: view.ssh_proxy_choice,
        found_count: view.local_proxies.len(),
        scanning: view.proxy_scanning,
        override_count: view.ssh_proxy_overrides.len(),
    }
}

/// 视图 → 按键映射页几何输入（每组可见行数从 flat 下标反推）。
pub(crate) fn keymap_pane_state_view(view: &SettingsView) -> KeymapPaneState {
    let mut pane = KeymapPaneState {
        readonly_visible: view.keymap_readonly_visible.len() as u8,
        clash: view.keymap_clash_note.is_some(),
        ..Default::default()
    };
    let mut start = 0usize;
    for (group, (.., count)) in keymap::GROUPS.iter().enumerate() {
        let end = start + count;
        pane.visible[group] =
            view.keymap_visible.iter().filter(|flat| (start..end).contains(*flat)).count() as u8;
        start = end;
    }
    pane
}

/// 动作行的 [立即推送, 立即拉取] 按钮矩形。
pub(crate) fn sync_button_rects(
    (rx, ry, _, rh): (f32, f32, f32, f32),
    scale: f32,
) -> [(f32, f32, f32, f32); 2] {
    let s = |v: f32| v * scale;
    let w = s(150.0);
    let h = rh - s(12.0);
    let y = ry + (rh - h) / 2.0;
    [(rx, y, w, h), (rx + w + s(12.0), y, w, h)]
}

/// Export/restore segmented control from the backup prototype. The hit boxes
/// are the same inner slots that are painted, including the 3px outer inset.
pub(crate) fn backup_segment_rects(
    (rx, ry, rw, rh): (f32, f32, f32, f32),
    scale: f32,
) -> [(f32, f32, f32, f32); 2] {
    let inset = 3.0 * scale;
    let inner_w = (rw - inset * 2.0).max(0.0);
    let slot_w = inner_w * 0.5;
    [
        (rx + inset, ry + inset, slot_w, rh - inset * 2.0),
        (rx + inset + slot_w, ry + inset, slot_w, rh - inset * 2.0),
    ]
}

/// 远程备份动作行紧跟当前协议的最后一个可见输入行（字段数随协议变化，
/// 行位跟着上移）。hit / quad / text 三个 pass 共用，按钮与点击区不漂移。
/// 行距从几何自身推导（ROW_H 随密度变化，是 `settings_geometry` 的局部）。
pub(crate) fn backup_remote_actions_rect(
    geometry: &SettingsGeometry,
    scale: f32,
    field_count: usize,
) -> (f32, f32, f32, f32) {
    let (bx, _, bw, bh) = geometry.backup_remote_actions;
    let pitch = geometry.backup_remote_fields[1].1 - geometry.backup_remote_fields[0].1;
    let y = geometry.backup_remote_fields[0].1 + field_count as f32 * pitch + 12.0 * scale;
    (bx, y, bw, bh)
}

pub(crate) fn backup_item_selected(selection: BackupSelection, index: usize) -> bool {
    match index {
        0 => selection.appearance,
        1 => selection.config,
        2 => selection.ssh,
        3 => selection.sync,
        4 => selection.assistant,
        5 => selection.session,
        6 => selection.directory_history,
        7 => selection.command_history,
        _ => selection.fonts,
    }
}

/// 键位行的 keycap 矩形（quad 与 text 两个 pass 共用同一几何）。
pub(crate) fn keymap_keycap_rect(
    (rx, ry, rw, rh): (f32, f32, f32, f32),
    label: &str,
    cell_w: f32,
    scale: f32,
) -> (f32, f32, f32, f32) {
    let s = |v: f32| v * scale;
    let cols: usize = label.chars().map(|c| c.width().unwrap_or(0)).sum();
    let cap_w = cols as f32 * cell_w + s(24.0);
    let cap_h = rh - s(14.0);
    (rx + rw - s(16.0) - cap_w, ry + (rh - cap_h) / 2.0, cap_w, cap_h)
}

/// 键位行右侧的展示文本：(文本, 是否自定义, 是否有绑定)。捕获态的
/// 「按下新按键…」由调用侧替换。
pub(crate) fn keymap_row_value(view: &SettingsView, index: usize) -> (String, bool, bool) {
    if view.keymap_capture == Some(index) {
        // 按住修饰键时实时回显（"Ctrl+…"），否则给占位 + 取消提示。
        let text = if view.keymap_capture_preview.is_empty() {
            view.language
                .pick("按下新按键…（Esc 取消）", "Press new keys… (Esc cancels)")
                .to_owned()
        } else {
            format!("{}…", view.keymap_capture_preview)
        };
        return (text, false, false);
    }
    if index == keymap::QUICK_TERMINAL_ROW {
        return (
            keymap::display_stored_combo(&view.quick_terminal_hotkey),
            view.quick_terminal_hotkey != keymap::DEFAULT_QUICK_TERMINAL_HOTKEY,
            true,
        );
    }
    let action_index = index - 1;
    match view.keymap.get(action_index).and_then(|slot| slot.as_ref()) {
        Some((combo, customized)) => (combo.clone(), *customized, true),
        None => (view.language.pick("未绑定", "Unbound").to_owned(), false, false),
    }
}

/// Preview sample layout shared by the quad pass (cursor demo) and the text
/// pass (sample lines): 16px inner pad, 1.4× line pitch.
pub(crate) fn preview_line_y(top: f32, cell_h: f32, line: f32, scale: f32) -> f32 {
    top + 16.0 * scale + line * (cell_h * 1.4)
}
/// Columns of "❯ " before the demo cursor on the preview's prompt line.
pub(crate) const PREVIEW_PROMPT_COLS: usize = 2;

pub(crate) fn dropdown_selected_index(view: &SettingsView, dropdown: SettingsDropdown) -> Option<usize> {
    match dropdown {
        SettingsDropdown::Shell => {
            view.shells.iter().position(|(id, _, _)| view.shell_id.as_deref() == Some(id.as_str()))
        },
        // 加一：弹层第 0 行是搜索框，候选整体下移一行。
        SettingsDropdown::Font => {
            // 多级 fallback 列表按主族高亮（issue #33）。
            let primary = crate::renderer::primary_font_family(&view.font_family);
            view.fonts.iter().position(|family| family == primary).map(|slot| slot + 1)
        },
        SettingsDropdown::BackgroundFit => {
            BACKGROUND_FIT_OPTIONS.iter().position(|fit| *fit == view.background_image_fit)
        },
        SettingsDropdown::BackgroundAlignment => BACKGROUND_ALIGNMENT_OPTIONS
            .iter()
            .position(|alignment| *alignment == view.background_image_alignment),
        SettingsDropdown::Language => {
            LANGUAGE_OPTIONS.iter().position(|preference| *preference == view.language_preference)
        },
        SettingsDropdown::Accept => ACCEPT_OPTIONS.iter().position(|key| *key == view.accept),
        SettingsDropdown::CompletionStyle => {
            COMPLETION_STYLE_OPTIONS.iter().position(|style| *style == view.completion_style)
        },
        SettingsDropdown::BackupProtocol => {
            BACKUP_PROTOCOL_OPTIONS.iter().position(|protocol| *protocol == view.backup_protocol)
        },
        SettingsDropdown::TabReveal => {
            TAB_REVEAL_OPTIONS.iter().position(|motion| *motion == view.tab_reveal)
        },
        SettingsDropdown::Density => DENSITY_OPTIONS.iter().position(|d| *d == view.density),
        SettingsDropdown::NewTabPosition => {
            NEW_TAB_POSITION_OPTIONS.iter().position(|position| *position == view.new_tab_position)
        },
        SettingsDropdown::CellWidthMode => {
            CELL_WIDTH_MODE_OPTIONS.iter().position(|mode| *mode == view.cell_width_mode)
        },
        SettingsDropdown::CursorShape => {
            CURSOR_SHAPE_OPTIONS.iter().position(|shape| *shape == view.cursor_shape)
        },
        SettingsDropdown::SshProxyMode => {
            SSH_PROXY_MODE_OPTIONS.iter().position(|mode| *mode == view.ssh_proxy_mode)
        },
        SettingsDropdown::SshProxyProtocol => MANUAL_PROXY_PROTOCOL_OPTIONS
            .iter()
            .position(|protocol| *protocol == view.ssh_proxy_protocol),
        SettingsDropdown::SshJumpHost => crate::ssh_proxy::jump_target(&view.ssh_proxy_inputs[0])
            .and_then(|target| view.ssh_hosts.iter().position(|host| host.destination == target)),
        SettingsDropdown::BackgroundColor => view
            .background
            .and_then(|current| BACKGROUND_SWATCHES.iter().position(|color| *color == current)),
    }
}

pub(crate) fn dropdown_hover_index(hover: SettingsHit, dropdown: SettingsDropdown) -> Option<usize> {
    match (dropdown, hover) {
        (SettingsDropdown::Shell, SettingsHit::ShellPickerRow(index)) => Some(index),
        (SettingsDropdown::Font, SettingsHit::FontPickerRow(index)) => Some(index + 1),
        (SettingsDropdown::BackgroundFit, SettingsHit::FitOption(index)) => Some(index),
        (SettingsDropdown::BackgroundAlignment, SettingsHit::AlignOption(index)) => Some(index),
        (SettingsDropdown::Language, SettingsHit::Language(preference)) => {
            LANGUAGE_OPTIONS.iter().position(|option| *option == preference)
        },
        (SettingsDropdown::Accept, SettingsHit::AcceptOption(index)) => Some(index),
        (SettingsDropdown::CompletionStyle, SettingsHit::CompletionStyleOption(index)) => {
            Some(index)
        },
        (SettingsDropdown::BackupProtocol, SettingsHit::BackupProtocolOption(index)) => Some(index),
        (SettingsDropdown::TabReveal, SettingsHit::TabRevealOption(index)) => Some(index),
        (SettingsDropdown::Density, SettingsHit::DensityOption(index)) => Some(index),
        (SettingsDropdown::NewTabPosition, SettingsHit::NewTabPositionOption(index)) => Some(index),
        (SettingsDropdown::CellWidthMode, SettingsHit::CellWidthModeOption(index)) => Some(index),
        (SettingsDropdown::CursorShape, SettingsHit::CursorShapeOption(index)) => Some(index),
        (SettingsDropdown::SshProxyMode, SettingsHit::SshProxyModeOption(index)) => Some(index),
        (SettingsDropdown::SshProxyProtocol, SettingsHit::SshProxyProtocolOption(index)) => {
            Some(index)
        },
        (SettingsDropdown::SshJumpHost, SettingsHit::SshJumpHostOption(index)) => Some(index),
        _ => None,
    }
}
