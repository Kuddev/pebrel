//! Native input, source preservation and cross-mode history regression tests.

use super::*;
use gpui::{EntityInputHandler, Modifiers, TestAppContext};

#[gpui::test]
fn unchanged_activation_skips_parse_without_losing_an_earlier_pending_edit(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("activation.md");
    std::fs::write(&path, "# Heading\n\n中文段落\n").unwrap();
    let (file, mut cx) = tests::open_live(path, cx);
    cx.update(|_, cx| file.update(cx, |view, cx| view.finish_live_edit(cx)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        file.update(cx, |view, cx| {
            let revision = view.revision;
            for _ in 0..10 {
                view.begin_live_edit(0, window, cx);
                view.finish_live_edit(cx);
                view.begin_live_edit(1, window, cx);
                view.finish_live_edit(cx);
            }
            assert_eq!(view.revision, revision, "unchanged activations must not schedule parsing");
            view.begin_live_edit(1, window, cx);
        });
    });
    cx.simulate_input("新内容");
    cx.run_until_parked();
    cx.update(|window, cx| {
        file.update(cx, |view, cx| {
            view.finish_live_edit(cx);
            assert!(view.preview_stale);
            view.begin_live_edit(0, window, cx);
            view.finish_live_edit(cx);
        });
    });
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_millis(300));
    cx.run_until_parked();
    file.read_with(&cx, |view, cx| {
        assert!(!view.preview_stale);
        assert!(view.outline.blocks[1].contains("新内容"));
        assert_eq!(
            view.source_slice(view.outline.source_ranges[1].clone(), cx).as_deref(),
            Some(view.outline.blocks[1].as_str())
        );
    });
}

#[gpui::test]
fn formatted_edit_saves_source_without_rewriting_other_blocks(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("inline.md");
    let original = "## 中文 **标题** ##\n\n[link](https://example.test \"title\")\n\n~~~rust\nlet n = 1;\n~~~\n";
    std::fs::write(&path, original).unwrap();
    let (file, mut cx) = tests::open_live(path.clone(), cx);
    let input = cx.update(|window, cx| {
        file.update(cx, |view, cx| {
            view.live_mode = true;
            view.begin_live_edit(0, window, cx);
            let edit = view.live_edit.as_ref().unwrap();
            assert_eq!(edit.input.read(cx).value(), "中文 标题");
            edit.input.update(cx, |input, cx| input.set_selected_range(7..13, cx));
            edit.input.clone()
        })
    });
    cx.simulate_input("新标题🌿");
    cx.run_until_parked();
    assert!(file.read_with(&cx, |view, _| view.live_edit.as_ref().unwrap().input == input));
    let expected = original.replacen("**标题**", "**新标题🌿**", 1);
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx).to_string()), expected);
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
    assert!(!file.read_with(&cx, |view, _| view.is_dirty()));
}

#[gpui::test]
fn undo_redo_crosses_formatted_source_and_finished_blocks(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("history.md");
    let original = "# Title\n\n**Body**\n";
    std::fs::write(&path, original).unwrap();
    let (file, mut cx) = tests::open_live(path, cx);
    cx.update(|window, cx| {
        file.update(cx, |view, cx| {
            view.live_mode = true;
            view.begin_live_edit(1, window, cx);
        })
    });
    cx.simulate_input("!");
    cx.run_until_parked();
    let changed = file.read_with(&cx, |view, cx| view.draft(cx).to_string());
    assert_ne!(changed, original);
    cx.simulate_keystrokes("ctrl-enter");
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx).to_string()), original);
    cx.update(|window, cx| file.update(cx, |view, cx| view.toggle_preview(window, cx)));
    cx.simulate_keystrokes("ctrl-shift-z");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx).to_string()), changed);
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx).to_string()), original);
}

#[gpui::test]
fn composition_keeps_native_input_identity_and_marked_selection(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ime.md");
    std::fs::write(&path, "**你好**\n").unwrap();
    let (file, mut cx) = tests::open_live(path, cx);
    let input = cx.update(|window, cx| {
        file.update(cx, |view, cx| {
            view.live_mode = true;
            view.begin_live_edit(0, window, cx);
            view.live_edit.as_ref().unwrap().input.clone()
        })
    });
    for (text, length) in [("s", 1), ("sh", 2), ("世", 1)] {
        cx.update(|window, cx| {
            input.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, text, Some(length..length), window, cx);
            })
        });
        cx.run_until_parked();
        assert!(file.read_with(&cx, |view, _| view.live_edit.as_ref().unwrap().input == input));
    }
    cx.update(|window, cx| input.update(cx, |input, cx| input.unmark_text(window, cx)));
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx).to_string()), "**你好世**\n");
    assert_eq!(input.read_with(&cx, |input, _| input.selected_range()), 9..9);
}

#[gpui::test]
fn composition_before_the_first_layout_keeps_its_caret(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("first-frame.md");
    std::fs::write(&path, "你好").unwrap();
    let (file, mut cx) = tests::open_live(path, cx);
    let input = cx.update(|window, cx| {
        let input = file.update(cx, |view, cx| {
            let click = gpui::ClickEvent::Mouse(gpui::MouseClickEvent {
                down: Default::default(),
                up: Default::default(),
            });
            view.begin_live_edit_at(0, Some(&click), window, cx);
            view.live_edit.as_ref().unwrap().input.clone()
        });
        input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "世", Some(1..1), window, cx);
        });
        let _ = window.draw(cx);
        window.simulate_next_frame(cx);
        input
    });
    cx.run_until_parked();
    assert_eq!(input.read_with(&cx, |input, _| input.selected_range()), 9..9);
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), "你好世");
}

#[gpui::test]
fn enter_creates_an_editable_paragraph_and_undo_restores_the_heading(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("paragraph.md");
    std::fs::write(&path, "# Title").unwrap();
    let (file, mut cx) = tests::open_live(path, cx);
    cx.update(|window, cx| {
        file.update(cx, |view, cx| {
            view.live_mode = true;
            view.begin_live_edit(0, window, cx);
        })
    });
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx).to_string()), "# Title\n\n");
    cx.simulate_input("New paragraph");
    cx.run_until_parked();
    assert_eq!(
        file.read_with(&cx, |view, cx| view.draft(cx).to_string()),
        "# Title\n\nNew paragraph"
    );
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx).to_string()), "# Title");
}

#[gpui::test]
fn clicking_near_the_start_places_the_caret_near_the_start(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("caret.md");
    std::fs::write(&path, "A paragraph with a long tail.").unwrap();
    let (file, mut cx) = tests::open_live(path, cx);
    cx.update(|window, cx| {
        file.update(cx, |view, cx| {
            view.live_mode = true;
            cx.notify();
        });
        let _ = window.draw(cx);
    });
    let bounds = cx.debug_bounds("markdown-preview-block-0").unwrap();
    let point = gpui::point(bounds.left() + px(2.0), bounds.top() + px(12.0));
    cx.simulate_mouse_down(point, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(point, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
        window.simulate_next_frame(cx);
    });
    cx.run_until_parked();
    let caret =
        file.read_with(&cx, |view, cx| view.live_edit.as_ref().unwrap().input.read(cx).cursor());
    assert!(caret <= 1, "click at {point:?}, block {bounds:?}, actual caret {caret}");
}

#[gpui::test]
fn first_click_uses_painted_glyphs_after_the_frame_callback(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("first-click.md");
    let source = "中文🌿 mixed text with a long tail for positioning.";
    std::fs::write(&path, source).unwrap();
    let (file, mut cx) = tests::open_live(path, cx);
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let bounds = cx.debug_bounds("markdown-preview-block-0").unwrap();
    let point = gpui::point(bounds.left() + px(108.0), bounds.top() + px(14.0));
    cx.simulate_mouse_down(point, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(point, MouseButton::Left, Modifiers::default());
    cx.update(|window, cx| {
        // 真实刷新先执行下一帧回调，再绘制刚刚挂载的 Input。
        window.simulate_next_frame(cx);
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    file.read_with(&cx, |view, cx| {
        let input = view.live_edit.as_ref().unwrap().input.read(cx);
        let caret = input.cursor();
        assert!(caret > 0 && caret < source.len(), "unexpected caret: {caret}");
        assert!(source.is_char_boundary(caret));
        let caret_bounds = input.range_to_bounds(&(caret..caret)).unwrap();
        assert!(
            f32::from(caret_bounds.left() - point.x).abs() <= 12.0,
            "click {point:?}, caret {caret_bounds:?}"
        );
    });
}

#[gpui::test]
fn activating_heading_preserves_following_paragraph_position(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let mut positions = Vec::new();
    for (scale, level) in
        [1.0, 1.25, 1.5, 2.0].into_iter().flat_map(|scale| (1..=6).map(move |level| (scale, level)))
    {
        let path = directory.path().join(format!("heading-{scale}-{level}.md"));
        std::fs::write(&path, format!("{} A heading\n\nFollowing paragraph", "#".repeat(level)))
            .unwrap();
        let (file, mut visual) = tests::open_live(path, cx);
        visual.update(|window, cx| {
            window.set_scale_factor(scale);
            file.update(cx, |_, cx| cx.notify());
            let _ = window.draw(cx);
        });
        let before = visual.debug_bounds("markdown-preview-block-1").unwrap().top();
        visual.update(|window, cx| {
            file.update(cx, |view, cx| view.begin_live_edit(0, window, cx));
            let _ = window.draw(cx);
        });
        visual.run_until_parked();
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let during = visual.debug_bounds("markdown-preview-block-1").unwrap().top();
        let live = visual.debug_bounds("markdown-live-block").unwrap();
        let line_height = file.read_with(&visual, |view, cx| {
            view.live_edit.as_ref().unwrap().input.read(cx).line_height()
        });
        visual.update(|window, cx| {
            file.update(cx, |view, cx| view.finish_live_edit(cx));
            let _ = window.draw(cx);
        });
        let after = visual.debug_bounds("markdown-preview-block-1").unwrap().top();
        positions.push((scale, level, before, during, after, live, line_height));
    }
    assert!(
        positions
            .iter()
            .all(|(_, _, before, during, after, _, _)| f32::from(*before - *during).abs() <= 0.1
                && f32::from(*before - *after).abs() <= 0.1),
        "heading activation changed document geometry: {positions:#?}"
    );
}
