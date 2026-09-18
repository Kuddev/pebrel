//! Conditional publication of theme-related preferences.
//!
//! These cold operations run on the editor's background executor. The shared
//! settings crate still owns key replacement semantics; this adapter supplies
//! the application's existing lock and atomic-file boundary. A revision check
//! detects changes since the dialog loaded. External writers that do not use
//! the same lock can still race after that check.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use super::{DocumentError, ThemeDocument};

const MAX_PREFERENCE_BYTES: usize = 1024 * 1024;

/// Prepare one explicit application of a stored custom theme. Both the
/// picker and editor use these updates, so existing personal overrides cannot
/// accidentally mask a value the user has just confirmed in the preview.
/// Absent optional theme values keep the corresponding personal preference.
/// Preparing these values performs no I/O and does not apply the theme.
pub(crate) fn custom_theme_updates(
    document: &ThemeDocument,
    foreground: Option<nebula_settings::Rgb8>,
) -> Result<Vec<(&'static str, String)>, DocumentError> {
    let id = document.id().filter(|_| !document.is_builtin()).ok_or_else(|| {
        DocumentError::Invalid("applying a custom theme requires a saved library id".to_owned())
    })?;
    let definition = document.definition()?;
    let card = definition.layout.card;
    let mut updates = vec![
        ("custom_theme", id.to_owned()),
        ("theme", definition.base.prompt_name().to_owned()),
        ("follow_system_theme", "0".to_owned()),
        ("background", nebula_settings::format_hex_rgb(definition.terminal.background)),
        ("theme_foreground", foreground.map(nebula_settings::format_hex_rgb).unwrap_or_default()),
        ("pane_card_radius", card.radius.to_string()),
        ("pane_card_gutter", card.gutter.to_string()),
        ("pane_card_shadow", if card.shadow { "1" } else { "0" }.to_owned()),
        ("pane_card_divider", card.divider.to_string()),
    ];
    if let Some(family) = definition.typography.font_family {
        updates.push(("font_family", family));
    }
    if let Some(size) = definition.typography.font_size {
        updates.push(("font_size", size.to_string()));
    }
    if let Some(opacity) = definition.effects.opacity {
        updates.push(("opacity", opacity.to_string()));
    }
    if let Some(blur) = definition.effects.blur {
        updates.push(("blur", blur.settings_value().to_owned()));
    }
    if let Some(shape) = definition.effects.cursor_shape {
        updates.push(("cursor_shape", shape.settings_value().to_owned()));
    }
    Ok(updates)
}

/// Exact bytes last observed by the dialog, including an absent-file revision.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PreferenceRevision(Option<Vec<u8>>);

pub(crate) fn load() -> io::Result<PreferenceRevision> {
    read_at(&nebula_settings::settings_path())
}

pub(crate) fn save(
    expected: &PreferenceRevision,
    updates: &[(&str, String)],
) -> io::Result<PreferenceRevision> {
    save_at(&nebula_settings::settings_path(), expected, updates)
}

fn read_at(path: &Path) -> io::Result<PreferenceRevision> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(PreferenceRevision(None));
        },
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take((MAX_PREFERENCE_BYTES + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_PREFERENCE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "settings file exceeds 1 MiB"));
    }
    std::str::from_utf8(&bytes).map_err(|error| {
        io::Error::new(io::ErrorKind::InvalidData, format!("settings are not UTF-8: {error}"))
    })?;
    Ok(PreferenceRevision(Some(bytes)))
}

fn save_at(
    path: &Path,
    expected: &PreferenceRevision,
    updates: &[(&str, String)],
) -> io::Result<PreferenceRevision> {
    if updates.iter().any(|(key, value)| {
        key.is_empty() || key.contains(['=', '\r', '\n']) || value.contains(['\r', '\n'])
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a setting key or value contains a line separator",
        ));
    }
    let _lock = crate::atomic_file::try_lifetime_lock(path)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::WouldBlock, "settings are being saved by another window")
    })?;
    let current = read_at(path)?;
    if current != *expected {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "settings changed outside this dialog; reload before applying the theme",
        ));
    }
    let text = std::str::from_utf8(current.0.as_deref().unwrap_or_default())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let updated = nebula_settings::apply_updates(text, updates).into_bytes();
    if updated.len() > MAX_PREFERENCE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "settings file exceeds 1 MiB"));
    }
    if current.0.as_deref() != Some(updated.as_slice()) {
        crate::atomic_file::write(path, &updated)?;
    }
    Ok(PreferenceRevision(Some(updated)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmed_theme_options_replace_old_active_overrides_together() {
        let mut definition =
            nebula_settings::ThemeDefinition::from_builtin(nebula_settings::ThemeName::Nord);
        definition.terminal.foreground = [0xdd, 0xcc, 0xbb];
        definition.typography.font_size = Some(18.0);
        definition.typography.font_family = Some("Cascadia Code".to_owned());
        definition.effects.opacity = Some(0.7);
        definition.effects.blur = Some(nebula_settings::BlurModeName::Acrylic);
        definition.layout.card.radius = 12.0;
        definition.layout.card.gutter = 8.0;
        let document = super::super::from_definition(&definition)
            .unwrap()
            .fork("custom-confirmed", "Confirmed")
            .unwrap();
        let initial = "font_size=14\nfont_family=Old Font\nopacity=1\nblur=none\npane_card_radius=0\ntheme_foreground=#ffffff\nshell=cmd\n";
        let text = nebula_settings::apply_updates(
            initial,
            &custom_theme_updates(&document, None).unwrap(),
        );
        let runtime = nebula_settings::RuntimeSettings::from_raw(
            &nebula_settings::RawSettings::from_text(&text),
        );
        assert_eq!(runtime.custom_theme.as_deref(), Some("custom-confirmed"));
        assert_eq!(runtime.theme_foreground, None);
        assert_eq!(runtime.font_size_px, Some(18.0));
        assert_eq!(runtime.font_family.as_deref(), Some("Cascadia Code"));
        assert_eq!(runtime.opacity, 0.7);
        assert_eq!(runtime.blur, nebula_settings::BlurModeName::Acrylic);
        assert_eq!(runtime.pane_card_radius, Some(12.0));
        assert_eq!(runtime.pane_card_gutter, Some(8.0));
        assert_eq!(runtime.shell.as_deref(), Some("cmd"));
    }

    #[test]
    fn absent_theme_options_preserve_personal_settings_and_preview_override_is_explicit() {
        let document = super::super::builtin_document(nebula_settings::ThemeName::Nord)
            .unwrap()
            .fork("custom-colors-only", "Colors only")
            .unwrap();
        let foreground = [0xf0, 0xd0, 0xa0];
        let text = nebula_settings::apply_updates(
            "font_family=My Font\nfont_size=19\nopacity=0.85\nblur=mica\n",
            &custom_theme_updates(&document, Some(foreground)).unwrap(),
        );
        let runtime = nebula_settings::RuntimeSettings::from_raw(
            &nebula_settings::RawSettings::from_text(&text),
        );
        assert_eq!(runtime.font_family.as_deref(), Some("My Font"));
        assert_eq!(runtime.font_size_px, Some(19.0));
        assert_eq!(runtime.opacity, 0.85);
        assert_eq!(runtime.blur, nebula_settings::BlurModeName::Mica);
        assert_eq!(runtime.theme_foreground, Some(foreground));
    }

    #[test]
    fn applying_theme_and_foreground_is_one_revision_and_preserves_other_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.txt");
        let initial = "# personal settings\r\nTHEME=Paper\r\nfont_family=Maple Mono NF CN\r\ncustom_key=keep\r\n";
        std::fs::write(&path, initial).unwrap();
        let revision = read_at(&path).unwrap();
        let applied = save_at(
            &path,
            &revision,
            &[
                ("theme", "Nord".to_owned()),
                ("custom_theme", "".to_owned()),
                ("theme_foreground", "#e0bc91".to_owned()),
            ],
        )
        .unwrap();
        assert_eq!(read_at(&path).unwrap(), applied);
        let saved = std::fs::read_to_string(&path).unwrap();
        let runtime = nebula_settings::RuntimeSettings::from_raw(
            &nebula_settings::RawSettings::from_text(&saved),
        );
        assert_eq!(runtime.theme, nebula_settings::ThemeName::Nord);
        assert_eq!(runtime.theme_foreground, Some([0xe0, 0xbc, 0x91]));
        assert_eq!(runtime.font_family.as_deref(), Some("Maple Mono NF CN"));
        assert!(saved.contains("custom_key=keep"));
        assert!(saved.contains("# personal settings"));
        assert_eq!(save_at(&path, &applied, &[]).unwrap(), applied);
    }

    #[test]
    fn stale_dialog_cannot_overwrite_an_external_edit_or_a_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.txt");
        let absent = read_at(&path).unwrap();
        std::fs::write(&path, "font_size=19\n").unwrap();
        let error = save_at(&path, &absent, &[("theme", "Nord".to_owned())]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "font_size=19\n");
        let loaded = read_at(&path).unwrap();
        std::fs::write(&path, "font_size=21\n").unwrap();
        assert!(save_at(&path, &loaded, &[("theme", "Paper".to_owned())]).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "font_size=21\n");
    }

    #[test]
    fn busy_writer_and_invalid_input_preserve_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.txt");
        std::fs::write(&path, "theme=Paper\n").unwrap();
        let loaded = read_at(&path).unwrap();
        let lock = crate::atomic_file::try_lifetime_lock(&path).unwrap().unwrap();
        assert_eq!(
            save_at(&path, &loaded, &[("theme", "Nord".to_owned())]).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(lock);
        assert_eq!(
            save_at(&path, &loaded, &[("font_family", "Mono\nopacity=0".to_owned())])
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "theme=Paper\n");
    }

    #[test]
    fn malformed_or_oversized_settings_are_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.txt");
        std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(read_at(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(std::fs::read(&path).unwrap(), [0xff, 0xfe, 0x00]);
        std::fs::write(&path, vec![b'x'; MAX_PREFERENCE_BYTES + 1]).unwrap();
        assert_eq!(read_at(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), (MAX_PREFERENCE_BYTES + 1) as u64);
    }
}
