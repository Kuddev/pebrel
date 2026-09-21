//! Reader selection across adjacent inline Markdown fragments.
//!
//! The fragment views remain selectable so the window-level coordinator can
//! resolve a drag that crosses their hitboxes. This test intentionally checks
//! the rendered reading text, rather than source delimiters: selecting a
//! formatted fragment must not activate its live editor.

use super::*;
use gpui::{Modifiers, MouseButton, TestAppContext, VisualTestContext};
use gpui_component::WindowExt as _;

fn open(
    source: &str,
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<TextFileView>, VisualTestContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("inline-selection.md");
    std::fs::write(&path, source).unwrap();
    let (file, window) = tests::open(path, cx);
    (directory, file, window)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
}

fn part_containing(
    file: &Entity<TextFileView>,
    needle: &str,
    occurrence: usize,
    cx: &VisualTestContext,
) -> usize {
    file.read_with(cx, |view, cx| {
        let source = view.draft(cx);
        let structure = view.outline.structures[0].as_ref().expect("inline structure");
        structure
            .parts
            .iter()
            .enumerate()
            .filter(|(_, part)| source[part.range.clone()].contains(needle))
            .nth(occurrence)
            .unwrap_or_else(|| panic!("missing inline {needle:?} in {source:?}: {structure:?}"))
            .0
    })
}

#[gpui::test]
fn dragging_across_inline_fragments_selects_reading_text_without_entering_live_edit(
    cx: &mut TestAppContext,
) {
    let source = "Before **bold** $x$ _italic_ after";
    let (_directory, file, mut cx) = open(source, cx);
    let first = part_containing(&file, "Before", 0, &cx);
    let last = part_containing(&file, "after", 0, &cx);

    draw(&mut cx);
    let first_bounds = cx
        .debug_bounds(format!("markdown-inline-part-0-{first}").leak())
        .expect("first inline fragment bounds");
    let last_bounds = cx
        .debug_bounds(format!("markdown-inline-part-0-{last}").leak())
        .expect("last inline fragment bounds");
    assert!(first_bounds.right() <= last_bounds.left(), "fragments must stay ordered");

    let start = gpui::point(first_bounds.left() + px(0.1), first_bounds.center().y);
    let end = gpui::point(last_bounds.right() - px(0.1), last_bounds.center().y);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    draw(&mut cx);
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    draw(&mut cx);
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    draw(&mut cx);

    let selected = cx.update(|window, cx| window.selected_text(cx).to_string());
    let normalized = selected.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        normalized.contains("Before bold") && normalized.contains("italic after"),
        "selected: {selected:?}"
    );
    assert!(file.read_with(&cx, |view, _| view.live_edit.is_none()));
    cx.simulate_keystrokes("ctrl-c");
    cx.run_until_parked();
    assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "Before bold $x$ italic after");
}
