use super::*;
use gpui::{TestAppContext, VisualTestContext};
use gpui_component::Root;

#[test]
fn link_destination_edit_preserves_nested_labels_titles_and_other_occurrences() {
    use super::link_actions::replace_destination;
    let source = r#"[a [nested] `](` \\*label*](https://old.test "title ]( &amp; tail")"#;
    let replacement = replace_destination(source, "https://new.test/?a=1&copy;=2").unwrap();
    assert!(replacement.starts_with(r#"[a [nested] `](` \\*label*](<"#));
    let ast = markdown::to_mdast(&replacement, &markdown::ParseOptions::gfm()).unwrap();
    let link = ast.children().unwrap()[0]
        .children()
        .unwrap()
        .iter()
        .find_map(
            |node| {
                if let markdown::mdast::Node::Link(link) = node { Some(link) } else { None }
            },
        )
        .unwrap();
    assert_eq!(link.url, "https://new.test/?a=1&copy;=2");
    assert_eq!(link.title.as_deref(), Some("title ]( & tail"));
    assert_eq!(
        replace_destination("[same][ref]", "https://new.test").unwrap(),
        "[same](<https://new.test>)"
    );
}

#[gpui::test]
fn rewritten_preview_edits_the_link_occurrence_after_matching_code_text(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("link-occurrence.md");
    let raw = "[same](https://example.com)";
    let source = format!("`{raw}` <img src=\"a.png\"> {raw} and {raw}");
    std::fs::write(&path, &source).unwrap();
    let (file, mut window) = open(path, cx);
    // 预览重写使偏移变化；代码里的同样语法不能计为一个真实链接。
    let rendered = source.replace("a.png", "https://example.com/expanded-image.png");
    let start = rendered.rfind(raw).unwrap();
    window.update(|_, cx| {
        file.update(cx, |view, cx| {
            view.update_link_hover(
                0,
                None,
                &rendered,
                Some(gpui_component::text::TextLinkHover {
                    url: "https://example.com".into(),
                    source_range: Some(start..start + raw.len()),
                    position: gpui::point(px(20.0), px(20.0)),
                    line_height: px(20.0),
                }),
                cx,
            );
            assert_eq!(
                view.link_hover.as_ref().unwrap().range,
                Some(source.rfind(raw).unwrap()..source.len())
            );
        })
    });
}

#[gpui::test]
fn link_hover_callbacks_release_evicted_text_views(cx: &mut TestAppContext) {
    for source in [
        "[link](https://example.com)",
        "- [link](https://example.com)",
        "[link](https://example.com) $x$",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hover-lifetime.md");
        std::fs::write(&path, source).unwrap();
        let (file, mut window) = open(path, cx);
        window.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        let views = file.read_with(&window, |view, _| {
            view.blocks
                .borrow()
                .iter()
                .flatten()
                .map(Entity::downgrade)
                .chain(view.inline_views.borrow().values().cloned())
                .collect::<Vec<_>>()
        });
        assert!(!views.is_empty());
        window.update(|window, cx| file.update(cx, |view, cx| view.toggle_preview(window, cx)));
        for _ in 0..2 {
            window.update(|window, cx| {
                window.draw(cx).clear(cx);
            });
        }
        assert!(
            views.iter().all(|view| view.upgrade().is_none()),
            "a hover callback must not retain its own text view"
        );
    }
}

#[gpui::test]
fn link_hover_actions_copy_and_open_the_complete_target_without_editing(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("link-actions.md");
    let url = format!("https://example.com/{}7", "123456".repeat(80));
    let source = format!("[visible link]({url})");
    std::fs::write(&path, &source).unwrap();
    let (file, mut cx) = open(path.clone(), cx);
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let block = cx.debug_bounds("markdown-preview-block-0").unwrap();
    cx.simulate_mouse_move(
        gpui::point(block.left() + px(20.0), block.top() + px(12.0)),
        None,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let copy = cx.debug_bounds("markdown-link-copy").expect("hover creates the action strip");
    cx.simulate_mouse_move(copy.center(), None, gpui::Modifiers::default());
    cx.simulate_click(copy.center(), gpui::Modifiers::default());
    assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), url);
    let open = cx.debug_bounds("markdown-link-open").unwrap();
    cx.simulate_click(open.center(), gpui::Modifiers::default());
    assert_eq!(cx.opened_url(), Some(url));
    file.read_with(&cx, |view, cx| {
        assert_eq!(view.draft(cx), source);
        assert!(!view.dirty);
    });
    assert_eq!(std::fs::read_to_string(path).unwrap(), source);
}

#[gpui::test]
fn link_edit_cancel_apply_and_reader_undo_change_only_the_selected_occurrence(
    cx: &mut TestAppContext,
) {
    struct DialogLayer;
    impl Render for DialogLayer {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().children(Root::render_dialog_layer(window, cx))
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("link-edit.md");
    let source = "[same][ref] and [same][ref]\n\n[ref]: https://old.example";
    std::fs::write(&path, source).unwrap();
    let (file, mut cx) = open(path.clone(), cx);
    cx.update(|window, cx| {
        cx.set_reduce_motion(true);
        let layer = cx.new(|_| DialogLayer);
        window.replace_root(cx, |window, cx| Root::new(layer, window, cx));
    });
    let start = source.rfind("[same][ref]").unwrap();
    for cancel in [true, false] {
        cx.update(|window, cx| {
            file.update(cx, |view, cx| {
                view.edit_link(
                    start..start + "[same][ref]".len(),
                    "https://old.example".into(),
                    view.content_revision,
                    window,
                    cx,
                );
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        let modifier = match crate::platform::Platform::current() {
            crate::platform::Platform::MacOS => "cmd",
            _ => "ctrl",
        };
        cx.simulate_keystrokes(&format!("{modifier}-a"));
        cx.simulate_input("https://new.example/full7");
        let button = cx
            .debug_bounds(if cancel { "confirm-dialog-cancel" } else { "confirm-dialog-ok" })
            .unwrap();
        cx.simulate_click(button.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        let expected = if cancel {
            source.to_owned()
        } else {
            source.replacen("and [same][ref]", "and [same](<https://new.example/full7>)", 1)
        };
        assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), expected);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            source,
            "link editing uses the normal unsaved document transaction"
        );
    }
    cx.update(|window, cx| {
        file.update(cx, |view, cx| {
            view.travel_history(false, window, cx);
        })
    });
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), source);
}

#[gpui::test]
fn reader_mode_segment_precedes_divider_and_tracks_selected_button(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("reader-mode.md");
    std::fs::write(&path, "# Reader").unwrap();
    cx.update(|cx| cx.set_reduce_motion(true));
    let (file, mut cx) = open(path, cx);
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let modes = cx.debug_bounds("file-view-modes").unwrap();
    let divider = cx.debug_bounds("file-toolbar-divider").unwrap();
    assert!(modes.right() < divider.left());
    let source = cx.debug_bounds("file-mode-source").unwrap();
    cx.simulate_click(source.center(), gpui::Modifiers::default());
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(!file.read(cx).preview);
    });
    let indicator = cx.debug_bounds("file-mode-indicator").unwrap();
    assert!((indicator.center().x - source.center().x).abs() < px(1.0));
    let reader = cx.debug_bounds("file-mode-reading").unwrap();
    cx.simulate_click(reader.center(), gpui::Modifiers::default());
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(file.read(cx).preview);
    });
    let indicator = cx.debug_bounds("file-mode-indicator").unwrap();
    assert!((indicator.center().x - reader.center().x).abs() < px(1.0));
}

#[gpui::test]
fn list_and_table_links_keep_the_same_hover_actions(cx: &mut TestAppContext) {
    for source in [
        "- [link](https://example.com/full7)",
        "| Name |\n| --- |\n| [link](https://example.com/full7) |",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("structured-link.md");
        std::fs::write(&path, source).unwrap();
        let (file, mut window) = open(path, cx);
        let part = file.read_with(&window, |view, _| {
            view.outline.structures[0]
                .as_ref()
                .unwrap()
                .parts
                .iter()
                .position(|part| source[part.range.clone()].contains("[link]"))
                .unwrap()
        });
        window.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        let selector = match part {
            0 => "markdown-part-0-0",
            1 => "markdown-part-0-1",
            _ => panic!("unexpected fixture part {part}"),
        };
        let bounds = window.debug_bounds(selector).unwrap();
        window.simulate_mouse_move(
            gpui::point(bounds.left() + px(20.0), bounds.top() + px(12.0)),
            None,
            gpui::Modifiers::default(),
        );
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        let copy = window.debug_bounds("markdown-link-copy").unwrap();
        window.simulate_click(copy.center(), gpui::Modifiers::default());
        assert_eq!(
            window.read_from_clipboard().unwrap().text().unwrap(),
            "https://example.com/full7"
        );
    }
}

pub(super) fn open(
    path: PathBuf,
    cx: &mut TestAppContext,
) -> (Entity<TextFileView>, VisualTestContext) {
    open_with_mode(path, false, cx)
}

/// Existing live-editor regressions opt in explicitly; the product opens in reader mode.
pub(super) fn open_live(
    path: PathBuf,
    cx: &mut TestAppContext,
) -> (Entity<TextFileView>, VisualTestContext) {
    open_with_mode(path, true, cx)
}

fn open_with_mode(
    path: PathBuf,
    live_mode: bool,
    cx: &mut TestAppContext,
) -> (Entity<TextFileView>, VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        super::super::math_view::register(cx);
        init(cx);
    });
    let mut file = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut view = TextFileView::new(path, window, cx);
            if live_mode {
                view.live_mode = true;
            }
            view
        });
        file = Some(view.clone());
        Root::new(view, window, cx)
    });
    window.run_until_parked();
    (file.unwrap(), window.clone())
}

#[gpui::test]
fn default_reader_blocks_edits_but_keeps_selection_and_code_copy(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("reader.md");
    let source = "# Title\n\nText to select\n\n- [ ] Task\n\n```rust\nlet x = 42;\n```\n";
    std::fs::write(&path, source).unwrap();
    let (file, mut cx) = open(path.clone(), cx);
    cx.simulate_resize(gpui::size(px(1100.0), px(1000.0)));
    for selector in ["markdown-preview-block-1", "markdown-task-box-2", "pebrel-code-text"] {
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        let bounds = cx.debug_bounds(selector).expect(selector);
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        cx.simulate_input("must not edit");
        cx.simulate_keystrokes("backspace");
        cx.run_until_parked();
        file.read_with(&cx, |view, cx| {
            assert!(view.preview && !view.live_mode && view.live_edit.is_none());
            assert_eq!(view.draft(cx), source);
            assert!(!view.dirty);
        });
    }
    assert!(cx.debug_bounds("markdown-language-picker").is_some());
    // Read-only applies to commands too, not just the visible controls.
    cx.update(|window, cx| {
        file.update(cx, |view, cx| {
            view.commit_structure_edit(0..1, "changed", None, window, cx);
            view.focus.focus(window, cx);
        })
    });
    cx.simulate_keystrokes("ctrl-a ctrl-c");
    cx.run_until_parked();
    assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), source);
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let code = cx.debug_bounds("pebrel-code-block").unwrap();
    cx.simulate_mouse_move(code.center(), None, gpui::Modifiers::default());
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let copy = cx.debug_bounds("markdown-copy-code").unwrap();
    cx.simulate_click(copy.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap().trim_end(), "let x = 42;");
    assert!(!file.read_with(&cx, |view, _| view.dirty));
    assert_eq!(std::fs::read_to_string(path).unwrap(), source);
}

#[gpui::test]
fn returning_to_reader_keeps_source_edits_and_blocks_undo_until_source_is_open(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source-edit.md");
    let source = "Original";
    std::fs::write(&path, source).unwrap();
    let (file, mut cx) = open(path, cx);
    cx.update(|window, cx| file.read(cx).focus.clone().focus(window, cx));
    let modifier = match crate::platform::Platform::current() {
        crate::platform::Platform::MacOS => "cmd",
        _ => "ctrl",
    };
    cx.simulate_keystrokes(&format!("{modifier}-/ {modifier}-a"));
    cx.simulate_input("Updated");
    cx.run_until_parked();
    cx.simulate_keystrokes(&format!("{modifier}-/ {modifier}-z"));
    cx.run_until_parked();
    file.read_with(&cx, |view, cx| {
        assert!(view.preview && !view.live_mode);
        assert_eq!(view.draft(cx), "Updated");
        assert!(view.dirty);
    });
    cx.simulate_keystrokes(&format!("{modifier}-/ {modifier}-z"));
    cx.run_until_parked();
    assert_eq!(file.read_with(&cx, |view, cx| view.draft(cx)), source);
}

#[gpui::test]
fn source_mode_releases_preview_views_without_changing_the_document(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preview.md");
    let source = "# Title\n\n$x^2$\n";
    std::fs::write(&path, source).unwrap();
    let (file, mut cx) = open(path, cx);
    cx.update(|window, cx| {
        file.update(cx, |view, cx| {
            assert!(view.preview && !view.loading);
            let block = cx.new(|cx| TextViewState::markdown("cached preview", cx));
            view.blocks.borrow_mut()[0] = Some(block);
            view.all_selected = true;
            view.preview_selection_scroll_active = true;
            let count = view.outline.blocks.len();
            view.toggle_preview(window, cx);
            assert!(!view.preview && !view.all_selected);
            assert!(!view.preview_selection_scroll_active);
            assert!(view.blocks.borrow().iter().all(Option::is_none));
            assert_eq!(view.input.read(cx).value().as_ref(), source);
            assert_eq!(view.outline.blocks.len(), count);
            assert!(!view.dirty);
            view.toggle_preview(window, cx);
            assert!(view.preview);
            assert!(view.blocks.borrow().iter().all(Option::is_none));
        });
    });
}

#[gpui::test]
fn keyboard_save_writes_the_file_and_conflicts_preserve_the_draft(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("code.rs");
    std::fs::write(&path, "fn original() {}\n").unwrap();
    let (file, mut cx) = open(path.clone(), cx);
    cx.update(|window, cx| {
        file.update(cx, |view, cx| {
            assert!(!view.loading);
            view.input.update(cx, |input, cx| {
                input.replace_all("fn edited() {}\n", window, cx);
                input.focus(window, cx);
            });
        })
    });
    cx.run_until_parked();
    assert!(file.read_with(&cx, |file, _| file.is_dirty()));
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "fn edited() {}\n");
    assert!(!file.read_with(&cx, |file, _| file.is_dirty()));

    cx.update(|window, cx| {
        file.update(cx, |view, cx| {
            view.input.update(cx, |input, cx| input.replace_all("my draft", window, cx));
        })
    });
    cx.run_until_parked();
    std::fs::write(&path, "external change").unwrap();
    cx.update(|window, cx| file.update(cx, |view, cx| view.reload(window, cx)));
    assert_eq!(file.read_with(&cx, |view, cx| view.input.read(cx).value().to_string()), "my draft");
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "external change");
    assert!(file.read_with(&cx, |view, _| view.is_dirty()
        && matches!(view.notice, Some((Message::EditorConflict, _)))));
}

#[gpui::test]
fn outline_arrow_folds_without_navigating_or_rewriting_the_document(cx: &mut TestAppContext) {
    use gpui::Modifiers;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tree.md");
    let text = "# Root\n\n## Child\n\n### Leaf\n\n## Sibling\n";
    std::fs::write(&path, text).unwrap();
    let (file, mut cx) = open(path.clone(), cx);
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let arrow = cx.debug_bounds("outline-fold-0").unwrap().center();
    cx.simulate_mouse_down(arrow, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(arrow, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    assert!(file.read_with(&cx, |view, _| view.collapsed_headings.contains(&0)));
    assert_eq!(file.read_with(&cx, |view, _| view.selected_heading), None);
    assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    assert_eq!(file.read_with(&cx, |view, cx| view.input.read(cx).value().to_string()), text);
}

#[gpui::test]
fn document_opens_read_only_and_source_shortcut_preserves_draft_and_details(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("chrome.md");
    let source = "# Title\n\nOriginal text\n";
    std::fs::write(&path, source).unwrap();
    let (file, mut cx) = open(path, cx);
    assert!(file.read_with(&cx, |view, _| view.preview && !view.live_mode));
    cx.update(|window, cx| file.read(cx).focus.clone().focus(window, cx));
    for preview in [false, true, false, true] {
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(cx.debug_bounds("file-mode-read").is_none());
        assert!(cx.debug_bounds("file-mode-live").is_none());
        cx.simulate_keystrokes("ctrl-/");
        cx.run_until_parked();
        assert_eq!(file.read_with(&cx, |view, _| view.preview), preview);
        assert_eq!(file.read_with(&cx, |view, cx| view.input.read(cx).value().to_string()), source);
        assert!(!file.read_with(&cx, |view, _| view.dirty));
    }
    for (selector, info) in
        [("file-info", true), ("file-info", true), ("file-toggle-outline", false)]
    {
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let bounds = cx.debug_bounds(selector).unwrap();
        assert!(bounds.size.height >= px(reader_presentation::PANEL_HEADER_HEIGHT - 1.0));
        assert!(bounds.size.width >= px(48.0));
        // Click the padding above the text, not only the label center.
        let point = gpui::point(bounds.center().x, bounds.origin.y + px(4.0));
        cx.simulate_mouse_down(point, MouseButton::Left, gpui::Modifiers::default());
        cx.simulate_mouse_up(point, MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();
        assert!(file.read_with(&cx, |view, _| view.show_details));
        assert_eq!(file.read_with(&cx, |view, _| view.info), info);
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(cx.debug_bounds("file-details-indicator").unwrap().size.height, px(2.0));
        if info {
            for action in ["info-copy-path", "info-copy-name", "info-reveal", "info-open"] {
                assert!(cx.debug_bounds(action).unwrap().size.height >= px(32.0));
            }
        }
    }
}

#[gpui::test]
fn code_actions_copy_raw_source_and_search_languages(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("code.md");
    let source = "```text\nlet x = 42;\n```\n";
    std::fs::write(&path, source).unwrap();
    let (file, mut cx) = open_live(path, cx);
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let block = cx.debug_bounds("markdown-preview-block-0").unwrap();
    let surface = cx.debug_bounds("pebrel-code-block").unwrap();
    let text = cx.debug_bounds("pebrel-code-text").unwrap();
    let language = cx.debug_bounds("markdown-language-picker").unwrap();
    assert_eq!(surface.size.height, text.size.height, "copy must not reserve a layout row");
    assert!(language.top() >= surface.top() && language.bottom() <= surface.top() + px(40.0));
    assert!((language.left() - surface.left() - px(10.0)).abs() < px(1.0));
    cx.simulate_mouse_move(block.center(), None, gpui::Modifiers::default());
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let copy = cx.debug_bounds("markdown-copy-code").unwrap().center();
    cx.simulate_mouse_move(copy, None, gpui::Modifiers::default());
    cx.simulate_mouse_down(copy, MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_up(copy, MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let copied = cx.read_from_clipboard().unwrap().text().unwrap();
    assert_eq!(copied.trim_end_matches('\n'), "let x = 42;");
    let picker = cx.debug_bounds("markdown-language-picker").unwrap().center();
    cx.simulate_mouse_down(picker, MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_up(picker, MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_input("rust");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(file.read_with(&cx, |view, _| view.outline.block_source(0).starts_with("```rust")));
    assert_eq!(
        file.read_with(&cx, |view, cx| view.input.read(cx).value().to_string()),
        source.replacen("```text", "```rust", 1)
    );
    assert!(file.read_with(&cx, |view, _| view.dirty));
}

#[gpui::test]
fn long_code_line_does_not_expand_the_reader_or_reserve_a_toolbar(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("long-code.md");
    std::fs::write(&path, format!("```text\n{}\n```\n", "x".repeat(2000))).unwrap();
    let (_, mut cx) = open(path, cx);
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let surface = cx.debug_bounds("pebrel-code-block").unwrap();
    let viewport = cx.debug_bounds("markdown-preview-viewport").unwrap();
    assert!(surface.size.width <= px(reader_presentation::PAGE_WIDTH));
    assert!(surface.right() <= viewport.right());
    assert!(
        surface.size.height < px(120.0),
        "one logical code line must scroll horizontally, not grow into a wrapped wall"
    );
}

#[gpui::test]
fn editing_during_a_save_stays_dirty_and_markdown_jumps_to_source(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.md");
    std::fs::write(&path, "# First\n\n## Second\ntext\n").unwrap();
    let (file, mut cx) = open(path.clone(), cx);
    cx.update(|window, cx| {
        file.update(cx, |view, cx| {
            assert_eq!(view.outline.headings.len(), 2);
            view.preview = false;
            view.jump_to_heading(1, window, cx);
            assert_eq!(view.input.read(cx).cursor_position().line, 2);
            view.input.update(cx, |input, cx| input.replace_all("saved snapshot", window, cx));
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        file.update(cx, |view, cx| {
            view.save(cx).detach();
            view.input.update(cx, |input, cx| input.replace_all("next draft", window, cx));
        })
    });
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "saved snapshot");
    assert!(file.read_with(&cx, |view, _| view.is_dirty()));
    assert_eq!(
        file.read_with(&cx, |view, cx| view.input.read(cx).value().to_string()),
        "next draft"
    );
}

#[gpui::test]
fn markdown_preview_and_outline_keep_visible_scrollbar_hosts(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("outline.md");
    std::fs::write(
        &path,
        (0..40).map(|index| format!("## Heading {index}\n\nBody {index}\n\n")).collect::<String>(),
    )
    .unwrap();
    let (_, mut cx) = open(path, cx);
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });

    assert_eq!(cx.debug_bounds("markdown-preview-scrollbar").unwrap().size.width, px(16.0));
    assert_eq!(cx.debug_bounds("markdown-outline-scrollbar").unwrap().size.width, px(16.0));
}

#[gpui::test]
fn reader_scrolls_over_both_margins_and_top_whitespace(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("margins.md");
    std::fs::write(&path, "Paragraph\n\n".repeat(80)).unwrap();
    let (file, mut cx) = open(path, cx);
    file.update(&mut cx, |view, cx| {
        view.show_details = false;
        cx.notify();
    });
    cx.simulate_resize(gpui::size(px(1100.0), px(600.0)));
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let viewport = cx.debug_bounds("markdown-preview-viewport").unwrap();
    let positions = [
        gpui::point(viewport.left() + px(4.0), viewport.center().y),
        gpui::point(viewport.right() - px(20.0), viewport.center().y),
        gpui::point(viewport.center().x, viewport.top() + px(4.0)),
    ];
    for position in positions {
        file.update(&mut cx, |view, cx| {
            view.scroll.scroll_to(gpui::ListOffset { item_ix: 0, offset_in_item: px(0.0) });
            cx.notify();
        });
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        assert!(!file.read_with(&cx, |view, _| view.scroll.viewport_bounds().contains(&position)));
        cx.simulate_event(gpui::ScrollWheelEvent {
            position,
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(-60.0))),
            modifiers: Default::default(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        cx.run_until_parked();
        let offset = file.read_with(&cx, |view, _| view.scroll.logical_scroll_top());
        assert!(offset.item_ix > 0 || offset.offset_in_item > px(0.0), "margin {position:?}");
    }
}

#[gpui::test]
fn long_display_formula_scrolls_horizontally_without_widening_the_page(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("long-formula.md");
    std::fs::write(&path, format!("$$\n{}x\n$$", "x + ".repeat(160))).unwrap();
    let (file, mut cx) = open(path, cx);
    file.update(&mut cx, |view, cx| {
        view.show_details = false;
        cx.notify();
    });
    cx.simulate_resize(gpui::size(px(600.0), px(600.0)));
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let viewport = cx.debug_bounds("markdown-preview-viewport").unwrap();
    let scroll = cx.debug_bounds("markdown-math-scroll").unwrap();
    let formula = cx.debug_bounds("markdown-math-formula").unwrap();
    assert!(scroll.left() >= viewport.left() && scroll.right() <= viewport.right());
    assert!(formula.size.width > scroll.size.width, "long formula keeps its natural size");
    assert!((formula.left() - scroll.left()).abs() < px(1.0), "the start remains reachable");
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: scroll.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(-120.0), px(0.0))),
        modifiers: Default::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert!(cx.debug_bounds("markdown-math-formula").unwrap().left() < formula.left());
}
