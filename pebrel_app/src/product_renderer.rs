//! 正式 GPUI 产品仍会复用少量与 OpenGL 无关的显示值类型。
//!
//! 这里刻意不暴露旧 `Renderer`、GL shader 或 crossfont 栅格器；这些类型
//! 只是配置和布局合同，供 GPUI 与 `legacy-shell` 各自的绘制后端消费。

#[path = "renderer/image_layout.rs"]
pub mod image;

pub mod ui {
    /// Straight-alpha color shared by configuration and GPUI adapters.
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub struct Rgba {
        pub r: u8,
        pub g: u8,
        pub b: u8,
        pub a: u8,
    }

    impl Rgba {
        pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
            Self { r, g, b, a }
        }

        pub fn with_alpha(self, alpha: f32) -> Self {
            Self { a: (alpha.clamp(0.0, 1.0) * 255.0) as u8, ..self }
        }
    }
}
