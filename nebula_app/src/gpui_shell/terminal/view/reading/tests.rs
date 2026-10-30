use super::*;
use crate::gpui_shell::terminal::element::snapshot_probe;
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use gpui_component::{Root, WindowExt as _};
use nebula_terminal::event_loop::StreamProcessor;
use std::sync::mpsc::Receiver;

mod output_hover;

#[test]
#[ignore = "requires a native desktop and isolated PEBREL_READING_QA_DIR/config and source"]
fn native_result_desktop_visual_review() {
    use nebula_terminal::inline_content::Packet;
    use std::{path::PathBuf, time::Duration};

    let output = PathBuf::from(std::env::var_os("PEBREL_READING_QA_DIR").expect("QA output"));
    assert!(output.is_absolute());
    assert_eq!(
        std::env::var_os("PEBREL_CONFIG_DIR").map(PathBuf::from),
        Some(output.join("config"))
    );
    assert!(!output.join("ready.json").exists(), "use a fresh isolated fixture");
    let mime = std::env::var("PEBREL_READING_QA_MIME").expect("fixture MIME");
    assert!(matches!(mime.as_str(), "application/json" | "text/markdown"));
    let source = std::fs::read_to_string(output.join("source")).unwrap();
    let wire = format!(
        "{}\r\n{}{}\r\nFollowing terminal output",
        Packet::Begin { wire_id: 8000, rows: 2, mime, title: "Reading fixture".into() }.encode(),
        Packet::Append { wire_id: 8000, text: source }.encode(),
        Packet::End { wire_id: 8000, code: 0 }.encode()
    );
    gpui_platform::application().with_assets(crate::gpui_shell::assets::NebulaAssets).run(
        move |cx| {
            crate::gpui_shell::init(cx, None);
            let mut terminal = None;
            let mut receiver = None;
            let handle = cx
                .open_window(
                    gpui::WindowOptions {
                        window_bounds: Some(gpui::WindowBounds::Windowed(Bounds::new(
                            point(px(20.0), px(20.0)),
                            gpui::size(px(900.0), px(680.0)),
                        ))),
                        focus: false,
                        ..Default::default()
                    },
                    |window, cx| {
                        let view = cx.new(|cx| {
                            TerminalView::new(
                                8000,
                                (100, 32),
                                TerminalLaunch::Local {
                                    cwd: Some(output.clone()),
                                    shell: Some(nebula_terminal::tty::Shell::new(
                                        "pebrel-test-missing-shell-executable".into(),
                                        vec![],
                                    )),
                                    shell_name: None,
                                },
                                window,
                                cx,
                            )
                        });
                        receiver = Some(view.update(cx, |view, cx| {
                            // 复用 VT 公共入口和真实结果投影，不启动用户 Shell。
                            let (session, receiver, _, proxy) = session::test_session_with_events();
                            view.session = Some(session);
                            view.error = None;
                            view.exited = None;
                            view.copy_on_select = false;
                            view.ghost_enabled = false;
                            view.spawn_at = std::time::Instant::now() - Duration::from_secs(1);
                            StreamProcessor::default().feed(
                                &mut *view.session.as_ref().unwrap().term.lock(),
                                &proxy,
                                wire.as_bytes(),
                            );
                            view.observe_reading_output(cx);
                            receiver
                        }));
                        terminal = Some(view.clone());
                        cx.new(|cx| Root::new(view, window, cx))
                    },
                )
                .unwrap();
            let terminal = terminal.unwrap();
            let receiver = receiver.unwrap();
            cx.spawn(async move |cx| {
                for frame in 0..300 {
                    cx.background_executor().timer(Duration::from_millis(100)).await;
                    cx.update_window(handle.into(), |_, window, cx| {
                        window.refresh();
                        let _ = window.draw(cx);
                        for message in receiver.try_iter() {
                            if let Msg::Resize(size) | Msg::ResizeGrid(size) = message {
                                terminal.read(cx).session.as_ref().unwrap().term.lock().resize(
                                    nebula_terminal::term::test::TermSize::new(
                                        size.num_cols as usize,
                                        size.num_lines as usize,
                                    ),
                                );
                            }
                        }
                        if frame == 20 {
                            assert_eq!(terminal.read(cx).native_results.len(), 1);
                            std::fs::write(
                                output.join("ready.json"),
                                serde_json::to_vec(&serde_json::json!({"pid": std::process::id()}))
                                    .unwrap(),
                            )
                            .unwrap();
                        }
                    })
                    .unwrap();
                    if output.join("capture-complete").exists() {
                        break;
                    }
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        },
    );
}

#[gpui::test]
fn ordinary_cat_uses_native_layout_and_keeps_following_prompt(cx: &mut TestAppContext) {
    use nebula_terminal::grid::Dimensions as _;
    let (view, mut cx, _, proxy) = open(cx);
    let source = (0..80)
        .map(|i| format!("## 第 {i} 段\r\n\r\n中文与 English 正文。\r\n\r\n"))
        .collect::<String>();
    let wire = format!(
        "\x1b]1337;SetUserVar=pebrel_shell=cHdzaDox\x07\x1b]1337;SetUserVar=pebrel_command=cHdzaDoxCmNhdCBsb25nLm1k\x07\x1b]133;C\x07{source}\x1b]133;D;0\x07prompt>"
    );
    feed(&view, &mut cx, &proxy, wire.as_bytes());
    for _ in 0..5 {
        draw(&mut cx);
    }
    view.read_with(&cx, |view, cx| {
        assert_eq!(view.native_results.len(), 1);
        let result = view.native_results.entities.values().next().unwrap().read(cx);
        assert!(result.height() > px(600.0));
        let term = view.session.as_ref().unwrap().term.lock();
        let p = view.native_results.projection.borrow();
        let rows = p.source_rows(view.grid_rows());
        let snap = nebula_terminal::render::RenderSnapshot::capture_rows(
            &term,
            &nebula_terminal::render::SnapshotConfig {
                rows: rows as u16,
                cols: term.columns() as u16,
            },
            |row| !p.covers(row),
        );
        let cursor =
            snap.cursor.as_ref().expect("prompt must be included beyond the replaced source rows");
        assert!(
            p.row(cursor.row as f32) >= 0.0 && p.row(cursor.row as f32) < view.grid_rows() as f32
        );
        let painted = snap
            .segments
            .iter()
            .flat_map(|seg| seg.cells.iter().map(|cell| cell.text.as_str()))
            .collect::<String>();
        assert!(painted.contains("prompt>"), "{painted:?}");
        assert!(!painted.contains("第"), "native source must not be painted twice");
    });
}

#[gpui::test]
fn native_result_compact_hover_tools_and_copy_feedback(cx: &mut TestAppContext) {
    use nebula_terminal::inline_content::Packet;
    let (view, mut cx, receiver, proxy) = open(cx);
    let source = "fatal: missing repository\n";
    let wire = format!(
        "{}\r\n{}\r\n{}{}",
        Packet::Begin {
            wire_id: 90,
            rows: 16,
            mime: "text/x-git-progress".into(),
            title: "git status".into()
        }
        .encode(),
        Packet::Place { wire_id: 90, row: 1 }.encode(),
        Packet::AppendStderr { wire_id: 90, text: source.into() }.encode(),
        Packet::End { wire_id: 90, code: 128 }.encode()
    );
    feed(&view, &mut cx, &proxy, wire.as_bytes());
    draw(&mut cx);
    let bounds = cx.debug_bounds("native-result").unwrap();
    assert_eq!(bounds.size.height, view.read_with(&cx, |view, _| view.line_height * 2.0));
    assert!(cx.debug_bounds("native-result-tools").is_none());
    let hover = bounds.origin + point(px(10.0), px(5.0));
    cx.simulate_mouse_move(hover, None, Modifiers::default());
    draw(&mut cx);
    assert!(cx.debug_bounds("native-result-tools").is_some());
    assert!(cx.debug_bounds("native-result-hover-line").is_some());
    assert_eq!(cx.debug_bounds("native-result").unwrap(), bounds);
    let copy = cx.debug_bounds("native-result-copy").unwrap();
    assert!(copy.size.width > px(0.0) && copy.size.height > px(0.0), "{copy:?}");
    assert!(bounds.contains(&copy.center()), "button {copy:?} outside {bounds:?}");
    cx.simulate_mouse_move(copy.center(), None, Modifiers::default());
    draw(&mut cx);
    assert_eq!(
        cx.debug_bounds("native-result-copy"),
        Some(copy),
        "button moved or disappeared before click"
    );
    cx.simulate_click(copy.center(), Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(point(px(0.0), px(0.0)), None, Modifiers::default());
    draw(&mut cx);
    assert_eq!(clipboard(&mut cx).as_deref(), Some(source));
    assert!(cx.debug_bounds("native-result-tools").is_some());
    cx.executor().advance_clock(std::time::Duration::from_millis(1600));
    draw(&mut cx);
    assert!(cx.debug_bounds("native-result-tools").is_none());
    cx.simulate_mouse_move(hover, None, Modifiers::default());
    draw(&mut cx);
    let raw_button = cx.debug_bounds("native-result-raw").unwrap().center();
    cx.simulate_click(raw_button, Modifiers::default());
    draw(&mut cx);
    assert!(
        cx.debug_bounds("native-result-raw-content").unwrap().size.height >= bounds.size.height
    );
    let view_button = cx.debug_bounds("native-result-view").unwrap().center();
    cx.simulate_mouse_move(view_button, None, Modifiers::default());
    draw(&mut cx);
    cx.simulate_click(view_button, Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(point(px(0.0), px(0.0)), None, Modifiers::default());
    draw(&mut cx);
    assert!(cx.debug_bounds("native-result-tools").is_none(), "mouse focus must not pin tools");
    cx.simulate_keystrokes("right");
    draw(&mut cx);
    assert!(cx.debug_bounds("native-result-tools").is_some(), "keyboard focus reveals tools");
    cx.simulate_keystrokes("escape");
    draw(&mut cx);
    assert!(cx.debug_bounds("native-result-tools").is_none());
    assert!(input(&receiver).is_empty());
}

#[gpui::test]
fn native_result_mount_preserves_grid_and_isolates_pointer(cx: &mut TestAppContext) {
    use nebula_terminal::inline_content::Packet;
    let (view, mut cx, receiver, proxy) = open(cx);
    let packets = [
        Packet::Begin {
            wire_id: 89,
            rows: 12,
            mime: "text/markdown".into(),
            title: "Native Markdown".into(),
        },
        Packet::Append { wire_id: 89, text: "# Heading\n\n**Native** result".into() },
        Packet::End { wire_id: 89, code: 0 },
    ];
    let mut wire = packets[0].encode();
    for row in 1..12 {
        wire.push_str("\r\n");
        wire.push_str(&Packet::Place { wire_id: 89, row }.encode());
    }
    wire.push_str("\r\n");
    wire.extend(packets[1..].iter().map(Packet::encode));
    feed(&view, &mut cx, &proxy, wire.as_bytes());
    draw(&mut cx);
    assert_eq!(view.read_with(&cx, |view, _| view.native_results.len()), 1);
    let bounds = cx.debug_bounds("native-result").unwrap();
    for button in [MouseButton::Left, MouseButton::Middle, MouseButton::Right] {
        cx.simulate_mouse_down(bounds.center(), button, Modifiers::default());
        cx.simulate_mouse_up(bounds.center(), button, Modifiers::default());
    }
    draw(&mut cx);
    assert!(input(&receiver).is_empty());
    assert!(!view.read_with(&cx, |view, _| view.selecting));
    let heading = cx.debug_bounds("native-markdown-heading").unwrap();
    let start = heading.origin + point(px(1.0), heading.size.height * 0.5);
    let end = start + point(px(120.0), px(0.0));
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    draw(&mut cx);
    let selected = cx.update(|window, cx| window.selected_text(cx));
    assert!(selected.contains("Heading"), "Markdown drag selection: {selected:?}");
    cx.simulate_keystrokes(match crate::platform::Platform::current() {
        crate::platform::Platform::MacOS => "cmd-c",
        _ => "ctrl-c",
    });
    assert_eq!(clipboard(&mut cx).as_deref(), Some(selected.trim()));
    assert!(input(&receiver).is_empty());
    assert!(!view.read_with(&cx, |view, _| view.selecting));
    cx.simulate_resize(gpui::size(px(550.0), px(600.0)));
    draw(&mut cx);
    assert!(cx.debug_bounds("native-result").is_some());
    feed(&view, &mut cx, &proxy, b"\x1b[1;2HX");
    draw(&mut cx);
    assert!(view.read_with(&cx, |view, _| view.native_results.is_empty()));
    feed(&view, &mut cx, &proxy, b"\x1bc");
    draw(&mut cx);
    assert!(view.read_with(&cx, |view, _| view.native_results.is_empty()));
}

#[gpui::test]
fn native_result_long_document_uses_terminal_scroll_and_retains_identity(cx: &mut TestAppContext) {
    use nebula_terminal::inline_content::Packet;
    let (view, mut cx, receiver, proxy) = open(cx);
    let body = (0..80).map(|i| format!("paragraph {i}\n\n")).collect::<String>();
    let wire = format!(
        "{}\r\n{}{}prompt>",
        Packet::Begin { wire_id: 200, rows: 2, mime: "text/markdown".into(), title: "long".into() }
            .encode(),
        Packet::Append { wire_id: 200, text: body.clone() }.encode(),
        Packet::End { wire_id: 200, code: 0 }.encode()
    );
    feed(&view, &mut cx, &proxy, wire.as_bytes());
    for _ in 0..4 {
        draw(&mut cx);
    }
    let (id, entity, height) = view.read_with(&cx, |view, cx| {
        let (&id, entity) = view.native_results.entities.iter().next().unwrap();
        (id, entity.entity_id(), entity.read(cx).height())
    });
    assert!(height > px(600.0), "document must expand past the viewport: {height:?}");
    let (_, history) = view.read_with(&cx, |view, _| view.scroll_state());
    assert!(history > 80, "expanded output contributes to terminal scrollback");
    view.update(&mut cx, |view, cx| {
        view.scroll_to_offset(history, 0);
        cx.notify();
    });
    draw(&mut cx);
    let before = view.read_with(&cx, |view, _| view.scroll_state().0);
    let at = point(px(80.0), px(120.0));
    cx.simulate_mouse_move(at, None, Modifiers::default());
    cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: gpui::ScrollDelta::Lines(point(0.0, -3.0)),
        modifiers: Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    draw(&mut cx);
    assert!(view.read_with(&cx, |view, _| view.scroll_state().0) < before);
    view.update(&mut cx, |view, cx| {
        view.scroll_to_offset(history / 2, before);
        cx.notify();
    });
    draw(&mut cx);
    let anchor_y = cx.debug_bounds("native-result").unwrap().origin.y;
    // 滚动条可能按用户的 Hover 偏好隐藏；先走真实命中区，才能验收拖动。
    let edge = view.read_with(&cx, |view, _| {
        view.origin + point(view.cell_width * view.cols as f32 - px(2.0), px(100.0))
    });
    cx.simulate_mouse_move(edge, None, Modifiers::default());
    draw(&mut cx);
    let thumb = view.read_with(&cx, |view, _| {
        let (offset, history) = view.scroll_state();
        view.scrollbar_thumb(offset, history).unwrap()
    });
    assert!(cx.debug_bounds("native-result").unwrap().right() <= thumb.origin.x);
    cx.simulate_mouse_down(thumb.center(), MouseButton::Left, Modifiers::default());
    assert!(
        view.read_with(&cx, |view, _| view.scrollbar_drag.is_some()),
        "main scrollbar remains reachable beside native output"
    );
    cx.simulate_mouse_up(thumb.center(), MouseButton::Left, Modifiers::default());
    draw(&mut cx);
    cx.simulate_resize(gpui::size(px(700.0), px(600.0)));
    for _ in 0..4 {
        draw(&mut cx);
    }
    assert!(
        (cx.debug_bounds("native-result").unwrap().origin.y - anchor_y).abs() <= px(1.0),
        "resize must preserve the offset inside the same document"
    );
    feed(&view, &mut cx, &proxy, b"\x1b[?1049h");
    draw(&mut cx);
    view.update(&mut cx, |view, _| {
        view.session.as_ref().unwrap().term.lock().scroll_display(Scroll::Delta(1));
    });
    feed(&view, &mut cx, &proxy, b"\x1b[?1049l");
    for _ in 0..3 {
        draw(&mut cx);
    }
    assert!(
        (cx.debug_bounds("native-result").unwrap().origin.y - anchor_y).abs() <= px(1.0),
        "alternate-screen scrolling must preserve the primary document anchor"
    );
    view.update(&mut cx, |view, cx| {
        view.scroll_to_offset(0, before);
        cx.notify();
    });
    draw(&mut cx);
    assert_eq!(
        view.read_with(&cx, |view, _| view.native_results.entities[&id].entity_id()),
        entity
    );
    let prompt = view.read_with(&cx, |view, _| {
        let term = view.session.as_ref().unwrap().term.lock();
        let origin = term.viewport_origin_for(view.rows);
        let row = (term.grid().cursor.point.line - origin).0;
        view.origin
            + point(
                view.cell_width * 3.5,
                view.line_height * (view.native_results.projection.borrow().row(row as f32) + 0.5),
            )
    });
    let mapped = view.read_with(&cx, |view, _| view.grid_point(prompt).0);
    assert_eq!(mapped.line.0, 1, "prompt hit must resolve to the original VT row");
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            use nebula_terminal::render::{RenderSnapshot, SnapshotConfig};
            let snapshot = RenderSnapshot::capture(
                &*view.session.as_ref().unwrap().term.lock(),
                &SnapshotConfig { rows: view.rows as u16, cols: view.cols as u16 },
            );
            let cursor = snapshot.cursor.as_ref().unwrap();
            let row = view.native_results.projection.borrow().row(cursor.row as f32);
            let geometry = super::super::cursor::CursorGeometry {
                screen: (1, false, 0),
                grid: (view.cols, view.rows),
                metrics: (view.cell_width.as_f32(), view.line_height.as_f32(), 1.0),
                projection_shift: 0,
                row_projection_shift: row - cursor.row as f32,
                shape: cursor.shape,
            };
            view.cursor_animation.reset();
            let visual = view
                .visual_cursor(Some(cursor), cursor.col as usize, geometry, true, window, cx)
                .unwrap();
            assert!(
                (visual.row - f64::from(row)).abs() < 0.001,
                "2.2.2 smooth cursor must share the native result's vertical projection"
            );
        })
    });
    assert!(input(&receiver).is_empty());
    let mut costs = Vec::new();
    for step in 0..40 {
        let started = std::time::Instant::now();
        view.update(&mut cx, |view, cx| {
            let (current, total) = view.scroll_state();
            view.scroll_to_offset(total * (step % 20) / 20, current);
            cx.notify();
        });
        draw(&mut cx);
        costs.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    costs.sort_by(f64::total_cmp);
    crate::gpui_shell::try_write_stderr(format_args!(
        "native-result synthetic-window scroll: 40 frames, median={:.3} ms, max={:.3} ms\n",
        costs[costs.len() / 2],
        costs.last().unwrap()
    ));
}

#[gpui::test]
fn ordinary_directory_folds_only_middle_and_keeps_original_links(cx: &mut TestAppContext) {
    use base64::Engine as _;
    use nebula_terminal::grid::Dimensions as _;
    let (view, mut cx, receiver, proxy) = open_with_cache(cx, true);
    cx.simulate_resize(gpui::size(px(900.0), px(480.0)));
    draw(&mut cx);
    view.update(&mut cx, |view, _| {
        let mut config = crate::config::UiConfig::default();
        Arc::make_mut(&mut config.hints.enabled[0]).action =
            crate::config::ui_config::HintAction::Action(
                crate::config::ui_config::HintInternalAction::Copy,
            );
        view.hint_config = Arc::new(config);
    });
    let count: usize = 31;
    let source = (0..count)
        .map(|i| {
            format!("\x1b[32m\x1b]8;;file:///fold/{i}\x1b\\file-{i:02}\x1b]8;;\x1b\\\x1b[0m\r\n")
        })
        .collect::<String>();
    let owner = base64::engine::general_purpose::STANDARD.encode("bash:1");
    let command = base64::engine::general_purpose::STANDARD.encode("bash:1\nls -l");
    let wire = format!(
        "\x1b]1337;SetUserVar=pebrel_shell={owner}\x07\x1b]1337;SetUserVar=pebrel_command={command}\x07\x1b]133;C\x07{source}\x1b]133;D;0\x07prompt>"
    );
    feed(&view, &mut cx, &proxy, wire.as_bytes());
    for _ in 0..4 {
        draw(&mut cx);
    }
    assert!(
        view.read_with(&cx, |view, _| view.native_results.entities.is_empty()),
        "directory output must not become a native table"
    );
    assert!(cx.debug_bounds("native-result").is_none());
    view.read_with(&cx, |view, _| {
        let term = view.session.as_ref().unwrap().term.lock();
        let p = view.native_results.projection.borrow();
        let first = term.grid().cursor.point.line - count;
        let origin = term.viewport_origin_for(view.rows);
        for i in 0..count {
            let row = (first + i - origin).0 as usize;
            assert_eq!(p.covers(row), (10..count - 10).contains(&i), "source row {i}");
            let cell = &term.grid()[TermPoint::new(first + i, Column(0))];
            assert_eq!(cell.hyperlink().unwrap().uri(), format!("file:///fold/{i}"));
            assert_eq!(
                cell.fg,
                nebula_terminal::vte::ansi::Color::Named(
                    nebula_terminal::vte::ansi::NamedColor::Green
                )
            );
        }
        assert_eq!(p.row((first + (count - 1) - origin).0 as f32), 20.0);
        let cells = super::super::super::osc_links::dashed_cells_where(
            &term,
            &view.hint_config,
            p.source_rows(view.rows),
            view.cols,
            |row| !p.covers(row),
        );
        assert!(
            cells.contains_key(&(((first + (count - 1) - origin).0 as u16), 0)),
            "tail hyperlinks remain decorated"
        );
    });
    let position = |view: &Entity<TerminalView>, cx: &VisualTestContext, row: usize, col: f32| {
        view.read_with(cx, |view, _| {
            let term = view.session.as_ref().unwrap().term.lock();
            let line = term.grid().cursor.point.line - count + row;
            let source = (line - term.viewport_origin_for(view.rows)).0 as f32;
            point(
                view.origin.x + view.cell_width * col,
                view.origin.y
                    + view.line_height
                        * (view.native_results.projection.borrow().row(source) + 0.5),
            )
        })
    };
    cx.update(|window, cx| view.update(cx, |view, cx| view.open_reading(false, window, cx)));
    for row in [0, 15, 30] {
        view.update(&mut cx, |view, cx| {
            let term = view.session.as_ref().unwrap().term.lock();
            let line = (term.grid().scrolled_out() + term.history_size()) as i64
                + i64::from(term.grid().cursor.point.line.0)
                - count as i64
                + row as i64;
            view.reading.panel.as_mut().unwrap().highlight = Some(ReadingSpan {
                epoch: term.reading_epoch(),
                start: ReadingPoint { line: line as usize, column: 0 },
                end: ReadingPoint { line: line as usize, column: 7 },
            });
            cx.notify();
        });
        draw(&mut cx);
        let painted = cx.update(|_, cx| snapshot_probe::last(view.entity_id(), cx));
        if row == 15 {
            assert!(
                painted.reading_highlights.is_empty(),
                "hidden text must not highlight the fold control"
            );
        } else {
            assert_eq!(painted.reading_highlights.len(), 1, "visible source row {row}");
            let highlight = painted.reading_highlights[0];
            let expected = position(&view, &cx, row, 0.0);
            assert_eq!(highlight.origin.x, expected.x);
            assert_eq!(highlight.origin.y + highlight.size.height * 0.5, expected.y);
            assert_eq!(highlight.size.width, painted.cell_size.width * 7.0);
        }
    }
    cx.update(|window, cx| view.update(cx, |view, cx| view.close_reading(window, cx)));
    draw(&mut cx);
    let modifiers = if crate::platform::Platform::current() == crate::platform::Platform::MacOS {
        Modifiers { platform: true, ..Modifiers::default() }
    } else {
        Modifiers { control: true, ..Modifiers::default() }
    };
    for row in [0, 21, 30] {
        let point = position(&view, &cx, row, 0.25);
        cx.simulate_mouse_move(point, None, modifiers);
        draw(&mut cx);
        assert_eq!(
            view.read_with(&cx, |view, _| view
                .link_hover
                .as_ref()
                .unwrap()
                .hint
                .hyperlink()
                .unwrap()
                .uri()
                .to_owned()),
            format!("file:///fold/{row}")
        );
        cx.simulate_mouse_down(point, MouseButton::Left, modifiers);
        cx.simulate_mouse_up(point, MouseButton::Left, Modifiers::default());
        draw(&mut cx);
        assert_eq!(clipboard(&mut cx).unwrap(), format!("file:///fold/{row}"));
    }
    // 跨折叠区拖选仍复制完整原文，隐藏中段不是被删除的数据。
    let start = position(&view, &cx, 0, 0.25);
    let end = position(&view, &cx, count - 1, 6.75);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    draw(&mut cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| assert!(view.copy_selection(true, window, cx)))
    });
    assert_eq!(
        clipboard(&mut cx).unwrap(),
        (0..count).map(|i| format!("file-{i:02}")).collect::<Vec<_>>().join("\n")
    );
    // 控制行紧接前十行；用真实终端布局定位，避免查询缓存帧不会重放的 debug selector。
    let toggle = position(&view, &cx, 10, 4.0);
    let folded = cx.update(|_, cx| snapshot_probe::last(view.entity_id(), cx));
    assert!(!folded.text.contains("file-15"));
    cx.simulate_click(toggle, Modifiers::default());
    draw(&mut cx);
    let expanded = cx.update(|_, cx| snapshot_probe::last(view.entity_id(), cx));
    assert!(expanded.count > folded.count);
    assert!(expanded.text.contains("file-15"));
    let middle = position(&view, &cx, 15, 0.25);
    cx.simulate_mouse_move(middle, None, modifiers);
    draw(&mut cx);
    cx.simulate_mouse_down(middle, MouseButton::Left, modifiers);
    cx.simulate_mouse_up(middle, MouseButton::Left, Modifiers::default());
    draw(&mut cx);
    assert_eq!(clipboard(&mut cx).unwrap(), "file:///fold/15");
    cx.simulate_click(toggle, Modifiers::default());
    draw(&mut cx);
    let collapsed = cx.update(|_, cx| snapshot_probe::last(view.entity_id(), cx));
    assert!(collapsed.count > expanded.count);
    assert!(!collapsed.text.contains("file-15"));
    let prompt = position(&view, &cx, count, 0.25);
    view.read_with(&cx, |view, _| {
        assert_eq!(
            view.grid_point(prompt).0.line,
            view.session.as_ref().unwrap().term.lock().grid().cursor.point.line
        )
    });
    assert!(input(&receiver).is_empty());
}

#[gpui::test]
fn ordinary_short_directory_keeps_every_line_without_controls(cx: &mut TestAppContext) {
    use base64::Engine as _;
    let (view, mut cx, _, proxy) = open(cx);
    let owner = base64::engine::general_purpose::STANDARD.encode("pwsh:1");
    let command = base64::engine::general_purpose::STANDARD.encode("pwsh:1\nls");
    let source = (0..20).map(|i| format!("file-{i:02}\r\n")).collect::<String>();
    let wire = format!(
        "\x1b]1337;SetUserVar=pebrel_shell={owner}\x07\x1b]1337;SetUserVar=pebrel_command={command}\x07\x1b]133;C\x07{source}\x1b]133;D;0\x07prompt>"
    );
    feed(&view, &mut cx, &proxy, wire.as_bytes());
    for _ in 0..3 {
        draw(&mut cx);
    }
    assert!(cx.debug_bounds("terminal-output-fold-toggle").is_none());
    assert!(cx.debug_bounds("native-result").is_none());
    view.read_with(&cx, |view, _| {
        let p = view.native_results.projection.borrow();
        for row in 0..20 {
            assert!(!p.covers(row));
        }
    });
}

#[gpui::test]
fn native_result_revalidates_live_rows_that_scroll_into_history(cx: &mut TestAppContext) {
    use base64::Engine as _;
    use nebula_terminal::grid::Dimensions as _;
    let (view, mut cx, _, proxy) = open(cx);
    for column in [1, 2] {
        feed(&view, &mut cx, &proxy, b"\x1bc");
        let owner = base64::engine::general_purpose::STANDARD.encode("bash:1");
        let command = base64::engine::general_purpose::STANDARD.encode("bash:1\ncat sample.md");
        let wire = format!(
            "\x1b]1337;SetUserVar=pebrel_shell={owner}\x07\x1b]1337;SetUserVar=pebrel_command={command}\x07\x1b]133;C\x07one\r\ntwo\r\nthree\r\n\x1b]133;D;0\x07prompt>"
        );
        feed(&view, &mut cx, &proxy, wire.as_bytes());
        for _ in 0..4 {
            draw(&mut cx);
        }
        assert_eq!(view.read_with(&cx, |view, _| view.native_results.len()), 1);
        let (screen, epoch) = view.read_with(&cx, |view, _| {
            let term = view.session.as_ref().unwrap().term.lock();
            (term.screen_lines(), term.reading_epoch())
        });
        // 一次输入既覆盖中间行又把它滚入历史；col=2 保留 marker，须校验指纹。
        let rewrite = format!("\x1b[2;{column}HNEW\x1b[{screen};1H\r\n\r\n\r\n");
        feed(&view, &mut cx, &proxy, rewrite.as_bytes());
        for _ in 0..3 {
            draw(&mut cx);
        }
        view.read_with(&cx, |view, _| {
            let term = view.session.as_ref().unwrap().term.lock();
            assert_eq!(term.reading_epoch(), epoch);
            assert!(
                view.native_results.is_empty(),
                "stale output must not cover rewritten historical text; column {column}"
            );
        });
    }
}

#[gpui::test]
fn native_result_json_expands_all_rows_and_keeps_body_font(cx: &mut TestAppContext) {
    use nebula_terminal::inline_content::Packet;
    let (view, mut cx, _, proxy) = open(cx);
    let source = format!(
        "[{}]",
        (0..100).map(|i| format!("{{\"index\":{i}}}")).collect::<Vec<_>>().join(",")
    );
    let wire = format!(
        "{}\r\n{}{}",
        Packet::Begin {
            wire_id: 201,
            rows: 2,
            mime: "application/json".into(),
            title: "table".into()
        }
        .encode(),
        Packet::Append { wire_id: 201, text: source }.encode(),
        Packet::End { wire_id: 201, code: 0 }.encode()
    );
    feed(&view, &mut cx, &proxy, wire.as_bytes());
    for _ in 0..4 {
        draw(&mut cx);
    }
    view.read_with(&cx, |view, cx| {
        let result = view.native_results.entities.values().next().unwrap().read(cx);
        assert!(result.height() >= view.line_height * 100.0);
        assert_eq!(result.body_font_size(), view.font_size);
        let (_, _, projected_rows) = view.native_results.placements().into_iter().next().unwrap();
        assert!(
            (projected_rows * view.line_height.as_f32() - result.height().as_f32()).abs() < 0.01,
            "following terminal content must use the table's measured height without row padding"
        );
    });
}

#[gpui::test]
fn native_result_row_hover_notifies_inside_a_cached_parent(cx: &mut TestAppContext) {
    use nebula_terminal::inline_content::Packet;
    use std::{cell::Cell, rc::Rc};

    let (view, mut cx, _, proxy) = open_with_cache(cx, true);
    let wire = format!(
        "{}\r\n{}{}",
        Packet::Begin {
            wire_id: 203,
            rows: 2,
            mime: "text/x-directory-listing".into(),
            title: "hover".into()
        }
        .encode(),
        Packet::Append { wire_id: 203, text: "alpha\nbeta".into() }.encode(),
        Packet::End { wire_id: 203, code: 0 }.encode(),
    );
    feed(&view, &mut cx, &proxy, wire.as_bytes());
    let mut positions = None;
    for _ in 0..5 {
        draw(&mut cx);
        if let (Some(first), Some(second)) =
            (cx.debug_bounds("native-table-row-0"), cx.debug_bounds("native-table-row-1"))
        {
            positions = Some([first.center(), second.center()]);
        }
    }
    let [first, second] = positions.expect("actual table row geometry");
    let result = view
        .read_with(&cx, |view, _| view.native_results.entities.values().next().unwrap().clone());
    let notifications = Rc::new(Cell::new(0));
    let observed = notifications.clone();
    let _subscription =
        cx.update(|_, cx| cx.observe(&result, move |_, _| observed.set(observed.get() + 1)));
    cx.simulate_mouse_move(first, None, Modifiers::default());
    draw(&mut cx);
    for position in [second, first, second] {
        notifications.set(0);
        cx.simulate_mouse_move(position, None, Modifiers::default());
        // 先检查鼠标事件自身产生通知；强制 draw 会让缺失的 hover 失效机制看似正常。
        assert!(notifications.get() > 0, "crossing rows must notify before another forced draw");
        draw(&mut cx);
    }
}

#[gpui::test]
fn native_result_table_filter_and_sort_follow_real_controls(cx: &mut TestAppContext) {
    use nebula_terminal::inline_content::Packet;
    let (view, mut cx, receiver, proxy) = open(cx);
    let source = "-rw-r--r-- 2 Oct 25 12:00  alpha-long-filename.rs\n-rw-r--r-- 10 Oct 25 12:00  beta.rs\n-rw-r--r-- 1 Oct 25 12:00  gamma.rs";
    let wire = format!(
        "{}\r\n{}{}",
        Packet::Begin {
            wire_id: 202,
            rows: 2,
            mime: "text/x-powershell-directory".into(),
            title: "table".into()
        }
        .encode(),
        Packet::Append { wire_id: 202, text: source.into() }.encode(),
        Packet::End { wire_id: 202, code: 0 }.encode()
    );
    feed(&view, &mut cx, &proxy, wire.as_bytes());
    for _ in 0..5 {
        draw(&mut cx);
    }
    let rows = |view: &Entity<TerminalView>, cx: &VisualTestContext| {
        view.read_with(cx, |view, cx| {
            view.native_results
                .entities
                .values()
                .next()
                .unwrap()
                .read(cx)
                .displayed_rows()
                .into_iter()
                .map(|row| vec![row[3].clone(), row[1].clone()])
                .collect::<Vec<_>>()
        })
    };
    assert!(cx.debug_bounds("native-result-filter-input").is_none());
    let names = cx.debug_bounds("native-table-column-3").unwrap();
    let sizes = cx.debug_bounds("native-table-column-1").unwrap();
    assert!(names.size.width > sizes.size.width, "columns must fit their actual content");
    cx.simulate_click(
        sizes.origin + point(px(5.0), sizes.size.height - px(5.0)),
        Modifiers::default(),
    );
    draw(&mut cx);
    assert_eq!(rows(&view, &cx)[0], ["beta.rs", "10"]);
    // 目录列按 Name/Mode/Size 排列，使用真实 Tab 导航进入 Size 表头。
    cx.simulate_keystrokes("tab");
    draw(&mut cx);
    cx.simulate_keystrokes("tab");
    draw(&mut cx);
    cx.simulate_keystrokes("tab");
    draw(&mut cx);
    cx.simulate_keystrokes("enter");
    cx.simulate_event(gpui::KeyUpEvent { keystroke: gpui::Keystroke::parse("enter").unwrap() });
    draw(&mut cx);
    assert_eq!(rows(&view, &cx)[0], ["gamma.rs", "1"]);
    let bounds = cx.debug_bounds("native-result").unwrap();
    cx.simulate_mouse_move(bounds.origin + point(px(10.0), px(10.0)), None, Modifiers::default());
    draw(&mut cx);
    let filter = cx.debug_bounds("native-result-filter-toggle").unwrap().center();
    cx.simulate_mouse_move(filter, None, Modifiers::default());
    draw(&mut cx);
    cx.simulate_click(filter, Modifiers::default());
    draw(&mut cx);
    assert!(cx.debug_bounds("native-result-filter-input").is_some());
    cx.simulate_input("beta");
    draw(&mut cx);
    assert_eq!(rows(&view, &cx), vec![vec!["beta.rs".to_owned(), "10".to_owned()]]);
    cx.simulate_keystrokes("escape");
    draw(&mut cx);
    assert!(cx.debug_bounds("native-result-filter-input").is_none());
    assert_eq!(rows(&view, &cx).len(), 3);
    assert!(input(&receiver).is_empty());
}

#[gpui::test]
fn native_result_json_tree_folds_filters_and_selects_without_pty_input(cx: &mut TestAppContext) {
    use nebula_terminal::inline_content::Packet;
    let (view, mut cx, receiver, proxy) = open(cx);
    let source = r#"{"code":200,"message":"success","data":{"tags":["vip","new-user"],"address":{"province":"广东  省 <&>","city":"深圳市","street":"科技路"},"orders":[{"orderId":"A20240101"},{"orderId":"A20240102"}]}}"#;
    let wire = format!(
        "{}\r\n{}{}",
        Packet::Begin {
            wire_id: 209,
            rows: 2,
            mime: "application/json".into(),
            title: "JSON".into()
        }
        .encode(),
        Packet::Append { wire_id: 209, text: source.into() }.encode(),
        Packet::End { wire_id: 209, code: 0 }.encode()
    );
    feed(&view, &mut cx, &proxy, wire.as_bytes());
    for _ in 0..6 {
        draw(&mut cx);
    }
    assert!(cx.debug_bounds("native-json-toggle-3").is_some());
    assert!(cx.debug_bounds("native-json-toggle-4").is_none(), "short array stays inline");
    assert!(cx.debug_bounds("native-json-row-16").is_none(), "address starts collapsed");
    let toggle = cx.debug_bounds("native-json-toggle-7").unwrap().center();
    cx.simulate_click(toggle, Modifiers::default());
    for _ in 0..3 {
        draw(&mut cx);
    }
    let text = cx.debug_bounds("native-json-text-16").unwrap();
    let start = text.origin + point(px(1.0), text.size.height * 0.5);
    let city = cx.debug_bounds("native-json-text-18").unwrap();
    let end = city.origin + point(px(220.0), city.size.height * 0.5);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    draw(&mut cx);
    let selected = cx.update(|window, cx| window.selected_text(cx));
    assert!(selected.contains("province"), "JSON drag selection: {selected:?}");
    assert!(selected.contains("广东  省 <&>"), "JSON whitespace and escaping: {selected:?}");
    assert!(selected.contains("city"), "JSON selection crosses text rows: {selected:?}");
    cx.simulate_keystrokes(match crate::platform::Platform::current() {
        crate::platform::Platform::MacOS => "cmd-c",
        _ => "ctrl-c",
    });
    assert_eq!(clipboard(&mut cx).as_deref(), Some(selected.trim()));
    let bounds = cx.debug_bounds("native-result").unwrap();
    cx.simulate_mouse_move(bounds.origin + point(px(5.0), px(5.0)), None, Modifiers::default());
    draw(&mut cx);
    let raw = cx.debug_bounds("native-result-raw").unwrap().center();
    cx.simulate_click(raw, Modifiers::default());
    draw(&mut cx);
    let preview = cx.debug_bounds("native-result-view").unwrap().center();
    cx.simulate_click(preview, Modifiers::default());
    draw(&mut cx);
    assert!(cx.debug_bounds("native-json-row-16").is_some(), "fold state survives source mode");
    let filter = cx.debug_bounds("native-result-filter-toggle").unwrap().center();
    cx.simulate_click(filter, Modifiers::default());
    draw(&mut cx);
    cx.simulate_input("A20240102");
    for _ in 0..3 {
        draw(&mut cx);
    }
    assert!(cx.debug_bounds("native-json-row-30").is_some());
    assert!(cx.debug_bounds("native-json-row-26").is_none());
    cx.simulate_keystrokes("escape");
    draw(&mut cx);
    assert!(cx.debug_bounds("native-json-row-16").is_some());
    assert!(input(&receiver).is_empty());
    assert!(!view.read_with(&cx, |view, _| view.selecting));
}

#[gpui::test]
fn native_result_markdown_keeps_compact_blocks_and_copies_only_code(cx: &mut TestAppContext) {
    use nebula_terminal::inline_content::Packet;
    let (view, mut cx, receiver, proxy) = open(cx);
    let code = "# a comment\nprintf 'hello'";
    let source = format!(
        "# Heading\n\nParagraph with `code`.\n\n```bash\n{code}\n```\n\n| Name | Kind | 操作 |\n| --- | --- | --- |\n| alpha | text | 选择与复制 |\n| bravo | bytes | 排序与筛选 |\n"
    );
    let wire = format!(
        "{}\r\n{}{}",
        Packet::Begin {
            wire_id: 203,
            rows: 2,
            mime: "text/markdown".into(),
            title: "document".into()
        }
        .encode(),
        Packet::Append { wire_id: 203, text: source }.encode(),
        Packet::End { wire_id: 203, code: 0 }.encode()
    );
    feed(&view, &mut cx, &proxy, wire.as_bytes());
    for _ in 0..10 {
        draw(&mut cx);
    }
    let heading = cx.debug_bounds("native-markdown-heading").unwrap();
    let code_block = cx.debug_bounds("native-code-block").unwrap();
    let table = cx.debug_bounds("native-markdown-table").unwrap();
    assert!(heading.top() < code_block.top() && code_block.bottom() <= table.top());
    assert!(code_block.size.height < px(140.0), "a short code block must fit its content");
    let one_line_rows = view
        .read_with(&cx, |view, _| px((view.font_size.as_f32() * 1.65 + 12.0).ceil() * 3.0 + 3.0));
    assert!(table.size.height <= one_line_rows, "short CJK cells must fit their measured columns");
    cx.simulate_mouse_move(code_block.center(), None, Modifiers::default());
    draw(&mut cx);
    let copy = cx.debug_bounds("native-code-copy").unwrap().center();
    cx.simulate_click(copy, Modifiers::default());
    draw(&mut cx);
    assert_eq!(
        clipboard(&mut cx).as_deref(),
        Some(code),
        "no fence, language or line numbers in copied code"
    );
    assert_eq!(cx.debug_bounds("native-code-block"), Some(code_block));
    let identities = |view: &Entity<TerminalView>, cx: &VisualTestContext| {
        view.read_with(cx, |view, cx| {
            view.native_results.entities.values().next().unwrap().read(cx).code_identities()
        })
    };
    let retained = identities(&view, &cx);
    assert_eq!(retained.len(), 1);
    feed(&view, &mut cx, &proxy, b"\x1b[?1049h");
    draw(&mut cx);
    feed(&view, &mut cx, &proxy, b"\x1b[?1049l");
    for _ in 0..5 {
        draw(&mut cx);
    }
    assert_eq!(
        identities(&view, &cx),
        retained,
        "offscreen code must retain language and parsing state"
    );
    cx.simulate_mouse_move(point(px(0.0), px(0.0)), None, Modifiers::default());
    draw(&mut cx);
    assert!(input(&receiver).is_empty());
}

#[gpui::test]
fn native_result_diff_keeps_file_boundaries_copy_and_collapse(cx: &mut TestAppContext) {
    use nebula_terminal::inline_content::Packet;
    let (view, mut cx, receiver, proxy) = open(cx);
    let source = "diff --git a/src/config.ts b/src/config.ts\n--- a/src/config.ts\n+++ b/src/config.ts\n@@ -12,3 +12,4 @@\n export const output = {\n-  format: 'text',\n+  format: 'auto',\n+  preserveSource: true,\n };\ndiff --git a/old b/new\nsimilarity index 100%\nrename from old\nrename to new\n";
    let wire = format!(
        "{}\r\n{}{}",
        Packet::Begin {
            wire_id: 204,
            rows: 2,
            mime: "text/x-diff".into(),
            title: "git diff".into()
        }
        .encode(),
        Packet::Append { wire_id: 204, text: source.into() }.encode(),
        Packet::End { wire_id: 204, code: 0 }.encode()
    );
    feed(&view, &mut cx, &proxy, wire.as_bytes());
    for _ in 0..10 {
        draw(&mut cx);
    }
    let expanded = cx.debug_bounds("native-diff-file-0").unwrap();
    assert!(cx.debug_bounds("native-diff-body-0").is_some());
    assert!(cx.debug_bounds("native-diff-file-1").unwrap().top() >= expanded.bottom());
    assert!(
        cx.debug_bounds("native-code-copy").is_none(),
        "diff uses the original patch copy action"
    );
    let collapse = cx.debug_bounds("native-diff-collapse-0").unwrap();
    assert!(collapse.size.width >= px(28.0) && collapse.size.height >= px(28.0));
    cx.simulate_mouse_move(collapse.center(), None, Modifiers::default());
    draw(&mut cx);
    assert!(
        !cx.debug_bounds("native-result-tools").unwrap().intersects(&collapse),
        "floating tools must leave the file collapse target reachable"
    );
    cx.simulate_click(collapse.origin + point(px(3.0), px(3.0)), Modifiers::default());
    for _ in 0..4 {
        draw(&mut cx);
    }
    assert!(cx.debug_bounds("native-diff-body-0").is_none());
    assert!(cx.debug_bounds("native-diff-file-0").unwrap().size.height < expanded.size.height);
    // 容器焦点不是 Tab 停靠点；第一个 Tab 就进入第一个文件的折叠按钮。
    cx.simulate_keystrokes("tab");
    draw(&mut cx);
    cx.simulate_keystrokes("enter");
    cx.simulate_event(gpui::KeyUpEvent { keystroke: gpui::Keystroke::parse("enter").unwrap() });
    for _ in 0..4 {
        draw(&mut cx);
    }
    assert!(
        input(&receiver).is_empty(),
        "file control navigation must not send Tab or Enter to PTY"
    );
    assert!(
        cx.debug_bounds("native-diff-body-0").is_some(),
        "keyboard activation expands the file"
    );
    cx.simulate_keystrokes("enter");
    cx.simulate_event(gpui::KeyUpEvent { keystroke: gpui::Keystroke::parse("enter").unwrap() });
    for _ in 0..4 {
        draw(&mut cx);
    }
    assert!(
        cx.debug_bounds("native-diff-body-0").is_none(),
        "keyboard focus survives the first activation"
    );
    let bounds = cx.debug_bounds("native-result").unwrap();
    cx.simulate_mouse_move(bounds.origin + point(px(10.0), px(10.0)), None, Modifiers::default());
    draw(&mut cx);
    let copy = cx.debug_bounds("native-result-copy").unwrap().center();
    cx.simulate_click(copy, Modifiers::default());
    draw(&mut cx);
    assert_eq!(
        clipboard(&mut cx).as_deref(),
        Some(source),
        "copy retains headers, hunk markers, and original signs"
    );
    cx.simulate_keystrokes("escape");
    draw(&mut cx);
    assert!(input(&receiver).is_empty());
}

#[gpui::test]
fn native_result_unsupported_diff_preserves_raw_output(cx: &mut TestAppContext) {
    use nebula_terminal::inline_content::Packet;
    let (view, mut cx, receiver, proxy) = open(cx);
    let source = "diff --cc file\n@@@ -1,1 -1,1 +1,1 @@@\n++new\n";
    let wire = format!(
        "{}\r\n{}{}",
        Packet::Begin {
            wire_id: 205,
            rows: 2,
            mime: "text/x-diff".into(),
            title: "git diff".into()
        }
        .encode(),
        Packet::Append { wire_id: 205, text: source.into() }.encode(),
        Packet::End { wire_id: 205, code: 0 }.encode()
    );
    feed(&view, &mut cx, &proxy, wire.as_bytes());
    for _ in 0..6 {
        draw(&mut cx);
    }
    assert!(cx.debug_bounds("native-diff-file-0").is_none());
    assert!(cx.debug_bounds("native-result-raw-content").is_some());
    let bounds = cx.debug_bounds("native-result").unwrap();
    cx.simulate_mouse_move(bounds.origin + point(px(10.0), px(10.0)), None, Modifiers::default());
    draw(&mut cx);
    let copy = cx.debug_bounds("native-result-copy").unwrap().center();
    cx.simulate_click(copy, Modifiers::default());
    draw(&mut cx);
    assert_eq!(clipboard(&mut cx).as_deref(), Some(source));
    assert!(input(&receiver).is_empty());
}

#[gpui::test]
fn native_result_eviction_releases_offscreen_parsed_state(cx: &mut TestAppContext) {
    use nebula_terminal::inline_content::Packet;
    let (view, mut cx, _, proxy) = open(cx);
    let mut stream = StreamProcessor::default();
    let mut first = None;
    for id in 0..34 {
        let wire = format!(
            "{}\r\n{}{}",
            Packet::Begin {
                wire_id: id,
                rows: 2,
                mime: "text/markdown".into(),
                title: "retention".into()
            }
            .encode(),
            Packet::Append { wire_id: id, text: format!("# Result {id}\n\ncontent") }.encode(),
            Packet::End { wire_id: id, code: 0 }.encode()
        );
        view.update(&mut cx, |view, cx| {
            stream.feed(&mut view.session.as_ref().unwrap().term.lock(), &proxy, wire.as_bytes());
            cx.notify();
        });
        for _ in 0..3 {
            draw(&mut cx);
        }
        if id == 0 {
            first =
                view.read_with(&cx, |view, _| view.native_results.entities.keys().next().copied());
        }
    }
    view.read_with(&cx, |view, _| {
        assert!(!view.native_results.entities.contains_key(&first.unwrap()));
        assert!(view.native_results.entities.len() <= 32);
    });
}

struct CachedTerminal(Entity<TerminalView>);
impl Render for CachedTerminal {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // 与工作区相同的缓存边界；不以整窗 refresh 掩盖终端漏发通知。
        div().size_full().child(self.0.clone().cached(gpui::StyleRefinement::default().size_full()))
    }
}

fn open(
    cx: &mut TestAppContext,
) -> (Entity<TerminalView>, VisualTestContext, Receiver<Msg>, session::EventProxy) {
    open_with_cache(cx, false)
}

fn open_with_cache(
    cx: &mut TestAppContext,
    cached: bool,
) -> (Entity<TerminalView>, VisualTestContext, Receiver<Msg>, session::EventProxy) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::terminal::init(cx);
        crate::gpui_shell::math_view::register(cx);
        snapshot_probe::enable(cx);
        cx.set_global(Settings::load(nebula_settings::ThemeName::Nord));
    });
    let mut result = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            TerminalView::new(
                42,
                (80, 24),
                TerminalLaunch::Local {
                    cwd: None,
                    shell: Some(nebula_terminal::tty::Shell::new(
                        "pebrel-test-missing-shell-executable".into(),
                        vec![],
                    )),
                    shell_name: None,
                },
                window,
                cx,
            )
        });
        let (receiver, proxy) = view.update(cx, |view, _| {
            let (session, receiver, _, proxy) = session::test_session_with_events();
            view.session = Some(session);
            view.error = None;
            view.exited = None;
            view.copy_on_select = false;
            view.ghost_enabled = false;
            // 布局仍走真实路径；跳过启动宽限期避免定时器掩盖 resize 断言。
            view.spawn_at = std::time::Instant::now() - std::time::Duration::from_secs(1);
            (receiver, proxy)
        });
        let focus_handle = view.read(cx).focus_handle.clone();
        window.focus(&focus_handle, cx);
        result = Some((view.clone(), receiver, proxy));
        if cached {
            Root::new(cx.new(|_| CachedTerminal(view)), window, cx)
        } else {
            Root::new(view, window, cx)
        }
    });
    let mut window = window.clone();
    window.simulate_resize(gpui::size(px(900.0), px(600.0)));
    window.update(|window, _| window.activate_window());
    draw(&mut window);
    let (view, receiver, proxy) = result.unwrap();
    // Flush the real initial geometry before measuring overlay-only changes.
    view.update(&mut window, |view, _| view.pending_resize = None);
    receiver.try_iter().for_each(drop);
    (view, window, receiver, proxy)
}
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
}
fn feed(
    view: &Entity<TerminalView>,
    cx: &mut VisualTestContext,
    proxy: &session::EventProxy,
    bytes: &[u8],
) {
    view.update(cx, |view, cx| {
        StreamProcessor::default().feed(
            &mut *view.session.as_ref().unwrap().term.lock(),
            proxy,
            bytes,
        );
        view.observe_reading_output(cx);
        cx.notify();
    });
}
fn input(receiver: &Receiver<Msg>) -> Vec<u8> {
    receiver
        .try_iter()
        .filter_map(|message| match message {
            Msg::Input(bytes) => Some(bytes.into_owned()),
            _ => None,
        })
        .flatten()
        .collect()
}
fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
}
fn execution(text: &str) -> String {
    format!("\x1b]133;A\x07$ \x1b]133;B\x07echo test\r\n\x1b]133;C\x07{text}\x1b]133;D;0\x07")
}

#[gpui::test]
fn reading_overlay_stays_inside_a_narrow_pane(cx: &mut TestAppContext) {
    let (view, mut cx, _, _) = open(cx);
    cx.simulate_resize(gpui::size(px(300.0), px(360.0)));
    draw(&mut cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| view.open_reading(false, window, cx));
    });
    draw(&mut cx);
    let bounds = cx.debug_bounds("terminal-reading-overlay").unwrap();
    assert!(bounds.origin.x >= px(0.0));
    assert!(bounds.origin.x + bounds.size.width <= px(300.0));
}

#[gpui::test]
fn search_tab_traverses_controls_without_sending_terminal_tab(cx: &mut TestAppContext) {
    let (view, mut cx, receiver, _) = open(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| view.open_reading(false, window, cx));
    });
    draw(&mut cx);
    let input_focus = view.read_with(&cx, |view, cx| {
        view.reading.panel.as_ref().unwrap().input.read(cx).focus_handle(cx)
    });
    assert!(cx.update(|window, _| input_focus.is_focused(window)));
    cx.simulate_keystrokes("tab");
    draw(&mut cx);
    assert!(!cx.update(|window, _| input_focus.is_focused(window)));
    assert!(input(&receiver).is_empty());
    cx.simulate_keystrokes("shift-tab");
    draw(&mut cx);
    assert!(cx.update(|window, _| input_focus.is_focused(window)));
    cx.simulate_keystrokes("escape");
    draw(&mut cx);
    cx.simulate_keystrokes("tab");
    assert_eq!(input(&receiver), b"\t");
}

#[gpui::test]
fn restored_workspace_focus_keeps_search_input_out_of_pty(cx: &mut TestAppContext) {
    let (view, mut cx, receiver, _) = open(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| view.open_reading(false, window, cx));
        // 沿用工作区 focus_active 的延迟路径，覆盖搜索打开后的 FocusRequested 往返。
        let focus = view.read(cx).focus_handle(cx);
        window.defer(cx, move |window, cx| window.focus(&focus, cx));
    });
    draw(&mut cx);
    let input_focus = view.read_with(&cx, |view, cx| {
        view.reading.panel.as_ref().unwrap().input.read(cx).focus_handle(cx)
    });
    assert!(cx.update(|window, _| input_focus.is_focused(window)));
    cx.simulate_input("find this");
    draw(&mut cx);
    assert_eq!(
        view.read_with(&cx, |view, cx| {
            view.reading.panel.as_ref().unwrap().input.read(cx).value().to_string()
        }),
        "find this"
    );
    assert!(input(&receiver).is_empty());
}

#[gpui::test]
fn search_input_and_overlay_mouse_never_write_pty_or_resize(cx: &mut TestAppContext) {
    let (view, mut cx, receiver, proxy) = open(cx);
    feed(&view, &mut cx, &proxy, b"hello world");
    let size = view.read_with(&cx, |view, _| (view.cols, view.rows));
    cx.update(|window, cx| view.update(cx, |view, cx| view.open_reading(false, window, cx)));
    draw(&mut cx);
    assert!(view.read_with(&cx, |view, _| view.reading.panel.is_some()));
    cx.simulate_input("world");
    cx.simulate_keystrokes("left backspace");
    draw(&mut cx);
    let overlay = cx.debug_bounds("terminal-reading-overlay").unwrap();
    // 紧凑搜索条的中央属于输入框；右键会打开 Linux 菜单并先消费 Esc。
    // 此处验证面板留白的鼠标隔离，明确命中输入框之外的内边距。
    let at = gpui::point(overlay.left() + px(2.0), overlay.center().y);
    assert!(overlay.contains(&at));
    assert!(!cx.debug_bounds("reading-input").unwrap().contains(&at));
    for button in [MouseButton::Middle, MouseButton::Right] {
        cx.simulate_mouse_down(at, button, Modifiers::default());
        cx.simulate_mouse_up(at, button, Modifiers::default());
    }
    draw(&mut cx);
    assert_eq!(view.read_with(&cx, |view, _| (view.cols, view.rows)), size);
    assert!(!receiver.try_iter().any(|message| matches!(message, Msg::Input(_) | Msg::Resize(_))));
    cx.simulate_keystrokes("escape");
    draw(&mut cx);
    assert!(view.read_with(&cx, |view, _| view.reading.panel.is_none()));
    assert!(input(&receiver).is_empty());
    cx.simulate_keystrokes("escape");
    assert_eq!(input(&receiver), b"\x1b");
}

#[gpui::test]
fn ctrl_f_is_raw_for_shell_agent_alt_and_mouse_but_explicit_search_works(cx: &mut TestAppContext) {
    let (view, mut cx, receiver, proxy) = open(cx);
    cx.simulate_keystrokes("ctrl-f");
    assert_eq!(input(&receiver), b"\x06");
    assert!(view.read_with(&cx, |view, _| view.reading.panel.is_none()));
    view.update(&mut cx, |view, _| view.running_program = Some("codex".into()));
    cx.simulate_keystrokes("ctrl-f");
    assert_eq!(input(&receiver), b"\x06");
    assert!(view.read_with(&cx, |view, _| view.reading.panel.is_none()));
    view.update(&mut cx, |view, _| view.running_program = None);
    for modes in [b"\x1b[?1049h".as_slice(), b"\x1b[?1049l\x1b[?1000h"] {
        feed(&view, &mut cx, &proxy, modes);
        cx.simulate_keystrokes("ctrl-f");
        assert_eq!(input(&receiver), b"\x06");
        assert!(view.read_with(&cx, |view, _| view.reading.panel.is_none()));
    }
    cx.update(|window, cx| view.update(cx, |view, cx| view.open_reading(false, window, cx)));
    draw(&mut cx);
    cx.simulate_input("explicit");
    assert!(input(&receiver).is_empty());
}

#[gpui::test]
fn two_command_copies_are_separate_and_invalid_scope_preserves_clipboard(cx: &mut TestAppContext) {
    let (view, mut cx, receiver, proxy) = open(cx);
    feed(
        &view,
        &mut cx,
        &proxy,
        format!("{}{}", execution("first\r\n"), execution("second\r\n")).as_bytes(),
    );
    cx.update(|window, cx| view.update(cx, |view, cx| view.open_reading(true, window, cx)));
    let commands =
        view.read_with(&cx, |view, _| view.reading.panel.as_ref().unwrap().commands.clone());
    assert_eq!(commands.len(), 2);
    for (id, expected) in [(commands[1].id, "first\n"), (commands[0].id, "second\n")] {
        view.update(&mut cx, |view, cx| {
            view.choose_reading_scope(Some(id), cx);
            view.copy_reading_command(cx);
        });
        cx.run_until_parked();
        assert_eq!(clipboard(&mut cx).as_deref(), Some(expected));
    }
    // Reset invalidates the frozen source; failure must not publish a partial payload.
    feed(&view, &mut cx, &proxy, b"\x1bc");
    view.update(&mut cx, |view, cx| view.copy_reading_command(cx));
    cx.run_until_parked();
    assert_eq!(clipboard(&mut cx).as_deref(), Some("second\n"));
    assert!(input(&receiver).is_empty());
}

#[gpui::test]
fn live_hit_guard_rejects_repaint_without_epoch_change(cx: &mut TestAppContext) {
    let (view, mut cx, _, proxy) = open(cx);
    feed(&view, &mut cx, &proxy, b"same");
    let source = view.read_with(&cx, |view, _| view.session.as_ref().unwrap().term.clone());
    let available = source.lock().reading_span();
    let span = ReadingSpan {
        epoch: available.epoch,
        start: available.start,
        end: ReadingPoint { line: available.start.line, column: 4 },
    };
    let guard = worker::validate_hit(&source, span, "same", true).unwrap();
    feed(&view, &mut cx, &proxy, b"\rnew!");
    assert_eq!(source.lock().reading_epoch(), span.epoch);
    assert!(!worker::guard_valid(&source.lock(), span, &guard));
}
