use super::*;

use nebula_settings::{CursorShapeName, ThemeDefinition, ThemeEffects, ThemeName, ThemeTypography};

#[test]
fn every_builtin_round_trips_through_the_native_envelope() {
    assert_eq!(builtin_documents().len(), ThemeName::BUILTIN.len());
    for theme in ThemeName::BUILTIN {
        let definition = ThemeDefinition::from_builtin(theme);
        let document = from_definition(&definition).expect("built-in document");
        assert_eq!(document.definition().expect("resolved definition"), definition);
    }
}

#[test]
fn optional_runtime_overrides_stay_optional() {
    let mut definition = ThemeDefinition::from_builtin(ThemeName::Nord);
    definition.typography = ThemeTypography { font_size: Some(18.0), ..ThemeTypography::default() };
    definition.effects = ThemeEffects::default();
    let document = from_definition(&definition).expect("native document");
    let resolved = document.definition().expect("resolved definition");
    assert_eq!(resolved, definition);
    assert!(document.to_value()["typography"]["font_family"].is_null());
    assert!(document.to_value()["effects"]["blur"].is_null());
}

#[test]
fn native_round_trip_preserves_layout_cursor_and_typography_overrides() {
    let mut definition = ThemeDefinition::from_builtin(ThemeName::Nord);
    definition.layout.padding_x = Some(32.0);
    definition.layout.padding_y = Some(24.0);
    definition.effects.cursor_shape = Some(CursorShapeName::Beam);
    definition.typography.font_family = Some("Cascadia Code".to_owned());
    definition.typography.font_size = Some(15.0);
    definition.typography.line_height = Some(1.6);
    definition.typography.ligatures = false;
    definition.typography.ui_font_family = Some("Segoe UI".to_owned());
    definition.typography.ui_font_size = Some(13.0);

    let document = from_definition(&definition).expect("native document");
    assert_eq!(document.definition().expect("resolved definition"), definition);
}

#[test]
fn with_definition_replaces_known_extended_fields_and_keeps_vendor_data() {
    let mut value = builtin_documents().remove(0).to_value();
    value["vendor"] = serde_json::json!({"future": true});
    let document = ThemeDocument::from_value(value).expect("document");

    let mut definition = document.definition().expect("definition");
    definition.typography.ligatures = false;
    definition.typography.ui_font_family = Some("Segoe UI".to_owned());
    definition.typography.ui_font_size = Some(13.0);
    definition.layout.padding_x = Some(32.0);
    definition.effects.cursor_shape = Some(CursorShapeName::Underline);

    let replaced = document.with_definition(&definition).expect("replacement");
    assert_eq!(replaced.to_value()["vendor"]["future"], true);
    assert_eq!(replaced.definition().expect("resolved replacement"), definition);
}

#[test]
fn native_merge_preserves_unknown_fields() {
    let mut value = builtin_documents().remove(0).to_value();
    value["vendor"] = serde_json::json!({"future": true});
    value["terminal"]["vendor_color"] = serde_json::json!("#102030");
    let document = ThemeDocument::from_value(value).expect("document");
    let mut definition = document.definition().expect("definition");
    definition.terminal.foreground = [1, 2, 3];
    let replaced = document.with_definition(&definition).expect("replacement");
    assert_eq!(replaced.to_value()["vendor"]["future"], true);
    assert_eq!(replaced.to_value()["terminal"]["vendor_color"], "#102030");
    assert_eq!(replaced.color("terminal", "foreground"), Some([1, 2, 3]));
}

#[test]
fn store_fork_uses_revision_and_name_conflicts() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = ThemeLibraryStore::new(directory.path());
    let first = store.fork_builtin("Nord", Some("Nord")).expect("first fork");
    assert_eq!(first.name(), "Nord (2)");
    assert_eq!(first.revision(), 1);
    let second = store.fork_builtin("Nord", Some("Nord")).expect("second fork");
    assert_eq!(second.name(), "Nord (3)");
    assert_eq!(second.revision(), 1);

    let stale = first.with_name("Changed").expect("rename");
    let error = store.save(&stale, RevisionPrecondition::Exact(0)).expect_err("stale revision");
    assert!(matches!(error, StoreError::Conflict { .. }));
    assert_eq!(store.load(first.id().expect("id")).expect("load").name(), first.name());

    let path = store.path_for(first.id().expect("id")).expect("path");
    let mut external = first.to_value();
    external["terminal"]["foreground"] = serde_json::json!("#010203");
    std::fs::write(&path, serde_json::to_vec(&external).expect("json")).expect("external edit");
    let error = store
        .save(&first, RevisionPrecondition::Unchanged(first.clone()))
        .expect_err("same revision external edit");
    assert!(matches!(error, StoreError::Conflict { .. }));
}

#[test]
fn store_import_names_are_utf8_safe_at_the_byte_limit() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = ThemeLibraryStore::new(directory.path());
    let builtin = builtin_document(ThemeName::Nord).expect("built-in document");

    // 42 CJK characters (126 bytes) plus one ASCII byte exercises a valid
    // name immediately below the 128-byte limit.
    let near_limit = format!("{}x", "界".repeat(42));
    let near_limit_theme = store.import(&builtin, Some(&near_limit)).expect("near-limit import");
    assert_eq!(near_limit_theme.name().len(), 127);

    // The first candidate is truncated on a character boundary. The second
    // candidate must reserve bytes for its suffix instead of truncating the
    // suffix away and returning the same display name.
    let long_name = "主题".repeat(30);
    let first = store.import(&builtin, Some(&long_name)).expect("long import");
    let second = store.import(&builtin, Some(&long_name)).expect("duplicate long import");
    assert!(first.name().len() <= 128);
    assert!(second.name().len() <= 128);
    assert_ne!(first.name(), second.name());
    assert!(second.name().ends_with(" (2)"));
}

#[test]
fn format_adapters_are_bounded_and_static() {
    let inspection = inspect(
        "{\"name\":\"Solar\",\"background\":\"#001122\",\"foreground\":\"#ddeeff\",\"black\":\"#000000\"}",
        "solar.json",
    );
    assert_eq!(inspection.candidates.len(), 1);
    assert!(inspection.candidates[0].warnings.iter().any(|warning| warning.contains("ANSI")));

    let kitty = inspect(
        "foreground #ddeeff\nbackground #001122\ninclude ~/.config/kitty/theme.conf\ncolor0 #000000\n",
        "theme.conf",
    );
    assert_eq!(kitty.candidates.len(), 1);
    assert!(kitty.candidates[0].warnings.iter().any(|warning| warning.contains("not executed")));

    let unsupported = inspect("<plist><dict/></plist>", "theme.itermcolors");
    assert!(unsupported.candidates.is_empty());
    assert!(unsupported.diagnostics[0].message.contains("unavailable"));
}
