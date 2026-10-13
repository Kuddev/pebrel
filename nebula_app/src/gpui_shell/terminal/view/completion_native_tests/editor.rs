//! Native editor acceptance, including input before the PTY has echoed it.

use super::*;
use crate::display::CompletionStyle;

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

fn mode(view: &mut TerminalView, style: CompletionStyle, cx: &mut gpui::Context<TerminalView>) {
    cx.global_mut::<Settings>().completion_style = style;
    view.apply_settings(cx);
}

async fn probe_buffer(
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
    .map_err(|error| error.to_string())?;
    wait_for(cx, window, view, |view| {
        view.completion_editor.reported_line_for_test() == Some(expected)
    })
    .await
}

async fn prompt(
    cx: &mut gpui::AsyncApp,
    window: gpui::AnyWindowHandle,
    view: &gpui::Entity<TerminalView>,
) -> Result<(), String> {
    wait_for(cx, window, view, |view| {
        view.completion_editor.ready_for_test()
            && view
                .session
                .as_ref()
                .is_some_and(|session| session.term.lock().nebula_prompt_active())
    })
    .await?;
    probe_buffer(cx, window, view, "").await
}

async fn accept_list(
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
                    "native candidate list is painted"
                );
                key(view, "enter", window, cx);
            })
        })
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

async fn execute(
    cx: &mut gpui::AsyncApp,
    window: gpui::AnyWindowHandle,
    view: &gpui::Entity<TerminalView>,
    repository: &std::path::Path,
    branch: &str,
) -> Result<(), String> {
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| key(view, "enter", window, cx))
    })
    .map_err(|error| error.to_string())?;
    let expected = format!("ref: refs/heads/{branch}");
    wait_for(cx, window, view, |_| {
        std::fs::read_to_string(repository.join(".git/HEAD"))
            .is_ok_and(|head| head.trim() == expected)
    })
    .await?;
    prompt(cx, window, view).await
}

pub(super) async fn run(
    cx: &mut gpui::AsyncApp,
    window: gpui::AnyWindowHandle,
    view: &gpui::Entity<TerminalView>,
    repository: &std::path::Path,
) -> Result<Vec<serde_json::Value>, String> {
    let mut reports = Vec::new();
    for (style, name) in [
        (CompletionStyle::Inline, "inline"),
        (CompletionStyle::Popup, "popup"),
        (CompletionStyle::Hybrid, "hybrid"),
    ] {
        prompt(cx, window, view).await?;
        let branch = format!("qa/rapid-{name}");
        let prefix = format!("git switch qa/rapid-{}", &name[..2]);
        let expected = format!("git switch {branch}");
        let before =
            std::fs::read(repository.join(".git/HEAD")).map_err(|error| error.to_string())?;
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| {
                mode(view, style, cx);
                view.replace_text_in_range(None, &prefix, window, cx);
                // No echo wait: the editor query must be ordered after the input.
                view.on_terminal_tab(&TerminalTab, window, cx);
                view.on_terminal_tab(&TerminalTab, window, cx);
            })
        })
        .map_err(|error| error.to_string())?;
        accept_list(cx, window, view, style).await?;
        wait_for(cx, window, view, |view| view.suggest.screen_line.trim() == expected).await?;
        assert_eq!(
            std::fs::read(repository.join(".git/HEAD")).unwrap(),
            before,
            "rapid Tab only edits"
        );
        execute(cx, window, view, repository, &branch).await?;
        reports.push(serde_json::json!({"scenario":"immediate-tab", "mode":format!("{style:?}"), "accepted":expected}));

        let unicode = cx
            .update_window(window, |_, _, cx| view.read(cx).completion_editor.permits_insert("😀"))
            .map_err(|error| error.to_string())?;
        let stem = format!("qa/editor-{name}{}", if unicode { "" } else { "-basic" });
        let line = format!("git switch \"{stem}-中-old\" --quiet");
        let ending = if unicode { "中文😀" } else { "中文" };
        let expected = format!("git switch \"{stem}-{ending}\" --quiet");
        let branch = format!("{stem}-{ending}");
        let before = std::fs::read(repository.join(".git/HEAD")).unwrap();
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| {
                mode(view, style, cx);
                view.replace_text_in_range(None, &line, window, cx);
                for _ in "-old\" --quiet".chars() {
                    key(view, "left", window, cx);
                }
                view.on_terminal_tab(&TerminalTab, window, cx);
            })
        })
        .map_err(|error| error.to_string())?;
        accept_list(cx, window, view, style).await?;
        if style == CompletionStyle::Inline {
            wait_for(cx, window, view, |view| !view.completion_editor.is_querying()).await?;
        }
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| key(view, "end", window, cx))
        })
        .map_err(|error| error.to_string())?;
        wait_for(cx, window, view, |view| view.suggest.screen_line.trim() == expected).await?;
        assert_eq!(
            std::fs::read(repository.join(".git/HEAD")).unwrap(),
            before,
            "caret acceptance only edits"
        );
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| {
                // Submit with the caret in the middle. Native accepted text, rather
                // than a reconstructed keystroke mirror, must enter history.
                for _ in " --quiet".chars() {
                    key(view, "left", window, cx);
                }
            })
        })
        .map_err(|error| error.to_string())?;
        execute(cx, window, view, repository, &branch).await?;
        wait_for(cx, window, view, |_| {
            crate::completion::history_hint_for_test(
                &crate::nebula_history::HistoryScope::Local,
                &format!("git switch \"{stem}-中"),
            )
            .as_deref()
                == Some(if unicode { "文😀\" --quiet" } else { "文\" --quiet" })
        })
        .await?;
        reports.push(serde_json::json!({"scenario":"middle-unicode-quoted-token", "mode":format!("{style:?}"), "accepted":expected, "following_options_preserved":true, "history_verified":true, "non_bmp_supported":unicode}));
    }

    for cancel in ["escape", "settings"] {
        prompt(cx, window, view).await?;
        let line = "git switch qa/rapid-in";
        let before = std::fs::read(repository.join(".git/HEAD")).unwrap();
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| {
                mode(view, CompletionStyle::Hybrid, cx);
                view.replace_text_in_range(None, line, window, cx);
                view.on_terminal_tab(&TerminalTab, window, cx);
                if cancel == "escape" {
                    key(view, "escape", window, cx);
                } else {
                    mode(view, CompletionStyle::Inline, cx);
                }
            })
        })
        .map_err(|error| error.to_string())?;
        wait_for(cx, window, view, |view| {
            !view.completion_editor.is_querying() && !view.suggest.completion_popup_requested
        })
        .await?;
        assert_eq!(std::fs::read(repository.join(".git/HEAD")).unwrap(), before);
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| {
                assert!(
                    view.suggest.completion_items.is_empty(),
                    "cancelled results cannot reopen a list"
                );
                // Native prediction makes a grid prefix unreadable. Verify the
                // actual editor after cancellation without reopening its list.
                view.completion_editor.clear_report_for_test();
                let query = super::super::super::keymap::encode(
                    &gpui::Keystroke::parse("ctrl-shift-f12").unwrap(),
                    &view.term_mode(),
                )
                .unwrap();
                view.write_bytes(query);
            })
        })
        .map_err(|error| error.to_string())?;
        wait_for(cx, window, view, |view| {
            view.completion_editor.reported_line_for_test() == Some(line)
        })
        .await?;
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| {
                assert!(!view.completion_editor.is_querying());
                assert!(view.suggest.completion_items.is_empty());
                key(view, "end", window, cx);
                key(view, "ctrl-u", window, cx);
            })
        })
        .map_err(|error| error.to_string())?;
        prompt(cx, window, view).await?;
        reports.push(serde_json::json!({"scenario":format!("cancel-{cancel}"), "stale_result_rejected":true, "native_buffer_verified":true}));
    }

    if std::env::var("PEBREL_COMPLETION_QA_PREDICTION").as_deref() == Ok("1") {
        let output = PathBuf::from(std::env::var_os("PEBREL_COMPLETION_QA_DIR").unwrap());
        assert_eq!(
            std::fs::read_to_string(output.join("prediction-source.txt")).unwrap().trim(),
            "History"
        );
        // AddToHistory requires ReadLine to have initialized the native editor.
        // Seed through the real prompt, so startup cannot silently omit history.
        let seed = "foreach ($mode in 'inline','popup','hybrid') { [Microsoft.PowerShell.PSConsoleReadLine]::AddToHistory(\"git switch qa/prediction-$mode\") }; 'ready' | Set-Content -LiteralPath (Join-Path $env:PEBREL_COMPLETION_QA_DIR 'prediction-history-ready.txt')";
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.replace_text_in_range(None, seed, window, cx);
                key(view, "enter", window, cx);
            })
        })
        .map_err(|error| error.to_string())?;
        wait_for(cx, window, view, |_| {
            std::fs::read_to_string(output.join("prediction-history-ready.txt"))
                .is_ok_and(|value| value.trim() == "ready")
        })
        .await?;
        for (style, name) in [
            (CompletionStyle::Inline, "inline"),
            (CompletionStyle::Popup, "popup"),
            (CompletionStyle::Hybrid, "hybrid"),
        ] {
            prompt(cx, window, view).await?;
            let prefix = format!("git switch qa/prediction-{}", &name[..2]);
            let expected = format!("git switch qa/prediction-{name}");
            let before = std::fs::read(repository.join(".git/HEAD")).unwrap();
            cx.update_window(window, |_, window, cx| {
                view.update(cx, |view, cx| {
                    mode(view, style, cx);
                    view.replace_text_in_range(None, &prefix[..prefix.len() - 1], window, cx);
                })
            })
            .map_err(|error| error.to_string())?;
            probe_buffer(cx, window, view, &prefix[..prefix.len() - 1]).await?;
            cx.update_window(window, |_, window, cx| {
                view.update(cx, |view, cx| {
                    view.replace_text_in_range(None, &prefix[prefix.len() - 1..], window, cx);
                })
            })
            .map_err(|error| error.to_string())?;
            wait_for(cx, window, view, |view| {
                view.session.as_ref().is_some_and(|session| {
                    let term = session.term.lock();
                    let visible =
                        term.grid().display_iter().map(|cell| cell.cell.c).collect::<String>();
                    visible.contains(&expected)
                        && crate::display::nebula_prompt_line_from_raw_grid(
                            &term,
                            term.grid().cursor.point,
                            "",
                            &view.suggest.suggest_env,
                        )
                        .is_none()
                })
            })
            .await?;
            cx.update_window(window, |_, window, cx| {
                view.update(cx, |view, cx| view.on_terminal_tab(&TerminalTab, window, cx))
            })
            .map_err(|error| error.to_string())?;
            accept_list(cx, window, view, style).await?;
            wait_for(cx, window, view, |view| view.suggest.screen_line.trim() == expected).await?;
            assert_eq!(std::fs::read(repository.join(".git/HEAD")).unwrap(), before);
            execute(cx, window, view, repository, &format!("qa/prediction-{name}")).await?;
            reports.push(serde_json::json!({"scenario":"native-history-prediction", "mode":format!("{style:?}"), "accepted":expected, "prediction_text_excluded":true}));
        }
        let command = "(Get-PSReadLineOption).PredictionSource.ToString() | Set-Content -LiteralPath (Join-Path $env:PEBREL_COMPLETION_QA_DIR 'prediction-after.txt')";
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.replace_text_in_range(None, command, window, cx);
                key(view, "enter", window, cx);
            })
        })
        .map_err(|error| error.to_string())?;
        wait_for(cx, window, view, |_| {
            std::fs::read_to_string(output.join("prediction-after.txt"))
                .is_ok_and(|value| value.trim() == "History")
        })
        .await?;
        prompt(cx, window, view).await?;
    }
    Ok(reports)
}
