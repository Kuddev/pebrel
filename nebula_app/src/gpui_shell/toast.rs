//! GPUI 壳的提示三层（裁定见 `display/toast.rs` 模块文档）：
//!
//! - **toast**：已经结束的事实，没有待办动作（恢复成功、复制完成）。
//!   走组件库 `NotificationList`（`Root` 自带渲染层），默认 5s 自动消失。
//! - **消息栏**：值得驻留、要用户看见的事（配置错误、断路器、AI 回合完成）。
//!   同一渲染层，默认停留 [`BANNER_TTL`] 后自动退场，带关闭按钮。
//! - **模态**：阻断性决策。调用点自建 `Dialog`，不经此模块。
//!
//! 通知显示时长可覆盖 toast、消息栏及更新卡片的默认值，也可保持常驻；
//! 仅影响新显示或刷新的卡片，不改变系统通知、实际待处理状态或确认结果。
//!
//! 判据是「有没有待办动作」而不是严重度。toast 允许自动消失的前提：每条
//! 同时落一行日志——toast 不承载唯一副本，错过那几秒的用户仍能在日志里
//! 查到。
//!
//! 与旧壳合同的已知差异：默认停留 5s（旧壳 4.2s）；同屏保留最新三条，
//! 重复冷却在本层补齐（600ms，同文本）。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement as _, ParentElement as _, Styled as _,
    Window, div, px,
};
use gpui_component::notification::Notification;
use gpui_component::{Root, WindowExt as _};

use crate::gpui_shell::prelude::*;

pub use crate::display::ToastKind;

/// 同一文本短时间连发只刷新语境，不叠一摞相同卡片（旧壳
/// `DUPLICATE_COOLDOWN` 同值同义；只挡逐字相同的文本）。
const DUPLICATE_COOLDOWN: Duration = Duration::from_millis(600);
const TOAST_TTL: Duration = Duration::from_secs(5);
const MAX_NOTIFICATIONS: usize = 3;

/// 组件库 Notification 默认固定 `w_112`，短短一句也会铺成近半个窗口。
/// Nebula 的 toast 按内容收缩；过长消息到此上限后自然换行。
const TOAST_MIN_WIDTH: f32 = 240.0;
const TOAST_MAX_WIDTH: f32 = 440.0;

/// 驻留消息的兜底寿命。
///
/// 「不自动消失」本意是保证用户一定看见，实际效果是队列只增不减（用户
/// 08-23 报的「完成提示一直不消失」）：AI 回合完成、后台 tab 的 OSC 9 通知
/// 这类事件会持续产出，没人手动点掉就一直摞在右下角，而这些消息里根本没有
/// 待办动作可点。
///
/// 所以驻留改成「长而有限」：90s 远长于 toast 的 5s，够人从别的窗口回来看
/// 一眼；无人值守时也保证自己退场。手动点掉仍然立刻生效。
const BANNER_TTL: Duration = Duration::from_secs(90);

static LAST_TOAST: Mutex<Option<(String, Instant)>> = Mutex::new(None);
static PANE_BANNERS: Mutex<PaneBannerGate> = Mutex::new(PaneBannerGate { recent: Vec::new() });

/// A component notification identity, not a separate queue or lifetime manager.
struct AiToast;
struct PaneToast;

#[cfg(all(test, feature = "gpui-test-support"))]
mod duration_tests;

/// Apply the AI visibility switch to existing notices in every open window.
/// Duration changes apply at insertion or refresh, not to already visible cards.
/// Native system notifications have a separate delivery path and remain intact.
pub(crate) fn init(cx: &mut App) {
    cx.observe_global::<super::config::Settings>(|cx| {
        if super::config::ai_toasts_enabled(cx) {
            return;
        }
        for handle in cx.windows() {
            let _ = handle.update(cx, |_, window, cx| {
                if window.root::<Root>().flatten().is_some() {
                    window.remove_notification::<AiToast>(cx);
                }
            });
        }
    })
    .detach();
}

#[derive(Default)]
struct PaneBannerGate {
    recent: Vec<(u64, ToastKind, String, Instant)>,
}

impl PaneBannerGate {
    fn accepts(&mut self, pane_id: u64, kind: ToastKind, text: &str, now: Instant) -> bool {
        self.recent
            .retain(|(_, _, _, born)| now.saturating_duration_since(*born) < DUPLICATE_COOLDOWN);
        if self.recent.iter().any(|(pane, previous_kind, previous_text, _)| {
            *pane == pane_id && *previous_kind == kind && previous_text == text
        }) {
            return false;
        }
        // Recovery ends the previous result, even when the same error returns
        // before the duplicate cooldown has elapsed.
        self.recent.retain(|(pane, _, _, _)| *pane != pane_id);
        self.recent.push((pane_id, kind, text.to_owned(), now));
        true
    }
}

fn is_duplicate(text: &str) -> bool {
    let mut last = LAST_TOAST.lock().unwrap_or_else(|poison| poison.into_inner());
    let duplicate = last
        .as_ref()
        .is_some_and(|(prev, born)| prev == text && born.elapsed() < DUPLICATE_COOLDOWN);
    if !duplicate {
        *last = Some((text.to_owned(), Instant::now()));
    }
    duplicate
}

fn note(kind: ToastKind, text: String) -> Notification {
    // 通知栏是「一眼带过」的层。正文先收短：模型回合完成的正文可能是整段
    // 回答（OSC 9 更是无界），不设上限时按固定宽度排出来比窗口还高，锚在
    // 右下就会整条溢出屏幕、把关闭按钮顶到看不见的地方。完整原文仍在日志
    // 与终端的「阅读 / 原文」里。
    let text = crate::notify::clamp_toast_body(&text);
    let note = match kind {
        ToastKind::Info => Notification::info(text),
        ToastKind::Success => Notification::success(text),
        ToastKind::Warning => Notification::warning(text),
    };
    // `refine_style` 在组件自身的 `w_112` 之后应用，所以这里能可靠覆盖
    // 固定宽度；auto 让短消息收紧，max_w 给长路径/错误信息提供换行约束。
    note.w_auto().min_w(px(TOAST_MIN_WIDTH)).max_w(px(TOAST_MAX_WIDTH))
}

/// 窗口构造闭包里 `NebulaWorkspace::new` 早于外层 `Root::new` 返回；此时
/// 直接 push 会让组件库查找尚未安装的首层 Root 并 panic。启动期通知延后
/// 到当前 effect cycle 结束，正常运行期仍同步推送。
/// 允许少数业务通知复用 Nebula 的启动期 Root 守卫和右下角宿主；通知内容与
/// 动作仍由业务模块定义，避免把更新等领域状态塞进通用 toast 层。
/// `default_timeout` preserves the caller's original lifetime in default mode;
/// `None` means the card originally remained until dismissed.
pub(crate) fn push_notification(
    window: &mut Window,
    cx: &mut App,
    notification: Notification,
    default_timeout: Option<Duration>,
) {
    push_with_duration(window, cx, notification, default_timeout, false);
}

/// Nebula 的通知层定位：右下角固定 20px 安全边距，最新一条在最下，旧的
/// 向上堆叠。组件库默认 `NotificationList` 写死在右上角且拉满视口高度；
/// 这里继续复用它的 Notification 实体、生命周期、关闭和动画，只替换宿主
/// 的排列层，不复制组件内部状态机。
pub fn render_layer(window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let root = window.root::<Root>()??;
    let list = root.read(cx).notification.clone();
    let mut items = list.read(cx).notifications();
    // 与旧壳同屏上限一致。保留队尾三条，顺序仍是旧→新，因此 v_flex
    // 底部锚定后最新项自然落在最下面。
    if items.len() > MAX_NOTIFICATIONS {
        items.drain(..items.len() - MAX_NOTIFICATIONS);
    }
    if items.is_empty() {
        return None;
    }
    Some(
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_end()
            .justify_end()
            .p(px(20.0))
            // 约束动画和阴影的绘制范围。裁剪本身不限制卡片/堆叠高度，
            // 关闭按钮是否可达仍须由窗口尺寸与真实布局回归验证。
            .overflow_hidden()
            // 用全视口透明宿主明确右下锚点；宿主不注册命中，只有实际
            // Notification 卡片会 occlude/接收鼠标，周围终端仍可正常点击。
            .child(v_flex().items_end().gap_2().children(items))
            .into_any_element(),
    )
}

/// 弹一条短提示（默认自动消失）。空文本与 600ms 内的重复文本静默丢弃。
pub fn toast(window: &mut Window, cx: &mut App, kind: ToastKind, text: impl Into<String>) {
    let text = text.into();
    if text.trim().is_empty() || is_duplicate(&text) {
        return;
    }
    log::info!("toast [{kind:?}]: {text}");
    push_notification(window, cx, note(kind, text), Some(TOAST_TTL));
}

/// 驻留一条消息（消息栏层）：默认停留远长于 toast，但**有上限**，见
/// [`BANNER_TTL`]。用于「值得驻留」的事——没什么值得驻留的请用 [`toast`]。
///
/// 这里也挂一个空的 `on_click`：组件库的点击包装会先 `dismiss` 再调回调，
/// 所以整张卡片点一下就消失。驻留消息的关闭按钮只在悬停时出现，没有这条
/// 路径的话「怎么都关不掉」就是字面事实。
pub fn banner(window: &mut Window, cx: &mut App, kind: ToastKind, text: impl Into<String>) {
    let text = text.into();
    if text.trim().is_empty() {
        return;
    }
    log::warn!("banner [{kind:?}]: {text}");
    push_banner(window, cx, note(kind, text).on_click(|_, _, _| {}), false);
}

pub(crate) fn banner_for_pane(
    window: &mut Window,
    cx: &mut App,
    kind: ToastKind,
    text: impl Into<String>,
    pane_id: u64,
    source: &crate::notify::Notification,
) {
    let ai_toast = source.is_ai();
    let text = text.into();
    if text.trim().is_empty()
        || !PANE_BANNERS.lock().unwrap_or_else(|poison| poison.into_inner()).accepts(
            pane_id,
            kind,
            &text,
            Instant::now(),
        )
    {
        return;
    }
    log::info!("pane banner [{kind:?}] pane={pane_id}: {text}");
    let mut notification = note(kind, text).on_click(move |_, _, cx| {
        cx.defer(move |cx| super::workspace::windowing::focus_notification(Some(pane_id), cx));
    });
    if ai_toast {
        notification = notification.id1::<AiToast>(("ai-result", pane_id));
    } else {
        notification = notification.id1::<PaneToast>(("pane-result", pane_id));
    }
    // Pi completion has a shorter source default. The shared duration setting
    // still overrides this value, including the explicit Persistent choice.
    let ttl = match source {
        crate::notify::Notification::AiTurn { program, message: None, attention: false }
            if program == "pi" =>
        {
            Duration::from_secs(5)
        },
        _ => BANNER_TTL,
    };
    push_with_duration(window, cx, notification, Some(ttl), ai_toast);
}

pub(crate) fn confirmation_for_pane(
    window: &mut Window,
    cx: &mut App,
    text: String,
    pane_id: u64,
    confirmation: crate::gpui_shell::terminal::confirmation::Confirmation,
) {
    let language = super::config::ui_language(cx);
    let allow_label = language.text(crate::i18n::Message::CommonYes).to_owned();
    let deny_label = language.text(crate::i18n::Message::CommonNo).to_owned();
    let request_id = confirmation.id;
    let notification = note(ToastKind::Warning, text)
        .id1::<AiToast>(format!("pane-{pane_id}-request-{request_id}"))
        .on_click(move |_, _, cx| {
            cx.defer(move |cx| super::workspace::windowing::focus_notification(Some(pane_id), cx));
        })
        .content(move |_, _, cx| {
            if !confirmation.choices.is_empty() {
                return v_flex()
                    .gap_2()
                    .child(div().text_sm().child(confirmation.question.clone()))
                    .children(confirmation.choices.iter().enumerate().map(|(choice, label)| {
                        Button::new(("confirmation-choice", choice))
                            .debug_selector(|| format!("confirmation-choice-{choice}"))
                            .w_full()
                            .min_h(px(32.0))
                            .tooltip(label.clone())
                            .label(label.clone())
                            .small()
                            .ghost()
                            .on_click(cx.listener(move |note, _, window, cx| {
                                cx.stop_propagation();
                                let feedback = window.window_handle();
                                note.dismiss(window, cx);
                                cx.defer(move |cx| {
                                    reply_to_choice(pane_id, request_id, choice, Some(feedback), cx)
                                });
                            }))
                    }))
                    .into_any_element();
            }
            v_flex()
                .gap_2()
                .child(div().text_sm().child(confirmation.question.clone()))
                .child(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new(("confirmation-deny", request_id))
                                .icon(IconName::Close)
                                .label(deny_label.clone())
                                .ghost()
                                .small()
                                .on_click(cx.listener(move |note, _, window, cx| {
                                    cx.stop_propagation();
                                    let feedback = window.window_handle();
                                    note.dismiss(window, cx);
                                    cx.defer(move |cx| {
                                        reply_to_confirmation(
                                            pane_id, request_id, false, feedback, cx,
                                        )
                                    });
                                })),
                        )
                        .child(
                            Button::new(("confirmation-allow", request_id))
                                .icon(IconName::Check)
                                .label(allow_label.clone())
                                .primary()
                                .small()
                                .on_click(cx.listener(move |note, _, window, cx| {
                                    cx.stop_propagation();
                                    let feedback = window.window_handle();
                                    note.dismiss(window, cx);
                                    cx.defer(move |cx| {
                                        reply_to_confirmation(
                                            pane_id, request_id, true, feedback, cx,
                                        )
                                    });
                                })),
                        ),
                )
                .into_any_element()
        });
    push_banner(window, cx, notification, true);
}

fn reply_to_confirmation(
    pane_id: u64,
    request_id: u64,
    allow: bool,
    feedback: gpui::AnyWindowHandle,
    cx: &mut App,
) {
    reply_to_choice(pane_id, request_id, usize::from(!allow), Some(feedback), cx);
}

pub(crate) fn reply_to_choice(
    pane_id: u64,
    request_id: u64,
    choice: usize,
    feedback: Option<gpui::AnyWindowHandle>,
    cx: &mut App,
) {
    super::workspace::windowing::focus_notification(Some(pane_id), cx);
    let applied = super::workspace::windowing::notification_view(pane_id, cx)
        .is_some_and(|view| view.update(cx, |view, cx| view.answer_choice(request_id, choice, cx)));
    if !applied && let Some(feedback) = feedback {
        let _ = feedback.update(cx, |_, window, cx| {
            let language = crate::display::LanguagePreference::from(
                nebula_settings::RuntimeSettings::load().language,
            )
            .resolved();
            toast(
                window,
                cx,
                ToastKind::Warning,
                language.text(crate::i18n::Message::CommonConfirmationExpired),
            );
        });
    }
}

fn push_banner(window: &mut Window, cx: &mut App, notification: Notification, ai_toast: bool) {
    push_with_duration(window, cx, notification, Some(BANNER_TTL), ai_toast);
}

fn push_with_duration(
    window: &mut Window,
    cx: &mut App,
    notification: Notification,
    default_timeout: Option<Duration>,
    ai_toast: bool,
) {
    if ai_toast && !super::config::ai_toasts_enabled(cx) {
        return;
    }
    let note = notification.autohide(false);
    if window.root::<Root>().flatten().is_some() {
        window.push_notification(note, cx);
        finish_notification_insertion(window, cx, default_timeout);
    } else {
        window.defer(cx, move |window, cx| {
            // The preference may change before the startup Root is installed.
            if ai_toast && !super::config::ai_toasts_enabled(cx) {
                return;
            }
            if window.root::<Root>().flatten().is_some() {
                window.push_notification(note, cx);
                finish_notification_insertion(window, cx, default_timeout);
            } else {
                log::warn!("notification dropped because the window Root is unavailable");
            }
        });
    }
}

/// Release cards outside the visible stack, then schedule the inserted card's expiry.
/// Persistent cards have no timer. Retiring a hidden card emits the component's
/// removal event without invoking click, action or close callbacks. As with identity
/// replacement, retirement discards presentation only; domain state stays with its owner.
///
/// 组件库把 autohide 做成 bool、5s 写死在 `NotificationList::push` 里，没有
/// 自定义时长的入口；为这个改 fork 要动 rev pin，不值得。换个办法：push 之后
/// 队尾那条就是我们刚推的，抓住它的**弱**引用，到点自己调 `dismiss`。用户
/// 提前点掉时弱引用已经失效，定时器什么也不做，不会去误关后来的消息。
fn finish_notification_insertion(
    window: &mut Window,
    cx: &mut App,
    default_timeout: Option<Duration>,
) {
    let Some(Some(root)) = window.root::<Root>() else { return };
    let list = root.read(cx).notification.clone();
    let notes = list.read(cx).notifications();
    for note in notes.iter().take(notes.len().saturating_sub(MAX_NOTIFICATIONS)) {
        note.update(cx, |_, cx| cx.emit(gpui::DismissEvent));
    }
    let timeout = cx
        .try_global::<super::config::Settings>()
        .map(|settings| settings.notification_duration)
        .unwrap_or_default()
        .timeout(default_timeout);
    let Some(timeout) = timeout else { return };
    let Some(note) = notes.last() else { return };
    let note = note.downgrade();
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(timeout).await;
            let _ = cx.update(|window, cx| {
                if let Some(note) = note.upgrade() {
                    note.update(cx, |note, cx| note.dismiss(window, cx));
                }
            });
        })
        .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_gate_blocks_only_rapid_identical_text() {
        // 进程级状态：用本测试独有的文本，别的测试并发跑也不会撞。
        assert!(!is_duplicate("toast-dup-gate-a"));
        assert!(is_duplicate("toast-dup-gate-a"));
        assert!(!is_duplicate("toast-dup-gate-b"), "不同文本不受冷却影响");
    }

    #[test]
    fn pane_banners_keep_distinct_sources_and_attention() {
        let now = Instant::now();
        let mut gate = PaneBannerGate::default();
        assert!(gate.accepts(1, ToastKind::Info, "done", now));
        assert!(gate.accepts(2, ToastKind::Info, "done", now));
        assert!(!gate.accepts(1, ToastKind::Info, "done", now));
        assert!(gate.accepts(1, ToastKind::Warning, "done", now));
        assert!(gate.accepts(1, ToastKind::Warning, "permission", now));
        assert!(!gate.accepts(1, ToastKind::Warning, "permission", now));
        assert!(gate.accepts(1, ToastKind::Info, "done", now));
        assert!(!gate.accepts(1, ToastKind::Info, "done", now));
        assert!(gate.accepts(1, ToastKind::Info, "done", now + DUPLICATE_COOLDOWN));
        assert_eq!(gate.recent.len(), 1);
    }

    #[cfg(feature = "gpui-test-support")]
    mod interaction {
        use super::*;
        use gpui::{
            AppContext as _, BorrowAppContext as _, Context, EntityId, Render, TestAppContext,
            VisualTestContext,
        };

        use crate::gpui_shell::config::Settings;
        use crate::gpui_shell::terminal::confirmation::Confirmation;

        struct Empty;

        impl Render for Empty {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
                div()
            }
        }

        fn initialize(cx: &mut TestAppContext, enabled: bool) {
            cx.update(|cx| {
                gpui_component::init(cx);
                let mut settings = Settings::load(nebula_settings::ThemeName::Nord);
                settings.ai_toasts = enabled;
                settings.notification_duration = nebula_settings::NotificationDuration::Default;
                cx.set_global(settings);
                init(cx);
            });
        }

        fn ids(cx: &mut VisualTestContext) -> Vec<EntityId> {
            cx.update(|window, cx| {
                window.notifications(cx).iter().map(|note| note.entity_id()).collect()
            })
        }

        fn pi_completion() -> crate::notify::Notification {
            crate::notify::Notification::AiTurn {
                program: "pi".into(),
                message: None,
                attention: false,
            }
        }

        fn settle_dismissal(cx: &mut VisualTestContext) {
            cx.run_until_parked();
            cx.background_executor.advance_clock(Duration::from_millis(200));
            cx.run_until_parked();
        }

        #[gpui::test]
        fn numbered_choice_buttons_have_real_hit_targets_and_report_a_closed_pane(
            cx: &mut TestAppContext,
        ) {
            struct NotificationSurface;
            impl Render for NotificationSurface {
                fn render(
                    &mut self,
                    window: &mut Window,
                    cx: &mut Context<Self>,
                ) -> impl gpui::IntoElement {
                    div().relative().size_full().children(render_layer(window, cx))
                }
            }
            initialize(cx, true);
            cx.update(|cx| {
                cx.set_reduce_motion(true);
                crate::gpui_shell::workspace::windowing::initialize(
                    cx,
                    crate::runtime_api::RuntimeHub::new(),
                );
            });
            let mut surface = None;
            let (_, cx) = cx.add_window_view(|window, cx| {
                let view = cx.new(|_| NotificationSurface);
                surface = Some(view.clone());
                Root::new(view, window, cx)
            });
            let surface = surface.unwrap();
            cx.update(|window, cx| {
                confirmation_for_pane(
                    window,
                    cx,
                    "Choose a scope".into(),
                    99991,
                    Confirmation {
                        id: 99991,
                        question: "Choose a scope".into(),
                        choices: vec!["Current directory".into(), "Whole workspace".into()],
                    },
                )
            });
            cx.update(|_, cx| surface.update(cx, |_, cx| cx.notify()));
            cx.run_until_parked();
            cx.background_executor.advance_clock(Duration::from_millis(200));
            cx.run_until_parked();
            cx.update(|window, cx| {
                surface.update(cx, |_, cx| cx.notify());
                let _ = window.draw(cx);
            });
            let original = ids(cx);
            assert_eq!(original.len(), 1);
            let bounds =
                cx.debug_bounds("confirmation-choice-1").expect("numbered option is rendered");
            assert!(bounds.size.width >= px(100.0) && bounds.size.height >= px(32.0));
            cx.simulate_mouse_down(
                bounds.center(),
                gpui::MouseButton::Left,
                gpui::Modifiers::default(),
            );
            cx.simulate_mouse_up(
                bounds.center(),
                gpui::MouseButton::Left,
                gpui::Modifiers::default(),
            );
            settle_dismissal(cx);
            let current = ids(cx);
            assert!(!current.contains(&original[0]), "the clicked stale card is dismissed");
            assert_eq!(
                current.len(),
                1,
                "the closed source produces visible expired-request feedback"
            );
        }

        #[gpui::test]
        fn disabling_ai_toasts_removes_only_ai_cards_and_can_be_reenabled(cx: &mut TestAppContext) {
            initialize(cx, true);
            let (_, cx) = cx.add_window_view(|window, cx| Root::new(cx.new(|_| Empty), window, cx));
            cx.update(|window, cx| {
                banner_for_pane(
                    window,
                    cx,
                    ToastKind::Info,
                    "AI completed",
                    7101,
                    &pi_completion(),
                );
                confirmation_for_pane(
                    window,
                    cx,
                    "AI confirmation".into(),
                    7101,
                    Confirmation { choices: Vec::new(), id: 7201, question: "Continue?".into() },
                );
                banner(window, cx, ToastKind::Warning, "Configuration needs attention");
            });
            cx.run_until_parked();
            let original = ids(cx);
            assert_eq!(original.len(), 3);
            cx.update(|_, cx| {
                cx.update_global::<Settings, _>(|settings, _| settings.ai_toasts = false);
            });
            settle_dismissal(cx);
            assert_eq!(ids(cx), vec![original[2]]);
            cx.update(|_, cx| {
                cx.update_global::<Settings, _>(|settings, _| settings.ai_toasts = true);
            });
            cx.run_until_parked();
            assert_eq!(ids(cx), vec![original[2]], "reenabling must not replay old notices");
            cx.update(|window, cx| {
                banner_for_pane(
                    window,
                    cx,
                    ToastKind::Info,
                    "Next AI turn",
                    7101,
                    &pi_completion(),
                );
            });
            assert_eq!(ids(cx).len(), 2);
        }

        #[gpui::test]
        fn pi_completion_expires_without_dismissing_attention_or_later_turns(
            cx: &mut TestAppContext,
        ) {
            initialize(cx, true);
            let (_, cx) = cx.add_window_view(|window, cx| Root::new(cx.new(|_| Empty), window, cx));
            cx.update(|window, cx| {
                banner_for_pane(
                    window,
                    cx,
                    ToastKind::Info,
                    "First Pi completion",
                    7301,
                    &pi_completion(),
                );
                let failed = crate::notify::Notification::AiTurnIssue {
                    program: "pi".into(),
                    message: None,
                    outcome: crate::ai_hook::AiTurnOutcome::Failed,
                };
                banner_for_pane(window, cx, ToastKind::Info, "Pi failure", 7302, &failed);
                confirmation_for_pane(
                    window,
                    cx,
                    "Permission".into(),
                    7303,
                    Confirmation { choices: Vec::new(), id: 7304, question: "Continue?".into() },
                );
            });
            cx.run_until_parked();
            let original = ids(cx);
            assert_eq!(original.len(), 3);
            cx.background_executor.advance_clock(Duration::from_secs(4));
            cx.run_until_parked();
            assert_eq!(ids(cx), original, "completion remains readable before expiry");
            cx.update(|window, cx| {
                banner_for_pane(
                    window,
                    cx,
                    ToastKind::Info,
                    "Second Pi completion",
                    7301,
                    &pi_completion(),
                );
            });
            cx.run_until_parked();
            let later = *ids(cx).last().unwrap();
            cx.background_executor.advance_clock(Duration::from_secs(1));
            settle_dismissal(cx);
            assert_eq!(ids(cx), vec![original[1], original[2], later]);
            cx.background_executor.advance_clock(Duration::from_secs(4));
            settle_dismissal(cx);
            assert_eq!(ids(cx), original[1..]);
        }

        #[gpui::test]
        fn disabled_ai_cards_cannot_reenter_the_component_queue(cx: &mut TestAppContext) {
            initialize(cx, false);
            let (_, cx) = cx.add_window_view(|window, cx| Root::new(cx.new(|_| Empty), window, cx));
            cx.update(|window, cx| {
                banner_for_pane(
                    window,
                    cx,
                    ToastKind::Info,
                    "Hidden AI turn",
                    7102,
                    &pi_completion(),
                );
                confirmation_for_pane(
                    window,
                    cx,
                    "Hidden AI confirmation".into(),
                    7102,
                    Confirmation { choices: Vec::new(), id: 7202, question: "Continue?".into() },
                );
                banner_for_pane(
                    window,
                    cx,
                    ToastKind::Info,
                    "Build finished",
                    7102,
                    &crate::notify::Notification::Bell { program: None },
                );
            });
            cx.run_until_parked();
            assert_eq!(ids(cx).len(), 1, "ordinary terminal notices remain available");
        }

        #[gpui::test]
        fn deferred_startup_banner_rechecks_the_current_preference(cx: &mut TestAppContext) {
            initialize(cx, true);
            let (_, cx) = cx.add_window_view(|window, cx| {
                banner_for_pane(
                    window,
                    cx,
                    ToastKind::Info,
                    "Queued AI turn",
                    7103,
                    &pi_completion(),
                );
                cx.update_global::<Settings, _>(|settings, _| settings.ai_toasts = false);
                Root::new(cx.new(|_| Empty), window, cx)
            });
            settle_dismissal(cx);
            assert!(ids(cx).is_empty());
        }
    }
}
