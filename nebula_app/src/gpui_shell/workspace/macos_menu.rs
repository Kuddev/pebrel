use gpui::{App, Menu, MenuItem, OsAction, SystemMenuType};

use super::*;
use crate::i18n::Message;

pub(super) fn init(cx: &mut App) {
    let language = super::workspace_ui_language();
    let text = |message| language.text(message);
    cx.set_menus([
        Menu::new("Pebrel").items([
            MenuItem::action(format!("{} Pebrel", text(Message::CommonAbout)), OpenAbout),
            MenuItem::separator(),
            MenuItem::action(text(Message::CommonSettings), OpenSettings),
            MenuItem::separator(),
            MenuItem::os_submenu(text(Message::CommonMenuServices), SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action(text(Message::CommonHidePebrel), HideApplication),
            MenuItem::action(text(Message::CommonHideOthers), HideOtherApplications),
            MenuItem::separator(),
            MenuItem::action(text(Message::CommonQuitPebrel), QuitApplication),
        ]),
        Menu::new(text(Message::CommonMenuFile)).items([
            MenuItem::action(text(Message::CommonNewWindow), NewWindow),
            MenuItem::action(text(Message::CommonNewTab), NewTerminal),
            MenuItem::separator(),
            MenuItem::action(text(Message::CommonCloseWindow), CloseWindow),
        ]),
        Menu::new(text(Message::CommonMenuEdit)).items([
            MenuItem::os_action(
                text(Message::CommonCut),
                gpui_component::input::Cut,
                OsAction::Cut,
            ),
            MenuItem::os_action(
                text(Message::CommonCopy),
                gpui_component::input::Copy,
                OsAction::Copy,
            ),
            MenuItem::os_action(
                text(Message::CommonPaste),
                gpui_component::input::Paste,
                OsAction::Paste,
            ),
            MenuItem::os_action(
                text(Message::CommonSelectAll),
                gpui_component::input::SelectAll,
                OsAction::SelectAll,
            ),
        ]),
        Menu::new(text(Message::CommonMenuView))
            .items([MenuItem::action(text(Message::EditorFullscreen), ToggleFullscreen)]),
        Menu::new(text(Message::CommonMenuWindow))
            .items([MenuItem::action(text(Message::CommonMenuMinimize), MinimizeWindow)]),
    ]);
}
