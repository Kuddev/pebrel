use super::*;
use gpui::{
    AppContext as _, BorrowAppContext as _, Context, EntityId, Render, TestAppContext,
    VisualTestContext,
};
use nebula_settings::NotificationDuration;
use std::{cell::Cell, rc::Rc};

use crate::gpui_shell::config::Settings;
use crate::gpui_shell::terminal::confirmation::BinaryConfirmation;

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
        let mut window = open(duration, cx);
        let actions = Rc::new(Cell::new(0));
        let invoked = actions.clone();
        let update_action = actions.clone();
        window.update(|window, cx| {
            toast(window, cx, ToastKind::Success, format!("Timed toast {duration:?}"));
            banner(window, cx, ToastKind::Warning, "Timed non-AI banner");
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
        });
        advance(&mut window, duration.timeout(None).unwrap() - Duration::from_millis(1));
        assert_eq!(ids(&mut window).len(), 4, "every card kind uses the selected lifetime");
        advance(&mut window, Duration::from_millis(1));
        settle_dismissal(&mut window);
        assert!(ids(&mut window).is_empty());
        assert_eq!(actions.get(), 0, "autohide must not perform the card's action");
    }
}

#[gpui::test]
fn persistent_mode_has_no_expiry_and_retains_manual_dismissal(cx: &mut TestAppContext) {
    let mut window = open(NotificationDuration::Persistent, cx);
    window.update(|window, cx| {
        toast(window, cx, ToastKind::Success, "Persistent ordinary toast");
        banner(window, cx, ToastKind::Warning, "Persistent non-AI banner");
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
            BinaryConfirmation { id: 8301, question: "Continue?".into() },
        );
    });
    advance(&mut window, Duration::from_secs(3600));
    assert_eq!(ids(&mut window).len(), 4);
    window.update(|window, cx| {
        for notification in window.notifications(cx).iter() {
            notification.update(cx, |notification, cx| notification.dismiss(window, cx));
        }
    });
    settle_dismissal(&mut window);
    assert!(ids(&mut window).is_empty());
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
            BinaryConfirmation { id: 8302, question: "Continue?".into() },
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
            BinaryConfirmation { id: 8303, question: "Continue?".into() },
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
    let mut window = open(NotificationDuration::Default, cx);
    let persistent = window.update(|window, cx| {
        push_notification(window, cx, note(ToastKind::Info, "Default update fixture".into()), None);
        let persistent = window.notifications(cx)[0].entity_id();
        banner(window, cx, ToastKind::Warning, "Default non-AI configuration fixture");
        toast(window, cx, ToastKind::Success, "Default ordinary copy fixture");
        confirmation_for_pane(
            window,
            cx,
            "Default confirmation fixture".into(),
            8204,
            BinaryConfirmation { id: 8304, question: "Continue?".into() },
        );
        persistent
    });
    assert_eq!(ids(&mut window).len(), 4);
    advance(&mut window, Duration::from_secs(5));
    settle_dismissal(&mut window);
    assert_eq!(ids(&mut window).len(), 3);
    advance(&mut window, Duration::from_secs(85));
    settle_dismissal(&mut window);
    assert_eq!(ids(&mut window), vec![persistent]);
    advance(&mut window, Duration::from_secs(3600));
    assert_eq!(ids(&mut window), vec![persistent]);
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
            BinaryConfirmation { id: 8305, question: "Continue?".into() },
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
