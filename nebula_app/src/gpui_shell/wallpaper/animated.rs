//! 动画后端的选择只发生在此处；共享壁纸入口不拥有平台专用字段。
#[cfg(all(windows, feature = "video-background"))]
#[path = "animated/native.rs"]
mod implementation;
#[cfg(all(target_os = "macos", feature = "video-background"))]
#[path = "animated/av_player.rs"]
mod implementation;
#[cfg(not(all(any(windows, target_os = "macos"), feature = "video-background")))]
#[path = "animated/unavailable.rs"]
mod implementation;

pub(super) use implementation::*;
