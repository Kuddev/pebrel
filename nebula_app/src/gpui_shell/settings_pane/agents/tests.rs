use super::*;
use crate::ai_hook::integrations::HookInspection;
use futures::FutureExt as _;
use gpui::{Modifiers, TestAppContext, VisualTestContext, point, size};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

type Reply = (Result<(), String>, Vec<AgentIntegration>);

fn rows(installed: bool) -> Vec<AgentIntegration> {
    vec![AgentIntegration {
        agent: AgentKind::Claude,
        executable: Some("C:/test/claude.exe".into()),
        hook: Some(nebula_settings::AgentHook::Claude),
        inspection: HookInspection {
            available: true,
            installed,
            enabled: installed,
            config_path: Some("C:/test/.claude/settings.json".into()),
            ..Default::default()
        },
    }]
}

fn fixture(
    cx: &mut TestAppContext,
) -> (
    Entity<SettingsPane>,
    &mut VisualTestContext,
    futures::channel::oneshot::Sender<Reply>,
    Arc<AtomicUsize>,
) {
    let (sender, receiver) = futures::channel::oneshot::channel();
    let receiver = Arc::new(Mutex::new(Some(receiver)));
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
    });
    let mut pane = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| SettingsPane::new(window, cx));
        view.update(cx, |pane, _| {
            pane.active_section = 10;
            pane.agents.rows = Some(rows(true));
            pane.agents.test_operation = Some(Arc::new(move |hook, enabled| {
                assert_eq!(hook, nebula_settings::AgentHook::Claude);
                assert!(!enabled, "the real installed state makes the first action removal");
                counted.fetch_add(1, Ordering::SeqCst);
                let receiver = receiver.lock().unwrap().take().expect("one operation at a time");
                async move { receiver.await.unwrap() }.boxed()
            }));
        });
        pane = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    window.simulate_resize(size(px(1040.0), px(900.0)));
    window.run_until_parked();
    (pane.unwrap(), window, sender, calls)
}

#[gpui::test]
fn agents_content_is_centered_and_paths_leave_room_for_hook_status(cx: &mut TestAppContext) {
    let (pane, window, reply, _) = fixture(cx);
    drop(reply);
    pane.update(window, |pane, cx| {
        pane.agents.rows.as_mut().unwrap()[0].executable = Some(
            "C:/Users/example/AppData/Local/a-very-long-installation-directory/bin/claude.exe"
                .into(),
        );
        cx.notify();
    });
    for width in [1440.0, 800.0] {
        window.simulate_resize(size(px(width), px(900.0)));
        window.run_until_parked();
        window.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        let row = window.debug_bounds("agent-hook-row-0").unwrap();
        let path = window.debug_bounds("agent-cli-path-0").unwrap();
        let status = window.debug_bounds("agent-hook-status-0").unwrap();
        let center = px((width + SETTINGS_NAV_WIDTH) / 2.0);
        assert!((f32::from(row.center().x - center)).abs() <= 2.0);
        assert!(row.size.width <= px(720.0));
        assert!(path.size.width > px(0.0));
        assert!(path.right() <= status.left());
        assert!(status.right() + px(40.0) <= row.right());
    }
}

#[gpui::test]
fn installed_hook_row_accepts_padding_clicks_and_reports_write_failure(cx: &mut TestAppContext) {
    if !crate::platform::CAPABILITIES.ai_hook_server {
        return;
    }
    let (pane, window, reply, calls) = fixture(cx);
    let bounds = window.debug_bounds("agent-hook-row-0").expect("Agents row rendered");
    assert!(bounds.size.height >= px(64.0));
    assert!(bounds.right() <= px(1040.0));
    // The row's left padding is a real hit target, not just the label or switch.
    let padding = bounds.origin + point(px(5.0), px(5.0));
    window.simulate_click(padding, Modifiers::default());
    window.run_until_parked();
    assert_eq!(pane.read_with(window, |pane, _| pane.agents.busy), Some(AgentKind::Claude));
    window.simulate_click(padding, Modifiers::default());
    window.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    reply.send((Err("fixture write denied".into()), rows(true))).unwrap();
    window.run_until_parked();
    pane.read_with(window, |pane, _| {
        assert!(pane.agents.busy.is_none());
        assert!(pane.agents.rows.as_ref().unwrap()[0].inspection.installed);
        assert!(matches!(&pane.agents.feedback, Some((AgentKind::Claude, Err(error))) if error == "fixture write denied"));
    });
}

#[gpui::test]
fn hook_switch_supports_keyboard_and_shows_success_only_after_completion(cx: &mut TestAppContext) {
    if !crate::platform::CAPABILITIES.ai_hook_server {
        return;
    }
    let (pane, window, reply, calls) = fixture(cx);
    window.update(|window, cx| {
        let focus = pane.read(cx).agents.focus[0].clone();
        window.focus(&focus, cx);
    });
    window.simulate_keystrokes("space");
    window.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(pane.read_with(window, |pane, _| pane.agents.feedback.is_none()));
    reply.send((Ok(()), rows(false))).unwrap();
    window.run_until_parked();
    pane.read_with(window, |pane, _| {
        assert!(!pane.agents.rows.as_ref().unwrap()[0].inspection.enabled);
        assert!(matches!(&pane.agents.feedback, Some((AgentKind::Claude, Ok(false)))));
    });
}

#[gpui::test]
fn configuration_without_an_executable_is_not_an_installed_agent(cx: &mut TestAppContext) {
    let (pane, window, reply, calls) = fixture(cx);
    drop(reply);
    pane.update(window, |pane, cx| {
        let mut rows = rows(false);
        rows[0].executable = None;
        assert!(rows[0].inspection.available);
        assert!(matches!(
            agent_status(&rows[0], false),
            Message::SettingsAgentsOff | Message::SettingsAgentsUnavailable
        ));
        assert!(!can_toggle(&rows[0]));
        rows[0].inspection.installed = true;
        assert!(matches!(agent_status(&rows[0], false), Message::SettingsAgentsInstalled));
        rows[0].inspection.installed = false;
        pane.agents.rows = Some(rows);
        cx.notify();
    });
    window.run_until_parked();
    let bounds = window.debug_bounds("agent-hook-row-0").unwrap();
    window.simulate_click(bounds.center(), Modifiers::default());
    window.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    pane.read_with(window, |pane, _| assert!(pane.agents.busy.is_none()));
}

#[gpui::test]
fn repeated_switches_follow_completed_state_and_ignore_inflight_clicks(cx: &mut TestAppContext) {
    if !crate::platform::CAPABILITIES.ai_hook_server {
        return;
    }
    let (pane, window, unused, _) = fixture(cx);
    drop(unused);
    let pending = Arc::new(Mutex::new(std::collections::VecDeque::new()));
    let queued = pending.clone();
    window.update(|_, cx| {
        pane.update(cx, |pane, _| {
            pane.agents.test_operation = Some(Arc::new(move |_, enabled| {
                let (sender, receiver) = futures::channel::oneshot::channel::<Reply>();
                queued.lock().unwrap().push_back((enabled, sender));
                async move { receiver.await.unwrap() }.boxed()
            }));
        });
    });
    for cycle in 0..12 {
        let enabled = cycle % 2 != 0;
        let bounds = window.debug_bounds("agent-hook-row-0").unwrap();
        window.simulate_click(bounds.origin + point(px(5.0), px(5.0)), Modifiers::default());
        window.run_until_parked();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.simulate_keystrokes("space");
        window.run_until_parked();
        assert_eq!(pending.lock().unwrap().len(), 1, "in-flight clicks must not enqueue writes");
        let (requested, reply) = pending.lock().unwrap().pop_front().unwrap();
        assert_eq!(requested, enabled);
        reply.send((Ok(()), rows(enabled))).unwrap();
        window.run_until_parked();
        pane.read_with(window, |pane, _| {
            assert!(pane.agents.busy.is_none());
            assert_eq!(pane.agents.rows.as_ref().unwrap()[0].inspection.enabled, enabled);
            assert!(matches!(&pane.agents.feedback, Some((AgentKind::Claude, Ok(actual))) if *actual == enabled));
        });
    }
}

#[gpui::test]
fn closing_settings_does_not_cancel_a_submitted_hook_write_or_reopen_the_view(
    cx: &mut TestAppContext,
) {
    if !crate::platform::CAPABILITIES.ai_hook_server {
        return;
    }
    let (pane, window, reply, calls) = fixture(cx);
    let bounds = window.debug_bounds("agent-hook-row-0").unwrap();
    window.simulate_click(bounds.center(), Modifiers::default());
    window.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let weak = pane.downgrade();
    window.update(|window, _| window.remove_window());
    drop(pane);
    reply.send((Ok(()), rows(false))).unwrap();
    window.run_until_parked();
    assert!(weak.upgrade().is_none());
}
