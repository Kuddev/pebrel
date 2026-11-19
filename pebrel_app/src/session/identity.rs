//! Provider identities used to resume a saved conversation.
//!
//! Hook lifecycle IDs are not necessarily conversation IDs. In particular,
//! Codex resumes a rollout's thread UUID, not its hook session/group UUID.

/// Read a Codex thread UUID from its native rollout path without consulting
/// the host filesystem: Windows, WSL and SSH paths belong to their own shell.
pub(crate) fn codex_rollout_id(path: &str) -> Option<&str> {
    if !super::valid_native_session_file(path) {
        return None;
    }
    let filename = path.rsplit(['/', '\\']).next()?;
    let stem = filename.strip_prefix("rollout-")?.strip_suffix(".jsonl")?;
    let start = stem.len().checked_sub(36)?;
    let id = stem.get(start..)?;
    if start == 0 || stem.as_bytes().get(start - 1) != Some(&b'-') {
        return None;
    }
    id.bytes()
        .enumerate()
        .all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) { byte == b'-' } else { byte.is_ascii_hexdigit() }
        })
        .then_some(id)
}

impl super::AgentSession {
    /// Upgrade snapshots carrying a native file before comparing hook
    /// acknowledgements or generating a command. Unrecognized file names keep
    /// the saved ID; missing filename evidence cannot erase an existing target.
    pub(crate) fn normalize_identity(&mut self) {
        if self.source == "codex"
            && let Some(id) = self.session_file.as_deref().and_then(codex_rollout_id)
        {
            self.session_id = Some(id.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THREAD: &str = "0199a213-c2a4-7cf5-8f6b-d746fbb6e86c";

    #[test]
    fn native_paths_keep_thread_identity_in_every_execution_environment() {
        for root in [
            "/home/user/.codex/sessions",
            r"C:\Users\user\.codex\sessions",
            r"\\server\data",
            "/home/用户/.codex/sessions",
        ] {
            let file = format!("{root}/rollout-2026-09-21T10-00-00-{THREAD}.jsonl");
            assert_eq!(codex_rollout_id(&file), Some(THREAD));
        }
        for path in [
            format!("rollout-date-{THREAD}.jsonl"),
            format!("/tmp/{THREAD}.jsonl"),
            format!("/tmp/rollout-{THREAD}.jsonl"),
            format!("/tmp/rollout-date-{THREAD}.jsonl\n"),
            "/tmp/rollout-date-invalid-id.jsonl".into(),
        ] {
            assert_eq!(codex_rollout_id(&path), None, "{path}");
        }
    }

    #[test]
    fn unrecognized_native_file_keeps_the_saved_resume_command() {
        for path in [
            "/sessions/conversation.jsonl",
            r"C:\sessions\conversation.jsonl",
            "/sessions/rollout-date-not-a-uuid.jsonl",
        ] {
            let saved = super::super::AgentSession {
                source: "codex".into(),
                session_id: Some("saved-thread".into()),
                session_file: Some(path.into()),
            };
            assert_eq!(saved.resume_command().as_deref(), Some("codex resume saved-thread"));
        }
    }

    #[test]
    fn unrecognized_native_file_cannot_erase_an_existing_snapshot_identity() {
        let mut saved: super::super::AgentSession = serde_json::from_value(serde_json::json!({
            "source": "codex",
            "session_id": "saved-thread",
            "session_file": "/sessions/conversation.jsonl",
        }))
        .unwrap();
        let original = saved.clone();
        saved.normalize_identity();
        saved.normalize_identity();
        assert_eq!(saved, original, "normalization keeps the ID and native file intact");
        assert_eq!(saved.resume_command().as_deref(), Some("codex resume saved-thread"));
    }

    #[test]
    fn an_unrecognized_file_still_needs_a_safe_existing_id_to_resume() {
        for id in [None, Some("".into()), Some("bad; echo injected".into()), Some("x".repeat(65))] {
            let mut saved = super::super::AgentSession {
                source: "codex".into(),
                session_id: id.clone(),
                session_file: Some("/sessions/conversation.jsonl".into()),
            };
            assert!(saved.resume_command().is_none());
            saved.normalize_identity();
            assert_eq!(saved.session_id, id);
            assert!(saved.resume_command().is_none());
        }
    }

    #[test]
    fn snapshot_resume_uses_the_rollout_thread_instead_of_the_hook_group() {
        for id in [Some("different-hook-group".into()), None] {
            let mut saved = super::super::AgentSession {
                source: "codex".into(),
                session_id: id,
                session_file: Some(format!(
                    "/home/user/.codex/sessions/rollout-date-{THREAD}.jsonl"
                )),
            };
            assert_eq!(saved.resume_command(), Some(format!("codex resume {THREAD}")));
            saved.normalize_identity();
            assert_eq!(saved.session_id.as_deref(), Some(THREAD));
            assert_eq!(saved.resume_command(), Some(format!("codex resume {THREAD}")));
        }
    }
}
