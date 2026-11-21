use super::{SaveReason, SessionPersistence};
use crate::platform::session_end::test_support::set_pending;
use crate::session::{AgentSession, LayoutSession, Session, TabSession};

struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        set_pending(false);
    }
}

fn workspace() -> Session {
    Session::new(
        0,
        (0..5)
            .map(|index| {
                let mut tab = TabSession::single(format!("E:/fixture/{index}"), None, None);
                tab.layout = Some(LayoutSession::Pane {
                    custom_name: None,
                    launch: None,
                    cwd: tab.cwd.clone(),
                    agent: Some(AgentSession {
                        source: "pi".into(),
                        session_id: Some(format!("fixture-{index}")),
                        session_file: Some(format!("E:/fixture/{index}/session.jsonl")),
                    }),
                });
                tab
            })
            .collect(),
    )
}

fn checkpoint() -> (SessionPersistence, Session, Reset) {
    set_pending(false);
    let expected = workspace();
    let mut state = SessionPersistence::default();
    state.save_with(Some(expected.clone()), SaveReason::Checkpoint, |_| Ok(())).unwrap();
    (state, expected, Reset)
}

#[test]
fn system_shutdown_cannot_replace_five_tabs_and_pi_targets_with_three_or_zero() {
    let (mut state, expected, _reset) = checkpoint();
    set_pending(true);
    let mut partial = expected.clone();
    partial.tabs.truncate(3);
    for (session, reason) in [
        (Some(partial), SaveReason::TabsClosed),
        (Some(Session::new(0, vec![])), SaveReason::WindowClose),
        (None, SaveReason::Quit),
    ] {
        state.save_with(session, reason, |_| Ok(())).unwrap();
        assert_eq!(state.saved.as_ref().unwrap().tabs, expected.tabs);
    }
    assert!(!state.quitting, "system teardown must not latch an uncancellable ordinary quit");
}

#[test]
fn system_shutdown_checkpoint_cannot_drop_pi_identity_or_another_window() {
    let (mut state, expected, _reset) = checkpoint();
    set_pending(true);
    let mut partial = expected.clone();
    partial.tabs.truncate(1);
    partial.tabs[0].layout = None;
    state.save_with(Some(partial), SaveReason::Checkpoint, |_| Ok(())).unwrap();
    assert_eq!(state.saved.as_ref().unwrap().tabs, expected.tabs);
}

#[test]
fn canceled_system_shutdown_accepts_new_work_and_explicit_tab_closure() {
    let (mut state, mut current, _reset) = checkpoint();
    set_pending(true);
    state.save_with(Some(Session::new(0, vec![])), SaveReason::Quit, |_| Ok(())).unwrap();
    set_pending(false);
    current.tabs.truncate(1);
    state.save_with(Some(current.clone()), SaveReason::Checkpoint, |_| Ok(())).unwrap();
    assert_eq!(state.saved.as_ref().unwrap().tabs, current.tabs);
    state.save_with(Some(Session::new(0, vec![])), SaveReason::TabsClosed, |_| Ok(())).unwrap();
    assert!(state.saved.as_ref().unwrap().tabs.is_empty());
}

#[test]
fn later_shutdown_uses_the_new_snapshot_after_a_cancel() {
    let (mut state, mut current, _reset) = checkpoint();
    set_pending(true);
    state.save_with(None, SaveReason::Quit, |_| Ok(())).unwrap();
    set_pending(false);
    current.tabs.truncate(2);
    state.save_with(Some(current.clone()), SaveReason::TabsClosed, |_| Ok(())).unwrap();
    set_pending(true);
    state.save_with(Some(Session::new(0, vec![])), SaveReason::WindowClose, |_| Ok(())).unwrap();
    assert_eq!(state.saved.as_ref().unwrap().tabs, current.tabs);
}

#[test]
fn protected_final_write_failure_remains_retryable_and_does_not_save_empty() {
    let (mut state, expected, _reset) = checkpoint();
    set_pending(true);
    assert!(
        state
            .save_with(Some(Session::new(0, vec![])), SaveReason::WindowClose, |_| Err(
                std::io::Error::other("storage unavailable")
            ))
            .is_err()
    );
    assert!(!state.saved.as_ref().unwrap().clean_exit);
    let mut written = None;
    state
        .save_with(None, SaveReason::Checkpoint, |session| {
            written = Some(session.clone());
            Ok(())
        })
        .unwrap();
    let saved = written.expect("failed final snapshot must actually be retried");
    assert!(saved.clean_exit);
    assert_eq!(saved.tabs, expected.tabs);
}

#[test]
fn repeated_system_notifications_do_not_recapture_partial_state_or_rewrite() {
    let (mut state, expected, _reset) = checkpoint();
    set_pending(true);
    state.save_with(None, SaveReason::WindowClose, |_| Ok(())).unwrap();
    set_pending(true);
    state
        .save_with(Some(Session::new(0, vec![])), SaveReason::WindowClose, |_| {
            panic!("unchanged protected snapshot must not be rewritten")
        })
        .unwrap();
    assert_eq!(state.saved.as_ref().unwrap().tabs, expected.tabs);
}

#[test]
fn system_shutdown_without_a_checkpoint_keeps_the_first_nonempty_snapshot() {
    let _reset = Reset;
    let mut state = SessionPersistence::default();
    set_pending(true);
    state.save_with(None, SaveReason::Quit, |_| panic!("no snapshot exists")).unwrap();
    let expected = workspace();
    state.save_with(Some(expected.clone()), SaveReason::TabsClosed, |_| Ok(())).unwrap();
    state.save_with(Some(Session::new(0, vec![])), SaveReason::WindowClose, |_| Ok(())).unwrap();
    assert_eq!(state.saved.as_ref().unwrap().tabs, expected.tabs);
}

#[test]
fn system_shutdown_does_not_resurrect_tabs_closed_before_its_notification() {
    let (mut state, _, _reset) = checkpoint();
    state.save_with(Some(Session::new(0, vec![])), SaveReason::TabsClosed, |_| Ok(())).unwrap();
    set_pending(true);
    state.save_with(None, SaveReason::Quit, |_| Ok(())).unwrap();
    assert!(state.saved.as_ref().unwrap().tabs.is_empty());
}

#[test]
fn pi_explicitly_exited_before_the_system_notification_is_not_revived() {
    let (mut state, mut current, _reset) = checkpoint();
    for tab in &mut current.tabs {
        if let Some(LayoutSession::Pane { agent, .. }) = &mut tab.layout {
            *agent = None;
        }
    }
    state.save_with(Some(current.clone()), SaveReason::Checkpoint, |_| Ok(())).unwrap();
    set_pending(true);
    state.save_with(Some(Session::new(0, vec![])), SaveReason::WindowClose, |_| Ok(())).unwrap();
    assert_eq!(state.saved.as_ref().unwrap().tabs, current.tabs);
}

#[test]
fn an_empty_startup_during_system_shutdown_never_erases_storage() {
    let _reset = Reset;
    let mut state = SessionPersistence::default();
    set_pending(true);
    state
        .save_with(Some(Session::new(0, vec![])), SaveReason::WindowClose, |_| {
            panic!("there is no workspace to replace storage with")
        })
        .unwrap();
    assert!(state.saved.is_none());
}

#[test]
fn an_elevated_session_writes_the_snapshot_on_every_reason() {
    let _reset = Reset;
    set_pending(false);
    let expected = workspace();
    // Same constructor as application startup: elevation is not an input to the
    // workspace snapshot, so an elevated process persists like any other.
    for reason in [
        SaveReason::Checkpoint,
        SaveReason::TabsClosed,
        SaveReason::WindowClose,
        SaveReason::Quit,
    ] {
        let mut state = SessionPersistence::default();
        let mut wrote = 0;
        state
            .save_with(Some(expected.clone()), reason, |session| {
                assert_eq!(session.tabs.len(), expected.tabs.len());
                wrote += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(wrote, 1, "save reason {reason:?} must reach shared storage");
    }
}

#[test]
fn scheduled_update_reads_the_workspace_before_the_first_write() {
    let _reset = Reset;
    set_pending(false);
    // The handoff still needs a complete workspace even when nothing has been
    // written yet, which is the case right after a failed checkpoint.
    let mut state = SessionPersistence::default();
    let expected = workspace();
    let _ = state
        .save_with(Some(expected.clone()), SaveReason::Checkpoint, |_| {
            Err(std::io::Error::other("storage is unavailable right now"))
        })
        .ok();
    let windows = state.update_windows().expect("the checkpoint still holds the workspace");
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].tabs.len(), expected.tabs.len());
}
