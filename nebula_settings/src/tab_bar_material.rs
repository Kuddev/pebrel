//! File-only material overrides shared by sidebar and top tab layouts.
use crate::{BlurModeName, RawSettings};

/// Missing, `inherit`, and invalid values preserve the existing window material.
/// Native backdrop capabilities are resolved by the application, not this model.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TabBarMaterial {
    pub opacity: Option<f32>,
    pub blur: Option<BlurModeName>,
}

impl TabBarMaterial {
    pub fn from_raw(raw: &RawSettings) -> Self {
        Self {
            opacity: raw
                .f32("tab_bar_opacity")
                .filter(|value| value.is_finite())
                .map(|value| value.clamp(0.0, 1.0)),
            blur: raw.value("tab_bar_blur").and_then(BlurModeName::from_settings),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RuntimeSettings, apply_updates};

    #[test]
    fn old_and_invalid_configs_inherit_without_affecting_the_window() {
        for value in ["", "inherit", "invalid", "NaN", "inf", "-inf"] {
            let text =
                format!("opacity=0.8\nblur=mica\ntab_bar_opacity={value}\ntab_bar_blur={value}\n");
            let runtime = RuntimeSettings::from_raw(&RawSettings::from_text(&text));
            assert_eq!(runtime.tab_bar_material, TabBarMaterial::default(), "{value}");
            assert_eq!(runtime.opacity, 0.8);
            assert_eq!(runtime.blur, BlurModeName::Mica);
        }
    }

    #[test]
    fn opacity_is_bounded_and_explicit_none_is_distinct_from_inherit() {
        for (value, expected) in [("-1", 0.0), ("0", 0.0), ("0.65", 0.65), ("2", 1.0)] {
            let raw = RawSettings::from_text(&format!(
                "TAB_BAR_OPACITY = {value}\nTAB_BAR_BLUR = none\n"
            ));
            assert_eq!(
                TabBarMaterial::from_raw(&raw),
                TabBarMaterial { opacity: Some(expected), blur: Some(BlurModeName::None) }
            );
        }
    }

    #[test]
    fn both_tab_positions_read_the_same_round_tripped_keys() {
        for position in ["sidebar", "top"] {
            for mode in ["none", "mica", "mica-alt", "aero", "acrylic"] {
                let text = apply_updates(
                    "opacity=1\nblur=none\nfuture_setting=keep\n",
                    &[
                        ("tabs_position", position.into()),
                        ("tab_bar_opacity", "0.65".into()),
                        ("tab_bar_blur", mode.into()),
                    ],
                );
                let runtime = RuntimeSettings::from_raw(&RawSettings::from_text(&text));
                assert_eq!(runtime.tab_bar_material.opacity, Some(0.65));
                assert_eq!(runtime.tab_bar_material.blur, BlurModeName::from_settings(mode));
                assert_eq!(runtime.opacity, 1.0);
                assert_eq!(runtime.blur, BlurModeName::None);
                assert!(text.contains("future_setting=keep"));
            }
        }
    }
}
