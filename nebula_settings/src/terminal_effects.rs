use crate::RawSettings;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EffectAnimation {
    Off,
    #[default]
    Focused,
    Always,
}

impl EffectAnimation {
    pub const VALUES: &'static [&'static str] = &["off", "focused", "always"];

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "off" => Some(Self::Off),
            "focused" => Some(Self::Focused),
            "always" => Some(Self::Always),
            _ => None,
        }
    }

    pub fn settings_value(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Focused => "focused",
            Self::Always => "always",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerminalEffects {
    pub enabled: bool,
    pub paths: Vec<String>,
    pub animation: EffectAnimation,
}

impl TerminalEffects {
    pub const PATH_KEYS: [&'static str; 8] = [
        "terminal_effect_path",
        "terminal_effect_path_2",
        "terminal_effect_path_3",
        "terminal_effect_path_4",
        "terminal_effect_path_5",
        "terminal_effect_path_6",
        "terminal_effect_path_7",
        "terminal_effect_path_8",
    ];

    pub fn from_raw(raw: &RawSettings) -> Self {
        Self {
            enabled: raw.bool_on("terminal_effect_enabled").unwrap_or(false),
            paths: Self::PATH_KEYS
                .iter()
                .filter_map(|key| raw.value(key).map(str::to_owned))
                .collect(),
            animation: raw
                .value("terminal_effect_animation")
                .and_then(EffectAnimation::parse)
                .unwrap_or_default(),
        }
    }

    pub fn source_updates(paths: &[String]) -> Result<Vec<(&'static str, String)>, &'static str> {
        if paths.len() > Self::PATH_KEYS.len() {
            return Err("at most eight effect sources are supported");
        }
        if paths.iter().any(|path| {
            path.is_empty() || path.trim() != path || path.chars().any(char::is_control)
        }) {
            return Err("effect paths must fit the line-based settings format");
        }
        // 一次写全八个槽位，移除/换序后不会遗留旧文件；首槽保留原来的单文件键。
        Ok(Self::PATH_KEYS
            .iter()
            .enumerate()
            .map(|(index, key)| (*key, paths.get(index).cloned().unwrap_or_default()))
            .collect())
    }

    pub fn request(&self) -> Result<Option<&[String]>, &'static str> {
        if !self.enabled {
            return Ok(None);
        }
        if self.paths.is_empty() {
            return Err("terminal effect is enabled without a WGSL source");
        }
        Self::source_updates(&self.paths)?;
        Ok(Some(&self.paths))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_activation_is_independent_of_background_and_animation() {
        let text = "terminal_effect_path=效果.wgsl\nterminal_effect_animation=always\nbackground_effect_neon_vortex=true\n";
        let raw = RawSettings::from_text(text);
        assert_eq!(TerminalEffects::from_raw(&raw).request(), Ok(None));
        let updated = crate::apply_updates(text, &[("terminal_effect_enabled", "true".into())]);
        let settings = crate::RuntimeSettings::from_raw(&RawSettings::from_text(&updated));
        assert_eq!(settings.terminal_effects.request().unwrap().unwrap(), ["效果.wgsl"]);
        assert_eq!(settings.terminal_effects.animation, EffectAnimation::Always);
        assert!(settings.background_effects.neon_vortex);
    }

    #[test]
    fn defaults_and_invalid_values_do_not_authorize_execution() {
        assert_eq!(TerminalEffects::default().animation, EffectAnimation::Focused);
        assert!(
            TerminalEffects::from_raw(&RawSettings::from_text("terminal_effect_enabled=true\n"))
                .request()
                .is_err()
        );
        let settings = TerminalEffects::from_raw(&RawSettings::from_text(
            "terminal_effect_enabled=trueish\nterminal_effect_animation=unknown\n",
        ));
        assert_eq!(settings, TerminalEffects::default());
        for value in EffectAnimation::VALUES {
            assert_eq!(EffectAnimation::parse(value).unwrap().settings_value(), *value);
        }
    }

    #[test]
    fn ordered_sources_round_trip_without_stale_slots_or_implicit_activation() {
        let original = "terminal_effect_path=first.wgsl\nterminal_effect_path_2=second.wgsl\nterminal_effect_path_8=last.wgsl\nbackground_media_kind=video\nunknown=keep\n";
        let mut sources = TerminalEffects::from_raw(&RawSettings::from_text(original)).paths;
        assert_eq!(sources, ["first.wgsl", "second.wgsl", "last.wgsl"]);
        sources.swap(0, 2);
        sources.remove(1);
        let updated =
            crate::apply_updates(original, &TerminalEffects::source_updates(&sources).unwrap());
        let restored = TerminalEffects::from_raw(&RawSettings::from_text(&updated));
        assert_eq!(restored.paths, ["last.wgsl", "first.wgsl"]);
        assert!(!restored.enabled);
        assert_eq!(restored.request(), Ok(None));
        assert!(updated.contains("background_media_kind=video"));
        assert!(updated.contains("unknown=keep"));
        assert!(TerminalEffects::source_updates(&vec!["a.wgsl".into(); 9]).is_err());
        assert!(
            TerminalEffects::source_updates(&["a.wgsl\nterminal_effect_enabled=true".into()])
                .is_err()
        );
        let cleared =
            crate::apply_updates(&updated, &TerminalEffects::source_updates(&[]).unwrap());
        assert!(TerminalEffects::from_raw(&RawSettings::from_text(&cleared)).paths.is_empty());
    }
}
