//! Local session editor: the view owns form state and asynchronous store operations.

use super::*;
use crate::i18n::Message;
use crate::terminal_profiles::{TerminalProfile, TerminalProfiles};

pub(super) fn palette_row(language: crate::display::UiLanguage) -> WorkspacePaletteRow {
    WorkspacePaletteRow {
        group_order: 2,
        group: language.text(Message::LocalProfilesTitle).into(),
        label: language.text(Message::LocalProfilesManage).into(),
        hint: String::new(),
        hint_style: WorkspacePaletteHintStyle::Shortcut,
        search: "local shell session profile environment 本地 会话 环境 变量".into(),
        action: WorkspacePaletteAction::LocalProfiles,
        icon: None,
        icon_glyph: None,
        icon_path: None,
    }
}

impl NebulaWorkspace {
    pub(super) fn open_local_profiles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor =
            cx.new(|cx| LocalProfiles::new(crate::terminal_profiles::store_path(), window, cx));
        let language = crate::gpui_shell::config::ui_language(cx);
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title(language.text(Message::LocalProfilesTitle))
                .w(px(680.0))
                .child(editor.clone())
        });
    }
}

struct VariableRow {
    name: Entity<InputState>,
    value: Entity<InputState>,
}

struct LocalProfiles {
    path: std::path::PathBuf,
    entries: Vec<TerminalProfile>,
    original: Option<TerminalProfile>,
    id: Option<String>,
    name: Entity<InputState>,
    program: Entity<InputState>,
    args: Entity<InputState>,
    args_snapshot: Vec<String>,
    cwd: Entity<InputState>,
    shell_id: Entity<InputState>,
    variables: Vec<VariableRow>,
    advanced: bool,
    busy: bool,
    error: Option<String>,
    notice: Option<Message>,
    task: Option<gpui::Task<()>>,
}

impl LocalProfiles {
    fn new(path: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        fn input(
            window: &mut Window,
            cx: &mut Context<LocalProfiles>,
            message: Message,
        ) -> Entity<InputState> {
            let language = crate::gpui_shell::config::ui_language(cx);
            cx.new(|cx| InputState::new(window, cx).placeholder(language.text(message)))
        }
        let name = input(window, cx, Message::LocalProfilesName);
        let program = input(window, cx, Message::LocalProfilesProgram);
        let args = cx.new(|cx| InputState::new(window, cx).multi_line(true).rows(2));
        let cwd = input(window, cx, Message::LocalProfilesCwd);
        let shell_id = input(window, cx, Message::LocalProfilesShellId);
        let mut this = Self {
            path,
            entries: vec![],
            original: None,
            id: None,
            name,
            program,
            args,
            args_snapshot: Vec::new(),
            cwd,
            shell_id,
            variables: vec![],
            advanced: false,
            busy: false,
            error: None,
            notice: None,
            task: None,
        };
        this.refresh(cx);
        this
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        let path = self.path.clone();
        let read =
            cx.background_executor().spawn(async move { TerminalProfiles::load_from(&path) });
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = read.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(store) => {
                        this.entries = store.profiles().to_vec();
                        this.error = None;
                    },
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn select(
        &mut self,
        profile: Option<TerminalProfile>,
        duplicate: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        self.id = if duplicate { None } else { profile.as_ref().map(|profile| profile.id.clone()) };
        self.original = if duplicate { None } else { profile.clone() };
        self.args_snapshot =
            profile.as_ref().map(|profile| profile.args.clone()).unwrap_or_default();
        self.error = None;
        self.notice = None;
        for (input, value) in [
            (&self.name, profile.as_ref().map(|p| p.name.clone()).unwrap_or_default()),
            (
                &self.program,
                profile
                    .as_ref()
                    .map(|p| p.command.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            (&self.args, profile.as_ref().map(|p| p.args.join("\n")).unwrap_or_default()),
            (
                &self.cwd,
                profile
                    .as_ref()
                    .and_then(|p| p.cwd.as_ref().map(|p| p.to_string_lossy().into_owned()))
                    .unwrap_or_default(),
            ),
            (&self.shell_id, profile.as_ref().and_then(|p| p.shell_id.clone()).unwrap_or_default()),
        ] {
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
        self.variables.clear();
        if let Some(profile) = profile {
            for (name, value) in profile.env.0 {
                self.add_variable(name, value, window, cx);
            }
        }
        self.name.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn add_variable(
        &mut self,
        name: String,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(name, window, cx);
            input
        });
        let value = cx.new(|cx| {
            let mut input = InputState::new(window, cx).masked(true);
            input.set_value(value, window, cx);
            input
        });
        self.variables.push(VariableRow { name, value });
        cx.notify();
    }

    fn form(&self, cx: &App) -> Result<TerminalProfile, String> {
        let value = |input: &Entity<InputState>| input.read(cx).value().to_string();
        let env = crate::profile_environment::ProfileEnvironment::from_pairs(
            self.variables.iter().map(|row| (value(&row.name), value(&row.value))),
        )
        .map_err(|error| error.to_string())?;
        let args_text = value(&self.args);
        let args = if self.args_snapshot.join("\n") == args_text {
            self.args_snapshot.clone()
        } else if args_text.is_empty() {
            vec![]
        } else {
            args_text.split('\n').map(String::from).collect()
        };
        let cwd = value(&self.cwd);
        Ok(TerminalProfile {
            id: match &self.id {
                Some(id) => id.clone(),
                None => TerminalProfiles::new_id().map_err(|error| error.to_string())?,
            },
            name: value(&self.name),
            command: value(&self.program).into(),
            args,
            cwd: (!cwd.is_empty()).then(|| cwd.into()),
            shell_id: {
                let explicit = value(&self.shell_id);
                if explicit.trim().is_empty() { None } else { Some(explicit.trim().to_owned()) }
            },
            env,
        })
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        match self.form(cx) {
            Ok(profile) => self.edit(Some(profile), cx),
            Err(error) => {
                self.error = Some(error);
                self.notice = None;
                cx.notify();
            },
        }
    }

    fn edit(&mut self, replacement: Option<TerminalProfile>, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        self.notice = None;
        let path = self.path.clone();
        let expected = self.original.clone();
        let selected = replacement.clone();
        let write = cx.background_executor().spawn(async move {
            if let Some(profile) = &replacement {
                if !profile.command.is_file() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "shell executable does not exist",
                    ));
                }
                if profile.cwd.as_ref().is_some_and(|cwd| !cwd.is_dir()) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "starting directory does not exist",
                    ));
                }
                if profile.args.iter().any(|arg| arg.contains('\0')) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "argument contains NUL",
                    ));
                }
            }
            TerminalProfiles::edit_at(&path, expected.as_ref(), replacement)
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = write.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(store) => {
                        this.entries = store.profiles().to_vec();
                        this.id = selected.as_ref().map(|profile| profile.id.clone());
                        this.args_snapshot = selected
                            .as_ref()
                            .map(|profile| profile.args.clone())
                            .unwrap_or_default();
                        this.original = selected;
                        this.notice = Some(if this.original.is_some() {
                            Message::LocalProfilesSaved
                        } else {
                            Message::LocalProfilesDeleted
                        });
                    },
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Render for LocalProfiles {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let muted = cx.theme().muted_foreground;
        let busy = self.busy;
        v_flex()
            .w_full()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(language.text(Message::LocalProfilesStorage)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("local-profile-new")
                            .debug_selector(|| "local-profile-new".into())
                            .label(language.text(Message::LocalProfilesNew))
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.select(None, false, window, cx)
                            })),
                    )
                    .child(
                        Button::new("local-profile-reload")
                            .ghost()
                            .label(language.text(Message::LocalProfilesReload))
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    ),
            )
            .child(
                v_flex()
                    .id("local-profile-list")
                    .max_h(px(100.0))
                    .overflow_y_scroll()
                    .gap_1()
                    .children(self.entries.clone().into_iter().enumerate().map(
                        |(index, profile)| {
                            let copy = profile.clone();
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "local-profile-select-{index}"
                                    )))
                                    .ghost()
                                    .label(profile.name.clone())
                                    .disabled(busy)
                                    .on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            this.select(Some(profile.clone()), false, window, cx)
                                        },
                                    )),
                                )
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "local-profile-duplicate-{index}"
                                    )))
                                    .debug_selector(move || {
                                        format!("local-profile-duplicate-{index}")
                                    })
                                    .ghost()
                                    .label(language.text(Message::LocalProfilesDuplicate))
                                    .disabled(busy)
                                    .on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            this.select(Some(copy.clone()), true, window, cx)
                                        },
                                    )),
                                )
                        },
                    )),
            )
            .child(
                v_flex()
                    .id("local-profile-form")
                    .max_h(px(380.0))
                    .overflow_y_scroll()
                    .gap_2()
                    .children(
                        [
                            (&self.name, Message::LocalProfilesName),
                            (&self.program, Message::LocalProfilesProgram),
                            (&self.args, Message::LocalProfilesArgs),
                            (&self.cwd, Message::LocalProfilesCwd),
                        ]
                        .into_iter()
                        .map(|(input, message)| {
                            v_flex()
                                .gap_1()
                                .child(div().text_sm().child(language.text(message)))
                                .child(Input::new(input).disabled(busy).w_full())
                        }),
                    )
                    .when(self.advanced, |form| {
                        form.child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_sm()
                                        .child(language.text(Message::LocalProfilesShellId)),
                                )
                                .child(Input::new(&self.shell_id).disabled(busy).w_full()),
                        )
                    })
                    .children(self.variables.iter().enumerate().map(|(index, row)| {
                        h_flex()
                            .gap_2()
                            .w_full()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .child(language.text(Message::LocalProfilesVariable)),
                                    )
                                    .child(Input::new(&row.name).disabled(busy).w_full()),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .child(language.text(Message::LocalProfilesValue)),
                                    )
                                    .child(
                                        Input::new(&row.value)
                                            .mask_toggle()
                                            .disabled(busy)
                                            .w_full(),
                                    ),
                            )
                            .child(
                                Button::new(SharedString::from(format!(
                                    "local-profile-remove-variable-{index}"
                                )))
                                .ghost()
                                .label(language.text(Message::LocalProfilesRemove))
                                .disabled(busy)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.variables.remove(index);
                                        cx.notify();
                                    },
                                )),
                            )
                    })),
            )
            .child(
                Button::new("local-profile-advanced")
                    .ghost()
                    .label(language.text(Message::LocalProfilesAdvanced))
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.advanced = !this.advanced;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("local-profile-add-variable")
                    .debug_selector(|| "local-profile-add-variable".into())
                    .ghost()
                    .label(language.text(Message::LocalProfilesAddVariable))
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.add_variable(String::new(), String::new(), window, cx)
                    })),
            )
            .when_some(self.error.clone(), |root, error| {
                root.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(language.format(Message::LocalProfilesFailed, &[("error", &error)])),
                )
            })
            .when_some(self.notice, |root, notice| {
                root.child(div().text_sm().child(language.text(notice)))
            })
            .when(busy, |root| {
                root.child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(language.text(Message::LocalProfilesLoading)),
                )
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("local-profile-cancel")
                            .debug_selector(|| "local-profile-cancel".into())
                            .ghost()
                            .label(language.text(Message::EditorCancel))
                            .on_click(cx.listener(|_, _, window, cx| window.close_dialog(cx))),
                    )
                    .child(
                        Button::new("local-profile-save")
                            .debug_selector(|| "local-profile-save".into())
                            .primary()
                            .label(language.text(Message::LocalProfilesSave))
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                    )
                    .child(
                        Button::new("local-profile-delete")
                            .ghost()
                            .label(language.text(Message::LocalProfilesDelete))
                            .disabled(busy || self.original.is_none())
                            .on_click(cx.listener(|this, _, _, cx| this.edit(None, cx))),
                    ),
            )
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;
