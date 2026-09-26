//! Workspace keyboard registration, shared action adaptation and user overrides.

use super::*;

/// 工作区静态默认绑定的 combo 集（[`init`] 的镜像）。撤销已失效的自定义
/// 注入时要排除：gpui 的 NoAction 打在静态默认键上会误杀基础功能。
#[cfg(test)]
pub(super) const STATIC_DEFAULT_COMBOS: &[&str] = &[
    "ctrl-shift-t",
    "ctrl-shift-e",
    "ctrl-shift-w",
    "ctrl-shift-b",
    "ctrl-,",
    "ctrl-shift-p",
    "ctrl-k",
    "ctrl-shift-f",
    "escape",
    "ctrl-shift-d",
    "ctrl-alt-shift-s",
    "f2",
    "ctrl-shift-enter",
    "ctrl-alt-left",
    "ctrl-alt-right",
    "ctrl-alt-up",
    "ctrl-alt-down",
    "ctrl-tab",
    "ctrl-shift-tab",
    "ctrl-shift-pageup",
    "ctrl-shift-pagedown",
    "ctrl-shift-g",
    "ctrl-=",
    "ctrl-+",
    "ctrl--",
    "ctrl-0",
    "ctrl-shift-c",
    #[cfg(not(target_os = "macos"))]
    "ctrl-c",
    "ctrl-v",
    "ctrl-shift-v",
    "alt-enter",
    "ctrl-shift-o",
];

/// 存储格式 combo（`ctrl+shift+t`）→ gpui 绑定串（`ctrl-shift-t`）。键名
/// 两套体系同构（小写命名键 + 单字符）；digitN 折回数字，plus/minus 折回
/// `+`/`-`（`+` 是存储分隔符，必须先占位再替换）。
/// 最后由 GPUI 解析并规范化修饰键别名和顺序，保证旧 Win+、新 Cmd+ 和
/// 录制结果在覆盖/恢复默认时具有同一个运行时身份。
pub(super) fn gpui_binding_combo(combo: &str) -> String {
    let combo = combo
        .to_ascii_lowercase()
        .replace("plus", "\u{1}")
        .replace("minus", "\u{2}")
        .replace('+', "-")
        .replace("digit", "")
        .replace('\u{1}', "+")
        .replace('\u{2}', "-");
    gpui::Keystroke::parse(&combo).map(|key| key.unparse()).unwrap_or(combo)
}

/// 注册工作区快捷键；在 `gpui_component::init` 之后调用一次。
pub(super) fn init(cx: &mut App) {
    cx.bind_keys(default_workspace_bindings());
    // 平台判定复用已有入口；共享表不意味着给其他平台注册 ⌘ 快捷键。
    if crate::platform::Platform::current() == crate::platform::Platform::MacOS {
        bind_macos_command_keys(cx);
    }
    // 退出不属于任何视图，挂全局兜底：⌘Q 与用户自定义的 `keybind=…:Quit`
    // 都落到与托盘退出同一条「先落盘会话与草稿，再停 PTY」的路径。
    cx.on_action(|_: &QuitApp, cx: &mut App| cx.defer(super::windowing::quit_all));
}

/// 工作区静态默认键位表；与 [`STATIC_DEFAULT_COMBOS`] 互为镜像。
pub(super) fn default_workspace_bindings() -> Vec<KeyBinding> {
    let mut bindings = vec![
        KeyBinding::new("ctrl-shift-t", NewTerminal, None),
        KeyBinding::new("ctrl-alt-shift-s", recipes::OpenLayoutRecipes, None),
        KeyBinding::new("ctrl-shift-e", NewWindow, None),
        KeyBinding::new("ctrl-shift-w", CloseActiveTerminal, None),
        KeyBinding::new("ctrl-shift-b", ToggleSidebar, None),
        KeyBinding::new("ctrl-,", OpenSettings, None),
        KeyBinding::new("ctrl-shift-p", ToggleCommandPalette, None),
        KeyBinding::new("ctrl-k", ToggleShellPicker, None),
        KeyBinding::new("ctrl-shift-f", ToggleFileTree, None),
        // Esc 只在命令/Shell 面板打开时关面板。绑成 `None` 会在终端聚焦时
        // 抢走按键，CC/Codex 收不到 0x1b（旧壳无 overlay 时 Esc 一定进 PTY）。
        KeyBinding::new("escape", CloseCommandPalette, Some(PALETTE_KEY_CONTEXT)),
        // 分屏（旧壳 nebula_key_bindings 同键位）：ctrl+shift+d 左右、
        // ctrl+shift+s 上下、ctrl+shift+enter 缩放、ctrl+alt+方向切聚焦。
        KeyBinding::new("ctrl-shift-d", SplitRight, None),
        KeyBinding::new("ctrl-shift-s", SplitDown, None),
        KeyBinding::new("ctrl-shift-enter", ToggleZoom, None),
        KeyBinding::new("ctrl-alt-left", FocusPaneLeft, None),
        KeyBinding::new("ctrl-alt-right", FocusPaneRight, None),
        KeyBinding::new("ctrl-alt-up", FocusPaneUp, None),
        KeyBinding::new("ctrl-alt-down", FocusPaneDown, None),
        KeyBinding::new("ctrl-tab", SelectNextTab, None),
        KeyBinding::new("ctrl-shift-tab", SelectPreviousTab, None),
        // 移动标签与终端回滚翻页分开：回滚只处理不带 Ctrl 的
        // Shift+PageUp / PageDown，带 Ctrl 时移动当前标签。
        KeyBinding::new("ctrl-shift-pageup", MoveTabLeft, None),
        KeyBinding::new("ctrl-shift-pagedown", MoveTabRight, None),
        KeyBinding::new("ctrl-shift-g", ToggleGitPanel, None),
        KeyBinding::new("ctrl-=", IncreaseFontSize, None),
        KeyBinding::new("ctrl-+", IncreaseFontSize, None),
        KeyBinding::new("ctrl--", DecreaseFontSize, None),
        KeyBinding::new("ctrl-0", ResetFontSize, None),
        KeyBinding::new("ctrl-shift-c", CopySelection, None),
        // 复制优先：终端聚焦时有选区复制并清选区，无选区经 handler
        // 的 `cx.propagate()` 落成 ^C。带终端上下文，重命名/输入框聚焦时
        // ctrl+c 归输入框自己（Input -> Copy）。
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-c", CopySelection, Some(crate::gpui_shell::terminal::KEY_CONTEXT)),
        // 终端粘贴只在终端焦点路径命中。Input 自己带 `Input -> Paste`；这里若
        // 无上下文，会因注册更晚而抢走弹窗/设置页输入框的 Ctrl+V。
        KeyBinding::new("ctrl-v", PasteClipboard, Some(crate::gpui_shell::terminal::KEY_CONTEXT)),
        KeyBinding::new(
            "ctrl-shift-v",
            PasteClipboard,
            Some(crate::gpui_shell::terminal::KEY_CONTEXT),
        ),
        KeyBinding::new("ctrl-shift-v", gpui_component::input::Paste, Some("Input")),
        KeyBinding::new("alt-enter", ToggleFullscreen, None),
        KeyBinding::new("ctrl-shift-o", OpenQuickJump, None),
    ];
    // Tab selection and rename use the same defaults displayed by Settings.
    bindings.extend(crate::display::keymap::default_shortcuts().into_iter().filter_map(
        |(combo, action)| {
            if action == crate::config::Action::RenameTab {
                return custom_workspace_binding(&combo, &action);
            }
            let action = SelectTab::from_config(&action)?;
            Some(KeyBinding::new(&gpui_binding_combo(&combo), action, None))
        },
    ));
    bindings
}

/// macOS 的原生修饰键是 ⌘：在 Ctrl 绑定之外**追加**一套 ⌘ 绑定，不替换。
/// 追加而非替换有两个原因：Ctrl+Shift 组合在 Mac 终端里没有别的含义，留着
/// 不碍事；而 ⌘C/⌘V 必须存在，否则 Mac 用户第一反应就是「复制粘贴坏了」。
/// 终端里的 Ctrl+C 仍然是 SIGINT——这里只绑 ⌘，不碰 Ctrl 的语义。
///
/// 键位来自 `display::keymap::MACOS_COMMAND_ALIASES`：设置页的反查、解绑与
/// 恢复读的是同一张表，两处不会再各自漂移。注册必须留在这里、且早于用户
/// 自定义键，这样 `clear_action` 注入的 NoAction 才压得住静态默认绑定。
fn bind_macos_command_keys(cx: &mut App) {
    let mut bindings: Vec<KeyBinding> = crate::display::keymap::MACOS_COMMAND_ALIASES
        .iter()
        .filter_map(|(combo, action)| workspace_binding_in_context(combo, action, None))
        .collect();
    // 剩下这些没有对应的 `config::Action`，或者需要单独的作用域。
    bindings.extend([
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        // 设置页与对话框的输入框自带 ⌘V，作用域必须和终端分开。
        KeyBinding::new("cmd-v", gpui_component::input::Paste, Some("Input")),
    ]);
    cx.bind_keys(bindings);
}

/// Typed GPUI adapter for the shared numbered/last-tab actions. The existing
/// keybind format remains the authority; this is not another persisted action.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = nebula_workspace, no_json)]
pub(super) struct SelectTab {
    index: Option<usize>,
}

/// 退出应用，macOS 上绑 ⌘Q（`Action::Quit` 的 GPUI 侧落点）。处理注册在
/// [`init`] 的全局兜底上，与绑定同一处维护。
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = nebula_workspace, no_json)]
pub(super) struct QuitApp;

impl SelectTab {
    fn from_config(action: &crate::config::Action) -> Option<Self> {
        use crate::config::Action;
        let index = match action {
            Action::SelectTab1 => Some(0),
            Action::SelectTab2 => Some(1),
            Action::SelectTab3 => Some(2),
            Action::SelectTab4 => Some(3),
            Action::SelectTab5 => Some(4),
            Action::SelectTab6 => Some(5),
            Action::SelectTab7 => Some(6),
            Action::SelectTab8 => Some(7),
            Action::SelectTab9 => Some(8),
            Action::SelectLastTab => None,
            _ => return None,
        };
        Some(Self { index })
    }

    pub(super) fn index_for(&self, tab_count: usize) -> Option<usize> {
        self.index.or_else(|| tab_count.checked_sub(1)).filter(|&index| index < tab_count)
    }
}

#[cfg(test)]
mod tests;

pub(super) fn custom_workspace_binding(
    combo: &str,
    action: &crate::config::Action,
) -> Option<KeyBinding> {
    workspace_binding_in_context(combo, action, None)
}

fn workspace_binding_in_context(
    combo: &str,
    action: &crate::config::Action,
    scope: Option<&str>,
) -> Option<KeyBinding> {
    use crate::config::Action;
    let combo = gpui_binding_combo(combo);
    if let Some(action) = SelectTab::from_config(action) {
        return Some(KeyBinding::new(&combo, action, scope));
    }
    match action {
        Action::ToggleCommandPalette => Some(KeyBinding::new(&combo, ToggleCommandPalette, scope)),
        Action::ToggleShellPicker => Some(KeyBinding::new(&combo, ToggleShellPicker, scope)),
        Action::CreateNewTab => Some(KeyBinding::new(&combo, NewTerminal, scope)),
        Action::CreateNewWindow => Some(KeyBinding::new(&combo, NewWindow, scope)),
        Action::CloseTab => Some(KeyBinding::new(&combo, CloseActiveTerminal, scope)),
        Action::RenameTab => Some(KeyBinding::new(&combo, RenameActiveTab, scope)),
        Action::ToggleFilesPanel => Some(KeyBinding::new(&combo, ToggleFileTree, scope)),
        Action::ToggleGitPanel => Some(KeyBinding::new(&combo, ToggleGitPanel, scope)),
        Action::SplitRight => Some(KeyBinding::new(&combo, SplitRight, scope)),
        Action::SplitDown => Some(KeyBinding::new(&combo, SplitDown, scope)),
        Action::ToggleZoom => Some(KeyBinding::new(&combo, ToggleZoom, scope)),
        Action::FocusPaneLeft => Some(KeyBinding::new(&combo, FocusPaneLeft, scope)),
        Action::FocusPaneRight => Some(KeyBinding::new(&combo, FocusPaneRight, scope)),
        Action::FocusPaneUp => Some(KeyBinding::new(&combo, FocusPaneUp, scope)),
        Action::FocusPaneDown => Some(KeyBinding::new(&combo, FocusPaneDown, scope)),
        Action::SelectNextTab => Some(KeyBinding::new(&combo, SelectNextTab, scope)),
        Action::SelectPreviousTab => Some(KeyBinding::new(&combo, SelectPreviousTab, scope)),
        Action::IncreaseFontSize => Some(KeyBinding::new(&combo, IncreaseFontSize, scope)),
        Action::DecreaseFontSize => Some(KeyBinding::new(&combo, DecreaseFontSize, scope)),
        Action::ResetFontSize => Some(KeyBinding::new(&combo, ResetFontSize, scope)),
        Action::Copy => Some(KeyBinding::new(
            &combo,
            CopySelection,
            scope.or(Some(crate::gpui_shell::terminal::KEY_CONTEXT)),
        )),
        Action::Paste => Some(KeyBinding::new(
            &combo,
            PasteClipboard,
            scope.or(Some(crate::gpui_shell::terminal::KEY_CONTEXT)),
        )),
        Action::ToggleFullscreen => Some(KeyBinding::new(&combo, ToggleFullscreen, scope)),
        Action::OpenQuickJump => Some(KeyBinding::new(&combo, OpenQuickJump, scope)),
        // macOS 的退出键走与托盘退出同一条路径：先落盘会话与草稿，再停 PTY。
        Action::Quit => Some(KeyBinding::new(&combo, QuitApp, scope)),
        // 屏蔽应用动作后，ReceiveChar 仍交给终端编码，保留改键释放旧键的语义。
        Action::None | Action::ReceiveChar => Some(KeyBinding::new(&combo, gpui::NoAction, scope)),
        _ => None,
    }
}

impl NebulaWorkspace {
    pub(super) fn apply_custom_keybinds(&mut self, cx: &mut Context<Self>) {
        self.update_keybinds(nebula_settings::keybind_pairs(), cx);
    }

    fn update_keybinds(&mut self, raw: Vec<(String, String)>, cx: &mut Context<Self>) {
        let applied: Vec<_> = raw.iter().map(|(combo, _)| gpui_binding_combo(combo)).collect();
        let defaults = crate::display::keymap::default_shortcuts();
        let mut bindings = Vec::new();
        for stale in self.custom_keybinds_applied.iter().filter(|combo| !applied.contains(combo)) {
            let restored =
                defaults.iter().rev().find(|(combo, _)| gpui_binding_combo(combo) == *stale);
            if let Some((combo, action)) = restored {
                for scope in [None, Some(crate::gpui_shell::terminal::KEY_CONTEXT)] {
                    if let Some(binding) = workspace_binding_in_context(combo, action, scope) {
                        bindings.push(binding);
                    }
                }
            } else {
                for scope in [None, Some(crate::gpui_shell::terminal::KEY_CONTEXT)] {
                    bindings.push(KeyBinding::new(stale, gpui::NoAction, scope));
                }
            }
        }
        for (combo, action) in raw {
            let Some(action) = crate::display::keymap::parse_action(&action) else { continue };
            if crate::display::keymap::parse_combo(&combo).is_none() {
                continue;
            }
            // Register at both workspace and terminal depth so a cleared default
            // cannot continue to intercept input through a more specific context.
            for scope in [None, Some(crate::gpui_shell::terminal::KEY_CONTEXT)] {
                if let Some(binding) = workspace_binding_in_context(&combo, &action, scope) {
                    bindings.push(binding);
                }
            }
        }
        self.custom_keybinds_applied = applied;
        cx.bind_keys(bindings);
    }
}
