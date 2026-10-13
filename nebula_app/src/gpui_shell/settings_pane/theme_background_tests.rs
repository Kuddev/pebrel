//! Real controls, picker lifetimes, and Save/Apply publication boundaries.
use super::*;

fn reveal(selector: &'static str, window: &mut VisualTestContext) {
    reveal_editor_control(selector, window);
}

pub(super) fn choose_image(path: &std::path::Path, window: &mut VisualTestContext) {
    reveal("theme-editor-image-choose", window);
    click("theme-editor-image-choose", window);
    assert!(window.did_prompt_for_paths());
    respond_with_image_path(path, window);
}

fn respond_with_image_path(path: &std::path::Path, window: &mut VisualTestContext) {
    let path = path.to_owned();
    window.simulate_path_prompt_response(move |options| {
        assert!(options.files);
        assert!(!options.directories);
        assert!(!options.multiple, "a theme selects one background image");
        Some(vec![path])
    });
    draw(window);
}

#[gpui::test]
fn background_controls_are_draft_only_and_native_export_keeps_effects(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    let settings_before = settings_file_snapshot();
    let library_before = custom_theme_ids();
    open_theme_editor(&mut window);
    reveal("theme-editor-advanced-toggle", &mut window);
    click("theme-editor-advanced-toggle", &mut window);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("背景.png");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([20, 40, 80, 255])).save(&path).unwrap();
    choose_image(&path, &mut window);
    assert!(pane.read_with(&mut window, |pane, cx| {
        pane.theme_editor.as_ref().unwrap().background_preview.read(cx).ready()
    }));
    assert!(
        window.debug_bounds("theme-editor-wallpaper-preview").is_some(),
        "the right preview paints the selected image"
    );
    reveal("theme-editor-image-opacity", &mut window);
    edit_input("theme-editor-image-opacity", "42%", &mut window);
    reveal("theme-editor-blur", &mut window);
    click("theme-editor-blur", &mut window);
    press("down", &mut window);
    press("enter", &mut window);
    let draft = editor_draft(&pane, &mut window);
    assert_eq!(draft.effects.background_image.as_deref(), path.to_str());
    assert_eq!(draft.effects.background_image_opacity, Some(0.42));
    assert_eq!(draft.effects.blur, Some(nebula_settings::BlurModeName::None));
    assert_eq!(settings_file_snapshot(), settings_before);
    let document = editor_document(&pane, &mut window);
    let artifact = crate::theme_library::export(&document, ThemeFormat::Pebrel).unwrap();
    let imported = crate::theme_library::inspect(&artifact.text, "background.pebrel-theme.json");
    assert_eq!(imported.candidates[0].document.definition().unwrap().effects, draft.effects);

    reveal("theme-editor-image-opacity", &mut window);
    edit_input("theme-editor-image-opacity", "101%", &mut window);
    assert!(
        pane.read_with(&mut window, |pane, _| pane.theme_editor.as_ref().unwrap().error.is_some())
    );
    // Input changes while typing: 1 and 10 are valid before 101 becomes
    // invalid. The last valid draft remains, but invalid visible text must
    // prevent saving that draft.
    assert_eq!(editor_draft(&pane, &mut window).effects.background_image_opacity, Some(0.1));
    click("theme-editor-save", &mut window);
    assert_eq!(settings_file_snapshot(), settings_before);
    assert_eq!(
        custom_theme_ids(),
        library_before,
        "invalid opacity must not save the last valid draft"
    );
    edit_input("theme-editor-image-opacity", "0%", &mut window);
    assert_eq!(editor_draft(&pane, &mut window).effects.background_image_opacity, Some(0.0));
    reveal("theme-editor-image-clear", &mut window);
    click("theme-editor-image-clear", &mut window);
    assert_eq!(editor_draft(&pane, &mut window).effects.background_image.as_deref(), Some(""));
    assert!(
        window.debug_bounds("theme-editor-wallpaper-preview").is_none(),
        "removal also clears the preview"
    );
    assert_eq!(settings_file_snapshot(), settings_before);
}

#[gpui::test]
fn invalid_background_shows_preview_error_without_publishing_settings(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    let before_settings = settings_file_snapshot();
    open_theme_editor(&mut window);
    reveal("theme-editor-advanced-toggle", &mut window);
    click("theme-editor-advanced-toggle", &mut window);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid.png");
    std::fs::write(&path, b"not an image").unwrap();
    choose_image(&path, &mut window);
    assert!(window.debug_bounds("theme-editor-wallpaper-preview").is_none());
    assert_eq!(
        pane.read_with(&mut window, |pane, cx| pane
            .theme_editor
            .as_ref()
            .unwrap()
            .background_preview
            .read(cx)
            .status_message()),
        Some(crate::i18n::Message::WallpaperLoadFailed)
    );
    assert_eq!(settings_file_snapshot(), before_settings);
}

#[gpui::test]
fn background_save_apply_publishes_image_and_independent_opacities(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let _settings_guard = SettingsBytesGuard::capture();
    std::fs::create_dir_all(nebula_settings::settings_dir()).unwrap();
    std::fs::write(nebula_settings::settings_path(), TEST_SETTINGS).unwrap();
    let mut cleanup = CustomThemeCleanup::default();
    let before_ids = custom_theme_ids();
    let (pane, mut window) = open_settings(cx);
    open_theme_editor(&mut window);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("applied.png");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([20, 40, 80, 255])).save(&path).unwrap();
    edit_input("theme-editor-opacity", "80%", &mut window);
    reveal("theme-editor-advanced-toggle", &mut window);
    click("theme-editor-advanced-toggle", &mut window);
    choose_image(&path, &mut window);
    reveal("theme-editor-image-opacity", &mut window);
    edit_input("theme-editor-image-opacity", "35%", &mut window);
    let before_settings = settings_file_snapshot();
    assert_eq!(RuntimeSettings::load().background_image, None);
    click("theme-editor-save-apply", &mut window);
    for document in custom_theme_documents() {
        if document.id().is_some_and(|id| !before_ids.iter().any(|old| old == id)) {
            cleanup.track(&document);
        }
    }
    assert!(pane.read_with(&mut window, |pane, _| pane.theme_editor.is_none()));
    let runtime = RuntimeSettings::load();
    assert_eq!(runtime.background_image.as_deref(), path.to_str());
    assert_eq!(runtime.background_image_opacity, 0.35);
    assert_eq!(runtime.opacity, 0.8);
    assert_ne!(settings_file_snapshot(), before_settings);
    let applied =
        ThemeLibraryStore::default().load(runtime.custom_theme.as_deref().unwrap()).unwrap();
    assert_eq!(applied.definition().unwrap().effects.background_image.as_deref(), path.to_str());
}

#[gpui::test]
fn late_background_picker_cannot_change_a_reopened_editor(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    let before_settings = settings_file_snapshot();
    open_theme_editor(&mut window);
    reveal("theme-editor-advanced-toggle", &mut window);
    click("theme-editor-advanced-toggle", &mut window);
    reveal("theme-editor-image-choose", &mut window);
    click("theme-editor-image-choose", &mut window);
    assert!(window.did_prompt_for_paths());
    click("theme-editor-cancel", &mut window);
    open_theme_editor(&mut window);
    let before = editor_draft(&pane, &mut window);
    respond_with_image_path(std::path::Path::new("late.png"), &mut window);
    assert_eq!(editor_draft(&pane, &mut window), before);
    assert_eq!(settings_file_snapshot(), before_settings);
}
