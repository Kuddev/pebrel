//! Opt-in acceptance through the actual Windows clipboard, GPUI view and ConPTY.

use super::*;
use gpui::{EntityInputHandler as _, WindowBounds, WindowOptions, size};
use gpui_component::ActiveTheme as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Surface(gpui::Entity<TerminalView>);

impl Render for Surface {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(cx.theme().background).child(self.0.clone())
    }
}

async fn wait_for(
    cx: &mut gpui::AsyncApp,
    window: gpui::AnyWindowHandle,
    view: &gpui::Entity<TerminalView>,
    check: impl Fn(&TerminalView) -> bool,
) -> Result<(), String> {
    for _ in 0..500 {
        if cx
            .update_window(window, |_, window, cx| {
                window.refresh();
                check(view.read(cx))
            })
            .map_err(|error| error.to_string())?
        {
            return Ok(());
        }
        cx.background_executor().timer(Duration::from_millis(20)).await;
    }
    cx.update_window(window, |_, _, cx| {
        let view = view.read(cx);
        format!(
            "native clipboard timeout: editor={:?}, error={:?}, exited={:?}, screen={:?}",
            view.completion_editor,
            view.error,
            view.exited,
            view.runtime_screen_state()
        )
    })
    .map_err(|error| error.to_string())
    .and_then(Err)
}

async fn buffer(
    cx: &mut gpui::AsyncApp,
    window: gpui::AnyWindowHandle,
    view: &gpui::Entity<TerminalView>,
    expected: &str,
) -> Result<(), String> {
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, _| {
            view.completion_editor.clear_report_for_test();
            view.write_bytes(view.completion_editor_query_bytes());
        });
    })
    .map_err(|error| error.to_string())?;
    wait_for(cx, window, view, |view| {
        view.completion_editor.reported_line_for_test() == Some(expected)
    })
    .await
    .map_err(|error| format!("expected buffer {expected:?}: {error}"))
}

async fn key(
    cx: &mut gpui::AsyncApp,
    window: gpui::AnyWindowHandle,
    view: &gpui::Entity<TerminalView>,
    key: &str,
) -> Result<(), String> {
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.on_key_down(
                &KeyDownEvent {
                    keystroke: gpui::Keystroke::parse(key).unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                },
                window,
                cx,
            );
        });
    })
    .map_err(|error| error.to_string())
}

async fn paste(
    cx: &mut gpui::AsyncApp,
    window: gpui::AnyWindowHandle,
    view: &gpui::Entity<TerminalView>,
    text: &str,
) -> Result<(), String> {
    cx.update_window(window, |_, window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(text.into()));
        view.update(cx, |view, cx| {
            if std::env::var("PEBREL_NATIVE_CLIPBOARD_VT").as_deref() == Ok("1") {
                // Exercise ConPTY's translated VT-key path on the same native shell.
                feed(view, b"\x1b[?9001l");
            }
            view.paste(window, cx);
        });
    })
    .map_err(|error| error.to_string())
}

async fn run_cases(
    cx: &mut gpui::AsyncApp,
    window: gpui::AnyWindowHandle,
    view: &gpui::Entity<TerminalView>,
    output: &Path,
    owned_clipboard: &mut String,
) -> Result<serde_json::Value, String> {
    wait_for(cx, window, view, |view| {
        view.completion_editor.ready()
            && view.native_clipboard_ready()
            && view
                .session
                .as_ref()
                .is_some_and(|session| session.term.lock().nebula_prompt_active())
    })
    .await?;
    buffer(cx, window, view, "").await?;
    let first = "PasteFixture 'line1'\r\nPasteFixture 'line2'\r\nPasteFixture 'line3'\r\n";
    *owned_clipboard = first.into();
    paste(cx, window, view, first).await?;
    let normalized = first.replace("\r\n", "\n");
    buffer(cx, window, view, &normalized).await?;
    let execution = output.join("execution.txt");
    if execution.exists() {
        return Err("paste executed before Enter".into());
    }
    key(cx, window, view, "ctrl-z").await?;
    buffer(cx, window, view, "").await?;
    key(cx, window, view, "ctrl-y").await?;
    buffer(cx, window, view, &normalized).await?;
    if execution.exists() {
        return Err("undo/redo accepted the command".into());
    }
    key(cx, window, view, "enter").await?;
    wait_for(cx, window, view, |_| {
        std::fs::read_to_string(&execution).is_ok_and(|text| text == "line1\nline2\nline3\n")
    })
    .await?;
    wait_for(cx, window, view, |view| view.native_clipboard_ready()).await?;
    buffer(cx, window, view, "").await?;

    let literal = "Write-Output @'\r\nclipboard\t中文😀\r\nline two\r\n'@\r\n";
    *owned_clipboard = literal.into();
    paste(cx, window, view, literal).await?;
    // PSReadLine Paste strips CR and expands tabs to four spaces. Assert its
    // native semantics, including here-string contents, rather than raw bytes.
    buffer(cx, window, view, "Write-Output @'\nclipboard    中文😀\nline two\n'@\n").await?;
    wait_for(cx, window, view, |view| {
        view.runtime_screen_state().is_some_and(|(text, _)| text.contains("中文😀"))
    })
    .await?;
    key(cx, window, view, "ctrl-c").await?;
    wait_for(cx, window, view, |view| view.native_clipboard_ready()).await?;
    buffer(cx, window, view, "").await?;

    let initial = "Write-Output 'before-after'";
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.replace_text_in_range(None, initial, window, cx));
    })
    .map_err(|error| error.to_string())?;
    buffer(cx, window, view, initial).await?;
    for _ in 0..6 {
        key(cx, window, view, "left").await?;
    }
    let middle = "中文😀\t\r\nsecond-";
    *owned_clipboard = middle.into();
    paste(cx, window, view, middle).await?;
    let expected = "Write-Output 'before-中文😀    \nsecond-after'";
    buffer(cx, window, view, expected).await?;
    if std::fs::read_to_string(&execution).map_err(|error| error.to_string())?
        != "line1\nline2\nline3\n"
    {
        return Err("later pastes executed implicitly".into());
    }

    if std::env::var("PEBREL_NATIVE_CLIPBOARD_CAPTURE").as_deref() == Ok("1") {
        std::fs::write(output.join("capture-ready"), std::process::id().to_string())
            .map_err(|error| error.to_string())?;
        for _ in 0..1000 {
            if output.join("capture-complete").exists() {
                break;
            }
            cx.background_executor().timer(Duration::from_millis(20)).await;
        }
        if !output.join("capture-complete").exists() {
            return Err("capture not acknowledged".into());
        }
    }
    key(cx, window, view, "ctrl-c").await?;
    Ok(serde_json::json!({
        "multiline_unsubmitted": true,
        "one_undo_and_redo": true,
        "explicit_enter_executed_once": true,
        "native_tab_normalization_unicode_and_newlines": true,
        "visible_unicode": true,
        "cancel": true,
        "middle_insertion": true,
        "middle_buffer": expected,
    }))
}

#[test]
#[ignore = "requires Windows desktop and fresh PEBREL_NATIVE_CLIPBOARD_QA_DIR/config"]
fn native_clipboard_shell_end_to_end() {
    assert!(
        crate::platform::CAPABILITIES.native_shell_clipboard,
        "requires a supported native shell editor"
    );
    let output = PathBuf::from(std::env::var_os("PEBREL_NATIVE_CLIPBOARD_QA_DIR").unwrap());
    assert!(output.is_absolute());
    assert_eq!(
        std::env::var_os("PEBREL_CONFIG_DIR").map(PathBuf::from),
        Some(output.join("config"))
    );
    assert!(!output.join("result.json").exists(), "use a fresh fixture directory");
    std::fs::create_dir_all(output.join("config")).unwrap();
    let program =
        std::env::var("PEBREL_TEST_POWERSHELL").unwrap_or_else(|_| "powershell.exe".into());
    let shell = crate::platform::shell::native_clipboard_qa_shell(program.clone(), &output);
    let result = Arc::new(Mutex::new(None));
    let after = result.clone();
    gpui_platform::application().with_assets(crate::gpui_shell::assets::NebulaAssets).run(
        move |cx| {
            crate::gpui_shell::register_bundled_fonts(cx);
            gpui_component::init(cx);
            crate::gpui_shell::scientific_render::init(cx);
            let mut settings = Settings::load(nebula_settings::ThemeName::Nord);
            settings.ghost = false;
            cx.set_global(settings);
            crate::gpui_shell::theme::apply_chrome_theme(cx);
            let previous_clipboard = cx.read_from_clipboard();
            let mut terminal = None;
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            point(px(80.0), px(80.0)),
                            size(px(1000.0), px(580.0)),
                        ))),
                        focus: false,
                        ..Default::default()
                    },
                    |window, cx| {
                        window.set_window_title("Pebrel native multiline paste acceptance");
                        let view = cx.new(|cx| {
                            TerminalView::new(
                                9567,
                                (110, 30),
                                TerminalLaunch::Local {
                                    cwd: Some(output.clone()),
                                    shell: Some(shell),
                                    shell_name: None,
                                },
                                window,
                                cx,
                            )
                        });
                        window.focus(&view.read(cx).focus_handle.clone(), cx);
                        terminal = Some(view.clone());
                        let surface = cx.new(|_| Surface(view));
                        cx.new(|cx| gpui_component::Root::new(surface, window, cx))
                    },
                )
                .unwrap();
            let terminal = terminal.unwrap();
            cx.spawn(async move |cx| {
                let mut owned_clipboard = String::new();
                let run =
                    run_cases(cx, window.into(), &terminal, &output, &mut owned_clipboard).await;
                let report = match &run {
                    Ok(checks) => {
                        serde_json::json!({"passed":true,"shell":program,"checks":checks})
                    },
                    Err(error) => serde_json::json!({"passed":false,"shell":program,"error":error}),
                };
                std::fs::write(
                    output.join("result.json"),
                    serde_json::to_vec_pretty(&report).unwrap(),
                )
                .unwrap();
                *result.lock().unwrap() = Some(run);
                let _ = cx.update_window(window.into(), |_, window, cx| {
                    if cx.read_from_clipboard().and_then(|clip| clip.text()).as_deref()
                        == Some(&owned_clipboard)
                    {
                        cx.write_to_clipboard(
                            previous_clipboard
                                .unwrap_or_else(|| ClipboardItem::new_string(String::new())),
                        );
                    }
                    window.remove_window();
                });
                cx.update(|cx| cx.quit());
            })
            .detach();
        },
    );
    after.lock().unwrap().take().expect("native acceptance completed").unwrap();
}
