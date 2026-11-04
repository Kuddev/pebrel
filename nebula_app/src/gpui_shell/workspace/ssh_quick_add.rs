//! 工作区内的 SSH 快速添加浮层（#864）。
//!
//! # 为什么不是导航到设置页
//!
//! 启动器里输入一个还没保存的地址时，用户意图是「连上它」，不是「去配置」。
//! 把人送进设置页再送回来，等于用一个页面切换换一次表单填写。所以这里把
//! 表单浮在终端上，保存后直接开标签连接。
//!
//! # 为什么不复制一份完整编辑器
//!
//! 设置页的编辑器要承载代理、跳板、私钥、图标、测试连接；那是主机库的管理
//! 面。快捷连接只需要「账号 + 地址 + 端口 + 认证方式 + 密码」这几项。这里
//! 只做子集，但**校验、拼接、落盘和凭据全部复用既有权威**：
//! `validate_ssh_destination`、`join_destination_*`、`SshProfiles::upsert/save`、
//! `ssh_credentials::store_password`。复杂配置仍走设置页（表单里的
//! 「更多设置」按钮），两边写的是同一份 `ssh_profiles.json`。

use gpui::{AppContext as _, Entity, Focusable as _, IntoElement, Window};
use gpui::{InteractiveElement as _, ParentElement as _, Styled as _, div, px};

use crate::gpui_shell::prelude::*;
use crate::gpui_shell::ssh_settings::editor::{editor_field, editor_hint, editor_input};
use crate::i18n::Message;

use super::NebulaWorkspace;

const QUICK_ADD_WIDTH: f32 = 460.0;

/// 身份验证分段选择与设置页完全同款、同序（密码/密钥/自动/交互式），
/// 字段顺序也与其基础表单一致。差异只有一点：密钥文件需要文件选择器，
/// 浮层不收集私钥，保存时引导到完整编辑器。
pub(super) struct SshQuickAddState {
    username: Entity<InputState>,
    address: Entity<InputState>,
    port: Entity<InputState>,
    password: Entity<InputState>,
    label: Entity<InputState>,
    auth: crate::ssh_profiles::SshAuthMode,
    error: Option<String>,
}

/// 每个字段都是独立 `InputState`；统一走这里，掩码与占位符口径一致。
fn quick_input(
    value: &str,
    placeholder: &'static str,
    masked: bool,
    window: &mut Window,
    cx: &mut gpui::App,
) -> Entity<InputState> {
    let entity = cx.new(|cx| {
        let mut state = InputState::new(window, cx).placeholder(placeholder);
        if masked {
            state = state.masked(true);
        }
        state
    });
    entity.update(cx, |state, cx| {
        state.set_value(value.to_owned(), window, cx);
        cx.notify();
    });
    entity
}

impl SshQuickAddState {
    fn new(destination: &str, window: &mut Window, cx: &mut gpui::App) -> Self {
        let (address_with_user, port) = crate::display::split_destination_port(destination);
        let (username, address) = crate::display::split_destination_user(&address_with_user);
        let language = crate::gpui_shell::config::ui_language(cx);
        let username = if username.is_empty() { "root".to_owned() } else { username };
        let username = quick_input(&username, "", false, window, cx);
        let address = quick_input(&address, "", false, window, cx);
        let port = quick_input(&port, "", false, window, cx);
        let password = quick_input(
            "",
            language.pick("留空则连接时询问", "Leave empty to ask when connecting"),
            true,
            window,
            cx,
        );
        let label = quick_input("", "", false, window, cx);
        Self {
            username,
            address,
            port,
            password,
            label,
            auth: crate::ssh_profiles::SshAuthMode::Auto,
            error: None,
        }
    }

    /// 组合成最终 destination：用户名与端口填过就以输入为准，否则沿用地址串
    /// 里自带的 `user@` / `:port`，最后交给 `validate_ssh_destination` 裁决。
    fn draft_destination(&self, cx: &gpui::App) -> Result<String, QuickAddError> {
        let address = self.address.read(cx).value().trim().to_owned();
        if address.is_empty() {
            return Err(QuickAddError::MissingAddress);
        }
        let (inline_user, host) = crate::display::split_destination_user(&address);
        let (host, inline_port) = crate::display::split_destination_port(&host);
        let typed_port = self.port.read(cx).value().trim().to_owned();
        let port = if !typed_port.is_empty() {
            if !typed_port.parse::<u16>().is_ok_and(|value| value > 0) {
                return Err(QuickAddError::InvalidPort);
            }
            typed_port
        } else {
            inline_port
        };
        let typed_user = self.username.read(cx).value().trim().to_owned();
        let user = if !inline_user.is_empty() { inline_user } else { typed_user };
        let destination = crate::display::join_destination_port(
            &crate::display::join_destination_user(&user, &host),
            &port,
        );
        crate::ssh_profiles::validate_ssh_destination(&destination)
            .map_err(QuickAddError::Rejected)?;
        Ok(destination)
    }
}

#[derive(Debug, PartialEq)]
pub(super) enum QuickAddError {
    MissingAddress,
    InvalidPort,
    Rejected(String),
    Io(String),
    KeysNeedAdvancedEditor,
}

/// 落盘一次快捷添加。独立函数让测试能指向临时文件，避免触碰真实主机库。
fn persist_quick_add(
    state: &SshQuickAddState,
    path: &std::path::Path,
    cx: &gpui::App,
) -> Result<String, QuickAddError> {
    let destination = state.draft_destination(cx)?;
    let auth = state.auth;
    if auth == crate::ssh_profiles::SshAuthMode::PublicKey {
        // 浮层没有私钥文件选择器：空私钥列表的公钥 profile 一定连不上，
        // 直接引导到完整编辑器，而不是写进一份坏数据。
        return Err(QuickAddError::KeysNeedAdvancedEditor);
    }
    let password = state.password.read(cx).value().to_string();
    let label = {
        let label = state.label.read(cx).value().trim().to_owned();
        (!label.is_empty()).then_some(label)
    };
    let mut profiles = crate::ssh_profiles::SshProfiles::load(path)
        .map_err(|error| QuickAddError::Io(error.to_string()))?;
    profiles.upsert(crate::ssh_profiles::SshProfileAuth {
        destination: destination.clone(),
        auth,
        private_keys: Vec::new(),
        label,
        icon: None,
        connection: crate::ssh_profiles::SshConnectionOptions::default(),
    });
    profiles.save(path).map_err(|error| QuickAddError::Io(error.to_string()))?;
    if auth == crate::ssh_profiles::SshAuthMode::Password
        && !password.is_empty()
        && let Err(error) =
            crate::ssh_credentials::store_password(&destination, password.as_bytes())
    {
        return Err(QuickAddError::Io(error.to_string()));
    }
    Ok(destination)
}

impl NebulaWorkspace {
    pub(super) fn open_ssh_quick_add(
        &mut self,
        destination: String,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let state = SshQuickAddState::new(&destination, window, cx);
        let address_focus = state.address.read(cx).focus_handle(cx);
        self.ssh_quick_add = Some(state);
        window.focus(&address_focus, cx);
        cx.notify();
    }

    pub(super) fn close_ssh_quick_add(&mut self, cx: &mut gpui::Context<Self>) {
        if self.ssh_quick_add.take().is_some() {
            cx.notify();
        }
    }

    /// 把浮层里的草稿写进主机库并保存密码；成功返回规范化 destination。
    /// 与连接分开，测试可以只验证落盘而不真的建 SSH 会话。
    pub(super) fn save_quick_add_host(&self, cx: &gpui::App) -> Result<String, QuickAddError> {
        let state = self.ssh_quick_add.as_ref().ok_or(QuickAddError::MissingAddress)?;
        persist_quick_add(state, &crate::display::nebula_data_dir().join("ssh_profiles.json"), cx)
    }

    /// 保存到主机库后立刻连接。落盘失败时不连接，浮层保留并显示原因。
    pub(super) fn submit_ssh_quick_add(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let language = crate::gpui_shell::config::ui_language(cx);
        let destination = match self.save_quick_add_host(cx) {
            Ok(destination) => destination,
            Err(QuickAddError::MissingAddress) => {
                self.quick_add_error(
                    language.text(Message::HostsQuickAddAddressRequired).to_owned(),
                    cx,
                );
                return;
            },
            Err(QuickAddError::InvalidPort) => {
                self.quick_add_error(
                    language.text(Message::HostsQuickAddPortInvalid).to_owned(),
                    cx,
                );
                return;
            },
            Err(QuickAddError::KeysNeedAdvancedEditor) => {
                self.quick_add_error(
                    language.text(Message::HostsQuickAddKeysAdvanced).to_owned(),
                    cx,
                );
                return;
            },
            Err(QuickAddError::Rejected(error) | QuickAddError::Io(error)) => {
                self.quick_add_error(error, cx);
                return;
            },
        };
        self.close_ssh_quick_add(cx);
        // `add_ssh_terminal` 负责记住主机、开标签并走内建 SSH 连接链路。
        self.add_ssh_terminal(destination, window, cx);
    }

    /// 「更多设置」：带着已填地址跳到设置页的完整编辑器。浮层里拆好的
    /// 用户名/端口经 `draft_destination` 组合回 `user@host:port`，设置页
    /// 编辑器会再拆进各自的框；校验不过就把原始地址原样带过去。
    pub(super) fn open_quick_add_advanced(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let destination = self
            .ssh_quick_add
            .as_ref()
            .map(|state| {
                state
                    .draft_destination(cx)
                    .unwrap_or_else(|_| state.address.read(cx).value().trim().to_owned())
            })
            .unwrap_or_default();
        self.close_ssh_quick_add(cx);
        self.open_settings(window, cx);
        if let Some((pane, _)) = &self.settings_surface {
            let pane = pane.clone();
            window.defer(cx, move |window, cx| {
                pane.update(cx, |pane, cx| pane.manage_launcher_ssh(destination, true, window, cx));
            });
        }
        cx.notify();
    }

    fn quick_add_error(&mut self, error: String, cx: &mut gpui::Context<Self>) {
        if let Some(state) = self.ssh_quick_add.as_mut() {
            state.error = Some(error);
        }
        cx.notify();
    }

    pub(super) fn render_ssh_quick_add(
        &self,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let state = self.ssh_quick_add.as_ref()?;
        let language = crate::gpui_shell::config::ui_language(cx);
        let theme = cx.theme();
        // 字段顺序与设置页基础表单一致：名称(可选) → 地址*+端口 → 提示 →
        // destination 预览 → 用户名 → 分隔线 → 身份验证分段（同款组件同序）。
        let preview = state.draft_destination(cx).ok();
        use crate::ssh_profiles::SshAuthMode;
        let modes = [
            ("ssh-quick-add-password", language.pick("密码", "Password"), SshAuthMode::Password),
            ("ssh-quick-add-key", language.pick("密钥", "Key"), SshAuthMode::PublicKey),
            ("ssh-quick-add-auto", language.pick("自动", "Auto"), SshAuthMode::Auto),
            (
                "ssh-quick-add-interactive",
                language.pick("交互式", "Interactive"),
                SshAuthMode::KeyboardInteractive,
            ),
        ];
        let selected_auth = modes.iter().position(|(_, _, mode)| *mode == state.auth).unwrap_or(0);
        let auth_buttons = modes
            .iter()
            .map(|(id, label, mode)| {
                let mode = *mode;
                Button::new(*id)
                    .label(*label)
                    .custom(
                        gpui_component::button::ButtonCustomVariant::new(cx)
                            .hover(theme.foreground.opacity(0.04))
                            .active(theme.foreground.opacity(0.08)),
                    )
                    .small()
                    .flex_1()
                    .min_w_0()
                    .h(px(31.0))
                    .rounded(gpui_component::button::ButtonRounded::Size(px(5.0)))
                    .px_1()
                    .toggled(state.auth == mode)
                    .text_color(if state.auth == mode {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(state) = this.ssh_quick_add.as_mut() {
                            state.auth = mode;
                            state.error = None;
                        }
                        cx.notify();
                    }))
            })
            .collect::<Vec<_>>();
        let (shows_password, shows_keys) = crate::display::auth_sections(state.auth);
        let mut form = v_flex()
            .w_full()
            .child(
                v_flex()
                    .gap(px(7.0))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .font_medium()
                                    .child(language.pick("主机名称", "Host name")),
                            )
                            .child(editor_hint(language.pick("可选", "Optional"), cx)),
                    )
                    .child(editor_input(
                        &state.label,
                        language.pick("主机名称", "Host name"),
                        window,
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .mt_5()
                    .gap_3()
                    .items_start()
                    .child(div().flex_1().min_w_0().child(editor_field(
                        language.pick("主机地址 *", "Host address *"),
                        editor_input(
                            &state.address,
                            language.pick("主机地址", "Host address"),
                            window,
                            cx,
                        ),
                    )))
                    .child(div().w(px(84.0)).flex_shrink_0().child(editor_field(
                        language.pick("端口", "Port"),
                        editor_input(&state.port, language.pick("端口", "Port"), window, cx),
                    ))),
            )
            .child(editor_hint(language.tr("settings.ssh.address_hint"), cx).mt_1());
        if let Some(preview) = preview {
            form = form.child(editor_hint(preview, cx).mt_2().truncate());
        }
        form = form.child(div().mt_4().child(editor_field(
            language.pick("用户名", "Username"),
            editor_input(&state.username, language.pick("用户名", "Username"), window, cx),
        )));
        form = form.child(
            v_flex()
                .mt_5()
                .pt_5()
                .border_t_1()
                .border_color(theme.border)
                .child(
                    div()
                        .text_xs()
                        .font_medium()
                        .child(language.pick("身份验证", "Authentication")),
                )
                .child(
                    h_flex().mt_2().p(px(3.0)).rounded(px(8.0)).bg(theme.muted).child(
                        crate::gpui_shell::widgets::segmented::SegmentedRow::new(
                            "ssh-quick-add-auth-modes",
                            selected_auth,
                            h_flex().w_full().h(px(31.0)).gap(px(3.0)).children(auth_buttons),
                            div().rounded(px(5.0)).bg(theme.popover).shadow_sm(),
                        )
                        .w_full(),
                    ),
                ),
        );
        if shows_password {
            form = form.child(div().mt_4().child(editor_field(
                language.pick("密码", "Password"),
                editor_input(&state.password, language.pick("密码", "Password"), window, cx),
            )));
        }
        if shows_keys {
            form = form
                .child(editor_hint(language.text(Message::HostsQuickAddKeysAdvanced), cx).mt_2());
        }
        if let Some(error) = state.error.clone() {
            form = form.child(div().mt_2().text_xs().text_color(theme.danger).child(error));
        }

        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .occlude()
                .bg(theme.overlay)
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_ssh_quick_add(cx)),
                )
                .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                    match event.keystroke.key.as_str() {
                        "escape" => {
                            this.close_ssh_quick_add(cx);
                            cx.stop_propagation();
                        },
                        "enter" if !event.keystroke.modifiers.shift => {
                            this.submit_ssh_quick_add(window, cx);
                            cx.stop_propagation();
                        },
                        _ => {},
                    }
                }))
                .child(
                    v_flex()
                        .w(px(QUICK_ADD_WIDTH))
                        .max_w_full()
                        .gap(px(18.0))
                        .rounded(px(14.0))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.popover)
                        .p(px(22.0))
                        .shadow_lg()
                        .occlude()
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .child(language.text(Message::HostsQuickAddTitle)),
                        )
                        .child(form)
                        .child(
                            h_flex()
                                .justify_between()
                                .gap_2()
                                .child(
                                    Button::new("ssh-quick-add-advanced")
                                        .label(language.pick("更多设置", "More settings"))
                                        .ghost()
                                        .small()
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_quick_add_advanced(window, cx)
                                        })),
                                )
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .child(
                                            Button::new("ssh-quick-add-cancel")
                                                .label(language.text(Message::CommonCancel))
                                                .ghost()
                                                .small()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.close_ssh_quick_add(cx)
                                                })),
                                        )
                                        .child(
                                            Button::new("ssh-quick-add-save")
                                                .debug_selector(|| "ssh-quick-add-save".to_owned())
                                                .label(
                                                    language
                                                        .text(Message::HostsQuickAddSaveAndConnect),
                                                )
                                                .primary()
                                                .small()
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.submit_ssh_quick_add(window, cx)
                                                })),
                                        ),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;

    /// `add_window_view` 需要一个 Render 根视图；浮层状态在闭包里创建，
    /// 这里只当窗口宿主。
    struct TestHost;
    impl gpui::Render for TestHost {
        fn render(
            &mut self,
            _window: &mut Window,
            _cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            gpui::div()
        }
    }

    fn init_and_build<'a>(
        destination: &str,
        cx: &'a mut gpui::TestAppContext,
    ) -> (SshQuickAddState, &'a mut gpui::VisualTestContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(crate::gpui_shell::config::Settings::load(
                nebula_settings::ThemeName::Nord,
            ));
        });
        let mut state = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            state = Some(SshQuickAddState::new(destination, window, cx));
            TestHost
        });
        (state.expect("state built"), cx)
    }

    #[test]
    fn quick_add_prefills_user_and_port_from_the_typed_address() {
        // 不依赖窗口：预填拆分只走 `split_destination_*`。
        let (address_with_user, port) =
            crate::display::split_destination_port("deploy@box.example:2222");
        let (username, address) = crate::display::split_destination_user(&address_with_user);
        assert_eq!(
            (username.as_str(), address.as_str(), port.as_str()),
            ("deploy", "box.example", "2222")
        );
    }

    #[gpui::test]
    fn quick_add_draft_composes_and_validates(cx: &mut gpui::TestAppContext) {
        let (state, cx) = init_and_build("core.example.cn", cx);
        cx.update(|window, cx| {
            assert_eq!(state.draft_destination(cx).unwrap(), "root@core.example.cn");
            // 端口非法直接拒绝，不进入落盘。
            state.port.update(cx, |input, cx| input.set_value("70000", window, cx));
            assert_eq!(state.draft_destination(cx).unwrap_err(), QuickAddError::InvalidPort);
            state.port.update(cx, |input, cx| input.set_value("2222", window, cx));
            assert_eq!(state.draft_destination(cx).unwrap(), "root@core.example.cn:2222");
            // 带非法字符交给权威校验拒绝；空地址单独报"缺地址"。
            state.address.update(cx, |input, cx| input.set_value("bad host", window, cx));
            assert!(matches!(state.draft_destination(cx), Err(QuickAddError::Rejected(_))));
            state.address.update(cx, |input, cx| input.set_value("", window, cx));
            assert_eq!(state.draft_destination(cx).unwrap_err(), QuickAddError::MissingAddress);
        });
    }

    #[gpui::test]
    fn quick_add_persists_a_nameless_profile_before_any_connection(cx: &mut gpui::TestAppContext) {
        let (mut state, cx) = init_and_build("core.example.cn", cx);
        let directory = std::env::temp_dir().join(format!(
            "pebrel-quick-add-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("ssh_profiles.json");
        cx.update(|_, cx| {
            let destination = persist_quick_add(&state, &path, cx).unwrap();
            assert_eq!(destination, "root@core.example.cn");
        });
        let profiles = crate::ssh_profiles::SshProfiles::load(&path).unwrap();
        let profile = profiles.for_destination("root@core.example.cn");
        // 没填名称：保持空，列表回落显示地址，而不是自动起「主机 N」。
        assert_eq!(profile.label, None);
        assert_eq!(profile.auth, crate::ssh_profiles::SshAuthMode::Auto);
        // 公钥模式：浮层收集不了私钥文件，落盘必须被守卫拦下，
        // 且不能把空私钥的坏 profile 写进文件。
        cx.update(|_, cx| {
            state.auth = crate::ssh_profiles::SshAuthMode::PublicKey;
            assert_eq!(
                persist_quick_add(&state, &path, cx).unwrap_err(),
                QuickAddError::KeysNeedAdvancedEditor
            );
        });
        assert_eq!(
            crate::ssh_profiles::SshProfiles::load(&path)
                .unwrap()
                .for_destination("root@core.example.cn")
                .auth,
            crate::ssh_profiles::SshAuthMode::Auto,
            "rejected save must not overwrite the stored profile"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }
}
