//! HTML 三态的 UI 所有者；配对、权限和监听生命周期只调用 mobile_connection。

use super::*;
use crate::{
    gpui_shell::copy_feedback::CopyFeedback,
    i18n::Message,
    mobile_connection::{self as connection, Failure, Mode, Preferences, Snapshot, Status},
};
use pebrel_mobile_link::{endpoint::RelayAccess, qr::PairingQr};

mod relay;
#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;
mod view;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Off,
    Pairing,
    Paired,
}

pub(super) struct MobileState {
    initialized: bool,
    loading: bool,
    syncing: bool,
    snapshot: Option<Snapshot>,
    mode: Mode,
    pairing_open: bool,
    pairing_baseline: Vec<String>,
    operation: bool,
    sequence: u64,
    generation: Option<u64>,
    failure: Option<Failure>,
    addresses: Vec<connection::LanAddress>,
    address_select: SharedSelect,
    port_input: Entity<InputState>,
    port_placeholder: String,
    port_focus: view::PortFocus,
    _port_focus_subscription: Subscription,
    qr_payload: Option<String>,
    qr: Option<Arc<RenderImage>>,
    expires_at: Option<u64>,
    copy_feedback: Entity<CopyFeedback>,
    monitor: Option<Task<()>>,
    relay_open: bool,
    relay_focus: FocusHandle,
    relay_inputs: Vec<Entity<InputState>>,
    relay_edit: u64,
    relay_loading: bool,
    relay_result: Option<Message>,
    saved_relay: Option<String>,
    server_select: SharedSelect,
    server_hosts: Vec<(String, String)>,
}

impl MobileState {
    pub(super) fn new(window: &mut Window, cx: &mut Context<SettingsPane>) -> Self {
        let port_input = cx.new(|cx| {
            InputState::new(window, cx)
                .pattern(regex::Regex::new(r"^\d{0,5}$").expect("port pattern"))
        });
        let port_focus_subscription = cx.subscribe(&port_input, |this, _, event, cx| {
            let focused = match event {
                InputEvent::Focus => true,
                InputEvent::Blur => false,
                _ => return,
            };
            this.mobile.port_focus.set_focused(focused);
            cx.notify();
        });
        Self {
            initialized: false,
            loading: false,
            syncing: false,
            snapshot: None,
            mode: Mode::Lan,
            pairing_open: false,
            pairing_baseline: Vec::new(),
            operation: false,
            sequence: 0,
            generation: None,
            failure: None,
            addresses: Vec::new(),
            address_select: cx
                .new(|cx| SelectState::new(Vec::<SharedString>::new(), None, window, cx)),
            port_input,
            port_placeholder: String::new(),
            port_focus: view::PortFocus::default(),
            _port_focus_subscription: port_focus_subscription,
            qr_payload: None,
            qr: None,
            expires_at: None,
            copy_feedback: cx.new(|_| CopyFeedback::new()),
            monitor: None,
            relay_open: false,
            relay_focus: cx.focus_handle(),
            relay_inputs: (0..5)
                .map(|i| cx.new(|cx| InputState::new(window, cx).masked(matches!(i, 2 | 3))))
                .collect(),
            relay_edit: 0,
            relay_loading: false,
            relay_result: None,
            saved_relay: None,
            server_select: cx
                .new(|cx| SelectState::new(Vec::<SharedString>::new(), None, window, cx)),
            server_hosts: Vec::new(),
        }
    }

    pub(super) fn phase(&self) -> Phase {
        if !self.snapshot.as_ref().is_some_and(|s| s.preferences.enabled) {
            Phase::Off
        } else if self.pairing_open || self.snapshot.as_ref().is_none_or(|s| s.devices.is_empty()) {
            Phase::Pairing
        } else {
            Phase::Paired
        }
    }

    fn preferences(&self) -> Preferences {
        self.snapshot.as_ref().map(|s| s.preferences.clone()).unwrap_or_default()
    }

    fn display_snapshot(&mut self, snapshot: Snapshot) {
        if self.pairing_open
            && snapshot
                .devices
                .iter()
                .any(|d| d.connected && !self.pairing_baseline.contains(&d.id))
        {
            self.pairing_open = false;
        }
        let payload = snapshot.connection(self.mode).and_then(|c| c.invitation.clone());
        if self.qr_payload != payload {
            self.expires_at = payload
                .as_deref()
                .and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok())
                .and_then(|v| v["secure"]["expiresAt"].as_u64());
            self.qr = payload.as_deref().and_then(|payload| {
                let qr = PairingQr::encode(payload.as_bytes()).ok()?;
                let scale = (512 / qr.width()).clamp(1, 8);
                let side = (qr.width() * scale) as u32;
                let pixels = image::RgbaImage::from_raw(side, side, qr.rgba(scale).ok()?)?;
                Some(Arc::new(RenderImage::new([image::Frame::new(pixels)])))
            });
            self.qr_payload = payload;
        }
        self.snapshot = Some(snapshot);
    }

    fn valid_qr(&self) -> bool {
        // 切换监听地址时旧邀请已经失效，完成事务前不再展示或复制旧二维码。
        !self.operation
            && self.qr.is_some()
            && self.expires_at.is_some_and(|until| until > now())
            && self
                .snapshot
                .as_ref()
                .and_then(|s| s.connection(self.mode))
                .is_some_and(|c| matches!(c.status, Status::Waiting | Status::Connected))
    }
}

impl Drop for MobileState {
    fn drop(&mut self) {
        if let Some(generation) = self.generation {
            connection::cancel(generation);
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_secs())
        .unwrap_or(0)
}

impl SettingsPane {
    fn mobile_sync_port_placeholder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = crate::gpui_shell::config::ui_language(cx);
        let placeholder = self
            .mobile
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.lan.as_ref())
            .filter(|connection| matches!(connection.status, Status::Waiting | Status::Connected))
            .and_then(|connection| connection.address.strip_prefix("wss://"))
            .and_then(|address| address.parse::<std::net::SocketAddr>().ok())
            .map(|address| address.port().to_string())
            .unwrap_or_else(|| language.text(Message::MobileAutomatic).to_owned());
        if self.mobile.port_placeholder != placeholder {
            // 自动分配的实际端口只是显示提示；不把聚焦、失焦误保存为固定端口。
            self.mobile.port_placeholder = placeholder.clone();
            self.mobile
                .port_input
                .update(cx, |input, cx| input.set_placeholder(placeholder, window, cx));
        }
    }

    fn mobile_initialize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mobile.initialized {
            return;
        }
        self.mobile.initialized = true;
        self.mobile.loading = true;
        let feedback = self.mobile.copy_feedback.clone();
        self._subscriptions.push(cx.observe(&feedback, |_, _, cx| cx.notify()));
        let select = self.mobile.address_select.clone();
        self._subscriptions.push(cx.subscribe_in(&select, window, |this, _, event, window, cx| {
            if matches!(event, SelectEvent::Confirm(_))
                && !this.mobile.syncing
                && !this.mobile.operation
            {
                let mut preferences = this.mobile.preferences();
                preferences.address = this.mobile_selected_address(cx).or(preferences.address);
                if preferences.enabled && preferences.lan_enabled {
                    this.mobile_apply(preferences, None, Some(Mode::Lan), false, window, cx);
                }
            }
        }));
        let input = self.mobile.port_input.clone();
        self._subscriptions.push(cx.subscribe_in(&input, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. })
                && !this.mobile.syncing
                && !this.mobile.operation
            {
                let value = this.mobile.port_input.read(cx).value();
                let port =
                    if value.trim().is_empty() { Ok(0) } else { value.trim().parse::<u16>() };
                match port {
                    Ok(port) if port != this.mobile.preferences().port => {
                        let mut preferences = this.mobile.preferences();
                        preferences.port = port;
                        this.mobile_apply(preferences, None, Some(Mode::Lan), false, window, cx);
                    },
                    Err(_) => {
                        this.mobile.failure = Some(Failure::Invalid);
                        cx.notify();
                    },
                    _ => {},
                }
            }
        }));
        for input in self.mobile.relay_inputs.clone() {
            self._subscriptions.push(cx.subscribe(&input, |this, _, event, _| {
                if matches!(event, InputEvent::Change) && !this.mobile.syncing {
                    this.mobile.relay_edit = this.mobile.relay_edit.wrapping_add(1);
                    this.mobile.relay_result = None;
                }
            }));
        }
        let load = cx.background_executor().spawn(async {
            (connection::snapshot(), connection::addresses(), connection::saved_relay())
        });
        cx.spawn_in(window, async move |this, cx| {
            let (snapshot, addresses, relay) = load.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.mobile.loading = false;
                match snapshot {
                    Ok(snapshot) => this.mobile.display_snapshot(snapshot),
                    Err(error) => this.mobile.failure = Some(error),
                }
                if let Ok(addresses) = addresses {
                    this.mobile_set_addresses(addresses, window, cx);
                }
                if let Ok(relay) = relay {
                    this.mobile.saved_relay = relay;
                }
                this.mobile_sync_servers(window, cx);
                let port = this.mobile.preferences().port;
                this.mobile.syncing = true;
                this.mobile.port_input.update(cx, |input, cx| {
                    input.set_value(
                        if port == 0 { String::new() } else { port.to_string() },
                        window,
                        cx,
                    )
                });
                this.mobile.syncing = false;
                cx.notify();
            });
        })
        .detach();
        self.mobile.monitor = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let Ok(sequence) = this.update(cx, |this, _| {
                    (!this.mobile.operation && !this.mobile.loading).then_some(this.mobile.sequence)
                }) else {
                    break;
                };
                let Some(sequence) = sequence else { continue };
                let result = cx.background_executor().spawn(async { connection::snapshot() }).await;
                if this
                    .update(cx, |this, cx| {
                        if this.mobile.sequence != sequence || this.mobile.operation {
                            return;
                        }
                        if let Ok(snapshot) = result {
                            this.mobile.display_snapshot(snapshot);
                        }
                        if this.active_section == MOBILE_SECTION {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    fn mobile_run(
        &mut self,
        generation: Option<u64>,
        close_relay: bool,
        work: impl FnOnce() -> Result<Snapshot, Failure> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(previous) = self.mobile.generation.take() {
            connection::cancel(previous);
        }
        self.mobile.sequence = self.mobile.sequence.wrapping_add(1);
        let sequence = self.mobile.sequence;
        self.mobile.generation = generation;
        self.mobile.operation = true;
        self.mobile.failure = None;
        let task = cx.background_executor().spawn(async move { work() });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.mobile.sequence != sequence {
                    return;
                }
                this.mobile.operation = false;
                this.mobile.generation = None;
                match result {
                    Ok(snapshot) => {
                        this.mobile.display_snapshot(snapshot);
                        if close_relay {
                            this.mobile.saved_relay = this.mobile_relay_json(cx).ok();
                            this.mobile.relay_open = false;
                            window.focus(&this.focus_handle, cx);
                        }
                    },
                    Err(Failure::Cancelled) => {},
                    Err(error) => {
                        this.mobile.failure = Some(error);
                        // 事务回滚后显示已提交的网卡，不能让草稿地址看起来已经生效。
                        if let Some(snapshot) = this.mobile.snapshot.as_ref() {
                            let selected = snapshot.preferences.address.and_then(|address| {
                                this.mobile
                                    .addresses
                                    .iter()
                                    .position(|entry| entry.address == address)
                            });
                            this.mobile.syncing = true;
                            this.mobile.address_select.update(cx, |select, cx| {
                                select.set_selected_index(
                                    selected.map(|row| IndexPath::default().row(row)),
                                    window,
                                    cx,
                                );
                            });
                            this.mobile.syncing = false;
                        }
                    },
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn mobile_apply(
        &mut self,
        preferences: Preferences,
        relay: Option<String>,
        route: Option<Mode>,
        close_relay: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let generation = connection::begin();
        self.mobile_run(
            Some(generation),
            close_relay,
            move || connection::apply(generation, preferences, relay, route),
            window,
            cx,
        );
    }

    fn mobile_selected_address(&self, cx: &App) -> Option<std::net::IpAddr> {
        self.mobile
            .address_select
            .read(cx)
            .selected_index(cx)
            .and_then(|i| self.mobile.addresses.get(i.row))
            .map(|a| a.address)
    }

    fn mobile_enable(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut preferences = self.mobile.preferences();
        preferences.enabled = true;
        preferences.address = self.mobile_selected_address(cx).or(preferences.address);
        if !preferences.lan_enabled && !preferences.relay_enabled {
            preferences.lan_enabled = true;
        }
        self.mobile.pairing_open =
            self.mobile.snapshot.as_ref().is_none_or(|s| s.devices.is_empty());
        self.mobile.pairing_baseline = self
            .mobile
            .snapshot
            .as_ref()
            .map(|s| s.devices.iter().map(|d| d.id.clone()).collect())
            .unwrap_or_default();
        self.mobile_apply(preferences, None, None, false, window, cx);
    }

    fn mobile_pause(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut preferences = self.mobile.preferences();
        preferences.enabled = false;
        self.mobile.pairing_open = false;
        self.mobile_apply(preferences, None, None, false, window, cx);
    }

    fn mobile_pair(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mobile.pairing_open = true;
        self.mobile.pairing_baseline = self
            .mobile
            .snapshot
            .as_ref()
            .map(|s| s.devices.iter().map(|d| d.id.clone()).collect())
            .unwrap_or_default();
        let mode = self.mobile.mode;
        if self.mobile.snapshot.as_ref().and_then(|s| s.connection(mode)).is_some() {
            self.mobile_run(None, false, move || connection::refresh_invitation(mode), window, cx);
        } else {
            self.mobile_mode(mode, window, cx);
        }
    }

    fn mobile_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.mobile.mode = mode;
        if let Some(snapshot) = self.mobile.snapshot.clone() {
            self.mobile.display_snapshot(snapshot);
        }
        if self.mobile.snapshot.as_ref().and_then(|s| s.connection(mode)).is_none() {
            if mode == Mode::Relay && self.mobile.saved_relay.is_none() {
                self.mobile_open_relay(window, cx);
                return;
            }
            let mut preferences = self.mobile.preferences();
            preferences.enabled = true;
            match mode {
                Mode::Lan => {
                    preferences.lan_enabled = true;
                    preferences.address = self.mobile_selected_address(cx).or(preferences.address);
                },
                Mode::Relay => preferences.relay_enabled = true,
            }
            self.mobile_apply(preferences, None, Some(mode), false, window, cx);
        } else {
            cx.notify();
        }
    }

    fn mobile_set_addresses(
        &mut self,
        addresses: Vec<connection::LanAddress>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous = self.mobile_selected_address(cx).or(self.mobile.preferences().address);
        let selected = connection::available_address(previous, &addresses)
            .and_then(|ip| addresses.iter().position(|a| a.address == ip));
        let labels = addresses
            .iter()
            .map(|a| SharedString::from(format!("{} · {}", a.name, a.address)))
            .collect();
        self.mobile.addresses = addresses;
        self.mobile.syncing = true;
        self.mobile.address_select.update(cx, |select, cx| {
            select.set_items(labels, window, cx);
            select.set_selected_index(
                selected.map(|row| IndexPath::default().row(row)),
                window,
                cx,
            );
        });
        self.mobile.syncing = false;
    }

    fn mobile_refresh_addresses(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mobile.loading {
            return;
        }
        self.mobile.loading = true;
        let task = cx.background_executor().spawn(async { connection::addresses() });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.mobile.loading = false;
                match result {
                    Ok(addresses) => {
                        this.mobile_set_addresses(addresses, window, cx);
                    },
                    Err(_) => this.mobile.failure = Some(Failure::Address),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn mobile_copy(&mut self, cx: &mut Context<Self>) {
        if !self.mobile.valid_qr() {
            return;
        }
        if let Some(payload) = self.mobile.qr_payload.clone() {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(payload));
            self.mobile.copy_feedback.update(cx, |feedback, cx| feedback.mark_copied(cx));
        }
    }

    fn mobile_remove(
        &mut self,
        id: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = crate::gpui_shell::config::ui_language(cx);
        let owner = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, _| {
            let owner = owner.clone();
            let id = id.clone();
            confirm_dialog(
                dialog,
                window,
                language.format(Message::MobileRemoveTitle, &[("name", &name)]),
                language.text(Message::MobileRemoveDescription),
                language.text(Message::MobileRemove),
                language.text(Message::CommonCancel),
                ButtonVariant::Danger,
            )
            .on_ok(move |_, window, cx| {
                let id = id.clone();
                let _ = owner.update(cx, |this, cx| {
                    this.mobile_run(None, false, move || connection::revoke_device(&id), window, cx)
                });
                true
            })
        });
    }
}
