//! Opt-in native rendering evidence using synthetic VT, never a user's shell.

use super::super::*;
use gpui::{WindowBounds, WindowOptions, size};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

fn fixture() -> String {
    let mut vt = String::from("\x1b[?1049h\x1b[?25l\x1b[2J\x1b[H");
    // 65 × 65 pixels encoded as Braille: a circle must stay round, with no
    // banding every fourth dot row. Right-hand controls exercise text and joins.
    for row in 0..17 {
        vt.push_str(&format!("\x1b[{};1H", row + 1));
        for col in 0..33 {
            let mut bits = 0;
            for (bit, (dx, dy)) in [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2), (0, 3), (1, 3)]
                .into_iter()
                .enumerate()
            {
                let x = col * 2 + dx - 32;
                let y = row * 4 + dy - 32;
                if (26 * 26..=32 * 32).contains(&(x * x + y * y)) {
                    bits |= 1 << bit;
                }
            }
            vt.push(char::from_u32(0x2800 + bits).unwrap());
        }
    }
    for (row, text) in [
        "ASCII / 中文 / Å É ê Ǻ é / gypq",
        "\x1b[1mBold 中文 ÅÉ gypq\x1b[0m",
        "\x1b[3mItalic 中文 ÅÉ gypq\x1b[0m",
        "⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿",
        "⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿",
        "⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿",
        "┌──────────────┐ ┏━━━━━━┓",
        "│ Box joins    │ ┃ 中文 ┃",
        "└──────────────┘ ┗━━━━━━┛",
        "████████████████ ▄▀▌▐░▒▓",
        "████████████████ ▀▄▐▌░▒▓",
        "\x1b[48;2;80;100;120m Powerline \x1b[0;38;2;80;100;120m\x1b[0m",
    ]
    .iter()
    .enumerate()
    {
        vt.push_str(&format!("\x1b[{};38H{text}", row + 1));
    }
    vt
}

#[test]
#[ignore = "requires a native desktop and fresh isolated PEBREL_GRID_QA_DIR/config"]
fn native_character_grid_preserves_art_text_and_viewport() {
    let output = PathBuf::from(std::env::var_os("PEBREL_GRID_QA_DIR").expect("QA output"));
    assert!(output.is_absolute());
    assert_eq!(
        std::env::var_os("PEBREL_CONFIG_DIR").map(PathBuf::from),
        Some(output.join("config"))
    );
    assert!(!output.join("metrics.json").exists(), "use a fresh fixture");
    let success = Arc::new(Mutex::new(false));
    let result = success.clone();
    gpui_platform::application().with_assets(crate::gpui_shell::assets::NebulaAssets).run(move |cx| {
        crate::gpui_shell::register_bundled_fonts(cx);
        gpui_component::init(cx);
        crate::gpui_shell::scientific_render::init(cx);
        let mut settings = Settings::load(nebula_settings::ThemeName::Nord);
        settings.font_size_px = 14.0;
        settings.base_font_size_px = 14.0;
        assert_eq!((settings.font_offset_x, settings.font_offset_y), (0.0, 0.0));
        cx.set_global(settings);
        crate::gpui_shell::theme::apply_chrome_theme(cx);
        let mut terminal = None;
        let mut receiver = None;
        let window_handle = cx.open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(20.0), px(20.0)), size(px(900.0), px(480.0)),
            ))),
            focus: false,
            ..Default::default()
        }, |window, cx| {
            let view = cx.new(|cx| TerminalView::new(9000, (100, 28), TerminalLaunch::Local {
                cwd: Some(output.clone()),
                shell: Some(nebula_terminal::tty::Shell::new("pebrel-test-missing-shell-executable".into(), vec![])),
                shell_name: None,
            }, window, cx));
            receiver = Some(view.update(cx, |view, _| {
                let receiver = view.install_completion_test_session();
                startup_tests::feed(view, fixture().as_bytes());
                receiver
            }));
            terminal = Some(view.clone());
            cx.new(|cx| gpui_component::Root::new(view, window, cx))
        }).unwrap();
        let terminal = terminal.unwrap();
        let receiver = receiver.unwrap();
        cx.spawn(async move |cx| {
            cx.background_executor().timer(Duration::from_millis(600)).await;
            let mut costs = Vec::new();
            for _ in 0..60 {
                cx.update_window(window_handle.into(), |_, window, cx| {
                    window.refresh();
                    let start = Instant::now();
                    let _ = window.draw(cx);
                    costs.push(start.elapsed().as_secs_f64() * 1000.0);
                    // The synthetic session has no I/O thread. Consume the
                    // actual viewport notifications as that thread would;
                    // never derive the grid independently in the fixture.
                    for message in receiver.try_iter() {
                        if let Msg::Resize(size) | Msg::ResizeGrid(size) = message {
                            let view = terminal.read(cx);
                            assert_eq!(size.cell_width, (view.cell_width.as_f32() * window.scale_factor()).round() as u16);
                            assert_eq!(size.cell_height, (view.line_height.as_f32() * window.scale_factor()).round() as u16);
                            view.session.as_ref().unwrap().term.lock().resize(
                                nebula_terminal::term::test::TermSize::new(size.num_cols as usize, size.num_lines as usize));
                        }
                    }
                }).unwrap();
                cx.background_executor().timer(Duration::from_millis(16)).await;
            }
            let metrics = cx.update_window(window_handle.into(), |_, window, cx| {
                let view = terminal.read(cx);
                let (w, h) = super::cell_metrics(window, cx);
                assert_eq!((view.cell_width, view.line_height), (w, h));
                let term = view.session.as_ref().unwrap().term.lock();
                assert_eq!((term.columns(), term.screen_lines()), (view.cols, view.rows));
                let font_id = cx.text_system().resolve_font(&view.font);
                let shaped = window.text_system().shape_line("M".into(), view.font_size,
                    &[gpui::TextRun { len: 1, font: view.font.clone(), color: gpui::white(),
                        background_color: None, underline: None, strikethrough: None }], None);
                assert_eq!(shaped.runs[0].font_id, font_id);
                serde_json::json!({"scale": window.scale_factor(), "font": view.font.family.as_ref(),
                    "font_id": format!("{font_id:?}"), "font_size": view.font_size.as_f32(),
                    "ascent": shaped.ascent.as_f32(), "descent": shaped.descent.as_f32(),
                    "cell": [w.as_f32(), h.as_f32()], "grid": [view.cols, view.rows],
                    "origin": [view.origin.x.as_f32(), view.origin.y.as_f32()],
                    "draw_cpu_ms": costs})
            }).unwrap();
            std::fs::write(output.join("metrics.json"), serde_json::to_vec_pretty(&metrics).unwrap()).unwrap();
            for _ in 0..100 {
                if output.join("capture-complete").exists() { break; }
                cx.background_executor().timer(Duration::from_millis(200)).await;
            }
            *result.lock().unwrap() = true;
            drop(receiver);
            let _ = cx.update_window(window_handle.into(), |_, window, _| window.remove_window());
            cx.update(|cx| cx.quit());
        }).detach();
    });
    assert!(*success.lock().unwrap());
}
