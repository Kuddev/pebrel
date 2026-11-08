//! Script-facing desktop settings; value semantics stay in nebula_settings.

use std::collections::BTreeMap;
use std::path::Path;

use nebula_config_derive::SerdeReplace;
use nebula_settings::{RawSettings, UiValueKind, ui_value_kind};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

#[derive(Serialize, SerdeReplace, Default, Clone, Debug, PartialEq)]
#[serde(transparent)]
pub struct DesktopUi(BTreeMap<String, toml::Value>);

impl<'de> Deserialize<'de> for DesktopUi {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = BTreeMap::<String, toml::Value>::deserialize(deserializer)?;
        for (key, value) in &values {
            let kind = ui_value_kind(key)
                .ok_or_else(|| D::Error::custom(format!("unknown UI setting: ui.{key}")))?;
            let valid = match kind {
                UiValueKind::Boolean => value.is_bool(),
                UiValueKind::Number => {
                    value.is_integer() || value.as_float().is_some_and(f64::is_finite)
                },
                UiValueKind::String => {
                    value.as_str().is_some_and(|text| !text.chars().any(char::is_control))
                },
            };
            if !valid {
                return Err(D::Error::custom(format!("ui.{key} requires a {kind:?} value")));
            }
        }
        if values.contains_key("sidebar_width") && values.contains_key("sidebar_w") {
            return Err(D::Error::custom(
                "use only ui.sidebar_width, not both sidebar width aliases",
            ));
        }
        Ok(Self(values))
    }
}

impl DesktopUi {
    pub(crate) fn values(&self, config_dir: &Path) -> Vec<(String, String)> {
        self.0
            .iter()
            .map(|(key, value)| {
                let value = match value {
                    toml::Value::String(value)
                        if key == "background_image" && !value.is_empty() =>
                    {
                        config_dir.join(value).to_string_lossy().into_owned()
                    },
                    toml::Value::String(value) => value.clone(),
                    // Some older preferences (ghost) use 0/1 rather than textual booleans.
                    toml::Value::Boolean(value) => u8::from(*value).to_string(),
                    value => value.to_string(),
                };
                let key = if key == "sidebar_width" { "sidebar_w" } else { key.as_str() };
                (key.to_owned(), value)
            })
            .collect()
    }
}

pub(crate) fn defaults(
    config: &super::UiConfig,
    config_dir: &Path,
) -> Result<RawSettings, &'static str> {
    let mut values = config.ui.values(config_dir);
    values.extend(config.shaders.values(config_dir)?);
    Ok(RawSettings::from_values(values))
}
