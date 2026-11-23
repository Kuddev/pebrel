use super::*;
use crate::display::command_palette::PaletteAction;
use crate::gpui_shell::config::Settings;
use crate::i18n::UiLanguage;
use gpui::{BorrowAppContext as _, TestAppContext};

#[gpui::test]
fn command_palette_labels_follow_the_active_language(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        super::super::init(cx);
        windowing::initialize(cx, crate::runtime_api::RuntimeHub::new());
        let runtime = pebrel_settings::RuntimeSettings::from_raw(&Default::default());
        let mut settings = Settings::load_with_runtime(pebrel_settings::ThemeName::Nord, runtime);
        settings.ui_language = UiLanguage::EnUs;
        cx.set_global(settings);
    });
    let mut workspace = None;
    let (_, mut window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            NebulaWorkspace::new(
                window,
                None,
                None,
                1,
                crate::runtime_api::RuntimeHub::new(),
                windowing::WorkspaceStartup::Empty,
                windowing::WindowRole::Regular,
                cx,
            )
        });
        workspace = Some(view.clone());
        Root::new(view, window, cx)
    });
    let workspace = workspace.unwrap();
    window.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            let row = |workspace: &NebulaWorkspace, cx: &App, action: PaletteAction| {
                workspace
                    .filtered_palette_rows(cx)
                    .into_iter()
                    .find(|row| {
                        matches!(&row.action, WorkspacePaletteAction::Shared(value) if *value == action)
                    })
                    .expect("built-in command must remain available")
            };
            let english = row(workspace, cx, PaletteAction::NewTab);
            assert_eq!(english.label, "New tab");
            assert_eq!(row(workspace, cx, PaletteAction::CloseTab).label, "Close tab");
            assert_eq!(row(workspace, cx, PaletteAction::NextTab).label, "Next tab");

            cx.update_global::<Settings, _>(|settings, _| settings.ui_language = UiLanguage::ZhCn);
            let chinese = row(workspace, cx, PaletteAction::NewTab);
            assert_eq!(chinese.label, "新建标签页");
            assert_eq!(chinese.search, english.search);
            assert_eq!(chinese.hint, english.hint);

            cx.update_global::<Settings, _>(|settings, _| settings.ui_language = UiLanguage::EnUs);
            assert_eq!(row(workspace, cx, PaletteAction::NewTab).label, "New tab");
        });
    });
}
