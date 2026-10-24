//! 持久化的普通工作区窗口几何：五个 flat 键（`window_x`/`window_y`/
//! `window_width`/`window_height` + `window_maximized`），跟随
//! `quick_terminal_width/height` 的既有写盘先例。

/// 普通窗口的上次布局。坐标是逻辑像素，与启动路径的 `Bounds::centered`
/// 同一坐标系；`maximized` 时的 x/y/宽高是还原框，恢复时交给平台的
/// 最大化窗口语义。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

impl WindowGeometry {
    /// 与启动路径的 `window_min_size` 一致的下限；上限与
    /// [`crate::QuickTerminalSize`] 相同，拦截异常写入。
    pub const MIN_WIDTH: f32 = 760.0;
    pub const MIN_HEIGHT: f32 = 540.0;
    pub const MAX_EDGE: f32 = 16384.0;

    #[inline]
    pub fn new(x: f32, y: f32, width: f32, height: f32, maximized: bool) -> Option<Self> {
        (x.is_finite()
            && y.is_finite()
            && width.is_finite()
            && height.is_finite()
            && width >= Self::MIN_WIDTH
            && height >= Self::MIN_HEIGHT
            && width <= Self::MAX_EDGE
            && height <= Self::MAX_EDGE)
            .then_some(Self { x, y, width, height, maximized })
    }

    /// 写盘键值；与 `RawSettings` 的读侧互逆。
    pub fn persisted_values(self) -> [(&'static str, String); 5] {
        [
            ("window_x", self.x.to_string()),
            ("window_y", self.y.to_string()),
            ("window_width", self.width.to_string()),
            ("window_height", self.height.to_string()),
            ("window_maximized", (self.maximized as u8).to_string()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RawSettings, RuntimeSettings};

    #[test]
    fn window_geometry_round_trips_and_invalid_dimensions_fall_back() {
        let raw = RawSettings::from_text(
            "window_x=120.5\nwindow_y=64\nwindow_width=1280\nwindow_height=800\nwindow_maximized=1\n",
        );
        let settings = RuntimeSettings::from_raw(&raw);
        assert_eq!(
            settings.window_geometry,
            Some(WindowGeometry {
                x: 120.5,
                y: 64.0,
                width: 1280.0,
                height: 800.0,
                maximized: true
            })
        );

        let empty = RuntimeSettings::from_raw(&RawSettings::from_text("theme=Nord\n"));
        assert_eq!(empty.window_geometry, None);

        // 任一边缘非法即整组忽略，避免恢复出半个几何。
        let broken = RawSettings::from_text(
            "window_x=10\nwindow_y=10\nwindow_width=100\nwindow_height=800\n",
        );
        assert_eq!(RuntimeSettings::from_raw(&broken).window_geometry, None);
    }

    #[test]
    fn persisted_values_round_trip_through_the_text_format() {
        let geometry = WindowGeometry::new(8.0, 40.0, 1024.0, 640.0, false).unwrap();
        let values = geometry.persisted_values();
        let text = values
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            RuntimeSettings::from_raw(&RawSettings::from_text(&format!("{text}\n")))
                .window_geometry,
            Some(geometry)
        );
    }
}
