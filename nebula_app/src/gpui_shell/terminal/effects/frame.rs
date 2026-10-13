use super::super::{colors::Palette, cursor_painter::CursorPaint};
use gpui::{Bounds, Pixels, Rgba};
use nebula_terminal::{
    term::color::Colors,
    vte::ansi::{Color, CursorShape, NamedColor},
};
use std::{sync::Arc, time::Instant};

#[derive(Clone, Copy, Default, PartialEq)]
struct Cursor {
    rect: [f32; 4],
    color: [f32; 4],
    style: u32,
    visible: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};

    #[test]
    fn encoded_layout_preserves_integer_flags_and_cursor_history() {
        let palette = Palette::default();
        let colors = Colors::default();
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(100.0), px(50.0)));
        let mut cursor = CursorPaint {
            rect: Bounds::new(point(px(5.0), px(10.0)), size(px(8.0), px(16.0))),
            shape: CursorShape::Beam,
            focused: true,
            visible: true,
            block_color: palette.cursor,
            stroke: palette.cursor,
            text_color: None,
        };
        let mut history = History::default();
        let first = history.encode([100, 50], bounds, 1.0, Some(&cursor), true, &palette, &colors);
        let word = |bytes: &[u8], index: usize| {
            u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
        };
        assert_eq!(first.len(), super::super::compiler::UNIFORM_BYTES);
        assert_eq!((word(&first, 8), word(&first, 9), word(&first, 10)), (1, 0, 1));
        assert_eq!(&first[3 * 16..4 * 16], &first[4 * 16..5 * 16]);
        cursor.rect.origin.x = px(20.0);
        let second =
            history.encode([100, 50], bounds, 1.0, Some(&cursor), false, &palette, &colors);
        assert_eq!(word(&second, 9), 1);
        assert_eq!(word(&second, 10), 0);
        assert_eq!(f32::from_bits(word(&second, 12)), 20.0);
        assert_eq!(f32::from_bits(word(&second, 16)), 5.0);
        assert_eq!(f32::from_bits(word(&second, 13 * 4)), palette.ansi[0].r);
    }
}

#[derive(Default)]
pub(super) struct History {
    started: Option<Instant>,
    last_time: f32,
    index: u32,
    current: Option<Cursor>,
    previous: Cursor,
    cursor_changed: f32,
    focused: bool,
    focus_changed: f32,
}

fn rgba(color: Rgba) -> [f32; 4] {
    [color.r, color.g, color.b, color.a]
}

impl History {
    pub fn unfocus(&mut self) {
        if self.focused {
            self.focused = false;
            self.focus_changed = self.started.map_or(0.0, |time| time.elapsed().as_secs_f32());
        }
    }

    pub fn encode(
        &mut self,
        extent: [i32; 2],
        bounds: Bounds<Pixels>,
        scale: f32,
        cursor: Option<&CursorPaint>,
        focused: bool,
        palette: &Palette,
        colors: &Colors,
    ) -> Arc<[u8]> {
        let started = self.started.get_or_insert_with(Instant::now);
        let seconds = started.elapsed().as_secs_f32();
        let delta = if self.index == 0 { 0.0 } else { seconds - self.last_time };
        self.last_time = seconds;
        let current = cursor
            .map(|cursor| Cursor {
                rect: [
                    f32::from(cursor.rect.left() - bounds.left()) * scale,
                    f32::from(if cursor.shape == CursorShape::Underline {
                        cursor.rect.bottom() - gpui::px(2.0) - bounds.top()
                    } else {
                        cursor.rect.top() - bounds.top()
                    }) * scale,
                    f32::from(if cursor.shape == CursorShape::Beam {
                        gpui::px(2.0)
                    } else {
                        cursor.rect.size.width
                    }) * scale,
                    f32::from(if cursor.shape == CursorShape::Underline {
                        gpui::px(2.0)
                    } else {
                        cursor.rect.size.height
                    }) * scale,
                ],
                color: rgba(if cursor.shape == CursorShape::Block {
                    cursor.block_color
                } else {
                    cursor.stroke
                }),
                style: match cursor.shape {
                    CursorShape::Block if cursor.focused => 0,
                    CursorShape::Block | CursorShape::HollowBlock => 1,
                    CursorShape::Beam => 2,
                    CursorShape::Underline => 3,
                    _ => 4,
                },
                visible: cursor.shape != CursorShape::Hidden && (!cursor.focused || cursor.visible),
            })
            .unwrap_or(Cursor { style: 4, ..Default::default() });
        if self.current != Some(current) {
            self.previous = self.current.unwrap_or(current);
            self.current = Some(current);
            self.cursor_changed = seconds;
        }
        if self.index == 0 || self.focused != focused {
            self.focused = focused;
            self.focus_changed = seconds;
        }
        let mut bytes = Vec::with_capacity(super::compiler::UNIFORM_BYTES);
        let floats = |value: [f32; 4]| value.map(f32::to_bits);
        let foreground = palette.resolve(Color::Named(NamedColor::Foreground), colors, false);
        let background = palette.resolve(Color::Named(NamedColor::Background), colors, false);
        let blocks = [
            floats([
                extent[0] as f32,
                extent[1] as f32,
                1.0 / extent[0] as f32,
                1.0 / extent[1] as f32,
            ]),
            floats([seconds, delta, self.cursor_changed, self.focus_changed]),
            [1, self.index, focused as u32, current.visible as u32],
            floats(current.rect),
            floats(self.previous.rect),
            floats(current.color),
            floats(self.previous.color),
            [current.style, self.previous.style, self.previous.visible as u32, 0],
            floats(rgba(foreground)),
            floats(rgba(background)),
            floats(rgba(palette.cursor_text.unwrap_or(background))),
            floats(rgba(palette.selection_foreground.unwrap_or(foreground))),
            floats(rgba(palette.selection)),
        ];
        for block in blocks {
            for word in block {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }
        for index in 0..=u8::MAX {
            for word in floats(rgba(palette.resolve(Color::Indexed(index), colors, false))) {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }
        self.index = self.index.wrapping_add(1);
        debug_assert_eq!(bytes.len(), super::compiler::UNIFORM_BYTES);
        bytes.into()
    }
}
