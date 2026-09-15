//! Settings special tab for Nebula's runtime appearance and completion settings.
//!
//! Mirrors the `command_palette` split, but goes one step further: besides the
//! *model* (sections, hit-testing, geometry, and the `nebula_settings.txt`
//! runtime store) this module also owns the panel's *rendering* — both the
//! background [`push_quads`] and the [`draw_text`] labels — so the giant
//! `display::mod` no longer carries the settings UI. The input layer stays the
//! only place that mutates state, reaching the `Display` methods that wrap this
//! model; rendering reads a snapshot [`SettingsView`] handed in each frame.
//!
//! Being a descendant module of `display`, this file can freely use the parent's
//! private helpers (`contains_rect`, `truncate_tab_label`, `nebula_data_dir`,
//! `NebulaTheme::palette`, `AcceptKey`, …) via `super::` — no visibility
//! churn needed in `mod.rs`.

use unicode_width::UnicodeWidthChar;

use nebula_terminal::vte::ansi::CursorShape;

use crate::config::UiConfig;
use crate::display::color::Rgb;
use crate::encrypted_backup::BackupSelection;
use crate::renderer::image::{BackgroundImageAlignment, BackgroundImageFit};
use crate::renderer::ui::{Rgba, UiQuad};
use crate::renderer::{GlyphCache, Renderer};

pub use super::background_color_model::BgPickerPart;
pub(crate) use super::background_color_model::{BACKGROUND_SWATCHES, hsv_to_rgb, rgb_to_hsv};
use super::keymap;
pub(crate) use super::network_proxy_model::{
    MANUAL_PROXY_PROTOCOL_OPTIONS, ManualProxyProtocol, ProxyTestStatus, manual_proxy_parts,
    manual_proxy_value,
};
use super::ui::theme::Skin;
use super::ui::{icons, os_icons, surface, text_field, tokens, widgets};
use super::{
    AcceptKey, CompletionStyle, LanguagePreference, NebulaShell, NebulaTheme, SizeInfo, UiLanguage,
    chrome_settings_button_rect, contains_rect, nebula_data_dir, truncate_tab_label,
};

// Visual language: one flat panel color, one hairline, three text grays, ONE
// accent — hierarchy comes from typography and spacing. Every color is a
// [`Skin`] token from `display::theme` (single source of truth), so the page
// flips correctly between the light and dark theme families.

/// WebDAV 同步还处于内部迭代阶段。保留状态、持久化与后端实现，但在交互闭环
/// 完成前不向用户暴露入口；集中守门可避免绘制、命中和滚动高度各自遗漏。
const SHOW_WEBDAV_SYNC_SETTINGS: bool = false;

/// 备份页对用户开放（2026-08-13）：本地加密导出/恢复此前已完整，本次加上
/// 多协议远程备份（目录/WebDAV/S3/SFTP，见 `crate::backup_remote`）后闭环
/// 成立。恢复的可视化预览仍是后续增强，不再作为门禁。
const SHOW_BACKUP_SETTINGS: bool = true;

/// Sidebar sections of the settings panel. Deliberately small: only sections
/// with real functionality behind them are listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NebulaSettingsSection {
    /// Themes, custom colors, wallpaper, cursor and window opacity.
    #[default]
    Appearance,
    /// Completion behaviour plus the raw `nebula_settings.txt` config file.
    Profiles,
    /// OpenAI-compatible AI providers and their OS-backed API keys.
    Providers,
    /// Saved SSH destinations and hidden-host recovery.
    Ssh,
    /// Outbound proxy policy for SSH connections.
    Proxy,
    /// Selection/clipboard behaviour (the 「交互」 page).
    Interaction,
    /// Read-only shortcut sheet + pointer to `[[keyboard.bindings]]` remapping.
    Keymap,
    /// Power-user switches (session residency on close, …).
    Advanced,
    /// Password-protected export and restore of Nebula-owned data.
    Backup,
}

impl NebulaSettingsSection {
    fn label(self, language: UiLanguage) -> &'static str {
        match self {
            Self::Appearance => language.pick("外观", "Appearance"),
            Self::Profiles => language.pick("配置文件", "Profiles"),
            Self::Providers => language.pick("供应商", "Providers"),
            Self::Ssh => "SSH",
            Self::Proxy => language.pick("网络", "Network"),
            Self::Interaction => language.pick("交互", "Interaction"),
            Self::Keymap => language.pick("按键映射", "Key bindings"),
            Self::Advanced => language.pick("高级", "Advanced"),
            Self::Backup => language.pick("备份", "Backup"),
        }
    }
}

fn nav_icon(section: NebulaSettingsSection) -> icons::SettingsNavIcon {
    match section {
        NebulaSettingsSection::Appearance => icons::SettingsNavIcon::Appearance,
        NebulaSettingsSection::Profiles => icons::SettingsNavIcon::Profiles,
        NebulaSettingsSection::Providers => icons::SettingsNavIcon::Providers,
        NebulaSettingsSection::Ssh => icons::SettingsNavIcon::Ssh,
        NebulaSettingsSection::Proxy => icons::SettingsNavIcon::Proxy,
        NebulaSettingsSection::Interaction => icons::SettingsNavIcon::Interaction,
        NebulaSettingsSection::Keymap => icons::SettingsNavIcon::Keymap,
        NebulaSettingsSection::Advanced => icons::SettingsNavIcon::Advanced,
        NebulaSettingsSection::Backup => icons::SettingsNavIcon::Backup,
    }
}

/// Shortcut sheet shown in 设置→按键映射. Editable rows live in
/// [`keymap::EDITABLE_ACTIONS`]; the read-only extras in
/// [`keymap::READONLY_ROWS`] (spec 002).

/// Which independently draggable opacity control is being adjusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsOpacityTarget {
    Terminal,
    BackgroundImage,
}

/// Which inline dropdown (combobox) is currently expanded. At most one at a
/// time; the option list floats over later rows instead of pushing them down.
/// 用户范式（2026-07-23）：凡是多选项的设置一律做成内嵌下拉框，
/// 不再用"点击循环切换"——所有选项必须先可见再选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsDropdown {
    Shell,
    Font,
    BackgroundFit,
    BackgroundAlignment,
    Language,
    Accept,
    CompletionStyle,
    CursorShape,
    TabReveal,
    /// 备份：远程备份协议（关闭/目录/WebDAV/S3/SFTP）。
    BackupProtocol,
    /// 代理：SSH 连接代理模式（关闭/系统/自定义）。
    SshProxyMode,
    /// 网络→指定代理→手动填写：地址协议（SOCKS5/HTTP）。
    SshProxyProtocol,
    /// 网络→指定代理→SSH 跳板：已保存主机的选择下拉。
    SshJumpHost,
    /// 外观密度（标准/紧凑）。
    Density,
    NewTabPosition,
    CellWidthMode,
    /// 背景色：色板网格 + 16 进制输入的专用浮层（不是通用行列表）。
    BackgroundColor,
}

pub(super) const BACKGROUND_FIT_OPTIONS: [BackgroundImageFit; 4] = [
    BackgroundImageFit::Fill,
    BackgroundImageFit::Uniform,
    BackgroundImageFit::UniformToFill,
    BackgroundImageFit::None,
];

pub(super) const BACKGROUND_ALIGNMENT_OPTIONS: [BackgroundImageAlignment; 9] = [
    BackgroundImageAlignment::TopLeft,
    BackgroundImageAlignment::Top,
    BackgroundImageAlignment::TopRight,
    BackgroundImageAlignment::Left,
    BackgroundImageAlignment::Center,
    BackgroundImageAlignment::Right,
    BackgroundImageAlignment::BottomLeft,
    BackgroundImageAlignment::Bottom,
    BackgroundImageAlignment::BottomRight,
];

pub(super) const LANGUAGE_OPTIONS: &[LanguagePreference] = LanguagePreference::ALL;

pub(super) const ACCEPT_OPTIONS: [AcceptKey; 3] =
    [AcceptKey::Both, AcceptKey::Tab, AcceptKey::Right];

pub(super) const COMPLETION_STYLE_OPTIONS: [CompletionStyle; 2] =
    [CompletionStyle::Inline, CompletionStyle::Popup];

pub(super) const TAB_REVEAL_OPTIONS: [TabRevealMotion; 2] =
    [TabRevealMotion::Slide, TabRevealMotion::Instant];

/// 远程备份协议下拉的行序。`display` 的 `set_backup_protocol_option`
/// 按同一数组下标持久化，两侧永不错位。
pub(crate) const BACKUP_PROTOCOL_OPTIONS: [crate::backup_remote::BackupProtocol; 5] = [
    crate::backup_remote::BackupProtocol::Off,
    crate::backup_remote::BackupProtocol::Folder,
    crate::backup_remote::BackupProtocol::WebDav,
    crate::backup_remote::BackupProtocol::S3,
    crate::backup_remote::BackupProtocol::Sftp,
];

/// 下拉行序即为这里的顺序；连接时的真正决策在 `crate::ssh_proxy`。
pub(super) const SSH_PROXY_MODE_OPTIONS: [crate::ssh_proxy::ProxyMode; 3] = [
    crate::ssh_proxy::ProxyMode::Off,
    crate::ssh_proxy::ProxyMode::System,
    crate::ssh_proxy::ProxyMode::Custom,
];

fn ssh_proxy_mode_label(mode: crate::ssh_proxy::ProxyMode, language: UiLanguage) -> &'static str {
    match mode {
        crate::ssh_proxy::ProxyMode::Off => language.pick("不使用代理", "No proxy"),
        crate::ssh_proxy::ProxyMode::System => language.pick("跟随系统", "Follow system"),
        crate::ssh_proxy::ProxyMode::Custom => language.pick("自定义代理", "Custom proxy"),
    }
}

fn manual_proxy_protocol_label(
    protocol: ManualProxyProtocol,
    _language: UiLanguage,
) -> &'static str {
    match protocol {
        ManualProxyProtocol::Socks5 => "SOCKS5",
        ManualProxyProtocol::Http => "HTTP",
    }
}

/// 设置页的悬停层使用当前主题的 accent，而不是固定的灰色或另一套绿色。
/// 透明度在浅色/深色主题分别取值，保证两种底色上都只是轻微提示。
fn settings_skin(theme: NebulaTheme) -> Skin {
    let mut skin = theme.skin();
    let (hover_alpha, strong_alpha) = if skin.is_light { (22, 34) } else { (30, 46) };
    skin.hover = Rgba::new(skin.accent.r, skin.accent.g, skin.accent.b, hover_alpha);
    skin.hover_strong = Rgba::new(skin.accent.r, skin.accent.g, skin.accent.b, strong_alpha);
    skin
}

/// 网络页几何的动态输入：模式与子模式决定下方内容的种类与高度，覆盖行数
/// 决定「每主机覆盖」列表的高度。命中 / 绘制 / 滚动上限三方共用同一份，
/// 保证控件与点击区不漂移（组件化范式：几何同源）。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ProxyChoice {
    Detected(usize),
    #[default]
    Manual,
    Jump,
    Command,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ProxyPaneState {
    pub mode: crate::ssh_proxy::ProxyMode,
    /// 指定代理列表当前选中项。发现项的下标对应 `local_proxies` 快照。
    pub choice: ProxyChoice,
    pub found_count: usize,
    pub scanning: bool,
    /// 设置了每主机链路覆盖的主机数（profiles.json 的 `proxy` 字段）。
    pub override_count: usize,
}

/// 按键映射页几何的动态输入：搜索过滤后的每组可见行数 + 冲突提示占位。
/// 数组与 [`keymap::GROUPS`] 对齐（长度由 keymap 侧测试锁定为 5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeymapPaneState {
    pub visible: [u8; 5],
    pub readonly_visible: u8,
    /// 是否显示冲突提示条（占一段版面，无冲突不留空洞）。
    pub clash: bool,
}

/// 每主机覆盖行的摘要：`direct` / `jump:` / 代理 URL → 人话。解析失败时
/// 原样展示——错值也该被看见，而不是被摘要藏起来。
pub(super) fn ssh_proxy_override_summary(value: &str, language: UiLanguage) -> String {
    let value = value.trim();
    if value.eq_ignore_ascii_case("direct") {
        return language.pick("不走代理", "Direct (no proxy)").to_owned();
    }
    match crate::ssh_proxy::ProxyLink::parse(value) {
        Ok(crate::ssh_proxy::ProxyLink::Jump(target)) => {
            format!(
                "{}{}{}",
                language.pick("SSH 跳板 · 经 ", "Jump host · via "),
                target,
                language.pick(" 转发", "")
            )
        },
        Ok(crate::ssh_proxy::ProxyLink::Server(server)) => {
            format!("{} · {}", language.pick("指定代理", "Custom proxy"), server.display())
        },
        Ok(crate::ssh_proxy::ProxyLink::Command(_)) => language
            .pick("自定义命令 · stdin/stdout 转发", "Custom command · stdin/stdout")
            .to_owned(),
        Err(_) => value.to_owned(),
    }
}

pub(super) const DENSITY_OPTIONS: [super::ui::tokens::Density; 2] =
    [super::ui::tokens::Density::Standard, super::ui::tokens::Density::Compact];
pub(super) const NEW_TAB_POSITION_OPTIONS: [NewTabPosition; 2] =
    [NewTabPosition::AfterCurrent, NewTabPosition::End];
pub(super) const CELL_WIDTH_MODE_OPTIONS: [CellWidthMode; 2] =
    [CellWidthMode::Compact, CellWidthMode::Relaxed];

/// Order mirrors the appearance page the user referenced.
pub(super) const CURSOR_SHAPE_OPTIONS: [CursorShape; 4] =
    [CursorShape::Beam, CursorShape::Underline, CursorShape::Block, CursorShape::HollowBlock];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TabRevealMotion {
    #[default]
    Slide,
    Instant,
}

impl TabRevealMotion {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "slide" => Some(Self::Slide),
            "instant" => Some(Self::Instant),
            _ => None,
        }
    }

    fn settings_value(self) -> &'static str {
        match self {
            Self::Slide => "slide",
            Self::Instant => "instant",
        }
    }
}

fn tab_reveal_label(motion: TabRevealMotion, language: UiLanguage) -> &'static str {
    match motion {
        TabRevealMotion::Slide => language.pick("滑动", "Slide"),
        TabRevealMotion::Instant => language.pick("立即", "Instant"),
    }
}

pub(super) fn density_label(
    density: super::ui::tokens::Density,
    language: UiLanguage,
) -> &'static str {
    match density {
        super::ui::tokens::Density::Standard => language.pick("标准", "Standard"),
        super::ui::tokens::Density::Compact => language.pick("紧凑", "Compact"),
    }
}

pub(super) fn density_parse(value: &str) -> Option<super::ui::tokens::Density> {
    match value.trim().to_ascii_lowercase().as_str() {
        "standard" => Some(super::ui::tokens::Density::Standard),
        "compact" => Some(super::ui::tokens::Density::Compact),
        _ => None,
    }
}

pub(super) fn density_settings_value(density: super::ui::tokens::Density) -> &'static str {
    match density {
        super::ui::tokens::Density::Standard => "standard",
        super::ui::tokens::Density::Compact => "compact",
    }
}

/// 新标签插入策略：真正创建标签时，新标签在标签顺序中的落点。
/// 上游兼容默认是紧邻当前标签之后。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum NewTabPosition {
    #[default]
    AfterCurrent,
    End,
}

impl NewTabPosition {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "after_current" => Some(Self::AfterCurrent),
            "end" => Some(Self::End),
            _ => None,
        }
    }

    fn settings_value(self) -> &'static str {
        match self {
            Self::AfterCurrent => "after_current",
            Self::End => "end",
        }
    }
}

fn new_tab_position_label(position: NewTabPosition, language: UiLanguage) -> &'static str {
    match position {
        NewTabPosition::AfterCurrent => language.pick("当前标签之后", "After current"),
        NewTabPosition::End => language.pick("列表末尾", "End"),
    }
}

/// 单元格宽度模式：终端把字体的非整数设计宽度转换为整像素列宽的方式。
/// 「紧凑」保持上游的向下取整并作为兼容默认；「宽松」采用最接近整数取整，
/// 补足向下取整丢掉的那一像素。它只影响列宽，不改变单元格高度、
/// 字形比例或原生界面排版。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum CellWidthMode {
    #[default]
    Compact,
    Relaxed,
}

impl CellWidthMode {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "compact" => Some(Self::Compact),
            "relaxed" => Some(Self::Relaxed),
            _ => None,
        }
    }

    fn settings_value(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Relaxed => "relaxed",
        }
    }
}

fn cell_width_mode_label(mode: CellWidthMode, language: UiLanguage) -> &'static str {
    match mode {
        CellWidthMode::Compact => language.pick("紧凑", "Compact"),
        CellWidthMode::Relaxed => language.pick("宽松", "Relaxed"),
    }
}

pub(super) fn cursor_shape_label(shape: CursorShape, language: UiLanguage) -> &'static str {
    match shape {
        CursorShape::Beam => language.pick("条形（│）", "Bar (│)"),
        CursorShape::Underline => language.pick("下划线（_）", "Underscore (_)"),
        CursorShape::Block => language.pick("实心框（█）", "Filled box (█)"),
        CursorShape::HollowBlock => language.pick("空心框（□）", "Empty box (□)"),
        CursorShape::Hidden => language.pick("隐藏", "Hidden"),
    }
}

fn accept_label(accept: AcceptKey, language: UiLanguage) -> &'static str {
    match accept {
        AcceptKey::Right => language.pick("右方向键", "Right arrow"),
        AcceptKey::Tab => "Tab",
        AcceptKey::Both => language.pick("Tab 或右方向键", "Tab or Right arrow"),
    }
}

fn completion_style_label(style: CompletionStyle, language: UiLanguage) -> &'static str {
    match style {
        CompletionStyle::Inline => language.pick("行内灰字", "Inline ghost"),
        CompletionStyle::Popup => language.pick("弹窗列表", "Popup list"),
    }
}

fn backup_protocol_label(
    protocol: crate::backup_remote::BackupProtocol,
    language: UiLanguage,
) -> &'static str {
    use crate::backup_remote::BackupProtocol;
    match protocol {
        BackupProtocol::Off => language.pick("关闭", "Off"),
        BackupProtocol::Folder => language.pick("本地 / 网络目录", "Local / network folder"),
        BackupProtocol::WebDav => "WebDAV",
        BackupProtocol::S3 => language.pick("S3 兼容存储", "S3-compatible storage"),
        BackupProtocol::Sftp => language.pick("SFTP（SSH 主机）", "SFTP (SSH host)"),
    }
}

/// 远程备份输入行的标签（槽位语义随协议变化）。
fn backup_remote_field_label(
    protocol: crate::backup_remote::BackupProtocol,
    index: usize,
    language: UiLanguage,
) -> &'static str {
    use crate::backup_remote::BackupProtocol;
    match (protocol, index) {
        (BackupProtocol::Folder, 0) => language.pick("备份目录", "Backup folder"),
        (BackupProtocol::WebDav, 0) => language.pick("目录 URL", "Directory URL"),
        (BackupProtocol::WebDav, 1) => language.pick("用户名", "Username"),
        (BackupProtocol::WebDav, 2) => language.pick("WebDAV 密码", "WebDAV password"),
        (BackupProtocol::S3, 0) => "Endpoint",
        (BackupProtocol::S3, 1) => language.pick("区域", "Region"),
        (BackupProtocol::S3, 2) => language.pick("存储桶 / 前缀", "Bucket / prefix"),
        (BackupProtocol::S3, 3) => "Access Key",
        (BackupProtocol::S3, 4) => "Secret Key",
        (BackupProtocol::Sftp, 0) => language.pick("SSH 目标", "SSH destination"),
        (BackupProtocol::Sftp, 1) => language.pick("远端目录", "Remote directory"),
        _ => "",
    }
}

/// 远程备份输入框的空值占位示例。
fn backup_remote_field_placeholder(
    protocol: crate::backup_remote::BackupProtocol,
    index: usize,
    language: UiLanguage,
) -> &'static str {
    use crate::backup_remote::BackupProtocol;
    match (protocol, index) {
        (BackupProtocol::Folder, 0) => r"D:\Backups\Nebula 或 \\nas\share\nebula",
        (BackupProtocol::WebDav, 0) => "https://dav.example.com/nebula/",
        (BackupProtocol::WebDav, 1) => language.pick("WebDAV 用户名", "WebDAV username"),
        (BackupProtocol::S3, 0) => "https://s3.us-east-1.amazonaws.com",
        (BackupProtocol::S3, 1) => "us-east-1",
        (BackupProtocol::S3, 2) => "my-bucket/nebula",
        (BackupProtocol::S3, 3) => "AKIA…",
        (BackupProtocol::Sftp, 0) => "user@host[:port]",
        (BackupProtocol::Sftp, 1) => "/home/user/backups",
        _ => language.pick("未设置", "Not set"),
    }
}

/// 远程备份输入框的展示内容：`(文本, 是否占位, 列数)`。密文槽显示为掩码
/// 点；超宽截尾部（编辑总发生在末尾）。列数供 caret 定位。
fn backup_remote_input_display(
    view: &SettingsView,
    index: usize,
    max_cols: usize,
) -> (String, bool, usize) {
    let language = view.language;
    let protocol = view.backup_protocol;
    let secret = crate::backup_remote::secret_field(protocol) == Some(index);
    let raw = &view.backup_remote_inputs[index];
    if raw.is_empty() {
        let text = if secret && view.backup_remote_secret_set {
            language.pick("已保存（输入以更换）", "Saved (type to replace)").to_owned()
        } else {
            backup_remote_field_placeholder(protocol, index, language).to_owned()
        };
        return (text, true, 0);
    }
    if secret {
        let dots = raw.chars().count().min(24);
        return ("●".repeat(dots), false, dots);
    }
    let (text, cols) = text_tail(raw, max_cols);
    (text, false, cols)
}

fn language_label(preference: LanguagePreference, language: UiLanguage) -> &'static str {
    match preference {
        LanguagePreference::System => language.pick("跟随系统", "Follow system"),
        _ => preference.shared().native_name(),
    }
}

/// Hit result for the top-left Nebula settings affordance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsHit {
    None,
    Toggle,
    Panel,
    Nav(NebulaSettingsSection),
    Theme(NebulaTheme),
    Language(LanguagePreference),
    SystemThemeToggle,
    GhostToggle,
    AcceptCycle,
    /// Completion style combobox trigger + its expanded option rows.
    CompletionStyleCycle,
    CompletionStyleOption(usize),
    ShellCycle,
    StartupDirectory,
    StartupDirectoryClear,
    /// One of the expanded shell picker rows (index into detected_shells).
    ShellPickerRow(usize),
    FontCycle,
    /// Imported-font picker rows; the final row is always "导入字体…".
    FontPickerRow(usize),
    /// 字体弹层顶部的搜索框。点它是定位光标，不是关掉弹层。
    FontSearchField,
    /// Font-size spinner steppers on the "字号" row.
    FontSizeUp,
    FontSizeDown,
    /// Cursor group: shape dropdown + its option rows, and the blink toggle.
    CursorShapeDropdown,
    CursorShapeOption(usize),
    CursorBlinkToggle,
    /// 交互: copy-on-select toggle row.
    CopyOnSelectToggle,
    /// 交互: 拖拽调节左侧栏宽 / 右抽屉宽。开启走确认框（reflow 开销）。
    /// SSH HOSTS 分界高度不归它管，始终可拖。
    PanelResizeToggle,
    /// 交互: 全宽字形 bold run 用 Regular 字形（粗体提亮不加粗）。
    CjkBoldToggle,
    TabRevealDropdown,
    TabRevealOption(usize),
    DensityDropdown,
    DensityOption(usize),
    NewTabPositionDropdown,
    NewTabPositionOption(usize),
    CellWidthModeDropdown,
    CellWidthModeOption(usize),
    /// Language combobox trigger (options resolve to [`SettingsHit::Language`]).
    LanguageDropdown,
    /// Expanded dropdown option rows for the cycle-style settings.
    AcceptOption(usize),
    FitOption(usize),
    AlignOption(usize),
    /// Restore one address from the persistent hidden-host list.
    RestoreHiddenSsh(usize),
    /// SSH 主机行本体：不是可点动作，只承载 hover 底色并让右缘三枚图标显形。
    /// 2026-08-11 用户裁定：静态那一行只留身份信息，动作藏进 hover。
    SshHostRow(usize),
    /// SSH settings page: connect to a saved destination.
    SshHostConnect(usize),
    /// SSH settings page: edit a saved destination.
    SshHostEdit(usize),
    /// SSH settings page: delete a saved destination (config aliases are
    /// hidden instead — Nebula never edits ~/.ssh/config).
    SshHostDelete(usize),
    /// SSH settings page: re-read ~/.ssh/config immediately.
    SshImportConfig,
    /// SSH settings page: open the existing SSH editor for a new host.
    SshAddHost,
    /// AI provider management: preset/add, select, edit and persist actions.
    ProviderAdd,
    ProviderRow(usize),
    ProviderField(usize),
    ProviderSave,
    ProviderTest,
    ProviderDelete,
    ProviderEnableToggle(usize),
    ProviderCodexGoalsToggle,
    ProviderCodexRemoteToggle,
    ProviderApplyCodex,
    FetchToggle,
    PowerlineToggle,
    BlurToggle,
    OpacitySlider,
    BackgroundColor,
    /// 背景色浮层：真调色盘的饱和度/明度面（按下开始拖拽取色）。
    BackgroundSvPlane,
    /// 背景色浮层：色相横条。
    BackgroundHueBar,
    /// 背景色浮层：色板网格里的一格。
    BackgroundSwatch(usize),
    /// 背景色浮层：16 进制输入框。
    BackgroundHexInput,
    /// 背景色浮层内部的空白（吞掉点击且不关闭浮层）。
    BackgroundPopupPanel,
    BackgroundImage,
    BackgroundImageClear,
    BackgroundImageFit,
    BackgroundImageAlignment,
    BackgroundImageCoverChrome,
    BackgroundImageOpacitySlider,
    OpenConfigFile,
    ImportTerminal,
    Reset,
    /// 高级: keep the resident server (detach) on window close.
    KeepSessionToggle,
    /// 高级: 启动时恢复上次会话（也是崩溃恢复的总开关）。
    RestoreSessionToggle,
    /// 高级: 冷恢复时自动接续各 pane 里的 AI 对话（claude/codex resume）。
    ResumeAiToggle,
    /// 高级: 常驻系统托盘图标（agent 等待输入时变 attention 态）。
    TrayToggle,
    /// 高级→同步: 输入框（0=url 1=用户名 2=WebDAV 密码 3=E2E 口令）。
    SyncInput(usize),
    SyncAutoPullToggle,
    SyncPushButton,
    SyncPullButton,
    /// 代理→SSH 连接代理: 模式下拉触发行。
    SshProxyModeDropdown,
    SshProxyModeOption(usize),
    /// 网络页：按当前已提交设置执行一次真实出网测试。
    SshProxyTest,
    /// 网络→指定代理→手动填写：协议下拉。
    SshProxyProtocolDropdown,
    SshProxyProtocolOption(usize),
    /// 网络: 输入框（0=代理地址（手动填写展开行） 1=绕过列表）。
    SshProxyInput(usize),
    /// 网络→指定代理: 列表单选行（先是本机发现，随后是三种其他方式）。
    SshProxyLinkPick(usize),
    /// 网络→指定代理: 重新执行本机协议握手扫描。
    SshProxyRescan,
    /// 网络→指定代理→SSH 跳板: 主机下拉触发行。
    SshJumpHostDropdown,
    SshJumpHostOption(usize),
    /// 网络: 每主机覆盖行（值=覆盖列表下标），点击开该主机的编辑器。
    SshProxyOverrideEdit(usize),
    /// 按键映射: 页顶搜索框（过滤动作名与按键）。
    KeymapSearchField,
    /// 按键映射: one editable action row (click → capture a new combo).
    /// 值 = **可见槽位**（过滤后的顺序），chrome 经 display 映射回 flat 行。
    KeymapRow(usize),
    /// 按键映射: one read-only extras row. 不可点击，但 hover 得有着落——
    /// 2026-08-09 裁定：列表每一项都要轻量色变反馈，无位移。
    KeymapReadonlyRow(usize),
    BackupSelection(usize),
    BackupExport,
    BackupRestore,
    /// 备份→远程备份: 协议下拉触发行 + 展开的选项行。
    BackupProtocolCycle,
    BackupProtocolOption(usize),
    /// 备份→远程备份: 输入框（槽位语义随协议变化）。
    BackupRemoteField(usize),
    BackupRemotePush,
    BackupRemotePull,
}

/// Stable animation slots for the settings switches. Rendering and the shared
/// motion clock both use this mapping, so one switch can never borrow another
/// switch's thumb position while the pointer moves between rows.
pub(super) const SETTINGS_TOGGLE_COUNT: usize = 17;

pub(super) fn settings_toggle_slot(hit: SettingsHit) -> Option<usize> {
    Some(match hit {
        SettingsHit::SystemThemeToggle => 0,
        SettingsHit::GhostToggle => 1,
        SettingsHit::CursorBlinkToggle => 2,
        SettingsHit::CopyOnSelectToggle => 3,
        SettingsHit::PanelResizeToggle => 4,
        SettingsHit::CjkBoldToggle => 5,
        SettingsHit::FetchToggle => 6,
        SettingsHit::PowerlineToggle => 7,
        SettingsHit::BlurToggle => 8,
        SettingsHit::KeepSessionToggle => 9,
        SettingsHit::RestoreSessionToggle => 10,
        SettingsHit::SyncAutoPullToggle => 11,
        SettingsHit::BackgroundImageCoverChrome => 12,
        SettingsHit::ProviderCodexGoalsToggle => 13,
        SettingsHit::ProviderCodexRemoteToggle => 14,
        // 追加在尾部：槽位是稳定映射，重排会让开关借走彼此的动画状态。
        SettingsHit::ResumeAiToggle => 15,
        SettingsHit::TrayToggle => 16,
        _ => return None,
    })
}

// ---- runtime settings store (`Nebula/nebula_settings.txt`) ----

pub(super) mod model;

pub(super) use self::model::{
    NebulaRuntimeSettings, cursor_shape_settings_value, format_hex_rgb, nebula_settings_load,
};
pub(crate) use self::model::parse_hex_rgb;

fn background_image_fit_label(fit: BackgroundImageFit, language: UiLanguage) -> &'static str {
    match fit {
        BackgroundImageFit::Fill => language.pick("拉伸", "Fill"),
        BackgroundImageFit::Uniform => language.pick("适应", "Uniform"),
        BackgroundImageFit::UniformToFill => language.pick("填充", "Uniform to fill"),
        BackgroundImageFit::None => language.pick("原始尺寸", "None"),
    }
}

fn background_image_alignment_label(
    alignment: BackgroundImageAlignment,
    language: UiLanguage,
) -> &'static str {
    match alignment {
        BackgroundImageAlignment::TopLeft => language.pick("左上", "Top left"),
        BackgroundImageAlignment::Top => language.pick("顶部", "Top"),
        BackgroundImageAlignment::TopRight => language.pick("右上", "Top right"),
        BackgroundImageAlignment::Left => language.pick("左侧", "Left"),
        BackgroundImageAlignment::Center => language.pick("居中", "Center"),
        BackgroundImageAlignment::Right => language.pick("右侧", "Right"),
        BackgroundImageAlignment::BottomLeft => language.pick("左下", "Bottom left"),
        BackgroundImageAlignment::Bottom => language.pick("底部", "Bottom"),
        BackgroundImageAlignment::BottomRight => language.pick("右下", "Bottom right"),
    }
}

pub(super) fn nebula_settings_mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(nebula_settings::settings_path()).and_then(|meta| meta.modified()).ok()
}

/// Persist runtime settings next to the history file.
pub(super) fn nebula_settings_write(settings: &NebulaRuntimeSettings) {
    let accept = match settings.accept {
        AcceptKey::Right => "right",
        AcceptKey::Tab => "tab",
        AcceptKey::Both => "both",
    };
    let completion_style = settings.completion_style.settings_value();
    let background = settings.background.map(format_hex_rgb).unwrap_or_default();
    let background_image = settings.background_image.as_deref().unwrap_or("");
    // A picked detected-shell id (cmd/pwsh/nu/wsl:X) is written verbatim; the
    // 2-value enum is the fallback for the built-in powershell/bash choice.
    let shell =
        settings.shell_id.clone().unwrap_or_else(|| settings.shell.settings_value().to_owned());
    let startup_directory = settings
        .startup_directory
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let theme = settings.theme.prompt_name();
    let path = nebula_settings::settings_path();
    let pinned_hosts = settings.pinned_hosts.join(",");
    let saved_hosts = settings.saved_hosts.join(",");
    let hidden_hosts = settings.hidden_hosts.join(",");
    let font_size = settings.font_size.map(|size| format!("{size:.1}")).unwrap_or_default();
    let mut keybinds = String::new();
    for (combo, action) in &settings.keybinds {
        keybinds.push_str(&format!("keybind={combo}:{action}\n"));
    }
    let quick_terminal_hotkey = settings.quick_terminal_hotkey.trim();
    let ssh_proxy_mode = settings.ssh_proxy_mode.as_str();
    let ssh_proxy_url = settings.ssh_proxy_url.trim();
    let ssh_proxy_no_proxy = settings.ssh_proxy_no_proxy.trim();
    let _ = std::fs::write(
        path,
        format!(
            "language={}\ntheme={theme}\nfollow_system_theme={}\nghost={}\naccept={accept}\ncompletion_style={completion_style}\nshell={shell}\nstartup_directory={startup_directory}\nfont_family={}\nfont_size={font_size}\ncursor_shape={}\ncursor_blink={}\ncopy_on_select={}\ncjk_bold_regular={}\ntabs_position={}\ntab_reveal={}\ndensity={}\nnew_tab_position={}\ncell_width_mode={}\nfetch={}\npowerline={}\nkeep_session={}\nrestore_session={}\nresume_ai={}\ntray={}\nblur={}\nopacity={:.2}\nbackground={background}\nbackground_image={background_image}\nbackground_image_opacity={:.2}\nbackground_image_fit={}\nbackground_image_alignment={}\nbackground_image_cover_chrome={}\npanel_resize={}\nsidebar_w={:.0}\ndrawer_w={:.0}\nhosts_band={:.0}\npinned_hosts={pinned_hosts}\nsaved_hosts={saved_hosts}\nhidden_hosts={hidden_hosts}\nssh_proxy_mode={ssh_proxy_mode}\nssh_proxy_url={ssh_proxy_url}\nssh_proxy_no_proxy={ssh_proxy_no_proxy}\nquick_terminal_hotkey={quick_terminal_hotkey}\n{keybinds}",
            settings.language.as_str(),
            settings.follow_system_theme as u8,
            settings.ghost as u8,
            settings.font_family,
            cursor_shape_settings_value(settings.cursor_shape),
            settings.cursor_blink as u8,
            settings.copy_on_select as u8,
            settings.cjk_bold_regular as u8,
            settings.tabs_position.settings_value(),
            settings.tab_reveal.settings_value(),
            density_settings_value(settings.density),
            settings.new_tab_position.settings_value(),
            settings.cell_width_mode.settings_value(),
            settings.fetch as u8,
            settings.powerline as u8,
            settings.keep_session as u8,
            settings.restore_session as u8,
            settings.resume_ai as u8,
            settings.tray as u8,
            // 与 GPUI 壳共用 `blur` 键，写回枚举名而不是 0/1：写 0/1 会让那边
            // 把它当成旧布尔值走迁移分支，用户显式选的 acrylic 每次经旧壳存盘
            // 都会掉档。旧壳自己只有开关，开启一律落到性能安全的 mica。
            if settings.blur { "mica" } else { "none" },
            settings.opacity,
            settings.background_image_opacity,
            settings.background_image_fit.settings_value(),
            settings.background_image_alignment.settings_value(),
            settings.background_image_cover_chrome as u8,
            settings.panel_resize as u8,
            settings.sidebar_w,
            settings.drawer_w,
            settings.hosts_band,
        ),
    );
}

// ---- geometry + hit-testing ----

pub(super) mod geometry;

pub(crate) use geometry::{
    appearance_preview_wallpaper_rects, background_color_picker_rects, background_color_popup,
    font_popup_row_count, font_popup_scrollbar, font_popup_slot, font_search_field_rect,
    keymap_search_rect, opacity_from_pointer, opacity_slider_rect, provider_input_rect,
    settings_max_scroll, ssh_proxy_input_rect, BackgroundColorPopup,
};
use geometry::{
    fit_provider_rows, settings_geometry, dropdown_anchor, font_popup_window, row_action_rect,
    ssh_host_action_rect, SettingsGeometry, STANDARD_ROW_ACTION_W, settings_viewport_h,
    popup_visible_index, advanced_content_end,
};

mod hit;
mod view;
mod popups;
mod render;
mod pages;

pub use hit::settings_hit;
pub(crate) use view::{SettingsView, SshSettingsHost};
use view::{
    backup_item_selected, backup_remote_actions_rect, backup_segment_rects,
    dropdown_hover_index, dropdown_selected_index, keymap_keycap_rect, keymap_pane_state_view,
    keymap_row_value, preview_line_y, provider_input_display, proxy_pane_state,
    ssh_proxy_expand_control, ssh_proxy_input_display, ssh_proxy_manual_controls,
    ssh_proxy_mode_control, ssh_proxy_test_button, sync_button_rects, sync_input_display,
    sync_input_rect, text_tail, PREVIEW_PROMPT_COLS,
};
pub(super) use popups::{draw_popup_text, push_popup_quads};
use render::{
    draw_big_text, draw_button_label, keymap_group_title, proxy_section_title_y, row_label,
    row_label_with_right_inset, section_title, warning_lines,
};

pub(super) fn push_quads(
    view: &SettingsView,
    quads: &mut Vec<UiQuad>,
    size: &SizeInfo,
    scale: f32,
) {
    let s = |v: f32| v * scale;
    let sk = settings_skin(view.theme);

    let mut geometry = settings_geometry(
        size,
        scale,
        view.area,
        view.scroll,
        view.hidden_hosts.len(),
        view.ssh_hosts.len(),
        view.density,
        proxy_pane_state(view),
        keymap_pane_state_view(view),
    );
    fit_provider_rows(&mut geometry, view.providers.len());
    let (px, py, pw, ph) = geometry.popup;
    // Scrolled content is clipped EXACTLY at the viewport edges: quads that
    // cross the fixed header separator or the popup's bottom edge are cut at
    // the line via [`UiQuad::clip_y`] (uv-remapped, so rounded corners and
    // glows are truncated mid-shape instead of bleeding past the hairline).
    let clip_top = geometry.content_top;
    let clip_bot = py + ph - s(6.0);
    let clip = |quads: &mut Vec<UiQuad>, quad: UiQuad| {
        if let Some(quad) = quad.clip_y(clip_top, clip_bot) {
            quads.push(quad);
        }
    };
    // 通用组件（widgets）不感知视口裁剪：输出先落到 staged，再统一过 clip。
    let mut staged: Vec<UiQuad> = Vec::new();

    // The page is flush with the active tab card. No veil, drop shadow or
    // second window outline: depth belongs to the app shell, not this page.
    quads.push(UiQuad::solid(px, py, pw, ph, s(12.0), sk.panel));
    // Sidebar and content use spacing alone; no structural divider lines.
    let section = view.section;
    for (nav_section, nx, ny, nw, nh) in geometry.nav {
        if nav_section == NebulaSettingsSection::Backup && !SHOW_BACKUP_SETTINGS {
            continue;
        }
        if nav_section == section {
            // 2026-08-09 对齐原型 .nav-item.on：选中 = accent_soft，与侧栏
            // tab、网络页单选行同 token。回滚: sk.surface（中性档）。
            quads.push(UiQuad::solid(nx, ny, nw, nh, s(8.0), sk.accent_soft));
        } else if view.hover == SettingsHit::Nav(nav_section) {
            quads.push(UiQuad::solid(nx, ny, nw, nh, s(8.0), sk.hover));
        }
        let icon_ink = if nav_section == section {
            Rgba::new(sk.ink_strong.r, sk.ink_strong.g, sk.ink_strong.b, 235)
        } else if view.hover == SettingsHit::Nav(nav_section) {
            Rgba::new(sk.icon_hover.r, sk.icon_hover.g, sk.icon_hover.b, 230)
        } else {
            Rgba::new(sk.icon.r, sk.icon.g, sk.icon.b, 190)
        };
        // 空心图标（代理的中继环）挖空用的必须是行的**有效底色**：面板色
        // 与选中/悬浮药丸（半透明）合成后的结果，猜错环心就是一块色斑。
        let icon_cutout = if nav_section == section {
            // 选中底换 accent_soft 后挖空必须跟着换，否则环心是旧色斑。
            // 回滚: icons::blend_over(sk.panel, sk.surface)
            icons::blend_over(sk.panel, sk.accent_soft)
        } else if view.hover == SettingsHit::Nav(nav_section) {
            icons::blend_over(sk.panel, sk.hover)
        } else {
            sk.panel
        };
        let icon_x = if geometry.compact_nav { nx + (nw - s(18.0)) * 0.5 } else { nx + s(10.0) };
        icons::push_settings_nav_icon(
            quads,
            nav_icon(nav_section),
            (icon_x, ny + s(7.0), s(18.0), s(18.0)),
            scale,
            icon_ink,
            icon_cutout,
        );
    }

    // Reset: a quiet ghost button in the header. SSH intentionally has no
    // page-level "Upgrade" action; host management is the complete surface.
    if !matches!(section, NebulaSettingsSection::Ssh | NebulaSettingsSection::Providers) {
        let (rx, ry, rw, rh) = geometry.reset;
        quads.push(UiQuad::solid(rx, ry, rw, rh, s(8.0), sk.surface));
        let hovered = view.hover == SettingsHit::Reset;
        if hovered {
            quads.push(UiQuad::solid(rx, ry, rw, rh, s(8.0), sk.hover));
        }
    }

    // Settings groups are unframed. Natural row spacing carries hierarchy;
    // controls provide their own local hover feedback and click targets.
    let group_frame = |_quads: &mut Vec<UiQuad>, _first_row: (f32, f32, f32, f32), _rows: usize| {};
    let row_hover = |_quads: &mut Vec<UiQuad>, _rect: (f32, f32, f32, f32), _hovered: bool| {};
    let action_button = |quads: &mut Vec<UiQuad>, row, logical_w: f32, hovered: bool| {
        let rect = row_action_rect(row, scale, logical_w);
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
    // Widget wrappers: stage → viewport-clip → push. Every multi-option row
    // shares ONE combobox component (user ruling 2026-07-23), hover/press
    // feedback included, so no page ever hand-rolls its own control again.
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
    let slider = |quads: &mut Vec<UiQuad>, staged: &mut Vec<UiQuad>, hit, value: f32, hot: bool| {
        widgets::push_slider(staged, hit, value, scale, &sk, hot);
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

    match section {
        NebulaSettingsSection::Appearance => {
            pages::appearance::push_appearance_quads(
                view, quads, size, scale, &geometry, &sk, clip_top, clip_bot,
            );
        },
        NebulaSettingsSection::Profiles => {
            pages::profiles::push_profiles_quads(
                view, quads, size, scale, &geometry, &sk, clip_top, clip_bot,
            );
        },
        NebulaSettingsSection::Providers => {
            pages::providers::push_providers_quads(
                view, quads, size, scale, &geometry, &sk, clip_top, clip_bot,
            );
        },
        NebulaSettingsSection::Ssh => {
            pages::ssh::push_ssh_quads(
                view, quads, size, scale, &geometry, &sk, clip_top, clip_bot,
            );
        },
        NebulaSettingsSection::Proxy => {
            pages::proxy::push_proxy_quads(
                view, quads, size, scale, &geometry, &sk, clip_top, clip_bot,
            );
        },
        NebulaSettingsSection::Interaction => {
            pages::interaction::push_interaction_quads(
                view, quads, size, scale, &geometry, &sk, clip_top, clip_bot,
            );
        },
        NebulaSettingsSection::Keymap => {
            pages::keymap::push_keymap_quads(
                view, quads, size, scale, &geometry, &sk, clip_top, clip_bot,
            );
        },
        NebulaSettingsSection::Advanced => {
            pages::advanced::push_advanced_quads(
                view, quads, size, scale, &geometry, &sk, clip_top, clip_bot,
            );
        },
        NebulaSettingsSection::Backup => {
            pages::backup::push_backup_quads(
                view, quads, size, scale, &geometry, &sk, clip_top, clip_bot,
            );
        },
    }
    // section actually overflows (same style as the pane scrollbar: thin
    // rounded thumb, no track).
    let content_h = match section {
        NebulaSettingsSection::Appearance => geometry.appearance_h,
        NebulaSettingsSection::Profiles => geometry.profiles_h,
        NebulaSettingsSection::Providers => geometry.providers_h,
        NebulaSettingsSection::Ssh => geometry.ssh_h,
        NebulaSettingsSection::Proxy => geometry.proxy_h,
        NebulaSettingsSection::Interaction => geometry.interaction_h,
        NebulaSettingsSection::Keymap => geometry.keymap_h,
        NebulaSettingsSection::Advanced => geometry.advanced_h,
        NebulaSettingsSection::Backup => geometry.backup_h,
    };
    let viewport_h = settings_viewport_h(ph, scale);
    if content_h > viewport_h {
        let max_scroll = content_h - viewport_h;
        let frac = (view.scroll / max_scroll).clamp(0.0, 1.0);
        let track_h = viewport_h - s(12.0);
        let thumb_h = (track_h * viewport_h / content_h).max(s(28.0));
        let ty = clip_top + s(6.0) + (track_h - thumb_h) * frac;
        let tx = px + pw - s(7.0);
        quads.push(UiQuad::solid(
            tx,
            ty,
            s(4.0),
            thumb_h,
            s(2.0),
            sk.scrollbar_thumb.with_alpha(0.45),
        ));
    }
}

/// The floating dropdown option list. `draw_chrome` paints these AFTER the
/// base text pass (a separate `draw_ui` call), so page labels can never bleed
/// through the popup plate — the same modal layering rule the command palette
/// needed.
pub(super) fn draw_text(
    view: &SettingsView,
    r: &mut Renderer,
    gc: &mut GlyphCache,
    size: &SizeInfo,
    scale: f32,
) -> Vec<(String, (f32, f32, f32, f32))> {
    let s = |v: f32| v * scale;
    let cell_w = size.cell_width();
    let cell_h = size.cell_height();
    let sk = settings_skin(view.theme);
    let language = view.language;

    let mut geometry = settings_geometry(
        size,
        scale,
        view.area,
        view.scroll,
        view.hidden_hosts.len(),
        view.ssh_hosts.len(),
        view.density,
        proxy_pane_state(view),
        keymap_pane_state_view(view),
    );
    fit_provider_rows(&mut geometry, view.providers.len());
    // Kept for parity with [`draw_popup_text`]'s shell icons; the base page
    // currently stages no icon draws of its own.
    let mut icon_draws = Vec::new();
    let (px, py, _pw, ph) = geometry.popup;
    let (content_x, content_y, _content_w, _) = geometry.content;
    // Text has no scissor, so unlike the quad pass (which cuts quads at the
    // viewport edges) a text block is drawn only when it fits ENTIRELY inside
    // the viewport — a glyph must never cross the header hairline.
    let clip_top = geometry.content_top;
    let clip_bot = py + ph - s(6.0);
    let visible = |ry: f32, rh: f32| ry >= clip_top && ry + rh <= clip_bot;
    let row_text_y = |ry: f32, rh: f32| {
        if geometry.stacked_rows { ry + s(9.0) } else { ry + (rh - cell_h) / 2.0 }
    };
    // 通用 combobox 的当前值：控件框内左对齐，截断在 chevron 井之前。
    let combobox_value = |r: &mut Renderer,
                          gc: &mut GlyphCache,
                          row: (f32, f32, f32, f32),
                          value: &str,
                          ink: Rgb| {
        let rect = widgets::combobox_rect(row, scale);
        let tx = widgets::combobox_text_x(rect, scale);
        let right = widgets::combobox_text_right(rect, scale);
        let max_chars = ((right - tx).max(cell_w) / cell_w).floor().max(1.0) as usize;
        let value = truncate_tab_label(value, max_chars);
        r.draw_chrome_text(size, tx, rect.1 + (rect.3 - cell_h) / 2.0, ink, &value, gc);
    };
    let combobox_value_rect = |r: &mut Renderer,
                               gc: &mut GlyphCache,
                               rect: (f32, f32, f32, f32),
                               value: &str,
                               ink: Rgb| {
        let tx = widgets::combobox_text_x(rect, scale);
        let right = widgets::combobox_text_right(rect, scale);
        let max_chars = ((right - tx).max(cell_w) / cell_w).floor().max(1.0) as usize;
        let value = truncate_tab_label(value, max_chars);
        r.draw_chrome_text(size, tx, rect.1 + (rect.3 - cell_h) / 2.0, ink, &value, gc);
    };
    // Group titles hang 42px above their first row (title + 16px gap) and
    // scroll with it.
    let group_y = |row_y: f32| row_y - s(42.0);
    let title_h = s(26.0);

    let section = view.section;
    // Brand title in the sidebar header. The compact rail is intentionally
    // icon-only, so a long brand label must not compete with the content.
    if !geometry.compact_nav {
        draw_big_text(
            r,
            gc,
            size,
            scale,
            px + s(24.0),
            py + s(22.0),
            1.5,
            sk.ink_strong,
            language.pick("Nebula 设置", "Nebula Settings"),
        );
    }
    if !matches!(section, NebulaSettingsSection::Ssh | NebulaSettingsSection::Providers) {
        // Center the reset label inside its ghost button.
        let (rx, ry, rw, rh) = geometry.reset;
        let label = if geometry.stacked_rows {
            "↶"
        } else {
            language.pick("恢复默认设置", "Restore defaults")
        };
        let cols: usize = label.chars().map(|c| c.width().unwrap_or(0)).sum();
        let tx = rx + (rw - cols as f32 * cell_w) / 2.0;
        r.draw_chrome_text(size, tx, ry + (rh - cell_h) / 2.0, sk.ink_dim, label, gc);
    }
    // Sidebar navigation labels share the icon geometry and visibility gate
    // from the quad/hit passes, so hidden entries cannot leave ghost text.
    for (nav_section, nx, ny, _nw, nh) in geometry.nav {
        if nav_section == NebulaSettingsSection::Backup && !SHOW_BACKUP_SETTINGS {
            continue;
        }
        if geometry.compact_nav {
            continue;
        }
        let active = nav_section == section;
        let hovered = view.hover == SettingsHit::Nav(nav_section);
        r.draw_chrome_text(
            size,
            nx + s(38.0),
            ny + (nh - cell_h) / 2.0,
            if active {
                sk.ink_strong
            } else if hovered {
                sk.ink
            } else {
                sk.ink_dim
            },
            nav_section.label(view.language),
            gc,
        );
    }
    let group_text_h = cell_h * 0.78;
    if !geometry.compact_nav {
        for (rect, label) in geometry
            .nav_groups
            .into_iter()
            .zip([language.pick("连接", "Connections"), language.pick("系统", "System")])
        {
            r.draw_ui_text(
                size,
                rect.0 + s(10.0),
                widgets::centered_y(rect.1, rect.3, group_text_h),
                0.78,
                sk.ink_dim,
                nebula_terminal::term::cell::Flags::empty(),
                label,
                gc,
            );
        }
    }
    // Content header: the big section title alone. (No subtitle — the nav
    // label + title already say everything; the old dim sentence only added
    // noise under the heading.)
    draw_big_text(
        r,
        gc,
        size,
        scale,
        content_x + s(24.0),
        content_y + s(20.0),
        1.6,
        sk.ink_strong,
        section.label(view.language),
    );

    match section {
        NebulaSettingsSection::Appearance => {
            pages::appearance::draw_appearance_text(
                view, r, gc, size, scale, &geometry, &sk, language, cell_w, cell_h,
                &mut icon_draws, content_x, px, clip_top, clip_bot, title_h,
            );
        },
        NebulaSettingsSection::Profiles => {
            pages::profiles::draw_profiles_text(
                view, r, gc, size, scale, &geometry, &sk, language, cell_w, cell_h,
                &mut icon_draws, content_x, px, clip_top, clip_bot, title_h,
            );
        },
        NebulaSettingsSection::Providers => {
            pages::providers::draw_providers_text(
                view, r, gc, size, scale, &geometry, &sk, language, cell_w, cell_h,
                &mut icon_draws, content_x, px, clip_top, clip_bot, title_h,
            );
        },
        NebulaSettingsSection::Ssh => {
            pages::ssh::draw_ssh_text(
                view, r, gc, size, scale, &geometry, &sk, language, cell_w, cell_h,
                &mut icon_draws, content_x, px, clip_top, clip_bot, title_h,
            );
        },
        NebulaSettingsSection::Proxy => {
            pages::proxy::draw_proxy_text(
                view, r, gc, size, scale, &geometry, &sk, language, cell_w, cell_h,
                &mut icon_draws, content_x, px, clip_top, clip_bot, title_h,
            );
        },
        NebulaSettingsSection::Interaction => {
            pages::interaction::draw_interaction_text(
                view, r, gc, size, scale, &geometry, &sk, language, cell_w, cell_h,
                &mut icon_draws, content_x, px, clip_top, clip_bot, title_h,
            );
        },
        NebulaSettingsSection::Keymap => {
            pages::keymap::draw_keymap_text(
                view, r, gc, size, scale, &geometry, &sk, language, cell_w, cell_h,
                &mut icon_draws, content_x, px, clip_top, clip_bot, title_h,
            );
        },
        NebulaSettingsSection::Advanced => {
            pages::advanced::draw_advanced_text(
                view, r, gc, size, scale, &geometry, &sk, language, cell_w, cell_h,
                &mut icon_draws, content_x, px, clip_top, clip_bot, title_h,
            );
        },
        NebulaSettingsSection::Backup => {
            pages::backup::draw_backup_text(
                view, r, gc, size, scale, &geometry, &sk, language, cell_w, cell_h,
                &mut icon_draws, content_x, px, clip_top, clip_bot, title_h,
            );
        },
    }
    icon_draws
}


#[cfg(test)]
mod tests;
