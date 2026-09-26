//! 本机快照在 UI 线程采集；编码、sidecar 和会话提交交给后台执行器。

use super::super::session_persistence::{LocalSnapshot, prepare_windows};
use super::*;
use crate::recent_output::storage::{Archive, OutputStore};
use futures::{FutureExt, channel::oneshot, future::Shared};
use std::io;

pub(super) struct OutputPersistence {
    store: OutputStore,
    session_path: PathBuf,
    tail: Option<Shared<oneshot::Receiver<()>>>,
    latest_archive: Option<Archive>,
    clear_epoch: u64,
}

impl Default for OutputPersistence {
    fn default() -> Self {
        Self {
            store: OutputStore::new(crate::recent_output::storage::archive_path()),
            session_path: crate::session::session_path(),
            tail: None,
            latest_archive: None,
            clear_epoch: 0,
        }
    }
}

pub(super) fn settings_changed(cx: &mut App) {
    if cx
        .try_global::<crate::gpui_shell::config::Settings>()
        .is_none_or(|settings| settings.command_output_enabled)
    {
        return;
    }
    let registry = cx.global_mut::<WindowRegistry>();
    registry.session_persistence.disable_output_references();
    let state = &mut registry.output_persistence;
    state.clear_epoch += 1;
    state.latest_archive = None;
    // Cancel stale captures; the serialized checkpoint must also finish before quit.
    state.store.next_generation();
    let snapshots = capture(None, cx);
    let checkpoint = enqueue_with_mode(snapshots, SaveReason::Checkpoint, true, cx);
    cx.spawn(async move |_| {
        if let Err(error) = checkpoint.await {
            log::warn!("Could not checkpoint disabled command output: {error}");
        }
    })
    .detach();
}

fn capture(mut current: Option<LocalSnapshot>, cx: &App) -> Vec<(bool, LocalSnapshot)> {
    let mut entries = cx
        .global::<WindowRegistry>()
        .entries
        .iter()
        .filter(|entry| entry.role == WindowRole::Regular)
        .cloned()
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.runtime_window_id);
    let active = cx
        .active_window()
        .filter(|handle| entries.iter().any(|entry| entry.handle == *handle))
        .or_else(|| {
            entries.iter().max_by_key(|entry| entry.last_activated).map(|entry| entry.handle)
        });
    entries
        .into_iter()
        .filter_map(|entry| {
            let snapshot = if current
                .as_ref()
                .is_some_and(|snapshot| snapshot.window_id == entry.runtime_window_id)
            {
                current.take()?
            } else {
                entry.workspace.upgrade()?.read(cx).snapshot_local_session(cx)
            };
            Some((active == Some(entry.handle), snapshot))
        })
        .collect()
}

/// 按采集顺序串行提交，关窗/退出必须等待成功，不能把已入队当作已保存。
pub(in crate::gpui_shell::workspace) fn save(
    current: Option<LocalSnapshot>,
    reason: SaveReason,
    cx: &mut App,
) -> gpui::Task<io::Result<bool>> {
    let snapshots = capture(current, cx);
    enqueue(snapshots, reason, cx)
}

fn enqueue(
    snapshots: Vec<(bool, LocalSnapshot)>,
    reason: SaveReason,
    cx: &mut App,
) -> gpui::Task<io::Result<bool>> {
    enqueue_with_mode(snapshots, reason, false, cx)
}

fn enqueue_with_mode(
    snapshots: Vec<(bool, LocalSnapshot)>,
    reason: SaveReason,
    without_output: bool,
    cx: &mut App,
) -> gpui::Task<io::Result<bool>> {
    let epoch = cx.global::<WindowRegistry>().output_persistence.clear_epoch;
    let (done, receiver) = oneshot::channel();
    let previous =
        cx.global_mut::<WindowRegistry>().output_persistence.tail.replace(receiver.shared());
    cx.spawn(async move |cx| {
        let result = async {
            if let Some(previous) = previous {
                let _ = previous.await;
            }
            let (mut session, mut archive) =
                cx.background_executor().spawn(async move { prepare_windows(snapshots) }).await?;
            let job = cx.update(|cx| {
                let enabled =
                    cx.global::<crate::gpui_shell::config::Settings>().command_output_enabled;
                let registry = cx.global_mut::<WindowRegistry>();
                // 清除前采集、清除后才完成编码的任务只能保存布局。
                let discard_output =
                    without_output || epoch != registry.output_persistence.clear_epoch || !enabled;
                if discard_output {
                    if let Some(session) = &mut session {
                        for tab in &mut session.tabs {
                            tab.output_refs.clear();
                        }
                    }
                    archive = Archive::new(Default::default());
                }
                // 最后一扇窗口已关闭时，沿用同一份快照的输出，不用空档覆盖它。
                if session.as_ref().is_some_and(|session| !session.tabs.is_empty())
                    || reason == SaveReason::TabsClosed
                {
                    registry.output_persistence.latest_archive = Some(archive);
                }
                let pending = registry.session_persistence.prepare(session, reason)?;
                let archive = registry
                    .output_persistence
                    .latest_archive
                    .clone()
                    .unwrap_or_else(|| Archive::new(Default::default()));
                let store = registry.output_persistence.store.clone();
                let generation = store.next_generation();
                Some((
                    pending,
                    archive,
                    store,
                    generation,
                    registry.output_persistence.session_path.clone(),
                    discard_output,
                ))
            });
            let Some((pending, archive, store, generation, path, without_output)) = job else {
                return Ok(true);
            };
            let (pending, result) = cx
                .background_executor()
                .spawn(async move {
                    let references = crate::session::load_local_from(&path)
                        .map(|session| {
                            session
                                .tabs
                                .into_iter()
                                .flat_map(|tab| tab.output_refs)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let result = if without_output {
                        store.publish_without_output(generation, || {
                            crate::session::save_local_to(&path, &pending.session)
                        })
                    } else {
                        store.save_referenced(generation, &archive, Some(&references), || {
                            crate::session::save_local_to(&path, &pending.session)
                        })
                    };
                    (pending, result)
                })
                .await;
            if !result? {
                return Ok(false);
            }
            Ok(cx.update(|cx| {
                cx.global_mut::<WindowRegistry>().session_persistence.complete(pending)
            }))
        }
        .await;
        let _ = done.send(());
        result
    })
}

/// 清除作用于所有本机窗格，不清屏、不关终端；任务不依赖设置页存活。
pub(crate) fn clear(cx: &mut App) -> gpui::Task<io::Result<bool>> {
    let views = cx
        .global::<WindowRegistry>()
        .entries
        .iter()
        .filter_map(|entry| entry.workspace.upgrade())
        .flat_map(|workspace| {
            workspace
                .read(cx)
                .tabs
                .iter()
                .flat_map(|tab| match tab {
                    WorkspaceTab::Terminal { panes, .. } => {
                        panes.iter().map(|pane| pane.view.clone()).collect()
                    },
                    _ => Vec::new(),
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for view in views {
        view.update(cx, |view, _| view.clear_recent_output());
    }
    let registry = cx.global_mut::<WindowRegistry>();
    registry.session_persistence.clear_output_references();
    let state = &mut registry.output_persistence;
    state.clear_epoch += 1;
    state.latest_archive = None;
    state.store.next_generation();
    let store = state.store.clone();
    let (done, receiver) = oneshot::channel();
    let previous = state.tail.replace(receiver.shared());
    cx.spawn(async move |cx| {
        if let Some(previous) = previous {
            let _ = previous.await;
        }
        let result = cx
            .background_executor()
            .spawn(async move { store.clear(store.next_generation()) })
            .await;
        let _ = done.send(());
        result
    })
}

#[cfg(all(test, feature = "gpui-test-support"))]
pub(crate) fn set_command_output_test_path(cx: &mut App, path: PathBuf) {
    let state = &mut cx.global_mut::<WindowRegistry>().output_persistence;
    state.session_path = path.with_file_name("session.json");
    state.store = OutputStore::new(path);
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;

    #[gpui::test]
    fn clear_invalidates_queued_output_and_does_not_erase_later_commands(
        cx: &mut gpui::TestAppContext,
    ) {
        super::super::transfer_tests::initialize_test(cx);
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("output.json");
        let make_snapshot = |label: &str| {
            use nebula_terminal::{
                event::VoidListener,
                term::{Config, Term},
                vte::ansi,
            };
            let mut term = Term::new(
                Config::default(),
                &crate::gpui_shell::terminal::session::GridSize { columns: 80, screen_lines: 24 },
                VoidListener,
            );
            let mut parser: ansi::Processor = ansi::Processor::new();
            let mut recent = crate::recent_output::RecentOutput::default();
            parser.advance(&mut term, label.as_bytes());
            recent.begin(term.grid());
            parser.advance(&mut term, b"\r\nresult\r\n");
            recent.finish(term.grid());
            LocalSnapshot {
                session: crate::session::Session::new(
                    0,
                    vec![crate::session::TabSession::single("C:/test".into(), None, None)],
                ),
                outputs: vec![vec![(1, recent.snapshot(term.grid()))]],
                window_id: 1,
            }
        };
        let (old, clearing) = cx.update(|cx| {
            let mut settings =
                crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord);
            settings.command_output_enabled = true;
            cx.set_global(settings);
            let registry = cx.global_mut::<WindowRegistry>();
            registry.session_persistence =
                super::super::super::session_persistence::tests::ordinary_window();
            registry.output_persistence.store = OutputStore::new(output.clone());
            registry.output_persistence.session_path = dir.path().join("session.json");
            let old = enqueue(vec![(true, make_snapshot("old"))], SaveReason::Checkpoint, cx);
            let clearing = clear(cx);
            (old, clearing)
        });
        cx.run_until_parked();
        let _ = old.now_or_never().expect("old save finishes");
        assert!(clearing.now_or_never().unwrap().unwrap());
        assert!(!output.exists(), "queued snapshots must not resurrect cleared output");
        let snapshot = make_snapshot("new");
        let expected = snapshot.outputs[0][0].1.clone();
        let new = cx.update(|cx| enqueue(vec![(true, snapshot)], SaveReason::Checkpoint, cx));
        cx.run_until_parked();
        assert!(new.now_or_never().unwrap().unwrap());
        let session: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("session.json")).unwrap())
                .unwrap();
        let reference = session["local_output_refs"][0][0].as_str().unwrap();
        let mut archive = Archive::read_from(&output).unwrap();
        let restored = archive.take_records(reference).expect("new command is published");
        assert_eq!(
            serde_json::to_value(restored).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }

    #[gpui::test]
    fn runtime_close_waits_for_publication_and_reports_write_failure(
        cx: &mut gpui::TestAppContext,
    ) {
        super::super::transfer_tests::initialize_test(cx);
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("not-a-directory");
        std::fs::write(&blocker, b"block").unwrap();
        let (handle, workspace, close) = cx.update(|cx| {
            let (id, workspace) = super::super::transfer_tests::open_test_window(cx, 1);
            let registry = cx.global_mut::<WindowRegistry>();
            registry.session_persistence =
                super::super::super::session_persistence::tests::ordinary_window();
            registry.output_persistence.store = OutputStore::new(blocker.join("output.json"));
            registry.output_persistence.session_path = dir.path().join("session.json");
            let handle = entry_by_id(id, cx).unwrap().handle;
            let close = workspace
                .update(cx, |workspace, cx| workspace.runtime_close_window(Some(id), cx))
                .unwrap();
            assert!(cx.windows().contains(&handle));
            (handle, workspace, close)
        });
        cx.run_until_parked();
        let result = close.now_or_never().expect("close task must finish");
        assert_eq!(result.unwrap_err().code, "session_save_failed");
        cx.update(|cx| {
            assert!(cx.windows().contains(&handle));
            assert!(!workspace.read(cx).window_close_pending);
            cx.global_mut::<WindowRegistry>().output_persistence.store =
                OutputStore::new(dir.path().join("output.json"));
        });
        let close = cx
            .update(|cx| {
                workspace.update(cx, |workspace, cx| workspace.runtime_close_window(None, cx))
            })
            .unwrap();
        cx.run_until_parked();
        let result = close.now_or_never().expect("close task must finish").unwrap();
        assert_eq!(result["action"]["closed"], true);
        assert!(dir.path().join("output.json").is_file());
        assert!(dir.path().join("session.json").is_file());
        cx.update(|cx| assert!(!cx.windows().contains(&handle)));
    }

    #[gpui::test]
    fn application_quit_publishes_output_before_finishing(cx: &mut gpui::TestAppContext) {
        super::super::transfer_tests::initialize_test(cx);
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("output.json");
        let session = dir.path().join("session.json");
        cx.update(|cx| {
            super::super::transfer_tests::open_test_window(cx, 1);
            let registry = cx.global_mut::<WindowRegistry>();
            registry.session_persistence =
                super::super::super::session_persistence::tests::ordinary_window();
            registry.output_persistence.store = OutputStore::new(output.clone());
            registry.output_persistence.session_path = session.clone();
            super::super::shutdown::quit_all(cx);
            assert!(!session.exists());
        });
        cx.run_until_parked();
        assert!(output.is_file(), "quit must save the output sidecar");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(session).unwrap()).unwrap();
        assert_eq!(value["tabs"].as_array().unwrap().len(), 1);
        assert_eq!(value["clean_exit"], true);
    }

    #[gpui::test]
    fn application_quit_output_failure_is_retryable(cx: &mut gpui::TestAppContext) {
        super::super::transfer_tests::initialize_test(cx);
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("not-a-directory");
        std::fs::write(&blocker, b"block").unwrap();
        cx.update(|cx| {
            super::super::transfer_tests::open_test_window(cx, 1);
            let registry = cx.global_mut::<WindowRegistry>();
            registry.session_persistence =
                super::super::super::session_persistence::tests::ordinary_window();
            registry.output_persistence.store = OutputStore::new(blocker.join("output.json"));
            registry.output_persistence.session_path = dir.path().join("session.json");
            super::super::shutdown::quit_all(cx);
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert!(!cx.global::<WindowRegistry>().quit_pending, "failed save must cancel quit")
        });
        assert!(!dir.path().join("session.json").exists());
    }

    #[gpui::test]
    fn window_close_waits_for_output_and_session_publication(cx: &mut gpui::TestAppContext) {
        super::super::transfer_tests::initialize_test(cx);
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("output.json");
        let session = dir.path().join("session.json");
        let handle = cx.update(|cx| {
            let (id, workspace) = super::super::transfer_tests::open_test_window(cx, 1);
            let registry = cx.global_mut::<WindowRegistry>();
            registry.session_persistence =
                super::super::super::session_persistence::tests::ordinary_window();
            registry.output_persistence.store = OutputStore::new(output.clone());
            registry.output_persistence.session_path = session.clone();
            let handle = entry_by_id(id, cx).unwrap().handle;
            handle
                .update(cx, |_, window, cx| {
                    workspace.update(cx, |workspace, cx| {
                        assert!(!workspace.close_window_after_documents(window, cx));
                        assert!(workspace.window_close_pending);
                    })
                })
                .unwrap();
            assert!(!session.exists());
            handle
        });
        cx.run_until_parked();
        assert!(output.is_file(), "closing must publish the output sidecar");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(session).unwrap()).unwrap();
        assert_eq!(value["tabs"].as_array().unwrap().len(), 1);
        assert_eq!(value["clean_exit"], true);
        cx.update(|cx| assert!(!cx.windows().contains(&handle)));
    }

    #[gpui::test]
    fn window_close_preserves_document_edited_while_save_is_pending(cx: &mut gpui::TestAppContext) {
        check_pending_close_draft(cx, false);
    }

    #[gpui::test]
    fn runtime_close_preserves_document_edited_while_save_is_pending(
        cx: &mut gpui::TestAppContext,
    ) {
        check_pending_close_draft(cx, true);
    }

    fn check_pending_close_draft(cx: &mut gpui::TestAppContext, runtime: bool) {
        super::super::transfer_tests::initialize_test(cx);
        let dir = tempfile::tempdir().unwrap();
        let document = dir.path().join("draft.md");
        std::fs::write(&document, "original").unwrap();
        let (handle, workspace) = cx.update(|cx| {
            let (id, workspace) = super::super::transfer_tests::open_test_window(cx, 1);
            let handle = entry_by_id(id, cx).unwrap().handle;
            handle
                .update(cx, |_, window, cx| {
                    workspace.update(cx, |workspace, cx| {
                        workspace.open_document_at_startup(document.clone(), window, cx);
                    });
                })
                .unwrap();
            (handle, workspace)
        });
        cx.run_until_parked();
        let (release, waiting) = oneshot::channel();
        cx.update(|cx| {
            let registry = cx.global_mut::<WindowRegistry>();
            registry.session_persistence =
                super::super::super::session_persistence::tests::ordinary_window();
            registry.output_persistence.store = OutputStore::new(dir.path().join("output.json"));
            registry.output_persistence.session_path = dir.path().join("session.json");
            registry.output_persistence.tail = Some(waiting.shared());
            handle
                .update(cx, |_, window, cx| {
                    workspace.update(cx, |workspace, cx| {
                        if runtime {
                            workspace.runtime_close_window(None, cx).unwrap().detach();
                        } else {
                            assert!(!workspace.should_close_window(window, cx));
                        }
                        assert!(workspace.window_close_pending);
                    });
                })
                .unwrap();
        });
        cx.run_until_parked();
        let file = cx.update(|cx| {
            let file = workspace.read(cx).tabs.iter().find_map(|tab| tab.file_editor(cx)).unwrap();
            handle
                .update(cx, |_, window, cx| {
                    crate::gpui_shell::file_editor::tests::replace_draft(
                        &file,
                        "new unsaved draft",
                        window,
                        cx,
                    );
                })
                .unwrap();
            file
        });
        cx.run_until_parked();
        cx.update(|cx| assert!(file.read(cx).is_dirty()));
        release.send(()).unwrap();
        cx.run_until_parked();
        cx.update(|cx| {
            assert!(
                cx.windows().contains(&handle),
                "new draft must prevent asynchronous window destruction"
            );
            assert!(!workspace.read(cx).window_close_pending);
            assert_eq!(file.read(cx).draft(cx).as_ref(), "new unsaved draft");
        });
        assert_eq!(std::fs::read_to_string(document).unwrap(), "original");
    }

    #[gpui::test]
    fn disabling_output_queues_durable_reference_free_session_without_erasing_old_history(
        cx: &mut gpui::TestAppContext,
    ) {
        super::super::transfer_tests::initialize_test(cx);
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("output.json");
        let session_path = dir.path().join("session.json");
        let mut old_session = crate::session::Session::new(
            0,
            vec![crate::session::TabSession::single("C:/test".into(), None, None)],
        );
        old_session.tabs[0].output_refs = vec!["old".into()];
        use nebula_terminal::{
            event::VoidListener,
            term::{Config, Term},
            vte::ansi,
        };
        let mut term = Term::new(
            Config::default(),
            &crate::gpui_shell::terminal::session::GridSize { columns: 80, screen_lines: 24 },
            VoidListener,
        );
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(&mut term, b"C:>old");
        let mut recent = crate::recent_output::RecentOutput::default();
        recent.begin(term.grid());
        parser.advance(&mut term, b"\r\nold output\r\n");
        recent.finish(term.grid());
        let old_archive = Archive::new(std::collections::BTreeMap::from([(
            "old".into(),
            recent.snapshot(term.grid()),
        )]));
        old_archive.write_to(&output).unwrap();
        crate::session::save_local_to(&session_path, &old_session).unwrap();
        let (release, waiting) = oneshot::channel();
        let queued = cx.update(|cx| {
            let mut settings =
                crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord);
            settings.command_output_enabled = true;
            cx.set_global(settings);
            let registry = cx.global_mut::<WindowRegistry>();
            registry.session_persistence =
                super::super::super::session_persistence::tests::ordinary_window();
            registry.output_persistence.store = OutputStore::new(output.clone());
            registry.output_persistence.session_path = session_path.clone();
            registry.output_persistence.tail = Some(waiting.shared());
            enqueue(
                vec![(
                    true,
                    LocalSnapshot {
                        session: old_session.clone(),
                        outputs: vec![vec![(1, Vec::new())]],
                        window_id: 1,
                    },
                )],
                SaveReason::Checkpoint,
                cx,
            )
        });
        cx.run_until_parked();
        cx.update(|cx| {
            cx.global_mut::<crate::gpui_shell::config::Settings>().command_output_enabled = false;
        });
        cx.run_until_parked();
        release.send(()).unwrap();
        cx.run_until_parked();
        assert!(queued.now_or_never().is_some());
        assert!(
            cx.update(|cx| cx
                .global::<WindowRegistry>()
                .session_persistence
                .update_windows()
                .is_ok()),
            "disabling must finish its durable checkpoint before acknowledging the session"
        );
        assert!(
            crate::session::load_local_from(&session_path)
                .unwrap()
                .tabs
                .iter()
                .all(|tab| tab.output_refs.is_empty())
        );
        let mut after = Archive::read_from(&output).unwrap();
        assert!(after.take_records("old").is_some(), "the toggle must preserve old history");
    }

    #[gpui::test]
    fn disabling_during_publication_serializes_reference_free_checkpoint(
        cx: &mut gpui::TestAppContext,
    ) {
        super::super::transfer_tests::initialize_test(cx);
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("output.json");
        let session_path = dir.path().join("session.json");
        let mut old = crate::session::Session::new(
            0,
            vec![crate::session::TabSession::single("C:/test".into(), None, None)],
        );
        old.tabs[0].output_refs = vec!["old".into()];
        let mut new = old.clone();
        new.tabs[0].output_refs = vec!["new".into()];
        use nebula_terminal::{
            event::VoidListener,
            term::{Config, Term},
            vte::ansi,
        };
        let mut term = Term::new(
            Config::default(),
            &crate::gpui_shell::terminal::session::GridSize { columns: 80, screen_lines: 24 },
            VoidListener,
        );
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(&mut term, b"C:>old");
        let mut recent = crate::recent_output::RecentOutput::default();
        recent.begin(term.grid());
        parser.advance(&mut term, b"\r\nold output\r\n");
        recent.finish(term.grid());
        let records = recent.snapshot(term.grid());
        Archive::new(std::collections::BTreeMap::from([("old".into(), records.clone())]))
            .write_to(&output)
            .unwrap();
        crate::session::save_local_to(&session_path, &old).unwrap();
        let store = OutputStore::new(output.clone());
        let generation = store.next_generation();
        cx.update(|cx| {
            let mut settings =
                crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord);
            settings.command_output_enabled = true;
            cx.set_global(settings);
            let registry = cx.global_mut::<WindowRegistry>();
            let state = &mut registry.session_persistence;
            *state = super::super::super::session_persistence::tests::ordinary_window();
            let pending = state.prepare(Some(old.clone()), SaveReason::Checkpoint).unwrap();
            assert!(state.complete(pending));
            registry.output_persistence.store = store.clone();
            registry.output_persistence.session_path = session_path.clone();
        });
        let (entered, started) = std::sync::mpsc::channel();
        let (release, resume) = std::sync::mpsc::channel();
        let publishing = std::thread::spawn({
            let store = store.clone();
            let session_path = session_path.clone();
            move || {
                let archive =
                    Archive::new(std::collections::BTreeMap::from([("new".into(), records)]));
                store.save_referenced(generation, &archive, Some(&["old".into()]), || {
                    entered.send(()).unwrap();
                    resume.recv().unwrap();
                    crate::session::save_local_to(&session_path, &new)
                })
            }
        });
        started.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        cx.update(|cx| {
            cx.global_mut::<crate::gpui_shell::config::Settings>().command_output_enabled = false;
        });
        assert!(cx.update(|cx| {
            cx.global::<WindowRegistry>().session_persistence.update_windows().is_err()
        }));
        assert_eq!(
            crate::session::load_local_from(&session_path).unwrap().tabs[0].output_refs,
            ["old"],
            "the checkpoint must wait behind in-flight publication"
        );
        release.send(()).unwrap();
        assert!(publishing.join().unwrap().unwrap());
        cx.run_until_parked();
        assert!(cx.update(|cx| {
            cx.global::<WindowRegistry>().session_persistence.update_windows().is_ok()
        }));
        let restored = crate::session::load_local_from(&session_path).unwrap();
        assert!(restored.tabs[0].output_refs.is_empty());
        let mut remaining = Archive::read_from(&output).unwrap();
        assert!(remaining.take_records("old").is_some(), "existing valid history is not clear");
    }

    #[gpui::test]
    fn disabling_then_enabling_output_invalidates_queued_snapshot(cx: &mut gpui::TestAppContext) {
        super::super::transfer_tests::initialize_test(cx);
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("output.json");
        let session = dir.path().join("session.json");
        let (release, waiting) = oneshot::channel();
        let save = cx.update(|cx| {
            use nebula_terminal::{
                event::VoidListener,
                term::{Config, Term},
                vte::ansi,
            };
            let mut term = Term::new(
                Config::default(),
                &crate::gpui_shell::terminal::session::GridSize { columns: 80, screen_lines: 24 },
                VoidListener,
            );
            let mut parser: ansi::Processor = ansi::Processor::new();
            parser.advance(&mut term, b"old command");
            let mut recent = crate::recent_output::RecentOutput::default();
            recent.begin(term.grid());
            parser.advance(&mut term, b"\r\nold output\r\n");
            recent.finish(term.grid());
            let mut settings =
                crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord);
            settings.command_output_enabled = true;
            cx.set_global(settings);
            let registry = cx.global_mut::<WindowRegistry>();
            registry.session_persistence =
                super::super::super::session_persistence::tests::ordinary_window();
            registry.output_persistence.store = OutputStore::new(output.clone());
            registry.output_persistence.session_path = session.clone();
            registry.output_persistence.tail = Some(waiting.shared());
            enqueue(
                vec![(
                    true,
                    LocalSnapshot {
                        session: crate::session::Session::new(
                            0,
                            vec![crate::session::TabSession::single("C:/test".into(), None, None)],
                        ),
                        outputs: vec![vec![(1, recent.snapshot(term.grid()))]],
                        window_id: 1,
                    },
                )],
                SaveReason::Checkpoint,
                cx,
            )
        });
        cx.run_until_parked();
        cx.update(|cx| {
            cx.global_mut::<crate::gpui_shell::config::Settings>().command_output_enabled = false
        });
        cx.run_until_parked();
        cx.update(|cx| {
            cx.global_mut::<crate::gpui_shell::config::Settings>().command_output_enabled = true
        });
        cx.run_until_parked();
        release.send(()).unwrap();
        cx.run_until_parked();
        assert!(save.now_or_never().unwrap().unwrap());
        let restored = crate::session::load_local_from(&session).unwrap();
        assert!(
            restored.tabs.iter().all(|tab| tab.output_refs.is_empty()),
            "re-enabling must not publish pre-disable snapshots"
        );
    }

    #[gpui::test]
    fn failed_output_write_keeps_window_open_for_retry(cx: &mut gpui::TestAppContext) {
        super::super::transfer_tests::initialize_test(cx);
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("not-a-directory");
        std::fs::write(&blocker, b"block").unwrap();
        let session = dir.path().join("session.json");
        let (handle, workspace) = cx.update(|cx| {
            let (id, workspace) = super::super::transfer_tests::open_test_window(cx, 1);
            let registry = cx.global_mut::<WindowRegistry>();
            registry.session_persistence =
                super::super::super::session_persistence::tests::ordinary_window();
            registry.output_persistence.store = OutputStore::new(blocker.join("output.json"));
            registry.output_persistence.session_path = session.clone();
            let handle = entry_by_id(id, cx).unwrap().handle;
            handle
                .update(cx, |_, window, cx| {
                    workspace.update(cx, |workspace, cx| {
                        assert!(!workspace.close_window_after_documents(window, cx));
                    })
                })
                .unwrap();
            (handle, workspace)
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert!(
                cx.windows().contains(&handle),
                "a failed save must not destroy the live terminal"
            );
            assert!(!workspace.read(cx).window_close_pending, "close must be retryable");
        });
        assert!(!session.exists(), "failed sidecar must not publish new session references");
    }

    #[gpui::test]
    fn autosave_tick_publishes_through_background_output_store(cx: &mut gpui::TestAppContext) {
        super::super::transfer_tests::initialize_test(cx);
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("output.json");
        let session = dir.path().join("session.json");
        cx.update(|cx| {
            super::super::transfer_tests::open_test_window(cx, 1);
            let registry = cx.global_mut::<WindowRegistry>();
            registry.session_persistence =
                super::super::super::session_persistence::tests::ordinary_window();
            registry.output_persistence.store = OutputStore::new(output.clone());
            registry.output_persistence.session_path = session.clone();
            autosave_tick(cx);
            assert!(!session.exists(), "autosave must not perform disk I/O in the UI callback");
        });
        cx.run_until_parked();
        assert!(output.is_file(), "autosave must write sidecar before session publication");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(session).unwrap()).unwrap();
        assert_eq!(value["tabs"].as_array().unwrap().len(), 1);
        assert_eq!(value["clean_exit"], false);
    }
}

pub(super) fn checkpoint(cx: &mut App) {
    let registry = cx.global::<WindowRegistry>();
    if registry.quit_pending
        || registry.output_persistence.tail.as_ref().is_some_and(|tail| tail.peek().is_none())
    {
        return;
    }
    let save = save(None, SaveReason::Checkpoint, cx);
    cx.spawn(async move |cx| match save.await {
        Ok(true) => cx.update(|cx| super::acknowledge_session_restore(cx)),
        Ok(false) => {},
        Err(error) => log::warn!("Could not checkpoint command output and session: {error}"),
    })
    .detach();
}
