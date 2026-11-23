//! Terminal BEL (`^G`) notification mode preference.

/// 终端 BEL（`^G`）的通知方式：关 / 闪烁 / 声音 / 两者。
/// 缺省 `Both`：旧壳给 AI CLI 回合结束的可听提示默认是开的。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum BellModeName {
    None,
    Visual,
    Audible,
    #[default]
    Both,
}

impl BellModeName {
    pub fn from_settings(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" | "off" => Some(Self::None),
            "visual" => Some(Self::Visual),
            "audible" => Some(Self::Audible),
            "both" => Some(Self::Both),
            _ => None,
        }
    }

    pub fn settings_value(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Visual => "visual",
            Self::Audible => "audible",
            Self::Both => "both",
        }
    }

    pub fn visual(self) -> bool {
        matches!(self, Self::Visual | Self::Both)
    }

    pub fn audible(self) -> bool {
        matches!(self, Self::Audible | Self::Both)
    }
}
