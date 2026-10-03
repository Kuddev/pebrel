//! Native package actions, transaction boundaries, and picker cancellation.
use super::*;
use crate::theme_library::package::{Author, CheckedPackage, export_package};
use std::path::{Path, PathBuf};

fn choose(path: &Path, window: &mut VisualTestContext) {
    assert!(window.did_prompt_for_paths());
    let path = path.to_owned();
    window.simulate_path_prompt_response(move |options| {
        assert!(options.files && !options.directories && !options.multiple);
        Some(vec![path])
    });
    draw(window);
}

fn fixture(dir: &Path, name: &str) -> PathBuf {
    let image = dir.join("background.png");
    image::RgbaImage::from_pixel(4, 3, image::Rgba([10, 20, 30, 255])).save(&image).unwrap();
    let mut definition = ThemeDefinition::from_builtin(ThemeName::Nord);
    definition.name = name.to_owned();
    definition.effects.background_image = Some(image.to_str().unwrap().to_owned());
    let document = crate::theme_library::from_definition(&definition).unwrap();
    let output = dir.join("fixture.pebrel-theme.zip");
    export_package(
        &document,
        Author { name: "Fixture author".into(), github: None },
        "1.0.0".into(),
        "MIT".into(),
        &output,
        None,
    )
    .unwrap();
    std::fs::remove_file(image).unwrap();
    output
}

fn open_import(window: &mut VisualTestContext) {
    click("theme-editor-package", window);
    click("theme-package-import-mode", window);
    click("theme-package-source-choose", window);
}

#[gpui::test]
fn zip_export_shares_background_preview_and_metadata_without_publishing(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    let before = settings_file_snapshot();
    let ids = custom_theme_ids();
    open_theme_editor(&mut window);
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("chosen.png");
    image::RgbaImage::from_pixel(4, 3, image::Rgba([10, 20, 30, 255])).save(&image).unwrap();
    reveal_editor_control("theme-editor-advanced-toggle", &mut window);
    click("theme-editor-advanced-toggle", &mut window);
    background_tests::choose_image(&image, &mut window);
    let original = editor_draft(&pane, &mut window);
    click("theme-editor-package", &mut window);
    click("theme-package-confirm", &mut window);
    assert!(
        pane.read_with(&mut window, |pane, _| pane
            .theme_package
            .as_ref()
            .unwrap()
            .output
            .is_none())
    );
    edit_input("theme-package-author", "Native author", &mut window);
    edit_input("theme-package-license", "MIT", &mut window);
    click("theme-package-preview-choose", &mut window);
    choose(&image, &mut window);
    click("theme-package-confirm", &mut window);
    let output = dir.path().join("shared.pebrel-theme.zip");
    let picked = output.clone();
    window.simulate_new_path_selection(move |_| Some(picked));
    draw(&mut window);
    assert!(window.debug_bounds("theme-package-success").is_some());
    let package = CheckedPackage::open(&output).unwrap();
    assert_eq!(package.manifest.author.name, "Native author");
    assert_eq!(package.manifest.license, "MIT");
    assert_eq!(package.manifest.resources.len(), 2);
    assert_eq!(
        package.document.definition().unwrap().effects.background_image.as_deref(),
        Some("assets/background.png")
    );
    assert_eq!(settings_file_snapshot(), before);
    assert_eq!(custom_theme_ids(), ids);
    assert_eq!(editor_draft(&pane, &mut window), original);
}

#[gpui::test]
fn zip_import_waits_for_confirmation_and_repeated_clicks_install_once(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let _settings_guard = SettingsBytesGuard::capture();
    std::fs::create_dir_all(nebula_settings::settings_dir()).unwrap();
    std::fs::write(nebula_settings::settings_path(), TEST_SETTINGS).unwrap();
    let mut cleanup = CustomThemeCleanup::default();
    let dir = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), &format!("Package native test {}", std::process::id()));
    let ids = custom_theme_ids();
    let before = settings_file_snapshot();
    let (pane, mut window) = open_settings(cx);
    open_theme_editor(&mut window);
    open_import(&mut window);
    choose(&path, &mut window);
    assert!(pane.read_with(&mut window, |pane, _| {
        pane.theme_package.as_ref().unwrap().inspected.is_some()
    }));
    assert_eq!(custom_theme_ids(), ids);
    assert_eq!(settings_file_snapshot(), before);
    let hit = window.debug_bounds("theme-package-confirm").unwrap();
    for _ in 0..2 {
        window.simulate_mouse_down(hit.center(), MouseButton::Left, Modifiers::default());
        window.simulate_mouse_up(hit.center(), MouseButton::Left, Modifiers::default());
    }
    draw(&mut window);
    let installed = pane.read_with(&mut window, |pane, _| {
        let state = pane.theme_package.as_ref().unwrap();
        state.installed.clone().unwrap_or_else(|| {
            panic!(
                "installation did not finish; phase={}, error={:?}, confirm={hit:?}",
                state.phase.busy(),
                state.error
            )
        })
    });
    cleanup.track(&installed);
    assert_eq!(custom_theme_ids().len(), ids.len() + 1);
    assert_eq!(settings_file_snapshot(), before);
    let background =
        PathBuf::from(installed.definition().unwrap().effects.background_image.unwrap());
    assert!(background.is_file());
    click("theme-package-edit", &mut window);
    assert!(window.debug_bounds("theme-package-dialog").is_none());
    assert_eq!(
        editor_draft(&pane, &mut window).effects.background_image.as_deref(),
        background.to_str()
    );
    assert!(window.debug_bounds("theme-editor-wallpaper-preview").is_some());
    assert_eq!(settings_file_snapshot(), before);
    click("theme-editor-save-apply", &mut window);
    for document in custom_theme_documents() {
        if document.id().is_some_and(|id| !ids.iter().any(|old| old == id)) {
            cleanup.track(&document);
        }
    }
    assert_eq!(RuntimeSettings::load().background_image.as_deref(), background.to_str());
    assert_ne!(settings_file_snapshot(), before);
    // This directory was created by this fixture's confirmed installation.
    let resource_dir = background.parent().unwrap().parent().unwrap();
    assert_eq!(
        resource_dir.parent().unwrap(),
        ThemeLibraryStore::default().root().join("packages").canonicalize().unwrap()
    );
    std::fs::remove_dir_all(resource_dir).unwrap();
}

#[gpui::test]
fn changed_zip_and_invalid_zip_show_inline_errors_without_installing(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let dir = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), "Initially reviewed package");
    let (pane, mut window) = open_settings(cx);
    let before = settings_file_snapshot();
    let ids = custom_theme_ids();
    open_theme_editor(&mut window);
    open_import(&mut window);
    choose(&path, &mut window);
    fixture(dir.path(), "Changed after review");
    click("theme-package-confirm", &mut window);
    assert!(window.debug_bounds("theme-package-error").is_some());
    assert!(pane.read_with(&mut window, |pane, _| {
        pane.theme_package.as_ref().unwrap().installed.is_none()
    }));
    assert_eq!(custom_theme_ids(), ids);
    std::fs::write(&path, b"invalid ZIP bytes").unwrap();
    click("theme-package-source-choose", &mut window);
    choose(&path, &mut window);
    assert!(window.debug_bounds("theme-package-error").is_some());
    assert!(pane.read_with(&mut window, |pane, _| {
        pane.theme_package.as_ref().unwrap().inspected.is_none()
    }));
    assert_eq!(custom_theme_ids(), ids);
    assert_eq!(settings_file_snapshot(), before);
}

#[gpui::test]
fn cancelled_zip_picker_cannot_modify_a_reopened_package_dialog(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let dir = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), "Late picker package");
    let (pane, mut window) = open_settings(cx);
    let before = settings_file_snapshot();
    open_theme_editor(&mut window);
    open_import(&mut window);
    click("theme-package-cancel", &mut window);
    click("theme-editor-package", &mut window);
    choose(&path, &mut window);
    assert!(pane.read_with(&mut window, |pane, _| {
        let state = pane.theme_package.as_ref().unwrap();
        state.mode == super::super::theme_package::Mode::Export
            && state.inspected.is_none()
            && state.error.is_none()
    }));
    assert_eq!(settings_file_snapshot(), before);
    press("escape", &mut window);
    assert!(pane.read_with(&mut window, |pane, _| pane.theme_package.is_none()));
    assert!(window.debug_bounds("theme-editor-dialog").is_some());
}

#[gpui::test]
fn package_controls_keep_hit_targets_and_keyboard_focus_in_a_narrow_window(
    cx: &mut TestAppContext,
) {
    narrow_package_controls(cx, crate::display::UiLanguage::EnUs);
}

#[gpui::test]
fn package_controls_keep_hit_targets_and_keyboard_focus_in_chinese(cx: &mut TestAppContext) {
    narrow_package_controls(cx, crate::display::UiLanguage::ZhCn);
}

fn narrow_package_controls(cx: &mut TestAppContext, language: crate::display::UiLanguage) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    window.update(|window, cx| {
        cx.global_mut::<crate::gpui_shell::config::Settings>().ui_language = language;
        window.refresh();
    });
    draw(&mut window);
    open_theme_editor(&mut window);
    window.simulate_resize(size(px(600.0), px(850.0)));
    draw(&mut window);
    let editor_bounds = window.debug_bounds("theme-editor-dialog").unwrap();
    for selector in
        ["theme-editor-import", "theme-editor-export", "theme-editor-package", "theme-editor-close"]
    {
        let hit = window.debug_bounds(selector).unwrap();
        assert!(hit.size.height >= px(32.0), "{language:?}: {selector} hit target: {hit:?}");
        assert!(
            hit.left() >= editor_bounds.left()
                && hit.right() <= editor_bounds.right()
                && hit.top() >= editor_bounds.top()
                && hit.bottom() <= editor_bounds.bottom(),
            "{language:?}: {selector} must stay inside the editor: hit={hit:?}, editor={editor_bounds:?}"
        );
    }
    click("theme-editor-package", &mut window);
    let dialog = window
        .debug_bounds("theme-package-dialog")
        .expect("ZIP dialog opens from its visible header control");
    for selector in [
        "theme-package-confirm",
        "theme-package-cancel",
        "theme-package-preview-choose",
        "theme-package-export-mode",
        "theme-package-import-mode",
    ] {
        let hit = window.debug_bounds(selector).unwrap();
        assert!(hit.size.height >= px(32.0));
        assert!(hit.left() >= dialog.left() && hit.right() <= dialog.right());
    }
    for _ in 0..12 {
        press("tab", &mut window);
        assert!(window.update(|window, cx| {
            pane.read(cx).theme_package.as_ref().unwrap().focus.contains_focused(window, cx)
        }));
    }
    press("escape", &mut window);
    assert!(pane.read_with(&mut window, |pane, _| pane.theme_package.is_none()));
    assert!(window.debug_bounds("theme-editor-dialog").is_some());
}
