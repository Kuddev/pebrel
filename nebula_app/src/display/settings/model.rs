// ---- runtime settings store (`Nebula/nebula_settings.txt`) ----

use nebula_terminal::vte::ansi::CursorShape;

use crate::config::UiConfig;
use crate::display::color::Rgb;
use crate::display::keymap;
use crate::display::{
    AcceptKey, CompletionStyle, LanguagePreference, NebulaShell, NebulaTheme,
};
use crate::renderer::image::{BackgroundImageAlignment, BackgroundImageFit};

use super::{CellWidthMode, NewTabPosition, TabRevealMotion, density_parse};

pub(crate) struct NebulaRuntimeSettings {
    pub(crate) language: LanguagePreference,
    pub(crate) ghost: bool,
    pub(crate) accept: AcceptKey,
    /// Inline ghost remainder vs popup candidate list.
    pub(crate) completion_style: CompletionStyle,
    pub(crate) shell: NebulaShell,
    /// Raw default-shell id (`shell=<id>`), when the user picked a detected
    /// shell the 2-value `shell` enum can't represent (cmd, pwsh, nushell, a
    /// WSL distro). `None` = the enum value is authoritative. Written verbatim
    /// so `shell_detect::resolve_id` and the PTY layer both see the real id.
    pub(crate) shell_id: Option<String>,
    /// Default working directory for fresh terminal tabs. `None` inherits the
    /// focused pane (or the process cwd for the first window).
    pub(crate) startup_directory: Option<std::path::PathBuf>,
    pub(crate) font_family: String,
    /// Terminal font size in LOGICAL px (`None` = follow the config file).
    /// Persisted so the settings spinner and Ctrl+wheel zoom survive restarts.
    pub(crate) font_size: Option<f32>,
    /// Default cursor shape; escape sequences (vim, claude) still override.
    pub(crate) cursor_shape: CursorShape,
    /// Default-on: a static cursor reads as a hang ("没有活动感").
    pub(crate) cursor_blink: bool,
    /// 交互: 选中即复制（copyOnSelect）。关 = 右键复制。
    pub(crate) copy_on_select: bool,
    /// 全宽字形（CJK 等）在 bold run 里用 Regular 字形（粗体提亮不加粗）。
    /// 默认开：小字号下雅黑 Bold fallback 与 Regular 混排发闷（任务 #4）。
    pub(crate) cjk_bold_regular: bool,
    /// 旧壳不渲染顶部标签栏，但必须保留这个共享设置，否则它整体写回配置
    /// 时会把 GPUI 壳选择的布局抹掉。
    pub(crate) tabs_position: nebula_settings::TabsPositionName,
    pub(crate) tab_reveal: TabRevealMotion,
    /// 界面外观预设：标准 / 紧凑。
    pub(crate) density: crate::display::ui::tokens::Density,
    pub(crate) new_tab_position: NewTabPosition,
    pub(crate) cell_width_mode: CellWidthMode,
    pub(crate) fetch: bool,
    pub(crate) powerline: bool,
    /// Window close keeps the PTYs alive in the resident process (detach /
    /// re-attach session restore). Off = closing a window kills its shells.
    pub(crate) keep_session: bool,
    /// 高级·会话：启动时恢复上次的标签。
    pub(crate) restore_session: bool,
    /// 高级·会话：冷恢复时自动接续各 pane 的 AI 对话（claude/codex resume）。
    pub(crate) resume_ai: bool,
    /// 高级：常驻系统托盘图标。
    pub(crate) tray: bool,
    /// 窗口背景模糊。Windows 11 上是 Mica（见
    /// `display::window::apply_windows_backdrop`），macOS / Wayland 上走
    /// winit 自己的实现。默认开：纯 alpha 会让背景的高频细节直接透上来压在
    /// 字上，低透明度下文字就读不出来了，而这正是透明度最常被调低的场景。
    pub(crate) blur: bool,
    pub(crate) opacity: f32,
    pub(crate) background: Option<Rgb>,
    pub(crate) background_image: Option<String>,
    pub(crate) background_image_opacity: f32,
    pub(crate) background_image_fit: BackgroundImageFit,
    pub(crate) background_image_alignment: BackgroundImageAlignment,
    pub(crate) background_image_cover_chrome: bool,
    /// Chrome theme. Persisted so a restart keeps the chosen look AND the
    /// powerline bridge file gets rewritten with the right name on boot
    /// (it used to be reset to the default theme every launch).
    pub(crate) theme: NebulaTheme,
    /// Automatically choose the light/dark member of the selected theme
    /// family when the operating system appearance changes.
    pub(crate) follow_system_theme: bool,
    /// SSH host aliases pinned to the top of the sidebar's "SSH HOSTS"
    /// section (right-click a host row), in pinned order.
    pub(crate) pinned_hosts: Vec<String>,
    /// SSH destinations auto-saved after a successful typed `ssh` connection,
    /// most recent first (see `Display::nebula_save_ssh_host`).
    pub(crate) saved_hosts: Vec<String>,
    /// SSH aliases explicitly removed from the sidebar. This is separate from
    /// `saved_hosts` because entries discovered in `~/.ssh/config` would
    /// otherwise reappear on the very next merge.
    pub(crate) hidden_hosts: Vec<String>,
    /// 交互：允许拖拽调节左侧栏宽 / SSH HOSTS 分界高 / 右抽屉宽。默认关，
    /// 开启走一次确认框——宽度拖动会实时重排终端，性能敏感。
    pub(crate) panel_resize: bool,
    /// 左侧栏逻辑宽；[`super::SIDEBAR_W_LOGICAL`] 是默认值。
    pub(crate) sidebar_w: f32,
    /// 右抽屉逻辑宽；布局时仍钳在窗口 42%。
    pub(crate) drawer_w: f32,
    /// SSH HOSTS 停靠区高度覆盖（逻辑 px）；0 = 自动弹性规则。
    pub(crate) hosts_band: f32,
    /// User keybinding overrides, raw `(combo, action)` pairs in file order
    /// (spec 002). Kept verbatim so unknown-but-valid future actions survive a
    /// load/save cycle; `display::keymap::build_bindings` parses them.
    pub(crate) keybinds: Vec<(String, String)>,
    /// 快速终端全局切换键，和普通动作绑定分开持久化。
    pub(crate) quick_terminal_hotkey: String,
    /// SSH 出站代理（全局三态）。解析与连接决策都在 `crate::ssh_proxy`，
    /// 这里只负责三个键在 `nebula_settings.txt` 里的持久化往返——写文件是
    /// 整体重写，键不进这个结构体就会在下一次保存时被抹掉。
    pub(crate) ssh_proxy_mode: crate::ssh_proxy::ProxyMode,
    pub(crate) ssh_proxy_url: String,
    /// 绕过列表原文（逗号分隔），按用户输入原样保存；拆分归 `ssh_proxy`。
    pub(crate) ssh_proxy_no_proxy: String,
}

/// Load runtime UI settings from `Nebula/nebula_settings.txt`; defaults when
/// absent. Format is one `key=value` per line so power users can edit it while
/// the graphical settings page catches up.
pub(crate) fn nebula_settings_load(config: &UiConfig) -> NebulaRuntimeSettings {
    let path = nebula_settings::settings_path();
    let mut settings = NebulaRuntimeSettings {
        language: LanguagePreference::System,
        ghost: true,
        accept: AcceptKey::Both,
        completion_style: CompletionStyle::Inline,
        shell: NebulaShell::PowerShell,
        shell_id: None,
        startup_directory: None,
        font_family: config.font.normal().family.clone(),
        font_size: None,
        cursor_shape: CursorShape::Beam,
        cursor_blink: true,
        copy_on_select: true,
        cjk_bold_regular: true,
        tabs_position: nebula_settings::TabsPositionName::Sidebar,
        tab_reveal: TabRevealMotion::Slide,
        density: crate::display::ui::tokens::Density::Standard,
        new_tab_position: NewTabPosition::AfterCurrent,
        cell_width_mode: CellWidthMode::Compact,
        // Off by default: the welcome screen pipes a whole script through the
        // fresh shell and repaints on resize — real startup-latency cost on
        // the critical path (user ruling: startup speed outranks the art).
        fetch: false,
        powerline: true,
        // Off by default (user ruling 2026-07-12): a plain terminal should die
        // clean on close. Residency leaves shells running in the background,
        // which reads as "the app didn't really exit" — opt IN, not out.
        keep_session: false,
        restore_session: true,
        // 默认开：恢复布局却丢下正聊到一半的对话，等于只恢复了一半现场。
        // 关掉它仍恢复标签/分屏/目录，只是不再敲 resume。
        resume_ai: true,
        // 默认开：托盘是 agent 等待提醒的常驻出口（任务栏闪烁会被忽略、
        // toast 会过期）；不想要常驻图标的人在设置里关。
        tray: true,
        // Both shells use the shared opt-in material default.
        blur: nebula_settings::BlurModeName::default().enabled(),
        opacity: config.window_opacity(),
        background: None,
        background_image: None,
        background_image_opacity: 0.38,
        background_image_fit: BackgroundImageFit::default(),
        background_image_alignment: BackgroundImageAlignment::default(),
        background_image_cover_chrome: false,
        theme: NebulaTheme::default(),
        // Preserve existing installations: automatic switching is opt-in so
        // an update never replaces an explicitly selected theme unexpectedly.
        follow_system_theme: false,
        pinned_hosts: Vec::new(),
        saved_hosts: Vec::new(),
        hidden_hosts: Vec::new(),
        panel_resize: false,
        sidebar_w: crate::display::SIDEBAR_W_LOGICAL,
        drawer_w: crate::display::side_panel::PANEL_W_LOGICAL,
        hosts_band: 0.0,
        keybinds: Vec::new(),
        quick_terminal_hotkey: keymap::DEFAULT_QUICK_TERMINAL_HOTKEY.to_owned(),
        ssh_proxy_mode: crate::ssh_proxy::ProxyMode::Off,
        ssh_proxy_url: String::new(),
        ssh_proxy_no_proxy: String::new(),
    };
    if let Ok(data) = std::fs::read_to_string(path) {
        for line in data.lines() {
            match line.split_once('=') {
                Some(("language", v)) => {
                    if let Some(language) = LanguagePreference::parse(v) {
                        settings.language = language;
                    }
                },
                Some(("ghost", v)) => settings.ghost = v.trim() != "0",
                Some(("theme", v)) => {
                    if let Some(theme) = NebulaTheme::from_prompt_name(v.trim()) {
                        settings.theme = theme;
                    }
                },
                Some(("accept", "right")) => settings.accept = AcceptKey::Right,
                Some(("accept", "tab")) => settings.accept = AcceptKey::Tab,
                Some(("accept", "both")) => settings.accept = AcceptKey::Both,
                Some(("completion_style", v)) => {
                    if let Some(style) = CompletionStyle::from_settings(v) {
                        settings.completion_style = style;
                    }
                },
                Some(("shell" | "executor", v)) => {
                    let v = v.trim();
                    if let Some(shell) = NebulaShell::from_settings(v) {
                        settings.shell = shell;
                    }
                    // Preserve the raw id for detected shells the enum can't
                    // represent (cmd, pwsh, nushell, wsl:<distro>); the enum
                    // still tracks the PTY-integrated executor family so the
                    // prompt bootstrap picks the right base.
                    if !v.is_empty() {
                        settings.shell_id = Some(v.to_owned());
                    }
                },
                Some(("font_family", v)) => {
                    let family = v.trim();
                    if !family.is_empty() {
                        settings.font_family = family.to_owned();
                    }
                },
                Some(("font_size", v)) => {
                    if let Ok(size) = v.trim().parse::<f32>() {
                        settings.font_size = Some(size.clamp(6.0, 72.0));
                    }
                },
                Some(("cursor_shape", v)) => {
                    if let Some(shape) = parse_cursor_shape(v) {
                        settings.cursor_shape = shape;
                    }
                },
                Some(("cursor_blink", v)) => settings.cursor_blink = parse_bool(v, true),
                Some(("copy_on_select", v)) => settings.copy_on_select = parse_bool(v, true),
                Some(("cjk_bold_regular", v)) => settings.cjk_bold_regular = parse_bool(v, true),
                Some(("tabs_position", v)) => {
                    settings.tabs_position =
                        nebula_settings::TabsPositionName::from_settings(v).unwrap_or_default();
                },
                Some(("tab_reveal", v)) => {
                    settings.tab_reveal = TabRevealMotion::parse(v).unwrap_or_default();
                },
                Some(("density", v)) => {
                    settings.density = density_parse(v).unwrap_or_default();
                },
                Some(("new_tab_position", v)) => {
                    settings.new_tab_position = NewTabPosition::parse(v).unwrap_or_default();
                },
                Some(("cell_width_mode", v)) => {
                    settings.cell_width_mode = CellWidthMode::parse(v).unwrap_or_default();
                },
                Some(("startup_directory", v)) => {
                    let path = std::path::PathBuf::from(v.trim());
                    if path.is_dir() {
                        settings.startup_directory = Some(path);
                    }
                },
                Some(("fetch", v)) => settings.fetch = parse_bool(v, true),
                Some(("powerline", v)) => settings.powerline = parse_bool(v, true),
                Some(("keep_session", v)) => settings.keep_session = parse_bool(v, false),
                Some(("restore_session", v)) => settings.restore_session = parse_bool(v, true),
                Some(("resume_ai", v)) => settings.resume_ai = parse_bool(v, true),
                Some(("tray", v)) => settings.tray = parse_bool(v, true),
                Some(("panel_resize", v)) => settings.panel_resize = parse_bool(v, false),
                Some(("sidebar_w", v)) => {
                    if let Ok(w) = v.trim().parse::<f32>() {
                        settings.sidebar_w =
                            w.clamp(crate::display::SIDEBAR_W_MIN, crate::display::SIDEBAR_W_MAX);
                    }
                },
                Some(("drawer_w", v)) => {
                    if let Ok(w) = v.trim().parse::<f32>() {
                        settings.drawer_w =
                            w.clamp(crate::display::DRAWER_W_MIN, crate::display::DRAWER_W_MAX);
                    }
                },
                Some(("hosts_band", v)) => {
                    if let Ok(h) = v.trim().parse::<f32>() {
                        settings.hosts_band =
                            if h > 0.0 { h.max(crate::display::HOSTS_BAND_MIN) } else { 0.0 };
                    }
                },
                Some(("blur", v)) => settings.blur = parse_blur_enabled(v),
                Some(("opacity", v)) => {
                    if let Ok(opacity) = v.trim().parse::<f32>() {
                        settings.opacity = opacity.clamp(0.0, 1.0);
                    }
                },
                Some(("background", v)) => settings.background = parse_hex_rgb(v.trim()),
                Some(("background_image", v)) => {
                    let v = v.trim();
                    settings.background_image = (!v.is_empty()).then(|| v.to_owned());
                },
                Some(("background_image_opacity", v)) => {
                    if let Ok(opacity) = v.trim().parse::<f32>() {
                        settings.background_image_opacity = opacity.clamp(0.0, 1.0);
                    }
                },
                Some(("background_image_fit", v)) => {
                    if let Some(fit) = BackgroundImageFit::parse(v) {
                        settings.background_image_fit = fit;
                    }
                },
                Some(("background_image_alignment", v)) => {
                    if let Some(alignment) = BackgroundImageAlignment::parse(v) {
                        settings.background_image_alignment = alignment;
                    }
                },
                Some(("background_image_cover_chrome", v)) => {
                    settings.background_image_cover_chrome = parse_bool(v, false);
                },
                Some(("pinned_hosts", v)) => {
                    settings.pinned_hosts = v
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect();
                },
                Some(("saved_hosts", v)) => {
                    settings.saved_hosts = v
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect();
                },
                Some(("follow_system_theme", v)) => {
                    settings.follow_system_theme = parse_bool(v, false)
                },
                Some(("hidden_hosts", v)) => {
                    settings.hidden_hosts = v
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect();
                },
                Some(("keybind", v)) => {
                    // `keybind=<combo>:<action>`；解析验证归 keymap 模块，这里
                    // 只收原文——非法行在构建绑定表时静默丢弃。
                    if let Some((combo, action)) = v.split_once(':') {
                        let (combo, action) = (combo.trim(), action.trim());
                        if !combo.is_empty() && !action.is_empty() {
                            settings.keybinds.push((combo.to_lowercase(), action.to_owned()));
                        }
                    }
                },
                Some(("quick_terminal_hotkey", v)) => {
                    let value = v.trim();
                    if value.parse::<global_hotkey::hotkey::HotKey>().is_ok() {
                        settings.quick_terminal_hotkey = value.to_owned();
                    }
                },
                Some(("ssh_proxy_mode", v)) => {
                    settings.ssh_proxy_mode = crate::ssh_proxy::ProxyMode::parse(v);
                },
                Some(("ssh_proxy_url", v)) => settings.ssh_proxy_url = v.trim().to_owned(),
                Some(("ssh_proxy_no_proxy", v)) => {
                    settings.ssh_proxy_no_proxy = v.trim().to_owned();
                },
                _ => {},
            }
        }
    }
    settings
}

fn parse_bool(value: &str, default: bool) -> bool {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

/// Project the shared material parser into the legacy shell's boolean switch.
fn parse_blur_enabled(value: &str) -> bool {
    nebula_settings::BlurModeName::from_settings(value).unwrap_or_default().enabled()
}

fn parse_cursor_shape(value: &str) -> Option<CursorShape> {
    match value.trim().to_ascii_lowercase().as_str() {
        "block" => Some(CursorShape::Block),
        "beam" | "bar" => Some(CursorShape::Beam),
        "underline" => Some(CursorShape::Underline),
        "hollow" => Some(CursorShape::HollowBlock),
        _ => None,
    }
}

pub(crate) fn cursor_shape_settings_value(shape: CursorShape) -> &'static str {
    match shape {
        CursorShape::Beam => "beam",
        CursorShape::Underline => "underline",
        CursorShape::HollowBlock => "hollow",
        CursorShape::Block | CursorShape::Hidden => "block",
    }
}

pub(crate) fn parse_hex_rgb(value: &str) -> Option<Rgb> {
    nebula_settings::parse_hex_rgb(value).map(|[r, g, b]| Rgb::new(r, g, b))
}

pub(crate) fn format_hex_rgb(rgb: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.r, rgb.g, rgb.b)
}