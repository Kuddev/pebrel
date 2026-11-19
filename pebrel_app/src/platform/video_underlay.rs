//! 原生视频图层的系统边界；不读取设置，不拥有 UI 计时器和提示。
use crate::renderer::image::{BackgroundImageAlignment, BackgroundImageFit};
use gpui::{Bounds, Pixels};

pub(crate) struct Layout {
    pub bounds: Bounds<Pixels>,
    pub anchor: Bounds<Pixels>,
    pub radius: Pixels,
    pub opacity: f32,
    pub fit: BackgroundImageFit,
    pub alignment: BackgroundImageAlignment,
}

#[cfg(all(target_os = "macos", feature = "video-background"))]
mod macos;
#[cfg(all(target_os = "macos", feature = "video-background"))]
pub(crate) use macos::NativeVideo;

#[cfg(not(all(target_os = "macos", feature = "video-background")))]
mod unavailable {
    pub(crate) struct NativeVideo;
    impl NativeVideo {
        pub(crate) fn open(_: std::path::PathBuf) -> anyhow::Result<Self> {
            anyhow::bail!("Native video underlay is unavailable")
        }
        pub(crate) fn available() -> bool {
            false
        }
        pub(crate) fn ready(&self) -> bool {
            false
        }
        pub(crate) fn failed(&self) -> bool {
            true
        }
        pub(crate) fn fail(&mut self) {}
        pub(crate) fn tick(
            &mut self,
            _: &[gpui::WindowId],
            _: bool,
            _: bool,
        ) -> anyhow::Result<bool> {
            Ok(false)
        }
        pub(crate) fn paint(
            &mut self,
            _: &super::Layout,
            _: &mut gpui::Window,
        ) -> anyhow::Result<()> {
            anyhow::bail!("Native video underlay is unavailable")
        }
    }
}
#[cfg(not(all(target_os = "macos", feature = "video-background")))]
pub(crate) use unavailable::NativeVideo;
