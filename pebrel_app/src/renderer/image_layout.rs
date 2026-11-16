//! Shared wallpaper sizing and alignment, independent of either renderer.

/// Wallpaper sizing modes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundImageFit {
    /// Distort the image to exactly match the window.
    Fill,
    /// Preserve aspect ratio and keep the entire image visible.
    Uniform,
    /// Preserve aspect ratio and crop the overflow (CSS `cover`).
    #[default]
    UniformToFill,
    /// Draw at the image's native pixel size.
    None,
}

impl BackgroundImageFit {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "fill" | "stretch" => Some(Self::Fill),
            "uniform" | "contain" => Some(Self::Uniform),
            "uniform_to_fill" | "uniformtofill" | "cover" => Some(Self::UniformToFill),
            "none" | "native" => Some(Self::None),
            _ => None,
        }
    }

    pub const fn settings_value(self) -> &'static str {
        match self {
            Self::Fill => "fill",
            Self::Uniform => "uniform",
            Self::UniformToFill => "uniform_to_fill",
            Self::None => "none",
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::Fill => Self::Uniform,
            Self::Uniform => Self::UniformToFill,
            Self::UniformToFill => Self::None,
            Self::None => Self::Fill,
        }
    }
}

/// Anchor used when the fitted wallpaper is larger or smaller than the window.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundImageAlignment {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl BackgroundImageAlignment {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "top-left" => Some(Self::TopLeft),
            "top" => Some(Self::Top),
            "top-right" => Some(Self::TopRight),
            "left" => Some(Self::Left),
            "center" | "centre" => Some(Self::Center),
            "right" => Some(Self::Right),
            "bottom-left" => Some(Self::BottomLeft),
            "bottom" => Some(Self::Bottom),
            "bottom-right" => Some(Self::BottomRight),
            _ => None,
        }
    }

    pub const fn settings_value(self) -> &'static str {
        match self {
            Self::TopLeft => "top_left",
            Self::Top => "top",
            Self::TopRight => "top_right",
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
            Self::BottomLeft => "bottom_left",
            Self::Bottom => "bottom",
            Self::BottomRight => "bottom_right",
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::TopLeft => Self::Top,
            Self::Top => Self::TopRight,
            Self::TopRight => Self::Left,
            Self::Left => Self::Center,
            Self::Center => Self::Right,
            Self::Right => Self::BottomLeft,
            Self::BottomLeft => Self::Bottom,
            Self::Bottom => Self::BottomRight,
            Self::BottomRight => Self::TopLeft,
        }
    }

    /// 对齐锚点系数（0=贴起始边，0.5=居中，1=贴结束边）。`pub`：GPUI 壳
    /// 的壁纸绘制复用同一套布局数学（gpui_shell::wallpaper）。
    pub const fn factors(self) -> (f32, f32) {
        match self {
            Self::TopLeft => (0.0, 0.0),
            Self::Top => (0.5, 0.0),
            Self::TopRight => (1.0, 0.0),
            Self::Left => (0.0, 0.5),
            Self::Center => (0.5, 0.5),
            Self::Right => (1.0, 0.5),
            Self::BottomLeft => (0.0, 1.0),
            Self::Bottom => (0.5, 1.0),
            Self::BottomRight => (1.0, 1.0),
        }
    }
}

pub(crate) fn wallpaper_rect(
    window_w: f32,
    window_h: f32,
    image_w: f32,
    image_h: f32,
    fit: BackgroundImageFit,
    alignment: BackgroundImageAlignment,
) -> (f32, f32, f32, f32) {
    let window_w = window_w.max(1.0);
    let window_h = window_h.max(1.0);
    let image_w = image_w.max(1.0);
    let image_h = image_h.max(1.0);

    let (draw_w, draw_h) = match fit {
        BackgroundImageFit::Fill => (window_w, window_h),
        BackgroundImageFit::Uniform => {
            let scale = (window_w / image_w).min(window_h / image_h);
            (image_w * scale, image_h * scale)
        },
        BackgroundImageFit::UniformToFill => {
            let scale = (window_w / image_w).max(window_h / image_h);
            (image_w * scale, image_h * scale)
        },
        BackgroundImageFit::None => (image_w, image_h),
    };
    let (align_x, align_y) = alignment.factors();
    let x0 = (window_w - draw_w) * align_x;
    let y0 = (window_h - draw_h) * align_y;
    (x0, y0, draw_w, draw_h)
}
