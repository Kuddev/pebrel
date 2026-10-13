//! Real PSReadLine echo through the production text-input and rendering path.

use super::*;
use gpui::{EntityInputHandler, WindowBounds, WindowOptions, size};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

#[test]
#[ignore = "requires native Windows, pwsh and fresh PEBREL_CURSOR_QA_DIR/config"]
fn native_psreadline_typing_animates_each_echoed_character() {
    let output = PathBuf::from(std::env::var_os("PEBREL_CURSOR_QA_DIR").expect("QA output"));
    assert!(output.is_absolute());
    assert_eq!(
        std::env::var_os("PEBREL_CONFIG_DIR").map(PathBuf::from),
        Some(output.join("config"))
    );
    let script = output.join("typing-shell.ps1");
    assert!(!script.exists(), "use a fresh fixture");
    std::fs::write(
        &script,
        r#"
Import-Module PSReadLine
Set-PSReadLineOption -HistorySaveStyle SaveNothing -PredictionSource None
function global:prompt { 'QA> ' }
[Console]::Write("$([char]27)[2J$([char]27)[H")
"#,
    )
    .unwrap();
    let passed = Arc::new(Mutex::new(false));
    let after_run = passed.clone();
    gpui_platform::application().with_assets(crate::gpui_shell::assets::NebulaAssets).run(
        move |cx| {
            crate::gpui_shell::register_bundled_fonts(cx);
            gpui_component::init(cx);
            crate::gpui_shell::scientific_render::init(cx);
            let mut settings = Settings::load(nebula_settings::ThemeName::Nord);
            settings.cursor_motion = nebula_settings::CursorMotion::Smooth;
            cx.set_global(settings);
            crate::gpui_shell::theme::apply_chrome_theme(cx);
            let mut terminal = None;
            let handle = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            point(px(60.0), px(70.0)),
                            size(px(900.0), px(500.0)),
                        ))),
                        focus: false,
                        ..Default::default()
                    },
                    |window, cx| {
                        let view = cx.new(|cx| {
                            TerminalView::new(
                                9901,
                                (80, 24),
                                TerminalLaunch::Local {
                                    cwd: Some(output.clone()),
                                    shell: Some(nebula_terminal::tty::Shell::new(
                                        "pwsh.exe".into(),
                                        vec![
                                            "-NoLogo".into(),
                                            "-NoProfile".into(),
                                            "-NoExit".into(),
                                            "-File".into(),
                                            script.to_string_lossy().into_owned(),
                                        ],
                                    )),
                                    shell_name: None,
                                },
                                window,
                                cx,
                            )
                        });
                        let focus = view.read(cx).focus_handle.clone();
                        window.focus(&focus, cx);
                        terminal = Some(view.clone());
                        cx.new(|cx| gpui_component::Root::new(view, window, cx))
                    },
                )
                .unwrap();
            let terminal = terminal.unwrap();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(Duration::from_secs(3)).await;
                let mut trace = Vec::new();
                let mut animated = 0;
                for text in ["a", "b", "c", "d", "中"] {
                    let before = cx
                        .update_window(handle.into(), |_, window, cx| {
                            terminal.update(cx, |view, cx| {
                                assert!(view.error.is_none(), "{:?}", view.error);
                                let before = view.cursor_animation.motion.visual.col;
                                view.replace_text_in_range(None, text, window, cx);
                                before
                            })
                        })
                        .unwrap();
                    let start = Instant::now();
                    let mut intermediate = false;
                    for _ in 0..50 {
                        cx.background_executor().timer(Duration::from_millis(5)).await;
                        cx.update_window(handle.into(), |_, _, cx| {
                            let view = terminal.read(cx);
                            let term = view.session.as_ref().unwrap().term.lock();
                            let logical = term.grid().cursor.point;
                            let visual = view.cursor_animation.motion.visual;
                            intermediate |=
                                visual.col > before && visual.col < logical.column.0 as f64;
                            trace.push(serde_json::json!({
                                "input": text, "us": start.elapsed().as_micros(),
                                "before": before, "logical": [logical.column.0, logical.line.0],
                                "visual": [visual.col, visual.row],
                                "shown": term.mode().contains(TermMode::SHOW_CURSOR),
                                "shape": format!("{:?}", term.cursor_style().shape),
                                "active": view.cursor_animation.motion.active(),
                                "marked": view.marked_text,
                            }));
                        })
                        .unwrap();
                    }
                    animated += usize::from(intermediate);
                }
                std::fs::write(
                    output.join("typing-frames.json"),
                    serde_json::to_vec_pretty(&trace).unwrap(),
                )
                .unwrap();
                *passed.lock().unwrap() = animated == 5;
                let _ = cx.update_window(handle.into(), |_, window, cx| {
                    terminal.read(cx).shutdown();
                    window.remove_window();
                });
                cx.update(|cx| cx.quit());
            })
            .detach();
        },
    );
    assert!(
        *after_run.lock().unwrap(),
        "every echoed character must have intermediate native frames"
    );
}
