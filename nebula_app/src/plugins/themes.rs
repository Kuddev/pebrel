//! 插件对主题库的贡献：自带主题包和生成命令的结果都由宿主校验后加入主题库，从不应用设置。

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::theme_library::package::{CheckedPackage, Origin};
use crate::theme_library::{ThemeDocument, ThemeLibraryStore, builtin_document, builtin_documents};

use super::install::Library;
use super::package::{Grant, Output, Package, package_file};
use super::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum BundledTheme {
    Added {
        package: String,
        theme_id: String,
        name: String,
    },
    /// The library already holds this exact package from this plugin version.
    Present {
        package: String,
        theme_id: String,
        name: String,
    },
    Failed {
        package: String,
        error: String,
    },
}

/// Validate every bundled package before an install becomes visible.
pub(super) fn check_bundled(stage: &Path, themes: &[String]) -> Result<()> {
    for theme in themes {
        CheckedPackage::open(&stage.join(theme))
            .map_err(|error| Error::new("invalid_theme_package", format!("{theme}: {error}")))?;
    }
    Ok(())
}

/// Add an installed plugin's bundled themes to the library. Reinstalling the
/// same version finds its earlier themes by receipt instead of duplicating them.
pub(crate) fn install_bundled(
    library: &Library,
    id: &str,
    store: &ThemeLibraryStore,
) -> Result<Vec<BundledTheme>> {
    let (package, grants) = library.open_enabled(id)?;
    if package.manifest.themes.is_empty() {
        return Ok(Vec::new());
    }
    require_grant(&grants)?;
    let existing = store.list().map_err(|error| Error::new("theme_library", error))?.custom;
    let mut results = Vec::new();
    for theme in &package.manifest.themes {
        let result = (|| {
            let path = package_file(&package.root, theme)?;
            let origin = Origin {
                catalog: source(&package),
                id: theme.clone(),
                version: package.manifest.version.clone(),
                sha256: sha256(&path)?,
            };
            if let Some(document) =
                existing.iter().find(|document| Origin::of(document).as_ref() == Some(&origin))
            {
                return Ok(BundledTheme::Present {
                    package: theme.clone(),
                    theme_id: document.id().unwrap_or_default().to_owned(),
                    name: document.name().to_owned(),
                });
            }
            let document = CheckedPackage::open(&path)
                .and_then(|checked| checked.install_from(store, Some(&origin)))
                .map_err(|error| Error::new("invalid_theme_package", error))?;
            Ok(BundledTheme::Added {
                package: theme.clone(),
                theme_id: document.id().unwrap_or_default().to_owned(),
                name: document.name().to_owned(),
            })
        })();
        results.push(result.unwrap_or_else(|error: Error| BundledTheme::Failed {
            package: theme.clone(),
            error: error.to_string(),
        }));
    }
    Ok(results)
}

/// Run an installed plugin's theme command and add the result to the library.
pub(crate) fn generate(
    library: &Library,
    id: &str,
    command: &str,
    store: &ThemeLibraryStore,
) -> Result<ThemeDocument> {
    let (package, grants) = library.open_enabled(id)?;
    require_grant(&grants)?;
    if !package
        .manifest
        .commands
        .iter()
        .any(|item| item.id == command && item.output == Output::Theme)
    {
        return Err(Error::new("command_not_found", format!("no theme command: {command}")));
    }
    let execution = super::execute(
        &package,
        command,
        json!({}),
        Duration::from_millis(3000),
        &mut super::runtime_call,
    )?;
    let document = document(execution.result, &package, command)?;
    store.import(&document, None).map_err(|error| Error::new("theme_library", error))
}

/// A theme command's JSON becomes a library document only after the host
/// validates it. With the grant the theme is added; without it (an uninstalled
/// package, or a run for checking) the validated document is only returned.
pub(super) fn finish(
    result: Value,
    package: &Package,
    command: &str,
    grants: Option<&[Grant]>,
    store: &ThemeLibraryStore,
) -> Result<Value> {
    let document = document(result, package, command)?;
    if grants.is_some_and(|grants| grants.contains(&Grant::ThemeLibrary)) {
        let stored =
            store.import(&document, None).map_err(|error| Error::new("theme_library", error))?;
        return Ok(json!({
            "added_to_library": true,
            "theme": { "id": stored.id(), "name": stored.name() },
        }));
    }
    Ok(json!({ "added_to_library": false, "theme": document.to_value() }))
}

/// Either a complete theme document, or `{ "base": "<built-in theme>", ...overrides }`.
fn document(result: Value, package: &Package, command: &str) -> Result<ThemeDocument> {
    let invalid =
        |error: crate::theme_library::DocumentError| Error::new("invalid_theme_output", error);
    let Value::Object(mut object) = result else {
        return Err(Error::new("invalid_theme_output", "theme commands return an object"));
    };
    let mut value = if object.contains_key("schema_version") {
        Value::Object(object)
    } else {
        let base = match object.remove("base") {
            None => None,
            Some(Value::String(base)) => Some(base),
            Some(_) => {
                return Err(Error::new("invalid_theme_output", "base must be a theme name"));
            },
        };
        let base = match base {
            None => builtin_document(nebula_settings::ThemeName::default()).ok(),
            Some(base) => builtin_documents()
                .into_iter()
                .find(|document| document.name() == base || document.id() == Some(base.as_str())),
        }
        .ok_or_else(|| Error::new("invalid_theme_output", "unknown base theme"))?;
        base.with_overrides(&object).map_err(invalid)?.to_value()
    };
    if !value.get("metadata").is_some_and(Value::is_object) {
        value["metadata"] = json!({});
    }
    value["metadata"]["generator"] = json!({
        "plugin": package.manifest.id,
        "version": package.manifest.version,
        "command": command,
    });
    ThemeDocument::from_value(value).map_err(invalid)
}

fn require_grant(grants: &[Grant]) -> Result<()> {
    if grants.contains(&Grant::ThemeLibrary) {
        return Ok(());
    }
    Err(Error::new("grant_required", "the plugin was not granted the theme library"))
}

/// Receipt source shared with catalog installs; plugins never claim a catalog URL.
fn source(package: &Package) -> String {
    format!("plugin:{}", package.manifest.id)
}

fn sha256(path: &Path) -> Result<String> {
    let io = |error: std::io::Error| Error::new("plugin_io", error);
    let mut file = File::open(path).map_err(io)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(io)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// CLI adapters over the user's own theme library.
pub(super) fn finish_for_user(
    result: Value,
    package: &Package,
    command: &str,
    grants: Option<&[Grant]>,
) -> Result<Value> {
    finish(result, package, command, grants, &ThemeLibraryStore::default())
}

pub(super) fn install_bundled_for_user(library: &Library, id: &str) -> Result<Value> {
    let themes = install_bundled(library, id, &ThemeLibraryStore::default())?;
    Ok(serde_json::to_value(themes).expect("theme results are JSON"))
}
