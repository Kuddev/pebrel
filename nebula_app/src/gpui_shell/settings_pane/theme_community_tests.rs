//! Community catalog cards inside the theme picker: install, failure and
//! apply-only-on-confirmation, driven through rendered hitboxes.
use super::super::appearance_picker::AppearanceSelection;
use super::*;
use crate::theme_library::community::{self, Entry, Installed, Listing};
use crate::theme_library::package::{Author, export_package};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

static ARCHIVE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Local stand-in for the HTTP download: same receipt and package checks.
fn install_fixture(
    entry: &Entry,
    store: &ThemeLibraryStore,
    _cancel: &AtomicBool,
) -> Result<ThemeDocument, community::Error> {
    if entry.id.ends_with("/broken") {
        return Err(community::Error::Integrity);
    }
    let archive = ARCHIVE.lock().unwrap().clone().expect("fixture archive");
    community::install_file(&archive, entry, store)
}

fn row(id: &str, archive: &[u8]) -> serde_json::Value {
    let sha256: String = Sha256::digest(archive).iter().map(|b| format!("{b:02x}")).collect();
    serde_json::json!({
        "id": id,
        "name": "Community fixture",
        "version": "1.0.0",
        "author": {"name": "Fixture author", "github": "fixture"},
        "source": "https://github.com/fixture/themes",
        "license": "MIT",
        "archive": {
            "url": "https://github.com/fixture/themes/releases/download/v1.0.0/fixture.pebrel-theme.zip",
            "sha256": sha256,
            "bytes": archive.len()
        },
        "unpacked_bytes": archive.len(),
        "video_bytes": 0,
        "description": "Fixture listing."
    })
}

fn fixture(dir: &std::path::Path, name: &str) -> Listing {
    let mut definition = ThemeDefinition::from_builtin(ThemeName::Nord);
    definition.name = name.to_owned();
    let document = crate::theme_library::from_definition(&definition).unwrap();
    let path = dir.join("fixture.pebrel-theme.zip");
    let author = Author { name: "Fixture author".into(), github: None };
    export_package(&document, author, "1.0.0".into(), "MIT".into(), &path, None).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    *ARCHIVE.lock().unwrap() = Some(path);
    let catalog = serde_json::json!({
        "schema_version": 1,
        "themes": [row("fixture/community", &bytes), row("fixture/broken", &bytes)],
    });
    Listing::parse(&serde_json::to_vec(&catalog).unwrap()).unwrap()
}

#[gpui::test]
fn community_install_adds_a_previewed_theme_and_applies_only_on_confirmation(
    cx: &mut TestAppContext,
) {
    let _fixture_guard = lock_theme_studio();
    let _settings_guard = SettingsBytesGuard::capture();
    std::fs::create_dir_all(nebula_settings::settings_dir()).unwrap();
    std::fs::write(nebula_settings::settings_path(), TEST_SETTINGS).unwrap();
    let mut cleanup = CustomThemeCleanup::default();
    let dir = tempfile::tempdir().unwrap();
    let name = format!("Community fixture {}", std::process::id());
    let listing = fixture(dir.path(), &name);
    let (pane, mut window) = open_settings(cx);
    click("open-theme-picker", &mut window);
    pane.update(&mut window, |pane, _| pane.seed_community(listing, install_fixture));
    click("appearance-filter-4", &mut window);
    assert!(window.debug_bounds("community-theme-list").is_some());
    assert!(window.debug_bounds("community-theme-1").is_some());
    assert!(
        window.debug_bounds("appearance-theme-grid").is_none_or(|grid| grid.size.height < px(1.0)),
        "built-in cards stay out of the Community filter"
    );
    let before = runtime_snapshot();
    let settings_before = settings_file_snapshot();

    click("community-action-1", &mut window);
    pane.read_with(&mut window, |pane, _| {
        let community = pane.appearance_picker.as_ref().unwrap().community.as_ref().unwrap();
        assert!(matches!(
            community.installs.get("fixture/broken"),
            Some(super::super::theme_community::InstallState::Failed(_))
        ));
        assert!(community.notice.is_none());
    });

    click("community-action-0", &mut window);
    let installed = pane.read_with(&mut window, |pane, _| {
        let picker = pane.appearance_picker.as_ref().unwrap();
        let community = picker.community.as_ref().unwrap();
        assert!(community.installs.get("fixture/community").is_none());
        assert!(community.notice.as_deref().is_some_and(|text| text.contains(&name)));
        let AppearanceSelection::Custom(index) = picker.draft else {
            panic!("the installed theme becomes the previewed draft");
        };
        let document = picker.custom_themes[index].clone();
        assert_eq!(document.name(), name);
        let entry = community.entry("fixture/community").unwrap();
        assert_eq!(
            community::installed(entry, &picker.custom_themes),
            Installed::Current(document.id().unwrap().to_owned())
        );
        document
    });
    cleanup.track(&installed);
    assert!(window.debug_bounds("community-notice").is_some());
    assert_eq!(runtime_snapshot(), before, "installing never applies the theme");
    assert_eq!(settings_file_snapshot(), settings_before);

    // A second click on the installed card is inert; no duplicate is created.
    click("community-action-0", &mut window);
    let copies = custom_theme_documents().iter().filter(|doc| doc.name() == name).count();
    assert_eq!(copies, 1);

    click("apply-appearance-picker", &mut window);
    assert_eq!(runtime_snapshot().custom_theme.as_deref(), installed.id());
    let store = ThemeLibraryStore::default();
    let id = installed.id().unwrap();
    store.delete(id, store.load(id).unwrap().revision()).unwrap();
}

#[gpui::test]
fn closing_the_picker_cancels_community_work_and_drops_late_results(cx: &mut TestAppContext) {
    let _fixture_guard = lock_theme_studio();
    let _settings_guard = SettingsBytesGuard::capture();
    std::fs::create_dir_all(nebula_settings::settings_dir()).unwrap();
    std::fs::write(nebula_settings::settings_path(), TEST_SETTINGS).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let listing = fixture(dir.path(), &format!("Community closed {}", std::process::id()));
    let (pane, mut window) = open_settings(cx);
    click("open-theme-picker", &mut window);
    pane.update(&mut window, |pane, _| pane.seed_community(listing.clone(), install_fixture));
    click("appearance-filter-4", &mut window);
    click("close-appearance-picker", &mut window);
    click("open-theme-picker", &mut window);
    pane.read_with(&mut window, |pane, _| {
        assert!(pane.appearance_picker.as_ref().unwrap().community.is_none());
    });
    // The new picker owns a new generation; it never sees the old state.
    pane.update(&mut window, |pane, _| pane.seed_community(listing, install_fixture));
    click("appearance-filter-4", &mut window);
    assert!(window.debug_bounds("community-theme-0").is_some());
}
