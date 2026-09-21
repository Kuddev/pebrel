//! Hit actual inline object bounds and verify that only their source is edited.

use super::*;
use gpui::{EntityInputHandler, Modifiers, TestAppContext, VisualTestContext};

fn part_for(
    file: &Entity<TextFileView>,
    needle: &str,
    occurrence: usize,
    cx: &VisualTestContext,
) -> usize {
    file.read_with(cx, |view, cx| {
        let source = view.draft(cx);
        let structure = view.outline.structures[0].as_ref().unwrap();
        structure
            .parts
            .iter()
            .enumerate()
            .filter(|(_, part)| &source[part.range.clone()] == needle)
            .nth(occurrence)
            .unwrap_or_else(|| panic!("missing inline {needle}: {structure:?}"))
            .0
    })
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let _ = window.draw(cx);
        window.simulate_next_frame(cx);
    });
    cx.run_until_parked();
}

fn click_part(part: usize, cx: &mut VisualTestContext) {
    draw(cx);
    let bounds = cx.debug_bounds(format!("markdown-inline-part-0-{part}").leak()).unwrap();
    assert!(bounds.size.width > px(0.0) && bounds.size.height > px(0.0), "{bounds:?}");
    cx.simulate_mouse_down(bounds.center(), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(bounds.center(), MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    draw(cx);
}

#[gpui::test]
fn second_identical_formula_edits_only_its_source_and_keeps_other_objects(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects.md");
    let source = "**Before** $x^2$ between $x^2$ **After**";
    std::fs::write(&path, source).unwrap();
    let (file, mut cx) = tests::open(path.clone(), cx);
    let first = part_for(&file, "$x^2$", 0, &cx);
    let second = part_for(&file, "$x^2$", 1, &cx);
    draw(&mut cx);
    let first_bounds = cx.debug_bounds(format!("markdown-inline-part-0-{first}").leak()).unwrap();
    let second_bounds = cx.debug_bounds(format!("markdown-inline-part-0-{second}").leak()).unwrap();
    assert!(first_bounds.right() <= second_bounds.left(), "inline formulas must share a row");
    assert!((first_bounds.bottom() - second_bounds.bottom()).abs() < px(3.0));
    click_part(second, &mut cx);
    let input = file.read_with(&cx, |view, cx| {
        let edit = view.live_edit.as_ref().unwrap();
        assert_eq!(edit.part, Some(second));
        assert_eq!(edit.input.read(cx).value(), "$x^2$");
        assert_eq!(edit.range.start, source.rfind("$x^2$").unwrap());
        assert!(!view.dirty);
        edit.input.clone()
    });
    assert!(cx.debug_bounds("markdown-math-formula").is_some(), "unclicked formula stays rendered");
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.set_selected_range(1..4, cx);
            input.replace_and_mark_text_in_range(None, "中", Some(1..1), window, cx);
        });
    });
    cx.run_until_parked();
    assert!(file.read_with(&cx, |view, _| view.live_edit.as_ref().unwrap().input == input));
    cx.update(|window, cx| input.update(cx, |input, cx| input.unmark_text(window, cx)));
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    let expected = "**Before** $x^2$ between $中$ **After**";
    assert_eq!(std::fs::read_to_string(path).unwrap(), expected);
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), source);
    cx.simulate_keystrokes("ctrl-shift-z");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), expected);
}

#[gpui::test]
fn image_source_is_local_and_surrounding_formula_stays_rendered(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    image::RgbaImage::from_pixel(24, 18, image::Rgba([80, 120, 180, 255]))
        .save(dir.path().join("image.png"))
        .unwrap();
    let path = dir.path().join("image.md");
    let source = "Before ![alt](image.png) $y^2$ **After**";
    std::fs::write(&path, source).unwrap();
    let (file, mut cx) = tests::open(path, cx);
    let image_part = part_for(&file, "![alt](image.png)", 0, &cx);
    click_part(image_part, &mut cx);
    file.read_with(&cx, |view, cx| {
        let edit = view.live_edit.as_ref().unwrap();
        assert_eq!(edit.input.read(cx).value(), "![alt](image.png)");
        assert_eq!(edit.range, 7..24);
        assert!(!view.dirty);
    });
    assert!(cx.debug_bounds("markdown-math-formula").is_some());
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw(&mut cx);
    assert!(file.read_with(&cx, |view, _| view.live_edit.is_none()));
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), source);
}
