//! Real native rendering with synthetic VT, isolated from account-backed shells.

use super::super::startup_tests::feed;
use super::*;
use gpui::{Entity, WindowBounds, WindowOptions, size};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

struct Grid(Vec<Entity<TerminalView>>);

impl Render for Grid {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().bg(gpui::rgb(0x2e3440)).children(self.0.chunks(2).map(
            |row| {
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .children(row.iter().map(|view| div().flex_1().min_w_0().child(view.clone())))
            },
        ))
    }
}

#[test]
#[ignore = "requires native desktop and fresh isolated PEBREL_CURSOR_QA_DIR/config"]
fn native_cursor_frames_interpolate_and_settle_in_one_and_four_panes() {
    let output = PathBuf::from(std::env::var_os("PEBREL_CURSOR_QA_DIR").expect("QA output"));
    assert!(output.is_absolute());
    assert_eq!(
        std::env::var_os("PEBREL_CONFIG_DIR").map(PathBuf::from),
        Some(output.join("config"))
    );
    assert!(!output.join("frames.json").exists(), "use a fresh fixture");
    let result = Arc::new(Mutex::new(false));
    let after_run = result.clone();
    gpui_platform::application().with_assets(crate::gpui_shell::assets::NebulaAssets).run(
        move |cx| {
            crate::gpui_shell::register_bundled_fonts(cx);
            gpui_component::init(cx);
            crate::gpui_shell::scientific_render::init(cx);
            let mut settings = Settings::load(nebula_settings::ThemeName::Nord);
            settings.cursor_motion = nebula_settings::CursorMotion::Smooth;
            cx.set_global(settings);
            crate::gpui_shell::theme::apply_chrome_theme(cx);
            let mut terminals = Vec::new();
            let mut receivers = Vec::new();
            let mut grid = None;
            let window_handle = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(60.0), px(70.0)), size(px(1000.0), px(700.0)),
                    ))),
                    focus: false,
                    ..Default::default()
                },
                |window, cx| {
                    for index in 0..4 {
                        let view = cx.new(|cx| TerminalView::new(
                            9000 + index, (80, 24),
                            TerminalLaunch::Local {
                                cwd: Some(output.clone()),
                                shell: Some(nebula_terminal::tty::Shell::new(
                                    "pebrel-test-missing-shell-executable".into(), vec![],
                                )),
                                shell_name: None,
                            }, window, cx,
                        ));
                        view.update(cx, |view, cx| {
                            let (session, receiver) = session::test_session();
                            receivers.push(receiver);
                            view.session = Some(session);
                            view.error = None;
                            view.exec_context = None;
                            view.apply_settings(cx);
                            feed(view, "\x1b[?1049h\x1b[?25h\x1b[2 q\x1b[HASCII -> != 中文 é ──\r\nCursor motion QA\x1b[1;1H".as_bytes());
                        });
                        terminals.push(view);
                    }
                    let focus = terminals[0].read(cx).focus_handle.clone();
                    window.focus(&focus, cx);
                    let layout = cx.new(|_| Grid(vec![terminals[0].clone()]));
                    grid = Some(layout.clone());
                    cx.new(|cx| gpui_component::Root::new(layout, window, cx))
                },
            ).unwrap();
            let grid = grid.unwrap();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(Duration::from_millis(500)).await;
                let mut reports = Vec::new();
                for count in [1, 4] {
                    cx.update_window(window_handle.into(), |_, window, cx| {
                        // Resize the presentation while keeping every terminal session.
                        grid.update(cx, |grid, cx| {
                            grid.0 = terminals[..count].to_vec();
                            cx.notify();
                        });
                        window.refresh();
                    }).unwrap();
                    cx.background_executor().timer(Duration::from_millis(300)).await;
                    let mut samples = Vec::new();
                    let mut costs = Vec::new();
                    for frame in 0..80 {
                        cx.update_window(window_handle.into(), |_, window, cx| {
                            if frame % 10 == 0 {
                                let column = if (frame / 10) % 2 == 0 { 7 } else { 1 };
                                for (index, terminal) in terminals[..count].iter().enumerate() {
                                    terminal.update(cx, |view, cx| {
                                        let shape = [2, 6, 4, 2][index];
                                        feed(view, format!("\x1b[{shape} q\x1b[1;{column}H").as_bytes());
                                        cx.notify();
                                    });
                                }
                            }
                            let start = Instant::now();
                            let _ = window.draw(cx);
                            costs.push(start.elapsed().as_secs_f64() * 1000.0);
                            samples.push(terminals[..count].iter().map(|view| {
                                view.read(cx).cursor_animation.motion.visual.col
                            }).collect::<Vec<_>>());
                        }).unwrap();
                        cx.background_executor().timer(Duration::from_millis(16)).await;
                    }
                    assert!(samples.iter().any(|row| row.iter().all(|col| *col > 0.0 && *col < 6.0)));
                    cx.background_executor().timer(Duration::from_millis(250)).await;
                    cx.update_window(window_handle.into(), |_, _, cx| {
                        for view in &terminals[..count] {
                            assert!(!view.read(cx).cursor_animation.motion.active());
                            assert!(!view.read(cx).cursor_animation.frame_pending);
                            assert_eq!(view.read(cx).cursor_animation.motion.visual.col, 0.0);
                        }
                    }).unwrap();
                    reports.push(serde_json::json!({"panes": count, "visual_columns": samples,
                        "draw_cpu_ms": costs, "settled_without_pending_frame": true}));
                }
                std::fs::write(output.join("frames.json"), serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
                std::fs::write(output.join("ready.json"), std::process::id().to_string()).unwrap();
                for _ in 0..100 {
                    if output.join("capture-complete").exists() {
                        break;
                    }
                    cx.background_executor().timer(Duration::from_millis(200)).await;
                }
                *result.lock().unwrap() = true;
                drop(receivers);
                let _ = cx.update_window(window_handle.into(), |_, window, _| window.remove_window());
                cx.update(|cx| cx.quit());
            }).detach();
        },
    );
    assert!(*after_run.lock().unwrap());
}
