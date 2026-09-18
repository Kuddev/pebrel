//! Format contracts exercise interchange as well as independent target schemas.

use super::*;
use nebula_settings::{ThemeDefinition, ThemeName};

const STATIC_FORMATS: [ThemeFormat; 6] = [
    ThemeFormat::Pebrel,
    ThemeFormat::WindowsTerminal,
    ThemeFormat::Kitty,
    ThemeFormat::Ghostty,
    ThemeFormat::WezTerm,
    ThemeFormat::Alacritty,
];

fn round_trip(document: &ThemeDocument, format: ThemeFormat) -> ThemeDocument {
    let artifact = export(document, format).expect("export supported format");
    let inspection = inspect(&artifact.text, format!("theme.{}", artifact.extension));
    assert!(inspection.diagnostics.is_empty(), "{format}: {:?}", inspection.diagnostics);
    assert_eq!(inspection.candidates.len(), 1, "{format}");
    inspection.candidates.into_iter().next().unwrap().document
}

#[test]
fn fifteen_presets_retain_default_and_ansi_colors_in_six_static_formats() {
    for theme in ThemeName::BUILTIN {
        let original = builtin_document(theme).unwrap();
        for format in STATIC_FORMATS {
            let imported = round_trip(&original, format);
            assert_eq!(
                imported.color("terminal", "background"),
                original.color("terminal", "background"),
                "{theme:?} / {format} background"
            );
            assert_eq!(
                imported.color("terminal", "foreground"),
                original.color("terminal", "foreground"),
                "{theme:?} / {format} foreground"
            );
            assert_eq!(imported.palette(), original.palette(), "{theme:?} / {format} ANSI");
            if format == ThemeFormat::Pebrel {
                assert_eq!(imported, original, "native envelope must preserve every field");
            }
        }
    }
}

#[test]
fn indexed_colors_and_cursor_selection_roles_survive_formats_that_support_them() {
    let mut definition = ThemeDefinition::from_builtin(ThemeName::Nord);
    definition.terminal.cursor = Some([17, 34, 51]);
    definition.terminal.cursor_text = Some([68, 85, 102]);
    definition.terminal.selection_background = Some([119, 136, 153]);
    definition.terminal.selection_foreground = Some([170, 187, 204]);
    for (index, color) in [(16, [11, 22, 33]), (129, [44, 55, 66]), (255, [77, 88, 99])] {
        assert!(definition.terminal.palette.set(index, color));
    }
    let document = from_definition(&definition).unwrap();
    for format in [
        ThemeFormat::Pebrel,
        ThemeFormat::Kitty,
        ThemeFormat::Ghostty,
        ThemeFormat::WezTerm,
        ThemeFormat::Alacritty,
    ] {
        let imported = round_trip(&document, format).definition().unwrap();
        assert_eq!(imported.terminal.palette, definition.terminal.palette, "{format}");
        assert_eq!(imported.terminal.cursor, definition.terminal.cursor, "{format}");
        assert_eq!(imported.terminal.cursor_text, definition.terminal.cursor_text, "{format}");
        assert_eq!(
            imported.terminal.selection_background, definition.terminal.selection_background,
            "{format}"
        );
        assert_eq!(
            imported.terminal.selection_foreground, definition.terminal.selection_foreground,
            "{format}"
        );
    }
}

#[test]
fn alacritty_uses_its_magenta_field_instead_of_windows_terminal_purple() {
    let source = r##"
[colors.primary]
background = "#001122"
foreground = "#ddeeff"
[colors.selection]
background = "#203040"
text = "#eeeeee"
[colors.normal]
black = "#010101"
red = "#020202"
green = "#030303"
yellow = "#040404"
blue = "#050505"
magenta = "#162738"
cyan = "#070707"
white = "#080808"
[colors.bright]
black = "#111111"
red = "#222222"
green = "#333333"
yellow = "#444444"
blue = "#555555"
magenta = "#a6b7c8"
cyan = "#777777"
white = "#888888"
"##;
    let inspection = inspect(source, "alacritty.toml");
    assert!(inspection.diagnostics.is_empty(), "{:?}", inspection.diagnostics);
    let candidate = &inspection.candidates[0];
    assert_eq!(
        candidate.document.color("terminal", "selection_background"),
        Some([0x20, 0x30, 0x40])
    );
    assert_eq!(candidate.document.palette().unwrap()[5], [0x16, 0x27, 0x38]);
    assert_eq!(candidate.document.palette().unwrap()[13], [0xa6, 0xb7, 0xc8]);
    let exported = export(&candidate.document, ThemeFormat::Alacritty).unwrap();
    let data: toml::Value = toml::from_str(&exported.text).unwrap();
    assert_eq!(data["colors"]["normal"]["magenta"].as_str(), Some("#162738"));
    assert_eq!(data["colors"]["bright"]["magenta"].as_str(), Some("#a6b7c8"));
    assert!(data["colors"]["normal"].get("purple").is_none());
    assert_eq!(data["colors"]["selection"]["background"].as_str(), Some("#203040"));
    assert!(data["colors"]["selection"].get("cursor").is_none());
}

#[test]
fn lossy_exports_report_omitted_fields_and_native_export_is_lossless() {
    let document = builtin_document(ThemeName::Nord).unwrap();
    for format in STATIC_FORMATS {
        let artifact = export(&document, format).unwrap();
        assert_eq!(artifact.losses.is_empty(), format == ThemeFormat::Pebrel, "{format}");
    }
    let windows = export(&document, ThemeFormat::WindowsTerminal).unwrap();
    assert!(windows.losses.iter().any(|loss| loss.contains("indexed")));
}

#[test]
fn invalid_colors_oversized_files_and_scripts_never_produce_an_applicable_theme() {
    for (text, filename) in [
        ("foreground = #zzzzzz\nbackground = #000000\n", "ghostty.conf"),
        ("return { colors = os.execute('echo unexpected') }", "wezterm.lua"),
        ("{\"schema_version\":999,\"name\":\"Future\"}", "future.pebrel-theme.json"),
    ] {
        let inspection = inspect(text, filename);
        assert!(inspection.candidates.is_empty(), "{filename}");
        assert!(!inspection.diagnostics.is_empty(), "{filename}");
    }
    let inspection = inspect(&"x".repeat(MAX_INPUT_BYTES + 1), "oversized.conf");
    assert!(inspection.candidates.is_empty());
    assert!(!inspection.diagnostics.is_empty());
}

#[test]
fn unrecognized_theme_envelopes_are_rejected_by_format() {
    let text = "[theme]\nname = 'Unknown'\nbackground = '#001122'\n";
    for filename in ["custom.theme", "custom.conf", "custom.json"] {
        let inspection = inspect(text, filename);
        assert!(inspection.candidates.is_empty(), "{filename}");
        assert!(!inspection.diagnostics.is_empty(), "{filename}");
    }
}

#[test]
fn a_small_input_cannot_expand_to_an_unbounded_number_of_theme_snapshots() {
    let scheme = serde_json::json!({
        "name": "Compact",
        "background": "#000000",
        "foreground": "#ffffff"
    });
    for collection in [
        serde_json::json!(vec![scheme.clone(); MAX_IMPORT_CANDIDATES + 1]),
        serde_json::json!({ "schemes": vec![scheme.clone(); MAX_IMPORT_CANDIDATES + 1] }),
        serde_json::json!({ "themes": vec![scheme; MAX_IMPORT_CANDIDATES + 1] }),
    ] {
        let text = serde_json::to_string(&collection).unwrap();
        assert!(text.len() < MAX_INPUT_BYTES);
        let inspection = inspect(&text, "collection.json");
        assert!(inspection.candidates.is_empty());
        assert!(!inspection.diagnostics.is_empty());
    }
}
