mod format;
mod locale;

macro_rules! t {
    ($key:literal $(,)?) => {
        $crate::i18n::UiLanguage::current().tr($key)
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {{
        let language = $crate::i18n::UiLanguage::current();
        let args = [$(
            (stringify!($name), format!("{}", $value)),
        )+];
        let refs = args.iter().map(|(name, value)| (*name, value.as_str())).collect::<Vec<_>>();
        language.tr_args($key, &refs)
    }};
}

pub(crate) use t;

pub use locale::system_locale;

include!(concat!(env!("OUT_DIR"), "/translations.rs"));

use std::sync::atomic::{AtomicUsize, Ordering};

/// Sentinel so an unset cache cannot collapse to `UiLanguage::ZhCn` (index 0).
const UNSET: usize = usize::MAX;
static CURRENT_LANGUAGE: AtomicUsize = AtomicUsize::new(UNSET);

impl LanguagePreference {
    pub fn parse(value: &str) -> Option<Self> {
        nebula_settings::LanguagePref::from_settings(value).map(Self::from)
    }

    pub const fn as_str(self) -> &'static str {
        self.shared().settings_value()
    }

    pub fn resolved(self) -> UiLanguage {
        let language =
            self.explicit().unwrap_or_else(|| UiLanguage::for_locale(system_locale().as_deref()));
        language.activate();
        language
    }
}

impl UiLanguage {
    /// Process-wide UI language for toasts, tray menus, and other threads
    /// that cannot read GPUI application state.
    pub fn current() -> Self {
        Self::ALL.get(CURRENT_LANGUAGE.load(Ordering::Relaxed)).copied().unwrap_or(Self::EnUs)
    }

    /// Pin the process-wide UI language after settings resolve.
    pub fn activate(self) {
        CURRENT_LANGUAGE.store(self as usize, Ordering::Relaxed);
    }

    pub fn for_locale(locale: Option<&str>) -> Self {
        locale
            .and_then(nebula_settings::LanguagePref::from_locale)
            .and_then(|preference| LanguagePreference::from(preference).explicit())
            .unwrap_or(Self::EnUs)
    }

    pub const fn text(self, message: Message) -> &'static str {
        MESSAGES[self as usize][message as usize]
    }

    pub fn pick<'text>(self, zh_cn: &'text str, en_us: &'text str) -> &'text str {
        match self {
            Self::ZhCn => zh_cn,
            Self::EnUs => en_us,
            _ => source_id(en_us).map(|message| self.text(message)).unwrap_or(en_us),
        }
    }

    pub fn tr(self, key: &'static str) -> &'static str {
        message_id(key).map(|message| self.text(message)).unwrap_or(key)
    }

    pub fn format(self, message: Message, args: &[(&str, &str)]) -> String {
        format::substitute(self.text(message), args)
    }

    pub fn tr_args(self, key: &'static str, args: &[(&str, &str)]) -> String {
        format::substitute(self.tr(key), args)
    }
}

#[cfg(test)]
mod tests;
