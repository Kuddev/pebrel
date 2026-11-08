//! Script shader options adapt to the shared settings and authorization contracts.

use std::path::Path;

use nebula_config_derive::SerdeReplace;
use nebula_settings::{BackgroundEffects, EffectAnimation, ShaderTiming, TerminalEffects};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

#[derive(Serialize, SerdeReplace, Default, Clone, Debug, PartialEq)]
#[serde(transparent)]
pub struct Shaders(Tables);

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
struct Tables {
    background: Option<Background>,
    terminal: Option<Terminal>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
struct Background {
    enabled: bool,
    preset: Option<String>,
    path: Option<String>,
    speed: Option<f32>,
    frame_rate: Option<u16>,
    parameter: Option<f32>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
struct Terminal {
    enabled: bool,
    paths: Vec<String>,
    animation: Option<String>,
    speed: Option<f32>,
    frame_rate: Option<u16>,
    parameters: Option<[f32; 4]>,
}

impl<'de> Deserialize<'de> for Shaders {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let tables = Tables::deserialize(deserializer)?;
        if let Some(background) = &tables.background {
            timing(background.speed, background.frame_rate).map_err(D::Error::custom)?;
            if background.parameter.is_some_and(|value| !value.is_finite()) {
                return Err(D::Error::custom("background shader parameter must be finite"));
            }
            if background.path.is_some() && background.preset.is_some() {
                return Err(D::Error::custom("choose a background shader path or preset"));
            }
            if let Some(preset) = &background.preset {
                if preset == "wgsl" || BackgroundEffects::selection_updates(preset).is_none() {
                    return Err(D::Error::custom("unknown background shader preset"));
                }
            }
            if let Some(path) = &background.path {
                TerminalEffects::source_updates(std::slice::from_ref(path))
                    .map_err(D::Error::custom)?;
            }
            if background.enabled
                && background.path.is_none()
                && background.preset.as_deref().is_none_or(|value| value == "off")
            {
                return Err(D::Error::custom("enabled background shader needs a path or preset"));
            }
        }
        if let Some(terminal) = &tables.terminal {
            timing(terminal.speed, terminal.frame_rate).map_err(D::Error::custom)?;
            TerminalEffects::source_updates(&terminal.paths).map_err(D::Error::custom)?;
            if terminal.enabled && terminal.paths.is_empty() {
                return Err(D::Error::custom("enabled terminal shader needs source paths"));
            }
            if terminal
                .animation
                .as_deref()
                .is_some_and(|value| EffectAnimation::parse(value).is_none())
            {
                return Err(D::Error::custom("shader animation must be off, focused or always"));
            }
            if terminal.parameters.is_some_and(|values| values.iter().any(|v| !v.is_finite())) {
                return Err(D::Error::custom("terminal shader parameters must be finite"));
            }
        }
        Ok(Self(tables))
    }
}

fn timing(speed: Option<f32>, frame_rate: Option<u16>) -> Result<(), &'static str> {
    let defaults = ShaderTiming::default();
    ShaderTiming {
        speed: speed.unwrap_or(defaults.speed),
        frame_rate: frame_rate.unwrap_or(defaults.frame_rate),
    }
    .validate()
}

fn timing_values(
    values: &mut Vec<(String, String)>,
    prefix: &str,
    speed: Option<f32>,
    frame_rate: Option<u16>,
) {
    if let Some(speed) = speed {
        values.push((format!("{prefix}_speed"), speed.to_string()));
    }
    if let Some(frame_rate) = frame_rate {
        values.push((format!("{prefix}_frame_rate"), frame_rate.to_string()));
    }
}

impl Shaders {
    pub(crate) fn values(&self, config_dir: &Path) -> Result<Vec<(String, String)>, &'static str> {
        let mut values = Vec::new();
        if let Some(background) = &self.0.background {
            let preset = if !background.enabled {
                "off"
            } else if background.path.is_some() {
                "wgsl"
            } else {
                background.preset.as_deref().unwrap_or("off")
            };
            values.extend(
                BackgroundEffects::selection_updates(preset)
                    .ok_or("unknown background shader preset")?
                    .into_iter()
                    .map(|(key, value)| (key.to_owned(), value)),
            );
            if let Some(path) = &background.path {
                let path = config_dir.join(path).to_string_lossy().into_owned();
                TerminalEffects::source_updates(std::slice::from_ref(&path))?;
                // Keep the existing persisted slot; retired GLSL keys stay inert.
                values.push(("background_wgsl_path".into(), path));
            }
            timing_values(
                &mut values,
                "background_shader",
                background.speed,
                background.frame_rate,
            );
            if let Some(parameter) = background.parameter {
                values.push(("background_shader_parameter".into(), parameter.to_string()));
            }
        }
        if let Some(terminal) = &self.0.terminal {
            values.push(("terminal_effect_enabled".into(), terminal.enabled.to_string()));
            let paths = terminal
                .paths
                .iter()
                .map(|path| config_dir.join(path).to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            values.extend(
                TerminalEffects::source_updates(&paths)?
                    .into_iter()
                    .map(|(key, value)| (key.to_owned(), value)),
            );
            if let Some(animation) = &terminal.animation {
                values.push(("terminal_effect_animation".into(), animation.clone()));
            }
            timing_values(&mut values, "terminal_effect", terminal.speed, terminal.frame_rate);
            if let Some(parameters) = terminal.parameters {
                values.extend(
                    TerminalEffects::PARAMETER_KEYS
                        .into_iter()
                        .zip(parameters)
                        .map(|(key, value)| (key.to_owned(), value.to_string())),
                );
            }
        }
        Ok(values)
    }
}
