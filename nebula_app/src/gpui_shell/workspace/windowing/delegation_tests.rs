use super::*;
use crate::ai_agents::AgentKind;
use crate::gpui_shell::terminal::view::TerminalView;
use crate::runtime_api::{RuntimeDelegationReceipt, RuntimeHub, RuntimeManagedAgent};
use gpui::TestAppContext;
use nebula_terminal::event::Event as TermEvent;
use nebula_terminal::event_loop::Msg;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy)]
enum Ingress {
    Osc,
    Native,
}

struct TestAgent {
    view: Entity<TerminalView>,
    pane_id: u64,
    kind: AgentKind,
    session: String,
    sequence: u64,
    ingress: Ingress,
}

impl TestAgent {
    fn envelope(&mut self, name: &str, message: &str) -> Vec<u8> {
        self.sequence += 1;
        let payload = json!({
            "hook_event_name": name,
            "session_id": self.session,
            "bridge_sequence": self.sequence,
            "last_assistant_message": message,
        });
        let source = self.kind.slug();
        let contract = if self.kind == AgentKind::Codex { " codex_hooks=full" } else { "" };
        let process = match self.ingress {
            Ingress::Osc => format!(" process={}:1", self.pane_id),
            Ingress::Native => String::new(),
        };
        // The authenticated receiving PTY must override this claimed pane.
        format!("nebula-hook/1 source={source} pane=999999{process}{contract}\n{payload}")
            .into_bytes()
    }

    fn inject(&self, envelope: Vec<u8>, cx: &mut TestAppContext) {
        cx.update(|cx| match self.ingress {
            Ingress::Osc => self.view.update(cx, |view, cx| {
                // Exercise the actual terminal-event ingress, not handle_ai_hook directly.
                view.remote_files_test_event(TermEvent::AiHookEnvelope(envelope), cx);
            }),
            Ingress::Native => {
                let hook =
                    crate::ai_hook::parse_remote_envelope(&envelope, Some(self.pane_id)).unwrap();
                dispatch_ai_events(vec![hook], cx);
            },
        });
    }

    fn send(&mut self, name: &str, message: &str, cx: &mut TestAppContext) {
        let envelope = self.envelope(name, message);
        self.inject(envelope, cx);
    }
}

struct DelegationFixture {
    origin: TestAgent,
    worker: TestAgent,
    origin_input: Receiver<Msg>,
    hub: RuntimeHub,
    target: RuntimeManagedAgent,
    receipt: RuntimeDelegationReceipt,
}

fn fixture(
    cx: &mut TestAppContext,
    origin_kind: AgentKind,
    worker_kind: AgentKind,
    origin_busy: bool,
    ingress: Ingress,
) -> DelegationFixture {
    transfer_tests::initialize_test(cx);
    let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    let (window_id, origin, worker, origin_input, hub) = cx.update(|cx| {
        let (window_id, workspace) = transfer_tests::open_test_window(cx, 2);
        let panes: Vec<_> = workspace
            .read(cx)
            .tabs
            .iter()
            .map(|tab| match tab {
                WorkspaceTab::Terminal { panes, .. } => (panes[0].id, panes[0].view.clone()),
                _ => unreachable!(),
            })
            .collect();
        let origin_input = panes[0].1.update(cx, |view, _| view.install_completion_test_session());
        panes[1].1.update(cx, |view, _| view.install_completion_test_session());
        let agent = |index: usize, kind| TestAgent {
            view: panes[index].1.clone(),
            pane_id: panes[index].0,
            kind,
            session: format!("delegation-fixture-{serial}-{index}"),
            sequence: 0,
            ingress,
        };
        (
            window_id,
            agent(0, origin_kind),
            agent(1, worker_kind),
            origin_input,
            cx.global::<WindowRegistry>().runtime_hub.clone(),
        )
    });
    let mut origin = origin;
    let mut worker = worker;
    origin.send("SessionStart", "", cx);
    origin.send(if origin_busy { "UserPromptSubmit" } else { "Stop" }, "", cx);
    worker.send("SessionStart", "", cx);
    worker.send("Stop", "", cx);
    cx.run_until_parked();
    let (target, receipt) = cx.update(|cx| {
        let target = hub
            .register_agent(
                format!("delegation-worker-{serial}"),
                worker_kind,
                window_id,
                worker.pane_id,
                Some(worker.session.clone()),
                None,
            )
            .unwrap();
        publish_runtime_snapshot(cx);
        let target = hub.active_agent(&target.agent_id, Some(target.generation)).unwrap();
        let receipt = hub.begin_delegation(origin.pane_id, &target).unwrap();
        (target, receipt)
    });
    worker.send("UserPromptSubmit", "", cx);
    DelegationFixture { origin, worker, origin_input, hub, target, receipt }
}

fn take_input(receiver: &Receiver<Msg>) -> String {
    let bytes: Vec<_> = receiver
        .try_iter()
        .filter_map(|message| match message {
            Msg::Input(bytes) => Some(bytes.into_owned()),
            _ => None,
        })
        .flatten()
        .collect();
    String::from_utf8(bytes).unwrap()
}

fn assert_callback(fixture: &DelegationFixture, task_id: &str, answer: &str) {
    let input = take_input(&fixture.origin_input);
    assert!(input.contains(answer), "structured worker answer missing: {input:?}");
    assert!(input.contains("worker_output="));
    assert_eq!(input.matches(task_id).count(), 1, "callback must be written exactly once");
    assert!(!fixture.hub.has_pending_delegation_callbacks());
}

#[gpui::test]
fn wsl_delegation_returns_claude_answer_to_codex(cx: &mut TestAppContext) {
    let mut fixture = fixture(cx, AgentKind::Codex, AgentKind::Claude, false, Ingress::Osc);
    fixture.worker.send("Stop", "WSL_CLAUDE_42", cx);
    cx.run_until_parked();
    assert_callback(&fixture, &fixture.receipt.task_id, "WSL_CLAUDE_42");
}

#[gpui::test]
fn wsl_delegation_returns_codex_answer_to_claude(cx: &mut TestAppContext) {
    let mut fixture = fixture(cx, AgentKind::Claude, AgentKind::Codex, false, Ingress::Osc);
    fixture.worker.send("Stop", "WSL_CODEX_42", cx);
    cx.run_until_parked();
    assert_callback(&fixture, &fixture.receipt.task_id, "WSL_CODEX_42");
}

#[gpui::test]
fn wsl_delegation_waits_for_origin_to_finish(cx: &mut TestAppContext) {
    let mut fixture = fixture(cx, AgentKind::Codex, AgentKind::Claude, true, Ingress::Osc);
    fixture.worker.send("Stop", "QUEUED_42", cx);
    cx.run_until_parked();
    assert!(take_input(&fixture.origin_input).is_empty());
    assert!(fixture.hub.has_pending_delegation_callbacks());

    fixture.origin.send("Stop", "origin finished its own turn", cx);
    cx.run_until_parked();
    assert_callback(&fixture, &fixture.receipt.task_id, "QUEUED_42");
}

#[gpui::test]
fn wsl_delegation_replayed_completion_cannot_consume_the_next_task(cx: &mut TestAppContext) {
    let mut fixture = fixture(cx, AgentKind::Codex, AgentKind::Claude, false, Ingress::Osc);
    let completed = fixture.worker.envelope("Stop", "FIRST_42");
    fixture.worker.inject(completed.clone(), cx);
    cx.run_until_parked();
    assert_callback(&fixture, &fixture.receipt.task_id, "FIRST_42");
    fixture.origin.send("Stop", "", cx);
    cx.run_until_parked();
    let next = fixture.hub.begin_delegation(fixture.origin.pane_id, &fixture.target).unwrap();

    fixture.worker.inject(completed, cx);
    cx.run_until_parked();
    assert!(take_input(&fixture.origin_input).is_empty());
    assert_eq!(
        fixture.hub.begin_delegation(fixture.origin.pane_id, &fixture.target).unwrap_err().code,
        "delegation_in_progress"
    );

    fixture.worker.send("UserPromptSubmit", "", cx);
    fixture.worker.send("Stop", "SECOND_84", cx);
    cx.run_until_parked();
    assert_callback(&fixture, &next.task_id, "SECOND_84");
}

#[gpui::test]
fn wsl_delegation_foreign_process_completion_does_not_finish_the_owner(cx: &mut TestAppContext) {
    let mut fixture = fixture(cx, AgentKind::Codex, AgentKind::Claude, false, Ingress::Osc);
    let completed = String::from_utf8(fixture.worker.envelope("Stop", "FOREIGN")).unwrap();
    let foreign =
        completed.replace(&format!("process={}:1", fixture.worker.pane_id), "process=999999:9");
    fixture.worker.inject(foreign.into_bytes(), cx);
    cx.run_until_parked();
    assert!(take_input(&fixture.origin_input).is_empty());
    assert_eq!(
        fixture.hub.begin_delegation(fixture.origin.pane_id, &fixture.target).unwrap_err().code,
        "delegation_in_progress"
    );

    fixture.worker.send("Stop", "OWNER_42", cx);
    cx.run_until_parked();
    assert_callback(&fixture, &fixture.receipt.task_id, "OWNER_42");
}

#[gpui::test]
fn native_delegation_keeps_the_same_completion_contract(cx: &mut TestAppContext) {
    let mut fixture = fixture(cx, AgentKind::Codex, AgentKind::Codex, false, Ingress::Native);
    fixture.worker.send("Stop", "NATIVE_42", cx);
    cx.run_until_parked();
    assert_callback(&fixture, &fixture.receipt.task_id, "NATIVE_42");
}

#[gpui::test]
fn native_delegation_preserves_claude_final_message(cx: &mut TestAppContext) {
    let mut fixture = fixture(cx, AgentKind::Codex, AgentKind::Claude, false, Ingress::Native);
    fixture.worker.send("Stop", "NATIVE_CLAUDE_42", cx);
    cx.run_until_parked();
    assert_callback(&fixture, &fixture.receipt.task_id, "NATIVE_CLAUDE_42");
}
