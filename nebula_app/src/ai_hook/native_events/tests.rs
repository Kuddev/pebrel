use std::sync::{Arc, Condvar, Mutex};

use serde_json::json;

use super::*;
use crate::ai_hook::{AiHookEvent, parse_envelope, parse_remote_envelope};

fn with_trusted_brain_root<R>(brain_root: &Path, f: impl FnOnce() -> R) -> R {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            TEST_TRUSTED_BRAIN_ROOT.with(|slot| *slot.borrow_mut() = None);
        }
    }
    TEST_TRUSTED_BRAIN_ROOT.with(|slot| *slot.borrow_mut() = Some(brain_root.to_path_buf()));
    let _reset = Reset;
    f()
}

fn write_antigravity_transcript(brain: &Path, id: &str, source: &str, step_type: &str) -> PathBuf {
    let logs = brain.join(id).join(".system_generated").join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let step = json!({"step_index": 0, "source": source, "type": step_type}).to_string();
    let content = format!("{step}\n");
    std::fs::write(logs.join("transcript.jsonl"), &content).unwrap();
    let full = logs.join("transcript_full.jsonl");
    std::fs::write(&full, &content).unwrap();
    full
}

fn write_subagent_record(brain: &Path, parent_id: &str, child_id: &str) {
    let subagents_dir = brain.join(parent_id).join(".system_generated").join("subagents");
    std::fs::create_dir_all(&subagents_dir).unwrap();
    std::fs::write(
        subagents_dir.join(format!("{child_id}.json")),
        json!({"conversationId": child_id, "state": "SUBAGENT_STATE_ALIVE"}).to_string(),
    )
    .unwrap();
}

#[test]
fn native_events_preserve_provider_and_resume_identity() {
    for source in ["copilot", "grok"] {
        for (event, kind) in [
            ("session-start", AiHookKind::SessionStart),
            ("prompt", AiHookKind::PromptSubmit),
            ("done", AiHookKind::TurnDone),
            ("tool-complete", AiHookKind::ToolComplete),
            ("session-end", AiHookKind::SessionEnd),
        ] {
            let raw = format!(
                "nebula-hook/1 source={source} pane=7 event={event}\n{{\"sessionId\":\"native-session\"}}"
            );
            let parsed = parse_envelope(raw.as_bytes()).unwrap();
            assert_eq!(parsed.source, source);
            assert_eq!(parsed.session_id.as_deref(), Some("native-session"));
            assert_eq!(parsed.kind, kind);
            assert_eq!(parsed.pane, Some(7));
        }
        let idle = format!(
            "nebula-hook/1 source={source} event=notification\n{{\"notification_type\":\"idle_prompt\"}}"
        );
        assert!(parse_envelope(idle.as_bytes()).is_none());
        let attention = format!(
            "nebula-hook/1 source={source} event=notification\n{{\"notificationType\":\"elicitation_dialog\",\"message\":\"Approve?\"}}"
        );
        assert_eq!(parse_envelope(attention.as_bytes()).unwrap().kind, AiHookKind::NeedsAttention);
    }
}

#[test]
fn cursor_resume_ids_are_source_specific() {
    for (source, field) in [("cursor", "conversation_id"), ("cursor", "session_id")] {
        for event in ["session-start", "prompt", "done", "tool-complete", "session-end"] {
            let raw = format!(
                "nebula-hook/1 source={source} pane=4 event={event}\n{{\"{field}\":\"native-id\",\"sessionId\":\"foreign-id\"}}"
            );
            let parsed = parse_envelope(raw.as_bytes()).unwrap();
            assert_eq!(parsed.source, source);
            assert_eq!(parsed.session_id.as_deref(), Some("native-id"));
        }
        let raw =
            format!("nebula-hook/1 source={source} event=done\n{{\"sessionId\":\"foreign-id\"}}");
        assert!(parse_envelope(raw.as_bytes()).unwrap().session_id.is_none());
    }
    for (status, outcome) in [
        ("completed", AiTurnOutcome::Succeeded),
        ("aborted", AiTurnOutcome::Cancelled),
        ("error", AiTurnOutcome::Failed),
    ] {
        let raw = format!(
            "nebula-hook/1 source=cursor event=done\n{{\"status\":\"{status}\",\"conversation_id\":\"id\"}}"
        );
        let parsed = parse_envelope(raw.as_bytes()).unwrap();
        assert_eq!(parsed.kind, AiHookKind::TurnDone);
        assert_eq!(parsed.turn_outcome, outcome);
    }
}

#[test]
fn omp_results_do_not_become_pi_sessions() {
    let parsed = parse_envelope(
        b"nebula-hook/1 source=omp pane=3\n{\"kind\":\"done\",\"session_id\":\"omp-session\",\"stop_reason\":\"error\"}",
    )
    .unwrap();
    assert_eq!(parsed.source, "omp");
    assert_eq!(parsed.session_id.as_deref(), Some("omp-session"));
    assert_eq!(parsed.kind, AiHookKind::TurnDone);
    assert_eq!(parsed.turn_outcome, AiTurnOutcome::Failed);
}

#[test]
fn native_failures_and_foreign_cursor_payloads_never_report_success() {
    let grok = parse_envelope(
        b"nebula-hook/1 source=grok event=failed\n{\"hookEventName\":\"StopFailure\",\"sessionId\":\"grok-session\"}",
    )
    .unwrap();
    assert_eq!(grok.turn_outcome, AiTurnOutcome::Failed);
    let retry =
        b"nebula-hook/1 source=copilot event=error\n{\"recoverable\":true,\"sessionId\":\"retry\"}";
    assert!(parse_envelope(retry).is_none());
    let failed = b"nebula-hook/1 source=copilot event=error\n{\"recoverable\":false,\"sessionId\":\"failed\"}";
    assert_eq!(parse_envelope(failed).unwrap().turn_outcome, AiTurnOutcome::Failed);
    assert!(
        parse_envelope(
            b"nebula-hook/1 source=cursor event=done\n{\"hookEventName\":\"Stop\",\"sessionId\":\"grok\"}"
        )
        .is_none()
    );
}

#[test]
fn antigravity_uses_its_conversation_id_and_only_classifies_known_stops() {
    let parse = |event: &str, body: &str| {
        let raw = format!("nebula-hook/1 source=antigravity pane=5 event={event}\n{body}");
        parse_envelope(raw.as_bytes())
    };
    for (event, kind) in [
        ("session-start", AiHookKind::SessionStart),
        ("tool-complete", AiHookKind::ToolComplete),
        ("done", AiHookKind::TurnDone),
    ] {
        let parsed = parse(event, r#"{"conversationId":"conv-1","workspacePaths":["/w"]}"#);
        let parsed = parsed.unwrap();
        assert_eq!((parsed.kind, parsed.session_id.as_deref()), (kind, Some("conv-1")));
    }
    let foreign = parse("done", r#"{"sessionId":"x","session_id":"y"}"#).unwrap();
    assert_eq!(foreign.session_id, None);
    for (body, outcome) in [
        (r#"{"terminationReason":"NO_TOOL_CALL","fullyIdle":true}"#, AiTurnOutcome::Succeeded),
        (r#"{"terminationReason":"model_stop","fullyIdle":true}"#, AiTurnOutcome::Succeeded),
        (r#"{"terminationReason":"model_stop"}"#, AiTurnOutcome::Unknown),
        (r#"{"terminationReason":"max_steps_exceeded"}"#, AiTurnOutcome::Incomplete),
        (r#"{"terminationReason":"model_stop","error":"quota"}"#, AiTurnOutcome::Failed),
        (r#"{"terminationReason":"user_cancelled"}"#, AiTurnOutcome::Unknown),
    ] {
        assert_eq!(parse("done", body).unwrap().turn_outcome, outcome, "{body}");
    }
}

#[test]
fn antigravity_hook_identity_resumes_after_session_round_trip() {
    let event = parse_envelope(
        b"nebula-hook/1 source=antigravity pane=5 event=session-start\n{\"conversationId\":\"b5f6c1c2-1111-2222-3333-444455556666\"}",
    )
    .unwrap();
    let saved = crate::session::AgentSession {
        source: event.source,
        session_id: event.session_id,
        session_file: event.session_file,
    };
    let json = serde_json::to_string(&saved).unwrap();
    let restored: crate::session::AgentSession = serde_json::from_str(&json).unwrap();
    let expected = "agy --conversation b5f6c1c2-1111-2222-3333-444455556666";
    assert_eq!(restored.resume_command().as_deref(), Some(expected));

    let manual = crate::ai_sessions::AiSession::test_session(
        crate::ai_agents::AgentKind::Antigravity,
        restored.session_id.as_deref().unwrap(),
        "Saved conversation",
    );
    assert_eq!(manual.resume_command().as_deref(), Some(expected));
    assert_eq!(manual.fork_command(), None);
}

#[test]
fn antigravity_resume_requires_a_safe_explicit_conversation_id() {
    for id in [None, Some(""), Some("x; calc"), Some("$(id)"), Some("two words"), Some("x\ny")] {
        let saved = crate::session::AgentSession {
            source: "antigravity".into(),
            session_id: id.map(str::to_owned),
            session_file: None,
        };
        assert_eq!(saved.resume_command(), None, "{id:?}");
    }
    assert_eq!(crate::ai_agents::AgentKind::Antigravity.resume_command(&"a".repeat(65)), None);
}

#[test]
fn antigravity_background_work_defers_completion_until_fully_idle() {
    use crate::ai_agents::AgentStatus;
    use crate::ai_hook::lifecycle::AgentActivity;
    let mut activity = AgentActivity::default();
    for (idle, expected) in [(false, AgentStatus::Working), (true, AgentStatus::Done)] {
        let wire = format!(
            "nebula-hook/1 source=antigravity pane=5 event=done\n{{\"conversationId\":\"conv-1\",\"terminationReason\":\"model_stop\",\"fullyIdle\":{idle}}}"
        );
        let event = parse_envelope(wire.as_bytes()).unwrap();
        assert!(event.capabilities().background_tasks);
        assert_eq!(event.active_background_tasks(), u32::from(!idle));
        assert!(activity.apply_hook(&event));
        assert_eq!(activity.status(), expected);
        assert_eq!(crate::notify::Notification::from_ai_hook(&event, None, false).is_some(), idle);
    }
    let event = parse_envelope(
        b"nebula-hook/1 source=antigravity event=done\n{\"terminationReason\":\"model_stop\"}",
    )
    .unwrap();
    assert!(crate::notify::Notification::from_ai_hook(&event, None, false).is_none());
}

#[test]
fn antigravity_in_process_subagents_never_hijack_primary_conversation_or_turn() {
    use crate::ai_agents::AgentStatus;
    use crate::ai_hook::lifecycle::AgentActivity;

    let temp = tempfile::tempdir().unwrap();
    let brain = temp.path().join(".gemini").join("antigravity-cli").join("brain");

    let root_id = "11111111-1111-4111-8111-111111111111";
    let sub_id = "22222222-2222-4222-8222-222222222222";
    let fork_id = "33333333-3333-4333-8333-333333333333";
    let next_root_id = "44444444-4444-4444-8444-444444444444";

    let root_transcript =
        write_antigravity_transcript(&brain, root_id, "USER_EXPLICIT", "USER_INPUT");
    let sub_transcript = write_antigravity_transcript(&brain, sub_id, "SYSTEM", "SYSTEM_MESSAGE");
    let fork_transcript =
        write_antigravity_transcript(&brain, fork_id, "USER_EXPLICIT", "USER_INPUT");
    let next_root_transcript =
        write_antigravity_transcript(&brain, next_root_id, "USER_EXPLICIT", "USER_INPUT");
    write_subagent_record(&brain, root_id, fork_id);

    with_trusted_brain_root(&brain, || {
        let send = |event: &str, id: &str, transcript: &Path, extra: Value| {
            let mut body = json!({
                "conversationId": id,
                "transcriptPath": transcript.to_str().unwrap(),
            });
            if let Some(extra_obj) = extra.as_object() {
                for (k, v) in extra_obj {
                    body[k] = v.clone();
                }
            }
            let wire = format!("nebula-hook/1 source=antigravity pane=5 event={event}\n{body}");
            parse_envelope(wire.as_bytes()).map(|mut parsed| {
                parsed.agent_pid = Some(4242);
                parsed
            })
        };

        let mut activity = AgentActivity::default();
        let mut active_session = None;
        fn accept(
            activity: &mut AgentActivity,
            active_session: &mut Option<String>,
            event: AiHookEvent,
        ) {
            assert!(activity.apply_hook(&event));
            if let Some(id) = event.session_id {
                *active_session = Some(id);
            }
        }

        accept(
            &mut activity,
            &mut active_session,
            send("session-start", root_id, &root_transcript, json!({})).unwrap(),
        );
        assert_eq!(activity.status(), AgentStatus::Idle);
        accept(
            &mut activity,
            &mut active_session,
            send("tool-complete", root_id, &root_transcript, json!({})).unwrap(),
        );
        assert_eq!(activity.status(), AgentStatus::Working);
        assert_eq!(active_session.as_deref(), Some(root_id));

        for (child_id, child_transcript) in [(sub_id, &sub_transcript), (fork_id, &fork_transcript)]
        {
            for (event, extra) in [
                ("session-start", json!({})),
                ("tool-complete", json!({})),
                ("done", json!({"terminationReason": "model_stop", "fullyIdle": true})),
            ] {
                assert!(
                    send(event, child_id, child_transcript, extra).is_none(),
                    "subagent {child_id} event {event} must be filtered before pane routing"
                );
            }
        }
        assert_eq!(activity.status(), AgentStatus::Working);
        assert_eq!(active_session.as_deref(), Some(root_id));

        accept(
            &mut activity,
            &mut active_session,
            send("tool-complete", root_id, &root_transcript, json!({})).unwrap(),
        );
        accept(
            &mut activity,
            &mut active_session,
            send(
                "done",
                root_id,
                &root_transcript,
                json!({"terminationReason": "model_stop", "fullyIdle": true}),
            )
            .unwrap(),
        );
        assert_eq!(activity.status(), AgentStatus::Done);
        assert_eq!(active_session.as_deref(), Some(root_id));

        let saved = crate::session::AgentSession {
            source: "antigravity".into(),
            session_id: active_session.clone(),
            session_file: None,
        };
        assert_eq!(
            saved.resume_command().as_deref(),
            Some("agy --conversation 11111111-1111-4111-8111-111111111111")
        );

        accept(
            &mut activity,
            &mut active_session,
            send("session-start", next_root_id, &next_root_transcript, json!({})).unwrap(),
        );
        assert_eq!(activity.status(), AgentStatus::Idle);
        assert_eq!(active_session.as_deref(), Some(next_root_id));
    });
}

#[test]
fn antigravity_forked_subagents_are_rejected_on_cold_start_and_after_parent_eviction() {
    let temp = tempfile::tempdir().unwrap();
    let brain = temp.path().join(".gemini").join("antigravity-cli").join("brain");

    let parent_id = "aaaa1111-1111-4111-8111-111111111111";
    let cold_fork_id = "bbbb2222-2222-4222-8222-222222222222";
    let evicted_fork_id = "cccc3333-3333-4333-8333-333333333333";

    let parent_transcript =
        write_antigravity_transcript(&brain, parent_id, "USER_EXPLICIT", "USER_INPUT");
    let cold_fork_transcript =
        write_antigravity_transcript(&brain, cold_fork_id, "USER_EXPLICIT", "USER_INPUT");
    let evicted_fork_transcript =
        write_antigravity_transcript(&brain, evicted_fork_id, "USER_EXPLICIT", "USER_INPUT");
    write_subagent_record(&brain, parent_id, cold_fork_id);
    write_subagent_record(&brain, parent_id, evicted_fork_id);

    with_trusted_brain_root(&brain, || {
        let send = |event: &str, id: &str, transcript: &Path| {
            let body = json!({
                "conversationId": id,
                "transcriptPath": transcript.to_str().unwrap(),
            });
            let wire = format!("nebula-hook/1 source=antigravity pane=5 event={event}\n{body}");
            parse_envelope(wire.as_bytes())
        };

        // Cold start / reconnect: child arrives before parent has ever entered memory cache.
        for event in ["session-start", "tool-complete", "done"] {
            assert!(
                send(event, cold_fork_id, &cold_fork_transcript).is_none(),
                "cold-start forked subagent {event} must be rejected even before parent is cached"
            );
        }

        assert_eq!(
            send("session-start", parent_id, &parent_transcript).unwrap().session_id.as_deref(),
            Some(parent_id)
        );

        // Evict parent from the bounded in-memory conversation cache, then verify another
        // forked child registered on disk under that parent is still rejected.
        for idx in 0..(MAX_TRACKED_ANTIGRAVITY_CONVERSATIONS + 8) {
            let filler_id = format!("filler-{idx:04}");
            let filler_transcript =
                write_antigravity_transcript(&brain, &filler_id, "USER_EXPLICIT", "USER_INPUT");
            assert!(send("session-start", &filler_id, &filler_transcript).is_some());
        }
        {
            let cache =
                ANTIGRAVITY_CONVERSATIONS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            assert!(
                !cache.iter().any(|(dir, _)| dir.ends_with(parent_id)),
                "parent must have been evicted from the bounded cache"
            );
        }

        for event in ["session-start", "tool-complete", "done"] {
            assert!(
                send(event, evicted_fork_id, &evicted_fork_transcript).is_none(),
                "forked subagent {event} must still be rejected after parent cache eviction"
            );
        }
    });
}

#[test]
fn antigravity_explicit_subagent_markers_and_untrusted_paths_are_handled_safely() {
    let parse_both = |body: Value| {
        let wire = format!("nebula-hook/1 source=antigravity pane=5 event=session-start\n{body}");
        (parse_envelope(wire.as_bytes()), parse_remote_envelope(wire.as_bytes(), Some(5)))
    };

    for subagent_body in [
        json!({
            "conversationId": "22222222-2222-4222-8222-222222222222",
            "parentConversationId": "11111111-1111-4111-8111-111111111111",
        }),
        json!({
            "conversationId": "22222222-2222-4222-8222-222222222222",
            "callerContext": "subagent",
        }),
        json!({
            "conversationId": "22222222-2222-4222-8222-222222222222",
            "rootConversationId": "11111111-1111-4111-8111-111111111111",
        }),
    ] {
        let (local, remote) = parse_both(subagent_body.clone());
        assert!(local.is_none() && remote.is_none(), "{subagent_body}");
    }

    for root_body in [
        json!({
            "conversationId": "11111111-1111-4111-8111-111111111111",
            "parentConversationId": "",
            "callerContext": "",
            "rootConversationId": "11111111-1111-4111-8111-111111111111",
            "agentName": "custom-agent",
        }),
        json!({
            "conversationId": "11111111-1111-4111-8111-111111111111",
            "transcriptPath": "/tmp/brain/../outside/transcript.jsonl",
        }),
    ] {
        let (local, remote) = parse_both(root_body.clone());
        assert!(local.is_some() && remote.is_some(), "{root_body}");
    }

    let temp = tempfile::tempdir().unwrap();
    let sub_id = "22222222-2222-4222-8222-222222222222";
    let trusted_brain = temp.path().join(".gemini").join("antigravity-cli").join("brain");
    let trusted_transcript =
        write_antigravity_transcript(&trusted_brain, sub_id, "SYSTEM", "SYSTEM_MESSAGE");
    let wire = format!(
        "nebula-hook/1 source=antigravity pane=5 event=session-start\n{}",
        json!({"conversationId": sub_id, "transcriptPath": trusted_transcript.to_str().unwrap()})
    );

    // Without trusting `trusted_brain`, paths outside the user's `.gemini` log roots are ignored.
    assert!(parse_envelope(wire.as_bytes()).is_some());

    with_trusted_brain_root(&trusted_brain, || {
        assert!(parse_envelope(wire.as_bytes()).is_none());
        assert!(
            parse_remote_envelope(wire.as_bytes(), Some(5)).is_some(),
            "remote envelopes must never inspect local host transcript files"
        );
    });
}

#[test]
fn antigravity_probe_enforces_regular_files_and_listener_wait_bound() {
    let temp = tempfile::tempdir().unwrap();
    let brain = temp.path().join(".gemini").join("antigravity-cli").join("brain");
    std::fs::create_dir_all(&brain).unwrap();

    // 1. Directory masquerading as transcript file is rejected before reading.
    let dir_id = "dir-transcript-conv";
    let dir_logs = brain.join(dir_id).join(".system_generated").join("logs");
    std::fs::create_dir_all(dir_logs.join("transcript.jsonl")).unwrap();
    std::fs::create_dir_all(dir_logs.join("transcript_full.jsonl")).unwrap();
    with_trusted_brain_root(&brain, || {
        let body = json!({
            "conversationId": dir_id,
            "artifactDirectoryPath": brain.join(dir_id).to_str().unwrap(),
        });
        let wire = format!("nebula-hook/1 source=antigravity pane=5 event=session-start\n{body}");
        assert!(parse_envelope(wire.as_bytes()).is_some());
    });

    // 2. Symlinked transcript file pointing outside `brain` is never followed.
    let outside = temp.path().join("outside-system.jsonl");
    std::fs::write(&outside, "{\"step_index\":0,\"source\":\"SYSTEM\"}\n").unwrap();
    let sym_id = "symlink-transcript-conv";
    let sym_logs = brain.join(sym_id).join(".system_generated").join("logs");
    std::fs::create_dir_all(&sym_logs).unwrap();
    let sym_transcript = sym_logs.join("transcript_full.jsonl");
    match crate::platform::local_paths::symlink_file_for_test(&outside, &sym_transcript) {
        Ok(()) => with_trusted_brain_root(&brain, || {
            let body = json!({
                "conversationId": sym_id,
                "transcriptPath": sym_transcript.to_str().unwrap(),
            });
            let wire =
                format!("nebula-hook/1 source=antigravity pane=5 event=session-start\n{body}");
            assert!(
                parse_envelope(wire.as_bytes()).is_some(),
                "symlinked transcript must be rejected instead of reading target"
            );
        }),
        Err(error)
            if crate::platform::Platform::current() == crate::platform::Platform::Windows
                && error.raw_os_error() == Some(1314) => {},
        Err(error) => panic!("failed to create symlink fixture: {error}"),
    }

    // 3. Unix FIFO at transcript path never blocks open() or read_line().
    let fifo_id = "fifo-transcript-conv";
    let fifo_logs = brain.join(fifo_id).join(".system_generated").join("logs");
    std::fs::create_dir_all(&fifo_logs).unwrap();
    let fifo_path = fifo_logs.join("transcript_full.jsonl");
    if let Some(created) = crate::platform::regular_file::create_fifo_for_test(&fifo_path) {
        created.unwrap();
        let started = Instant::now();
        with_trusted_brain_root(&brain, || {
            let body = json!({
                "conversationId": fifo_id,
                "transcriptPath": fifo_path.to_str().unwrap(),
            });
            let wire =
                format!("nebula-hook/1 source=antigravity pane=5 event=session-start\n{body}");
            assert!(parse_envelope(wire.as_bytes()).is_some());
        });
        assert!(started.elapsed() < Duration::from_millis(500), "FIFO open must never block");
    }

    // 4. Stalled background probe respects the explicit listener wait bound.
    let stalled_id = "stalled-probe-conv";
    let stalled_transcript =
        write_antigravity_transcript(&brain, stalled_id, "SYSTEM", "SYSTEM_MESSAGE");
    let worker = ProbeWorker::new();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    struct ReleaseGate(Arc<(Mutex<bool>, Condvar)>);
    impl Drop for ReleaseGate {
        fn drop(&mut self) {
            let (lock, cvar) = &*self.0;
            *lock.lock().unwrap_or_else(|e| e.into_inner()) = true;
            cvar.notify_all();
        }
    }
    let _release = ReleaseGate(Arc::clone(&gate));
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let gate_in_worker = Arc::clone(&gate);
    worker
        .tx
        .as_ref()
        .unwrap()
        .send(Box::new(move || {
            let _ = entered_tx.send(());
            let (lock, cvar) = &*gate_in_worker;
            let mut done = lock.lock().unwrap_or_else(|e| e.into_inner());
            while !*done {
                done = cvar.wait(done).unwrap_or_else(|e| e.into_inner());
            }
        }))
        .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();

    let payload = json!({
        "conversationId": stalled_id,
        "transcriptPath": stalled_transcript.to_str().unwrap(),
    });
    let started = Instant::now();
    let verdict = is_antigravity_subagent_with_worker(
        &payload,
        Some(stalled_id),
        true,
        std::slice::from_ref(&brain),
        &worker,
        Duration::from_millis(25),
    );
    assert!(!verdict, "stalled probe must time out safely rather than blocking listener");
    assert!(started.elapsed() < Duration::from_millis(500));
}
