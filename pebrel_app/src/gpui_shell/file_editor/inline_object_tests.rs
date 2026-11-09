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
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    draw(cx);
    if let Some(copy) = cx.debug_bounds("markdown-copy-math-latex") {
        assert!(!copy.contains(&bounds.center()), "copy={copy:?} covers object={bounds:?}");
    }
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
    let (file, mut cx) = tests::open_live(path.clone(), cx);
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
    let (file, mut cx) = tests::open_live(path, cx);
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

fn settle_object(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    draw(cx);
    draw(cx);
}

fn object_click(selector: &'static str, cx: &mut VisualTestContext) {
    draw(cx);
    let bounds = cx.debug_bounds(selector).expect(selector);
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    draw(cx);
    let position = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} disappeared after moving into {bounds:?}"))
        .center();
    cx.simulate_click(position, Modifiers::default());
    cx.run_until_parked();
}

fn object_type(input: &Entity<InputState>, text: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.set_selected_range(0..input.text().len(), cx);
            // set_value 是静默赋值；用户输入必须走会发送 Change 的原生路径。
            input.replace_text_in_range(None, text, window, cx);
        })
    });
    cx.run_until_parked();
}

#[gpui::test]
fn reader_formula_draft_floats_without_moving_text_and_edits_only_the_clicked_occurrence(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reader.md");
    let source = "Before $x^2$ between $x^2$ after\n\nFollowing paragraph";
    std::fs::write(&path, source).unwrap();
    let (file, mut cx) = tests::open(path.clone(), cx);
    cx.simulate_resize(gpui::size(px(1100.0), px(1000.0)));
    let second = part_for(&file, "$x^2$", 1, &cx);
    draw(&mut cx);
    let following = cx.debug_bounds("markdown-preview-block-1").unwrap();
    click_part(second, &mut cx);
    let formula = cx.debug_bounds(format!("markdown-inline-part-0-{second}").leak()).unwrap();
    let panel = cx.debug_bounds("markdown-object-editor").unwrap();
    assert!(panel.top() >= formula.bottom(), "editor opens below its paragraph");
    assert_eq!(cx.debug_bounds("markdown-preview-block-1").unwrap(), following);
    let paragraph = cx.debug_bounds("markdown-preview-block-0").unwrap();
    assert!((panel.center().x - paragraph.center().x).abs() < px(1.0));
    let input = file.read_with(&cx, |view, cx| {
        let edit = view.object_edit.as_ref().unwrap();
        assert_eq!(edit.part, second);
        assert_eq!(edit.input.read(cx).value(), "x^2");
        assert!(!view.live_mode && view.live_edit.is_none());
        edit.input.clone()
    });
    object_type(&input, "y^3", &mut cx);
    settle_object(&mut cx);
    file.read_with(&cx, |view, cx| {
        assert_eq!(view.draft(cx), source);
        assert_eq!(view.object_edit.as_ref().unwrap().preview, "$y^3$");
        assert!(view.is_dirty());
    });
    cx.simulate_keystrokes("ctrl-s ctrl-/");
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
    assert!(file.read_with(&cx, |view, cx| view.preview
        && view.object_edit.is_some()
        && view.draft(cx).as_ref() == source));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(file.read_with(&cx, |view, _| view.object_edit.is_none() && !view.is_dirty()));
    click_part(second, &mut cx);
    let input = file.read_with(&cx, |view, _| view.object_edit.as_ref().unwrap().input.clone());
    object_type(&input, "y^3", &mut cx);
    settle_object(&mut cx);
    object_click("markdown-object-apply", &mut cx);
    let expected = "Before $x^2$ between $y^3$ after\n\nFollowing paragraph";
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), expected);
    assert!(file.read_with(&cx, |view, _| view.object_edit.is_none() && view.is_dirty()));
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), source);
    cx.simulate_keystrokes("ctrl-shift-z");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), expected);
}

#[gpui::test]
fn display_formula_retains_native_input_during_ime_and_rejects_invalid_draft(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("formula.md");
    let source = "$$\nx^2\n$$\n\nFollowing paragraph";
    std::fs::write(&path, source).unwrap();
    let (file, mut cx) = tests::open(path, cx);
    cx.simulate_resize(gpui::size(px(1100.0), px(1000.0)));
    object_click("markdown-math-block-0", &mut cx);
    let input = file.read_with(&cx, |view, _| view.object_edit.as_ref().unwrap().input.clone());
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.set_selected_range(0..3, cx);
            input.replace_and_mark_text_in_range(None, "中", Some(1..1), window, cx);
        })
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(file.read_with(&cx, |view, _| {
        view.object_edit.as_ref().is_some_and(|edit| edit.input == input)
    }));
    object_type(&input, "\\frac{", &mut cx);
    settle_object(&mut cx);
    object_click("markdown-object-apply", &mut cx);
    assert!(file.read_with(&cx, |view, cx| view.object_edit.is_some()
        && view.draft(cx).as_ref() == source));
    assert!(cx.debug_bounds("markdown-object-error").is_some());
    object_type(&input, "y^3", &mut cx);
    settle_object(&mut cx);
    cx.simulate_keystrokes("ctrl-enter");
    cx.run_until_parked();
    assert_eq!(
        file.read_with(&cx, |view, cx| view.draft(cx)),
        "$$\ny^3\n$$\n\nFollowing paragraph"
    );
}

#[gpui::test]
fn image_fields_keep_reference_neighbors_and_ignore_a_picker_after_cancel(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    for name in ["image.png", "chosen image.png"] {
        image::RgbaImage::from_pixel(24, 18, image::Rgba([80, 120, 180, 255]))
            .save(dir.path().join(name))
            .unwrap();
    }
    let path = dir.path().join("images.md");
    let source = "Before ![alt][plot] $y^2$ ![other][plot] after\n\n[plot]: image.png \"Title\"";
    std::fs::write(&path, source).unwrap();
    let (file, mut cx) = tests::open(path, cx);
    cx.simulate_resize(gpui::size(px(1100.0), px(1000.0)));
    let part = part_for(&file, "![alt][plot]", 0, &cx);
    click_part(part, &mut cx);
    settle_object(&mut cx);
    let alt = file.read_with(&cx, |view, cx| {
        let edit = view.object_edit.as_ref().unwrap();
        assert_eq!(edit.input.read(cx).value(), "image.png");
        edit.alt.clone().unwrap()
    });
    object_type(&alt, "*中文* _图_ `code` ~~bar~~ [图] & description", &mut cx);
    settle_object(&mut cx);
    object_click("markdown-object-choose-image", &mut cx);
    assert!(cx.did_prompt_for_paths());
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    let picked = dir.path().join("chosen image.png");
    cx.simulate_path_prompt_response(move |_| Some(vec![picked]));
    cx.run_until_parked();
    assert!(file.read_with(&cx, |view, cx| view.object_edit.is_none()
        && view.draft(cx).as_ref() == source));
    click_part(part, &mut cx);
    let alt =
        file.read_with(&cx, |view, _| view.object_edit.as_ref().unwrap().alt.clone().unwrap());
    object_type(&alt, "*中文* _图_ `code` ~~bar~~ [图] & description", &mut cx);
    let address = file.read_with(&cx, |view, _| view.object_edit.as_ref().unwrap().input.clone());
    object_type(&address, "missing.png", &mut cx);
    settle_object(&mut cx);
    object_click("markdown-object-apply", &mut cx);
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), source);
    assert!(cx.debug_bounds("markdown-object-error").is_some());
    object_type(&address, "image.png", &mut cx);
    settle_object(&mut cx);
    object_click("markdown-object-choose-image", &mut cx);
    let chosen = dir.path().join("chosen image.png");
    let expected_address = chosen.to_string_lossy().replace('\\', "/");
    cx.simulate_path_prompt_response(move |_| Some(vec![chosen]));
    settle_object(&mut cx);
    object_click("markdown-object-apply", &mut cx);
    let result = file.read_with(&cx, |view, cx| view.draft(cx).to_string());
    assert!(result.contains("![other][plot] after"));
    assert!(result.ends_with("[plot]: image.png \"Title\""));
    let markdown::mdast::Node::Root(root) =
        markdown::to_mdast(&result, &markdown::ParseOptions::gfm()).unwrap()
    else {
        panic!()
    };
    let markdown::mdast::Node::Paragraph(paragraph) = &root.children[0] else { panic!() };
    let image = paragraph
        .children
        .iter()
        .find_map(
            |node| if let markdown::mdast::Node::Image(image) = node { Some(image) } else { None },
        )
        .unwrap();
    assert_eq!(image.url, expected_address);
    assert_eq!(image.alt, "*中文* _图_ `code` ~~bar~~ [图] & description");
    assert_eq!(image.title.as_deref(), Some("Title"));
}

#[gpui::test]
fn reader_formula_copy_click_does_not_open_an_editor(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("copy.md");
    std::fs::write(&path, "$$\nx^2\n$$\n\nFollowing paragraph").unwrap();
    let (file, mut cx) = tests::open(path, cx);
    cx.simulate_resize(gpui::size(px(1100.0), px(1000.0)));
    draw(&mut cx);
    let formula = cx.debug_bounds("markdown-math-formula").unwrap();
    cx.simulate_mouse_move(formula.center(), None, Modifiers::default());
    draw(&mut cx);
    let copy = cx.debug_bounds("markdown-copy-math-latex").unwrap();
    assert!(copy.bottom() <= formula.top(), "copy={copy:?} overlaps formula={formula:?}");
    object_click("markdown-copy-math-latex", &mut cx);
    let opened = file.read_with(&cx, |view, _| view.object_edit.is_some());
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("x^2".to_owned()),
        "formula={formula:?}, copy={copy:?}, editor_open={opened}"
    );
    assert!(!opened);
}
