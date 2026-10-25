//! Windows 流式纹理与原生下衬视图共享壁纸合同；系统 API 选择归平台层。
#[cfg(all(windows, feature = "video-background"))]
#[path = "animated/native.rs"]
mod implementation;
#[cfg(not(all(windows, feature = "video-background")))]
#[path = "animated/av_player.rs"]
mod implementation;
pub(super) use implementation::*;
