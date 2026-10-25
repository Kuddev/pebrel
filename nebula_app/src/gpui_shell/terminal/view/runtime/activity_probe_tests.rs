use super::*;
use crate::ai_agents::AgentStatus;
use crate::gpui_shell::terminal::view::startup_tests::open;
use gpui::TestAppContext;

#[gpui::test]
fn a_hook_owner_arriving_during_a_probe_invalidates_old_process_evidence(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    view.update(window, |view, cx| {
        let started = Some(std::time::Instant::now());
        view.command_started = started;
        view.session.as_mut().unwrap().shell_pid = 4242;
        // The probe started without a known agent; a real hook then identifies it.
        let mut event = crate::ai_hook::parse_remote_envelope(
            b"nebula-hook/1 source=claude pane=42\n{\"hook_event_name\":\"UserPromptSubmit\",\"session_id\":\"probe-owner\"}", Some(42)
        ).unwrap();
        event.agent_pid = Some(100);
        view.agent_activity.apply_hook(&event);
        view.running_program = Some("claude".into());
        assert_eq!(view.agent_activity.primary_pid(), Some(100));
        view.apply_activity_evidence(started, 4242, None, Ok(crate::process_tree::ActivityEvidence {
            agent: None, primary_present: None, child_present: false, busy: false,
        }), cx);
        assert_eq!(view.running_program.as_deref(), Some("claude"));
        assert_eq!(view.agent_activity.status(), AgentStatus::Working);
        // Evidence for the current owner still establishes a real process exit.
        view.apply_activity_evidence(started, 4242, Some(100), Ok(crate::process_tree::ActivityEvidence {
            agent: None, primary_present: Some(false), child_present: false, busy: false,
        }), cx);
        assert!(view.running_program.is_none());
    });
}
