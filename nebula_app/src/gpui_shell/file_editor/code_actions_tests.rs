//! Dedicated code-block picker and clipboard feedback coverage.

use super::*;
use std::time::Duration;

use gpui::{Modifiers, TestAppContext, VisualTestContext, point, px};
use gpui_component::Root;

use crate::gpui_shell::copy_feedback::{COPY_FEEDBACK_TTL, CopyFeedback};

fn open(path: PathBuf, cx: &mut TestAppContext) -> (Entity<TextFileView>, VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        super::super::math_view::register(cx);
        init(cx);
    });
    let mut file = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| TextFileView::new(path, window, cx));
        view.update(cx, |view, _| view.live_mode = true);
        file = Some(view.clone());
        Root::new(view, window, cx)
    });
    window.run_until_parked();
    (file.unwrap(), window.clone())
}

fn press(key: &str, cx: &mut VisualTestContext) {
    let keystroke = gpui::Keystroke::parse(key).unwrap();
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    // Div's keyboard click is committed on release. simulate_keystrokes only
    // sends KeyDown, which cannot exercise the real click lifecycle.
    cx.simulate_event(gpui::KeyUpEvent { keystroke });
}

#[gpui::test]
fn read_only_language_picker_changes_highlighting_without_editing_source(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("reader-language.md");
    let source = format!("```text\nfn main() {{}}\n```\n\n{}", "Trailing paragraph\n\n".repeat(80));
    std::fs::write(&path, &source).unwrap();
    let (file, mut cx) = super::tests::open(path.clone(), cx);
    let draw = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    };
    for choice in ["rust", "plaintext"] {
        draw(&mut cx);
        let picker = cx.debug_bounds("markdown-language-picker").unwrap();
        cx.simulate_click(picker.center(), Modifiers::default());
        cx.run_until_parked();
        cx.simulate_keystrokes(match crate::platform::Platform::current() {
            crate::platform::Platform::MacOS => "cmd-a",
            _ => "ctrl-a",
        });
        cx.simulate_input(choice);
        cx.run_until_parked();
        press("enter", &mut cx);
        cx.run_until_parked();
        draw(&mut cx);
        file.read_with(&cx, |view, cx| {
            assert!(view.preview && !view.live_mode && view.live_edit.is_none());
            assert_eq!(view.draft(cx).as_ref(), source);
            assert!(!view.dirty);
            assert_eq!(
                view.preview_code_languages.get(&(0, 0)).map(|value| value.as_ref()),
                Some(if choice == "plaintext" { "" } else { choice }),
            );
        });
        if choice == "rust" {
            assert!(cx.debug_bounds("markdown-language-current-rust").is_some());
            // Virtualizing the code block must not discard this document's display choice.
            file.update(&mut cx, |view, cx| {
                view.scroll.scroll_to_reveal_item(70);
                cx.notify();
            });
            draw(&mut cx);
            file.update(&mut cx, |view, cx| {
                view.scroll.scroll_to_reveal_item(0);
                cx.notify();
            });
            draw(&mut cx);
            assert!(cx.debug_bounds("markdown-language-current-rust").is_some());
        }
        let code = cx.debug_bounds("pebrel-code-block").unwrap();
        cx.simulate_mouse_move(code.center(), None, Modifiers::default());
        draw(&mut cx);
        let copy = cx.debug_bounds("markdown-copy-code").unwrap();
        cx.simulate_click(copy.center(), Modifiers::default());
        cx.run_until_parked();
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap().trim_end(), "fn main() {}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
    }
}

#[gpui::test]
fn picker_trigger_popup_search_and_rows_keep_html_geometry(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("picker.md");
    std::fs::write(&path, "```text\nlet x = 42;\n```\n").unwrap();
    let (_file, mut cx) = open(path, cx);

    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let trigger = cx.debug_bounds("markdown-language-picker").unwrap();
    assert_eq!(trigger.size.height, px(28.0));
    let point = trigger.center();
    cx.simulate_mouse_down(point, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(point, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(cx.debug_bounds("markdown-language-popup").is_some());
    let popup = cx.debug_bounds("markdown-language-popup").unwrap();
    assert!(
        popup.top() >= trigger.bottom(),
        "the popup opens below its trigger when space permits"
    );
    assert!(cx.debug_bounds("markdown-language-search").unwrap().size.height >= px(32.0));
    cx.simulate_input("rust");
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(cx.debug_bounds("markdown-language-option-rust").is_some());
}

#[gpui::test]
fn language_picker_fits_a_resized_window_and_keeps_search_usable(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("picker-edges.md");
    std::fs::write(&path, "```text\nlet x = 42;\n```\n").unwrap();
    let (file, mut cx) = open(path, cx);
    cx.update(|_, cx| {
        file.update(cx, |view, cx| {
            view.show_details = false;
            cx.notify();
        })
    });
    cx.simulate_resize(gpui::size(px(420.0), px(320.0)));
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let trigger = cx.debug_bounds("markdown-language-picker").unwrap();
    cx.simulate_click(trigger.center(), Modifiers::default());
    cx.run_until_parked();

    for (width, height) in [(420.0, 320.0), (320.0, 220.0)] {
        cx.simulate_resize(gpui::size(px(width), px(height)));
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        cx.run_until_parked();
        let popup = cx.debug_bounds("markdown-language-popup").expect("open popup survives resize");
        assert!(popup.left() >= px(8.0) && popup.right() <= px(width - 8.0), "{popup:?}");
        assert!(popup.top() >= px(8.0) && popup.bottom() <= px(height - 8.0), "{popup:?}");
        let search = cx.debug_bounds("markdown-language-search").unwrap();
        assert!(
            search.top() >= popup.top() && search.bottom() <= popup.bottom(),
            "{search:?} outside {popup:?}"
        );
    }

    cx.simulate_input("rust");
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let option = cx.debug_bounds("markdown-language-option-rust").unwrap();
    let popup = cx.debug_bounds("markdown-language-popup").unwrap();
    assert!(option.top() >= popup.top() && option.bottom() <= popup.bottom());
    cx.simulate_click(option.center(), Modifiers::default());
    cx.run_until_parked();
    assert!(file.read_with(&cx, |view, cx| view.draft(cx).starts_with("```rust\n")));
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(cx.debug_bounds("markdown-language-popup").is_none());
}

#[gpui::test]
fn code_copy_feedback_keeps_control_visible_after_pointer_leaves(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("copy.md");
    std::fs::write(&path, "```rust\nlet x = 42;\n```\n").unwrap();
    let (_file, mut cx) = open(path, cx);

    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let block = cx.debug_bounds("pebrel-code-block").unwrap();
    cx.simulate_mouse_move(block.center(), None, Modifiers::default());
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let copy = cx.debug_bounds("markdown-copy-code").unwrap();
    let click_point = copy.center();
    cx.simulate_mouse_down(click_point, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(click_point, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(point(px(0.0), px(0.0)), None, Modifiers::default());
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(cx.debug_bounds("markdown-copy-code-success").is_some());
    assert_eq!(
        cx.read_from_clipboard().unwrap().text().unwrap().trim_end_matches('\n'),
        "let x = 42;"
    );
}

#[gpui::test]
fn language_picker_keeps_mouse_and_keyboard_activation_distinct(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("picker-focus.md");
    std::fs::write(&path, "```text\nlet x = 42;\n```\n").unwrap();
    let (_file, mut cx) = open(path, cx);

    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let trigger = cx.debug_bounds("markdown-language-picker").unwrap().center();
    cx.simulate_mouse_down(trigger, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(trigger, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    assert!(!cx.update(|window, _| window.last_input_was_keyboard()));

    // Closing by Escape restores the trigger focus. Enter must then activate
    // the same control through its keyboard click path, not through a mouse
    // focus side effect.
    press("escape", &mut cx);
    assert!(cx.update(|window, _| window.last_input_was_keyboard()));
    press("enter", &mut cx);
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(cx.debug_bounds("markdown-language-popup").is_some());
    press("escape", &mut cx);
    press("space", &mut cx);
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(cx.debug_bounds("markdown-language-popup").is_some());
}

#[gpui::test]
fn copy_feedback_state_expires_after_its_shared_ttl(cx: &mut TestAppContext) {
    let feedback = cx.update(|cx| cx.new(|_| CopyFeedback::new()));
    cx.update(|cx| feedback.update(cx, |feedback, cx| feedback.mark_copied(cx)));
    assert!(feedback.read_with(cx, |feedback, _| feedback.is_copied()));
    cx.run_until_parked();

    // The test dispatcher owns the clock, so this does not sleep or make the
    // test depend on wall-clock scheduling.
    cx.executor().advance_clock(COPY_FEEDBACK_TTL + Duration::from_millis(1));
    cx.run_until_parked();
    assert!(!feedback.read_with(cx, |feedback, _| feedback.is_copied()));
}
