//! Exercise actual mouse/keyboard input in structured blocks, then verify the
//! canonical document and saved bytes, including virtual-list eviction.

use super::*;
use gpui::{Modifiers, TestAppContext, VisualTestContext};

fn open(
    source: &str,
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<TextFileView>, VisualTestContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("structured.md");
    std::fs::write(&path, source).unwrap();
    let (view, window) = tests::open_live(path, cx);
    (directory, view, window)
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("missing {selector}"));
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

fn select_all(file: &Entity<TextFileView>, cx: &mut VisualTestContext) {
    file.update(cx, |view, cx| {
        let input = &view.live_edit.as_ref().unwrap().input;
        input.update(cx, |input, cx| input.set_selected_range(0..input.value().len(), cx));
    });
}

#[gpui::test]
fn table_edits_cells_tabs_to_next_and_appends_a_row_without_rewriting_alignment(
    cx: &mut TestAppContext,
) {
    let source = "| Name | Value |\n| :--- | ---: |\n| A | **Old** |\n\nTail";
    let (directory, file, mut cx) = open(source, cx);
    click("markdown-part-0-3", &mut cx);
    assert_eq!(
        file.read_with(&cx, |view, cx| view.live_edit.as_ref().unwrap().input.read(cx).value()),
        "Old"
    );
    select_all(&file, &mut cx);
    cx.simulate_input("中文");
    cx.run_until_parked();
    assert_eq!(
        file.read_with(&cx, |view, cx| view.draft(cx)),
        source.replace("**Old**", "**中文**")
    );
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    cx.simulate_input("New");
    cx.run_until_parked();
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    cx.simulate_input("Cell");
    cx.run_until_parked();
    let draft = file.read_with(&cx, |view, cx| view.draft(cx).to_string());
    assert!(draft.contains("| :--- | ---: |"), "{draft}");
    assert!(
        draft.contains("New") && draft.contains("Cell") && draft.ends_with("\n\nTail"),
        "{draft}"
    );
    assert!(file.read_with(&cx, |view, _| view.live_edit.as_ref().unwrap().part.is_some()));
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(directory.path().join("structured.md")).unwrap(), draft);
}

#[gpui::test]
fn code_input_keeps_container_language_and_fences_while_enter_edits_code(cx: &mut TestAppContext) {
    let source = "~~~~rust demo\nlet value = 1;\n~~~~\n\nTail";
    let (_, file, mut cx) = open(source, cx);
    click("pebrel-code-text", &mut cx);
    let input = file.read_with(&cx, |view, cx| {
        let edit = view.live_edit.as_ref().unwrap();
        assert_eq!(edit.input.read(cx).value(), "let value = 1;");
        edit.input.clone()
    });
    assert!(cx.debug_bounds("pebrel-code-block").is_some());
    assert!(cx.debug_bounds("markdown-language-picker").is_some());
    select_all(&file, &mut cx);
    cx.simulate_input("let changed = 2;");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("done();");
    cx.run_until_parked();
    assert!(file.read_with(&cx, |view, _| view.live_edit.as_ref().unwrap().input == input));
    let draft = file.read_with(&cx, |view, cx| view.draft(cx).to_string());
    assert!(draft.starts_with("~~~~rust demo\nlet changed = 2;\n"), "{draft}");
    assert!(draft.ends_with("done();\n~~~~\n\nTail"), "{draft}");
    cx.simulate_keystrokes("ctrl-enter");
    cx.run_until_parked();
    assert!(file.read_with(&cx, |view, _| view.live_edit.is_none()));
}

#[gpui::test]
fn empty_code_fence_accepts_text_without_swallowing_the_closing_fence(cx: &mut TestAppContext) {
    let (_, file, mut cx) = open("```rust\n```", cx);
    click("pebrel-code-text", &mut cx);
    cx.simulate_input("code()");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), "```rust\ncode()\n```");
}

#[gpui::test]
fn list_enter_continues_items_tab_indents_and_empty_enter_exits(cx: &mut TestAppContext) {
    let (_, file, mut cx) = open("- First\n- Second", cx);
    click("markdown-part-0-1", &mut cx);
    file.update(&mut cx, |view, cx| {
        view.live_edit.as_ref().unwrap().input.update(cx, |input, cx| {
            let end = input.value().len();
            input.set_selected_range(end..end, cx);
        });
    });
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.simulate_input("Third");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), "- First\n- Second\n- Third");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), "- First\n- Second\n  - Third");
    cx.simulate_keystrokes("shift-tab");
    cx.run_until_parked();
    file.update(&mut cx, |view, cx| {
        view.live_edit
            .as_ref()
            .unwrap()
            .input
            .update(cx, |input, cx| input.set_selected_range(5..5, cx));
    });
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.simulate_input("Paragraph");
    cx.run_until_parked();
    assert_eq!(
        file.read_with(&cx, |view, cx| view.draft(cx)),
        "- First\n- Second\n- Third\n\nParagraph"
    );
}

#[gpui::test]
fn task_checkbox_stays_square_and_click_updates_only_its_marker(cx: &mut TestAppContext) {
    let source = "- [ ] Task\n- Neighbour";
    let (_, file, mut cx) = open(source, cx);
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let square = cx.debug_bounds("markdown-task-box-0").unwrap();
    assert_eq!(square.size.width, px(14.0));
    assert_eq!(square.size.height, px(14.0));
    click("markdown-task-0-3", &mut cx);
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), "- [x] Task\n- Neighbour");
    let checked = cx.debug_bounds("markdown-task-box-0").unwrap();
    assert_eq!(checked.size, square.size);
    click("markdown-task-0-3", &mut cx);
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), source);
}

#[gpui::test]
fn formula_itself_opens_latex_and_finishing_restores_rendering(cx: &mut TestAppContext) {
    let (_, file, mut cx) = open("$$\na^2 + b^2\n$$", cx);
    click("markdown-math-formula", &mut cx);
    assert_eq!(
        file.read_with(&cx, |view, cx| view.live_edit.as_ref().unwrap().input.read(cx).value()),
        "a^2 + b^2"
    );
    select_all(&file, &mut cx);
    cx.simulate_input("x^2");
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-enter");
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(file.read_with(&cx, |view, _| view.live_edit.is_none()));
    assert!(cx.debug_bounds("markdown-math-formula").is_some());
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), "$$\nx^2\n$$");
}

#[gpui::test]
fn display_formula_and_reading_column_are_centered_in_a_wide_view(cx: &mut TestAppContext) {
    let (_, file, mut cx) = open("$$\nE = mc^2\n$$", cx);
    file.update(&mut cx, |view, cx| {
        view.show_details = false;
        cx.notify();
    });
    cx.simulate_resize(gpui::size(px(1400.0), px(900.0)));
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let viewport = cx.debug_bounds("markdown-preview-viewport").unwrap();
    let block = cx.debug_bounds("markdown-preview-block-0").unwrap();
    let formula = cx.debug_bounds("markdown-math-formula").unwrap();
    assert!(f32::from(block.center().x - viewport.center().x).abs() < 1.0);
    assert!(f32::from(formula.center().x - block.center().x).abs() < 1.0);
}

#[gpui::test]
fn inline_formula_itself_can_be_clicked_to_reveal_source(cx: &mut TestAppContext) {
    let source = "Formula: $x^2 + y^2$.";
    let (_, file, mut cx) = open(source, cx);
    click("markdown-math-formula", &mut cx);
    assert_eq!(
        file.read_with(&cx, |view, cx| view.live_edit.as_ref().unwrap().input.read(cx).value()),
        "$x^2 + y^2$"
    );
    assert!(!file.read_with(&cx, |view, _| view.dirty));
}

#[gpui::test]
fn virtual_scroll_keeps_active_input_identity_and_does_not_materialize_the_document(
    cx: &mut TestAppContext,
) {
    let source = (0..600).map(|i| format!("Paragraph {i}.\n\n")).collect::<String>();
    let (_, file, mut cx) = open(&source, cx);
    click("markdown-preview-block-0", &mut cx);
    let input = file.read_with(&cx, |view, _| view.live_edit.as_ref().unwrap().input.clone());
    cx.simulate_input("X");
    cx.run_until_parked();
    let draft = file.read_with(&cx, |view, cx| view.draft(cx));
    for item_ix in [599, 0] {
        cx.update(|window, cx| {
            file.update(cx, |view, cx| {
                view.scroll.scroll_to(gpui::ListOffset { item_ix, offset_in_item: px(0.0) });
                cx.notify();
            });
            let _ = window.draw(cx);
        });
        cx.run_until_parked();
    }
    assert!(
        file.read_with(&cx, |view, _| view
            .live_edit
            .as_ref()
            .is_some_and(|edit| edit.input == input))
    );
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), draft);
    assert!(file.read_with(&cx, |view, _| view.blocks.borrow().iter().flatten().count()) < 60);
}

#[gpui::test]
fn typing_markdown_creates_heading_list_and_code_in_place(cx: &mut TestAppContext) {
    let (_, file, mut cx) = open("", cx);
    click("markdown-preview-block-0", &mut cx);
    cx.simulate_input("# ");
    cx.run_until_parked();
    cx.simulate_input("Title");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), "# Title");
    assert_eq!(
        file.read_with(&cx, |view, cx| view.live_edit.as_ref().unwrap().input.read(cx).value()),
        "Title"
    );
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.simulate_input("- ");
    cx.run_until_parked();
    cx.simulate_input("Item");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), "# Title\n\n- Item");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.simulate_input("```rust");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.simulate_input("fn main() {}");
    cx.run_until_parked();
    assert_eq!(
        file.read_with(&cx, |view, cx| view.draft(cx)),
        "# Title\n\n- Item\n\n```rust\nfn main() {}\n```"
    );
}

#[gpui::test]
fn pasted_markdown_table_becomes_editable_cells_and_undo_restores_empty_document(
    cx: &mut TestAppContext,
) {
    let (_, file, mut cx) = open("", cx);
    click("markdown-preview-block-0", &mut cx);
    let source = "| A | B |\n| --- | --- |\n| 1 | 2 |";
    cx.update(|_, cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(source.to_owned()));
    });
    cx.simulate_keystrokes(match crate::platform::Platform::current() {
        crate::platform::Platform::MacOS => "cmd-v",
        _ => "ctrl-v",
    });
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), source);
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(cx.debug_bounds("markdown-part-0-3").is_some());
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), "");
}

#[gpui::test]
fn keyboard_moves_across_paragraphs_and_backspace_merges_with_undo(cx: &mut TestAppContext) {
    let (_, file, mut cx) = open("First\n\nSecond", cx);
    click("markdown-preview-block-0", &mut cx);
    file.update(&mut cx, |view, cx| {
        view.live_edit
            .as_ref()
            .unwrap()
            .input
            .update(cx, |input, cx| input.set_selected_range(5..5, cx));
    });
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, _| view.live_edit.as_ref().unwrap().block), 1);
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), "FirstSecond");
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), "First\n\nSecond");
}
