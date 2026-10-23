//! Measured widths shared by rendering, overflow navigation and drag ordering.

use std::sync::Arc;

use gpui::{SharedString, Window, px};

pub(super) const MIN_WIDTH: f32 = 80.0;
pub(super) const MAX_WIDTH: f32 = 220.0;
pub(super) const GAP: f32 = 4.0;
pub(super) const NUDGE_WIDTH: f32 = 22.0;

pub(super) fn gap(style: nebula_settings::TabStyle) -> f32 {
    if style == nebula_settings::TabStyle::Chrome { -10.0 } else { GAP }
}

#[derive(Clone, Debug)]
pub(in crate::gpui_shell::workspace) struct TabStripLayout {
    slots: Arc<[(f32, f32)]>,
    pub width: f32,
    pub viewport: f32,
    pub left: f32,
    pub overflow: bool,
    pub gap: f32,
}

impl TabStripLayout {
    pub(super) fn new(
        widths: impl IntoIterator<Item = f32>,
        capacity: f32,
        inset: f32,
        gap: f32,
    ) -> Self {
        let mut end = 0.0;
        let slots: Vec<_> = widths
            .into_iter()
            .map(|width| {
                let slot = (end, width.clamp(MIN_WIDTH, MAX_WIDTH));
                end += slot.1 + gap;
                slot
            })
            .collect();
        let width = if slots.is_empty() { 0.0 } else { end - gap };
        let overflow = width > capacity + 0.5;
        let viewport = if overflow { (capacity - 2.0 * NUDGE_WIDTH).max(MIN_WIDTH) } else { width };
        Self {
            slots: slots.into(),
            width,
            viewport,
            left: inset + if overflow { NUDGE_WIDTH } else { 0.0 },
            overflow,
            gap,
        }
    }

    pub(super) fn tab_width(&self, index: usize) -> f32 {
        self.slots[index].1
    }

    pub(in crate::gpui_shell::workspace) fn tab_count(&self) -> usize {
        self.slots.len()
    }

    fn center(&self, index: usize) -> f32 {
        let (left, width) = self.slots[index];
        left + width * 0.5
    }

    pub(in crate::gpui_shell::workspace) fn drag_limits(
        &self,
        source: usize,
        len: usize,
    ) -> (f32, f32) {
        let center = self.center(source);
        (
            -center + self.center(0),
            self.center(len.min(self.slots.len()).saturating_sub(1)) - center,
        )
    }

    pub(in crate::gpui_shell::workspace) fn drag_target(
        &self,
        source: usize,
        offset: f32,
        len: usize,
    ) -> usize {
        let center = self.center(source) + offset;
        (0..len.min(self.slots.len()))
            .min_by(|&a, &b| {
                (self.center(a) - center).abs().total_cmp(&(self.center(b) - center).abs())
            })
            .unwrap_or(source)
    }

    pub(super) fn shift(&self, index: usize, source: usize, target: usize) -> f32 {
        if source < target && index > source && index <= target {
            -(self.tab_width(source) + self.gap)
        } else if source > target && index >= target && index < source {
            self.tab_width(source) + self.gap
        } else {
            0.0
        }
    }

    pub(super) fn nudge(&self, offset: f32, direction: f32) -> f32 {
        let current = -offset;
        let next = if direction > 0.0 {
            self.slots
                .iter()
                .map(|slot| slot.0)
                .find(|&left| left > current + 0.5)
                .unwrap_or(self.width)
        } else {
            self.slots
                .iter()
                .rev()
                .map(|slot| slot.0)
                .find(|&left| left < current - 0.5)
                .unwrap_or(0.0)
        };
        (-next).clamp(-(self.width - self.viewport).max(0.0), 0.0)
    }
}

pub(super) fn text_width(
    text: SharedString,
    family: SharedString,
    size: f32,
    window: &Window,
) -> f32 {
    let line = window.text_system().shape_line(
        text.clone(),
        px(size),
        &[gpui::TextRun {
            len: text.len(),
            font: gpui::font(family),
            color: Default::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    );
    f32::from(line.width).ceil()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_overlap_drives_the_same_navigation_and_reorder_geometry() {
        let layout = TabStripLayout::new(
            [100.0, 220.0, 100.0, 220.0],
            350.0,
            8.0,
            gap(nebula_settings::TabStyle::Chrome),
        );
        assert_eq!(layout.width, 610.0);
        assert_eq!(layout.nudge(0.0, 1.0), -90.0);
        assert_eq!(layout.drag_target(0, 160.0, 4), 1);
        assert_eq!(layout.shift(1, 0, 2), -90.0);
        assert_eq!(layout.drag_limits(0, 4), (0.0, 450.0));
    }

    #[test]
    fn mixed_widths_drive_overflow_navigation_and_drag_targets() {
        let layout = TabStripLayout::new([88.0, 220.0, 110.0, 900.0], 380.0, 8.0, GAP);
        assert!(layout.overflow);
        assert_eq!(layout.viewport, 336.0);
        assert_eq!(layout.tab_width(3), MAX_WIDTH);
        assert_eq!(layout.nudge(0.0, 1.0), -92.0);
        assert_eq!(layout.nudge(-92.0, -1.0), 0.0);
        assert_eq!(layout.drag_target(0, 220.0, 4), 1);
        assert_eq!(layout.drag_target(3, -1000.0, 4), 0);
        assert_eq!(layout.shift(1, 0, 2), -92.0);
        assert_eq!(layout.shift(1, 3, 0), 224.0);
        assert_eq!(layout.nudge(-10000.0, 1.0), -(layout.width - layout.viewport));
        assert!(!TabStripLayout::new([90.0, 110.0], 380.0, 8.0, GAP).overflow);
    }
}
