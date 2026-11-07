use super::*;

fn choose_saved_theme(
    pane: &Entity<SettingsPane>,
    window: &mut VisualTestContext,
    document: &ThemeDocument,
) {
    if window.debug_bounds("appearance-picker-dialog").is_none() {
        click("open-theme-picker", window);
    }
    click("appearance-filter-3", window);
    let selector = format!("appearance-option-{}", document.name());
    let bounds = window
        .update(|window, _| window.debug_bounds_for_selector(&selector))
        .expect("saved theme card");
    if let Some(viewport) = window.debug_bounds("appearance-theme-list-scroll") {
        if bounds.top() < viewport.top() || bounds.bottom() > viewport.bottom() {
            window.simulate_event(gpui::ScrollWheelEvent {
                position: viewport.center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(
                    px(0.0),
                    viewport.center().y - bounds.center().y,
                )),
                touch_phase: gpui::TouchPhase::Moved,
                modifiers: Modifiers::default(),
            });
            draw(window);
        }
    }
    let bounds = window
        .update(|window, _| window.debug_bounds_for_selector(&selector))
        .expect("visible saved theme card");
    click_point(bounds.center(), window);
    pane.read_with(window, |pane, _| {
        let picker = pane.appearance_picker.as_ref().unwrap();
        assert_eq!(picker.custom_document(picker.draft).unwrap().id(), document.id());
    });
}

fn saved_fixture(name: &str, cleanup: &mut CustomThemeCleanup) -> ThemeDocument {
    let document = ThemeLibraryStore::default()
        .import(&transfer_fixture_document("management"), Some(name))
        .unwrap();
    cleanup.track(&document);
    document
}

#[gpui::test]
fn saved_theme_can_be_edited_twice_without_creating_copies(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let _settings_guard = SettingsBytesGuard::capture();
    std::fs::write(nebula_settings::settings_path(), TEST_SETTINGS).unwrap();
    let mut cleanup = CustomThemeCleanup::default();
    let source = saved_fixture("000000-management-edit", &mut cleanup);
    let before_ids = custom_theme_ids();
    let store = ThemeLibraryStore::default();
    let (pane, mut window) = open_settings(cx);
    choose_saved_theme(&pane, &mut window, &source);
    let before_runtime = runtime_snapshot();
    let before_palette = palette_snapshot(&mut window);
    press("tab", &mut window);
    press("enter", &mut window);
    assert_eq!(editor_draft(&pane, &mut window).name, source.name());
    edit_input("theme-editor-foreground", "#aaccee", &mut window);
    click("theme-editor-save", &mut window);
    let saved = store.load(source.id().unwrap()).unwrap();
    assert_eq!(saved.revision(), source.revision() + 1);
    assert_eq!(saved.definition().unwrap().terminal.foreground, [0xaa, 0xcc, 0xee]);
    assert_eq!(custom_theme_ids(), before_ids);
    assert_eq!(runtime_snapshot(), before_runtime);
    assert_eq!(palette_snapshot(&mut window), before_palette);

    click("theme-editor-back", &mut window);
    click("edit-custom-theme", &mut window);
    assert_eq!(editor_draft(&pane, &mut window).terminal.foreground, [0xaa, 0xcc, 0xee]);
    edit_input("theme-editor-foreground", "#bbddff", &mut window);
    click("theme-editor-save-apply", &mut window);
    assert!(window.debug_bounds("theme-editor-dialog").is_none());
    assert_eq!(RuntimeSettings::load().custom_theme.as_deref(), source.id());
    let saved_again = store.load(source.id().unwrap()).unwrap();
    assert_eq!(saved_again.revision(), source.revision() + 2);
    assert_eq!(saved_again.definition().unwrap().terminal.foreground, [0xbb, 0xdd, 0xff]);
    assert_eq!(custom_theme_ids(), before_ids);
}

#[gpui::test]
fn saved_theme_delete_can_be_canceled_and_refreshes_custom_selection(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let _settings_guard = SettingsBytesGuard::capture();
    std::fs::write(nebula_settings::settings_path(), TEST_SETTINGS).unwrap();
    let mut cleanup = CustomThemeCleanup::default();
    let source = saved_fixture("000000-management-delete", &mut cleanup);
    let remaining = saved_fixture("000001-management-remaining", &mut cleanup);
    let (pane, mut window) = open_settings(cx);
    choose_saved_theme(&pane, &mut window, &source);
    let before = settings_file_snapshot();
    click("delete-custom-theme", &mut window);
    press("escape", &mut window);
    assert_eq!(ThemeLibraryStore::default().load(source.id().unwrap()).unwrap(), source);
    assert_eq!(settings_file_snapshot(), before);
    click("delete-custom-theme", &mut window);
    click("confirm-dialog-ok", &mut window);
    assert!(ThemeLibraryStore::default().load(source.id().unwrap()).is_err());
    assert_eq!(settings_file_snapshot(), before);
    pane.read_with(&mut window, |pane, _| {
        let picker = pane.appearance_picker.as_ref().unwrap();
        assert!(picker.custom_themes.iter().all(|theme| theme.id() != source.id()));
        assert!(!picker.apply_busy);
    });
    choose_saved_theme(&pane, &mut window, &remaining);
    click("apply-appearance-picker", &mut window);
    assert_eq!(RuntimeSettings::load().custom_theme.as_deref(), remaining.id());
}

#[gpui::test]
fn deleting_active_theme_switches_to_default_and_busy_failure_can_be_retried(
    cx: &mut TestAppContext,
) {
    let _fixture_guard = lock_theme_studio();
    let _settings_guard = SettingsBytesGuard::capture();
    std::fs::write(nebula_settings::settings_path(), TEST_SETTINGS).unwrap();
    let mut cleanup = CustomThemeCleanup::default();
    let source = saved_fixture("000000-management-active", &mut cleanup);
    let store = ThemeLibraryStore::default();
    let (pane, mut window) = open_settings(cx);
    choose_saved_theme(&pane, &mut window, &source);
    click("apply-appearance-picker", &mut window);
    assert_eq!(RuntimeSettings::load().custom_theme.as_deref(), source.id());
    choose_saved_theme(&pane, &mut window, &source);
    let lock =
        crate::atomic_file::try_lifetime_lock(&store.path_for(source.id().unwrap()).unwrap())
            .unwrap()
            .unwrap();
    click("delete-custom-theme", &mut window);
    click("confirm-dialog-ok", &mut window);
    assert_eq!(RuntimeSettings::load().theme, ThemeName::Nord);
    assert_eq!(RuntimeSettings::load().custom_theme, None);
    assert_eq!(store.load(source.id().unwrap()).unwrap(), source);
    pane.read_with(&mut window, |pane, _| {
        let picker = pane.appearance_picker.as_ref().unwrap();
        assert!(picker.error.is_some());
        assert!(!picker.apply_busy);
    });
    drop(lock);
    click("delete-custom-theme", &mut window);
    click("confirm-dialog-ok", &mut window);
    assert!(store.load(source.id().unwrap()).is_err());
    pane.read_with(&mut window, |pane, _| {
        let picker = pane.appearance_picker.as_ref().unwrap();
        assert_eq!(picker.draft, appearance_picker::AppearanceSelection::Theme(ThemeName::Nord));
        assert!(picker.error.is_none());
        assert!(!picker.apply_busy);
    });
}

#[gpui::test]
fn saved_theme_edit_and_delete_controls_fit_a_narrow_picker(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let _settings_guard = SettingsBytesGuard::capture();
    std::fs::write(nebula_settings::settings_path(), TEST_SETTINGS).unwrap();
    let mut cleanup = CustomThemeCleanup::default();
    let source = saved_fixture("000000-management-narrow", &mut cleanup);
    let (pane, mut window) = open_settings(cx);
    choose_saved_theme(&pane, &mut window, &source);
    window.simulate_resize(size(px(360.0), px(800.0)));
    draw(&mut window);
    let dialog = window.debug_bounds("appearance-picker-dialog").unwrap();
    let edit = window.debug_bounds("edit-custom-theme").unwrap();
    let delete = window.debug_bounds("delete-custom-theme").unwrap();
    assert!(edit.left() >= dialog.left() && delete.right() <= dialog.right());
    assert!(edit.right() <= delete.left());
    assert!(edit.top() >= dialog.top() && delete.bottom() <= dialog.bottom());
    click("edit-custom-theme", &mut window);
    assert_eq!(editor_draft(&pane, &mut window).name, source.name());
}
