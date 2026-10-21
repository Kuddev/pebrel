//! Static overflow fading; the shell status and close slot remain outside the mask.

use std::{cell::OnceCell, rc::Rc};

use gpui::{
    AnyElement, Bounds, ContentMask, FontWeight, InteractiveElement as _, IntoElement as _,
    ParentElement as _, ShapedLine, SharedString, StatefulInteractiveElement as _, Styled as _,
    TextAlign, canvas, div, point, px, size,
};

const FADE_WIDTH: f32 = 48.0;
const FADE_STEPS: usize = 24;

pub(super) fn render(
    index: usize,
    title: SharedString,
    family: SharedString,
    font_size: f32,
    text_width: f32,
    overflow: f32,
) -> AnyElement {
    let label = div()
        .id(("top-tab-label", index))
        .debug_selector(move || format!("top-tab-label-{index}"))
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .font_family(family)
        .text_size(px(font_size))
        .font_weight(FontWeight::NORMAL);
    if overflow <= 0.5 {
        return label
            .child(
                div()
                    .debug_selector(move || format!("top-tab-title-static-{index}"))
                    .whitespace_nowrap()
                    .child(title),
            )
            .into_any_element();
    }

    // Retain the normal text height and accessibility label without painting it twice.
    let mut content = div()
        .id(("top-tab-title-fade", index))
        .debug_selector(move || format!("top-tab-title-fade-{index}"))
        .relative()
        .w_full()
        .aria_label(title.clone())
        .child(
            div()
                .debug_selector(move || format!("top-tab-title-text-{index}"))
                .w(px(text_width))
                .whitespace_nowrap()
                .opacity(0.0)
                .child(title.clone()),
        );
    let shaped = Rc::new(OnceCell::<ShapedLine>::new());
    // Reuse one shaped line for a solid center and translucent strips.
    // Glyph alpha preserves wallpaper/transparency and also fades color emoji.
    for band in 0..=FADE_STEPS {
        let shaped = shaped.clone();
        let text = title.clone();
        let opacity = if band == 0 { 1.0 } else { 1.0 - (band as f32 - 0.5) / FADE_STEPS as f32 };
        content = content.child(
            // Canvas does not apply style opacity; Div applies it to the painted glyphs.
            div().absolute().inset_0().opacity(opacity).child(
                canvas(
                    move |_, window, _| {
                        shaped.get_or_init(|| {
                            let style = window.text_style();
                            window.text_system().shape_line(
                                text.clone(),
                                px(font_size),
                                &[style.to_run(text.len())],
                                None,
                            )
                        });
                        shaped
                    },
                    move |bounds, shaped, window, cx| {
                        if !bounds.intersects(&window.content_mask().bounds) {
                            return;
                        }
                        let width = f32::from(bounds.size.width);
                        let fade = FADE_WIDTH.min(width);
                        let step = fade / FADE_STEPS as f32;
                        let (left, width) = if band == 0 {
                            (0.0, width - fade)
                        } else {
                            (width - fade + (band - 1) as f32 * step, step)
                        };
                        if width <= 0.0 {
                            return;
                        }
                        let clip = Bounds::new(
                            point(bounds.left() + px(left), bounds.top()),
                            size(px(width), bounds.size.height),
                        );
                        window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                            let _ = shaped.get().unwrap().paint(
                                bounds.origin,
                                bounds.size.height,
                                TextAlign::Left,
                                None,
                                window,
                                cx,
                            );
                        });
                    },
                )
                .size_full(),
            ),
        );
    }
    label.child(content).into_any_element()
}
