//! Read scripts once before the event loop; settings saves consume plain cached data.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::config::{UiConfig, source::ConfigFormat};

use super::StartupWindow;

struct Snapshot {
    raw: toml::Value,
    notice: Option<String>,
}

static SNAPSHOT: OnceLock<Snapshot> = OnceLock::new();

pub(crate) fn initialize(config_file: Option<PathBuf>) -> StartupWindow {
    let mut window = StartupWindow::default();
    let mut snapshot = Snapshot { raw: toml::Value::Table(Default::default()), notice: None };
    match read(config_file) {
        Ok(Some((config, raw, defaults))) => {
            window = StartupWindow::from_config(&config);
            if nebula_settings::install_config_defaults(defaults).is_err() {
                log::warn!("Desktop configuration was already initialized");
            }
            snapshot.raw = raw;
        },
        Ok(None) => {},
        Err(error) => {
            super::super::try_write_stderr(format_args!("[pebrel:gpui] {error}"));
            snapshot.notice = Some(error);
        },
    }
    let _ = SNAPSHOT.set(snapshot);
    window
}

/// None is only used by isolated callers that have not bootstrapped the shell.
pub(super) fn raw(notice: &mut Option<String>) -> Option<toml::Value> {
    let snapshot = SNAPSHOT.get()?;
    if notice.is_none() {
        *notice = snapshot.notice.clone();
    }
    Some(snapshot.raw.clone())
}

fn read(
    config_file: Option<PathBuf>,
) -> Result<Option<(UiConfig, toml::Value, nebula_settings::RawSettings)>, String> {
    let override_path = config_file.or_else(|| {
        ["PEBREL_GPUI_CONFIG", "NEBULA_GPUI_CONFIG"].into_iter().find_map(|name| {
            std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
        })
    });
    let Some(source) = crate::config::source::discover(override_path)? else { return Ok(None) };
    let loaded = crate::config::load_source(&source)
        .map_err(|error| format!("{}: {error}", source.primary_path.display()))?;
    let raw = if source.format == ConfigFormat::Toml {
        // Preserve sparse TOML colors/fonts and their established theme precedence.
        let mut notice = None;
        let raw = super::load_merged_toml(
            &source.primary_path,
            &mut notice,
            crate::display::UiLanguage::EnUs,
        );
        if let Some(error) = notice {
            return Err(error);
        }
        raw
    } else {
        presentation_values(&loaded.config).map_err(|error| error.to_string())?
    };
    let config_dir = loaded
        .config
        .config_paths
        .first()
        .and_then(|path| path.parent())
        .unwrap_or_else(|| Path::new("."));
    let config_dir = config_dir.canonicalize().unwrap_or_else(|_| config_dir.to_owned());
    let defaults =
        crate::config::desktop::defaults(&loaded.config, &config_dir).map_err(str::to_owned)?;
    Ok(Some((loaded.config, raw, defaults)))
}

fn presentation_values(config: &UiConfig) -> Result<toml::Value, toml::ser::Error> {
    let defaults = UiConfig::default();
    let mut table = toml::Table::new();
    // Unconfigured sections do not replace native defaults just because the
    // typed loader filled its own default fields.
    if config.font != defaults.font {
        table.insert("font".into(), toml::Value::try_from(&config.font)?);
    }
    if config.colors != defaults.colors {
        table.insert("colors".into(), toml::Value::try_from(&config.colors)?);
    }
    if config.mouse != defaults.mouse {
        table.insert("mouse".into(), toml::Value::try_from(&config.mouse)?);
    }
    Ok(toml::Value::Table(table))
}
