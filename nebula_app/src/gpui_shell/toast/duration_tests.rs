use super::*;
use gpui::{
    AppContext as _, BorrowAppContext as _, Context, EntityId, Render, TestAppContext,
    VisualTestContext,
};
use nebula_settings::NotificationDuration;
use std::{cell::Cell, rc::Rc};

use crate::gpui_shell::config::Settings;
use crate::gpui_shell::terminal::confirmation::Confirmation;

struct Empty;

impl Render for Empty {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
        div()
    }
}

fn open(duration: NotificationDuration, cx: &mut TestAppContext) -> VisualTestContext {
    cx.update(|cx| {
        gpui_component::init(cx);
        let mut settings = Settings::load(nebula_settings::ThemeName::Nord);
        settings.ai_toasts = true;
        settings.notification_duration = duration;
        cx.set_global(settings);
        init(cx);
    });
    let (_, window) = cx.add_window_view(|window, cx| Root::new(cx.new(|_| Empty), window, cx));
    window.clone()
}

fn ids(cx: &mut VisualTestContext) -> Vec<EntityId> {
    cx.update(|window, cx| window.notifications(cx).iter().map(|note| note.entity_id()).collect())
}

fn advance(cx: &mut VisualTestContext, duration: Duration) {
    cx.run_until_parked();
    cx.background_executor.advance_clock(duration);
    cx.run_until_parked();
}

fn settle_dismissal(cx: &mut VisualTestContext) {
    advance(cx, Duration::from_millis(250));
}

fn pi_completion() -> crate::notify::Notification {
    crate::notify::Notification::AiTurn { program: "pi".into(), message: None, attention: false }
}

#[gpui::test]
fn pi_completion_uses_five_seconds_only_when_the_user_keeps_the_default(cx: &mut TestAppContext) {
    for (index, duration) in [
        NotificationDuration::Default,
        NotificationDuration::FiveSeconds,
        NotificationDuration::TenSeconds,
        NotificationDuration::ThirtySeconds,
        NotificationDuration::NinetySeconds,
        NotificationDuration::Persistent,
    ]
    .into_iter()
    .enumerate()
    {
        let mut window = open(duration, cx);
        window.update(|window, cx| {
            banner_for_pane(
                window,
                cx,
                ToastKind::Info,
                format!("Pi complete {duration:?}"),
                84000 + index as u64,
                &pi_completion(),
            );
        });
        let original = ids(&mut window);
        assert_eq!(original.len(), 1);
        if let Some(timeout) = duration.timeout(Some(Duration::from_secs(5))) {
            advance(&mut window, timeout - Duration::from_millis(1));
            assert_eq!(ids(&mut window), original);
            advance(&mut window, Duration::from_millis(1));
            settle_dismissal(&mut window);
            assert!(ids(&mut window).is_empty(), "{duration:?}");
        } else {
            advance(&mut window, Duration::from_secs(3600));
            assert_eq!(ids(&mut window), original, "explicit persistent choice wins");
        }
    }
}

#[gpui::test]
fn pane_result_replacement_preserves_other_panes_and_uses_the_selected_duration(
    cx: &mut TestAppContext,
) {
    let mut window = open(NotificationDuration::FiveSeconds, cx);
    let old = window.update(|window, cx| {
        let failure = crate::notify::Notification::AiTurnIssue {
            program: "pi".into(),
            message: None,
            outcome: crate::ai_hook::AiTurnOutcome::Failed,
        };
        banner_for_pane(window, cx, ToastKind::Warning, "Overloaded", 83001, &failure);
        let old = window.notifications(cx)[0].clone();
        banner_for_pane(window, cx, ToastKind::Info, "Other pane", 83002, &pi_completion());
        old
    });
    let original = ids(&mut window);
    advance(&mut window, Duration::from_secs(4));
    window.update(|window, cx| {
        banner_for_pane(window, cx, ToastKind::Info, "Recovered", 83001, &pi_completion());
    });
    let replaced = ids(&mut window);
    assert_eq!(replaced.len(), 2);
    assert_eq!(replaced[0], original[1]);
    assert_ne!(replaced[1], old.entity_id());
    advance(&mut window, Duration::from_secs(1));
    settle_dismissal(&mut window);
    assert_eq!(ids(&mut window), vec![replaced[1]]);
    advance(&mut window, Duration::from_secs(4));
    settle_dismissal(&mut window);
    assert!(ids(&mut window).is_empty());
}

#[gpui::test]
fn every_timed_mode_expires_at_its_selected_duration_without_activating_actions(
    cx: &mut TestAppContext,
) {
    for duration in [
        NotificationDuration::FiveSeconds,
        NotificationDuration::TenSeconds,
        NotificationDuration::ThirtySeconds,
        NotificationDuration::NinetySeconds,
    ] {
        // Exercise every kind without overflowing the three visible slots.
        for action_cards in [false, true] {
            let mut window = open(duration, cx);
            let actions = Rc::new(Cell::new(0));
            let invoked = actions.clone();
            let update_action = actions.clone();
            window.update(|window, cx| {
                if action_cards {
                    push_notification(
                        window,
                        cx,
                        note(ToastKind::Info, "Update-style action fixture".into())
                            .on_click(move |_, _, _| update_action.set(update_action.get() + 1)),
                        None,
                    );
                    push_banner(
                        window,
                        cx,
                        note(ToastKind::Info, "Timed fixture".into())
                            .id::<AiToast>()
                            .on_click(move |_, _, _| invoked.set(invoked.get() + 1)),
                        true,
                    );
                } else {
                    toast(window, cx, ToastKind::Success, format!("Timed toast {duration:?}"));
                    banner(window, cx, ToastKind::Warning, "Timed non-AI banner");
                }
            });
            advance(&mut window, duration.timeout(None).unwrap() - Duration::from_millis(1));
            assert_eq!(ids(&mut window).len(), 2, "every card kind uses the selected lifetime");
            advance(&mut window, Duration::from_millis(1));
            settle_dismissal(&mut window);
            assert!(ids(&mut window).is_empty());
            assert_eq!(actions.get(), 0, "autohide must not perform the card's action");
        }
    }
}

#[gpui::test]
fn persistent_mode_has_no_expiry_and_retains_manual_dismissal(cx: &mut TestAppContext) {
    for action_cards in [false, true] {
        let mut window = open(NotificationDuration::Persistent, cx);
        window.update(|window, cx| {
            if action_cards {
                push_notification(
                    window,
                    cx,
                    note(ToastKind::Info, "Persistent update fixture".into()),
                    None,
                );
                confirmation_for_pane(
                    window,
                    cx,
                    "Persistent confirmation".into(),
                    8201,
                    Confirmation { choices: Vec::new(), id: 8301, question: "Continue?".into() },
                );
            } else {
                toast(window, cx, ToastKind::Success, "Persistent ordinary toast");
                banner(window, cx, ToastKind::Warning, "Persistent non-AI banner");
            }
        });
        advance(&mut window, Duration::from_secs(3600));
        assert_eq!(ids(&mut window).len(), 2);
        window.update(|window, cx| {
            for notification in window.notifications(cx).iter() {
                notification.update(cx, |notification, cx| notification.dismiss(window, cx));
            }
        });
        settle_dismissal(&mut window);
        assert!(ids(&mut window).is_empty());
    }
}

#[gpui::test]
fn refreshed_confirmation_gets_a_new_deadline_even_if_the_old_entity_is_retained(
    cx: &mut TestAppContext,
) {
    let mut window = open(NotificationDuration::FiveSeconds, cx);
    let push = |window: &mut Window, cx: &mut App| {
        confirmation_for_pane(
            window,
            cx,
            "Refreshed confirmation".into(),
            8202,
            Confirmation { choices: Vec::new(), id: 8302, question: "Continue?".into() },
        );
    };
    let old = window.update(|window, cx| {
        push(window, cx);
        window.notifications(cx)[0].clone()
    });
    advance(&mut window, Duration::from_secs(3));
    window.update(push);
    let refreshed = ids(&mut window);
    assert_eq!(refreshed.len(), 1);
    assert_ne!(refreshed[0], old.entity_id());
    advance(&mut window, Duration::from_secs(2));
    settle_dismissal(&mut window);
    assert_eq!(ids(&mut window), refreshed, "the original deadline cannot close the replacement");
    advance(&mut window, Duration::from_secs(3));
    settle_dismissal(&mut window);
    assert!(ids(&mut window).is_empty());
}

#[gpui::test]
fn a_refreshed_persistent_card_cannot_be_closed_by_its_predecessors_timer(cx: &mut TestAppContext) {
    let mut window = open(NotificationDuration::FiveSeconds, cx);
    let push = |window: &mut Window, cx: &mut App| {
        confirmation_for_pane(
            window,
            cx,
            "Mode switch fixture".into(),
            8203,
            Confirmation { choices: Vec::new(), id: 8303, question: "Continue?".into() },
        );
    };
    window.update(push);
    advance(&mut window, Duration::from_secs(3));
    window.update(|window, cx| {
        cx.update_global::<Settings, _>(|settings, _| {
            settings.notification_duration = NotificationDuration::Persistent
        });
        push(window, cx);
    });
    let refreshed = ids(&mut window);
    advance(&mut window, Duration::from_secs(3600));
    assert_eq!(ids(&mut window), refreshed);
}

#[gpui::test]
fn default_mode_preserves_short_toasts_banners_and_persistent_update_notices(
    cx: &mut TestAppContext,
) {
    for ai_banner in [false, true] {
        let mut window = open(NotificationDuration::Default, cx);
        let persistent = window.update(|window, cx| {
            push_notification(
                window,
                cx,
                note(ToastKind::Info, "Default update fixture".into()),
                None,
            );
            let persistent = window.notifications(cx)[0].entity_id();
            if ai_banner {
                confirmation_for_pane(
                    window,
                    cx,
                    "Default confirmation fixture".into(),
                    8204,
                    Confirmation { choices: Vec::new(), id: 8304, question: "Continue?".into() },
                );
            } else {
                banner(window, cx, ToastKind::Warning, "Default non-AI configuration fixture");
            }
            toast(
                window,
                cx,
                ToastKind::Success,
                format!("Default ordinary copy fixture {ai_banner}"),
            );
            persistent
        });
        assert_eq!(ids(&mut window).len(), 3);
        advance(&mut window, Duration::from_secs(5));
        settle_dismissal(&mut window);
        assert_eq!(ids(&mut window).len(), 2);
        advance(&mut window, Duration::from_secs(85));
        settle_dismissal(&mut window);
        assert_eq!(ids(&mut window), vec![persistent]);
        advance(&mut window, Duration::from_secs(3600));
        assert_eq!(ids(&mut window), vec![persistent]);
    }
}

#[gpui::test]
fn duration_still_applies_to_non_ai_cards_when_ai_visibility_is_off(cx: &mut TestAppContext) {
    let mut window = open(NotificationDuration::TenSeconds, cx);
    window.update(|window, cx| {
        cx.update_global::<Settings, _>(|settings, _| settings.ai_toasts = false);
        toast(window, cx, ToastKind::Success, "Independent duration toast");
        banner(window, cx, ToastKind::Warning, "Independent duration banner");
        push_notification(
            window,
            cx,
            note(ToastKind::Info, "Independent update fixture".into()),
            None,
        );
        confirmation_for_pane(
            window,
            cx,
            "Hidden AI fixture".into(),
            8205,
            Confirmation { choices: Vec::new(), id: 8305, question: "Continue?".into() },
        );
    });
    assert_eq!(ids(&mut window).len(), 3);
    advance(&mut window, Duration::from_secs(10));
    settle_dismissal(&mut window);
    assert!(ids(&mut window).is_empty());
}

#[gpui::test]
fn a_duration_change_does_not_reschedule_already_visible_cards(cx: &mut TestAppContext) {
    let mut window = open(NotificationDuration::FiveSeconds, cx);
    window.update(|window, cx| banner(window, cx, ToastKind::Info, "Already visible fixture"));
    advance(&mut window, Duration::from_secs(2));
    let persistent = window.update(|window, cx| {
        cx.update_global::<Settings, _>(|settings, _| {
            settings.notification_duration = NotificationDuration::Persistent
        });
        banner(window, cx, ToastKind::Info, "New persistent fixture");
        window.notifications(cx).last().unwrap().entity_id()
    });
    advance(&mut window, Duration::from_secs(3));
    settle_dismissal(&mut window);
    assert_eq!(ids(&mut window), vec![persistent]);
    advance(&mut window, Duration::from_secs(3600));
    assert_eq!(ids(&mut window), vec![persistent]);
}

#[gpui::test]
fn startup_notifications_use_the_latest_duration_after_the_root_is_installed(
    cx: &mut TestAppContext,
) {
    let _ = open(NotificationDuration::FiveSeconds, cx);
    let (_, window) = cx.add_window_view(|window, cx| {
        toast(window, cx, ToastKind::Success, "Deferred startup duration fixture");
        banner(window, cx, ToastKind::Warning, "Deferred startup banner fixture");
        cx.update_global::<Settings, _>(|settings, _| {
            settings.notification_duration = NotificationDuration::Persistent
        });
        Root::new(cx.new(|_| Empty), window, cx)
    });
    let mut window = window.clone();
    advance(&mut window, Duration::from_secs(3600));
    assert_eq!(ids(&mut window).len(), 2);
}

#[gpui::test]
fn persistent_notification_bursts_release_hidden_cards_without_running_actions(
    cx: &mut TestAppContext,
) {
    let mut window = open(NotificationDuration::Persistent, cx);
    let actions = Rc::new(Cell::new(0));
    let notices = window.update(|window, cx| {
        (0..128)
            .map(|index| {
                let invoked = actions.clone();
                push_notification(
                    window,
                    cx,
                    note(ToastKind::Info, format!("Persistent burst {index}"))
                        .on_click(move |_, _, _| invoked.set(invoked.get() + 1)),
                    None,
                );
                window.notifications(cx).last().unwrap().downgrade()
            })
            .collect::<Vec<_>>()
    });
    advance(&mut window, Duration::from_secs(3600));
    let expected =
        notices.iter().rev().take(3).rev().map(|note| note.entity_id()).collect::<Vec<_>>();
    assert_eq!(ids(&mut window), expected, "hidden persistent cards must not form a backlog");
    assert!(notices[..125].iter().all(|note| note.upgrade().is_none()));
    assert_eq!(actions.get(), 0);
}

#[gpui::test]
fn overflow_removal_cannot_dismiss_a_refreshed_confirmation(cx: &mut TestAppContext) {
    let mut window = open(NotificationDuration::Persistent, cx);
    let (old, expected) = window.update(|window, cx| {
        let push = |window: &mut Window, cx: &mut App| {
            confirmation_for_pane(
                window,
                cx,
                "Overflow confirmation".into(),
                8206,
                Confirmation { choices: Vec::new(), id: 8306, question: "Continue?".into() },
            );
        };
        push(window, cx);
        let old = window.notifications(cx)[0].clone();
        for index in 0..3 {
            banner(window, cx, ToastKind::Info, format!("Newer card {index}"));
        }
        // The old entity's removal event has not been delivered yet.
        push(window, cx);
        let notes = window.notifications(cx);
        let expected =
            notes.iter().rev().take(3).rev().map(|note| note.entity_id()).collect::<Vec<_>>();
        assert_ne!(old.entity_id(), *expected.last().unwrap());
        (old, expected)
    });
    advance(&mut window, Duration::from_secs(3600));
    assert_eq!(ids(&mut window), expected);
    drop(old);
}
