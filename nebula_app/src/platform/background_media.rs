//! 非 UI 的解码器和拥有线程；播放时钟、可见性和 GPUI entity 留在调用方。
#[cfg(feature = "gif-background")]
mod gif_decoder;
mod video;
pub(crate) use video::{Cursor, Frame};
