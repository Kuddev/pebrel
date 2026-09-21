//! Real GPUI coverage for local Markdown source reveal.
//!
//! The clicks below use the native input's laid-out ranges. This keeps the
//! contract independent of font metrics while exercising the same mouse,
//! selection and composition paths as the reader.

use super::*;
use gpui::{EntityInputHandler, Modifiers, MouseButton, TestAppContext, VisualTestContext};
use std::ops::Range;

fn open(
    source: &str,
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<TextFileView>, VisualTestContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("inline-live.md");
    std::fs::write(&path, source).unwrap();
    let (file, window) = tests::open(path, cx);
    (directory, file, window)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

fn begin_paragraph(
    file: &Entity<TextFileView>,
    cx: &mut VisualTestContext,
) -> Entity<gpui_component::input::InputState> {
    let input = cx.update(|window, cx| {
        file.update(cx, |view, cx| {
            view.live_mode = true;
            view.begin_live_edit(0, window, cx);
            view.live_edit.as_ref().unwrap().input.clone()
        })
    });
    draw(cx);
    input
}

fn input_range_bounds(
    input: &Entity<gpui_component::input::InputState>,
    range: &Range<usize>,
    cx: &mut VisualTestContext,
) -> gpui::Bounds<gpui::Pixels> {
    input
        .read_with(cx, |input, _| input.range_to_bounds(range))
        .unwrap_or_else(|| panic!("input range has no laid-out bounds: {range:?}"))
}

fn click_input_range(
    input: &Entity<gpui_component::input::InputState>,
    range: Range<usize>,
    cx: &mut VisualTestContext,
) {
    let bounds = input_range_bounds(input, &range, cx);
    let point = bounds.center();
    cx.simulate_mouse_down(point, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(point, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
        window.simulate_next_frame(cx);
    });
    cx.run_until_parked();
}

fn text_range(text: &str, needle: &str) -> Range<usize> {
    let start = text.find(needle).unwrap_or_else(|| panic!("missing {needle:?} in {text:?}"));
    start..start + needle.len()
}

fn source_range(source: &str, needle: &str) -> Range<usize> {
    text_range(source, needle)
}

fn assert_reveal_is_clean(
    file: &Entity<TextFileView>,
    input: &Entity<gpui_component::input::InputState>,
    cx: &mut VisualTestContext,
    source: &str,
    reveal: Option<Range<usize>>,
    rendered: &str,
) {
    file.read_with(cx, |view, cx| {
        let edit = view.live_edit.as_ref().expect("live input remains active");
        assert_eq!(edit.input, input.clone(), "revealing an inline must reuse the native input");
        assert_eq!(edit.projection.revealed(), reveal);
        assert_eq!(view.draft(cx).as_ref(), source, "reveal must not edit the document");
        assert!(!view.is_dirty(), "reveal must not mark the document dirty");
        assert!(!view.preview_stale, "reveal must not schedule a preview parse");
        if reveal.is_none() {
            assert_eq!(edit.input.read(cx).value().as_ref(), rendered);
        }
    });
}

#[gpui::test]
fn first_preview_click_reveals_the_hit_inline_without_exposing_its_neighbour(
    cx: &mut TestAppContext,
) {
    let source = "**widebold** after _italic_";
    let (_, file, mut cx) = open(source, cx);
    draw(&mut cx);
    let bounds = cx.debug_bounds("markdown-preview-block-0").unwrap();
    let point = gpui::point(bounds.left() + px(18.0), bounds.top() + px(15.0));
    cx.simulate_mouse_down(point, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(point, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
        window.simulate_next_frame(cx);
    });
    cx.run_until_parked();
    file.read_with(&cx, |view, cx| {
        let edit = view.live_edit.as_ref().unwrap();
        assert_eq!(edit.input.read(cx).value(), "**widebold** after italic");
        assert_eq!(edit.projection.revealed(), Some(0..12));
        assert_eq!(view.draft(cx), source);
        assert!(!view.dirty);
    });
}

#[gpui::test]
fn clicking_an_inline_reveals_only_that_source_and_plain_text_collapses_it(
    cx: &mut TestAppContext,
) {
    let source = "Before **bold** and _italic_ then [link](https://example.test/path \"title\") plus `code` after";
    let (_directory, file, mut cx) = open(source, cx);
    let input = begin_paragraph(&file, &mut cx);
    let rendered = super::inline_edit::Projection::new(source).text;

    for (label, markdown) in [
        ("bold", "**bold**"),
        ("italic", "_italic_"),
        ("link", "[link](https://example.test/path \"title\")"),
        ("code", "`code`"),
    ] {
        let visible = text_range(&input.read_with(&cx, |input, _| input.value()), label);
        click_input_range(&input, visible, &mut cx);
        let expected = source_range(source, markdown);
        let value = input.read_with(&cx, |input, _| input.value());
        assert!(value.contains(markdown), "{label} source was not exposed: {value:?}");
        for (other_label, other_markdown) in [
            ("bold", "**bold**"),
            ("italic", "_italic_"),
            ("link", "[link](https://example.test/path \"title\")"),
            ("code", "`code`"),
        ] {
            if other_label != label {
                assert!(
                    !value.contains(other_markdown),
                    "revealing {label} must keep {other_label} rendered: {value:?}"
                );
            }
        }
        assert_reveal_is_clean(&file, &input, &mut cx, source, Some(expected), &rendered);
    }

    let visible = text_range(&input.read_with(&cx, |input, _| input.value()), "Before");
    click_input_range(&input, visible, &mut cx);
    assert_reveal_is_clean(&file, &input, &mut cx, source, None, &rendered);
}

#[gpui::test]
fn link_destination_can_be_saved_and_undone_without_touching_its_label(cx: &mut TestAppContext) {
    let source = "Before **bold** and [link](https://old.example/path \"title\") after";
    let (directory, file, mut cx) = open(source, cx);
    let path = directory.path().join("inline-live.md");
    let input = begin_paragraph(&file, &mut cx);
    let link_visible = text_range(&input.read_with(&cx, |input, _| input.value()), "link");
    click_input_range(&input, link_visible, &mut cx);

    let old_url = "https://old.example/path";
    let new_url = "https://new.example/path";
    let url_range = text_range(&input.read_with(&cx, |input, _| input.value()), old_url);
    input.update(&mut cx, |input, cx| input.set_selected_range(url_range, cx));
    cx.simulate_input(new_url);
    cx.run_until_parked();

    let expected = source.replacen(old_url, new_url, 1);
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx).to_string()), expected);
    assert!(file.read_with(&cx, |view, _| view.is_dirty()));
    assert_eq!(
        input.read_with(&cx, |input, _| input.value().to_string()),
        format!("Before bold and [link]({new_url} \"title\") after")
    );

    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
    assert!(!file.read_with(&cx, |view, _| view.is_dirty()));

    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx).to_string()), source);
    assert!(file.read_with(&cx, |view, _| view.is_dirty()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
    let resumed = file.read_with(&cx, |view, cx| {
        view.live_edit
            .as_ref()
            .expect("undo keeps the paragraph active")
            .input
            .read(cx)
            .value()
            .to_string()
    });
    let old_rendered = super::inline_edit::Projection::new(source).text;
    let old_link_revealed = "Before bold and [link](https://old.example/path \"title\") after";
    assert!(
        resumed == old_rendered || resumed == old_link_revealed,
        "undo may restore the link's local projection, but not whole-paragraph source: {resumed:?}"
    );
    assert!(!resumed.contains("**bold**"));
}

#[gpui::test]
fn cjk_composition_keeps_revealed_inline_and_native_input_identity(cx: &mut TestAppContext) {
    let source = "前 **你好** 后 [链接](https://example.test) 尾";
    let (_directory, file, mut cx) = open(source, cx);
    let input = begin_paragraph(&file, &mut cx);
    let visible = text_range(&input.read_with(&cx, |input, _| input.value()), "你好");
    click_input_range(&input, visible, &mut cx);
    let revealed = source_range(source, "**你好**");
    let before = file.read_with(&cx, |view, cx| {
        (
            view.draft(cx).to_string(),
            view.is_dirty(),
            view.preview_stale,
            view.live_edit.as_ref().unwrap().projection.revealed(),
        )
    });
    assert_eq!(before.0, source);
    assert!(!before.1 && !before.2);
    assert_eq!(before.3, Some(revealed.clone()));

    let caret = input.read_with(&cx, |input, _| {
        let value = input.value();
        value.find("**你好**").unwrap() + 2 + "你".len()
    });
    input.update(&mut cx, |input, cx| input.set_selected_range(caret..caret, cx));
    let marked_before = cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "世", Some(1..1), window, cx);
            input.marked_text_range(window, cx)
        })
    });
    cx.run_until_parked();

    let marked_after =
        cx.update(|window, cx| input.update(cx, |input, cx| input.marked_text_range(window, cx)));
    assert!(marked_before.is_some(), "composition must create a marked range");
    assert_eq!(marked_after, marked_before, "selection sync must not rewrite the marked range");
    file.read_with(&cx, |view, cx| {
        let edit = view.live_edit.as_ref().expect("composition keeps live editing");
        assert_eq!(edit.input, input);
        assert_eq!(
            edit.projection.revealed(),
            Some(revealed.start..revealed.end + "世".len()),
            "composition must extend the same revealed inline"
        );
        let value = edit.input.read(cx).value();
        assert!(value.contains("**你世好**"));
        assert!(value.contains("链接"), "an untouched neighboring link stays rendered");
        assert!(!value.contains("[链接]("));
        assert!(view.draft(cx).contains("**你世好**"));
        assert!(view.is_dirty(), "composition Change updates the draft");
    });

    cx.update(|window, cx| input.update(cx, |input, cx| input.unmark_text(window, cx)));
    cx.run_until_parked();
    let draft = file.read_with(&cx, |view, cx| view.draft(cx).to_string());
    assert!(
        draft.contains("**你世好**"),
        "composition should commit in the revealed inline: {draft}"
    );
    assert!(draft.contains("[链接](https://example.test)"));
    assert!(file.read_with(&cx, |view, _| view.live_edit.as_ref().unwrap().input == input));
}
