//! 键位自定义模型（spec 002）。
//!
//! `nebula_settings.txt` 中每行 `keybind=<combo>:<action>` 构成用户自定义
//! 表；`input/keyboard.rs::process_key_bindings` 先查这张表再查 config 的
//! 默认/TOML 表，命中即短路——改键遮蔽默认、`none` 禁用键都由这一条
//! 优先级规则实现。设置页「按键映射」的行数据与捕获逻辑也在这里。

use std::sync::OnceLock;

use winit::event::KeyEvent;
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

use crate::config::default_key_bindings;
use crate::config::{Action, BindingKey, BindingMode, KeyBinding, KeyLocation};

/// 设置页可编辑的动作（顺序即显示顺序）。数字系动作（SelectTab1..9、
/// LaunchProfile1..9）与 AI 贴入键属只读展示，不进这张表。
///
/// `QUICK_TERMINAL_ROW` 是设置页专用的全局快捷键行，不属于终端
/// `config::Action`；它放在动作行之前，保证用户在按键页打开时第一眼就能
/// 找到快速终端入口，同时仍复用同一套捕获/键帽交互。
pub(crate) const QUICK_TERMINAL_ROW: usize = 0;
pub(crate) const DEFAULT_QUICK_TERMINAL_HOTKEY: &str = "ctrl+`";

pub(crate) fn editable_row_count() -> usize {
    EDITABLE_ACTIONS.len() + 1
}

pub(crate) fn display_stored_combo(combo: &str) -> String {
    parse_combo(combo)
        .and_then(|(mods, key)| display_combo(mods, &key))
        .unwrap_or_else(|| combo.to_owned())
}

pub(crate) const EDITABLE_ACTIONS: &[(Action, &str, &str)] = &[
    // 顺序 = 设置页展示顺序：按 GROUPS 的连续区间分组（2026-08-09 学原型
    // 分组裁定）。持久化按 Action 名存储，重排只影响展示，不动用户数据。
    // -- 全局（快速终端行占本组第 0 行，见 QUICK_TERMINAL_ROW）--
    (Action::ToggleCommandPalette, "命令面板", "Command palette"),
    (Action::OpenQuickJump, "快速跳转", "Quick jump"),
    (Action::ToggleShellPicker, "Shell 选择器", "Shell picker"),
    (Action::CreateNewWindow, "新建窗口", "New window"),
    (Action::ToggleFullscreen, "全屏", "Fullscreen"),
    // -- 标签页 --
    (Action::CreateNewTab, "新建标签页", "New tab"),
    (Action::CloseTab, "关闭标签页 / 分屏", "Close tab / pane"),
    (Action::RenameTab, "重命名标签页", "Rename tab"),
    (Action::SelectNextTab, "下一个标签页", "Next tab"),
    (Action::SelectPreviousTab, "上一个标签页", "Previous tab"),
    // -- 窗格 --
    (Action::SplitRight, "左右分屏", "Split right"),
    (Action::SplitDown, "上下分屏", "Split down"),
    (Action::ToggleZoom, "放大当前分屏", "Zoom current pane"),
    (Action::FocusPaneLeft, "焦点：左侧分屏", "Focus pane left"),
    (Action::FocusPaneRight, "焦点：右侧分屏", "Focus pane right"),
    (Action::FocusPaneUp, "焦点：上方分屏", "Focus pane up"),
    (Action::FocusPaneDown, "焦点：下方分屏", "Focus pane down"),
    // -- 侧栏面板 --
    (Action::ToggleFilesPanel, "目录树面板", "Files panel"),
    (Action::ToggleGitPanel, "Git 面板", "Git panel"),
    // -- 终端 --
    (Action::PromptJumpUp, "跳到上一个提示符", "Previous prompt"),
    (Action::PromptJumpDown, "跳到下一个提示符", "Next prompt"),
    (Action::SearchForward, "搜索（向前）", "Search forward"),
    (Action::SearchBackward, "搜索（向后）", "Search backward"),
    (Action::Copy, "复制", "Copy"),
    (Action::Paste, "粘贴", "Paste"),
    (Action::IncreaseFontSize, "增大字号", "Font size up"),
    (Action::DecreaseFontSize, "减小字号", "Font size down"),
    (Action::ResetFontSize, "重置字号", "Reset font size"),
];

/// 设置页分组（无框裁定 2026-08-09：只用段标题+间距分组，不画容器）。
/// `usize` 是本组行数；区间连续覆盖全部可编辑行（含第 0 行快速终端）。
pub(crate) const GROUPS: &[(&str, &str, usize)] = &[
    ("全局", "Global", 6),
    ("标签页", "Tabs", 5),
    ("窗格", "Panes", 7),
    ("侧栏面板", "Side panels", 2),
    ("终端", "Terminal", 9),
];

/// 只读展示行（无法在图形页编辑，TOML/settings 行仍可覆盖其中的表驱动键）。
pub(crate) const READONLY_ROWS: &[(&str, &str, &str)] = &[
    (
        "切换到第 N 个标签页",
        "Select tab N",
        if cfg!(target_os = "macos") {
            "Option+1..9 / Ctrl+1..9 / Command+1..9"
        } else {
            "Alt+1..9 / Ctrl+1..9"
        },
    ),
    ("启动 Profile N", "Launch Profile N", "Ctrl+Shift+1..9"),
    ("贴入 AI 修复建议", "Paste AI fix suggestion", "Ctrl+."),
];

/// A caller-selected keyboard model; no process-global frontend state is needed.
#[derive(Clone, Copy)]
pub(crate) struct ShortcutPolicy {
    gpui: bool,
}

const LEGACY: ShortcutPolicy = ShortcutPolicy { gpui: false };
pub(crate) const GPUI: ShortcutPolicy = ShortcutPolicy { gpui: true };

const GPUI_READONLY_ROWS: &[(&str, &str, &str)] = &[
    ("切换到第 N 个标签页", "Select tab N", "Option+1..9 / Command+1..9"),
    READONLY_ROWS[1],
    READONLY_ROWS[2],
];

/// Apply only to GPUI GUI defaults, never user bindings or terminal modes.
pub(crate) fn gui_character_shortcut_enabled(mods: ModifiersState) -> bool {
    !cfg!(target_os = "macos") || mods != ModifiersState::CONTROL
}

impl ShortcutPolicy {
    pub(crate) fn readonly_rows(self) -> &'static [(&'static str, &'static str, &'static str)] {
        if self.gpui && cfg!(target_os = "macos") { GPUI_READONLY_ROWS } else { READONLY_ROWS }
    }
}

/// macOS 的 ⌘ 命令键别名：`(存储写法, 动作)`。
///
/// GPUI 壳在配置表之外**追加**一整套 ⌘ 绑定：配置表的 macOS 段只覆盖其中
/// 一半，另一半（⌘K、⌘D、⌘⇧P…）是壳自己加的。这张表是那套绑定的唯一权威：
/// `init()` 按它注册静态绑定，解绑与恢复按它写回 `keybind=` 行。留在表外
/// 就成了影子绑定——`clear_action` 不知道它，于是出现「⌘K 解绑后仍然弹出
/// Shell 选择器」（#238）。
///
/// 表里的写法会写进设置文件，超级键写 `cmd`（macOS 的读法）。历史配置和
/// 录制结果仍可使用 `win` / `super`；GPUI 适配层按解析后的按键规范化，
/// 覆盖与恢复默认不依赖这些别名或修饰键顺序的逐字相等。
///
/// GPUI 默认缓存统一合并这张表：注册、展示、解绑和恢复使用同一套 ⌘ 键。
///
/// 别名表是共享数据；实际注册由平台层入口限定为 macOS，便于跨平台核验分派。
pub(crate) const MACOS_COMMAND_ALIASES: &[(&str, Action)] = &[
    ("cmd+t", Action::CreateNewTab),
    ("cmd+n", Action::CreateNewWindow),
    // ⌘W 留给原生关窗；终端标签关闭的注册与设置恢复共用这个别名。
    ("shift+cmd+w", Action::CloseTab),
    ("shift+cmd+p", Action::ToggleCommandPalette),
    ("cmd+k", Action::ToggleShellPicker),
    ("shift+cmd+f", Action::ToggleFilesPanel),
    ("cmd+d", Action::SplitRight),
    ("shift+cmd+d", Action::SplitDown),
    ("shift+cmd+enter", Action::ToggleZoom),
    ("alt+cmd+left", Action::FocusPaneLeft),
    ("alt+cmd+right", Action::FocusPaneRight),
    ("alt+cmd+up", Action::FocusPaneUp),
    ("alt+cmd+down", Action::FocusPaneDown),
    ("shift+cmd+]", Action::SelectNextTab),
    ("shift+cmd+[", Action::SelectPreviousTab),
    ("cmd+1", Action::SelectTab1),
    ("cmd+2", Action::SelectTab2),
    ("cmd+3", Action::SelectTab3),
    ("cmd+4", Action::SelectTab4),
    ("cmd+5", Action::SelectTab5),
    ("cmd+6", Action::SelectTab6),
    ("cmd+7", Action::SelectTab7),
    ("cmd+8", Action::SelectTab8),
    ("cmd+9", Action::SelectLastTab),
    ("shift+cmd+g", Action::ToggleGitPanel),
    ("cmd+plus", Action::IncreaseFontSize),
    ("cmd+=", Action::IncreaseFontSize),
    ("cmd+minus", Action::DecreaseFontSize),
    ("cmd+digit0", Action::ResetFontSize),
    ("cmd+c", Action::Copy),
    ("cmd+v", Action::Paste),
    ("ctrl+cmd+f", Action::ToggleFullscreen),
    ("shift+cmd+o", Action::OpenQuickJump),
    ("cmd+q", Action::Quit),
];

pub(crate) fn action_label(
    row: &(Action, &'static str, &'static str),
    language: super::UiLanguage,
) -> &'static str {
    action_message(&row.0)
        .map_or_else(|| language.pick(row.1, row.2), |message| language.text(message))
}

fn action_message(action: &Action) -> Option<crate::i18n::Message> {
    use crate::i18n::Message;
    Some(match action {
        Action::ToggleCommandPalette => Message::SettingsKeymapActionCommandPalette,
        Action::OpenQuickJump => Message::SettingsKeymapActionQuickJump,
        Action::ToggleShellPicker => Message::SettingsKeymapActionShellPicker,
        Action::CreateNewWindow => Message::CommonNewWindow,
        Action::ToggleFullscreen => Message::SettingsKeymapActionFullscreen,
        Action::CreateNewTab => Message::SettingsKeymapActionNewTab,
        Action::CloseTab => Message::SettingsKeymapActionCloseTabPane,
        Action::RenameTab => Message::CommonRenameTab,
        Action::SelectNextTab => Message::SettingsKeymapActionNextTab,
        Action::SelectPreviousTab => Message::SettingsKeymapActionPreviousTab,
        Action::SplitRight => Message::SettingsKeymapActionSplitRight,
        Action::SplitDown => Message::SettingsKeymapActionSplitDown,
        Action::ToggleZoom => Message::SettingsKeymapActionZoomCurrentPane,
        Action::FocusPaneLeft => Message::SettingsKeymapActionFocusPaneLeft,
        Action::FocusPaneRight => Message::SettingsKeymapActionFocusPaneRight,
        Action::FocusPaneUp => Message::SettingsKeymapActionFocusPaneUp,
        Action::FocusPaneDown => Message::SettingsKeymapActionFocusPaneDown,
        Action::ToggleFilesPanel => Message::SettingsKeymapActionFilesPanel,
        Action::ToggleGitPanel => Message::SettingsKeymapActionGitPanel,
        Action::PromptJumpUp => Message::SettingsKeymapActionPreviousPrompt,
        Action::PromptJumpDown => Message::SettingsKeymapActionNextPrompt,
        Action::SearchForward => Message::SettingsKeymapActionSearchForward,
        Action::SearchBackward => Message::SettingsKeymapActionSearchBackward,
        Action::Copy => Message::CommonCopy,
        Action::Paste => Message::CommonPaste,
        Action::IncreaseFontSize => Message::SettingsKeymapActionFontSizeUp,
        Action::DecreaseFontSize => Message::SettingsKeymapActionFontSizeDown,
        Action::ResetFontSize => Message::SettingsKeymapActionResetFontSize,
        _ => return None,
    })
}

/// Settings-page heading for `GROUPS[index]`.
pub(crate) fn group_label(index: usize, language: super::UiLanguage) -> &'static str {
    use crate::i18n::Message;
    language.text(match index {
        0 => Message::SettingsKeymapGroupGlobal,
        1 => Message::SettingsKeymapGroupTabs,
        2 => Message::SettingsKeymapGroupPanes,
        3 => Message::SettingsKeymapGroupSidePanels,
        4 => Message::SettingsSidebarProfiles,
        _ => return GROUPS.get(index).map_or("", |(zh, en, _)| language.pick(zh, en)),
    })
}

/// Label for `READONLY_ROWS[index]`.
pub(crate) fn readonly_label(index: usize, language: super::UiLanguage) -> &'static str {
    use crate::i18n::Message;
    language.text(match index {
        0 => Message::SettingsKeymapReadonlySelectTabN,
        1 => Message::SettingsKeymapReadonlyLaunchProfileN,
        2 => Message::SettingsKeymapReadonlyPasteAiFixSuggestion,
        _ => return READONLY_ROWS.get(index).map_or("", |(zh, en, _)| language.pick(zh, en)),
    })
}

#[cfg(test)]
mod group_tests {
    use super::*;

    #[test]
    fn groups_cover_every_editable_row_exactly_once() {
        let total: usize = GROUPS.iter().map(|(.., count)| count).sum();
        assert_eq!(total, editable_row_count(), "分组区间必须连续铺满全部可编辑行");
        assert_eq!(GROUPS.len(), 5, "settings::KeymapPaneState 的数组长度写死为 5");
    }
}

/// The original free functions retain the legacy model for legacy callers.
#[cfg(test)]
fn cached_defaults() -> &'static [KeyBinding] {
    LEGACY.cached_defaults()
}

pub(crate) fn default_shortcuts() -> Vec<(String, Action)> {
    LEGACY.default_shortcuts()
}

pub(crate) fn rebind_action(raw: &mut Vec<(String, String)>, action: &Action, combo: String) {
    LEGACY.rebind_action(raw, action, combo);
}

pub(crate) fn clear_action(raw: &mut Vec<(String, String)>, action: &Action) {
    LEGACY.clear_action(raw, action);
}

pub(crate) fn reset_action(raw: &mut Vec<(String, String)>, action: &Action) {
    LEGACY.reset_action(raw, action);
}

fn is_numbered_tab_action(action: &Action) -> bool {
    matches!(
        action,
        Action::SelectTab1
            | Action::SelectTab2
            | Action::SelectTab3
            | Action::SelectTab4
            | Action::SelectTab5
            | Action::SelectTab6
            | Action::SelectTab7
            | Action::SelectTab8
            | Action::SelectTab9
            | Action::SelectLastTab
    )
}

impl ShortcutPolicy {
    /// Both caches are immutable derived data, selected by the caller each time.
    fn cached_defaults(self) -> &'static [KeyBinding] {
        static LEGACY_DEFAULTS: OnceLock<Vec<KeyBinding>> = OnceLock::new();
        static GPUI_DEFAULTS: OnceLock<Vec<KeyBinding>> = OnceLock::new();
        if !self.gpui {
            return LEGACY_DEFAULTS.get_or_init(default_key_bindings);
        }
        GPUI_DEFAULTS.get_or_init(|| {
            let mut defaults = default_key_bindings();
            if cfg!(target_os = "macos") {
                // Only these actions belong to the GUI default source. Native
                // Ctrl+L and mode-specific vi/search actions stay in the model.
                defaults.retain(|binding| {
                    let gui_action = is_numbered_tab_action(&binding.action)
                        || MACOS_COMMAND_ALIASES
                            .iter()
                            .any(|(_, action)| action == &binding.action);
                    !gui_action
                        || !matches!(
                            &binding.trigger,
                            BindingKey::Keycode { key: Key::Character(_), .. }
                        )
                        || gui_character_shortcut_enabled(binding.mods)
                });
                for (combo, action) in MACOS_COMMAND_ALIASES {
                    let (mods, trigger) =
                        parse_combo(combo).expect("valid built-in Command shortcut");
                    // GPUI aliases own their runtime chords, including Cmd+W.
                    defaults.retain(|binding| {
                        binding.mods != mods || !self.same_binding_key(&binding.trigger, &trigger)
                    });
                    defaults.push(KeyBinding {
                        trigger,
                        mods,
                        mode: BindingMode::empty(),
                        notmode: BindingMode::empty(),
                        action: action.clone(),
                    });
                }
            }
            defaults
        })
    }

    pub(crate) fn default_shortcuts(self) -> Vec<(String, Action)> {
        let mut shortcuts: Vec<(String, Action)> = self
            .cached_defaults()
            .iter()
            .filter_map(|binding| {
                if self.gpui && cfg!(target_os = "macos") {
                    for (combo, action) in MACOS_COMMAND_ALIASES {
                        if action == &binding.action
                            && parse_combo(combo).is_some_and(|(mods, trigger)| {
                                mods == binding.mods
                                    && self.same_binding_key(&trigger, &binding.trigger)
                            })
                        {
                            return Some(((*combo).to_owned(), binding.action.clone()));
                        }
                    }
                }
                canonical_combo(binding.mods, &binding.trigger)
                    .map(|combo| (combo, binding.action.clone()))
            })
            .collect();
        if !self.gpui && cfg!(target_os = "macos") {
            // Preserve the old clear/reset vocabulary without feeding GPUI
            // winners into the legacy runtime/default-keycap model. Numbered
            // Command aliases were added for GPUI, not to this legacy extra set.
            let mut extra: Vec<(String, Action)> = Vec::new();
            for (combo, action) in MACOS_COMMAND_ALIASES {
                if is_numbered_tab_action(action) {
                    continue;
                }
                let covered = shortcuts
                    .iter()
                    .chain(extra.iter())
                    .any(|(known, candidate)| candidate == action && self.same_combo(known, combo));
                if !covered {
                    extra.push(((*combo).to_owned(), action.clone()));
                }
            }
            shortcuts.extend(extra);
        }
        if self.gpui && cfg!(target_os = "macos") {
            // 原生关窗没有持久化 Action；恢复 Quit 不得把旧壳的 ⌘W 注入为退出应用。
            shortcuts.retain(|(combo, _)| !self.same_combo(combo, "cmd+w"));
        }
        shortcuts
    }

    /// Editing one action moves its shortcut; explicit other actions survive.
    pub(crate) fn rebind_action(
        self,
        raw: &mut Vec<(String, String)>,
        action: &Action,
        combo: String,
    ) {
        self.clear_action(raw, action);
        raw.push((combo, action_storage_name(action)));
    }

    pub(crate) fn clear_action(self, raw: &mut Vec<(String, String)>, action: &Action) {
        let mut combos: Vec<String> = self
            .default_shortcuts()
            .into_iter()
            .filter(|(_, candidate)| candidate == action)
            .map(|(combo, _)| combo)
            .collect();
        combos.extend(
            raw.iter()
                .filter(|(_, name)| parse_action(name).as_ref() == Some(action))
                .map(|(combo, _)| combo.clone()),
        );
        raw.retain(|(_, name)| parse_action(name).as_ref() != Some(action));
        for combo in combos {
            if raw
                .iter()
                .rev()
                .find(|(candidate, _)| self.same_combo(candidate, &combo))
                .is_some_and(|(_, name)| parse_action(name) != Some(Action::ReceiveChar))
            {
                continue;
            }
            raw.retain(|(candidate, name)| {
                !self.same_combo(candidate, &combo)
                    || parse_action(name) != Some(Action::ReceiveChar)
            });
            raw.push((combo, "ReceiveChar".into()));
        }
    }

    pub(crate) fn reset_action(self, raw: &mut Vec<(String, String)>, action: &Action) {
        let defaults = self.default_shortcuts();
        raw.retain(|(combo, name)| {
            let parsed = parse_action(name);
            parsed.as_ref() != Some(action)
                && !(parsed == Some(Action::ReceiveChar)
                    && defaults.iter().any(|(default, candidate)| {
                        candidate == action && self.same_combo(combo, default)
                    }))
        });
    }
}

// ---- combo 文本 ↔ 结构 ----

/// 命名键与存储别名的映射（存储侧统一小写）。
const NAMED_ALIASES: &[(&str, NamedKey)] = &[
    ("enter", NamedKey::Enter),
    ("tab", NamedKey::Tab),
    ("escape", NamedKey::Escape),
    ("space", NamedKey::Space),
    ("backspace", NamedKey::Backspace),
    ("delete", NamedKey::Delete),
    ("insert", NamedKey::Insert),
    ("home", NamedKey::Home),
    ("end", NamedKey::End),
    ("pageup", NamedKey::PageUp),
    ("pagedown", NamedKey::PageDown),
    ("up", NamedKey::ArrowUp),
    ("down", NamedKey::ArrowDown),
    ("left", NamedKey::ArrowLeft),
    ("right", NamedKey::ArrowRight),
    ("f1", NamedKey::F1),
    ("f2", NamedKey::F2),
    ("f3", NamedKey::F3),
    ("f4", NamedKey::F4),
    ("f5", NamedKey::F5),
    ("f6", NamedKey::F6),
    ("f7", NamedKey::F7),
    ("f8", NamedKey::F8),
    ("f9", NamedKey::F9),
    ("f10", NamedKey::F10),
    ("f11", NamedKey::F11),
    ("f12", NamedKey::F12),
    ("f13", NamedKey::F13),
    ("f14", NamedKey::F14),
    ("f15", NamedKey::F15),
    ("f16", NamedKey::F16),
    ("f17", NamedKey::F17),
    ("f18", NamedKey::F18),
    ("f19", NamedKey::F19),
    ("f20", NamedKey::F20),
    ("f21", NamedKey::F21),
    ("f22", NamedKey::F22),
    ("f23", NamedKey::F23),
    ("f24", NamedKey::F24),
];

/// 物理数字键（Shift 组合下 logical key 随布局漂移，只能按 scancode 绑）。
const DIGIT_CODES: [(&str, KeyCode); 10] = [
    ("digit1", KeyCode::Digit1),
    ("digit2", KeyCode::Digit2),
    ("digit3", KeyCode::Digit3),
    ("digit4", KeyCode::Digit4),
    ("digit5", KeyCode::Digit5),
    ("digit6", KeyCode::Digit6),
    ("digit7", KeyCode::Digit7),
    ("digit8", KeyCode::Digit8),
    ("digit9", KeyCode::Digit9),
    ("digit0", KeyCode::Digit0),
];

impl ShortcutPolicy {
    fn same_binding_key(self, left: &BindingKey, right: &BindingKey) -> bool {
        if left == right {
            return true;
        }
        // The GPUI adapter folds stored digitN scancodes into N (including captured
        // Command digits). Match that runtime identity without changing stored rows
        // or the legacy shell's distinct physical/logical keyboard semantics.
        if self.gpui {
            let pair = match (left, right) {
                (
                    BindingKey::Scancode(PhysicalKey::Code(code)),
                    BindingKey::Keycode { key: Key::Character(text), .. },
                )
                | (
                    BindingKey::Keycode { key: Key::Character(text), .. },
                    BindingKey::Scancode(PhysicalKey::Code(code)),
                ) => Some((code, text.as_str())),
                _ => None,
            };
            if let Some((code, text)) = pair {
                return DIGIT_CODES.iter().any(|(name, candidate)| {
                    code == candidate && name.strip_prefix("digit") == Some(text)
                });
            }
        }
        false
    }

    fn same_combo(self, left: &str, right: &str) -> bool {
        match (parse_combo(left), parse_combo(right)) {
            (Some((left_mods, left_key)), Some((right_mods, right_key))) => {
                left_mods == right_mods && self.same_binding_key(&left_key, &right_key)
            },
            (None, None) => true,
            _ => false,
        }
    }
}

/// `ctrl+shift+t` → (mods, key)。`plus` 是 `+` 键的别名（`+` 是分隔符）。
pub(crate) fn parse_combo(combo: &str) -> Option<(ModifiersState, BindingKey)> {
    let mut mods = ModifiersState::empty();
    let mut key: Option<BindingKey> = None;
    for part in combo.split('+') {
        let part = part.trim();
        if part.is_empty() {
            return None;
        }
        match part.to_lowercase().as_str() {
            "ctrl" | "control" => mods |= ModifiersState::CONTROL,
            "shift" => mods |= ModifiersState::SHIFT,
            "alt" => mods |= ModifiersState::ALT,
            "win" | "super" | "cmd" => mods |= ModifiersState::SUPER,
            lower => {
                if key.is_some() {
                    return None;
                }
                key = Some(parse_key_name(&lower)?);
            },
        }
    }
    key.map(|key| (mods, key))
}

fn parse_key_name(lower: &str) -> Option<BindingKey> {
    if let Some((_, code)) = DIGIT_CODES.iter().find(|(name, _)| *name == lower) {
        return Some(BindingKey::Scancode(PhysicalKey::Code(*code)));
    }
    if let Some((_, named)) = NAMED_ALIASES.iter().find(|(name, _)| *name == lower) {
        return Some(BindingKey::Keycode { key: Key::Named(*named), location: KeyLocation::Any });
    }
    let key = match lower {
        "esc" => Key::Named(NamedKey::Escape),
        "plus" => Key::Character("+".into()),
        "minus" => Key::Character("-".into()),
        _ if lower.chars().count() == 1 => Key::Character(lower.into()),
        _ => return None,
    };
    Some(BindingKey::Keycode { key, location: KeyLocation::Any })
}

/// (mods, key) → 存储格式 `ctrl+shift+t`。捕获结果与 parse 往返一致。
pub(crate) fn canonical_combo(mods: ModifiersState, key: &BindingKey) -> Option<String> {
    let mut out = String::new();
    if mods.control_key() {
        out.push_str("ctrl+");
    }
    if mods.shift_key() {
        out.push_str("shift+");
    }
    if mods.alt_key() {
        out.push_str("alt+");
    }
    if mods.super_key() {
        out.push_str("win+");
    }
    let name = key_storage_name(key)?;
    out.push_str(&name);
    Some(out)
}

fn key_storage_name(key: &BindingKey) -> Option<String> {
    match key {
        BindingKey::Scancode(physical) => DIGIT_CODES
            .iter()
            .find(|(_, code)| PhysicalKey::Code(*code) == *physical)
            .map(|(name, _)| (*name).to_owned()),
        BindingKey::Keycode { key: Key::Named(named), .. } => NAMED_ALIASES
            .iter()
            .find(|(_, candidate)| candidate == named)
            .map(|(name, _)| (*name).to_owned()),
        BindingKey::Keycode { key: Key::Character(c), .. } => {
            let c = c.as_str();
            match c {
                "+" => Some("plus".to_owned()),
                _ if c.chars().count() == 1 => Some(c.to_lowercase()),
                _ => None,
            }
        },
        BindingKey::Keycode { .. } => None,
    }
}

/// 设置页和快捷键提示的修饰键名称，按运行平台显示。
pub(crate) fn mods_prefix(mods: ModifiersState) -> String {
    let mut out = String::new();
    if mods.control_key() {
        out.push_str("Ctrl+");
    }
    if mods.shift_key() {
        out.push_str("Shift+");
    }
    if mods.alt_key() {
        out.push_str(if cfg!(target_os = "macos") { "Option+" } else { "Alt+" });
    }
    if mods.super_key() {
        out.push_str(if cfg!(target_os = "macos") {
            "Command+"
        } else if cfg!(windows) {
            "Win+"
        } else {
            "Super+"
        });
    }
    out
}

pub(crate) fn display_combo(mods: ModifiersState, key: &BindingKey) -> Option<String> {
    let mut out = mods_prefix(mods);
    let name = match key {
        BindingKey::Scancode(physical) => DIGIT_CODES
            .iter()
            .find(|(_, code)| PhysicalKey::Code(*code) == *physical)
            .map(|(name, _)| name.trim_start_matches("digit").to_owned())?,
        BindingKey::Keycode { key: Key::Named(named), .. } => {
            let alias = NAMED_ALIASES
                .iter()
                .find(|(_, candidate)| candidate == named)
                .map(|(name, _)| *name)?;
            match alias {
                "up" => "Up".to_owned(),
                "down" => "Down".to_owned(),
                "left" => "Left".to_owned(),
                "right" => "Right".to_owned(),
                "pageup" => "PageUp".to_owned(),
                "pagedown" => "PageDown".to_owned(),
                other => {
                    let mut chars = other.chars();
                    chars
                        .next()
                        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                        .unwrap_or_default()
                },
            }
        },
        BindingKey::Keycode { key: Key::Character(c), .. } => c.to_uppercase(),
        BindingKey::Keycode { .. } => return None,
    };
    out.push_str(&name);
    Some(out)
}

/// `keybind=` 行的动作名 → Action。variant 名大小写不敏感（复用
/// ConfigDeserialize），`none`/`unbound` 显式禁用。
pub(crate) fn parse_action(name: &str) -> Option<Action> {
    use serde::Deserialize as _;
    let name = name.trim();
    if name.eq_ignore_ascii_case("unbound") {
        return Some(Action::None);
    }
    Action::deserialize(toml::Value::String(name.to_owned())).ok()
}

/// Action → 存储名（Debug 名即 variant 名；仅 unit variants 会出现在
/// 自定义表里，Debug 输出必然干净）。
pub(crate) fn action_storage_name(action: &Action) -> String {
    format!("{action:?}")
}

/// raw `keybind=` 行 → 自定义绑定表。非法行静默跳过（与该文件其余键的
/// 容错一致）；行序即优先级（后写的行先匹配）。
pub(crate) fn build_bindings(raw: &[(String, String)]) -> Vec<KeyBinding> {
    let mut bindings = Vec::new();
    for (combo, action) in raw {
        let Some((mods, trigger)) = parse_combo(combo) else { continue };
        let Some(action) = parse_action(action) else { continue };
        bindings.push(KeyBinding {
            trigger,
            mods,
            mode: BindingMode::empty(),
            notmode: BindingMode::empty(),
            action,
        });
    }
    // 后写的行优先：倒序存放，匹配层顺序扫描第一条命中即用。
    bindings.reverse();
    bindings
}

/// 一个动作当前的有效键（设置页展示用）。
/// 返回 (展示文本, 是否来自用户自定义)；None = 未绑定。
pub(crate) fn effective_combo(action: &Action, custom: &[KeyBinding]) -> Option<(String, bool)> {
    LEGACY.effective_combo(action, custom)
}

impl ShortcutPolicy {
    pub(crate) fn effective_combo(
        self,
        action: &Action,
        custom: &[KeyBinding],
    ) -> Option<(String, bool)> {
        // custom 已是倒序（最新在前）。该动作的候选行必须没被更新的同键行
        // 遮蔽（手编文件可能给同一个键堆多行，运行时只有最前面那条生效——
        // 展示必须与匹配层口径一致，否则显示的键按下去没反应）。
        for (index, binding) in custom.iter().enumerate() {
            if binding.action != *action {
                continue;
            }
            let shadowed_by_newer = custom[..index].iter().any(|b| {
                self.same_binding_key(&b.trigger, &binding.trigger) && b.mods == binding.mods
            });
            if !shadowed_by_newer {
                return display_combo(binding.mods, &binding.trigger).map(|text| (text, true));
            }
        }
        // 默认表：倒序扫描让 platform/nebula 键（靠后 extend）压过 vi/legacy
        // 键；跳过 Numpad 专属与 Copy/Paste 媒体键这类不适合展示的触发器。
        let shadowed = |candidate: &KeyBinding| {
            custom.iter().any(|b| {
                self.same_binding_key(&b.trigger, &candidate.trigger) && b.mods == candidate.mods
            })
        };
        self.cached_defaults()
            .iter()
            .rev()
            .filter(|b| b.action == *action)
            .filter(|b| {
                !matches!(
                    &b.trigger,
                    BindingKey::Keycode { key: Key::Named(NamedKey::Copy | NamedKey::Paste), .. }
                        | BindingKey::Keycode { location: KeyLocation::Numpad, .. }
                )
            })
            .find(|b| !shadowed(b))
            .and_then(|b| display_combo(b.mods, &b.trigger).map(|text| (text, false)))
    }
}

// ---- 捕获 ----

/// 设置页捕获态下一次按键的解释结果。
pub(crate) enum CaptureOutcome {
    /// 纯修饰键或无法表示的键：保持捕获态继续等。
    Pending,
    /// Esc：取消捕获，不改绑定。
    Cancel,
    /// Bare Backspace explicitly disables the action's shortcuts.
    ClearCustom,
    /// 得到一个可存储的组合。
    Bind(String),
}

/// 把捕获态里按下的键翻译成存储 combo。
pub(crate) fn capture_combo(key: &KeyEvent, mods: ModifiersState) -> CaptureOutcome {
    match &key.logical_key {
        Key::Named(NamedKey::Escape) if mods.is_empty() => return CaptureOutcome::Cancel,
        Key::Named(NamedKey::Backspace) if mods.is_empty() => return CaptureOutcome::ClearCustom,
        Key::Named(
            NamedKey::Shift
            | NamedKey::Control
            | NamedKey::Alt
            | NamedKey::AltGraph
            | NamedKey::Super
            | NamedKey::Hyper
            | NamedKey::Meta
            | NamedKey::CapsLock
            | NamedKey::NumLock
            | NamedKey::ScrollLock
            | NamedKey::Fn
            | NamedKey::FnLock,
        ) => return CaptureOutcome::Pending,
        _ => {},
    }

    let binding_key = match key.key_without_modifiers() {
        Key::Named(named) => {
            BindingKey::Keycode { key: Key::Named(named), location: KeyLocation::Any }
        },
        Key::Character(c) => {
            let lower = c.to_lowercase();
            let single_letter = lower.chars().count() == 1
                && lower.chars().next().is_some_and(|ch| ch.is_ascii_alphabetic());
            if mods.shift_key() && !single_letter {
                // Shift+数字/符号的 logical key 随布局漂移，只有物理数字键
                // 能稳定表达（LaunchProfile 系的既有裁定）。
                match key.physical_key {
                    PhysicalKey::Code(code)
                        if DIGIT_CODES.iter().any(|(_, digit)| *digit == code) =>
                    {
                        BindingKey::Scancode(key.physical_key)
                    },
                    _ => return CaptureOutcome::Pending,
                }
            } else {
                BindingKey::Keycode {
                    key: Key::Character(lower.into()),
                    location: KeyLocation::Any,
                }
            }
        },
        _ => return CaptureOutcome::Pending,
    };

    match canonical_combo(mods, &binding_key) {
        Some(combo) => CaptureOutcome::Bind(combo),
        None => CaptureOutcome::Pending,
    }
}

/// GPUI 壳的捕获桥：`gpui::Keystroke` → 同一套 [`CaptureOutcome`] 语义
/// （Esc 取消、裸 Backspace 恢复默认、纯修饰键等待、可存储组合落盘）。
///
/// 键名对齐：gpui 在 Windows 把命名键报成小写名（"escape"/"up"/"f5"…），
/// 与 `NAMED_ALIASES` 的存储别名一一对应；单字符键即 `Key::Character`。
/// 两处已知偏差，均不伤可编辑动作（默认表里没有带数字/Shift+符号的）：
/// - 数字键报成 "1".."0"，这里折回 digitN scancode，保证与旧壳存储同构；
/// - Shift+符号（gpui 直接给出变换后的字符并清掉 shift 位）只能按字符
///   存储，旧壳 winit 层不会命中这类行——与旧壳编辑器「无法表示 → 等待」
///   相比是多存了一条死绑定，可接受。
#[cfg(feature = "gpui-shell")]
pub(crate) fn capture_gpui(keystroke: &::gpui::Keystroke) -> CaptureOutcome {
    let mods = gpui_modifiers(&keystroke.modifiers);
    let key = keystroke.key.trim().to_lowercase();
    match key.as_str() {
        "escape" if mods.is_empty() => return CaptureOutcome::Cancel,
        "backspace" if mods.is_empty() => return CaptureOutcome::ClearCustom,
        // 裸修饰键按下在 Windows 走 ModifiersChanged 而非 KeyDown，这里
        // 兜住其它平台的命名差异（"control"/"super"/"meta"…）。
        "control" | "ctrl" | "shift" | "alt" | "alternate" | "super" | "meta" | "command"
        | "win" | "function" | "capslock" | "numlock" | "scrolllock" => {
            return CaptureOutcome::Pending;
        },
        _ => {},
    }
    let storage_name = match key.as_str() {
        "0" => "digit0".to_owned(),
        "1" => "digit1".to_owned(),
        "2" => "digit2".to_owned(),
        "3" => "digit3".to_owned(),
        "4" => "digit4".to_owned(),
        "5" => "digit5".to_owned(),
        "6" => "digit6".to_owned(),
        "7" => "digit7".to_owned(),
        "8" => "digit8".to_owned(),
        "9" => "digit9".to_owned(),
        other => other.to_owned(),
    };
    let Some(binding_key) = parse_key_name(&storage_name) else {
        return CaptureOutcome::Pending;
    };
    match canonical_combo(mods, &binding_key) {
        Some(combo) => CaptureOutcome::Bind(combo),
        None => CaptureOutcome::Pending,
    }
}

/// gpui `Modifiers` → winit `ModifiersState`（位语义一致，直接搬运）。
#[cfg(feature = "gpui-shell")]
pub(crate) fn gpui_modifiers(modifiers: &::gpui::Modifiers) -> ModifiersState {
    let mut mods = ModifiersState::empty();
    if modifiers.control {
        mods |= ModifiersState::CONTROL;
    }
    if modifiers.shift {
        mods |= ModifiersState::SHIFT;
    }
    if modifiers.alt {
        mods |= ModifiersState::ALT;
    }
    if modifiers.platform {
        mods |= ModifiersState::SUPER;
    }
    mods
}

/// gpui `Modifiers` 的修饰键前缀回显（捕获态实时显示 "Ctrl+…"）。
#[cfg(feature = "gpui-shell")]
pub(crate) fn gpui_mods_prefix(modifiers: &::gpui::Modifiers) -> String {
    mods_prefix(gpui_modifiers(modifiers))
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(all(target_os = "macos", feature = "gpui-shell"))]
    fn command_digit_aliases_preserve_ownership_clear_and_reset() {
        use super::*;
        for (logical, physical, action) in [
            ("cmd+0", "cmd+digit0", Action::ResetFontSize),
            ("cmd+1", "cmd+digit1", Action::SelectTab1),
            ("cmd+9", "cmd+digit9", Action::SelectLastTab),
        ] {
            for combo in [logical, physical] {
                let mut raw = vec![(combo.into(), "ReceiveChar".into())];
                let fallback = (action == Action::SelectTab1).then(|| ("Option+1".into(), false));
                assert_eq!(
                    GPUI.effective_combo(&action, &build_bindings(&raw)),
                    fallback,
                    "{combo}"
                );
                GPUI.reset_action(&mut raw, &action);
                assert!(raw.is_empty(), "reset must release either spelling: {combo}");
                assert!(GPUI.effective_combo(&action, &[]).is_some());

                let mut raw = vec![(combo.into(), "ToggleShellPicker".into())];
                GPUI.clear_action(&mut raw, &action);
                assert!(raw.contains(&(combo.into(), "ToggleShellPicker".into())));
                assert!(
                    !raw.iter().any(|(candidate, name)| GPUI.same_combo(candidate, combo)
                        && parse_action(name) == Some(Action::ReceiveChar)),
                    "clearing another action must preserve the explicit owner of {combo}"
                );
                assert_eq!(GPUI.effective_combo(&action, &build_bindings(&raw)), None);
            }
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn control_character_defaults_and_reset_use_command_on_macos() {
        use super::*;
        let defaults = GPUI.default_shortcuts();
        for combo in ["ctrl+k", "ctrl+digit1", "ctrl+1"] {
            assert!(
                !defaults.iter().any(|(candidate, _)| parse_combo(candidate) == parse_combo(combo)),
                "{combo} must not be a GUI default"
            );
        }
        let action = Action::ToggleShellPicker;
        assert_eq!(GPUI.effective_combo(&action, &[]), Some(("Command+K".into(), false)));
        let mut raw = vec![("ctrl+k".into(), "ReceiveChar".into())];
        GPUI.rebind_action(&mut raw, &action, "ctrl+k".into());
        assert_eq!(
            GPUI.effective_combo(&action, &build_bindings(&raw)),
            Some(("Ctrl+K".into(), true))
        );
        GPUI.reset_action(&mut raw, &action);
        assert_eq!(
            GPUI.effective_combo(&action, &build_bindings(&raw)),
            Some(("Command+K".into(), false))
        );
        let mut old_rows = vec![("ctrl+k".into(), "ReceiveChar".into())];
        GPUI.reset_action(&mut old_rows, &action);
        assert_eq!(old_rows, [("ctrl+k".into(), "ReceiveChar".into())]);
        assert_eq!(
            GPUI.effective_combo(&action, &build_bindings(&old_rows)),
            Some(("Command+K".into(), false))
        );
    }

    #[test]
    fn frontend_policies_do_not_share_defaults_or_digit_identity() {
        use super::*;
        let legacy_defaults = default_shortcuts();
        let gpui_defaults = GPUI.default_shortcuts();
        let legacy_picker = effective_combo(&Action::ToggleShellPicker, &[]);
        let gpui_picker = GPUI.effective_combo(&Action::ToggleShellPicker, &[]);
        let (_, logical) = parse_combo("ctrl+1").unwrap();
        let (_, physical) = parse_combo("ctrl+digit1").unwrap();
        let custom = build_bindings(&[
            ("ctrl+1".into(), "ToggleShellPicker".into()),
            ("ctrl+digit1".into(), "ReceiveChar".into()),
        ]);
        for _ in 0..3 {
            assert!(GPUI.same_binding_key(&logical, &physical));
            assert!(!LEGACY.same_binding_key(&logical, &physical));
            assert_eq!(GPUI.default_shortcuts(), gpui_defaults);
            assert_eq!(default_shortcuts(), legacy_defaults);
            assert_eq!(GPUI.effective_combo(&Action::ToggleShellPicker, &[]), gpui_picker);
            assert_eq!(effective_combo(&Action::ToggleShellPicker, &[]), legacy_picker);
            assert_eq!(
                effective_combo(&Action::ToggleShellPicker, &custom),
                Some(("Ctrl+1".into(), true))
            );
            assert_eq!(GPUI.effective_combo(&Action::ToggleShellPicker, &custom), gpui_picker);
        }
        if cfg!(target_os = "macos") {
            assert_eq!(legacy_picker, Some(("Ctrl+K".into(), false)));
            assert_eq!(gpui_picker, Some(("Command+K".into(), false)));
            assert!(READONLY_ROWS[0].2.contains("Ctrl+1"));
            assert!(!GPUI.readonly_rows()[0].2.contains("Ctrl+1"));
            assert_eq!(GPUI.effective_combo(&Action::Quit, &[]), Some(("Command+Q".into(), false)));
        }
    }

    #[test]
    fn rename_binding_moves_persists_clears_and_restores() {
        use super::*;
        let action = Action::RenameTab;
        assert_eq!(effective_combo(&action, &[]), Some(("F2".into(), false)));
        let mut raw = Vec::new();
        rebind_action(&mut raw, &action, "ctrl+alt+r".into());
        let saved = nebula_settings::apply_keybinds("theme=nord\n", &raw);
        let mut restored = nebula_settings::keybind_pairs_from_text(&saved);
        let bindings = build_bindings(&restored);
        assert_eq!(
            effective_combo(&action, &bindings),
            Some((
                if cfg!(target_os = "macos") { "Ctrl+Option+R" } else { "Ctrl+Alt+R" }.into(),
                true
            ))
        );
        let (mods, trigger) = parse_combo("f2").unwrap();
        let f2 = bindings.iter().find(|b| b.mods == mods && b.trigger == trigger).unwrap();
        assert_eq!(f2.action, Action::ReceiveChar, "the old default must reach the CLI");
        clear_action(&mut restored, &action);
        assert_eq!(effective_combo(&action, &build_bindings(&restored)), None);
        reset_action(&mut restored, &action);
        assert_eq!(
            effective_combo(&action, &build_bindings(&restored)),
            Some(("F2".into(), false))
        );
    }

    #[test]
    fn rename_rebinding_preserves_explicit_ownership_of_the_old_key() {
        use super::*;
        let mut raw = vec![("f2".into(), "CreateNewTab".into())];
        rebind_action(&mut raw, &Action::RenameTab, "ctrl+alt+r".into());
        assert_eq!(raw[0], ("f2".into(), "CreateNewTab".into()));
        assert!(
            !raw.iter()
                .any(|(key, action)| key.eq_ignore_ascii_case("f2") && action == "ReceiveChar")
        );
    }

    #[test]
    fn rename_is_editable_and_localized_in_every_language() {
        use super::*;
        let row =
            EDITABLE_ACTIONS.iter().find(|(action, ..)| *action == Action::RenameTab).unwrap();
        for language in super::super::UiLanguage::ALL {
            assert_eq!(
                action_label(row, *language),
                language.text(crate::i18n::Message::CommonRenameTab)
            );
            assert!(!action_label(row, *language).is_empty());
        }
        assert_eq!(action_label(row, super::super::UiLanguage::ZhCn), "重命名标签页");
    }

    #[test]
    fn clearing_ctrl_k_survives_persistence_and_restore_is_explicit() {
        use super::*;
        let action = Action::ToggleShellPicker;
        let mut raw = vec![("ctrl+alt+k".to_owned(), action_storage_name(&action))];
        clear_action(&mut raw, &action);
        let saved = nebula_settings::apply_keybinds("theme=moss\n", &raw);
        let reloaded = nebula_settings::keybind_pairs_from_text(&saved);
        assert_eq!(raw, reloaded);
        let bindings = build_bindings(&reloaded);
        assert!(effective_combo(&action, &bindings).is_none());
        for combo in ["ctrl+k", "ctrl+alt+k"] {
            let (mods, trigger) = parse_combo(combo).unwrap();
            assert!(bindings.iter().any(|binding| binding.mods == mods
                && binding.trigger == trigger
                && binding.action == Action::ReceiveChar));
        }
        reset_action(&mut raw, &action);
        assert!(effective_combo(&action, &build_bindings(&raw)).is_some());
    }

    #[test]
    fn clearing_does_not_steal_a_default_reassigned_to_another_action() {
        use super::*;
        let mut raw = vec![("ctrl+k".to_owned(), "CreateNewTab".to_owned())];
        clear_action(&mut raw, &Action::ToggleShellPicker);
        let (mods, trigger) = parse_combo("ctrl+k").unwrap();
        let bindings = build_bindings(&raw);
        assert_eq!(
            bindings
                .iter()
                .find(|binding| binding.mods == mods && binding.trigger == trigger)
                .unwrap()
                .action,
            Action::CreateNewTab
        );
    }
    use super::*;

    #[test]
    fn combo_roundtrip() {
        for combo in
            ["ctrl+shift+t", "ctrl+tab", "alt+enter", "ctrl+alt+left", "f5", "ctrl+shift+digit1"]
        {
            let (mods, key) = parse_combo(combo).unwrap();
            assert_eq!(canonical_combo(mods, &key).as_deref(), Some(combo), "{combo}");
        }
    }

    #[test]
    fn quick_terminal_default_matches_global_hotkey_api() {
        let parsed = DEFAULT_QUICK_TERMINAL_HOTKEY
            .parse::<global_hotkey::hotkey::HotKey>()
            .expect("default quick-terminal shortcut must stay registrable");
        assert_eq!(parsed.key, global_hotkey::hotkey::Code::Backquote);
        assert!(parsed.mods.contains(global_hotkey::hotkey::Modifiers::CONTROL));
        assert_eq!(display_stored_combo(DEFAULT_QUICK_TERMINAL_HOTKEY), "Ctrl+`");
    }

    #[test]
    fn modifier_labels_follow_the_running_platform_without_changing_bindings() {
        let (mods, key) = parse_combo("ctrl+alt+win+k").unwrap();
        assert_eq!(
            display_combo(mods, &key).as_deref(),
            Some(if cfg!(target_os = "macos") {
                "Ctrl+Option+Command+K"
            } else if cfg!(windows) {
                "Ctrl+Alt+Win+K"
            } else {
                "Ctrl+Alt+Super+K"
            })
        );
        assert_eq!(canonical_combo(mods, &key).as_deref(), Some("ctrl+alt+win+k"));
        assert_eq!(
            READONLY_ROWS[0].2,
            if cfg!(target_os = "macos") {
                "Option+1..9 / Ctrl+1..9 / Command+1..9"
            } else {
                "Alt+1..9 / Ctrl+1..9"
            }
        );
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse_combo("").is_none());
        assert!(parse_combo("ctrl+").is_none());
        assert!(parse_combo("ctrl+foo+t").is_none());
        assert!(parse_combo("notakey").is_none());
    }

    #[cfg(feature = "gpui-shell")]
    #[test]
    fn gpui_capture_records_page_down_instead_of_leaving_it_pending() {
        let keystroke = gpui::Keystroke {
            modifiers: gpui::Modifiers::default(),
            key: "pagedown".to_owned(),
            key_char: None,
        };
        let CaptureOutcome::Bind(combo) = capture_gpui(&keystroke) else {
            panic!("PageDown 应结束录制并生成绑定");
        };
        assert_eq!(combo, "pagedown");
    }

    #[test]
    fn action_names_roundtrip() {
        for (action, ..) in EDITABLE_ACTIONS {
            let name = action_storage_name(action);
            assert_eq!(parse_action(&name).as_ref(), Some(action), "{name}");
        }
        assert_eq!(parse_action("none"), Some(Action::None));
        assert_eq!(parse_action("unbound"), Some(Action::None));
        assert!(parse_action("NotAnAction").is_none());
    }

    #[test]
    fn custom_binding_shadows_default_in_display() {
        // 自定义 Ctrl+Shift+T → SplitRight 后：SplitRight 显示新键（自定义），
        // CreateNewTab 的默认 Ctrl+Shift+T 被遮蔽 → 显示未绑定。
        let raw = vec![("ctrl+shift+t".to_owned(), "SplitRight".to_owned())];
        let custom = build_bindings(&raw);
        assert_eq!(
            effective_combo(&Action::SplitRight, &custom),
            Some(("Ctrl+Shift+T".to_owned(), true))
        );
        assert_eq!(effective_combo(&Action::CreateNewTab, &custom), None);
        // 未受影响的动作照常显示默认键。
        assert_eq!(
            effective_combo(&Action::CloseTab, &custom),
            Some(("Ctrl+Shift+W".to_owned(), false))
        );
    }

    #[test]
    fn none_disables_and_later_lines_win() {
        let raw = vec![
            ("ctrl+shift+w".to_owned(), "SplitDown".to_owned()),
            ("ctrl+shift+w".to_owned(), "none".to_owned()),
        ];
        let custom = build_bindings(&raw);
        // 后写的 none 生效：Ctrl+Shift+W 被禁用，SplitDown 回落默认键。
        assert_eq!(custom.first().map(|b| b.action.clone()), Some(Action::None));
        assert_eq!(
            effective_combo(&Action::SplitDown, &custom),
            Some(("Ctrl+Shift+S".to_owned(), false))
        );
    }

    #[test]
    fn hardcoded_migration_is_complete() {
        // 原 keyboard.rs 硬编码组合逐一在默认表有对应动作（防迁移漏键）。
        let expect = [
            ("ctrl+shift+t", Action::CreateNewTab),
            ("ctrl+shift+w", Action::CloseTab),
            ("ctrl+tab", Action::SelectNextTab),
            ("ctrl+shift+tab", Action::SelectPreviousTab),
            ("ctrl+shift+e", Action::CreateNewWindow),
            ("ctrl+shift+p", Action::ToggleCommandPalette),
            ("ctrl+shift+o", Action::OpenQuickJump),
            ("ctrl+k", Action::ToggleShellPicker),
            ("ctrl+shift+d", Action::SplitRight),
            ("ctrl+shift+s", Action::SplitDown),
            ("ctrl+shift+enter", Action::ToggleZoom),
            ("ctrl+alt+left", Action::FocusPaneLeft),
            ("ctrl+alt+right", Action::FocusPaneRight),
            ("ctrl+alt+up", Action::FocusPaneUp),
            ("ctrl+alt+down", Action::FocusPaneDown),
            ("ctrl+shift+f", Action::ToggleFilesPanel),
            ("ctrl+shift+g", Action::ToggleGitPanel),
            ("ctrl+shift+up", Action::PromptJumpUp),
            ("ctrl+shift+down", Action::PromptJumpDown),
            ("ctrl+1", Action::SelectTab1),
            ("alt+9", Action::SelectTab9),
            ("ctrl+shift+digit1", Action::LaunchProfile1),
            ("ctrl+shift+digit9", Action::LaunchProfile9),
        ];
        let defaults = cached_defaults();
        for (combo, action) in expect {
            let (mods, key) = parse_combo(combo).unwrap();
            assert!(
                defaults.iter().any(|b| b.action == action && b.mods == mods && b.trigger == key),
                "default table is missing {combo} → {action:?}"
            );
        }
    }

    /// Super 键标签按平台显示：macOS 是 Command，Windows 是 Win，Linux 是 Super。
    #[test]
    fn super_modifier_renders_per_platform() {
        let expected = if cfg!(target_os = "macos") {
            "Command+"
        } else if cfg!(windows) {
            "Win+"
        } else {
            "Super+"
        };
        assert_eq!(mods_prefix(ModifiersState::SUPER), expected);
        assert_eq!(
            mods_prefix(ModifiersState::CONTROL | ModifiersState::SUPER),
            format!("Ctrl+{expected}")
        );
    }

    /// 别名条目要能在存储格式里往返：`clear_action` 把表里的写法逐字写进
    /// 设置文件，解析层必须读回同一个组合。
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_command_aliases_round_trip_through_storage() {
        for (combo, _) in MACOS_COMMAND_ALIASES {
            let (mods, key) = parse_combo(combo).unwrap_or_else(|| panic!("{combo} 解析失败"));
            assert!(mods.super_key(), "{combo} 必须是 ⌘ 组合");
            let canonical =
                canonical_combo(mods, &key).unwrap_or_else(|| panic!("{combo} 无法写成存储格式"));
            assert_eq!(parse_combo(&canonical), Some((mods, key)), "{combo} 存储往返不一致");
        }
    }

    /// #238：macOS 上 Shell 选择器除了配置表的 Ctrl+K 还有一条壳自带的
    /// ⌘K。解绑必须把两条都解掉，否则静态默认绑定继续抢键，用户看到的就是
    /// 「解绑了但 ⌘K 照样弹面板」。
    #[cfg(target_os = "macos")]
    #[test]
    fn clearing_releases_the_macos_command_alias() {
        let action = Action::ToggleShellPicker;
        // 设置页仍按配置表的 Ctrl+K 展示：别名只进解绑口径，不进键帽反查。
        assert_eq!(effective_combo(&action, &[]), Some(("Ctrl+K".to_owned(), false)));
        let mut raw = Vec::new();
        clear_action(&mut raw, &action);
        // 解绑使用表里的写法；旧配置和录制结果的等价拼写由 GPUI 层规范化。
        assert!(
            raw.iter().any(|(combo, _)| combo == "cmd+k"),
            "⌘K 必须按表里的写法逐字写回，实际 {raw:?}"
        );
        // Ctrl+K 来自配置表、⌘K 来自别名表，两条都要落成 ReceiveChar。
        for combo in ["ctrl+k", "cmd+k"] {
            let parsed = parse_combo(combo);
            assert!(
                raw.iter().any(|(candidate, name)| parse_combo(candidate) == parsed
                    && parse_action(name) == Some(Action::ReceiveChar)),
                "{combo} 必须落成 ReceiveChar，否则运行时解不掉静态默认绑定"
            );
        }
        assert_eq!(effective_combo(&action, &build_bindings(&raw)), None, "解绑后不应再有有效键");
        reset_action(&mut raw, &action);
        assert!(
            effective_combo(&action, &build_bindings(&raw)).is_some(),
            "恢复要把两组默认键都还回来"
        );
    }
}
