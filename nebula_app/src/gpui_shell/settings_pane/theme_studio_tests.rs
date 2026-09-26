//! Rendered interaction coverage for the native theme studio.
//!
//! These tests deliberately enter through the settings page and use hitboxes
//! plus key events. The draft is compared with the persisted runtime and the
//! global palette so a test cannot pass by only exercising a state callback.

use super::*;
use crate::theme_library::{ThemeDocument, ThemeFormat, ThemeLibraryStore};
use gpui::{Modifiers, TestAppContext, VisualTestContext, size};
use gpui_component::Root;
use nebula_settings::{RawSettings, RuntimeSettings, ThemeDefinition, ThemeName};

const TEST_SETTINGS: &str =
    "theme=Nord\nfollow_system_theme=0\napp_icon=graphite-violet\nfont_size=15\n";

// These rendered fixtures share the real settings path and theme library.
// Readers must hold the same guard as Save/Apply tests so their before/after
// snapshots cannot observe another fixture's writes or restoration cleanup.
// Only this fixture group is serialized; the rest of the native suite stays parallel.
fn lock_theme_studio() -> std::sync::MutexGuard<'static, ()> {
    static FIXTURES: std::sync::Mutex<()> = std::sync::Mutex::new(());
    FIXTURES.lock().unwrap_or_else(|error| error.into_inner())
}

#[derive(Clone, Debug, PartialEq)]
struct RuntimeSnapshot {
    theme: ThemeName,
    follow_system_theme: bool,
    theme_foreground: Option<[u8; 3]>,
    custom_theme: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct PaletteSnapshot(Vec<u32>);

fn test_runtime() -> RuntimeSettings {
    RuntimeSettings::from_raw(&RawSettings::from_text(TEST_SETTINGS))
}

#[gpui::test]
fn ligature_menu_defaults_on_and_persists_keyboard_choices(cx: &mut TestAppContext) {
    use crate::gpui_shell::config::Settings;
    use nebula_settings::Ligatures;

    let _fixture_guard = lock_theme_studio();
    let _settings_guard = SettingsBytesGuard::capture();
    std::fs::create_dir_all(nebula_settings::settings_dir()).unwrap();
    std::fs::write(nebula_settings::settings_path(), TEST_SETTINGS).unwrap();
    let (pane, mut window) = open_settings(cx);
    pane.update(&mut window, |pane, cx| {
        pane.active_section = 1;
        cx.notify();
    });
    window.simulate_resize(size(px(1280.0), px(1800.0)));
    draw(&mut window);
    assert_eq!(RuntimeSettings::load().ligatures, Ligatures::On);
    assert!(window.read(|cx| cx.global::<Settings>().ligatures));
    for (preference, enabled) in [(Ligatures::Off, false), (Ligatures::Theme, true)] {
        click("settings-select-ligatures", &mut window);
        press("down", &mut window);
        press("enter", &mut window);
        assert_eq!(RuntimeSettings::load().ligatures, preference);
        assert_eq!(window.read(|cx| cx.global::<Settings>().ligatures), enabled);
        let reopened = window.update(|window, cx| cx.new(|cx| SettingsPane::new(window, cx)));
        reopened.read_with(&mut window, |pane, cx| {
            assert_eq!(pane.runtime.ligatures, preference);
            assert_eq!(
                pane.select_of("ligatures").unwrap().read(cx).selected_index(cx).unwrap().row,
                if preference == Ligatures::Off { 1 } else { 2 }
            );
        });
    }
    click("settings-select-ligatures", &mut window);
    press("up", &mut window);
    press("escape", &mut window);
    assert_eq!(RuntimeSettings::load().ligatures, Ligatures::Theme, "Escape cancels the choice");
    pane.read_with(&mut window, |pane, _| {
        assert_eq!(pane.setting_override("ligatures"), Some((true, "on".into())));
    });
}

#[gpui::test]
fn scrolling_controls_persist_dropdown_and_slider_without_changing_existing_history(
    cx: &mut TestAppContext,
) {
    use crate::gpui_shell::config::Settings;
    use nebula_terminal::event::VoidListener;
    use nebula_terminal::grid::Dimensions;
    use nebula_terminal::term::{Term, test::TermSize};

    let _fixture_guard = lock_theme_studio();
    let _settings_guard = SettingsBytesGuard::capture();
    std::fs::create_dir_all(nebula_settings::settings_dir()).unwrap();
    std::fs::write(nebula_settings::settings_path(), TEST_SETTINGS).unwrap();
    let (pane, mut window) = open_settings(cx);
    pane.update(&mut window, |pane, cx| {
        pane.active_section = 1;
        cx.notify();
    });
    window.simulate_resize(size(px(1280.0), px(1800.0)));
    draw(&mut window);
    let old_config = window.read(|cx| cx.global::<Settings>().term_config());
    let mut existing = Term::new(old_config, &TermSize::new(80, 24), VoidListener);
    existing.grid_mut().initialize_all();
    assert_eq!(existing.grid().history_size(), 10_000);

    assert_eq!(
        localized_select_labels(
            "scrollback_lines",
            nebula_settings::SCROLLBACK_VALUES,
            crate::display::UiLanguage::ZhCn
        ),
        ["1,000", "2,000", "5,000", "10,000", "20,000", "50,000", "100,000"]
            .map(SharedString::from)
    );
    click("settings-select-scrollback_lines", &mut window);
    for _ in 0..3 {
        press("down", &mut window);
    }
    press("enter", &mut window);
    assert_eq!(RuntimeSettings::load().scrollback_lines, 100_000);
    assert_eq!(window.read(|cx| cx.global::<Settings>().term_config().scrolling_history), 100_000);
    assert_eq!(existing.grid().history_size(), 10_000);

    let bounds = window.debug_bounds("scroll-speed-control").expect("scroll speed hitbox");
    let point = gpui::point(bounds.origin.x + px(80.0), bounds.center().y);
    window.simulate_mouse_down(point, MouseButton::Left, Modifiers::default());
    draw(&mut window);
    let preview = pane.read_with(&mut window, |pane, _| pane.runtime.scroll_speed);
    assert_ne!(preview, 1.0);
    assert_eq!(window.read(|cx| cx.global::<Settings>().scroll_speed), preview);
    assert_eq!(
        RuntimeSettings::load().scroll_speed,
        1.0,
        "dragging does not write settings on every frame"
    );
    window.simulate_mouse_up(point, MouseButton::Left, Modifiers::default());
    draw(&mut window);
    assert_eq!(RuntimeSettings::load().scroll_speed, preview);
    press("home", &mut window);
    assert_eq!(RuntimeSettings::load().scroll_speed, 0.25);
    press("right", &mut window);
    assert_eq!(RuntimeSettings::load().scroll_speed, 0.5);

    let reopened = window.update(|window, cx| cx.new(|cx| SettingsPane::new(window, cx)));
    reopened.read_with(&mut window, |pane, cx| {
        assert_eq!(pane.runtime.scrollback_lines, 100_000);
        assert_eq!(pane.scroll_speed_slider.read(cx).value().start(), 0.5);
        assert_eq!(
            pane.select_of("scrollback_lines").unwrap().read(cx).selected_value().unwrap().as_ref(),
            "100,000"
        );
    });
    pane.update_in(&mut window, |pane, window, cx| {
        pane.commit_scrollback_lines("1000", window, cx)
    });
    assert_eq!(RuntimeSettings::load().scrollback_lines, 1_000);
    assert_eq!(existing.grid().history_size(), 10_000);
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("missing hitbox: {selector}"));
    click_point(bounds.center(), cx);
}

fn click_point(point: gpui::Point<gpui::Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_down(point, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(point, MouseButton::Left, Modifiers::default());
    draw(cx);
}

fn click_first_theme_option(cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds("appearance-theme-grid").expect("theme option grid hitbox");
    click_point(gpui::point(bounds.origin.x + px(8.0), bounds.origin.y + px(8.0)), cx);
}

fn press(key: &str, cx: &mut VisualTestContext) {
    let keystroke = gpui::Keystroke::parse(key).expect("test keystroke");
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(gpui::KeyUpEvent { keystroke });
    draw(cx);
}

fn edit_input(selector: &'static str, value: &str, cx: &mut VisualTestContext) {
    // The selector belongs to the field wrapper so the label remains part of
    // the hitbox contract. Aim at the trailing input area instead of the
    // wrapper center, which can land on the label in a narrow two-column row.
    let bounds = cx.debug_bounds(selector).expect("visible input field");
    click_point(
        gpui::point(bounds.origin.x + bounds.size.width * 0.78, bounds.bottom() - px(16.0)),
        cx,
    );
    let select_all = match crate::platform::Platform::current() {
        crate::platform::Platform::MacOS => "cmd-a",
        _ => "ctrl-a",
    };
    cx.simulate_keystrokes(select_all);
    draw(cx);
    cx.simulate_input(value);
    draw(cx);
}

struct ThemeStudioHost {
    pane: Entity<SettingsPane>,
}

impl Render for ThemeStudioHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.pane.clone())
            // `SettingsPane` owns the dialog requests, while the test host
            // owns the component library's global modal layer just like the
            // production workspace does. Keep it above the feature overlays
            // (theme editor=0, component popovers=1, transfer=6) so real dialog buttons receive
            // pointer events in the same order as the application shell.
            .child(
                deferred(div().size_full().children(Root::render_dialog_layer(window, cx)))
                    .with_priority(10),
            )
    }
}

fn open_settings(cx: &mut TestAppContext) -> (Entity<SettingsPane>, VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_reduce_motion(true);
        let runtime = test_runtime();
        cx.set_global(crate::gpui_shell::config::Settings::load_with_runtime(
            ThemeName::Nord,
            runtime,
        ));
    });
    let mut pane = None;
    let (_, mut window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| SettingsPane::new(window, cx));
        view.update(cx, |pane, _| {
            pane.runtime = test_runtime();
        });
        pane = Some(view.clone());
        let host = cx.new(|_| ThemeStudioHost { pane: view });
        Root::new(host, window, cx)
    });
    window.simulate_resize(size(px(1280.0), px(1000.0)));
    window.update(|window, _| window.activate_window());
    draw(&mut window);
    (pane.expect("settings pane entity"), (*window).clone())
}

fn open_theme_editor(cx: &mut VisualTestContext) {
    click("open-theme-picker", cx);
    assert!(cx.debug_bounds("appearance-picker-dialog").is_some());
    click("customize-theme", cx);
    assert!(cx.debug_bounds("theme-editor-dialog").is_some());
}

fn runtime_snapshot() -> RuntimeSnapshot {
    let runtime = RuntimeSettings::load();
    RuntimeSnapshot {
        theme: runtime.theme,
        follow_system_theme: runtime.follow_system_theme,
        theme_foreground: runtime.theme_foreground,
        custom_theme: runtime.custom_theme,
    }
}

fn settings_file_snapshot() -> Option<Vec<u8>> {
    std::fs::read(nebula_settings::settings_path()).ok()
}

fn custom_theme_ids() -> Vec<String> {
    custom_theme_documents()
        .into_iter()
        .filter_map(|document| document.id().map(str::to_owned))
        .collect()
}

fn custom_theme_documents() -> Vec<ThemeDocument> {
    ThemeLibraryStore::default().list().expect("list native theme library").custom
}

struct SettingsBytesGuard {
    path: std::path::PathBuf,
    bytes: Option<Vec<u8>>,
}

impl SettingsBytesGuard {
    fn capture() -> Self {
        let path = nebula_settings::settings_path();
        Self { bytes: std::fs::read(&path).ok(), path }
    }
}

impl Drop for SettingsBytesGuard {
    fn drop(&mut self) {
        match &self.bytes {
            Some(bytes) => {
                let _ = std::fs::write(&self.path, bytes);
            },
            None => {
                let _ = std::fs::remove_file(&self.path);
            },
        }
    }
}

#[derive(Default)]
struct CustomThemeCleanup {
    ids: Vec<String>,
}

impl CustomThemeCleanup {
    fn track(&mut self, document: &ThemeDocument) {
        if let Some(id) = document.id() {
            self.ids.push(id.to_owned());
        }
    }
}

impl Drop for CustomThemeCleanup {
    fn drop(&mut self) {
        let store = ThemeLibraryStore::default();
        let Ok(snapshot) = store.list() else { return };
        for document in snapshot.custom {
            let Some(id) = document.id() else { continue };
            if self.ids.iter().any(|tracked| tracked == id) {
                let _ = store.delete(id, document.revision());
            }
        }
    }
}

fn remove_new_custom_themes(before_ids: &[String], name_prefix: &str) -> Vec<ThemeDocument> {
    let store = ThemeLibraryStore::default();
    let new_documents: Vec<_> = custom_theme_documents()
        .into_iter()
        .filter(|document| {
            document.id().is_some_and(|id| !before_ids.iter().any(|before| before == id))
                && document.name().starts_with(name_prefix)
        })
        .collect();
    for document in &new_documents {
        let id = document.id().expect("new custom theme id");
        store.delete(id, document.revision()).expect("remove test custom theme");
    }
    new_documents
}

fn transfer_temp_path(stem: &str) -> std::path::PathBuf {
    std::env::temp_dir()
        .join(format!("nebula-theme-transfer-{stem}-{}.pebrel-theme.json", std::process::id()))
}

fn transfer_fixture_document(stem: &str) -> ThemeDocument {
    let mut value = crate::theme_library::builtin_document(ThemeName::Nord)
        .expect("builtin transfer fixture")
        .to_value();
    value["name"] = serde_json::Value::String(format!(
        "Imported transfer fixture {stem} {}",
        std::process::id()
    ));
    value["id"] = serde_json::Value::String(format!("external-{stem}"));
    value["revision"] = serde_json::json!(17);
    value["builtin"] = serde_json::json!(false);
    value["terminal"]["background"] = serde_json::Value::String("#112233".to_owned());
    value["terminal"]["foreground"] = serde_json::Value::String("#ddeeff".to_owned());
    value["vendor"] = serde_json::json!({"future": true});
    value["terminal"]["vendor_color"] = serde_json::Value::String("#102030".to_owned());

    ThemeDocument::from_value(value).expect("valid transfer fixture")
}

fn write_transfer_fixture(stem: &str) -> (std::path::PathBuf, ThemeDocument) {
    let document = transfer_fixture_document(stem);
    let path = transfer_temp_path(stem);
    std::fs::write(&path, document.to_json_bytes().expect("serialize transfer fixture"))
        .expect("write transfer fixture");
    (path, document)
}

fn editor_document(pane: &Entity<SettingsPane>, cx: &mut VisualTestContext) -> ThemeDocument {
    pane.read_with(cx, |pane, _| {
        pane.document_for_draft().expect("theme editor document for draft")
    })
}

fn respond_with_import_path(path: &std::path::Path, cx: &mut VisualTestContext) {
    assert!(cx.did_prompt_for_paths(), "import path prompt is pending");
    let path = path.to_owned();
    cx.simulate_path_prompt_response(move |options| {
        assert!(options.files);
        assert!(options.multiple);
        Some(vec![path])
    });
    draw(cx);
}

fn rgba_snapshot(color: gpui::Rgba, out: &mut Vec<u32>) {
    out.extend([color.r.to_bits(), color.g.to_bits(), color.b.to_bits(), color.a.to_bits()]);
}

fn palette_values(palette: &crate::gpui_shell::terminal::colors::Palette) -> Vec<u32> {
    let mut values = Vec::new();
    rgba_snapshot(palette.foreground, &mut values);
    rgba_snapshot(palette.background, &mut values);
    rgba_snapshot(palette.bright_foreground, &mut values);
    rgba_snapshot(palette.dim_foreground, &mut values);
    rgba_snapshot(palette.cursor, &mut values);
    if let Some(cursor_text) = palette.cursor_text {
        rgba_snapshot(cursor_text, &mut values);
    }
    if let Some(cursor_stroke) = palette.cursor_stroke {
        rgba_snapshot(cursor_stroke, &mut values);
    }
    rgba_snapshot(palette.selection, &mut values);
    if let Some(selection_foreground) = palette.selection_foreground {
        rgba_snapshot(selection_foreground, &mut values);
    }
    for color in palette.ansi {
        rgba_snapshot(color, &mut values);
    }
    for color in palette.dim {
        rgba_snapshot(color, &mut values);
    }
    for (index, color) in &palette.indexed {
        values.push(u32::from(*index));
        rgba_snapshot(*color, &mut values);
    }
    values
}

fn palette_snapshot(cx: &mut VisualTestContext) -> PaletteSnapshot {
    PaletteSnapshot(cx.update(|_, cx| {
        palette_values(&cx.global::<crate::gpui_shell::config::Settings>().palette)
    }))
}

fn palette_foreground(cx: &mut VisualTestContext) -> [u8; 3] {
    cx.update(|_, cx| {
        let foreground = cx.global::<crate::gpui_shell::config::Settings>().palette.foreground;
        [
            (foreground.r * 255.0).round() as u8,
            (foreground.g * 255.0).round() as u8,
            (foreground.b * 255.0).round() as u8,
        ]
    })
}

fn reloaded_palette_snapshot(
    cx: &mut VisualTestContext,
    runtime: &RuntimeSettings,
) -> PaletteSnapshot {
    PaletteSnapshot(cx.update(|_, cx| {
        let theme = crate::gpui_shell::theme::resolve_theme_name(
            runtime.theme,
            runtime.follow_system_theme,
            crate::gpui_shell::theme::system_is_light(cx),
        );
        let settings =
            crate::gpui_shell::config::Settings::load_with_runtime(theme, runtime.clone());
        palette_values(&settings.palette)
    }))
}

fn editor_draft(pane: &Entity<SettingsPane>, cx: &mut VisualTestContext) -> ThemeDefinition {
    pane.read_with(cx, |pane, _| {
        pane.theme_editor.as_ref().expect("theme editor state").draft.clone()
    })
}

fn editor_template(pane: &Entity<SettingsPane>, cx: &mut VisualTestContext) -> ThemeName {
    pane.read_with(cx, |pane, _| pane.theme_editor.as_ref().expect("theme editor state").template)
}

#[gpui::test]
fn theme_editor_opens_from_the_picker_with_common_fields_and_collapsed_advanced(
    cx: &mut TestAppContext,
) {
    let _fixture_guard = lock_theme_studio();
    let (_pane, mut window) = open_settings(cx);
    open_theme_editor(&mut window);

    for selector in [
        "theme-editor-template",
        "theme-editor-name",
        "theme-editor-background",
        "theme-editor-foreground",
        "theme-editor-accent",
        "theme-editor-cursor",
        "theme-editor-font",
        "theme-editor-font-size",
        "theme-editor-line-height",
        "theme-editor-opacity",
    ] {
        assert!(window.debug_bounds(selector).is_some(), "common control is visible: {selector}");
    }
    assert!(window.debug_bounds("theme-editor-advanced-toggle").is_some());
    assert!(window.debug_bounds("theme-editor-ansi-0").is_none());

    click("theme-editor-advanced-toggle", &mut window);
    assert!(window.debug_bounds("theme-editor-ansi-0").is_some());
    click("theme-editor-advanced-toggle", &mut window);
    assert!(window.debug_bounds("theme-editor-ansi-0").is_none());
}

#[gpui::test]
fn theme_editor_input_is_a_local_draft_until_apply_and_preserves_builtin_source(
    cx: &mut TestAppContext,
) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    let before_runtime = runtime_snapshot();
    let before_palette = palette_snapshot(&mut window);
    let before_settings_file = settings_file_snapshot();
    open_theme_editor(&mut window);

    let source_template = editor_template(&pane, &mut window);
    let source_before = ThemeDefinition::from_builtin(source_template);
    let draft_before = editor_draft(&pane, &mut window);
    edit_input("theme-editor-foreground", "#ff4f70", &mut window);

    let draft_after = editor_draft(&pane, &mut window);
    assert_eq!(draft_after.terminal.foreground, [255, 79, 112]);
    assert_ne!(draft_after, draft_before);
    assert_eq!(ThemeDefinition::from_builtin(source_template), source_before);
    assert_eq!(runtime_snapshot(), before_runtime);
    assert_eq!(palette_snapshot(&mut window), before_palette);
    assert_eq!(settings_file_snapshot(), before_settings_file);
}

#[gpui::test]
fn theme_editor_template_keyboard_selection_replaces_only_the_draft(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    let before_runtime = runtime_snapshot();
    open_theme_editor(&mut window);
    let old_template = editor_template(&pane, &mut window);
    let old_source = ThemeDefinition::from_builtin(old_template);

    click("theme-editor-template", &mut window);
    press("down", &mut window);
    press("enter", &mut window);

    let new_template = editor_template(&pane, &mut window);
    assert_ne!(new_template, old_template);
    assert_eq!(ThemeDefinition::from_builtin(old_template), old_source);
    assert_eq!(runtime_snapshot(), before_runtime);
    assert!(window.debug_bounds("theme-editor-dialog").is_some());
}

#[gpui::test]
fn theme_editor_dirty_back_and_escape_show_confirmation_and_keep_or_discard_draft(
    cx: &mut TestAppContext,
) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    let before_runtime = runtime_snapshot();
    let before_palette = palette_snapshot(&mut window);
    open_theme_editor(&mut window);
    edit_input("theme-editor-name", "Local draft", &mut window);
    let changed = editor_draft(&pane, &mut window);

    click("theme-editor-back", &mut window);
    assert!(window.debug_bounds("confirm-dialog-cancel").is_some());
    click("confirm-dialog-cancel", &mut window);
    assert!(window.debug_bounds("theme-editor-dialog").is_some());
    assert_eq!(editor_draft(&pane, &mut window), changed);

    press("escape", &mut window);
    assert!(window.debug_bounds("confirm-dialog-ok").is_some());
    click("confirm-dialog-ok", &mut window);
    assert!(window.debug_bounds("theme-editor-dialog").is_none());
    assert_eq!(runtime_snapshot(), before_runtime);
    assert_eq!(palette_snapshot(&mut window), before_palette);
}

#[gpui::test]
fn theme_editor_back_returns_to_picker_and_preserves_its_draft(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    click("open-theme-picker", &mut window);
    click_first_theme_option(&mut window);
    click("theme-foreground-swatch-1", &mut window);
    let picker_before = pane.read_with(&mut window, |pane, _| {
        let picker = pane.appearance_picker.as_ref().expect("theme picker state");
        (picker.draft, picker.foreground_override)
    });

    click("customize-theme", &mut window);
    assert!(window.debug_bounds("theme-editor-dialog").is_some());
    edit_input("theme-editor-name", "Back navigation draft", &mut window);
    click("theme-editor-back", &mut window);
    assert!(window.debug_bounds("confirm-dialog-ok").is_some());
    click("confirm-dialog-ok", &mut window);

    assert!(window.debug_bounds("theme-editor-dialog").is_none());
    assert!(window.debug_bounds("appearance-picker-dialog").is_some());
    let picker_after = pane.read_with(&mut window, |pane, _| {
        let picker = pane.appearance_picker.as_ref().expect("restored theme picker state");
        (picker.draft, picker.foreground_override)
    });
    assert_eq!(picker_after, picker_before);
    click("cancel-appearance-picker", &mut window);
}

#[gpui::test]
fn theme_editor_save_only_persists_a_copy_without_changing_source_selection(
    cx: &mut TestAppContext,
) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    let before_runtime = runtime_snapshot();
    let before_palette = palette_snapshot(&mut window);
    let before_settings_file = settings_file_snapshot();
    let before_library_ids = custom_theme_ids();
    let saved_name = format!("Saved copy native test {}", std::process::id());
    open_theme_editor(&mut window);
    let source_template = editor_template(&pane, &mut window);
    let source_before = ThemeDefinition::from_builtin(source_template);
    edit_input("theme-editor-name", &saved_name, &mut window);
    click("theme-editor-save", &mut window);

    let after_save = runtime_snapshot();
    assert_eq!(after_save, before_runtime, "Save only must not publish active theme preferences");
    assert_eq!(palette_snapshot(&mut window), before_palette);
    assert_eq!(settings_file_snapshot(), before_settings_file);
    assert_eq!(ThemeDefinition::from_builtin(source_template), source_before);
    assert!(window.debug_bounds("theme-editor-dialog").is_some());

    let store = ThemeLibraryStore::default();
    let saved = store
        .list()
        .expect("list saved native theme")
        .custom
        .into_iter()
        .find(|document| {
            document.name().starts_with(saved_name.as_str())
                && document
                    .id()
                    .is_some_and(|id| !before_library_ids.iter().any(|before| before == id))
        })
        .expect("Save only writes an independent custom theme document");
    assert!(!saved.is_builtin());
    let id = saved.id().expect("saved custom theme id").to_owned();
    let revision = saved.revision();
    click("theme-editor-back", &mut window);
    assert!(window.debug_bounds("theme-editor-dialog").is_none());
    assert!(window.debug_bounds("appearance-picker-dialog").is_some());
    assert!(pane.read_with(&mut window, |pane, _| {
        pane.appearance_picker.as_ref().is_some_and(|picker| {
            picker.custom_themes.iter().any(|document| document.id() == Some(id.as_str()))
        })
    }));
    click("cancel-appearance-picker", &mut window);
    store.delete(&id, revision).expect("remove Save only test document");
}

#[gpui::test]
fn theme_editor_save_and_apply_persists_copy_and_reloads_its_foreground(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let _settings_guard = SettingsBytesGuard::capture();
    std::fs::write(nebula_settings::settings_path(), TEST_SETTINGS)
        .expect("write isolated apply settings");
    let mut theme_cleanup = CustomThemeCleanup::default();
    let (pane, mut window) = open_settings(cx);
    let before_runtime = runtime_snapshot();
    let before_palette = palette_snapshot(&mut window);
    let before_settings_file = settings_file_snapshot();
    let before_library_ids = custom_theme_ids();
    open_theme_editor(&mut window);
    let source_template = editor_template(&pane, &mut window);
    let source_before = ThemeDefinition::from_builtin(source_template);
    let saved_name = format!("Save and apply native test {}", std::process::id());
    let saved_foreground = [0xd8, 0x6b, 0x91];

    edit_input("theme-editor-name", &saved_name, &mut window);
    edit_input("theme-editor-foreground", "#d86b91", &mut window);
    let draft = editor_draft(&pane, &mut window);
    assert_eq!(draft.name, saved_name);
    assert_eq!(draft.terminal.foreground, saved_foreground);
    assert_eq!(runtime_snapshot(), before_runtime);
    assert_eq!(palette_snapshot(&mut window), before_palette);
    assert_eq!(settings_file_snapshot(), before_settings_file);

    click("theme-editor-save-apply", &mut window);
    assert!(window.debug_bounds("theme-editor-dialog").is_none());
    assert_eq!(ThemeDefinition::from_builtin(source_template), source_before);

    let after_documents = custom_theme_documents();
    for document in &after_documents {
        if document.id().is_some_and(|id| !before_library_ids.iter().any(|before| before == id))
            && document.name().starts_with(&saved_name)
        {
            theme_cleanup.track(document);
        }
    }
    let after_runtime = runtime_snapshot();
    let applied_id =
        after_runtime.custom_theme.as_deref().expect("Save and apply persists the custom theme id");
    assert!(!before_library_ids.iter().any(|id| id == applied_id));
    let applied = after_documents
        .iter()
        .find(|document| document.id() == Some(applied_id))
        .expect("applied custom theme remains in the library");
    assert!(applied.name().starts_with(&saved_name));
    assert_eq!(applied.color("terminal", "foreground"), Some(saved_foreground));
    assert_eq!(applied.id(), Some(applied_id));
    assert_ne!(after_runtime, before_runtime);
    assert_eq!(palette_foreground(&mut window), saved_foreground);
    let after_palette = palette_snapshot(&mut window);
    assert_ne!(after_palette, before_palette);
    assert_ne!(settings_file_snapshot(), before_settings_file);

    let reloaded_runtime = RuntimeSettings::load();
    assert_eq!(reloaded_runtime.custom_theme.as_deref(), Some(applied_id));
    assert_eq!(reloaded_palette_snapshot(&mut window, &reloaded_runtime), after_palette);
    assert_eq!(palette_foreground(&mut window), applied.color("terminal", "foreground").unwrap());
}

#[gpui::test]
fn invalid_color_stays_dirty_and_save_rejects_after_a_later_valid_edit(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    let before_runtime = runtime_snapshot();
    let before_palette = palette_snapshot(&mut window);
    let before_settings_file = settings_file_snapshot();
    let before_library_ids = custom_theme_ids();
    let saved_name = format!("Invalid color native test {}", std::process::id());
    open_theme_editor(&mut window);
    let draft_before = editor_draft(&pane, &mut window);

    edit_input("theme-editor-background", "not-a-color", &mut window);
    assert_eq!(
        editor_draft(&pane, &mut window).terminal.background,
        draft_before.terminal.background
    );

    // Invalid text is still an unsaved editor action. Back must not silently
    // close the editor while the invalid value is waiting for correction.
    click("theme-editor-back", &mut window);
    assert!(window.debug_bounds("confirm-dialog-ok").is_some());
    click("confirm-dialog-cancel", &mut window);
    assert!(window.debug_bounds("theme-editor-dialog").is_some());

    // A later valid edit must not clear the invalid field's save error.
    edit_input("theme-editor-name", &saved_name, &mut window);
    click("theme-editor-save", &mut window);
    assert!(window.debug_bounds("theme-editor-dialog").is_some());
    assert_eq!(runtime_snapshot(), before_runtime);
    assert_eq!(palette_snapshot(&mut window), before_palette);
    assert_eq!(settings_file_snapshot(), before_settings_file);
    assert!(remove_new_custom_themes(&before_library_ids, saved_name.as_str()).is_empty());
}

#[gpui::test]
fn non_finite_font_values_stay_out_of_the_draft_and_save_is_rejected(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    let before_runtime = runtime_snapshot();
    let before_palette = palette_snapshot(&mut window);
    let before_settings_file = settings_file_snapshot();
    let before_library_ids = custom_theme_ids();
    let saved_name = format!("Non-finite native test {}", std::process::id());
    open_theme_editor(&mut window);
    let draft_before = editor_draft(&pane, &mut window);

    edit_input("theme-editor-font-size", "NaN", &mut window);
    edit_input("theme-editor-line-height", "Inf", &mut window);
    edit_input("theme-editor-name", &saved_name, &mut window);
    let draft_after = editor_draft(&pane, &mut window);
    assert_eq!(draft_after.typography.font_size, draft_before.typography.font_size);
    assert_eq!(draft_after.typography.line_height, draft_before.typography.line_height);
    assert!(draft_after.typography.font_size.is_none_or(|value| value.is_finite()));
    assert!(draft_after.typography.line_height.is_none_or(|value| value.is_finite()));

    click("theme-editor-save", &mut window);
    assert!(window.debug_bounds("theme-editor-dialog").is_some());
    assert_eq!(runtime_snapshot(), before_runtime);
    assert_eq!(palette_snapshot(&mut window), before_palette);
    assert_eq!(settings_file_snapshot(), before_settings_file);
    assert!(remove_new_custom_themes(&before_library_ids, saved_name.as_str()).is_empty());
}

#[gpui::test]
fn theme_editor_font_select_changes_only_the_draft(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    pane.update(&mut window, |pane, _| {
        // Keep the real selector independent of the fonts installed on the host.
        pane.font_system = Some(Vec::new());
        pane.font_imported = vec!["Theme studio alternate family".to_owned()];
    });
    let before_runtime = runtime_snapshot();
    let before_palette = palette_snapshot(&mut window);
    let before_settings_file = settings_file_snapshot();
    open_theme_editor(&mut window);
    let (before_draft, selected_index, option_count) = pane.read_with(&mut window, |pane, cx| {
        let editor = pane.theme_editor.as_ref().expect("theme editor state");
        (
            editor.draft.clone(),
            editor.font_select.read(cx).selected_index(cx).map(|path| path.row),
            editor.font_options.len(),
        )
    });
    assert!(option_count > 1, "theme editor exposes more than one font option");
    assert!(window.debug_bounds("theme-editor-font").is_some());

    click("theme-editor-font", &mut window);
    if selected_index.is_some_and(|index| index + 1 < option_count) {
        press("down", &mut window);
    } else {
        press("up", &mut window);
    }
    press("enter", &mut window);

    let after_draft = editor_draft(&pane, &mut window);
    assert_ne!(
        after_draft.typography.font_family, before_draft.typography.font_family,
        "choosing a real Select option must update only the editor draft"
    );
    // The menu must receive mouse events above the editor, not merely accept
    // keyboard selection while its popup is hidden behind the modal surface.
    click("theme-editor-font", &mut window);
    let font_bounds = window.debug_bounds("theme-editor-font").unwrap();
    click_point(
        gpui::point(font_bounds.left() + px(24.0), font_bounds.bottom() + px(18.0)),
        &mut window,
    );
    assert_ne!(
        editor_draft(&pane, &mut window).typography.font_family,
        after_draft.typography.font_family,
        "the first visible font menu item must accept a real pointer click"
    );
    assert_eq!(runtime_snapshot(), before_runtime);
    assert_eq!(palette_snapshot(&mut window), before_palette);
    assert_eq!(settings_file_snapshot(), before_settings_file);

    click("theme-editor-cancel", &mut window);
    assert!(window.debug_bounds("confirm-dialog-ok").is_some());
    click("confirm-dialog-ok", &mut window);
    assert!(window.debug_bounds("appearance-picker-dialog").is_none());
    assert!(pane.read_with(&mut window, |pane, _| pane.theme_editor.is_none()));
    assert_eq!(settings_file_snapshot(), before_settings_file);
}

#[gpui::test]
fn theme_editor_color_picker_is_draft_only_and_cancel_restores(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    let before_runtime = runtime_snapshot();
    let before_palette = palette_snapshot(&mut window);
    let before_settings_file = settings_file_snapshot();
    open_theme_editor(&mut window);
    pane.update(&mut window, |pane, cx| {
        let editor = pane.theme_editor.as_mut().unwrap();
        editor.draft.terminal.cursor = None;
        // Exercise the inherited cursor used by light themes regardless of
        // the application's currently selected built-in theme.
        cx.notify();
    });
    // Typing a foreground through the rendered field also brings
    // its dependent presentation up to date without publishing preferences.
    edit_input("theme-editor-foreground", "#203040", &mut window);
    let before_draft = editor_draft(&pane, &mut window);
    assert_eq!(before_draft.terminal.cursor, None);

    click("theme-editor-foreground-swatch", &mut window);
    assert!(window.debug_bounds("theme-editor-color-sv").is_some());
    assert!(window.debug_bounds("theme-editor-color-hue").is_some());
    assert!(window.debug_bounds("theme-editor-color-hex").is_some());

    click("theme-editor-color-palette-0", &mut window);
    let changed_draft = editor_draft(&pane, &mut window);
    assert_ne!(changed_draft.terminal.foreground, before_draft.terminal.foreground);
    if before_draft.terminal.cursor.is_none() {
        assert_eq!(changed_draft.terminal.cursor, None, "inherited cursor stays inherited");
        assert_eq!(
            pane.read_with(&mut window, |pane, cx| pane
                .theme_editor
                .as_ref()
                .unwrap()
                .cursor_input
                .read(cx)
                .value()
                .to_string()),
            nebula_settings::format_hex_rgb(changed_draft.terminal.foreground),
            "the displayed cursor HEX follows the preview color"
        );
    }
    assert_eq!(runtime_snapshot(), before_runtime);
    assert_eq!(palette_snapshot(&mut window), before_palette);
    assert_eq!(settings_file_snapshot(), before_settings_file);

    click("confirm-dialog-cancel", &mut window);
    assert!(window.debug_bounds("theme-editor-dialog").is_some());
    assert_eq!(editor_draft(&pane, &mut window), before_draft);
    assert!(pane.read_with(&mut window, |pane, _| {
        pane.theme_editor.as_ref().is_some_and(|editor| editor.color_picker.is_none())
    }));
    assert_eq!(runtime_snapshot(), before_runtime);
    assert_eq!(palette_snapshot(&mut window), before_palette);
    assert_eq!(settings_file_snapshot(), before_settings_file);
}

#[gpui::test]
fn theme_and_icon_filter_pills_select_independently(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    click("open-theme-picker", &mut window);
    let theme_counts = pane.read_with(&mut window, |pane, _| {
        let picker = pane.appearance_picker.as_ref().expect("theme picker state");
        (picker.draft.choices(0).len(), picker.draft.choices(1).len())
    });
    assert!(window.debug_bounds("appearance-filter-0").is_some());
    assert!(window.debug_bounds("appearance-filter-1").is_some());
    assert_ne!(theme_counts.0, theme_counts.1);
    click("appearance-filter-1", &mut window);
    assert_eq!(
        pane.read_with(&mut window, |pane, _| pane.appearance_picker.as_ref().unwrap().filter),
        1
    );
    click("appearance-filter-2", &mut window);
    assert_eq!(
        pane.read_with(&mut window, |pane, _| pane.appearance_picker.as_ref().unwrap().filter),
        2
    );

    click("close-appearance-picker", &mut window);
    click("open-icon-picker", &mut window);
    let icon_counts = pane.read_with(&mut window, |pane, _| {
        let picker = pane.appearance_picker.as_ref().expect("icon picker state");
        (picker.draft.choices(0).len(), picker.draft.choices(1).len())
    });
    assert_ne!(icon_counts.0, icon_counts.1);
    click("appearance-filter-1", &mut window);
    assert_eq!(
        pane.read_with(&mut window, |pane, _| pane.appearance_picker.as_ref().unwrap().filter),
        1
    );
}

#[gpui::test]
fn theme_picker_foreground_swatches_are_local_until_apply(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    let before_runtime = runtime_snapshot();
    let before_palette = palette_snapshot(&mut window);
    click("open-theme-picker", &mut window);

    for index in 0..3 {
        let selector = match index {
            0 => "theme-foreground-swatch-0",
            1 => "theme-foreground-swatch-1",
            _ => "theme-foreground-swatch-2",
        };
        click(selector, &mut window);
        assert!(pane.read_with(&mut window, |pane, _| {
            pane.appearance_picker.as_ref().is_some_and(|picker| picker.draft.is_theme())
        }));
        assert_eq!(runtime_snapshot(), before_runtime);
        assert_eq!(palette_snapshot(&mut window), before_palette);
    }

    click("theme-foreground-custom-swatch", &mut window);
    assert!(window.debug_bounds("theme-foreground-palette-0").is_some());
    click("theme-foreground-palette-0", &mut window);
    let (dialog_open, color) = pane.read_with(&mut window, |pane, _| {
        (
            pane.theme_foreground_picker.dialog_open,
            pane.appearance_picker.as_ref().and_then(|picker| picker.foreground_override),
        )
    });
    assert!(
        window.debug_bounds("confirm-dialog-ok").is_some(),
        "color dialog after swatch: open={dialog_open}, foreground={color:?}"
    );
    assert!(dialog_open, "choosing a palette color must keep the dialog open");
    assert_eq!(color, Some([255, 255, 255]));
    click("confirm-dialog-ok", &mut window);
    assert_eq!(runtime_snapshot(), before_runtime);
    assert_eq!(palette_snapshot(&mut window), before_palette);
}

#[gpui::test]
fn theme_foreground_drag_hex_and_cancel_stay_local_in_a_narrow_window(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    let settings_before = settings_file_snapshot();
    let palette_before = palette_snapshot(&mut window);
    click("open-theme-picker", &mut window);
    window.simulate_resize(size(px(620.0), px(900.0)));
    draw(&mut window);
    let rainbow = window.debug_bounds("theme-foreground-custom-swatch").expect("visible rainbow");
    assert!(rainbow.right() <= px(620.0) && rainbow.bottom() <= px(900.0));
    let preview_before = pane.read_with(&mut window, |pane, _| {
        pane.appearance_picker.as_ref().unwrap().foreground_override
    });
    click("theme-foreground-custom-swatch", &mut window);
    click("theme-foreground-hue", &mut window);
    let bounds = window.debug_bounds("theme-foreground-sv").expect("visible SV panel");
    window.simulate_mouse_down(bounds.center(), MouseButton::Left, Modifiers::default());
    window.simulate_event(gpui::MouseMoveEvent {
        position: gpui::point(
            bounds.origin.x + bounds.size.width * 0.75,
            bounds.origin.y + bounds.size.height * 0.35,
        ),
        pressed_button: Some(MouseButton::Left),
        modifiers: Modifiers::default(),
    });
    window.simulate_mouse_up(bounds.center(), MouseButton::Left, Modifiers::default());
    draw(&mut window);
    let dragged = pane.read_with(&mut window, |pane, _| {
        assert!(pane.theme_foreground_picker.drag.is_none());
        pane.appearance_picker.as_ref().unwrap().foreground_override
    });
    assert_ne!(dragged, preview_before);
    assert_eq!(settings_file_snapshot(), settings_before);
    assert_eq!(palette_snapshot(&mut window), palette_before);
    click("confirm-dialog-cancel", &mut window);
    assert_eq!(
        pane.read_with(&mut window, |pane, _| {
            pane.appearance_picker.as_ref().unwrap().foreground_override
        }),
        preview_before
    );

    click("theme-foreground-custom-swatch", &mut window);
    edit_input("theme-foreground-hex", "#b9d8a7", &mut window);
    let selected = Some([0xb9, 0xd8, 0xa7]);
    assert_eq!(
        pane.read_with(&mut window, |pane, _| {
            pane.appearance_picker.as_ref().unwrap().foreground_override
        }),
        selected
    );
    click("confirm-dialog-ok", &mut window);
    assert_eq!(settings_file_snapshot(), settings_before);
    assert_eq!(palette_snapshot(&mut window), palette_before);
    click("cancel-appearance-picker", &mut window);
    assert!(window.debug_bounds("confirm-dialog-cancel").is_some());
    click("confirm-dialog-cancel", &mut window);
    assert_eq!(
        pane.read_with(&mut window, |pane, _| {
            pane.appearance_picker.as_ref().unwrap().foreground_override
        }),
        selected
    );
    click("cancel-appearance-picker", &mut window);
    click("confirm-dialog-ok", &mut window);
    assert!(pane.read_with(&mut window, |pane, _| pane.appearance_picker.is_none()));
    assert_eq!(settings_file_snapshot(), settings_before);
    assert_eq!(palette_snapshot(&mut window), palette_before);
}

#[gpui::test]
fn theme_picker_foreground_swatches_fit_above_footer_in_a_short_window(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let (pane, mut window) = open_settings(cx);
    let palette_before = palette_snapshot(&mut window);
    window.simulate_resize(size(px(900.0), px(590.0)));
    draw(&mut window);
    click("open-theme-picker", &mut window);

    let footer =
        window.debug_bounds("apply-appearance-picker").expect("appearance picker apply button");
    for selector in [
        "theme-foreground-swatch-0",
        "theme-foreground-swatch-1",
        "theme-foreground-swatch-2",
        "theme-foreground-custom-swatch",
    ] {
        let swatch = window.debug_bounds(selector).expect("foreground swatch");
        assert!(
            swatch.bottom() + px(17.0) <= footer.origin.y,
            "{selector} must clear the picker footer: swatch={swatch:?}, footer={footer:?}"
        );
    }

    let before_picker = pane.read_with(&mut window, |pane, _| {
        pane.appearance_picker.as_ref().map(|picker| picker.foreground_override)
    });
    click("theme-foreground-custom-swatch", &mut window);
    assert!(window.debug_bounds("confirm-dialog-cancel").is_some());
    click("confirm-dialog-cancel", &mut window);
    assert!(window.debug_bounds("appearance-picker-dialog").is_some());
    assert_eq!(
        pane.read_with(&mut window, |pane, _| {
            pane.appearance_picker.as_ref().map(|picker| picker.foreground_override)
        }),
        before_picker
    );
    assert_eq!(palette_snapshot(&mut window), palette_before);
    click("cancel-appearance-picker", &mut window);
}

#[gpui::test]
fn theme_transfer_import_uses_a_rendered_candidate_and_keeps_unknown_fields_and_preferences(
    cx: &mut TestAppContext,
) {
    let _fixture_guard = lock_theme_studio();
    let (import_path, fixture) = write_transfer_fixture("import");
    let (pane, mut window) = open_settings(cx);
    let before_runtime = runtime_snapshot();
    let before_palette = palette_snapshot(&mut window);
    let before_settings_file = settings_file_snapshot();
    let before_library_ids = custom_theme_ids();

    open_theme_editor(&mut window);
    let before_draft = editor_draft(&pane, &mut window);
    click("theme-editor-import", &mut window);
    respond_with_import_path(&import_path, &mut window);

    assert!(window.debug_bounds("theme-transfer-dialog").is_some());
    assert!(window.debug_bounds("theme-transfer-candidate-0").is_some());
    let candidate = pane.read_with(&mut window, |pane, _| {
        pane.selected_theme_import().cloned().expect("rendered import candidate")
    });
    let candidate_value = candidate.document.to_value();
    assert_eq!(candidate.document.name(), fixture.name());
    assert_eq!(candidate_value["vendor"]["future"], serde_json::json!(true));
    assert_eq!(candidate_value["terminal"]["vendor_color"], "#102030");

    // Select and confirm through the actual candidate card and modal action.
    click("theme-transfer-candidate-0", &mut window);
    click("theme-transfer-confirm", &mut window);
    assert!(window.debug_bounds("theme-transfer-dialog").is_none());
    assert!(window.debug_bounds("theme-editor-dialog").is_some());

    let imported_draft = editor_draft(&pane, &mut window);
    assert_ne!(imported_draft, before_draft);
    assert_eq!(imported_draft.name, fixture.name());
    assert_eq!(imported_draft.terminal.background, [0x11, 0x22, 0x33]);
    assert_eq!(imported_draft.terminal.foreground, [0xdd, 0xee, 0xff]);
    let imported_document = editor_document(&pane, &mut window);
    let imported_value = imported_document.to_value();
    assert_eq!(imported_value["vendor"]["future"], serde_json::json!(true));
    assert_eq!(imported_value["terminal"]["vendor_color"], "#102030");

    assert_eq!(runtime_snapshot(), before_runtime);
    assert_eq!(palette_snapshot(&mut window), before_palette);
    assert_eq!(settings_file_snapshot(), before_settings_file);
    assert_eq!(custom_theme_ids(), before_library_ids);
    std::fs::remove_file(import_path).expect("remove import fixture");
}

#[gpui::test]
fn theme_transfer_export_writes_a_valid_native_document_without_publishing_preferences(
    cx: &mut TestAppContext,
) {
    let _fixture_guard = lock_theme_studio();
    let (import_path, fixture) = write_transfer_fixture("export");
    let output_path = transfer_temp_path("export-output");
    let (pane, mut window) = open_settings(cx);
    let before_runtime = runtime_snapshot();
    let before_palette = palette_snapshot(&mut window);
    let before_settings_file = settings_file_snapshot();
    let before_library_ids = custom_theme_ids();

    open_theme_editor(&mut window);
    click("theme-editor-import", &mut window);
    respond_with_import_path(&import_path, &mut window);
    click("theme-transfer-candidate-0", &mut window);
    click("theme-transfer-confirm", &mut window);
    assert_eq!(editor_document(&pane, &mut window).name(), fixture.name());

    click("theme-editor-export", &mut window);
    assert!(window.debug_bounds("theme-transfer-dialog").is_some());
    assert!(window.debug_bounds("theme-transfer-format-Pebrel").is_some());

    // Exercise the rendered format controls and return to the lossless native
    // adapter before choosing the destination.
    click("theme-transfer-format-Kitty", &mut window);
    click("theme-transfer-format-Pebrel", &mut window);
    assert_eq!(
        pane.read_with(&mut window, |pane, _| pane.theme_transfer.export_format),
        ThemeFormat::Pebrel
    );
    click("theme-transfer-confirm", &mut window);
    let output_for_picker = output_path.clone();
    window.simulate_new_path_selection(move |_| Some(output_for_picker));
    draw(&mut window);

    assert!(window.debug_bounds("theme-transfer-done").is_some());
    let exported_bytes = std::fs::read(&output_path).expect("read exported theme");
    let exported = ThemeDocument::from_json_bytes(&exported_bytes).expect("parse exported theme");
    assert_eq!(exported.name(), fixture.name());
    assert_eq!(exported.color("terminal", "background"), Some([0x11, 0x22, 0x33]));
    assert_eq!(exported.color("terminal", "foreground"), Some([0xdd, 0xee, 0xff]));
    let exported_value = exported.to_value();
    assert_eq!(exported_value["vendor"]["future"], serde_json::json!(true));
    assert_eq!(exported_value["terminal"]["vendor_color"], "#102030");

    assert_eq!(runtime_snapshot(), before_runtime);
    assert_eq!(palette_snapshot(&mut window), before_palette);
    assert_eq!(settings_file_snapshot(), before_settings_file);
    assert_eq!(custom_theme_ids(), before_library_ids);
    click("theme-transfer-done", &mut window);
    std::fs::remove_file(import_path).expect("remove export import fixture");
    std::fs::remove_file(output_path).expect("remove exported theme");
}

#[gpui::test]
fn theme_transfer_cancel_and_invalid_input_leave_the_editor_recoverable(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let invalid_path = transfer_temp_path("invalid");
    std::fs::write(&invalid_path, b"not valid theme json").expect("write invalid theme");
    let (pane, mut window) = open_settings(cx);
    let before_runtime = runtime_snapshot();
    let before_palette = palette_snapshot(&mut window);
    let before_settings_file = settings_file_snapshot();
    let before_library_ids = custom_theme_ids();

    open_theme_editor(&mut window);
    let before_draft = editor_draft(&pane, &mut window);
    click("theme-editor-import", &mut window);
    assert!(window.did_prompt_for_paths());
    window.simulate_path_prompt_response(|_| None);
    draw(&mut window);
    assert!(window.debug_bounds("theme-transfer-dialog").is_none());
    assert_eq!(editor_draft(&pane, &mut window), before_draft);

    // A malformed native file shows a recoverable diagnostic instead of
    // closing the editor or changing any persisted state.
    click("theme-editor-import", &mut window);
    respond_with_import_path(&invalid_path, &mut window);
    assert!(window.debug_bounds("theme-transfer-dialog").is_some());
    assert!(window.debug_bounds("theme-transfer-candidate-0").is_none());
    assert!(
        pane.read_with(&mut window, |pane, _| {
            !pane.theme_transfer.import_diagnostics.is_empty()
        })
    );
    click("theme-transfer-cancel", &mut window);
    assert!(window.debug_bounds("theme-transfer-dialog").is_none());

    assert_eq!(editor_draft(&pane, &mut window), before_draft);
    assert_eq!(runtime_snapshot(), before_runtime);
    assert_eq!(palette_snapshot(&mut window), before_palette);
    assert_eq!(settings_file_snapshot(), before_settings_file);
    assert_eq!(custom_theme_ids(), before_library_ids);
    std::fs::remove_file(invalid_path).expect("remove invalid theme");
}

#[gpui::test]
fn theme_transfer_late_picker_result_is_ignored_after_rendered_cancel(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let (import_path, _) = write_transfer_fixture("late");
    let (pane, mut window) = open_settings(cx);
    let before_runtime = runtime_snapshot();
    let before_palette = palette_snapshot(&mut window);
    let before_settings_file = settings_file_snapshot();
    let before_library_ids = custom_theme_ids();

    open_theme_editor(&mut window);
    let before_draft = editor_draft(&pane, &mut window);
    click("theme-editor-import", &mut window);
    assert!(window.did_prompt_for_paths());
    click("theme-transfer-cancel", &mut window);
    assert!(window.debug_bounds("theme-transfer-dialog").is_none());

    // Complete the already-open native picker after the modal was closed. The
    // transfer sequence guard must discard this late result.
    respond_with_import_path(&import_path, &mut window);
    assert!(window.debug_bounds("theme-transfer-dialog").is_none());
    assert!(window.debug_bounds("theme-editor-dialog").is_some());
    assert_eq!(editor_draft(&pane, &mut window), before_draft);
    assert_eq!(runtime_snapshot(), before_runtime);
    assert_eq!(palette_snapshot(&mut window), before_palette);
    assert_eq!(settings_file_snapshot(), before_settings_file);
    assert_eq!(custom_theme_ids(), before_library_ids);
    std::fs::remove_file(import_path).expect("remove late import fixture");
}

#[gpui::test]
fn theme_picker_recommended_foreground_is_local_until_apply_and_restores_after_reopen(
    cx: &mut TestAppContext,
) {
    let _fixture_guard = lock_theme_studio();
    let _settings_guard = SettingsBytesGuard::capture();
    std::fs::write(nebula_settings::settings_path(), TEST_SETTINGS)
        .expect("write isolated picker settings");
    let (pane, mut window) = open_settings(cx);
    let before_runtime = runtime_snapshot();
    let before_palette = palette_snapshot(&mut window);
    let before_settings_file = settings_file_snapshot();

    click("open-theme-picker", &mut window);
    click("theme-foreground-swatch-1", &mut window);
    let mut selected_foreground = pane.read_with(&mut window, |pane, _| {
        pane.appearance_picker
            .as_ref()
            .and_then(|picker| picker.foreground_override)
            .expect("recommended foreground override")
    });
    // Keep the assertion independent of a stale runner file that happened to
    // contain the first recommendation before this isolated test started.
    if before_runtime.theme_foreground == Some(selected_foreground) {
        click("theme-foreground-swatch-2", &mut window);
        selected_foreground = pane.read_with(&mut window, |pane, _| {
            pane.appearance_picker
                .as_ref()
                .and_then(|picker| picker.foreground_override)
                .expect("second recommended foreground override")
        });
    }
    assert_eq!(runtime_snapshot(), before_runtime);
    assert_eq!(palette_snapshot(&mut window), before_palette);
    assert_eq!(settings_file_snapshot(), before_settings_file);

    click("apply-appearance-picker", &mut window);
    let after_apply_runtime = runtime_snapshot();
    let after_apply_palette = palette_snapshot(&mut window);
    let after_apply_settings = settings_file_snapshot();
    assert_eq!(after_apply_runtime.theme_foreground, Some(selected_foreground));
    assert_ne!(after_apply_runtime, before_runtime);
    assert_ne!(after_apply_palette, before_palette);
    assert_ne!(after_apply_settings, before_settings_file);

    click("open-theme-picker", &mut window);
    let reopened_foreground = pane.read_with(&mut window, |pane, _| {
        pane.appearance_picker.as_ref().and_then(|picker| picker.foreground_override)
    });
    assert_eq!(reopened_foreground, Some(selected_foreground));
    assert_eq!(runtime_snapshot().theme_foreground, Some(selected_foreground));
    click("cancel-appearance-picker", &mut window);
}

#[gpui::test]
fn theme_editor_save_only_forks_a_non_active_custom_template_without_publishing_it(
    cx: &mut TestAppContext,
) {
    let _fixture_guard = lock_theme_studio();
    let _settings_guard = SettingsBytesGuard::capture();
    std::fs::write(nebula_settings::settings_path(), TEST_SETTINGS)
        .expect("write isolated template settings");
    let mut theme_cleanup = CustomThemeCleanup::default();
    let source_name = format!("000000-nebula-template-source-{}", std::process::id());
    let source_seed = transfer_fixture_document("template-source");
    let source = ThemeLibraryStore::default()
        .import(&source_seed, Some(&source_name))
        .expect("create saved custom template");
    theme_cleanup.track(&source);
    let source_id = source.id().expect("custom source id").to_owned();
    let source_before = source.clone();

    let (pane, mut window) = open_settings(cx);
    let before_runtime = runtime_snapshot();
    let before_palette = palette_snapshot(&mut window);
    let before_settings_file = settings_file_snapshot();
    assert_eq!(before_runtime.custom_theme, None);

    // Open from the built-in draft, then use the real template Select to pick
    // the saved custom document. This deliberately leaves the source inactive.
    open_theme_editor(&mut window);
    let custom_template_index = ThemeName::BUILTIN.len()
        + custom_theme_documents()
            .iter()
            .position(|document| document.id() == Some(source_id.as_str()))
            .expect("source in template list");
    let current_template_index = pane.read_with(&mut window, |pane, cx| {
        pane.theme_editor
            .as_ref()
            .expect("theme editor")
            .template_select
            .read(cx)
            .selected_index(cx)
            .expect("selected template")
            .row
    });
    click("theme-editor-template", &mut window);
    for _ in current_template_index..custom_template_index {
        press("down", &mut window);
    }
    press("enter", &mut window);
    let selected_template_draft = editor_draft(&pane, &mut window);
    let source_definition = source.definition().expect("source definition");
    assert_eq!(selected_template_draft.terminal.background, source_definition.terminal.background);
    assert_eq!(selected_template_draft.terminal.foreground, source_definition.terminal.foreground);
    assert_eq!(runtime_snapshot(), before_runtime);
    assert_eq!(palette_snapshot(&mut window), before_palette);

    let saved_name = format!("000001-nebula-template-copy-{}", std::process::id());
    edit_input("theme-editor-name", &saved_name, &mut window);
    edit_input("theme-editor-foreground", "#c6d8ea", &mut window);
    let before_save_runtime = runtime_snapshot();
    let before_save_palette = palette_snapshot(&mut window);
    let before_save_settings = settings_file_snapshot();
    let before_save_ids = custom_theme_ids();
    click("theme-editor-save", &mut window);
    assert!(window.debug_bounds("theme-editor-dialog").is_some());

    let after_documents = custom_theme_documents();
    for document in &after_documents {
        if document.id().is_some_and(|id| !before_save_ids.iter().any(|before| before == id))
            && document.name().starts_with(&saved_name)
        {
            theme_cleanup.track(document);
        }
    }
    let source_after = after_documents
        .iter()
        .find(|document| document.id() == Some(source_id.as_str()))
        .expect("source remains in library");
    assert_eq!(source_after, &source_before);
    let copies: Vec<_> = after_documents
        .iter()
        .filter(|document| {
            document.id() != Some(source_id.as_str()) && document.name().starts_with(&saved_name)
        })
        .collect();
    assert_eq!(copies.len(), 1, "Save only creates one independent custom copy");
    assert_ne!(copies[0].id(), Some(source_id.as_str()));
    assert!(!copies[0].is_builtin());

    let after_save_runtime = runtime_snapshot();
    assert_eq!(after_save_runtime, before_save_runtime);
    assert_eq!(after_save_runtime.custom_theme, before_runtime.custom_theme);
    assert_eq!(palette_snapshot(&mut window), before_save_palette);
    assert_eq!(
        reloaded_palette_snapshot(&mut window, &RuntimeSettings::load()),
        before_save_palette
    );
    assert_eq!(settings_file_snapshot(), before_save_settings);
    assert_eq!(settings_file_snapshot(), before_settings_file);
}
