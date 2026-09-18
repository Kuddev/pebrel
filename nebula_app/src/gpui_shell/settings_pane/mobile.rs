//! Native mobile pairing card. Network/persistence work belongs to
//! mobile_connection; this view owns form, QR visibility and operation feedback.

use super::*;
use crate::gpui_shell::{copy_feedback::CopyFeedback, widgets::NebulaSwitch};
use crate::{
    i18n::Message,
    mobile_connection::{self as connection, Failure, Mode, Status},
};
use pebrel_mobile_link::{endpoint::RelayAccess, preview::RelaySettings, qr::PairingQr};

mod advanced;
mod pairing_board;
#[cfg(test)]
mod tests;
mod view;

pub(super) struct MobileState {
    mode: Mode,
    addresses: Vec<connection::LanAddress>,
    address_select: SharedSelect,
    server_select: SharedSelect,
    server_hosts: Vec<(String, String)>,
    server_input: Entity<InputState>,
    server_loading: bool,
    server_request: u64,
    server_failure: bool,
    inputs: Vec<Entity<InputState>>,
    form_values: Vec<String>,
    relay_version: u32,
    edit_sequence: u64,
    allow_input: bool,
    advanced: bool,
    initialized: bool,
    loading_addresses: bool,
    operation: Option<u64>,
    failure: Option<Failure>,
    snapshot: Option<connection::Snapshot>,
    qr: Option<Arc<RenderImage>>,
    qr_side: f32,
    copy_feedback: Entity<CopyFeedback>,
    monitor: Option<Task<()>>,
    devices: Vec<connection::DeviceSummary>,
    devices_loading: bool,
    devices_failure: bool,
    revoke_pending: Option<String>,
    revoking: bool,
    shortcut_failure: bool,
    expires_at: Option<u64>,
    auto_prepare: bool,
    device_sequence: u64,
    server_needs_check: bool,
    loading_saved_relay: bool,
}

impl MobileState {
    pub(super) fn new(window: &mut Window, cx: &mut Context<SettingsPane>) -> Self {
        let address_select =
            cx.new(|cx| SelectState::new(Vec::<SharedString>::new(), None, window, cx));
        let server_select =
            cx.new(|cx| SelectState::new(Vec::<SharedString>::new(), None, window, cx));
        let server_input = cx.new(|cx| InputState::new(window, cx).placeholder("root@server"));
        let inputs = ["wss://", "", "", "", "0", ""]
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                cx.new(|cx| {
                    InputState::new(window, cx).default_value(value).masked(matches!(index, 2 | 3))
                })
            })
            .collect();
        Self {
            mode: Mode::Lan,
            addresses: Vec::new(),
            address_select,
            server_select,
            server_hosts: Vec::new(),
            server_input,
            server_loading: false,
            server_request: 0,
            server_failure: false,
            inputs,
            form_values: ["wss://", "", "", "", "0", ""].into_iter().map(str::to_owned).collect(),
            relay_version: 2,
            edit_sequence: 0,
            allow_input: false,
            advanced: false,
            initialized: false,
            loading_addresses: false,
            operation: None,
            failure: None,
            snapshot: None,
            qr: None,
            qr_side: 240.0,
            copy_feedback: cx.new(|_| CopyFeedback::new()),
            monitor: None,
            devices: Vec::new(),
            devices_loading: false,
            devices_failure: false,
            revoke_pending: None,
            revoking: false,
            shortcut_failure: false,
            expires_at: None,
            auto_prepare: true,
            device_sequence: 0,
            server_needs_check: false,
            loading_saved_relay: false,
        }
    }

    fn display_snapshot(&mut self, snapshot: Option<connection::Snapshot>) {
        if let Some(snapshot) = &snapshot {
            self.mode = snapshot.mode;
            self.allow_input = snapshot.allow_input;
        }
        let changed = self.snapshot.as_ref().map(|s| &s.invitation)
            != snapshot.as_ref().map(|s| &s.invitation);
        if changed {
            if let Some(value) = snapshot.as_ref().filter(|value| !value.invitation.is_empty()) {
                self.expires_at = serde_json::from_str::<serde_json::Value>(&value.invitation)
                    .ok()
                    .and_then(|value| value["secure"]["expiresAt"].as_u64());
            } else if snapshot.is_none() {
                self.expires_at = None;
            }
            self.qr = snapshot.as_ref().and_then(|snapshot| {
                if snapshot.invitation.is_empty() {
                    return None;
                }
                let qr = PairingQr::encode(snapshot.invitation.as_bytes()).ok()?;
                let scale = (240 / qr.width()).clamp(1, 4);
                self.qr_side = (qr.width() * scale) as f32;
                let side = (qr.width() * scale * 2) as u32;
                let pixels = image::RgbaImage::from_raw(side, side, qr.rgba(scale * 2).ok()?)?;
                Some(Arc::new(RenderImage::new([image::Frame::new(pixels)])))
            });
        }
        self.snapshot = snapshot;
    }
}

impl Drop for MobileState {
    fn drop(&mut self) {
        if let Some(generation) = self.operation {
            connection::cancel(generation);
        }
    }
}

impl SettingsPane {
    fn mobile_stop(&mut self, cx: &mut Context<Self>) {
        self.mobile.auto_prepare = false;
        self.mobile.edit_sequence = self.mobile.edit_sequence.wrapping_add(1);
        self.mobile_invalidate(cx);
    }

    fn mobile_relay_json(&self) -> Result<String, Failure> {
        if self.mobile.server_needs_check {
            return Err(Failure::Invalid);
        }
        let values = &self.mobile.form_values;
        let relay = if self.mobile.relay_version == 2 {
            serde_json::json!({"version":2,"url":values[0].trim(),"room":values[1].trim(),
                "desktopToken":values[2].trim(),"mobileToken":values[3].trim(),"tlsPin":values[5].trim()}).to_string()
        } else {
            serde_json::json!({"version":1,"url":values[0].trim(),"device":values[1].trim(),
                "desktopToken":values[2].trim(),"mobileToken":values[3].trim(),"name":"Pebrel PC"})
            .to_string()
        };
        let valid = if self.mobile.relay_version == 2 {
            RelayAccess::parse(relay.as_bytes()).is_ok()
        } else {
            RelaySettings::parse(relay.as_bytes()).is_ok()
        };
        if valid { Ok(relay) } else { Err(Failure::Invalid) }
    }

    fn mobile_relay_valid(&self) -> bool {
        self.mobile_relay_json().is_ok()
    }

    fn mobile_generate_if_ready(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mobile.auto_prepare = true;
        let ready = match self.mobile.mode {
            Mode::Lan => !self.mobile.addresses.is_empty(),
            Mode::Relay => self.mobile_relay_valid(),
        };
        if ready && self.mobile.failure.is_none() {
            self.mobile_generate(window, cx);
        } else {
            cx.notify();
        }
    }

    fn mobile_refresh_devices(&mut self, cx: &mut Context<Self>) {
        if self.mobile.devices_loading || self.mobile.revoking {
            return;
        }
        self.mobile.devices_loading = true;
        let sequence = self.mobile.device_sequence;
        let load = cx.background_executor().spawn(async { connection::paired_devices() });
        cx.spawn(async move |this, cx| {
            let result = load.await;
            let _ = this.update(cx, |this, cx| {
                if sequence != this.mobile.device_sequence {
                    return;
                }
                this.mobile.devices_loading = false;
                match result {
                    Ok(devices) => {
                        this.mobile.devices = devices;
                        this.mobile.devices_failure = false;
                    },
                    Err(_) => this.mobile.devices_failure = true,
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn mobile_revoke_device(&mut self, id: String, cx: &mut Context<Self>) {
        if self.mobile.revoking {
            return;
        }
        self.mobile.revoking = true;
        self.mobile.device_sequence = self.mobile.device_sequence.wrapping_add(1);
        self.mobile.devices_loading = false;
        let target = id.clone();
        let operation =
            cx.background_executor().spawn(async move { connection::revoke_device(&target) });
        cx.spawn(async move |this, cx| {
            let result = operation.await;
            let _ = this.update(cx, |this, cx| {
                this.mobile.revoking = false;
                match result {
                    Ok(_) => {
                        this.mobile.devices.retain(|device| device.id != id);
                        this.mobile.revoke_pending = None;
                        this.mobile_refresh_devices(cx);
                    },
                    Err(_) => this.mobile.devices_failure = true,
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn mobile_invalidate(&mut self, cx: &mut Context<Self>) {
        self.mobile.server_request = self.mobile.server_request.wrapping_add(1);
        self.mobile.server_loading = false;
        self.mobile.server_failure = false;
        if self.mobile.snapshot.is_some() || self.mobile.operation.is_some() {
            connection::stop();
        }
        self.mobile.operation = None;
        self.mobile.display_snapshot(None);
        self.mobile.failure = None;
        self.mobile.copy_feedback.update(cx, |state, cx| state.clear(cx));
        cx.notify();
    }

    fn mobile_initialize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mobile.initialized {
            return;
        }
        self.mobile.initialized = true;
        self.mobile.loading_saved_relay = true;
        if let Some(snapshot) = connection::snapshot() {
            self.mobile.mode = snapshot.mode;
            self.mobile.display_snapshot(Some(snapshot));
        }
        let select = self.mobile.address_select.clone();
        self._subscriptions.push(cx.subscribe_in(&select, window, |this, _, event, window, cx| {
            if matches!(event, SelectEvent::Confirm(_)) {
                this.mobile_invalidate(cx);
                this.mobile_generate_if_ready(window, cx);
            }
        }));
        for input in self.mobile.inputs.clone() {
            self._subscriptions.push(cx.subscribe_in(&input, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let values: Vec<_> = this
                        .mobile
                        .inputs
                        .iter()
                        .map(|input| input.read(cx).value().to_string())
                        .collect();
                    if values != this.mobile.form_values {
                        this.mobile.form_values = values;
                        this.mobile.edit_sequence = this.mobile.edit_sequence.wrapping_add(1);
                        this.mobile_invalidate(cx);
                    }
                }
            }));
        }
        let feedback = self.mobile.copy_feedback.clone();
        self._subscriptions.push(cx.observe(&feedback, |_, _, cx| cx.notify()));
        self.mobile_refresh_addresses(window, cx);
        self.mobile_refresh_servers(window, cx);
        self.mobile_refresh_devices(cx);
        let server_input = self.mobile.server_input.clone();
        self._subscriptions.push(cx.subscribe_in(
            &server_input,
            window,
            |this, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.mobile.edit_sequence = this.mobile.edit_sequence.wrapping_add(1);
                    this.mobile.server_needs_check = true;
                    if this.mobile.mode == Mode::Relay {
                        this.mobile_invalidate(cx);
                    }
                }
            },
        ));
        let server_select = self.mobile.server_select.clone();
        self._subscriptions.push(cx.subscribe_in(
            &server_select,
            window,
            |this, _, event, window, cx| {
                if matches!(event, SelectEvent::Confirm(_)) {
                    if let Some(destination) = this
                        .mobile
                        .server_select
                        .read(cx)
                        .selected_index(cx)
                        .and_then(|index| this.mobile.server_hosts.get(index.row))
                        .map(|host| host.0.clone())
                    {
                        this.mobile
                            .server_input
                            .update(cx, |state, cx| state.set_value(destination, window, cx));
                    }
                }
            },
        ));
        let load = cx.background_executor().spawn(async { connection::saved_relay() });
        cx.spawn_in(window, async move |this, cx| {
            let saved = load.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.mobile.loading_saved_relay = false;
                if let Ok(Some(saved)) = saved {
                    if this.mobile.edit_sequence == 0 {
                        this.mobile_import(&saved, window, cx);
                    }
                }
                // Initial address discovery and credential loading may finish in
                // either order. Wait for both before auto-preparing the LAN QR.
                if this.mobile.auto_prepare
                    && this.mobile.snapshot.is_none()
                    && this.mobile.operation.is_none()
                {
                    this.mobile_generate_if_ready(window, cx);
                }
                cx.notify();
            });
        })
        .detach();
        self.mobile.monitor = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this
                    .update(cx, |this, cx| {
                        let snapshot = connection::snapshot();
                        if snapshot != this.mobile.snapshot {
                            this.mobile.display_snapshot(snapshot);
                        }
                        if this.active_section == 10 {
                            this.mobile_refresh_devices(cx);
                            // Countdown and device presence are live while this page is visible.
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

    fn mobile_refresh_addresses(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mobile.loading_addresses {
            return;
        }
        self.mobile.loading_addresses = true;
        let previous = self
            .mobile
            .address_select
            .read(cx)
            .selected_index(cx)
            .and_then(|index| self.mobile.addresses.get(index.row))
            .map(|entry| entry.address)
            .or_else(|| self.mobile.snapshot.as_ref().and_then(|snapshot| snapshot.address));
        let task = cx.background_executor().spawn(async { connection::addresses() });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.mobile.loading_addresses = false;
                match result {
                    Ok(addresses) => {
                        let selected = previous
                            .and_then(|ip| addresses.iter().position(|entry| entry.address == ip))
                            .or_else(|| (!addresses.is_empty()).then_some(0));
                        let items = addresses
                            .iter()
                            .map(|entry| {
                                SharedString::from(format!("{} ({})", entry.address, entry.name))
                            })
                            .collect();
                        this.mobile.addresses = addresses;
                        this.mobile.address_select.update(cx, |state, cx| {
                            state.set_items(items, window, cx);
                            state.set_selected_index(
                                selected.map(|row| IndexPath::default().row(row)),
                                window,
                                cx,
                            );
                        });
                        if previous.is_some()
                            && !this
                                .mobile
                                .addresses
                                .iter()
                                .any(|entry| Some(entry.address) == previous)
                        {
                            this.mobile_invalidate(cx);
                        }
                        if this.mobile.mode == Mode::Lan
                            && !this.mobile.loading_saved_relay
                            && this.mobile.auto_prepare
                            && this.mobile.snapshot.is_none()
                            && this.mobile.operation.is_none()
                        {
                            this.mobile_generate_if_ready(window, cx);
                        }
                    },
                    Err(_) => this.mobile.failure = Some(Failure::Address),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn mobile_import(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let version = serde_json::from_str::<serde_json::Value>(text)
            .ok()
            .and_then(|v| v["version"].as_u64());
        if version == Some(2) {
            match RelayAccess::parse(text.as_bytes()) {
                Ok(access) => {
                    self.mobile.server_needs_check = false;
                    self.mobile.relay_version = 2;
                    for (index, value) in [
                        (0, &access.url),
                        (1, &access.room),
                        (2, &access.desktop_token),
                        (3, &access.mobile_token),
                        (5, &access.tls_pin),
                    ] {
                        self.mobile.form_values[index] = value.clone();
                        self.mobile.inputs[index]
                            .update(cx, |input, cx| input.set_value(value.clone(), window, cx));
                    }
                    self.mobile.failure = None;
                },
                Err(_) => {
                    self.mobile.failure = Some(Failure::Invalid);
                    self.mobile.advanced = true;
                },
            }
            cx.notify();
            return;
        }
        match RelaySettings::parse(text.as_bytes()) {
            Ok(settings) => {
                self.mobile.server_needs_check = false;
                self.mobile.relay_version = 1;
                for (index, (input, value)) in self
                    .mobile
                    .inputs
                    .iter()
                    .zip([
                        settings.url,
                        settings.device,
                        settings.desktop_token,
                        settings.mobile_token,
                    ])
                    .enumerate()
                {
                    self.mobile.form_values[index] = value.clone();
                    input.update(cx, |input, cx| input.set_value(value, window, cx));
                }
                self.mobile.failure = None;
            },
            Err(_) => {
                self.mobile.failure = Some(Failure::Invalid);
                self.mobile.advanced = true;
            },
        }
        cx.notify();
    }

    fn mobile_generate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mobile.operation.is_some() || self.mobile.server_loading {
            return;
        }
        // A late credential load must not overwrite a form already submitted
        // by the user, including an invalid submission that needs correction.
        self.mobile.edit_sequence = self.mobile.edit_sequence.wrapping_add(1);
        let mode = self.mobile.mode;
        let address = self
            .mobile
            .address_select
            .read(cx)
            .selected_index(cx)
            .and_then(|index| self.mobile.addresses.get(index.row))
            .map(|entry| entry.address);
        let Ok(port) = self.mobile.inputs[4].read(cx).value().trim().parse::<u16>() else {
            self.mobile.failure = Some(Failure::Invalid);
            self.mobile.advanced = true;
            cx.notify();
            return;
        };
        let relay = match self.mobile_relay_json() {
            Ok(relay) => relay,
            Err(_) if mode == Mode::Lan => String::new(),
            Err(error) => {
                self.mobile.failure = Some(error);
                self.mobile.advanced = true;
                cx.notify();
                return;
            },
        };
        self.mobile_invalidate(cx);
        let generation = connection::begin();
        self.mobile.operation = Some(generation);
        let allow_input = self.mobile.allow_input;
        let task = cx.background_executor().spawn(async move {
            connection::start(generation, mode, address, port, allow_input, relay)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if this.mobile.operation != Some(generation) {
                    return;
                }
                this.mobile.operation = None;
                match result {
                    Ok(snapshot) => this.mobile.display_snapshot(Some(snapshot)),
                    Err(Failure::Cancelled) => {},
                    Err(error) => this.mobile.failure = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn mobile_refresh_servers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let load = cx.background_executor().spawn(async { connection::relay_hosts() });
        cx.spawn_in(window, async move |this, cx| {
            let hosts = load.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if let Ok(hosts) = hosts {
                    let labels = hosts
                        .iter()
                        .map(|host| SharedString::from(format!("{} ({})", host.1, host.0)))
                        .collect();
                    this.mobile.server_hosts = hosts;
                    this.mobile
                        .server_select
                        .update(cx, |state, cx| state.set_items(labels, window, cx));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn mobile_use_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mobile.server_loading || self.mobile.operation.is_some() {
            return;
        }
        let destination = self.mobile.server_input.read(cx).value().trim().to_owned();
        if destination.is_empty() {
            return;
        }
        self.mobile_invalidate(cx);
        self.mobile.edit_sequence = self.mobile.edit_sequence.wrapping_add(1);
        let sequence = self.mobile.edit_sequence;
        let request = self.mobile.server_request;
        self.mobile.server_loading = true;
        self.mobile.server_failure = false;
        let load =
            cx.background_executor().spawn(async move { connection::relay_from_ssh(&destination) });
        cx.spawn_in(window, async move |this, cx| {
            let result = load.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.mobile.server_request != request {
                    return;
                }
                this.mobile.server_loading = false;
                if this.mobile.edit_sequence != sequence || this.mobile.mode != Mode::Relay {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(data) => {
                        this.mobile_import(&data, window, cx);
                        this.mobile_generate_if_ready(window, cx);
                    },
                    Err(_) => this.mobile.server_failure = true,
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn mobile_import_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(
                crate::gpui_shell::config::ui_language(cx).text(Message::MobileImportFile).into(),
            ),
        });
        let sequence = self.mobile.edit_sequence;
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let read = cx
                .background_executor()
                .spawn(async move {
                    use std::io::Read;
                    let mut data = String::new();
                    std::fs::File::open(path)?.take(8193).read_to_string(&mut data)?;
                    if data.len() > 8192 {
                        return Err(std::io::Error::other("configuration_too_large"));
                    }
                    Ok::<_, std::io::Error>(data)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if sequence != this.mobile.edit_sequence || this.mobile.operation.is_some() {
                    return;
                }
                this.mobile_invalidate(cx);
                this.mobile.edit_sequence = this.mobile.edit_sequence.wrapping_add(1);
                match read {
                    Ok(data) => {
                        this.mobile_import(&data, window, cx);
                        this.mobile_generate_if_ready(window, cx);
                    },
                    Err(_) => {
                        this.mobile.failure = Some(Failure::Invalid);
                        cx.notify();
                    },
                }
            });
        })
        .detach();
    }
}
