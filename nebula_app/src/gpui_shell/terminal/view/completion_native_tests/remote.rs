//! Native window acceptance through real WSL or authenticated SSH startup.

use super::*;
use crate::display::CompletionStyle;
use gpui::EntityInputHandler as _;

#[derive(serde::Deserialize)]
struct Fixture {
    route: String,
    cwd: String,
    host_cwd: PathBuf,
    distro: Option<String>,
    destination: Option<String>,
    key: Option<PathBuf>,
    known_hosts: Option<PathBuf>,
}

#[test]
#[ignore = "requires an isolated native desktop and an owned real WSL/SSH fixture"]
fn remote_editor_completion_native_shell_end_to_end() {
    let output = PathBuf::from(std::env::var_os("PEBREL_COMPLETION_QA_DIR").unwrap());
    assert!(output.is_absolute());
    assert_eq!(
        std::env::var_os("PEBREL_CONFIG_DIR").map(PathBuf::from),
        Some(output.join("config"))
    );
    assert!(!output.join("result.json").exists(), "use a fresh QA directory");
    let fixture_path = std::env::var_os("PEBREL_COMPLETION_QA_REMOTE_FIXTURE").unwrap();
    let fixture: Fixture = serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
    let launch = match fixture.route.as_str() {
        "wsl" => TerminalLaunch::Local {
            cwd: Some(fixture.host_cwd.clone()),
            shell: Some(nebula_terminal::tty::Shell::new(
                "wsl.exe".into(),
                vec![
                    "--distribution".into(),
                    fixture.distro.clone().unwrap(),
                    "--cd".into(),
                    fixture.cwd.clone(),
                ],
            )),
            shell_name: None,
        },
        "ssh" => {
            let destination = fixture.destination.clone().unwrap();
            crate::ssh_session::runtime()
                .unwrap()
                .block_on(crate::ssh_session::completion::prepare_owned_fixture(
                    &destination,
                    fixture.key.as_deref().unwrap(),
                    fixture.known_hosts.as_deref().unwrap(),
                ))
                .unwrap();
            TerminalLaunch::Ssh { destination, cwd: Some(fixture.cwd.clone()) }
        },
        _ => panic!("unknown owned fixture route"),
    };
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
        let window = cx.open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(80.0), px(80.0)), size(px(1000.0), px(600.0))))),
            focus: false,
            ..Default::default()
        }, |window, cx| {
            let view = cx.new(|cx| TerminalView::new(9002, (100, 30), launch, window, cx));
            window.focus(&view.read(cx).focus_handle.clone(), cx);
            terminal = Some(view.clone());
            let surface = cx.new(|_| CompletionSurface(view));
            cx.new(|cx| gpui_component::Root::new(surface, window, cx))
        }).unwrap();
        let terminal = terminal.unwrap();
        cx.spawn(async move |cx| {
            let run = async {
                ready(cx, window.into(), &terminal).await?;
                let mut reports = Vec::new();
                for style in [CompletionStyle::Popup, CompletionStyle::Hybrid] {
                    ready(cx, window.into(), &terminal).await?;
                    cx.update_window(window.into(), |_, window, cx| terminal.update(cx, |view, cx| {
                        cx.global_mut::<Settings>().completion_style = style;
                        view.apply_settings(cx);
                        view.replace_text_in_range(None, "c", window, cx);
                        if style == CompletionStyle::Hybrid {
                            view.on_terminal_tab(&TerminalTab, window, cx);
                        }
                    })).map_err(|e| e.to_string())?;
                    wait_for(cx, window.into(), &terminal, |view| {
                        !view.completion_editor.is_querying() && !view.suggest.completion_items.is_empty()
                    }).await?;
                    // 超过旧的一秒查询超时，实际绘制期间菜单也必须保留，而非只检查回调结果。
                    for _ in 0..15 {
                        cx.background_executor().timer(Duration::from_millis(100)).await;
                        cx.update_window(window.into(), |_, window, cx| {
                            window.refresh();
                            window.draw(cx).clear(cx);
                            let view = terminal.read(cx);
                            if view.completion_popup_geometry().is_none() || view.completion_editor.is_querying() {
                                return Err(format!(
                                    "c-prefix menu state: expected={style:?} actual={:?} line={:?} items={} selected={:?} anchor={:?} size={}x{} calculation={} editor={:?} mode={:?}",
                                    view.completion_style,
                                    view.suggest.screen_line, view.suggest.completion_items.len(),
                                    view.suggest.completion_selected, view.suggest_anchor, view.cols, view.rows,
                                    view.suggestion_task.is_some(),
                                    view.completion_editor, view.term_mode(),
                                ));
                            }
                            Ok::<_, String>(())
                        }).map_err(|e| e.to_string())??;
                    }
                    let expected = cx.update_window(window.into(), |_, window, cx| terminal.update(cx, |view, cx| {
                        if view.suggest.completion_selected.is_none() { key(view, "down", window, cx); }
                        let item = &view.suggest.completion_items[view.suggest.completion_selected.unwrap()];
                        let mut expected = "c".to_owned();
                        for _ in 0..item.replace_chars { expected.pop(); }
                        expected.push_str(&item.insert);
                        key(view, "enter", window, cx);
                        expected
                    })).map_err(|e| e.to_string())?;
                    probe(cx, window.into(), &terminal, &expected).await?;
                    reports.push(serde_json::json!({"route":fixture.route,"mode":format!("{style:?}"),"scenario":"c-prefix-stable-menu-enter","accepted":expected,"observation_ms":1500,"native_buffer_verified":true}));
                    cx.update_window(window.into(), |_, window, cx| terminal.update(cx, |view, cx| key(view, "ctrl-u", window, cx))).map_err(|e| e.to_string())?;
                }
                for (style, name) in [(CompletionStyle::Inline, "inline"), (CompletionStyle::Popup, "popup"), (CompletionStyle::Hybrid, "hybrid")] {
                    ready(cx, window.into(), &terminal).await?;
                    let branch = format!("qa/remote-{name}");
                    let prefix = format!("git switch qa/remote-{}", &name[..2]);
                    let expected = format!("git switch {branch}");
                    let before = std::fs::read(fixture.host_cwd.join(".git/HEAD")).unwrap();
                    cx.update_window(window.into(), |_, window, cx| terminal.update(cx, |view, cx| {
                        cx.global_mut::<Settings>().completion_style = style;
                        view.apply_settings(cx);
                        view.replace_text_in_range(None, &prefix, window, cx);
                        view.on_terminal_tab(&TerminalTab, window, cx);
                    })).map_err(|e| e.to_string())?;
                    accept(cx, window.into(), &terminal, style).await?;
                    probe(cx, window.into(), &terminal, &expected).await?;
                    assert_eq!(std::fs::read(fixture.host_cwd.join(".git/HEAD")).unwrap(), before, "acceptance must only edit");
                    submit(cx, window.into(), &terminal, &fixture.host_cwd, &branch).await?;
                    reports.push(serde_json::json!({"route":fixture.route,"mode":format!("{style:?}"),"scenario":"immediate-tab","accepted":expected,"execution_verified":true}));

                    ready(cx, window.into(), &terminal).await?;
                    let typed = format!("git switch \"qa/middle-{name}-中-old\" --quiet");
                    let expected = format!("git switch \"qa/middle-{name}-中文😀\" --quiet");
                    let before = std::fs::read(fixture.host_cwd.join(".git/HEAD")).unwrap();
                    cx.update_window(window.into(), |_, window, cx| terminal.update(cx, |view, cx| {
                        view.replace_text_in_range(None, &typed, window, cx);
                        for _ in 0.."-old\" --quiet".chars().count() { key(view, "left", window, cx); }
                        view.on_terminal_tab(&TerminalTab, window, cx);
                    })).map_err(|e| e.to_string())?;
                    accept(cx, window.into(), &terminal, style).await?;
                    probe(cx, window.into(), &terminal, &expected).await?;
                    assert_eq!(std::fs::read(fixture.host_cwd.join(".git/HEAD")).unwrap(), before);
                    submit(cx, window.into(), &terminal, &fixture.host_cwd, &format!("qa/middle-{name}-中文😀")).await?;
                    reports.push(serde_json::json!({"route":fixture.route,"mode":format!("{style:?}"),"scenario":"middle-unicode","accepted":expected,"following_options_preserved":true,"execution_verified":true}));
                }
                ready(cx, window.into(), &terminal).await?;
                let line = "git switch qa/remote-in";
                cx.update_window(window.into(), |_, window, cx| terminal.update(cx, |view, cx| {
                    view.replace_text_in_range(None, line, window, cx);
                    view.on_terminal_tab(&TerminalTab, window, cx);
                    key(view, "escape", window, cx);
                })).map_err(|e| e.to_string())?;
                probe(cx, window.into(), &terminal, line).await?;
                cx.update_window(window.into(), |_, _, cx| {
                    assert!(!suggest::popup_active(&terminal.read(cx).suggest));
                }).map_err(|e| e.to_string())?;
                reports.push(serde_json::json!({"route":fixture.route,"scenario":"cancel-escape","native_buffer_verified":true}));
                Ok::<_, String>(reports)
            }.await;
            std::fs::write(output.join("result.json"), serde_json::to_vec_pretty(&run).unwrap()).unwrap();
            *result.lock().unwrap() = Some(run);
            let _ = cx.update_window(window.into(), |_, window, cx| { terminal.update(cx, |view, _| view.shutdown()); window.remove_window(); });
            cx.update(|cx| cx.quit());
        }).detach();
    });
    let outcome = after.lock().unwrap();
    assert!(outcome.as_ref().is_some_and(|r| r.is_ok()), "{outcome:?}");
}

fn key(
    view: &mut TerminalView,
    key: &str,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<TerminalView>,
) {
    view.on_key_down(
        &KeyDownEvent {
            keystroke: gpui::Keystroke::parse(key).unwrap(),
            is_held: false,
            prefer_character_input: false,
        },
        window,
        cx,
    );
}

async fn ready(
    cx: &mut gpui::AsyncApp,
    window: gpui::AnyWindowHandle,
    view: &gpui::Entity<TerminalView>,
) -> Result<(), String> {
    wait_for(cx, window, view, |view| {
        view.completion_editor.ready_for_test()
            && view.session.as_ref().is_some_and(|s| s.term.lock().nebula_prompt_active())
    })
    .await?;
    probe(cx, window, view, "").await
}

async fn probe(
    cx: &mut gpui::AsyncApp,
    window: gpui::AnyWindowHandle,
    view: &gpui::Entity<TerminalView>,
    expected: &str,
) -> Result<(), String> {
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, _| {
            view.completion_editor.clear_report_for_test();
            let query = view.completion_editor_query_bytes();
            view.write_bytes(query);
        })
    })
    .map_err(|e| e.to_string())?;
    wait_for(cx, window, view, |view| {
        view.completion_editor.reported_line_for_test() == Some(expected)
    })
    .await
}

async fn accept(
    cx: &mut gpui::AsyncApp,
    window: gpui::AnyWindowHandle,
    view: &gpui::Entity<TerminalView>,
    style: CompletionStyle,
) -> Result<(), String> {
    if style != CompletionStyle::Inline {
        wait_for(cx, window, view, |view| {
            !view.completion_editor.is_querying()
                && !view.suggest.completion_items.is_empty()
                && view.suggest.completion_selected.is_some()
        })
        .await?;
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| {
                assert!(
                    view.completion_popup_geometry().is_some(),
                    "list is painted through the product"
                );
                key(view, "enter", window, cx);
            })
        })
        .map_err(|e| e.to_string())?;
    } else {
        wait_for(cx, window, view, |view| !view.completion_editor.is_querying()).await?;
    }
    Ok(())
}

async fn submit(
    cx: &mut gpui::AsyncApp,
    window: gpui::AnyWindowHandle,
    view: &gpui::Entity<TerminalView>,
    repository: &std::path::Path,
    branch: &str,
) -> Result<(), String> {
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| key(view, "enter", window, cx))
    })
    .map_err(|e| e.to_string())?;
    wait_for(cx, window, view, |_| {
        std::fs::read_to_string(repository.join(".git/HEAD"))
            .is_ok_and(|v| v.trim() == format!("ref: refs/heads/{branch}"))
    })
    .await
}
