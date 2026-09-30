//! Native window + real shell/PTY + Git, using the product's input and paint paths.

use super::*;
use gpui::{EntityInputHandler as _, WindowBounds, WindowOptions, size};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

async fn wait_for(
    cx: &mut gpui::AsyncApp,
    window: gpui::AnyWindowHandle,
    view: &gpui::Entity<TerminalView>,
    check: impl Fn(&TerminalView) -> bool,
) -> Result<(), String> {
    for _ in 0..500 {
        let ready = cx
            .update_window(window, |_, window, cx| {
                window.refresh();
                check(view.read(cx))
            })
            .map_err(|error| error.to_string())?;
        if ready {
            return Ok(());
        }
        cx.background_executor().timer(Duration::from_millis(20)).await;
    }
    cx.update_window(window, |_, _, cx| {
        let view = view.read(cx);
        format!(
            "completion timeout: line={:?} ghost={:?} items={:?} error={:?} exited={:?}",
            view.suggest.screen_line,
            view.suggest.suggestion,
            view.suggest.completion_items,
            view.error,
            view.exited
        )
    })
    .map_err(|error| error.to_string())
    .and_then(Err)
}

#[test]
#[ignore = "requires a native desktop and fresh isolated PEBREL_COMPLETION_QA_DIR/config"]
fn git_completion_native_shell_end_to_end() {
    let output = PathBuf::from(std::env::var_os("PEBREL_COMPLETION_QA_DIR").expect("QA directory"));
    assert!(output.is_absolute());
    assert_eq!(
        std::env::var_os("PEBREL_CONFIG_DIR").map(PathBuf::from),
        Some(output.join("config"))
    );
    assert!(!output.join("result.json").exists(), "use a fresh QA directory");
    let repository = crate::git_completion::tests::repository();
    for branch in ["qa/inline", "qa/popup", "qa/hybrid", "qa/right"] {
        crate::git_completion::tests::git(repository.path(), &["branch", branch]);
    }
    let shell = crate::platform::shell::completion_qa_shell(&output);
    let result = Arc::new(Mutex::new(None));
    let after = result.clone();
    gpui_platform::application().with_assets(crate::gpui_shell::assets::NebulaAssets).run(move |cx| {
        crate::gpui_shell::register_bundled_fonts(cx);
        gpui_component::init(cx);
        crate::gpui_shell::scientific_render::init(cx);
        let mut settings = Settings::load(nebula_settings::ThemeName::Nord);
        settings.ghost = true;
        cx.set_global(settings);
        crate::gpui_shell::theme::apply_chrome_theme(cx);
        let mut terminal = None;
        let cwd = repository.path().to_owned();
        let window = cx.open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(80.0), px(80.0)), size(px(1000.0), px(600.0))))),
            focus: false,
            ..Default::default()
        }, |window, cx| {
            let view = cx.new(|cx| TerminalView::new(9001, (100, 30), TerminalLaunch::Local {
                cwd: Some(cwd), shell: Some(shell), shell_name: None,
            }, window, cx));
            window.focus(&view.read(cx).focus_handle.clone(), cx);
            terminal = Some(view.clone());
            cx.new(|cx| gpui_component::Root::new(view, window, cx))
        }).unwrap();
        let terminal = terminal.unwrap();
        cx.spawn(async move |cx| {
            let run = async {
                let mut reports = Vec::new();
                let mut previous = "main";
                // Every case starts from a real prompt, types through EntityInputHandler,
                // paints candidates, accepts through the keyboard handler, then executes.
                for (mode, prefix, expected, branch, right) in [
                    (crate::display::CompletionStyle::Inline, "git switch qa/in", "git switch qa/inline", "qa/inline", false),
                    (crate::display::CompletionStyle::Popup, "git switch \"qa/po\"", "git switch \"qa/popup\"", "qa/popup", false),
                    (crate::display::CompletionStyle::Hybrid, "git switch \"qa/hy", "git switch \"qa/hybrid\"", "qa/hybrid", false),
                    (crate::display::CompletionStyle::Hybrid, "git switch qa/ri", "git switch qa/right", "qa/right", true),
                ] {
                    wait_for(cx, window.into(), &terminal, |view| view.session.as_ref().is_some_and(|session| {
                        let term = session.term.lock();
                        crate::display::nebula_prompt_line_from_raw_grid(&term, term.grid().cursor.point, &view.suggest.line_buf, &view.suggest.suggest_env).is_some_and(|line| line.input.trim().is_empty())
                    })).await?;
                    cx.update_window(window.into(), |_, window, cx| terminal.update(cx, |view, cx| {
                        view.completion_style = mode;
                        view.replace_text_in_range(None, prefix, window, cx);
                    })).map_err(|error| error.to_string())?;
                    let start = std::time::Instant::now();
                    wait_for(cx, window.into(), &terminal, |view| if mode == crate::display::CompletionStyle::Popup { !view.suggest.completion_items.is_empty() } else { !view.suggest.suggestion.is_empty() }).await?;
                    let candidate_ms = start.elapsed().as_secs_f64() * 1000.0;
                    cx.update_window(window.into(), |_, window, cx| terminal.update(cx, |view, cx| {
                        if right {
                            view.on_key_down(&KeyDownEvent { keystroke: gpui::Keystroke::parse("right").unwrap(), is_held: false, prefer_character_input: false }, window, cx);
                        } else {
                            view.on_terminal_tab(&TerminalTab, window, cx);
                        }
                    })).map_err(|error| error.to_string())?;
                    if mode == crate::display::CompletionStyle::Hybrid && !right {
                        wait_for(cx, window.into(), &terminal, |view| !view.suggest.completion_items.is_empty()).await?;
                        cx.update_window(window.into(), |_, window, cx| terminal.update(cx, |view, cx| {
                            assert!(view.completion_popup_geometry().is_some(), "real popup layout");
                            assert!(view.suggest.screen_line.trim() == prefix, "Tab must not write");
                            view.on_key_down(&KeyDownEvent { keystroke: gpui::Keystroke::parse("enter").unwrap(), is_held: false, prefer_character_input: false }, window, cx);
                        })).map_err(|error| error.to_string())?;
                    }
                    wait_for(cx, window.into(), &terminal, |view| view.suggest.screen_line.trim() == expected).await?;
                    let head = std::fs::read_to_string(repository.path().join(".git/HEAD")).map_err(|error| error.to_string())?;
                    if head.trim() != format!("ref: refs/heads/{previous}") {
                        return Err("accepting completion executed the command".to_owned());
                    }
                    cx.update_window(window.into(), |_, window, cx| terminal.update(cx, |view, cx| {
                        view.on_key_down(&KeyDownEvent { keystroke: gpui::Keystroke::parse("enter").unwrap(), is_held: false, prefer_character_input: false }, window, cx);
                    })).map_err(|error| error.to_string())?;
                    wait_for(cx, window.into(), &terminal, |_| std::fs::read_to_string(repository.path().join(".git/HEAD")).is_ok_and(|head| head.trim() == format!("ref: refs/heads/{branch}"))).await?;
                    previous = branch;
                    reports.push(serde_json::json!({"mode": format!("{mode:?}"), "input": prefix, "accepted": expected, "branch": branch, "candidate_ms": candidate_ms, "right": right}));
                }
                Ok::<_, String>(reports)
            }.await;
            std::fs::write(output.join("result.json"), serde_json::to_vec_pretty(&run).unwrap()).unwrap();
            *result.lock().unwrap() = Some(run);
            let _ = cx.update_window(window.into(), |_, window, cx| { terminal.update(cx, |view, _| view.shutdown()); window.remove_window(); });
            cx.update(|cx| cx.quit());
        }).detach();
    });
    let outcome = after.lock().unwrap().take();
    assert!(outcome.as_ref().is_some_and(Result::is_ok), "{outcome:?}");
}
