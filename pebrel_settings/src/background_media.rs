/// Explicit background source kind. Paths never select a decoder by suffix.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackgroundMediaKind {
    #[default]
    Image,
    Video,
    Gif,
}

impl BackgroundMediaKind {
    pub const VALUES: &'static [&'static str] = &["image", "video", "gif"];

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "image" => Some(Self::Image),
            "video" => Some(Self::Video),
            "gif" => Some(Self::Gif),
            _ => None,
        }
    }

    pub fn settings_value(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Gif => "gif",
        }
    }

    pub fn is_animated(self) -> bool {
        matches!(self, Self::Video | Self::Gif)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_are_static_and_source_kind_is_explicit() {
        assert_eq!(BackgroundMediaKind::default(), BackgroundMediaKind::Image);
        assert_eq!(BackgroundMediaKind::parse("VIDEO"), Some(BackgroundMediaKind::Video));
        assert_eq!(BackgroundMediaKind::parse("movie.mp4"), None);
        for kind in
            [BackgroundMediaKind::Image, BackgroundMediaKind::Video, BackgroundMediaKind::Gif]
        {
            assert_eq!(BackgroundMediaKind::parse(kind.settings_value()), Some(kind));
        }
        assert!(!BackgroundMediaKind::Image.is_animated());
        assert!(BackgroundMediaKind::Gif.is_animated());
    }

    #[test]
    fn runtime_roundtrip_preserves_old_image_preferences() {
        use crate::{RawSettings, RuntimeSettings};
        let old =
            RuntimeSettings::from_raw(&RawSettings::from_text("background_image=poster.png\n"));
        assert_eq!(old.background_media_kind, BackgroundMediaKind::Image);
        assert_eq!(old.background_image.as_deref(), Some("poster.png"));
        let text = "background_media_kind=video\nbackground_image=scene.mp4\nfuture_key=keep\n";
        let video = RuntimeSettings::from_raw(&RawSettings::from_text(text));
        assert_eq!(video.background_media_kind, BackgroundMediaKind::Video);
        assert_eq!(video.background_image.as_deref(), Some("scene.mp4"));
        let updated = crate::apply_updates(
            text,
            &[
                ("background_media_kind", "image".to_owned()),
                ("background_image", "poster.png".to_owned()),
            ],
        );
        assert!(updated.contains("future_key=keep"));
        let restored = RuntimeSettings::from_raw(&RawSettings::from_text(&updated));
        assert_eq!(restored.background_media_kind, BackgroundMediaKind::Image);
        assert_eq!(restored.background_image.as_deref(), Some("poster.png"));
    }

    #[test]
    fn legacy_theme_paths_and_colors_only_themes_keep_distinct_source_rules() {
        let mut effects = crate::ThemeEffects::default();
        assert_eq!(effects.background_kind_override(), None);
        effects.background_image = Some("poster.png".to_owned());
        assert_eq!(effects.background_kind_override(), Some(BackgroundMediaKind::Image));
        effects.background_media_kind = Some(BackgroundMediaKind::Video);
        assert_eq!(effects.background_kind_override(), Some(BackgroundMediaKind::Video));
        effects.background_image = None;
        assert_eq!(effects.background_kind_override(), Some(BackgroundMediaKind::Video));
    }
}
