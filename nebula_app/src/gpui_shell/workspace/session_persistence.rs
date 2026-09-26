use crate::session::Session;
pub(super) use crate::session::combine_sessions;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SaveReason {
    Checkpoint,
    WindowClose,
    TabsClosed,
    Quit,
}

pub(super) struct SessionPersistence {
    latest: Option<Session>,
    saved: Option<Session>,
    quitting: bool,
    isolated: bool,
}

impl Default for SessionPersistence {
    fn default() -> Self {
        Self {
            latest: None,
            saved: None,
            quitting: false,
            isolated: crate::platform::elevation::requires_isolation(),
        }
    }
}

impl SessionPersistence {
    pub(super) fn update_windows(&self) -> std::io::Result<Vec<Session>> {
        self.saved
            .clone()
            .ok_or_else(|| std::io::Error::other("No durable workspace snapshot"))?
            .into_update_windows()
    }

    pub(super) fn cancel_quit(&mut self) {
        self.quitting = false;
    }

    pub(super) fn save(
        &mut self,
        current: Option<Session>,
        reason: SaveReason,
    ) -> std::io::Result<()> {
        self.save_with(current, reason, crate::session::try_save)
    }

    fn save_with(
        &mut self,
        current: Option<Session>,
        reason: SaveReason,
        write: impl FnOnce(&Session) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        // An administrator window must not overwrite the ordinary workspace or
        // cause its privileged shell command to be restored in a later session.
        if self.isolated {
            return Ok(());
        }
        let retry_checkpoint = reason == SaveReason::Checkpoint
            && current.as_ref().is_none_or(|session| session.tabs.is_empty());
        let candidate = if self.quitting {
            if reason != SaveReason::Quit {
                return Ok(());
            }
            self.latest.clone()
        } else {
            match reason {
                SaveReason::Checkpoint => {
                    current.filter(|session| !session.tabs.is_empty()).or_else(|| {
                        self.latest
                            .as_ref()
                            .filter(|latest| self.saved.as_ref() != Some(latest))
                            .cloned()
                    })
                },
                SaveReason::WindowClose | SaveReason::TabsClosed => current,
                SaveReason::Quit => current
                    .filter(|session| !session.tabs.is_empty())
                    .or_else(|| self.latest.clone()),
            }
        };
        let Some(mut session) = candidate else { return Ok(()) };
        if !retry_checkpoint {
            session.clean_exit = matches!(reason, SaveReason::WindowClose | SaveReason::Quit);
        }
        self.latest = Some(session.clone());
        if self.saved.as_ref() == Some(&session) {
            self.quitting |= reason == SaveReason::Quit;
            return Ok(());
        }
        // A failed final write is cancellable: windows stay live and subsequent
        // checkpoints must still be able to save newly confirmed identities.
        write(&session)?;
        self.saved = Some(session);
        self.quitting |= reason == SaveReason::Quit;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_windows_never_write_shared_session_storage() {
        let mut state = SessionPersistence { isolated: true, ..SessionPersistence::default() };
        for reason in [
            SaveReason::Checkpoint,
            SaveReason::TabsClosed,
            SaveReason::WindowClose,
            SaveReason::Quit,
        ] {
            let _ = state.save_with(Some(sample_session()), reason, |_| {
                panic!("privileged session reached shared storage")
            });
        }
        assert!(state.latest.is_none());
    }
    use crate::session::{AgentSession, LayoutSession, TabSession};

    fn ordinary_window() -> SessionPersistence {
        // Hosted Windows runners can be elevated. These tests exercise ordinary
        // window persistence; privileged isolation has its own negative test.
        SessionPersistence { isolated: false, ..SessionPersistence::default() }
    }

    fn sample_session() -> Session {
        let mut tab = TabSession::single("D:/work".into(), Some("Workspace".into()), None);
        tab.layout = Some(LayoutSession::Pane {
            launch: None,
            cwd: tab.cwd.clone(),
            agent: Some(AgentSession {
                session_file: None,
                source: "claude".into(),
                session_id: Some("saved-42".into()),
            }),
        });
        Session::new(0, vec![tab])
    }

    fn save_to(
        state: &mut SessionPersistence,
        path: &std::path::Path,
        current: Option<Session>,
        reason: SaveReason,
    ) {
        let _ = state.save_with(current, reason, |session| crate::session::save_to(path, session));
    }

    #[test]
    fn closing_last_window_then_quitting_preserves_tabs_and_ai_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.json");
        let expected = sample_session();
        let mut state = ordinary_window();
        save_to(&mut state, &path, Some(expected.clone()), SaveReason::WindowClose);
        save_to(&mut state, &path, None, SaveReason::Checkpoint);
        save_to(&mut state, &path, None, SaveReason::Quit);
        let restored = crate::session::load_from(&path).unwrap();
        assert!(crate::session::should_restore(&restored));
        assert!(restored.clean_exit);
        assert_eq!(restored.tabs, expected.tabs);
    }

    #[test]
    fn process_kill_restores_the_last_checkpoint_without_an_exit_callback() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.json");
        let expected = sample_session();
        let mut state = ordinary_window();
        save_to(&mut state, &path, Some(expected.clone()), SaveReason::Checkpoint);
        drop(state);
        let restored = crate::session::load_from(&path).unwrap();
        assert!(crate::session::should_restore(&restored));
        assert!(crate::session::was_crash(&restored));
        assert_eq!(restored.tabs, expected.tabs);
    }

    #[test]
    fn teardown_events_cannot_overwrite_the_final_snapshot() {
        let mut state = ordinary_window();
        let expected = sample_session();
        let _ = state.save_with(Some(expected.clone()), SaveReason::Quit, |_| Ok(()));
        for reason in [SaveReason::Checkpoint, SaveReason::TabsClosed, SaveReason::WindowClose] {
            let _ = state.save_with(Some(Session::new(0, vec![])), reason, |_| {
                panic!("teardown must not write a second snapshot")
            });
        }
        let _ = state.save_with(Some(Session::new(0, vec![])), SaveReason::Quit, |_| {
            panic!("repeated quit must retain the frozen snapshot")
        });
        assert_eq!(state.saved.unwrap().tabs, expected.tabs);
    }

    #[test]
    fn explicitly_closing_all_tabs_does_not_resurrect_them() {
        let mut state = ordinary_window();
        let _ = state.save_with(Some(sample_session()), SaveReason::Checkpoint, |_| Ok(()));
        let _ = state.save_with(Some(Session::new(0, vec![])), SaveReason::TabsClosed, |_| Ok(()));
        let _ = state.save_with(None, SaveReason::Quit, |_| Ok(()));
        let saved = state.saved.unwrap();
        assert!(!crate::session::should_restore(&saved));
        assert!(saved.clean_exit);
    }

    #[test]
    fn an_empty_startup_or_auxiliary_window_cannot_erase_a_saved_session() {
        let mut state = ordinary_window();
        for current in [None, Some(Session::new(0, vec![]))] {
            let _ = state.save_with(current, SaveReason::Checkpoint, |_| {
                panic!("initial empty state must not reach storage")
            });
        }
        let _ = state.save_with(None, SaveReason::Quit, |_| panic!("no snapshot to write"));
    }

    #[test]
    fn failed_checkpoint_and_final_writes_are_retried() {
        let mut state = ordinary_window();
        let session = sample_session();
        let fail = |_: &Session| Err(std::io::Error::other("storage unavailable"));
        let _ = state.save_with(Some(session.clone()), SaveReason::Checkpoint, fail);
        assert!(state.saved.is_none());
        let _ = state.save_with(Some(session.clone()), SaveReason::Checkpoint, |_| Ok(()));
        assert_eq!(state.saved.as_ref(), Some(&session));
        let _ = state.save_with(Some(session.clone()), SaveReason::Quit, fail);
        let _ = state.save_with(None, SaveReason::Quit, |_| Ok(()));
        let saved = state.saved.unwrap();
        assert_eq!(saved.tabs, session.tabs);
        assert!(saved.clean_exit);
    }

    #[test]
    fn failed_quit_reports_error_and_does_not_freeze_later_changes() {
        let mut state = ordinary_window();
        let original = sample_session();
        state.save_with(Some(original.clone()), SaveReason::Checkpoint, |_| Ok(())).unwrap();
        assert!(
            state
                .save_with(Some(original.clone()), SaveReason::Quit, |_| {
                    Err(std::io::Error::new(std::io::ErrorKind::StorageFull, "disk full"))
                })
                .is_err()
        );
        assert!(!state.quitting);
        let mut changed = original;
        changed.tabs[0].cwd = "D:/after-cancel".into();
        state.save_with(Some(changed.clone()), SaveReason::Checkpoint, |_| Ok(())).unwrap();
        assert_eq!(state.saved.as_ref(), Some(&changed));
        state.save_with(Some(changed), SaveReason::Quit, |_| Ok(())).unwrap();
        assert!(state.quitting);
    }

    #[test]
    fn unchanged_checkpoints_do_not_rewrite_storage() {
        let mut state = ordinary_window();
        let _ = state.save_with(Some(sample_session()), SaveReason::Checkpoint, |_| Ok(()));
        let _ = state.save_with(Some(sample_session()), SaveReason::Checkpoint, |_| {
            panic!("unchanged checkpoint must not write")
        });
    }

    #[test]
    fn failed_window_close_is_retried_even_without_remaining_windows() {
        let mut state = ordinary_window();
        let _ = state.save_with(Some(sample_session()), SaveReason::WindowClose, |_| {
            Err(std::io::Error::other("temporary write failure"))
        });
        let _ = state.save_with(None, SaveReason::Checkpoint, |session| {
            assert!(session.clean_exit);
            assert_eq!(session.tabs, sample_session().tabs);
            Ok(())
        });
        assert!(state.saved.unwrap().clean_exit);
    }

    #[test]
    fn failed_explicit_empty_snapshot_is_retried_without_resurrecting_tabs() {
        let mut state = ordinary_window();
        let _ = state.save_with(Some(sample_session()), SaveReason::Checkpoint, |_| Ok(()));
        let _ = state.save_with(Some(Session::new(0, vec![])), SaveReason::WindowClose, |_| {
            Err(std::io::Error::other("temporary write failure"))
        });
        let _ = state.save_with(None, SaveReason::Checkpoint, |session| {
            assert!(session.tabs.is_empty());
            Ok(())
        });
        assert!(state.saved.unwrap().tabs.is_empty());
    }

    #[test]
    fn all_windows_are_combined_with_the_active_tab_offset() {
        let first = sample_session();
        let mut second = sample_session();
        second.tabs.push(TabSession::single("D:/other".into(), None, None));
        second.active_tab = 1;
        second.boot_attempts = 2;
        let combined = combine_sessions([(false, first), (true, second)]).unwrap();
        assert_eq!(combined.tabs.len(), 3);
        assert_eq!(combined.active_tab, 2);
        assert_eq!(
            combined.boot_attempts, 2,
            "pending recovery in any window preserves the crash guard"
        );
        assert!(combine_sessions([]).is_none());
    }
}
