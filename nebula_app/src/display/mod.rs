//! The display subsystem including window management, font rasterization, and
//! GPU drawing.

use std::cmp;
use std::fmt::{self, Formatter};
use std::mem::{self, ManuallyDrop};
use std::num::NonZeroU32;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use glutin::config::GetGlConfig;
use glutin::context::{NotCurrentContext, PossiblyCurrentContext};
use glutin::display::GetGlDisplay;
use glutin::error::ErrorKind;
use glutin::prelude::*;
use glutin::surface::{Surface, SwapInterval, WindowSurface};

use log::{debug, info, warn};
use parking_lot::MutexGuard;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::keyboard::ModifiersState;
use winit::raw_window_handle::RawWindowHandle;
use winit::window::{CursorIcon, Theme as WinitTheme};

use crossfont::{Rasterize, Size as FontSize};
use unicode_width::UnicodeWidthChar;

use nebula_terminal::event::{EventListener, OnResize};
use nebula_terminal::grid::Dimensions as TermDimensions;
use nebula_terminal::index::{Column, Direction, Line, Point};
use nebula_terminal::selection::Selection;
use nebula_terminal::term::cell::Flags;
use nebula_terminal::term::{
    self, LineDamageBounds, MIN_COLUMNS, MIN_SCREEN_LINES, Term, TermDamage, TermMode,
};
use nebula_terminal::vte::ansi::{CursorShape, NamedColor};

use crate::config::UiConfig;
use crate::config::debug::RendererPreference;
use crate::config::font::Font;
use crate::config::window::Dimensions;
use crate::config::window::StartupMode;
use crate::display::bell::VisualBell;
use crate::display::color::{List, Rgb};
use crate::display::content::{RenderableContent, RenderableCursor};
use crate::display::cursor::IntoRects;
use crate::display::damage::{DamageTracker, damage_y_to_viewport_y};
use crate::display::hint::{HintMatch, HintState};
use crate::display::meter::Meter;
use crate::display::window::Window;
use crate::event::{Event, EventType, Mouse, SearchState};
use crate::message_bar::{self, MessageBuffer, MessageType};
use crate::renderer::Rasterizer;
use crate::renderer::image::{BackgroundImageAlignment, BackgroundImageFit};
use crate::renderer::rects::{RenderLine, RenderLines, RenderRect};
use crate::renderer::ui::{Gradient, Rgba, UiQuad};
use crate::renderer::{self, GlyphCache, Renderer, platform};
use crate::scheduler::{Scheduler, TimerId, Topic};
use crate::string::{ShortenDirection, StrShortener};

mod background_color_model;
pub mod color;
mod command_completion;
pub mod content;
pub mod cursor;
pub mod hint;
pub mod image_viewer;
mod input_state;
pub mod ui;
pub mod window;

mod chrome;
pub mod command_palette;
pub(crate) mod context_menu;
mod context_menu_model;
mod document_model;
mod file_operations;
pub mod markdown_view;
mod message_queue_entry;
mod network_proxy_model;
mod program_identity;
pub mod sftp_panel;
pub mod side_panel;
mod size_info;
pub(crate) mod state;
pub(crate) mod suggest_engine;
mod surface_opacity;
/// GPUI 壳的终端元素也用这里的 [`terminal_color::TerminalColorResolver`]：
/// 「应用写死的颜色要不要按当前主题矫正」两个壳必须是同一个答案，否则同一份
/// 输出在新旧壳读起来不一样。
pub(crate) mod terminal_color;
pub(crate) mod terminal_math;
mod text_path_model;
mod toast;

/// Processor uses the same persisted value before the first window exists so
/// the global quick-terminal shortcut is active from application startup.
pub(crate) fn quick_terminal_hotkey_from_settings(config: &UiConfig) -> String {
    settings::nebula_settings_load(config).quick_terminal_hotkey
}

pub use crate::i18n::{LanguagePreference, UiLanguage};
pub use background_color_model::BgPickerPart;
pub(crate) use background_color_model::{BACKGROUND_SWATCHES, hsv_to_rgb, rgb_to_hsv};
pub(crate) use chrome::chrome_settings_button_rect;
pub use chrome::{ChromeHit, TabDropAction, in_chrome_bar, resize_edge};
use chrome::{ChromeTabLayout, TabDrag, chrome_hit_with_tabs, chrome_tab_layout, contains_rect};
pub(crate) use command_completion::{
    NEBULA_GHOST_MAX, extract_program, nebula_command_hint, nebula_command_hints,
    nebula_commands_handle, nebula_is_command_position, nebula_path_wants_directory,
};
pub use context_menu_model::{ContextMenuAction, ContextMenuHit, ContextMenuTarget};
pub(crate) use file_operations::send_to_recycle_bin;
pub(crate) use input_state::{
    nebula_clear_line, nebula_input_backspace, nebula_input_char, nebula_input_delete_word,
    nebula_input_text, nebula_prompt_line_from_raw_grid,
    nebula_shell_prompt_restored_from_raw_grid, nebula_shell_ready_from_raw_grid,
};
#[cfg(windows)]
pub(crate) use input_state::{nebula_input_from_raw_grid, nebula_raw_grid_row_preview};
pub use program_identity::AiLogo;
pub(crate) use program_identity::{
    ai_logo, ai_logo_for_program, prepare_ai_logo_texture, program_icon,
};
pub use size_info::SizeInfo;
pub use state::{
    AcceptKey, AiSessionIdentity, CompletionStyle, NebulaCompletionItem, NebulaCompletionKind,
    NebulaConfirm, NebulaInlineImage, NebulaPaneState, NebulaShell, SplitDirection, SplitNav,
};
pub use suggest_engine::SuggestEnv;
pub(crate) use text_path_model::{
    fit_tail, percent_decode_lossy, strip_file_scheme, truncate_tab_label,
};
pub use toast::ToastKind;

pub(crate) mod file_dialog;
pub(crate) mod keymap;
mod settings;
pub(crate) mod ssh_connect;
mod ssh_editor_input;
mod ssh_editor_render;
mod ssh_ui;
mod text_input;
mod ux_anims;
mod powerline_icons;

use self::ux_anims::{NebulaUiAnims, ResizeHud, SettingsToggleAnim, UiAnim};
pub use self::ux_anims::SplitReveal;

use self::powerline_icons::{
    NebulaPowerlineIcon, NebulaPowerlineIconKind, remove_ssh_host_from_lists,
    restore_ssh_host_to_lists,
};
pub(crate) use self::powerline_icons::replays_untrusted_terminal_output;
mod chrome_tabs;
mod panel_layout;
mod settings_pane;
mod settings_persist;
mod backup_pane;
mod providers_pane;
mod proxy_pane;
mod keymap_pane;
mod pickers;
mod palette_glue;
mod side_panel_glue;
mod window_surface;
mod frame_pipeline;
mod pane_render;
mod overlays;
mod completion_glue;
mod terminal_overlays;

pub use self::panel_layout::{
    CHROME_BAR_LOGICAL, CONTENT_PAD_X_LOGICAL, DRAWER_COLLAPSE_AT, DRAWER_W_MAX, DRAWER_W_MIN,
    HOSTS_BAND_MIN, PANEL_DRAG_REFLOW_MS, SIDEBAR_COLLAPSE_AT, SIDEBAR_W_LOGICAL, SIDEBAR_W_MAX,
    SIDEBAR_W_MIN, PanelDrag, PanelDragKind, bottom_content_reserve, chrome_reserve,
    content_pad_x, sidebar_width,
};
pub(crate) use self::panel_layout::{
    BADGE_FLASH, UI_CORNER_RADIUS_LOGICAL, UI_SHELL_RADIUS_LOGICAL,
};
pub(super) use self::panel_layout::{UI_CARD_SEAM_LOGICAL, UI_HAIRLINE_LOGICAL};
use self::panel_layout::apply_min_window_size;


use ssh_ui::SshDeleteUndo;
pub(crate) use ssh_ui::merge_ssh_hosts;
pub use ssh_ui::{
    SSH_DELETE_UNDO_DURATION, SshEditorField, SshEditorHit, SshEditorRects, SshHostEditor,
    auth_sections, join_destination_port, join_destination_user, push_private_key,
    split_destination_port, split_destination_user,
};
pub use ui::theme::NebulaTheme;
pub(crate) use ui::theme::write_nebula_prompt_theme;
#[derive(Debug, Clone)]
enum BackupOperation {
    Export(std::path::PathBuf),
    Restore(std::path::PathBuf),
    /// 远程备份/恢复（协议与目的地在 `nebula_backup.txt`）。口令确认后由
    /// 事件层在后台线程执行——网络绝不进 UI 线程。
    RemotePush,
    RemotePull,
}

/// 口令确认后待执行的远程备份动作，`complete_backup_operation` 返回给
/// 输入层去分发事件（display 自己够不到 event proxy）。
#[derive(Debug, Clone)]
pub(crate) struct RemoteBackupRequest {
    pub upload: bool,
    pub passphrase: String,
    pub selection: crate::encrypted_backup::BackupSelection,
}

/// Shared caret blink phase for the chrome text editors (rename / filter /
/// commit boxes). 相位挂在**最后一次编辑活动**上而不是挂钟纪元：聚焦或打完
/// 字的那一刻光标必定是亮的，连续打字期间不闪。节律取自系统的
/// `GetCaretBlinkTime`。完整理由见 [`ui::caret`]。
///
/// 保留这层薄封装是因为已有十处调用点写作 `caret_blink_on()`；新代码直接用
/// [`ui::caret::is_on`]。
pub(crate) fn caret_blink_on() -> bool {
    ui::caret::is_on()
}
#[cfg(feature = "gpui-shell")]
pub(crate) use network_proxy_model::{
    MANUAL_PROXY_PROTOCOL_OPTIONS, ManualProxyProtocol, ProxyTestStatus, manual_proxy_parts,
    manual_proxy_value,
};
pub use settings::{NebulaSettingsSection, SettingsDropdown, SettingsHit, settings_hit};
pub(crate) use settings::{NewTabPosition, SettingsOpacityTarget};

/// 按显示列宽贪心断行（确认框正文等 UI 段落用）：CJK 逐字可断，行首空
/// 格吞掉；零宽字符跟随前一个字。不做拉丁连词回退——正文以中文为主，
/// 偶发的英文单词被折断可接受。
fn wrap_display_cols(text: &str, max_cols: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut cols = 0usize;
    for ch in text.chars() {
        let w = ch.width().unwrap_or(0);
        if w == 0 {
            line.push(ch);
            continue;
        }
        if cols + w > max_cols && cols > 0 {
            lines.push(std::mem::take(&mut line));
            cols = 0;
            if ch == ' ' {
                continue;
            }
        }
        line.push(ch);
        cols += w;
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

mod bell;
mod damage;
mod meter;

/// Label for the forward terminal search bar.
const FORWARD_SEARCH_LABEL: &str = "Search: ";

/// Label for the backward terminal search bar.
const BACKWARD_SEARCH_LABEL: &str = "Backward Search: ";

/// The character used to shorten the visible text like uri preview or search regex.
const SHORTENER: char = '…';

/// Private-use placeholders emitted by Nebula's injected prompt. They are
/// replaced with spaces before text rendering; the real icons are vector UI
/// quads, so no Nerd Font or bundled font is required.
const NEBULA_FOLDER_ICON_MARKER: char = '\u{E100}';
const NEBULA_GIT_BRANCH_ICON_MARKER: char = '\u{E101}';

/// Color which is used to highlight damaged rects when debugging.
const DAMAGE_RECT_COLOR: Rgb = Rgb::new(255, 0, 255);

/// Visible split divider gap. The drag hit target is intentionally wider.
pub(crate) const NEBULA_SPLIT_DIVIDER_GAP: f32 = 2.0;
pub(crate) const NEBULA_SPLIT_HIT_SLOP: f32 = 8.0;

/// How far the unfocused split is dimmed. Focus is conveyed by brightness, not
/// a border: the inactive pane is pushed back under a translucent veil so the
/// focused pane visually "lifts" without any outline.
/// `unfocused-split-opacity = 0.7` (i.e. a 0.3 dim veil).
pub(crate) const NEBULA_UNFOCUSED_SPLIT_DIM: f32 = 0.30;

/// The UI font role: the size chrome
/// typography rasterizes at and the cell chrome layout steps by. Anchored to
/// the config font at the window's DPI — never to the terminal zoom. Stage 3
/// exposes family/size as user config.
#[derive(Clone, Copy, Debug)]
struct NebulaUiFont {
    /// Role size in physical px (config size × DPI scale).
    px: f32,
    /// Cell the chrome layout steps by, from the role's REAL rasterized
    /// metrics (`compute_cell_size` over `GlyphCache::set_ui_font_size`).
    cell: (f32, f32),
}

pub(crate) fn nebula_debug_log(message: impl AsRef<str>) {
    crate::logging::debug_log(message);
}

/// Unconditional variant of [`nebula_debug_log`] for the link-click diagnosis:
/// clicks are rare (no perf concern), and requiring a relaunch with
/// NEBULA_DEBUG_LOG=1 would double every remote-debug round-trip. Remove or
/// downgrade to the gated logger once the link path is verified.
pub(crate) fn nebula_link_log(message: impl AsRef<str>) {
    use std::io::Write as _;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| format!("{}.{:03}", d.as_secs(), d.subsec_millis()))
        .unwrap_or_else(|_| "0.000".to_owned());
    let path = nebula_data_dir().join("pebrel_debug.log");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "[{ts}] {}", message.as_ref());
    }
}

/// Directory holding Nebula's persistent state, created on demand. Settings
/// live here next to the history file managed by [`crate::nebula_history`] and
/// the session snapshot managed by [`crate::session`].
///
/// Per-platform locations and the reasoning behind them live in
/// [`crate::platform::dirs`] — this is a thin alias kept for its 26 call sites.
pub(crate) fn nebula_data_dir() -> PathBuf {
    crate::platform::dirs::data_dir().to_path_buf()
}

/// Read one raw `key=value` from `nebula_settings.txt` (case-insensitive key).
/// The typed loader is `settings::nebula_settings_load`; this is for the few
/// callers (e.g. the default-shell id) that want the raw string verbatim.
pub(crate) fn nebula_settings_value(key: &str) -> Option<String> {
    let data = std::fs::read_to_string(nebula_settings::settings_path()).ok()?;
    data.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        k.trim().eq_ignore_ascii_case(key).then(|| v.trim().to_owned())
    })
}

/// 启动时是否回放 `session.json`（设置·高级→会话，默认开）。
///
/// 事件循环在建窗之前就要问这一句，那时还没有 `Display`，也没有 `UiConfig`
/// 之外的东西——所以走原始读取而不是 `settings::nebula_settings_load`：
/// 后者是 `pub(super)`，且会为了一个 bool 解析整份设置。
pub(crate) fn restore_session_enabled() -> bool {
    nebula_settings_value("restore_session")
        .map(|v| !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off"))
        .unwrap_or(true)
}

/// 托盘图标开关（设置·高级，默认开）。与 [`restore_session_enabled`] 同一
/// 处境：托盘在建窗之前初始化，只能走原始设置读取。
pub(crate) fn tray_enabled() -> bool {
    nebula_settings_value("tray")
        .map(|v| !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off"))
        .unwrap_or(true)
}

/// Truncate/pad `text` to exactly `width` display cells (wide chars count 2;
/// a wide char that would straddle the boundary is dropped and padded over).
fn nebula_pad_to_cells(text: &str, width: usize) -> String {
    let mut out = String::with_capacity(width);
    let mut used = 0usize;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if w == 0 {
            continue;
        }
        if used + w > width {
            break;
        }
        out.push(c);
        used += w;
    }
    for _ in used..width {
        out.push(' ');
    }
    out
}

#[derive(Debug)]
pub enum Error {
    /// Error with window management.
    Window(window::Error),

    /// Error dealing with fonts.
    Font(crossfont::Error),

    /// Error in renderer.
    Render(renderer::Error),

    /// Error during context operations.
    Context(glutin::error::Error),
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Window(err) => err.source(),
            Error::Font(err) => err.source(),
            Error::Render(err) => err.source(),
            Error::Context(err) => err.source(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Error::Window(err) => err.fmt(f),
            Error::Font(err) => err.fmt(f),
            Error::Render(err) => err.fmt(f),
            Error::Context(err) => err.fmt(f),
        }
    }
}

impl From<window::Error> for Error {
    fn from(val: window::Error) -> Self {
        Error::Window(val)
    }
}

impl From<crossfont::Error> for Error {
    fn from(val: crossfont::Error) -> Self {
        Error::Font(val)
    }
}

impl From<renderer::Error> for Error {
    fn from(val: renderer::Error) -> Self {
        Error::Render(val)
    }
}

impl From<glutin::error::Error> for Error {
    fn from(val: glutin::error::Error) -> Self {
        Error::Context(val)
    }
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct DisplayUpdate {
    pub dirty: bool,

    dimensions: Option<PhysicalSize<u32>>,
    cursor_dirty: bool,
    font: Option<Font>,
    terminal_colors_dirty: bool,
}

impl DisplayUpdate {
    pub fn dimensions(&self) -> Option<PhysicalSize<u32>> {
        self.dimensions
    }

    pub fn font(&self) -> Option<&Font> {
        self.font.as_ref()
    }

    pub fn cursor_dirty(&self) -> bool {
        self.cursor_dirty
    }

    pub fn terminal_colors_dirty(&self) -> bool {
        self.terminal_colors_dirty
    }

    pub fn set_dimensions(&mut self, dimensions: PhysicalSize<u32>) {
        self.dimensions = Some(dimensions);
        self.dirty = true;
    }

    pub fn set_font(&mut self, font: Font) {
        self.font = Some(font);
        self.dirty = true;
    }

    pub fn set_cursor_dirty(&mut self) {
        self.cursor_dirty = true;
        self.dirty = true;
    }

    fn set_terminal_colors_dirty(&mut self) {
        self.terminal_colors_dirty = true;
        self.dirty = true;
    }
}

/// The display wraps a window, font rasterizer, and GPU renderer.
pub struct Display {
    pub window: Window,

    pub size_info: SizeInfo,

    /// Hint highlighted by the mouse.
    pub highlighted_hint: Option<HintMatch>,
    /// Frames since hint highlight was created.
    highlighted_hint_age: usize,

    /// Hint highlighted by the vi mode cursor.
    pub vi_highlighted_hint: Option<HintMatch>,
    /// Frames since hint highlight was created.
    vi_highlighted_hint_age: usize,

    pub raw_window_handle: RawWindowHandle,

    /// UI cursor visibility for blinking.
    pub cursor_hidden: bool,

    /// When a split is active, the focused pane's geometry. Input and hint
    /// hit-testing use this (via `pane_view()`) so mouse coordinates map into
    /// the focused half-width grid rather than the full window, which would
    /// otherwise index past the grid and panic.
    pub nebula_pane_view: Option<SizeInfo>,

    /// Transient "cols × rows" HUD shown briefly after a window resize; it fades
    /// out over ~0.9s. `None` when nothing is showing.
    nebula_resize_hud: Option<ResizeHud>,

    /// 每个 pane 的 SSH 连接进度。成功时立刻移除——卡片让位给真实终端，
    /// 持续重绘也随之停止；失败保留，让用户读得到原因。
    nebula_ssh_connect: std::collections::HashMap<u64, ssh_connect::SshConnectState>,
    /// 聚焦 pane 的 id，由绘制流程每帧同步。连接卡片只画在聚焦 pane 里，
    /// 而 `nebula_pane_view` 只给几何、不给身份。
    nebula_focused_pane: u64,

    /// Skip the first resize (window creation) so no HUD flashes at startup.
    nebula_resize_hud_armed: bool,

    /// Indexed, persistent command history used to hint a whole previous
    /// command from its prefix.
    nebula_history: crate::nebula_history::NebulaHistory,
    /// Process-wide frecency model fed only by successful shell cwd reports.
    directory_history: crate::directory_history::DirectoryHistory,
    /// Executable commands for first-token completion: PATH executables plus, on
    /// Windows, the shell's cmdlets/functions/aliases. Filled on a background
    /// thread so the PowerShell probe never blocks startup.
    nebula_commands: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    /// Per-displayed-tab animated draw-x, eased toward the laid-out / drag
    /// target each frame so tab reorder "make way" slides instead of snapping.
    nebula_tab_anim: Vec<crate::motion::Spring>,
    nebula_tab_was_visible: Vec<bool>,
    /// Active scrollbar drag: the pointer's y-offset inside the thumb captured
    /// at press time, so the thumb tracks the pointer without jumping.
    pub nebula_scrollbar_drag: Option<f32>,
    /// Slide-in reveal for a freshly created split pane: its final rect, the
    /// split direction and the animation start time. Drawn as a shrinking
    /// bg-coloured cover in `draw_split_overlays`; cleared when done.
    pub nebula_split_reveal: Option<SplitReveal>,
    /// Pending destructive-action confirmation (close with busy children /
    /// multi-line paste), drawn as a centered modal that owns the keyboard.
    pub nebula_confirm: Option<NebulaConfirm>,
    /// Screen rects of the confirm modal's (primary, cancel) buttons, written
    /// by `draw_confirm_modal` each frame so the mouse hit-test can never
    /// drift from what was actually drawn. `None` while no modal shows.
    pub nebula_confirm_buttons: Option<((f32, f32, f32, f32), (f32, f32, f32, f32))>,
    /// Pending encrypted backup operation; the passphrase remains transient.
    nebula_backup_operation: Option<BackupOperation>,
    nebula_backup_passphrase: String,
    nebula_backup_passphrase_select_all: crate::display::text_input::SelectAllState,

    /// Most recently deleted SSH host while its action is still reversible.
    nebula_ssh_delete_undo: Option<SshDeleteUndo>,
    /// 焦点 pane 的助手建议条快照（spec 001）：每帧由 WindowContext 从
    /// `NebulaPaneState::ai_fix` 同步，绘制层只认自己的字段（撤销条同款）。
    pub nebula_ai_fix_bar: Option<crate::ai_assistant::AiFixState>,
    /// Undo button geometry published by the draw pass for exact hit-testing.
    nebula_ssh_delete_undo_rect: Option<(f32, f32, f32, f32)>,
    nebula_ssh_delete_undo_hover: bool,
    /// 在场的轻提示（右下角，自动消失）。见 [`toast`]。
    nebula_toasts: Vec<toast::Toast>,
    /// 消息栏关闭按钮：绘制矩形 + 墨色，由终端 pass 发布给 chrome pass。
    /// 几何来自 `message_bar::message_close_button_rect`，与输入层的命中共用
    /// 同一个 helper，所以画出来的和点得到的永远是同一块。
    nebula_message_close: Option<((f32, f32, f32, f32), Rgb)>,
    nebula_message_close_hover: bool,
    pub nebula_ssh_editor: Option<SshHostEditor>,
    pub nebula_ssh_editor_rects: Option<SshEditorRects>,
    nebula_ssh_editor_open: bool,
    nebula_ssh_editor_hover: SshEditorHit,
    /// 正在拖选的字段。鼠标按在输入框里时置位，松开清掉——拖拽的语义是
    /// "从按下的那个字符拉到现在这个字符"，所以中途划出框外也要继续跟。
    nebula_ssh_editor_drag: Option<ssh_ui::SshEditorDrag>,
    /// 「测试连接」点击时暂存的请求；input 层随后取走并交给 SSH runtime。
    /// display 不持有事件代理，这一格就是点击→网络之间的交接台。
    nebula_ssh_test_request: Option<crate::ssh_session::SshTestRequest>,
    /// Monotonic identity for SSH editor test requests. Results must match the
    /// exact attempt, not merely a destination that a user can edit back to.
    nebula_ssh_test_seq: u64,
    /// Inline images visible this frame, collected per pane during
    /// `draw_pane` (grid lock + pane viewport at hand) and drawn in one
    /// full-window pass in `present_frame` — mid-pane GL viewport swaps are
    /// fragile, one batched pass is not.
    nebula_frame_images: Vec<(u64, std::sync::Arc<Vec<u8>>, (u32, u32), (f32, f32, f32, f32))>,

    /// Theme currently painted. In automatic mode this is the light/dark
    /// member resolved from `nebula_theme_preference` and the system state.
    pub nebula_theme: NebulaTheme,
    /// Theme family explicitly selected by the user and written to settings.
    /// Kept separate from the painted theme so an automatic light switch does
    /// not forget which dark theme to restore later.
    nebula_theme_preference: NebulaTheme,
    pub nebula_follow_system_theme: bool,
    nebula_system_theme: Option<WinitTheme>,
    /// User-configured winit decoration override. Automatic Nebula theming
    /// temporarily clears it because winit only emits `ThemeChanged` while a
    /// window is following the operating system.
    nebula_window_theme_override: Option<WinitTheme>,
    pub nebula_settings_open: bool,
    pub nebula_special_tab_active: bool,
    nebula_language_preference: LanguagePreference,
    nebula_language: UiLanguage,
    /// Paths from the last successful app configuration generation.
    nebula_config_paths: Vec<PathBuf>,
    /// Live profile snapshot used by settings and palette render paths.
    nebula_profiles: Vec<crate::config::ui_config::Profile>,
    /// Settings content scroll offset in scaled px (0 = top of the section).
    nebula_settings_scroll: f32,
    /// Command palette (Ctrl+Shift+P): fuzzy launcher model + UI state.
    nebula_palette: command_palette::CommandPalette,
    /// Installed shells, detected once (registry + filesystem scan) and cached
    /// for the new-tab dropdown. `None` until the first menu open.
    nebula_detected_shells: Option<Vec<crate::shell_detect::DetectedShell>>,
    /// Right-side drawer: directory tree / git status of the focused cwd.
    pub nebula_side_panel: side_panel::SidePanel,
    /// Remote file drawer opened from an SSH destination context menu.
    pub nebula_sftp_panel: Option<sftp_panel::SftpPanel>,
    /// Shared chrome animation state. All sidebar/drawer transitions step here
    /// so easing/timing does not get scattered across render code.
    nebula_ui_anims: NebulaUiAnims,
    /// Active sidebar section inside the settings panel.
    nebula_settings_section: NebulaSettingsSection,
    nebula_chrome_hover: ChromeHit,
    nebula_sidebar_scroll_drag: Option<chrome::SidebarScrollDrag>,
    /// Bottom-docked queue affordance. The entry state lives separately from
    /// Tabs/SSH so real Agent events can be connected without changing chrome
    /// geometry or input contracts again.
    nebula_message_queue_entry: message_queue_entry::MessageQueueEntry,
    nebula_settings_hover: SettingsHit,
    /// Primary-button settings control currently held down for HTML-like
    /// toggle active feedback. Cleared on release or when the settings view closes.
    nebula_settings_pressed: SettingsHit,
    /// Active settings opacity drag: target plus the screen-space track used
    /// for pointer-to-value mapping. Values persist only when the drag ends.
    pub nebula_settings_opacity_drag: Option<(settings::SettingsOpacityTarget, f32, f32)>,
    /// 背景色调色盘的草稿 HSV。打开浮层时从生效色初始化；拖动期间它是唯一
    /// 权威——灰/黑/白点的色相经 RGB 往返会坍缩成 0，这里保住用户拨到的值。
    nebula_bg_picker_hsv: (f32, f32, f32),
    /// 进行中的调色盘拖拽（SV 面或色相条），值实时应用、松手落盘。
    pub nebula_bg_picker_drag: Option<settings::BgPickerPart>,
    /// Unified native right-click menu shared by tab and SSH rows. The menu
    /// owns its short open/close animation so no input path needs timers.
    nebula_context_menu: Option<context_menu::ContextMenu>,
    nebula_tab_labels: Vec<String>,
    /// 只有当前活动 pane 持有可信会话 ID 且 CLI 支持 fork 时为 true；
    /// 右键菜单据此决定是否展示“分叉 AI 会话”。
    nebula_tab_ai_fork: Vec<bool>,
    /// Per-tab custom accent. `None` follows the live theme accent.
    nebula_tab_colors: Vec<Option<Rgb>>,
    nebula_tab_bells: Vec<bool>,
    /// Per-tab "command is running" flags driving the sidebar spinners.
    nebula_tab_running: Vec<bool>,
    /// 每个标签是否停在「等你批准」上，画手掌而不是圆点。
    nebula_tab_attention: Vec<bool>,
    /// 每个 tab 的 shell 短标（pwsh / cmd / ubuntu / ssh…），空 = 不显示。
    /// 静默行（无任何徽章）的右侧亮它，回答"这个 tab 是什么环境"。
    nebula_tab_shells: Vec<String>,
    /// 上一条命令非零退出且未被看到，画警示三角。
    nebula_tab_failed: Vec<bool>,
    /// 刚成功收尾，正在放对勾闪现（[`BADGE_FLASH`] 之内）。
    nebula_tab_flashing: Vec<bool>,
    /// Per-tab real AI brand logo, textured over the icon slot.
    nebula_tab_logos: Vec<Option<AiLogo>>,
    /// Decoded (and, where appropriate, theme-tinted) logo pixels with stable renderer texture ids,
    /// keyed by (logo, ink, target physical size). Decode, tint and high-quality
    /// downsampling run once per key.
    nebula_ai_logo_cache: std::collections::HashMap<
        (AiLogo, [u8; 3], u32),
        (u64, std::sync::Arc<Vec<u8>>, (u32, u32)),
    >,
    /// Decoded shell icons (full-color PNGs) with stable texture ids, keyed by
    /// shell id (pwsh/cmd/nu/wsl:Ubuntu). Decode runs once per id.
    nebula_shell_icon_cache:
        std::collections::HashMap<String, (u64, std::sync::Arc<Vec<u8>>, (u32, u32))>,
    /// Brand logos staged by the chrome pass, drawn AFTER all chrome text.
    /// draw_inline_image flips viewport/blend around its draw; interleaving
    /// it with chrome text kills every glyph batch after it, so the textured
    /// icons get their own pass at the very end of the frame.
    nebula_chrome_logo_draws: Vec<(u64, std::sync::Arc<Vec<u8>>, (u32, u32), (f32, f32, f32, f32))>,
    nebula_active_tab: usize,
    /// In-progress tab reorder drag, if the pointer is grabbing a tab.
    nebula_tab_drag: Option<TabDrag>,
    /// Whether the tab bar may be reordered right now (false during a split,
    /// where the bar hides a pane and reordering is ambiguous).
    nebula_tabs_reorderable: bool,
    /// Whether the tab sidebar is folded away. When collapsed the grid
    /// reclaims the full width and only a reveal button remains in the top bar.
    nebula_sidebar_collapsed: bool,
    /// 左侧栏逻辑宽（拖拽调节的**已应用**值——reflow/持久化读它）。
    nebula_sidebar_w: f32,
    /// 右抽屉逻辑宽（同上；布局时仍钳在窗口 42%）。
    nebula_drawer_w: f32,
    /// SSH HOSTS 停靠区高度覆盖（逻辑 px），0 = 自动弹性规则。
    nebula_hosts_band: f32,
    /// 「拖拽调节侧栏」总开关（设置·交互，默认关，开启需过确认框）。
    pub nebula_panel_resize: bool,
    /// 聚焦 pane 的工作目录（shell 通过标题上报），每帧由 `draw` 灌进来。
    /// 命令面板的「工作目录」组用它：组名右缘挂路径，组里的复制 / 定位 /
    /// 新建标签页都作用在它身上。`None` = shell 没上报，那一组整组不出现。
    pub nebula_focused_cwd: Option<std::path::PathBuf>,
    /// 进行中的面板分界线拖拽（见 [`PanelDrag`]）。
    pub nebula_panel_drag: Option<PanelDrag>,
    /// SSH host aliases from `~/.ssh/config` for the sidebar's "SSH HOSTS"
    /// section, pinned entries first (see `nebula_pinned_hosts`).
    pub nebula_ssh_hosts: Vec<String>,
    /// Host names the user pinned to the top (right-click), persisted in the
    /// runtime settings file so the order survives restarts.
    nebula_pinned_hosts: Vec<String>,
    /// Destinations auto-saved from typed `ssh` commands once the connection
    /// confirmed (see `NebulaPaneState::pending_ssh_host`), most recent
    /// first, persisted. Merged into `nebula_ssh_hosts` after the pinned
    /// block, before the `~/.ssh/config` aliases.
    nebula_saved_hosts: Vec<String>,
    /// User-deleted SSH config aliases. Config files remain untouched; hiding
    /// them here makes Delete stable instead of letting the next merge revive
    /// the row immediately.
    nebula_hidden_hosts: Vec<String>,
    /// 地址 → 用户起的显示名，从 `ssh_profiles.json` 缓存而来。侧栏每帧都要
    /// 画这些行，读文件必须发生在保存那一刻，而不是绘制路径上。
    nebula_ssh_labels: std::collections::HashMap<String, String>,
    /// 地址 → 图标 id（`ui::os_icons`），缓存策略同上。缺项 = 自动。
    nebula_ssh_icons: std::collections::HashMap<String, String>,
    /// Accordion fold state of the two sidebar sections.
    nebula_tabs_section_open: bool,
    nebula_hosts_section_open: bool,
    /// Per-section scroll offsets, in whole rows (clamped by the layout).
    nebula_tabs_scroll: usize,
    nebula_hosts_scroll: usize,
    /// A grid resize happened whose PTY notification is deferred until the
    /// interactive resize settles (see `Topic::NebulaResizeSettle`): the
    /// in-box ConPTY repaints the whole viewport per resize, so notifying it
    /// on every drag tick floods the scrollback with shredded repaints.
    pub nebula_pty_resize_pending: bool,
    /// Whether inline ghost-text suggestions are shown at all.
    pub nebula_ghost_enabled: bool,
    /// Which key accepts a ghost suggestion.
    pub nebula_accept: AcceptKey,
    /// How completions surface: inline ghost remainder or a popup list.
    pub nebula_completion_style: CompletionStyle,
    /// Default executor used by new sessions when no explicit shell is configured.
    pub nebula_shell: NebulaShell,
    /// Raw default-shell id when the user picked a detected shell the 2-value
    /// `nebula_shell` enum can't represent (cmd/pwsh/nu/wsl:X). Drives the
    /// settings row label and is persisted verbatim.
    pub nebula_shell_id: Option<String>,
    /// User-selected working directory for newly created terminal tabs.
    pub nebula_startup_directory: Option<PathBuf>,
    /// Whether new sessions print the Nebula welcome/fetch screen.
    pub nebula_fetch_enabled: bool,
    /// Whether the injected prompt uses Nebula's powerline segments.
    pub nebula_powerline_enabled: bool,
    /// 窗口背景模糊（Windows 11 上是 Mica）。默认开，见 `settings.rs` 的
    /// 裁定注释。
    pub nebula_blur: bool,
    /// Closing a window detaches its panes into the resident process for
    /// re-attach (multiplexer restore). Off = close kills the shells.
    pub nebula_keep_session: bool,
    /// 启动时回放 `session.json`（正常关窗与崩溃恢复共用这一条路）。关掉
    /// 只是不回放——快照照写，导出工作区与崩溃诊断仍然可用。
    pub nebula_restore_session: bool,
    /// 冷恢复时自动接续各 pane 的 AI 对话（claude/codex resume，T1-2）。
    /// 消费方在 `window_context::resume_agent_sessions`。
    pub nebula_resume_ai: bool,
    /// 系统托盘常驻图标 + agent attention 状态（T1-3）。消费方在
    /// `crate::tray`；这里只是设置页的开关状态。
    pub nebula_tray: bool,
    /// Runtime window opacity controlled from Nebula settings.
    pub nebula_window_opacity: f32,
    /// Which settings combobox (floating option list) is expanded, if any.
    /// One field for every dropdown: shell, font, wallpaper fit/alignment,
    /// language, accept key and cursor shape all share the widget.
    pub nebula_settings_dropdown: Option<settings::SettingsDropdown>,
    /// Default cursor shape/blink from settings. Programs may still override
    /// the shape with DECSCUSR escapes (vim's mode cursor keeps working).
    pub nebula_cursor_shape: CursorShape,
    pub nebula_cursor_blink: bool,
    /// 交互: 选中即复制（copyOnSelect）。关 = 右键复制 / 粘贴。
    pub nebula_copy_on_select: bool,
    /// 全宽字形（CJK 等）bold run 用 Regular 字形（粗体提亮不加粗，#4）。
    pub nebula_cjk_bold_regular: bool,
    /// User keybinding overrides, raw `(combo, action)` from
    /// `nebula_settings.txt` in file order (persisted verbatim).
    pub(crate) nebula_keybinds: Vec<(String, String)>,
    /// 快速终端全局快捷键的持久值；系统注册由顶层 Processor 负责。
    pub nebula_quick_terminal_hotkey: String,
    /// SSH 出站代理（全局三态）的持久镜像；连接时的真正决策在
    /// `crate::ssh_proxy`（它直接读设置文件，不经过这里）。
    pub nebula_ssh_proxy_mode: crate::ssh_proxy::ProxyMode,
    pub nebula_ssh_proxy_url: String,
    pub nebula_ssh_proxy_no_proxy: String,
    /// 等待 Processor 确认注册的新值。设置页不会绕过全局管理器自行假设成功。
    pub(crate) nebula_quick_hotkey_request: Option<String>,
    pub(crate) nebula_quick_hotkey_error: Option<String>,
    /// Parsed override table (newest-first); consulted by
    /// `process_key_bindings` BEFORE the config table (spec 002).
    pub nebula_keymap: Vec<crate::config::KeyBinding>,
    /// When `Some(row)`, the Keymap settings page is capturing a new combo
    /// for `keymap::EDITABLE_ACTIONS[row]` and the keyboard is owned by it.
    pub nebula_keymap_capture: Option<usize>,
    /// 捕获态实时回显：当前按住的修饰键前缀（"Ctrl+Shift+"）。松开清空。
    pub nebula_keymap_capture_preview: String,
    /// 与 GPUI 壳共用的标签栏位置。旧壳只负责原样保留，不改变自身布局。
    pub nebula_tabs_position: nebula_settings::TabsPositionName,
    pub nebula_tab_reveal_motion: settings::TabRevealMotion,
    /// 界面外观预设。紧凑只在既有阶梯上降一档，不引入新的视觉数值
    /// （ADR-0002）；它不改变终端字体、单元格几何或 shell 输出。
    pub nebula_density: ui::tokens::Density,
    /// 新标签插入策略。只在**真正创建标签**时生效；会话恢复与工作区导入
    /// 保持各自记录的顺序，不读这个值。
    pub nebula_new_tab_position: settings::NewTabPosition,
    /// 单元格宽度模式。只作用于终端内容网格；Nebula 原生界面的字体单元格
    /// 始终按上游的向下取整计算，不随该偏好变化。
    pub nebula_cell_width_mode: settings::CellWidthMode,
    pub nebula_font_family: String,
    nebula_font_families: Vec<String>,
    /// 系统字体族的惰性缓存：首次展开字体目录时枚举一次，之后复用。
    /// 放在启动路径上会让每次冷启都付几百个族的等宽查询开销。
    nebula_system_fonts: Option<Vec<crate::font_install::SystemFontFamily>>,
    /// 字体目录的「显示全部」临时过滤开关，不持久化。
    nebula_font_show_all: bool,
    /// 字体目录的搜索串。匹配列表上显示的那个名字，不维护跨语言别名。
    /// 只在下拉打开期间存在，关闭即清空，不持久化。
    nebula_font_query: String,
    /// 搜索框的光标与选区。与图标搜索框、SSH 表单字段共用同一套模型：
    /// 新加的输入框继承行为，不必再实现一遍。
    nebula_font_query_cursor: ui::text_field::TextCursor,
    nebula_font_popup_scroll: usize,
    /// 字体弹层滚动条拖拽中的抓取偏移（thumb 内的 y 距离）。
    nebula_font_popup_drag: Option<f32>,
    /// 当前正在拖选的设置文本框：0=字体搜索，1=按键搜索，2=SSH 代理，
    /// 3=AI 供应商。
    /// 统一在 Display 保存拖选状态，避免鼠标离开输入框后选区停止更新。
    nebula_settings_text_drag: Option<(u8, usize)>,
    /// 目录中被判定为非等宽的族（小写名）。界面据此给比例字体警告——
    /// 固定网格下它们可能重叠或截断，但用户知情后仍可选择。
    nebula_font_proportional: std::collections::HashSet<String>,
    nebula_font_notice: Option<String>,
    /// Optional runtime clear/background color controlled from settings.
    pub nebula_background: Option<Rgb>,
    /// Optional background image path drawn as a full-window wallpaper.
    pub nebula_background_image: Option<String>,
    /// Wallpaper alpha, separate from the window opacity to preserve text contrast.
    pub nebula_background_image_opacity: f32,
    /// Wallpaper sizing and anchor settings (fill / fit / stretch / tile).
    pub nebula_background_image_fit: BackgroundImageFit,
    pub nebula_background_image_alignment: BackgroundImageAlignment,
    /// Off by default: wallpapers stay inside terminal content. Enabling this
    /// requires an explicit warning confirmation because it reduces chrome contrast.
    pub nebula_background_image_cover_chrome: bool,
    nebula_settings_mtime: Option<std::time::SystemTime>,
    nebula_bg_palette_index: usize,
    /// 背景色浮层的 16 进制草稿与聚焦态（浮层关闭时归零）。
    nebula_bg_hex_input: String,
    pub(crate) nebula_bg_hex_active: bool,
    /// 设置→高级→同步（WebDAV）的四个输入草稿：url、用户名、WebDAV
    /// 密码、E2E 口令。密码/口令只是「待保存」缓冲——提交即入凭据
    /// 管理器并清空，明文从不驻留。
    nebula_sync_inputs: [String; 4],
    /// 聚焦的同步输入框（0..4，对应 [`nebula_sync_inputs`] 下标）。
    pub(crate) nebula_sync_focus: Option<usize>,
    nebula_sync_auto_pull: bool,
    /// 凭据管理器里已有 [密码, 口令]（只存在性，绝不回读明文进 UI）。
    nebula_sync_secret_set: [bool; 2],
    /// 最近一次同步动作的结果 `(message, is_error)`，画在按钮行下方。
    pub(crate) nebula_sync_status: Option<(String, bool)>,
    nebula_sync_busy: bool,
    /// Provider metadata is safe to keep in the settings model; API keys stay
    /// behind the OS credential manager and only their masked hint is copied.
    nebula_providers: crate::ai_providers::ProviderStore,
    pub(crate) nebula_provider_inputs: [String; 6],
    nebula_provider_cursors: [ui::text_field::TextCursor; 6],
    pub(crate) nebula_provider_focus: Option<usize>,
    pub(crate) nebula_provider_status: Option<(String, bool)>,
    nebula_provider_test_request: Option<crate::ai_providers::ProviderTestRequest>,
    nebula_provider_test_seq: u64,
    nebula_provider_codex_confirm: Option<String>,
    nebula_backup_selection: crate::encrypted_backup::BackupSelection,
    pub(crate) nebula_backup_status: Option<(String, bool)>,
    /// 最近一次备份状态来自远程动作（true）还是本地导出/恢复（false）——
    /// 状态行画在触发它的那组控件旁边。
    nebula_backup_status_remote: bool,
    /// 远程备份协议（`nebula_backup.txt` 的缓存，设置页打开时装载）。
    nebula_backup_protocol: crate::backup_remote::BackupProtocol,
    /// 远程备份的 5 个输入槽草稿（语义随协议变化；密文槽只是「待保存」
    /// 缓冲——提交即入凭据管理器并清空，明文从不驻留）。
    nebula_backup_remote_inputs: [String; 5],
    pub(crate) nebula_backup_remote_focus: Option<usize>,
    /// 当前协议的密文凭据是否已在凭据管理器（占位文案用，不回读明文）。
    nebula_backup_remote_secret_set: bool,
    /// 远程备份/恢复动作进行中（后台线程），按钮变灰防重复分发。
    nebula_backup_busy: bool,
    /// 聚焦的 SSH 代理输入框（0=代理地址 1=绕过列表；正文直接编辑
    /// `nebula_ssh_proxy_url` / `nebula_ssh_proxy_no_proxy`，失焦提交落盘）。
    pub(crate) nebula_ssh_proxy_focus: Option<usize>,
    /// 手动地址、绕过列表、自定义命令三个输入框的光标/选区。
    nebula_ssh_proxy_cursor: [ui::text_field::TextCursor; 3],
    /// 聚焦那一刻的原值快照，Esc 取消编辑时还原。
    nebula_ssh_proxy_backup: [String; 2],
    /// 指定代理列表的选中项。非发现项可由持久化 URL 前缀恢复；发现项在
    /// 后台扫描完成后按 URL 精确匹配，绝不根据端口猜。
    nebula_ssh_proxy_choice: settings::ProxyChoice,
    /// 手动地址的协议选择独立保留；地址被清空时不能因为 URL 暂时为空就
    /// 把用户刚选的 HTTP 悄悄重置成默认 SOCKS5。
    nebula_ssh_proxy_protocol: settings::ManualProxyProtocol,
    nebula_local_proxies: Vec<crate::ssh_proxy::LocalProxyEndpoint>,
    nebula_proxy_scanning: bool,
    nebula_proxy_scan_request: bool,
    /// 「跟随系统」探测缓存：`(URL, 来自注册表)`。进网络页 / 切模式时
    /// 刷新；渲染只读——注册表是跨进程调用，不进逐帧路径。
    nebula_system_proxy_probe: Option<(String, bool)>,
    /// 网络页真实出网测试的状态、待发送请求和单调序号。序号用于丢弃用户
    /// 修改设置后才返回的旧结果。
    nebula_proxy_test_status: settings::ProxyTestStatus,
    nebula_proxy_test_request: Option<u64>,
    nebula_proxy_test_seq: u64,
    /// 按键映射页搜索框：查询串 + 聚焦态。过滤在读取时按需计算——28 行的
    /// 字符串匹配量级，不值得为它维护缓存失效。
    nebula_keymap_query: String,
    nebula_keymap_query_cursor: ui::text_field::TextCursor,
    nebula_keymap_search_focus: bool,

    /// Tab rename state: when `Some(index, text)`, a text input is shown over
    /// tab `index` with the current edit buffer `text`. The user types to edit,
    /// Enter commits, Esc cancels (double-click to rename).
    pub nebula_tab_rename: Option<(usize, String)>,
    /// True for the instant after a rename begins: the whole existing name
    /// reads as "selected" (nushell-style blue fill) and the first typed
    /// character replaces it wholesale. Cleared on the first edit.
    pub nebula_tab_rename_select_all: bool,
    /// Insertion caret inside the rename buffer, as a CHAR index (0..=chars).
    /// Click-to-place, arrow keys, and mid-string insert/delete all go
    /// through this — a rename is a real text field, not append-only.
    pub nebula_tab_rename_caret: usize,
    /// Left pixel of the rename buffer's first glyph, stashed by `draw_chrome`
    /// each frame the box shows. Click-to-place-caret maps pointer X through
    /// this — recomputing the draw-side layout in the input path would just
    /// let the two drift.
    pub nebula_tab_rename_text_x: f32,

    pub visual_bell: VisualBell,

    /// Mapped RGB values for each terminal color.
    pub colors: List,
    /// The user's configured color scheme, untouched by theme restyling —
    /// the base every `apply_term_colors` starts from.
    nebula_default_colors: List,
    /// Draw-time adaptation for application-owned RGB colors. The terminal
    /// grid retains the original values so protocol state and copying are exact.
    terminal_color_resolver: terminal_color::TerminalColorResolver,

    /// State of the keyboard hints.
    pub hint_state: HintState,

    /// Unprocessed display updates.
    pub pending_update: DisplayUpdate,

    /// The renderer update that takes place only once before the actual rendering.
    pub pending_renderer_update: Option<RendererUpdate>,

    /// The ime on the given display.
    pub ime: Ime,

    /// The state of the timer for frame scheduling.
    pub frame_timer: FrameTimer,

    /// Damage tracker for the given display.
    pub damage_tracker: DamageTracker,

    /// Font size used by the window.
    pub font_size: FontSize,

    /// UI 字体角色：chrome 排版锚定在这个
    /// 状态上，永不跟随终端缩放。Ctrl+滚轮 / 设置 spinner 只改
    /// `font_size`（终端网格与跟随它的文档查看器）。阶段 3 将把
    /// family/size 暴露为独立配置。
    nebula_ui_font: NebulaUiFont,

    /// 抽屉视图是否路由到 SFTP（聚焦 pane 的 SSH 身份与面板连接匹配）。
    /// 见 [`Self::route_side_panel`]。
    nebula_sftp_routed: bool,

    // Mouse point position when highlighting hints.
    hint_mouse_point: Option<Point>,

    renderer: ManuallyDrop<Renderer>,
    renderer_preference: Option<RendererPreference>,

    surface: ManuallyDrop<Surface<WindowSurface>>,

    context: ManuallyDrop<PossiblyCurrentContext>,

    glyph_cache: GlyphCache,
    meter: Meter,
}

/// 计算全屏 TUI 在网格之外需要补齐的垂直背景带。内部边缘必须停在 Pane 边界，
/// 只有接触终端外沿的 Pane 才能继续延伸到圆角卡片边缘。
fn alt_screen_vertical_padding_bands(
    window: &SizeInfo,
    pane: &SizeInfo,
    card_y: f32,
    card_height: f32,
) -> [Option<(f32, f32)>; 2] {
    const EDGE_EPSILON: f32 = 0.5;

    let window_grid_top = window.padding_y();
    let window_grid_bottom = window.height() - window.padding_bottom();
    let pane_top = pane.padding_y();
    let pane_bottom = pane.height() - pane.padding_bottom();
    let grid_bottom = pane_top + pane.screen_lines() as f32 * pane.cell_height();

    let band = |start: f32, end: f32| {
        let height = (end - start).max(0.0);
        (height > f32::EPSILON).then_some((start, height))
    };

    let top = if (pane_top - window_grid_top).abs() <= EDGE_EPSILON {
        band(card_y, pane_top)
    } else {
        None
    };
    let bottom_limit = if (pane_bottom - window_grid_bottom).abs() <= EDGE_EPSILON {
        card_y + card_height
    } else {
        pane_bottom
    };

    [top, band(grid_bottom, bottom_limit)]
}

/// Prefer the event loop's system-wide appearance over the window theme.
///
/// On Windows, `Window::theme()` is a cached per-window value and can still
/// contain the previous manual override immediately after `set_theme(None)`.
fn system_theme_snapshot(
    event_loop_theme: Option<WinitTheme>,
    window_theme: Option<WinitTheme>,
) -> Option<WinitTheme> {
    event_loop_theme.or(window_theme)
}

impl Display {
    pub fn new(
        window: Window,
        gl_context: NotCurrentContext,
        config: &UiConfig,
        system_theme: Option<WinitTheme>,
        _tabbed: bool,
    ) -> Result<Display, Error> {
        let raw_window_handle = window.raw_window_handle();

        let scale_factor = window.scale_factor as f32;
        let settings_init = settings::nebula_settings_load(config);
        let rasterizer = Rasterizer::new()?;
        crate::boot_trace("rasterizer ready");

        // 设置里保存过字号则优先生效（逻辑 px × 缩放），否则跟随配置文件；
        // Ctrl+滚轮 / 设置 spinner 改过的字号因此在重启后保持。
        let font_size = settings_init
            .font_size
            .map(|px| FontSize::from_px(px * scale_factor))
            .unwrap_or_else(|| config.font.size().scale(scale_factor));
        // UI 锚定字号始终取配置字号（0.7 默认）：终端字号的持久化缩放不
        // 影响 chrome。2026-07-28 曾试过锚定 settings 保存字号，实测 16.3px
        // 让 chrome 明显过大，用户裁定回到 0.7 默认——当时「图标变小」的
        // 真凶是 ambiguous 宽度缩放误伤 PUA 图标，已在 glyph_cache 排除。
        let ui_font_px = config.font.size().scale(scale_factor).as_px();
        #[cfg(windows)]
        let (rasterizer, required_font_install) = {
            let mut rasterizer = rasterizer;
            let installed = GlyphCache::font_family_available(
                &mut rasterizer,
                crate::font_install::REQUIRED_FONT_FAMILY,
                font_size,
            );
            let required = (!installed).then(|| NebulaConfirm::InstallRequiredFont {
                directory: crate::font_install::bundled_font_directory(),
            });
            (rasterizer, required)
        };
        #[cfg(not(windows))]
        let required_font_install = None;

        debug!("Loading \"{}\" font", &settings_init.font_family);
        let font =
            config.font.clone().with_family(settings_init.font_family.clone()).with_size(font_size);
        // 保存的字体偏好可能在两次启动之间消失（系统字体被卸载、导入文件
        // 被删）。那种情况本次回退到内置字体并告警，但**保留原偏好**——
        // 字体恢复可用后，下次启动自动回到用户的选择。
        let (mut glyph_cache, font_notice) = match GlyphCache::new(rasterizer, &font) {
            Ok(cache) => (cache, None),
            Err(error) => {
                let fallback = config
                    .font
                    .clone()
                    .with_family(crate::font_install::REQUIRED_FONT_FAMILY.to_owned())
                    .with_size(font_size);
                let notice = format!(
                    "字体「{}」本次不可用（{error}），暂用内置字体；偏好已保留。",
                    settings_init.font_family
                );
                let rasterizer = Rasterizer::new()?;
                (GlyphCache::new(rasterizer, &fallback)?, Some(notice))
            },
        };
        glyph_cache.wide_bold_use_regular = settings_init.cjk_bold_regular;
        #[cfg(windows)]
        let mut nebula_font_families = glyph_cache.private_font_families();
        #[cfg(not(windows))]
        let mut nebula_font_families = vec![settings_init.font_family.clone()];
        nebula_font_families.retain(|family| family != crate::font_install::REQUIRED_FONT_FAMILY);
        nebula_font_families.insert(0, crate::font_install::REQUIRED_FONT_FAMILY.to_owned());
        if !nebula_font_families.iter().any(|family| family == &settings_init.font_family) {
            nebula_font_families.push(settings_init.font_family.clone());
        }
        crate::boot_trace("glyph cache (font faces loaded)");

        let metrics = glyph_cache.font_metrics();
        let (cell_width, cell_height) =
            compute_cell_size(config, &metrics, settings_init.cell_width_mode);

        // Resize the window to the user-configured size, or a Windows
        // Terminal-like default when unset. A 116-column by 30-row canvas is
        // the standard startup size. Two inputs are deliberately excluded:
        // the session file's saved window size (stale/wrong-domain values
        // kept resurfacing as near-fullscreen launches), and the persisted
        // terminal zoom — the startup grid is priced at the CONFIG base font
        // size, because 116 columns of a Ctrl+wheel-enlarged cell is itself
        // a near-fullscreen window. The zoomed font still renders; it just
        // shows fewer columns in the standard-sized window.
        let dimensions = config
            .window
            .dimensions()
            .unwrap_or(crate::config::window::Dimensions { columns: 116, lines: 30 });
        let base_font_size = config.font.size().scale(scale_factor);
        let (base_cell_width, base_cell_height) = glyph_cache
            .metrics_at(base_font_size)
            .map(|base_metrics| {
                compute_cell_size(config, &base_metrics, settings_init.cell_width_mode)
            })
            .unwrap_or((cell_width, cell_height));
        let size = window_size(
            config,
            dimensions,
            base_cell_width,
            base_cell_height,
            scale_factor,
            settings_init.sidebar_w,
        );
        window.request_inner_size(size);

        // Create the GL surface to draw into.
        let surface = platform::create_gl_surface(
            &gl_context,
            window.inner_size(),
            window.raw_window_handle(),
        )?;

        // Make the context current.
        let context = gl_context.make_current(&surface)?;
        crate::boot_trace("surface + context current");

        // Let the OS refuse resizes that would collapse the grid below a usable
        // column count — without this, dragging narrow turns 2 columns of real
        // content into hundreds of soft-wrapped rows that overflow the
        // scrollback (data loss no reflow can undo).
        #[cfg(windows)]
        apply_min_window_size(&window, config, cell_width, cell_height, settings_init.sidebar_w);

        // Create renderer.
        let mut renderer = Renderer::new(&context, config.debug.renderer)?;
        crate::boot_trace("renderer (shaders compiled)");

        // Load font common glyphs to accelerate rendering.
        debug!("Filling glyph cache with common glyphs");
        renderer.with_loader(|mut api| {
            glyph_cache.reset_glyph_cache(&mut api);
        });
        crate::boot_trace("glyph cache warmed");

        let padding = config.window.padding(window.scale_factor as f32);
        let chrome = chrome_reserve(window.scale_factor as f32);
        let viewport_size = window.inner_size();

        // Create new size with at least one column and row.
        // Asymmetric from the start: the sidebar is expanded on launch, so the
        // left padding carries it while the right keeps the plain content
        // margin. Dynamic padding is dropped — the sidebar fixes the left edge.
        let scale = window.scale_factor as f32;
        let content_pad = content_pad_x(scale);
        let size_info = SizeInfo::new_fully_asymmetric(
            viewport_size.width as f32,
            viewport_size.height as f32,
            cell_width,
            cell_height,
            padding.0 + content_pad + sidebar_width(scale, false, settings_init.sidebar_w),
            padding.0 + content_pad,
            padding.1 + chrome,
            padding.1 + bottom_content_reserve(scale),
        );

        info!("Cell size: {cell_width} x {cell_height}");
        info!("Padding: {} x {}", size_info.padding_x(), size_info.padding_y());
        info!("Width: {}, Height: {}", size_info.width(), size_info.height());

        // Update OpenGL projection.
        renderer.resize(&size_info);

        // Clear screen.
        let nebula_window_theme_override = config.window.theme();
        if settings_init.follow_system_theme {
            window.set_theme(None);
        }
        let nebula_system_theme = system_theme_snapshot(system_theme, window.theme());
        let nebula_theme = if settings_init.follow_system_theme {
            nebula_system_theme
                .map(|theme| {
                    settings_init.theme.for_system_appearance(matches!(theme, WinitTheme::Light))
                })
                .unwrap_or(settings_init.theme)
        } else {
            settings_init.theme
        };
        let background_color = if settings_init.follow_system_theme {
            nebula_theme.palette().term_bg
        } else {
            settings_init.background.unwrap_or(config.colors.primary.background)
        };
        renderer.clear(background_color, settings_init.opacity);
        window.set_transparent(settings_init.opacity < 1.0);
        // 背景模糊的开关住在 nebula_settings.txt 里，不是基础配置侧的
        // `window.blur`——所以要在这里按真正的设置再压一次，否则窗口创建时
        // 用的是那个字段的默认值。
        window.set_blur(settings_init.blur);

        // Disable shadows for transparent windows on macOS.
        #[cfg(target_os = "macos")]
        window.set_has_shadow(settings_init.opacity >= 1.0);

        let is_wayland = matches!(raw_window_handle, RawWindowHandle::Wayland(_));

        // On Wayland we can safely ignore this call, since the window isn't visible until you
        // actually draw something into it and commit those changes.
        if !is_wayland {
            surface.swap_buffers(&context).expect("failed to swap buffers.");
            renderer.finish();
        }
        crate::boot_trace("first swap done");

        // Set resize increments for the newly created window.
        if config.window.resize_increments {
            window.set_resize_increments(Some(PhysicalSize::new(cell_width, cell_height)));
        }

        window.set_visible(true);
        crate::boot_trace("window visible");

        // Always focus new windows, even if no Nebula window is currently focused.
        #[cfg(target_os = "macos")]
        window.focus_window();

        if !_tabbed {
            match config.window.startup_mode {
                #[cfg(target_os = "macos")]
                StartupMode::SimpleFullscreen => window.set_simple_fullscreen(true),
                StartupMode::Maximized if !is_wayland => window.set_maximized(true),
                #[cfg(windows)]
                StartupMode::Fullscreen => window.set_fullscreen(true),
                _ => (),
            }
        }

        let hint_state = HintState::new(config.hints.alphabet());
        // Publish the RESTORED theme to the prompt bridge (writing the default
        // here used to reset the powerline colors on every launch).
        write_nebula_prompt_theme(nebula_theme);

        let mut damage_tracker = DamageTracker::new(size_info.screen_lines(), size_info.columns());
        damage_tracker.debug = config.debug.highlight_damage;

        // Disable vsync.
        if let Err(err) = surface.set_swap_interval(&context, SwapInterval::DontWait) {
            info!("Failed to disable vsync: {err}");
        }
        crate::boot_trace("swap interval set");

        // Terminal color table: the user's configured scheme, restyled by the
        // restored theme (light themes swap in a readable light ANSI set, and
        // the background OSC 11 reports must match the theme from frame one).
        let nebula_default_colors = List::from(&config.colors);
        let mut initial_colors = nebula_default_colors;
        nebula_theme.apply_term_colors(&mut initial_colors, &nebula_default_colors);

        let mut display = Self {
            context: ManuallyDrop::new(context),
            visual_bell: VisualBell::from(&config.bell),
            renderer: ManuallyDrop::new(renderer),
            renderer_preference: config.debug.renderer,
            surface: ManuallyDrop::new(surface),
            colors: initial_colors,
            nebula_default_colors,
            terminal_color_resolver: Default::default(),
            frame_timer: FrameTimer::new(),
            raw_window_handle,
            damage_tracker,
            glyph_cache,
            hint_state,
            size_info,
            font_size,
            nebula_ui_font: NebulaUiFont { px: ui_font_px, cell: (0.0, 0.0) },
            nebula_sftp_routed: true,
            window,
            pending_renderer_update: Default::default(),
            vi_highlighted_hint_age: Default::default(),
            highlighted_hint_age: Default::default(),
            vi_highlighted_hint: Default::default(),
            highlighted_hint: Default::default(),
            hint_mouse_point: Default::default(),
            pending_update: Default::default(),
            cursor_hidden: Default::default(),
            nebula_pane_view: None,
            nebula_resize_hud: None,
            nebula_ssh_connect: std::collections::HashMap::new(),
            nebula_focused_pane: 0,
            nebula_resize_hud_armed: false,
            nebula_history: {
                let history = crate::nebula_history::NebulaHistory::load();
                crate::boot_trace("history loaded");
                history
            },
            directory_history: crate::directory_history::global(),
            nebula_commands: nebula_commands_handle(),
            nebula_tab_anim: Vec::new(),
            nebula_tab_was_visible: vec![true],
            nebula_scrollbar_drag: None,
            nebula_split_reveal: None,
            nebula_confirm: required_font_install,
            nebula_confirm_buttons: None,
            nebula_backup_operation: None,
            nebula_backup_passphrase: String::new(),
            nebula_backup_passphrase_select_all: Default::default(),

            nebula_ssh_delete_undo: None,
            nebula_ai_fix_bar: None,
            nebula_ssh_delete_undo_rect: None,
            nebula_ssh_delete_undo_hover: false,
            nebula_toasts: Vec::new(),
            nebula_message_close: None,
            nebula_message_close_hover: false,
            nebula_ssh_editor: None,
            nebula_ssh_editor_rects: None,
            nebula_ssh_editor_open: false,
            nebula_ssh_editor_hover: SshEditorHit::None,
            nebula_ssh_editor_drag: None,
            nebula_ssh_test_request: None,
            nebula_ssh_test_seq: 0,
            nebula_frame_images: Vec::new(),
            nebula_theme,
            nebula_theme_preference: settings_init.theme,
            nebula_follow_system_theme: settings_init.follow_system_theme,
            nebula_system_theme,
            nebula_window_theme_override,
            nebula_settings_open: false,
            nebula_special_tab_active: false,
            nebula_language_preference: settings_init.language,
            nebula_language: settings_init.language.resolved(),
            nebula_config_paths: config.config_paths.clone(),
            nebula_profiles: config.profiles.clone(),
            nebula_settings_scroll: 0.0,
            nebula_palette: {
                let mut palette = command_palette::CommandPalette::new();
                palette.set_language(settings_init.language.resolved());
                palette
            },
            nebula_detected_shells: None,
            nebula_side_panel: side_panel::SidePanel::new(),
            nebula_sftp_panel: None,
            nebula_ui_anims: NebulaUiAnims::new(),
            nebula_settings_section: NebulaSettingsSection::default(),
            nebula_chrome_hover: ChromeHit::None,
            nebula_sidebar_scroll_drag: None,
            nebula_message_queue_entry: message_queue_entry::MessageQueueEntry::default(),
            nebula_settings_hover: SettingsHit::None,
            nebula_settings_pressed: SettingsHit::None,
            nebula_settings_opacity_drag: None,
            nebula_bg_picker_hsv: (220.0, 0.0, 0.0),
            nebula_bg_picker_drag: None,
            nebula_context_menu: None,
            nebula_settings_dropdown: None,
            nebula_cursor_shape: settings_init.cursor_shape,
            nebula_cursor_blink: settings_init.cursor_blink,
            nebula_copy_on_select: settings_init.copy_on_select,
            nebula_cjk_bold_regular: settings_init.cjk_bold_regular,
            nebula_keymap: keymap::build_bindings(&settings_init.keybinds),
            nebula_keybinds: settings_init.keybinds,
            nebula_quick_terminal_hotkey: settings_init.quick_terminal_hotkey,
            nebula_ssh_proxy_mode: settings_init.ssh_proxy_mode,
            nebula_ssh_proxy_url: settings_init.ssh_proxy_url,
            nebula_ssh_proxy_no_proxy: settings_init.ssh_proxy_no_proxy,
            nebula_quick_hotkey_request: None,
            nebula_quick_hotkey_error: None,
            nebula_keymap_capture: None,
            nebula_keymap_capture_preview: String::new(),
            nebula_tabs_position: settings_init.tabs_position,
            nebula_tab_reveal_motion: settings_init.tab_reveal,
            nebula_density: settings_init.density,
            nebula_new_tab_position: settings_init.new_tab_position,
            nebula_cell_width_mode: settings_init.cell_width_mode,
            nebula_font_family: settings_init.font_family,
            nebula_font_families,
            nebula_system_fonts: None,
            nebula_font_show_all: false,
            nebula_font_query: String::new(),
            nebula_font_query_cursor: Default::default(),
            nebula_font_popup_scroll: 0,
            nebula_font_popup_drag: None,
            nebula_settings_text_drag: None,
            nebula_font_proportional: std::collections::HashSet::new(),
            nebula_font_notice: font_notice,
            nebula_tab_labels: vec![".".to_owned()],
            nebula_tab_ai_fork: vec![false],
            nebula_tab_colors: vec![None],
            nebula_tab_bells: vec![false],
            nebula_tab_running: vec![false],
            nebula_tab_attention: vec![false],
            nebula_tab_shells: vec![String::new()],
            nebula_tab_failed: vec![false],
            nebula_tab_flashing: vec![false],
            nebula_tab_logos: vec![None],
            nebula_ai_logo_cache: Default::default(),
            nebula_shell_icon_cache: Default::default(),
            nebula_chrome_logo_draws: Vec::new(),
            nebula_active_tab: 0,
            nebula_tab_drag: None,
            nebula_tabs_reorderable: true,
            nebula_sidebar_collapsed: false,
            nebula_sidebar_w: settings_init.sidebar_w,
            nebula_drawer_w: settings_init.drawer_w,
            nebula_hosts_band: settings_init.hosts_band,
            nebula_panel_resize: settings_init.panel_resize,
            nebula_focused_cwd: None,
            nebula_panel_drag: None,
            nebula_ssh_hosts: merge_ssh_hosts(
                &settings_init.saved_hosts,
                &settings_init.pinned_hosts,
                &settings_init.hidden_hosts,
            ),
            nebula_pinned_hosts: settings_init.pinned_hosts.clone(),
            nebula_saved_hosts: settings_init.saved_hosts.clone(),
            nebula_hidden_hosts: settings_init.hidden_hosts.clone(),
            nebula_ssh_labels: crate::ssh_profiles::SshProfiles::load(
                &nebula_data_dir().join("ssh_profiles.json"),
            )
            .map(|profiles| profiles.labels())
            .unwrap_or_default(),
            nebula_ssh_icons: crate::ssh_profiles::SshProfiles::load(
                &nebula_data_dir().join("ssh_profiles.json"),
            )
            .map(|profiles| profiles.icons())
            .unwrap_or_default(),
            nebula_tabs_section_open: true,
            nebula_hosts_section_open: true,
            nebula_tabs_scroll: 0,
            nebula_hosts_scroll: 0,
            nebula_tab_rename: None,
            nebula_tab_rename_select_all: false,
            nebula_tab_rename_caret: 0,
            nebula_tab_rename_text_x: 0.0,
            nebula_pty_resize_pending: false,
            nebula_ghost_enabled: settings_init.ghost,
            nebula_accept: settings_init.accept,
            nebula_completion_style: settings_init.completion_style,
            nebula_shell: settings_init.shell,
            nebula_shell_id: settings_init.shell_id.clone(),
            nebula_startup_directory: settings_init.startup_directory,
            nebula_fetch_enabled: settings_init.fetch,
            nebula_powerline_enabled: settings_init.powerline,
            nebula_blur: settings_init.blur,
            nebula_keep_session: settings_init.keep_session,
            nebula_restore_session: settings_init.restore_session,
            nebula_resume_ai: settings_init.resume_ai,
            nebula_tray: settings_init.tray,
            nebula_window_opacity: settings_init.opacity,
            nebula_background: if settings_init.follow_system_theme {
                Some(nebula_theme.palette().term_bg)
            } else {
                settings_init.background
            },
            nebula_background_image: settings_init.background_image,
            nebula_background_image_opacity: settings_init.background_image_opacity,
            nebula_background_image_fit: settings_init.background_image_fit,
            nebula_background_image_alignment: settings_init.background_image_alignment,
            nebula_background_image_cover_chrome: settings_init.background_image_cover_chrome,
            nebula_settings_mtime: settings::nebula_settings_mtime(),
            nebula_bg_palette_index: 0,
            nebula_bg_hex_input: String::new(),
            nebula_bg_hex_active: false,
            nebula_sync_inputs: Default::default(),
            nebula_sync_focus: None,
            nebula_sync_auto_pull: false,
            nebula_sync_secret_set: [false; 2],
            nebula_sync_status: None,
            nebula_sync_busy: false,
            nebula_providers: crate::ai_providers::load(),
            nebula_provider_inputs: Default::default(),
            nebula_provider_cursors: Default::default(),
            nebula_provider_focus: None,
            nebula_provider_status: None,
            nebula_provider_test_request: None,
            nebula_provider_test_seq: 0,
            nebula_provider_codex_confirm: None,
            nebula_backup_selection: crate::encrypted_backup::BackupSelection::default(),
            nebula_backup_status: None,
            nebula_backup_status_remote: false,
            nebula_backup_protocol: Default::default(),
            nebula_backup_remote_inputs: Default::default(),
            nebula_backup_remote_focus: None,
            nebula_backup_remote_secret_set: false,
            nebula_backup_busy: false,
            nebula_ssh_proxy_focus: None,
            nebula_ssh_proxy_cursor: Default::default(),
            nebula_ssh_proxy_backup: Default::default(),
            nebula_ssh_proxy_choice: settings::ProxyChoice::Manual,
            nebula_ssh_proxy_protocol: settings::ManualProxyProtocol::Socks5,
            nebula_local_proxies: Vec::new(),
            nebula_proxy_scanning: false,
            nebula_proxy_scan_request: false,
            nebula_system_proxy_probe: None,
            nebula_proxy_test_status: settings::ProxyTestStatus::Idle,
            nebula_proxy_test_request: None,
            nebula_proxy_test_seq: 0,
            nebula_keymap_query: String::new(),
            nebula_keymap_query_cursor: Default::default(),
            nebula_keymap_search_focus: false,
            meter: Default::default(),
            ime: Default::default(),
        };
        // A persisted zoom means the very first frame already runs off the UI
        // base size — the font role must be pinned NOW, not after the first
        // font change funnels through handle_update.
        display.refresh_ui_font(config);
        display.nebula_ssh_proxy_protocol =
            settings::manual_proxy_parts(&display.nebula_ssh_proxy_url).0;
        display.nebula_ssh_proxy_choice =
            if crate::ssh_proxy::jump_target(&display.nebula_ssh_proxy_url).is_some() {
                settings::ProxyChoice::Jump
            } else if crate::ssh_proxy::command_target(&display.nebula_ssh_proxy_url).is_some() {
                settings::ProxyChoice::Command
            } else {
                settings::ProxyChoice::Manual
            };
        display.refresh_system_proxy_probe();
        Ok(display)
    }

    /// Request a new frame for a window on Wayland.
    fn request_frame(&mut self, scheduler: &mut Scheduler) {
        // Mark that we've used a frame.
        self.window.has_frame = false;

        // Get the display vblank interval.
        let monitor_vblank_interval = 1_000_000.
            / self
                .window
                .current_monitor()
                .and_then(|monitor| monitor.refresh_rate_millihertz())
                .unwrap_or(60_000) as f64;

        // Now convert it to micro seconds.
        let monitor_vblank_interval =
            Duration::from_micros((1000. * monitor_vblank_interval) as u64);

        let swap_timeout = self.frame_timer.compute_timeout(monitor_vblank_interval);

        let window_id = self.window.id();
        let timer_id = TimerId::new(Topic::Frame, window_id);
        let event = Event::new(EventType::Frame, window_id);

        scheduler.schedule(event, swap_timeout, false, timer_id);
    }
}

/// Map a pointer position in the currently visible tab rows back to the
/// storage index used by the pane list. The layout intentionally keeps hidden
/// rows as zero rectangles, so this function's input must already be filtered
/// to positive-size rows; keeping that invariant explicit prevents the two
/// coordinate spaces from being mixed again.
fn tab_drop_index_from_visible_rows(
    source: usize,
    y: f32,
    visible: &[(usize, (f32, f32, f32, f32))],
    tab_count: usize,
) -> usize {
    let Some((visible_start, _)) = visible.first() else { return source };
    let passed = visible
        .iter()
        .filter(|(index, rect)| *index != source && y > rect.1 + rect.3 * 0.5)
        .count();
    visible_start.saturating_add(passed).min(tab_count.saturating_sub(1))
}

impl Drop for Display {
    fn drop(&mut self) {
        // Switch OpenGL context before dropping, otherwise objects (like programs) from other
        // contexts might be deleted when dropping renderer.
        self.make_current();
        unsafe {
            ManuallyDrop::drop(&mut self.renderer);
            ManuallyDrop::drop(&mut self.context);
            ManuallyDrop::drop(&mut self.surface);
        }
    }
}

/// Input method state.
#[derive(Debug, Default)]
pub struct Ime {
    /// Whether the IME is enabled.
    enabled: bool,

    /// Current IME preedit.
    preedit: Option<Preedit>,
}

impl Ime {
    #[inline]
    pub fn set_enabled(&mut self, is_enabled: bool) {
        if is_enabled {
            self.enabled = is_enabled
        } else {
            // Clear state when disabling IME.
            *self = Default::default();
        }
    }

    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    #[inline]
    pub fn set_preedit(&mut self, preedit: Option<Preedit>) {
        self.preedit = preedit;
    }

    #[inline]
    pub fn preedit(&self) -> Option<&Preedit> {
        self.preedit.as_ref()
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Preedit {
    /// The preedit text.
    text: String,

    /// Byte offset for cursor start into the preedit text.
    ///
    /// `None` means that the cursor is invisible.
    cursor_byte_offset: Option<(usize, usize)>,

    /// The cursor offset from the end of the start of the preedit in char width.
    cursor_end_offset: Option<(usize, usize)>,
}

impl Preedit {
    pub fn new(text: String, cursor_byte_offset: Option<(usize, usize)>) -> Self {
        let cursor_end_offset = if let Some(byte_offset) = cursor_byte_offset {
            // Convert byte offset into char offset.
            let start_to_end_offset =
                text[byte_offset.0..].chars().fold(0, |acc, ch| acc + ch.width().unwrap_or(1));
            let end_to_end_offset =
                text[byte_offset.1..].chars().fold(0, |acc, ch| acc + ch.width().unwrap_or(1));

            Some((start_to_end_offset, end_to_end_offset))
        } else {
            None
        };

        Self { text, cursor_byte_offset, cursor_end_offset }
    }
}

/// Pending renderer updates.
///
/// All renderer updates are cached to be applied just before rendering, to avoid platform-specific
/// rendering issues.
#[derive(Debug, Default, Copy, Clone)]
pub struct RendererUpdate {
    /// Should resize the window.
    resize: bool,

    /// Clear font caches.
    clear_font_cache: bool,
}

/// The frame timer state.
pub struct FrameTimer {
    /// Base timestamp used to compute sync points.
    base: Instant,

    /// The last timestamp we synced to.
    last_synced_timestamp: Instant,

    /// The refresh rate we've used to compute sync timestamps.
    refresh_interval: Duration,
}

/// Calculate the cell dimensions based on font metrics.
///
/// This will return a tuple of the cell width and height.
#[inline]
fn compute_cell_size(
    config: &UiConfig,
    metrics: &crossfont::Metrics,
    cell_width_mode: settings::CellWidthMode,
) -> (f32, f32) {
    let offset_x = f64::from(config.font.offset.x);
    let offset_y = f64::from(config.font.offset.y);
    // 宽度取整方式由单元格宽度模式决定；高度始终向下取整，两种模式必须
    // 得到逐位相同的高度——该偏好只控制列宽。
    let raw_width = metrics.average_advance + offset_x;
    let width = match cell_width_mode {
        settings::CellWidthMode::Compact => raw_width.floor(),
        settings::CellWidthMode::Relaxed => raw_width.round(),
    };
    (width.max(1.) as f32, (metrics.line_height + offset_y).floor().max(1.) as f32)
}

/// Calculate the size of the window given padding, terminal dimensions and cell size.
fn window_size(
    config: &UiConfig,
    dimensions: Dimensions,
    cell_width: f32,
    cell_height: f32,
    scale_factor: f32,
    sidebar_w: f32,
) -> PhysicalSize<u32> {
    let padding = config.window.padding(scale_factor);
    let chrome = chrome_reserve(scale_factor);

    let grid_width = cell_width * dimensions.columns.max(MIN_COLUMNS) as f32;
    let grid_height = cell_height * dimensions.lines.max(MIN_SCREEN_LINES) as f32;

    // Left absorbs the sidebar (expanded by default), right is the plain
    // content margin, matching the asymmetric grid the sidebar produces.
    // 侧栏宽被拖宽过的话窗口相应更宽——启动公式仍是「字号 × 116 × 30」，
    // 列数不因侧栏变化而缩水。
    let pad_left =
        padding.0 + content_pad_x(scale_factor) + sidebar_width(scale_factor, false, sidebar_w);
    let pad_right = padding.0 + content_pad_x(scale_factor);
    let width = (grid_width + pad_left + pad_right).floor();
    let pad_top = padding.1 + chrome;
    let pad_bottom = padding.1 + bottom_content_reserve(scale_factor);
    let height = (pad_top + grid_height + pad_bottom).floor();

    PhysicalSize::new(width as u32, height as u32)
}

#[cfg(test)]
mod nebula_ux_tests {
    use nebula_terminal::grid::Dimensions;
    use winit::window::Theme as WinitTheme;

    use super::{
        AiLogo, NebulaConfirm, SizeInfo, ai_logo, alt_screen_vertical_padding_bands,
        compute_cell_size, extract_program, nebula_pad_to_cells, percent_decode_lossy,
        prepare_ai_logo_texture, program_icon, remove_ssh_host_from_lists,
        replays_untrusted_terminal_output, restore_ssh_host_to_lists, strip_file_scheme,
        system_theme_snapshot,
    };
    use crate::config::UiConfig;
    use crate::display::settings::CellWidthMode;

    /// 受控字体度量：只有 advance 与 line_height 参与单元格尺寸计算，
    /// 其余字段取任意合法值。
    fn metrics(average_advance: f64, line_height: f64) -> crossfont::Metrics {
        crossfont::Metrics {
            average_advance,
            line_height,
            descent: -4.0,
            underline_position: -2.0,
            underline_thickness: 1.0,
            strikeout_position: 5.0,
            strikeout_thickness: 1.0,
        }
    }

    #[test]
    fn relaxed_cell_width_rounds_up_the_fraction_compact_floors_it() {
        let config = UiConfig::default();
        // Maple Mono NF CN 这类字体的平均 advance 常落在 .5 以上，紧凑向下
        // 取整因此会少一像素——宽松就是为补这一像素而设。
        let m = metrics(9.6, 20.0);
        assert_eq!(compute_cell_size(&config, &m, CellWidthMode::Compact).0, 9.0);
        assert_eq!(compute_cell_size(&config, &m, CellWidthMode::Relaxed).0, 10.0);
    }

    #[test]
    fn a_fraction_below_half_stays_on_the_same_column_width_in_both_modes() {
        let config = UiConfig::default();
        let m = metrics(9.4, 20.0);
        assert_eq!(compute_cell_size(&config, &m, CellWidthMode::Compact).0, 9.0);
        assert_eq!(compute_cell_size(&config, &m, CellWidthMode::Relaxed).0, 9.0);
    }

    #[test]
    fn the_exact_half_boundary_rounds_away_from_zero_in_relaxed_mode() {
        let config = UiConfig::default();
        let m = metrics(9.5, 20.0);
        assert_eq!(compute_cell_size(&config, &m, CellWidthMode::Compact).0, 9.0);
        assert_eq!(compute_cell_size(&config, &m, CellWidthMode::Relaxed).0, 10.0);
    }

    #[test]
    fn both_modes_compute_the_same_cell_height() {
        let config = UiConfig::default();
        // 该偏好只控制列宽；高度必须逐位相同，否则行距会随模式漂移。
        for (advance, line_height) in [(9.6, 20.7), (7.5, 16.5), (12.2, 25.9)] {
            let m = metrics(advance, line_height);
            let compact = compute_cell_size(&config, &m, CellWidthMode::Compact);
            let relaxed = compute_cell_size(&config, &m, CellWidthMode::Relaxed);
            assert_eq!(compact.1, relaxed.1, "line_height {line_height} 的高度在两模式间漂移");
        }
    }

    #[test]
    fn both_modes_share_the_same_minimum_cell_width() {
        let config = UiConfig::default();
        // 退化度量（字体加载异常）不能产出 0 宽单元格——那会让网格除零。
        let m = metrics(0.3, 0.4);
        let compact = compute_cell_size(&config, &m, CellWidthMode::Compact);
        let relaxed = compute_cell_size(&config, &m, CellWidthMode::Relaxed);
        assert_eq!(compact.0, 1.0);
        assert_eq!(relaxed.0, 1.0);
        assert_eq!(compact.1, relaxed.1, "退化度量下高度也不得随模式漂移");
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn file_uri_tooltip_shows_decoded_path() {
        // `ls --hyperlink` percent-encodes CJK names; the tooltip must not.
        assert_eq!(
            strip_file_scheme("file:///D:/%E6%98%9F%E9%9B%B2/read%20me.txt"),
            "D:/星雲/read me.txt"
        );
        // Non-file URIs keep their encoding — it is part of their identity.
        assert_eq!(strip_file_scheme("https://a.b/c%20d"), "https://a.b/c%20d");
        // Malformed escapes and non-UTF-8 decodes survive verbatim.
        assert_eq!(percent_decode_lossy("100%"), "100%");
        assert_eq!(percent_decode_lossy("%zz"), "%zz");
        assert_eq!(percent_decode_lossy("%ff%fe"), "%ff%fe");
    }

    #[test]
    fn log_replay_commands_do_not_receive_terminal_query_answers() {
        for command in [
            "docker logs app",
            "docker compose logs -f api",
            "podman logs app",
            "kubectl logs pod/api",
            "journalctl -f -u nebula",
        ] {
            assert!(replays_untrusted_terminal_output(command), "{command}");
        }
        for command in ["docker run app", "kubectl exec pod -- sh", "cargo test", "nvim"] {
            assert!(!replays_untrusted_terminal_output(command), "{command}");
        }
    }

    #[test]
    fn popup_pad_counts_display_cells_and_drops_straddling_wide_chars() {
        assert_eq!(nebula_pad_to_cells("ab", 4), "ab  ");
        assert_eq!(nebula_pad_to_cells("目录", 4), "目录");
        // 第二个全宽字符放不进 3 格：丢弃并用空格补齐。
        assert_eq!(nebula_pad_to_cells("目录", 3), "目 ");
        assert_eq!(nebula_pad_to_cells("abcd", 3), "abc");
    }

    #[test]
    fn popup_label_elides_from_the_left() {
        assert_eq!(super::suggest_engine::elide_left("short", 10), "short");
        assert_eq!(super::suggest_engine::elide_left("abcdefgh", 5), "…efgh");
    }

    #[test]
    fn system_theme_snapshot_beats_a_stale_window_override() {
        assert_eq!(
            system_theme_snapshot(Some(WinitTheme::Dark), Some(WinitTheme::Light)),
            Some(WinitTheme::Dark)
        );
        assert_eq!(system_theme_snapshot(None, Some(WinitTheme::Light)), Some(WinitTheme::Light));
    }

    #[test]
    fn ssh_delete_undo_restores_saved_and_pinned_order() {
        let mut saved = strings(&["alpha", "target", "omega"]);
        let mut pinned = strings(&["target", "alpha"]);
        let mut hidden = strings(&["already-hidden"]);

        let snapshot =
            remove_ssh_host_from_lists("target", false, &mut saved, &mut pinned, &mut hidden);
        assert_eq!(snapshot, (Some(1), Some(0), false));
        assert_eq!(saved, strings(&["alpha", "omega"]));
        assert_eq!(pinned, strings(&["alpha"]));
        // A Nebula-managed host is deleted, not renamed to "hidden": nothing
        // may linger in the hidden section for it.
        assert_eq!(hidden, strings(&["already-hidden"]));

        restore_ssh_host_to_lists(
            "target",
            snapshot.0,
            snapshot.1,
            snapshot.2,
            &mut saved,
            &mut pinned,
            &mut hidden,
        );
        assert_eq!(saved, strings(&["alpha", "target", "omega"]));
        assert_eq!(pinned, strings(&["target", "alpha"]));
        assert_eq!(hidden, strings(&["already-hidden"]));
    }

    #[test]
    fn ssh_config_only_hide_is_fully_reversible() {
        let mut saved = Vec::new();
        let mut pinned = Vec::new();
        let mut hidden = Vec::new();

        let snapshot =
            remove_ssh_host_from_lists("config-alias", true, &mut saved, &mut pinned, &mut hidden);
        assert_eq!(snapshot, (None, None, false));
        assert_eq!(hidden, strings(&["config-alias"]));

        restore_ssh_host_to_lists(
            "config-alias",
            snapshot.0,
            snapshot.1,
            snapshot.2,
            &mut saved,
            &mut pinned,
            &mut hidden,
        );
        assert!(saved.is_empty());
        assert!(pinned.is_empty());
        assert!(hidden.is_empty());
    }

    /// A host that exists both as a saved entry and as a `~/.ssh/config`
    /// alias must be hidden on top of the saved-list removal, otherwise the
    /// config merge resurrects it on the next restart.
    #[test]
    fn ssh_delete_of_a_config_backed_saved_host_also_hides_the_alias() {
        let mut saved = strings(&["dual"]);
        let mut pinned = Vec::new();
        let mut hidden = Vec::new();

        let snapshot =
            remove_ssh_host_from_lists("dual", true, &mut saved, &mut pinned, &mut hidden);
        assert_eq!(snapshot, (Some(0), None, false));
        assert!(saved.is_empty());
        assert_eq!(hidden, strings(&["dual"]));

        restore_ssh_host_to_lists(
            "dual",
            snapshot.0,
            snapshot.1,
            snapshot.2,
            &mut saved,
            &mut pinned,
            &mut hidden,
        );
        assert_eq!(saved, strings(&["dual"]));
        assert!(hidden.is_empty());
    }

    #[test]
    fn asymmetric_bottom_reserve_recovers_rows_hidden_by_top_chrome() {
        let size = SizeInfo::new_fully_asymmetric(1000.0, 1000.0, 10.0, 20.0, 0.0, 0.0, 64.0, 16.0);
        assert_eq!(size.screen_lines(), 46);
        assert_eq!(size.padding_y(), 64.0);
        assert_eq!(size.padding_bottom(), 16.0);

        let old_symmetric = SizeInfo::new_asymmetric(1000.0, 1000.0, 10.0, 20.0, 0.0, 0.0, 64.0);
        assert_eq!(old_symmetric.screen_lines(), 43);
    }

    #[test]
    fn alternate_screen_padding_stays_inside_stacked_panes() {
        let window =
            SizeInfo::new_fully_asymmetric(1000.0, 700.0, 10.0, 20.0, 100.0, 20.0, 80.0, 20.0);
        let top =
            SizeInfo::new_fully_asymmetric(1000.0, 700.0, 10.0, 20.0, 100.0, 20.0, 80.0, 324.0);
        let bottom =
            SizeInfo::new_fully_asymmetric(1000.0, 700.0, 10.0, 20.0, 100.0, 20.0, 384.0, 20.0);

        assert_eq!(
            alt_screen_vertical_padding_bands(&window, &top, 56.0, 636.0),
            [Some((56.0, 24.0)), Some((360.0, 16.0))]
        );
        assert_eq!(
            alt_screen_vertical_padding_bands(&window, &bottom, 56.0, 636.0),
            [None, Some((664.0, 28.0))]
        );
    }

    #[test]
    fn missing_font_notice_can_be_dismissed() {
        let confirm =
            NebulaConfirm::InstallRequiredFont { directory: std::path::PathBuf::from("fonts") };

        assert!(confirm.can_dismiss());
    }

    #[test]
    fn tab_drop_ignores_scrolled_out_zero_rows() {
        // Storage indices 0..=2 and 13.. are hidden; only 3..=12 have screen
        // coordinates. A pointer within that window must never be shifted by
        // the hidden rows that the layout keeps for index stability.
        let visible: Vec<_> = (3..=12)
            .map(|index| (index, (0.0, 100.0 + (index - 3) as f32 * 30.0, 200.0, 24.0)))
            .collect();

        assert_eq!(super::tab_drop_index_from_visible_rows(5, 90.0, &visible, 16), 3);
        assert_eq!(super::tab_drop_index_from_visible_rows(5, 130.0, &visible, 16), 4);
        assert_eq!(super::tab_drop_index_from_visible_rows(5, 500.0, &visible, 16), 12);
        assert_eq!(super::tab_drop_index_from_visible_rows(5, 130.0, &[], 16), 5);
    }
}
