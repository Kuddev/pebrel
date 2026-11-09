//! 安装库与主题贡献：授权、只复制声明文件、启停、卸载保留主题，以及生成结果的校验。

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::install::Library;
use super::package::Grant;
use super::*;

const MANIFEST: &str = r#"
manifest_version = 1
api_version = 1
id = "example.ocean"
name = "Ocean"
version = "1.0.0"
entry = "init.luau"
[[commands]]
id = "generate"
title = "Generate Ocean"
handler = "generate"
output = "theme"
"#;

const SOURCE: &str = r##"
return {
    generate = function(ctx, args)
        return { base = "Nord", name = "Ocean", terminal = { background = "#001122" } }
    end,
}
"##;

fn write_package(directory: &Path, manifest: &str, source: &str) {
    fs::write(directory.join("plugin.toml"), manifest).unwrap();
    fs::write(directory.join("init.luau"), source).unwrap();
}

fn fixture() -> (tempfile::TempDir, tempfile::TempDir, Library) {
    let source = tempfile::tempdir().unwrap();
    write_package(source.path(), MANIFEST, SOURCE);
    let config = tempfile::tempdir().unwrap();
    let library = Library::new(config.path().join("plugins"));
    (source, config, library)
}

#[test]
fn install_requires_every_requested_grant_before_touching_the_library() {
    let (source, config, library) = fixture();
    let error = library.install(source.path(), &[]).unwrap_err();
    assert_eq!(error.code, "grant_required");
    assert_eq!(error.details.unwrap()["missing"], json!(["theme_library"]));
    assert!(!config.path().join("plugins/example.ocean").exists());
    assert!(library.list().unwrap().plugins.is_empty());
}

#[test]
fn install_copies_only_declared_files_and_records_host_state() {
    let (source, config, library) = fixture();
    fs::write(source.path().join("secret.txt"), "not declared").unwrap();
    fs::write(source.path().join("pebrel-install.json"), r#"{"enabled":true}"#).unwrap();
    let plugin = library.install(source.path(), &[Grant::ThemeLibrary]).unwrap();
    assert!(plugin.enabled);
    assert_eq!(plugin.grants, vec![Grant::ThemeLibrary]);
    assert_eq!(plugin.theme_commands[0].id, "generate");
    let installed = config.path().join("plugins/example.ocean");
    let mut files: Vec<_> = fs::read_dir(&installed)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    files.sort();
    assert_eq!(files, ["init.luau", "pebrel-install.json", "plugin.toml"]);
    let receipt: Value =
        serde_json::from_slice(&fs::read(installed.join("pebrel-install.json")).unwrap()).unwrap();
    assert_eq!(receipt["grants"], json!(["theme_library"]));
    assert_eq!(library.list().unwrap().plugins, vec![plugin]);
}

#[test]
fn unrequested_grants_are_not_recorded() {
    let source = tempfile::tempdir().unwrap();
    fs::write(source.path().join("plugin.toml"), MANIFEST.replace("output = \"theme\"", ""))
        .unwrap();
    fs::write(source.path().join("init.luau"), SOURCE).unwrap();
    let config = tempfile::tempdir().unwrap();
    let library = Library::new(config.path().join("plugins"));
    let plugin = library.install(source.path(), &[Grant::ThemeLibrary]).unwrap();
    assert!(plugin.grants.is_empty());
    assert!(plugin.theme_commands.is_empty());
}

#[test]
fn disabled_plugins_do_not_open_until_enabled_again() {
    let (source, _config, library) = fixture();
    library.install(source.path(), &[Grant::ThemeLibrary]).unwrap();
    assert!(!library.set_enabled("example.ocean", false).unwrap().enabled);
    assert_eq!(library.open_enabled("example.ocean").unwrap_err().code, "plugin_disabled");
    assert!(library.set_enabled("example.ocean", true).unwrap().enabled);
    assert!(library.open_enabled("example.ocean").is_ok());
}

#[test]
fn reinstall_replaces_the_previous_version_and_enables_it() {
    let (source, _config, library) = fixture();
    library.install(source.path(), &[Grant::ThemeLibrary]).unwrap();
    library.set_enabled("example.ocean", false).unwrap();
    write_package(source.path(), &MANIFEST.replace("1.0.0", "1.1.0"), SOURCE);
    let plugin = library.install(source.path(), &[Grant::ThemeLibrary]).unwrap();
    assert_eq!(plugin.version, "1.1.0");
    assert!(plugin.enabled);
    let listing = library.list().unwrap();
    assert_eq!(listing.plugins.len(), 1);
    assert!(listing.broken.is_empty());
}

#[test]
fn uninstall_removes_files_and_broken_installs_are_listed_not_hidden() {
    let (source, config, library) = fixture();
    library.install(source.path(), &[Grant::ThemeLibrary]).unwrap();
    let broken = config.path().join("plugins/example.broken");
    fs::create_dir_all(&broken).unwrap();
    let listing = library.list().unwrap();
    assert_eq!(listing.broken[0].id, "example.broken");
    library.uninstall("example.ocean").unwrap();
    library.uninstall("example.broken").unwrap();
    let listing = library.list().unwrap();
    assert!(listing.plugins.is_empty() && listing.broken.is_empty());
    assert_eq!(library.uninstall("example.ocean").unwrap_err().code, "plugin_not_installed");
    assert_eq!(library.uninstall("../themes").unwrap_err().code, "invalid_plugin_id");
}

#[test]
fn windows_device_names_cannot_become_plugin_directories() {
    let source = tempfile::tempdir().unwrap();
    write_package(source.path(), &MANIFEST.replace("example.ocean", "com1.theme"), SOURCE);
    let config = tempfile::tempdir().unwrap();
    let library = Library::new(config.path().join("plugins"));
    assert_eq!(
        library.install(source.path(), &[Grant::ThemeLibrary]).unwrap_err().code,
        "invalid_metadata"
    );
}

#[test]
fn theme_output_requires_a_luau_handler() {
    let manifest = r#"
manifest_version = 1
api_version = 1
id = "example.native"
name = "Native"
version = "1.0.0"
permissions = ["runtime.describe"]
[[commands]]
id = "describe"
title = "Describe"
method = "runtime.describe"
output = "theme"
"#;
    let source = tempfile::tempdir().unwrap();
    fs::write(source.path().join("plugin.toml"), manifest).unwrap();
    assert_eq!(Package::open(source.path()).unwrap_err().code, "invalid_command");
}

#[cfg(feature = "gpui-shell")]
mod theme_contributions {
    use super::*;
    use crate::plugins::themes::{self, BundledTheme};
    use crate::theme_library::package::{Author, Origin, export_package};
    use crate::theme_library::{ThemeLibraryStore, builtin_document};
    use nebula_settings::ThemeName;
    use std::time::Duration;

    fn bundled_fixture() -> (tempfile::TempDir, tempfile::TempDir, Library, ThemeLibraryStore) {
        let (source, config, library) = fixture();
        let document = builtin_document(ThemeName::default()).unwrap().with_name("Reef").unwrap();
        fs::create_dir(source.path().join("themes")).unwrap();
        export_package(
            &document,
            Author { name: "Example".into(), github: None },
            "1.0.0".into(),
            "MIT".into(),
            &source.path().join("themes/reef.pebrel-theme.zip"),
            None,
        )
        .unwrap();
        write_package(
            source.path(),
            &MANIFEST.replace(
                "entry = \"init.luau\"",
                "entry = \"init.luau\"\nthemes = [\"themes/reef.pebrel-theme.zip\"]",
            ),
            SOURCE,
        );
        let store = ThemeLibraryStore::new(config.path().join("themes"));
        (source, config, library, store)
    }

    #[test]
    fn bundled_themes_are_added_once_with_a_plugin_receipt() {
        let (source, _config, library, store) = bundled_fixture();
        library.install(source.path(), &[Grant::ThemeLibrary]).unwrap();
        let first = themes::install_bundled(&library, "example.ocean", &store).unwrap();
        let [BundledTheme::Added { theme_id, name, .. }] = first.as_slice() else {
            panic!("expected one added theme: {first:?}");
        };
        assert_eq!(name, "Reef");
        let again = themes::install_bundled(&library, "example.ocean", &store).unwrap();
        assert!(matches!(
            again.as_slice(),
            [BundledTheme::Present { theme_id: id, .. }] if id == theme_id
        ));
        let custom = store.list().unwrap().custom;
        assert_eq!(custom.len(), 1);
        let origin = Origin::of(&custom[0]).unwrap();
        assert_eq!(origin.catalog, "plugin:example.ocean");
        assert_eq!(origin.id, "themes/reef.pebrel-theme.zip");
        // 卸载插件不删除它加入主题库的主题。
        library.uninstall("example.ocean").unwrap();
        assert_eq!(store.list().unwrap().custom.len(), 1);
    }

    #[test]
    fn invalid_bundled_packages_fail_the_install() {
        let (source, config, library, _store) = bundled_fixture();
        fs::write(source.path().join("themes/reef.pebrel-theme.zip"), b"not a zip").unwrap();
        let error = library.install(source.path(), &[Grant::ThemeLibrary]).unwrap_err();
        assert_eq!(error.code, "invalid_theme_package");
        assert!(!config.path().join("plugins/example.ocean").exists());
    }

    fn run(package: &Package) -> Value {
        execute(package, "generate", json!({}), Duration::from_secs(5), &mut |_, _, _| {
            panic!("theme generation must not call Runtime")
        })
        .unwrap()
        .result
    }

    #[test]
    fn generated_spec_is_validated_and_added_only_with_the_grant() {
        let (source, config, _library) = fixture();
        let store = ThemeLibraryStore::new(config.path().join("themes"));
        let package = Package::open(source.path()).unwrap();
        let preview = themes::finish(run(&package), &package, "generate", None, &store).unwrap();
        assert_eq!(preview["added_to_library"], false);
        assert_eq!(preview["theme"]["terminal"]["background"], "#001122");
        assert_eq!(
            preview["theme"]["metadata"]["based_on"],
            builtin_document(ThemeName::Nord).unwrap().name()
        );
        assert_eq!(preview["theme"]["metadata"]["generator"]["plugin"], "example.ocean");
        assert!(store.list().unwrap().custom.is_empty());

        let added = themes::finish(
            run(&package),
            &package,
            "generate",
            Some(&[Grant::ThemeLibrary]),
            &store,
        )
        .unwrap();
        assert_eq!(added["added_to_library"], true);
        let stored = store.load(added["theme"]["id"].as_str().unwrap()).unwrap();
        assert_eq!(stored.name(), "Ocean");
        assert!(!stored.is_builtin());
    }

    #[test]
    fn example_theme_generator_produces_a_valid_document() {
        let source = tempfile::tempdir().unwrap();
        write_package(
            source.path(),
            include_str!("../../examples/plugins/theme-generator/plugin.toml"),
            include_str!("../../examples/plugins/theme-generator/init.luau"),
        );
        let package = Package::open(source.path()).unwrap();
        let result =
            execute(&package, "ocean", json!({}), Duration::from_secs(5), &mut |_, _, _| {
                panic!("the example does not call Runtime")
            })
            .unwrap()
            .result;
        let store = ThemeLibraryStore::new(source.path().join("themes"));
        let preview = themes::finish(result, &package, "ocean", None, &store).unwrap();
        assert_eq!(preview["theme"]["name"], "Ocean");
        assert_eq!(preview["added_to_library"], false);
    }

    #[test]
    fn invalid_generated_themes_are_rejected_before_the_library() {
        let config = tempfile::tempdir().unwrap();
        let store = ThemeLibraryStore::new(config.path().join("themes"));
        for (result, needle) in [
            ("'Ocean'", "object"),
            ("{ base = 'No Such Theme', name = 'Ocean' }", "unknown base"),
            ("{ name = 'Ocean', terminal = { background = 'blue' } }", "background"),
            ("{ schema_version = 1, name = 'Ocean' }", "missing"),
        ] {
            let source = tempfile::tempdir().unwrap();
            write_package(
                source.path(),
                MANIFEST,
                &format!("return {{ generate = function() return {result} end }}"),
            );
            let package = Package::open(source.path()).unwrap();
            let error = themes::finish(
                run(&package),
                &package,
                "generate",
                Some(&[Grant::ThemeLibrary]),
                &store,
            )
            .unwrap_err();
            assert_eq!(error.code, "invalid_theme_output", "{result}");
            assert!(error.message.contains(needle), "{result}: {}", error.message);
        }
        assert!(store.list().unwrap().custom.is_empty());
    }

    #[test]
    fn generate_runs_only_enabled_granted_theme_commands() {
        let (source, config, library) = fixture();
        let store = ThemeLibraryStore::new(config.path().join("themes"));
        library.install(source.path(), &[Grant::ThemeLibrary]).unwrap();
        let document = themes::generate(&library, "example.ocean", "generate", &store).unwrap();
        assert_eq!(document.name(), "Ocean");
        assert_eq!(
            themes::generate(&library, "example.ocean", "missing", &store).unwrap_err().code,
            "command_not_found"
        );
        library.set_enabled("example.ocean", false).unwrap();
        assert_eq!(
            themes::generate(&library, "example.ocean", "generate", &store).unwrap_err().code,
            "plugin_disabled"
        );
    }
}
