use std::sync::{Arc, Condvar, Mutex};

use serde_json::json;

use super::*;
use crate::ai_agents::AgentStatus;
use crate::ai_hook::lifecycle::AgentActivity;
use crate::ai_hook::{AiHookEvent, parse_envelope, parse_remote_envelope};

static TEST_PROBE_MUTEX: Mutex<()> = Mutex::new(());

fn temp_brain_root() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let base = if is_dir_and_ancestors_no_symlink(temp.path()) {
        temp.path().to_path_buf()
    } else {
        // Resolve system symlinks such as `/var -> /private/var` on macOS temp roots.
        std::fs::canonicalize(temp.path()).unwrap()
    };
    let brain = base.join(".gemini").join("antigravity-cli").join("brain");
    std::fs::create_dir_all(&brain).unwrap();
    (temp, brain)
}

fn with_trusted_brain_root<R>(brain_root: &Path, f: impl FnOnce() -> R) -> R {
    let _guard = TEST_PROBE_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
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

fn parse_antigravity_hook(
    event: &str,
    id: &str,
    transcript: &Path,
    pid: Option<u32>,
    extra: Value,
) -> Option<AiHookEvent> {
    let mut body = json!({
        "conversationId": id,
        "transcriptPath": transcript.to_str().unwrap(),
    });
    if let Some(obj) = extra.as_object() {
        for (k, v) in obj {
            body[k] = v.clone();
        }
    }
    let wire = format!("nebula-hook/1 source=antigravity pane=5 event={event}\n{body}");
    parse_envelope(wire.as_bytes()).map(|mut parsed| {
        parsed.agent_pid = pid;
        parsed
    })
}

fn route_antigravity_hook(
    activity: &mut AgentActivity,
    active_session: &mut Option<String>,
    maybe_event: Option<AiHookEvent>,
) -> bool {
    let Some(event) = maybe_event else { return false };
    if activity.apply_hook(&event) {
        if let Some(id) = event.session_id {
            *active_session = Some(id);
        }
        true
    } else {
        false
    }
}

#[test]
fn antigravity_in_process_subagents_never_hijack_primary_conversation_or_turn() {
    let (_temp, brain) = temp_brain_root();

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
            parse_antigravity_hook(event, id, transcript, Some(4242), extra)
        };
        let mut activity = AgentActivity::default();
        let mut active_session = None;

        assert!(route_antigravity_hook(
            &mut activity,
            &mut active_session,
            send("session-start", root_id, &root_transcript, json!({})),
        ));
        assert_eq!(activity.status(), AgentStatus::Idle);
        assert!(route_antigravity_hook(
            &mut activity,
            &mut active_session,
            send("tool-complete", root_id, &root_transcript, json!({})),
        ));
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

        assert!(route_antigravity_hook(
            &mut activity,
            &mut active_session,
            send("tool-complete", root_id, &root_transcript, json!({})),
        ));
        assert!(route_antigravity_hook(
            &mut activity,
            &mut active_session,
            send(
                "done",
                root_id,
                &root_transcript,
                json!({"terminationReason": "model_stop", "fullyIdle": true}),
            ),
        ));
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

        assert!(route_antigravity_hook(
            &mut activity,
            &mut active_session,
            send("session-start", next_root_id, &next_root_transcript, json!({})),
        ));
        assert_eq!(activity.status(), AgentStatus::Idle);
        assert_eq!(active_session.as_deref(), Some(next_root_id));
    });
}

#[test]
fn antigravity_forked_subagents_are_rejected_on_cold_start_and_after_parent_eviction() {
    let (_temp, brain) = temp_brain_root();

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
            parse_antigravity_hook(event, id, transcript, None, json!({}))
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

    let minimal_root = json!({
        "conversationId": "11111111-1111-4111-8111-111111111111",
        "parentConversationId": "",
        "callerContext": "",
        "rootConversationId": "11111111-1111-4111-8111-111111111111",
        "agentName": "custom-agent",
    });
    let (local, remote) = parse_both(minimal_root);
    assert!(local.is_some() && remote.is_some());

    // Local envelopes claiming malformed or untrusted paths remain unverified (`None`),
    // whereas remote envelopes ignore local filesystem claims entirely.
    let traversal_body = json!({
        "conversationId": "11111111-1111-4111-8111-111111111111",
        "transcriptPath": "/tmp/brain/../outside/transcript.jsonl",
    });
    let (local, remote) = parse_both(traversal_body);
    assert!(local.is_none() && remote.is_some());

    let (_temp, trusted_brain) = temp_brain_root();
    let sub_id = "22222222-2222-4222-8222-222222222222";
    let trusted_transcript =
        write_antigravity_transcript(&trusted_brain, sub_id, "SYSTEM", "SYSTEM_MESSAGE");
    let wire = format!(
        "nebula-hook/1 source=antigravity pane=5 event=session-start\n{}",
        json!({"conversationId": sub_id, "transcriptPath": trusted_transcript.to_str().unwrap()})
    );

    // Without trusting `trusted_brain`, local path claims outside `~/.gemini/antigravity-cli/brain`
    // are unverified and rejected before pane routing.
    assert!(parse_envelope(wire.as_bytes()).is_none());

    with_trusted_brain_root(&trusted_brain, || {
        assert!(parse_envelope(wire.as_bytes()).is_none());
        assert!(
            parse_remote_envelope(wire.as_bytes(), Some(5)).is_some(),
            "remote envelopes must never inspect local host transcript files"
        );
    });
}

#[test]
fn antigravity_probe_enforces_regular_files_and_ancestor_link_policy() {
    let (temp, brain) = temp_brain_root();
    let base = brain.parent().unwrap().parent().unwrap().parent().unwrap();

    // 1. Directory masquerading as transcript file is rejected as unverified (`None`).
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
        assert!(parse_envelope(wire.as_bytes()).is_none());
    });

    // 2. Symlinked transcript file pointing outside `brain` is never followed and stays unverified.
    let outside = base.join("outside-user.jsonl");
    std::fs::write(&outside, "{\"step_index\":0,\"source\":\"USER_EXPLICIT\"}\n").unwrap();
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
                parse_envelope(wire.as_bytes()).is_none(),
                "symlinked transcript must remain unverified instead of reading target"
            );
        }),
        Err(error)
            if crate::platform::Platform::current() == crate::platform::Platform::Windows
                && error.raw_os_error() == Some(1314) => {},
        Err(error) => panic!("failed to create symlink fixture: {error}"),
    }

    // 3. Symlink on the trusted `brain` directory itself or its `.gemini` ancestor is rejected.
    let real_store = base.join("real-store").join("brain");
    let link_conv = "ancestor-link-conv";
    write_antigravity_transcript(&real_store, link_conv, "USER_EXPLICIT", "USER_INPUT");

    let direct_link_brain = base.join("direct-link-home").join(".gemini").join("antigravity-cli");
    std::fs::create_dir_all(&direct_link_brain).unwrap();
    let brain_link = direct_link_brain.join("brain");
    match crate::platform::local_paths::symlink_directory_for_test(&real_store, &brain_link) {
        Ok(()) => {
            with_trusted_brain_root(&brain_link, || {
                let body = json!({
                    "conversationId": link_conv,
                    "artifactDirectoryPath": brain_link.join(link_conv).to_str().unwrap(),
                });
                let wire =
                    format!("nebula-hook/1 source=antigravity pane=5 event=session-start\n{body}");
                assert!(
                    parse_envelope(wire.as_bytes()).is_none(),
                    "symlinked trusted brain root itself must be rejected"
                );
            });

            let real_gemini = base.join("real-gemini");
            let real_gemini_brain = real_gemini.join("antigravity-cli").join("brain");
            write_antigravity_transcript(
                &real_gemini_brain,
                link_conv,
                "USER_EXPLICIT",
                "USER_INPUT",
            );
            let anc_home = base.join("ancestor-link-home");
            std::fs::create_dir_all(&anc_home).unwrap();
            let gemini_link = anc_home.join(".gemini");
            crate::platform::local_paths::symlink_directory_for_test(&real_gemini, &gemini_link)
                .unwrap();
            let ancestor_brain = gemini_link.join("antigravity-cli").join("brain");
            with_trusted_brain_root(&ancestor_brain, || {
                let body = json!({
                    "conversationId": link_conv,
                    "artifactDirectoryPath": ancestor_brain.join(link_conv).to_str().unwrap(),
                });
                let wire =
                    format!("nebula-hook/1 source=antigravity pane=5 event=session-start\n{body}");
                assert!(
                    parse_envelope(wire.as_bytes()).is_none(),
                    "symlinked ancestor above trusted brain root must be rejected"
                );
            });
        },
        Err(error)
            if crate::platform::Platform::current() == crate::platform::Platform::Windows
                && error.raw_os_error() == Some(1314) => {},
        Err(error) => panic!("failed to create directory symlink fixture: {error}"),
    }

    // 4. Unix FIFO at transcript path never blocks open() or read_line() and stays unverified.
    let _ = &temp;
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
            assert!(parse_envelope(wire.as_bytes()).is_none());
        });
        assert!(started.elapsed() < Duration::from_millis(500), "FIFO open must never block");
    }
}

#[test]
fn antigravity_timeout_queue_full_and_incomplete_scan_never_replace_foreground_identity() {
    let (_temp, brain) = temp_brain_root();
    let root_id = "root-primary-0001";
    let stalled_fork_id = "child-stalled-0002";
    let queued_fork_id = "child-queuefull-0003";
    let next_root_id = "root-primary-0004";

    let root_transcript =
        write_antigravity_transcript(&brain, root_id, "USER_EXPLICIT", "USER_INPUT");
    let stalled_fork_transcript =
        write_antigravity_transcript(&brain, stalled_fork_id, "USER_EXPLICIT", "USER_INPUT");
    let queued_fork_transcript =
        write_antigravity_transcript(&brain, queued_fork_id, "USER_EXPLICIT", "USER_INPUT");
    let next_root_transcript =
        write_antigravity_transcript(&brain, next_root_id, "USER_EXPLICIT", "USER_INPUT");
    write_subagent_record(&brain, root_id, stalled_fork_id);
    write_subagent_record(&brain, root_id, queued_fork_id);

    with_trusted_brain_root(&brain, || {
        let send = |event: &str, id: &str, transcript: &Path| {
            parse_antigravity_hook(event, id, transcript, Some(9001), json!({}))
        };

        let mut activity = AgentActivity::default();
        let mut active_session = None;

        route_antigravity_hook(
            &mut activity,
            &mut active_session,
            send("session-start", root_id, &root_transcript),
        );
        route_antigravity_hook(
            &mut activity,
            &mut active_session,
            send("tool-complete", root_id, &root_transcript),
        );
        assert_eq!(activity.status(), AgentStatus::Working);
        assert_eq!(active_session.as_deref(), Some(root_id));

        // 1. Occupy `PROBE_WORKER` so `parse_envelope` hits the 200 ms listener timeout on
        // a forked child whose parent record is already on disk.
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        struct ReleaseGate(Arc<(Mutex<bool>, Condvar)>);
        impl Drop for ReleaseGate {
            fn drop(&mut self) {
                let (lock, cvar) = &*self.0;
                *lock.lock().unwrap_or_else(|e| e.into_inner()) = true;
                cvar.notify_all();
            }
        }
        let release = ReleaseGate(Arc::clone(&gate));
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let gate_worker = Arc::clone(&gate);
        PROBE_WORKER
            .tx
            .as_ref()
            .unwrap()
            .send(Box::new(move || {
                let _ = entered_tx.send(());
                let (lock, cvar) = &*gate_worker;
                let mut done = lock.lock().unwrap_or_else(|e| e.into_inner());
                while !*done {
                    done = cvar.wait(done).unwrap_or_else(|e| e.into_inner());
                }
            }))
            .unwrap();
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let started = Instant::now();
        let timed_out = send("session-start", stalled_fork_id, &stalled_fork_transcript);
        let elapsed = started.elapsed();
        assert!(
            timed_out.is_none(),
            "probe timeout must stay Unverified instead of routing child SessionStart"
        );
        assert!(elapsed >= Duration::from_millis(150) && elapsed < Duration::from_secs(2));
        route_antigravity_hook(&mut activity, &mut active_session, timed_out);
        assert_eq!(activity.status(), AgentStatus::Working);
        assert_eq!(active_session.as_deref(), Some(root_id));

        // 2. Fill the remaining queue capacity so `try_send` fails immediately (`Full`);
        // full queue must also keep the unverified child dropped.
        for _ in 0..8 {
            let gate_queued = Arc::clone(&gate);
            let _ = PROBE_WORKER.tx.as_ref().unwrap().try_send(Box::new(move || {
                let (lock, cvar) = &*gate_queued;
                let mut done = lock.lock().unwrap_or_else(|e| e.into_inner());
                while !*done {
                    done = cvar.wait(done).unwrap_or_else(|e| e.into_inner());
                }
            }));
        }
        let full_started = Instant::now();
        let queue_full = send("session-start", queued_fork_id, &queued_fork_transcript);
        assert!(queue_full.is_none(), "full probe queue must not route unverified SessionStart");
        assert!(full_started.elapsed() < Duration::from_millis(100));
        route_antigravity_hook(&mut activity, &mut active_session, queue_full);
        assert_eq!(active_session.as_deref(), Some(root_id));

        drop(release);
        let (drained_tx, drained_rx) = mpsc::sync_channel(1);
        PROBE_WORKER
            .tx
            .as_ref()
            .unwrap()
            .send(Box::new(move || {
                let _ = drained_tx.send(());
            }))
            .unwrap();
        drained_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        // Once the worker drains, the active root turn can finish and legitimate `/new` root
        // conversations verify and switch cleanly, while the forked children remain rejected.
        assert!(send("session-start", stalled_fork_id, &stalled_fork_transcript).is_none());
        assert!(send("session-start", queued_fork_id, &queued_fork_transcript).is_none());
        route_antigravity_hook(
            &mut activity,
            &mut active_session,
            send("done", root_id, &root_transcript),
        );
        assert_eq!(active_session.as_deref(), Some(root_id));
        route_antigravity_hook(
            &mut activity,
            &mut active_session,
            send("session-start", next_root_id, &next_root_transcript),
        );
        assert_eq!(activity.status(), AgentStatus::Idle);
        assert_eq!(active_session.as_deref(), Some(next_root_id));

        // 3. Incomplete/anomalous parent subagent metadata (directory masquerading as `<id>.json`)
        // cannot prove primary status and stays Unverified (`None`).
        let ambig_id = "child-ambiguous-0005";
        let ambig_transcript =
            write_antigravity_transcript(&brain, ambig_id, "USER_EXPLICIT", "USER_INPUT");
        std::fs::create_dir_all(
            brain
                .join(next_root_id)
                .join(".system_generated")
                .join("subagents")
                .join(format!("{ambig_id}.json")),
        )
        .unwrap();
        assert!(send("session-start", ambig_id, &ambig_transcript).is_none());
        assert_eq!(active_session.as_deref(), Some(next_root_id));
    });
}

#[test]
fn antigravity_delayed_parent_subagent_record_rechecks_and_blocks_later_events() {
    let (_temp, brain) = temp_brain_root();
    let parent_id = "parent-visible-later-0001";
    let child_id = "child-visible-later-0002";
    let next_root_id = "next-root-after-done-0003";

    let parent_transcript =
        write_antigravity_transcript(&brain, parent_id, "USER_EXPLICIT", "USER_INPUT");
    let child_transcript =
        write_antigravity_transcript(&brain, child_id, "USER_EXPLICIT", "USER_INPUT");
    let next_root_transcript =
        write_antigravity_transcript(&brain, next_root_id, "USER_EXPLICIT", "USER_INPUT");

    with_trusted_brain_root(&brain, || {
        let send = |event: &str, id: &str, transcript: &Path, extra: Value| {
            parse_antigravity_hook(event, id, transcript, Some(7788), extra)
        };
        let mut activity = AgentActivity::default();
        let mut active_session = None;

        assert!(route_antigravity_hook(
            &mut activity,
            &mut active_session,
            send("session-start", parent_id, &parent_transcript, json!({})),
        ));
        assert!(route_antigravity_hook(
            &mut activity,
            &mut active_session,
            send("tool-complete", parent_id, &parent_transcript, json!({})),
        ));
        assert_eq!(activity.status(), AgentStatus::Working);
        assert_eq!(activity.primary_session(), Some(parent_id));
        assert_eq!(active_session.as_deref(), Some(parent_id));

        // Child arrives with inherited step 0 before the parent subagent record is readable on
        // disk: `parse_envelope` cannot prove subagent status yet, so `AgentActivity` must keep
        // the active `Working` foreground session instead of letting child `SessionStart` take over.
        let early_child_start =
            send("session-start", child_id, &child_transcript, json!({})).unwrap();
        assert!(!route_antigravity_hook(
            &mut activity,
            &mut active_session,
            Some(early_child_start)
        ));
        assert_eq!(activity.status(), AgentStatus::Working);
        assert_eq!(activity.primary_session(), Some(parent_id));
        assert_eq!(active_session.as_deref(), Some(parent_id));

        // Parent registration record now lands on disk; negative caching must not mask it.
        write_subagent_record(&brain, parent_id, child_id);
        assert!(
            send(
                "done",
                child_id,
                &child_transcript,
                json!({"terminationReason": "model_stop", "fullyIdle": true}),
            )
            .is_none(),
            "later `done` must re-check parent subagent records and reject the subagent"
        );
        assert!(send("tool-complete", child_id, &child_transcript, json!({})).is_none());
        assert!(send("session-start", child_id, &child_transcript, json!({})).is_none());

        // Primary session's own completion event is accepted and finishes the turn cleanly.
        assert!(route_antigravity_hook(
            &mut activity,
            &mut active_session,
            send(
                "done",
                parent_id,
                &parent_transcript,
                json!({"terminationReason": "model_stop", "fullyIdle": true}),
            ),
        ));
        assert_eq!(activity.status(), AgentStatus::Done);
        assert_eq!(activity.primary_session(), Some(parent_id));
        assert_eq!(active_session.as_deref(), Some(parent_id));

        // Control: once the turn has ended, a real `/new` in the same CLI process switches cleanly.
        activity.submitted_text("antigravity", "/new");
        assert!(route_antigravity_hook(
            &mut activity,
            &mut active_session,
            send("session-start", next_root_id, &next_root_transcript, json!({})),
        ));
        assert_eq!(activity.status(), AgentStatus::Idle);
        assert_eq!(activity.primary_session(), Some(next_root_id));
        assert_eq!(active_session.as_deref(), Some(next_root_id));
    });
}

#[test]
fn antigravity_confirmed_foreground_finishes_after_brain_exceeds_scan_limit() {
    let (_temp, brain) = temp_brain_root();
    let root_id = "confirmed-foreground-0001";
    let unknown_id = "unconfirmed-session-0002";

    let root_transcript =
        write_antigravity_transcript(&brain, root_id, "USER_EXPLICIT", "USER_INPUT");

    with_trusted_brain_root(&brain, || {
        let send = |event: &str, id: &str, transcript: &Path, extra: Value| {
            parse_antigravity_hook(event, id, transcript, Some(8899), extra)
        };

        let mut activity = AgentActivity::default();
        assert!(
            activity
                .apply_hook(&send("session-start", root_id, &root_transcript, json!({})).unwrap())
        );
        assert!(
            activity
                .apply_hook(&send("tool-complete", root_id, &root_transcript, json!({})).unwrap())
        );
        assert_eq!(activity.status(), AgentStatus::Working);
        assert_eq!(activity.primary_session(), Some(root_id));

        for idx in 0..=MAX_BRAIN_SCAN_DIRS {
            std::fs::create_dir(brain.join(format!("history-{idx:04}"))).unwrap();
        }
        let unknown_transcript =
            write_antigravity_transcript(&brain, unknown_id, "USER_EXPLICIT", "USER_INPUT");

        // Scan budget and fail-closed protection remain enforced for unknown conversations.
        assert!(send("session-start", unknown_id, &unknown_transcript, json!({})).is_none());

        // Already-confirmed foreground conversation still completes its own turn promptly.
        let done = send(
            "done",
            root_id,
            &root_transcript,
            json!({"terminationReason": "model_stop", "fullyIdle": true}),
        )
        .expect("confirmed foreground session must not drop its own completion at scan limit");
        assert!(activity.apply_hook(&done));
        assert_eq!(activity.status(), AgentStatus::Done);
        assert_eq!(activity.primary_session(), Some(root_id));
    });
}
