//! Theme-aware chrome, line and tag treatments for the same tab controls.

use gpui::{
    App, Div, InteractiveElement as _, IntoElement as _, ParentElement as _, Stateful, Styled as _,
    canvas, div, point, px,
};
use gpui_component::ActiveTheme as _;
use nebula_settings::TabStyle;

fn one_dark_background(cx: &App) -> Option<gpui::Hsla> {
    let resolved = crate::gpui_shell::theme::resolved_theme(cx);
    (resolved.base_name().prompt_name() == "OneDark" && !resolved.is_custom())
        .then_some(cx.theme().sidebar_accent)
}

fn one_dark_edge(cx: &App) -> Option<gpui::Hsla> {
    one_dark_background(cx).map(|background| background.blend(cx.theme().foreground.opacity(0.18)))
}

pub(super) fn apply(
    mut row: Stateful<Div>,
    style: TabStyle,
    active: bool,
    dragged: bool,
    hover_group: gpui::SharedString,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    match style {
        TabStyle::Chrome => {
            row = if active {
                row.rounded_tl(px(8.0)).rounded_tr(px(8.0))
            } else {
                row.rounded(px(8.0))
            };
            if active {
                let color = theme.sidebar_accent;
                // The lower corners flare into the page, as in a browser tab.
                row = row.child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            let w = f32::from(bounds.size.width);
                            let h = f32::from(bounds.size.height);
                            let at = |x, y| point(bounds.left() + px(x), bounds.top() + px(y));
                            let mut path = gpui::PathBuilder::fill();
                            path.move_to(at(0.0, h));
                            path.curve_to(at(12.0, h - 12.0), at(12.0, h));
                            path.line_to(at(12.0, 8.0));
                            path.curve_to(at(20.0, 0.0), at(12.0, 0.0));
                            path.line_to(at(w - 20.0, 0.0));
                            path.curve_to(at(w - 12.0, 8.0), at(w - 12.0, 0.0));
                            path.line_to(at(w - 12.0, h - 12.0));
                            path.curve_to(at(w, h), at(w - 12.0, h));
                            path.close();
                            if let Ok(path) = path.build() {
                                window.paint_path(path, color);
                            }
                        },
                    )
                    .absolute()
                    .inset_0()
                    .into_any_element(),
                );
            }
        },
        TabStyle::Minimal => {
            if active {
                row =
                    row.bg(one_dark_background(cx).unwrap_or(theme.list_hover.opacity(0.4))).child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .h(px(1.5))
                            .bg(one_dark_edge(cx).unwrap_or(theme.primary)),
                    );
            }
        },
        TabStyle::Tag => {
            let mut surface = div()
                .absolute()
                .top(px(2.0))
                .right(px(2.0))
                .bottom(px(2.0))
                .left(px(2.0))
                .rounded(px(7.0));
            if active {
                let background = one_dark_background(cx).unwrap_or_else(|| {
                    theme.primary.opacity(if theme.is_dark() { 0.18 } else { 0.12 })
                });
                let edge = one_dark_edge(cx).unwrap_or(theme.primary.opacity(0.6));
                surface = surface.border_1().border_color(edge).bg(background);
            } else if dragged {
                surface = surface.bg(theme.list_hover);
            } else {
                surface = surface.group_hover(hover_group, |style| style.bg(theme.list_hover));
            }
            row = row.relative().child(surface);
        },
    }
    row
}
