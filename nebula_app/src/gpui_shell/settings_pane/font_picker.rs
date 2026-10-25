use super::*;

use crate::font_install::{FontSource, REQUIRED_FONT_FAMILY};

/// 字体选择面板服务的三行设置字段：终端字体、中文回退字体、标签字体。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FontPickerTarget {
    Terminal,
    Cjk,
    Tab,
}

const FONT_PICKER_LIST_PREFERRED_HEIGHT: f32 = 220.0;
const FONT_PICKER_PANEL_WIDTH: f32 = 360.0;
const FONT_PICKER_PANEL_CHROME_HEIGHT: f32 = 74.0;
const FONT_PICKER_OFFSET_Y: f32 = 6.0;
const FONT_PICKER_WINDOW_MARGIN: f32 = 8.0;

impl SettingsPane {
    pub(super) fn new_font_family_input(
        runtime_font_family: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let initial = cx
            .try_global::<crate::gpui_shell::config::Settings>()
            .map(|settings| settings.font_family.clone())
            .or(runtime_font_family)
            .unwrap_or_else(|| REQUIRED_FONT_FAMILY.to_owned());
        let language = crate::gpui_shell::config::ui_language(cx);
        cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.text(crate::i18n::Message::SettingsFontEnterAFontFamily))
                .default_value(initial)
        })
    }

    /// 当前生效字体链（settings.txt 覆盖 toml 后的值，可能含逗号 fallback）。
    pub(in crate::gpui_shell) fn current_font_chain(&self, cx: &App) -> String {
        cx.try_global::<crate::gpui_shell::config::Settings>()
            .map(|settings| settings.font_family.clone())
            .unwrap_or_else(|| String::from(REQUIRED_FONT_FAMILY))
    }

    fn picker_input(&self) -> Entity<InputState> {
        match self.font_picker_target {
            FontPickerTarget::Terminal => self.font_family_input.clone(),
            FontPickerTarget::Cjk => self.font_family_cjk_input.clone(),
            FontPickerTarget::Tab => self.tab_font_family_input.clone(),
        }
    }

    fn picker_key(&self) -> &'static str {
        match self.font_picker_target {
            FontPickerTarget::Terminal => "font_family",
            FontPickerTarget::Cjk => "font_family_cjk",
            FontPickerTarget::Tab => "tab_font_family",
        }
    }

    fn picker_font_chain(&self, cx: &App) -> String {
        match self.font_picker_target {
            FontPickerTarget::Terminal => self.current_font_chain(cx),
            FontPickerTarget::Cjk => self
                .runtime
                .font_family_cjk
                .clone()
                .unwrap_or_else(|| REQUIRED_FONT_FAMILY.to_owned()),
            FontPickerTarget::Tab => self.effective_tab_font_chain(),
        }
    }

    fn configured_font_families(&self, cx: &App) -> Vec<String> {
        let mut families = crate::font_install::font_family_chain(&self.picker_font_chain(cx));
        if families.is_empty() {
            families.push(REQUIRED_FONT_FAMILY.to_owned());
        }
        families
    }

    fn open_font_picker(&mut self, cx: &mut Context<Self>) {
        if self.font_picker_open {
            return;
        }

        self.font_picker_open = true;
        self.ensure_font_catalog(cx);
        cx.notify();
    }

    pub(super) fn on_font_family_input_event(
        &mut self,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.on_picker_input_event(FontPickerTarget::Terminal, event, window, cx);
    }

    pub(super) fn on_cjk_font_family_input_event(
        &mut self,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.on_picker_input_event(FontPickerTarget::Cjk, event, window, cx);
    }

    pub(super) fn on_tab_font_family_input_event(
        &mut self,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.on_picker_input_event(FontPickerTarget::Tab, event, window, cx);
    }

    fn on_picker_input_event(
        &mut self,
        target: FontPickerTarget,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::Focus) {
            if self.font_picker_open && self.font_picker_target != target {
                self.close_font_picker(window, false, cx);
            }
            self.font_picker_target = target;
            self.open_font_picker(cx);
            return;
        }
        // A delayed blur from the other field must not commit the active field.
        if self.font_picker_target != target {
            return;
        }
        match event {
            InputEvent::Change => cx.notify(),
            InputEvent::PressEnter { .. } => self.close_font_picker(window, true, cx),
            InputEvent::Blur => self.close_font_picker(window, false, cx),
            InputEvent::Focus => {},
        }
    }

    /// 关闭弹层时才规范化并提交手写字体链。系统文件选择器接管焦点时不强抢焦点。
    pub(super) fn close_font_picker(
        &mut self,
        window: &mut Window,
        restore_focus: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.font_picker_open {
            return;
        }
        self.font_picker_open = false;
        self.commit_font_family_input(window, cx);
        if restore_focus {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    /// 系统字体和导入字体的探测都可能读大量文件，必须离开 UI 线程；目录
    /// 结果只装配一次，整个字体组下拉框共享这一份缓存。
    pub(super) fn ensure_font_catalog(&mut self, cx: &mut Context<Self>) {
        if self.font_system.is_some() || self.font_loading {
            return;
        }
        {
            self.font_loading = true;
            let text_system = cx.text_system().clone();
            let task = cx.background_executor().spawn(async move {
                let system = crate::platform::fonts::system_families(text_system);
                let imported: Vec<String> = crate::font_install::imported_font_files()
                    .iter()
                    .filter_map(|path| crate::platform::fonts::file_families(path).ok())
                    .flatten()
                    .collect();
                (system, imported)
            });
            cx.spawn(async move |this, cx| {
                let (system, imported) = task.await;
                let _ = this.update_in(cx, |pane, window, cx| {
                    pane.font_system = Some(system);
                    for family in imported {
                        if !pane
                            .font_imported
                            .iter()
                            .any(|known| known.eq_ignore_ascii_case(&family))
                        {
                            pane.font_imported.push(family);
                        }
                    }
                    pane.font_loading = false;
                    pane.refresh_theme_font_select(window, cx);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// 标签字体未设置时真正生效的链：跟随 `ui_font_family`，再落 Geist。
    /// 渲染侧三处落点同样只回落 `theme.font_family`，这里必须与它一致。
    fn effective_tab_font_chain(&self) -> String {
        self.runtime.tab_font_family.clone().unwrap_or_else(|| {
            self.runtime
                .ui_font_family
                .clone()
                .unwrap_or_else(|| crate::gpui_shell::typography::UI_FONT_FAMILY.to_owned())
        })
    }

    pub(super) fn new_tab_font_family_input(
        runtime: &RuntimeSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let initial = runtime.tab_font_family.clone().unwrap_or_else(|| {
            runtime
                .ui_font_family
                .clone()
                .unwrap_or_else(|| crate::gpui_shell::typography::UI_FONT_FAMILY.to_owned())
        });
        let language = crate::gpui_shell::config::ui_language(cx);
        cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(language.pick("输入字体名称", "Enter a font family"))
                .default_value(initial)
        })
    }

    fn commit_font_family_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.picker_font_chain(cx);
        let raw = self.picker_input().read(cx).value().to_string();
        let normalized = crate::font_install::normalize_font_family_chain(&raw);
        // 空链无法提供主字体；恢复当前生效值，而不是写入一个随后被忽略的空项。
        let next = if normalized.is_empty() {
            match self.font_picker_target {
                FontPickerTarget::Cjk => REQUIRED_FONT_FAMILY.to_owned(),
                // 清空标签字体 = 跟随界面字体链（含 ui_font_family 覆盖）。
                FontPickerTarget::Tab => self.effective_tab_font_chain(),
                FontPickerTarget::Terminal => current.clone(),
            }
        } else {
            normalized
        };
        self.set_font_family_input(next.clone(), window, cx);
        if next != current {
            self.persist(&[(self.picker_key(), next)], cx);
        }
    }

    fn set_font_family_input(&self, value: String, window: &mut Window, cx: &mut Context<Self>) {
        self.picker_input().update(cx, |input, cx| input.set_value(value, window, cx));
    }

    /// 加号始终把字体追加到组尾；与候选行的 WT 式末段补全是两个独立动作。
    fn append_font_family(&mut self, family: String, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.picker_font_chain(cx);
        let next = crate::font_install::append_font_fallback(&current, &family);
        self.set_font_family_input(next.clone(), window, cx);
        if next != current {
            self.persist(&[(self.picker_key(), next)], cx);
        }
        self.picker_input().update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    /// 候选点击替换最后一个逗号段；要追加 fallback 时先在输入末尾键入逗号。
    fn select_font_family(&mut self, family: String, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.picker_input().read(cx).value().to_string();
        let next = crate::font_install::complete_font_family_input(&raw, &family);
        let current = self.picker_font_chain(cx);
        self.set_font_family_input(next.clone(), window, cx);
        if next != current {
            self.persist(&[(self.picker_key(), next)], cx);
        }
        self.picker_input().update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn move_font_family(
        &mut self,
        index: usize,
        direction: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self.picker_font_chain(cx);
        let next = crate::font_install::move_font_family(&current, index, direction);
        self.set_font_family_input(next.clone(), window, cx);
        if next != current {
            self.persist(&[(self.picker_key(), next)], cx);
        }
    }

    fn remove_font_family(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.picker_font_chain(cx);
        let next = crate::font_install::remove_font_family(&current, index);
        self.set_font_family_input(next.clone(), window, cx);
        if next != current {
            self.persist(&[(self.picker_key(), next)], cx);
        }
    }

    /// 与标准 Select 共用 220px 控件列。Input 自己负责长文本的单行滚动，
    /// 逗号链再长也只能在字段内部移动，不能反向撑开设置行。
    fn font_picker_row(
        &self,
        target: FontPickerTarget,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let picker = cx.entity().downgrade();
        let (input, field_id, chevron_id, shell_id) = match target {
            FontPickerTarget::Terminal => (
                &self.font_family_input,
                "font-family-input",
                "font-picker-chevron",
                "font-family-input-shell",
            ),
            FontPickerTarget::Cjk => (
                &self.font_family_cjk_input,
                "font-family-cjk-input",
                "font-picker-cjk-chevron",
                "font-family-cjk-input-shell",
            ),
            FontPickerTarget::Tab => (
                &self.tab_font_family_input,
                "font-family-tab-input",
                "font-picker-tab-chevron",
                "font-family-tab-input-shell",
            ),
        };
        let focused = input.read(cx).focus_handle(cx).is_focused(window);
        let hover = cx.theme().foreground.opacity(if cx.theme().is_dark() { 0.08 } else { 0.06 });
        let edge = cx.theme().muted_foreground.opacity(0.65);
        let control = div()
            .id(shell_id)
            .rounded(px(6.0))
            .border_1()
            .border_color(if focused { cx.theme().ring } else { cx.theme().border })
            .hover(move |frame| {
                let frame = frame.bg(hover);
                if focused { frame } else { frame.border_color(edge) }
            })
            .relative()
            .w(px(SETTINGS_SELECT_WIDTH))
            .min_w_0()
            .h(px(36.0))
            .flex()
            .items_center()
            .debug_selector(move || field_id.to_owned())
            .flex_shrink_0()
            .overflow_hidden()
            .child(
                gpui::Styled::h_full(Input::new(input))
                    .appearance(false)
                    .focus_bordered(false)
                    .w_full()
                    .cleanable(false)
                    .suffix(Button::new(chevron_id)
                            .debug_selector(move || chevron_id.to_owned()).ghost().xsmall()
                        .icon(if self.font_picker_open && self.font_picker_target == target { IconName::ChevronUp } else { IconName::ChevronDown })
                        .tooltip(language.text(if self.font_picker_open && self.font_picker_target == target { crate::i18n::Message::SettingsFontCollapse } else { crate::i18n::Message::SettingsFontExpand }))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| { cx.stop_propagation(); })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let was_open = this.font_picker_open && this.font_picker_target == target;
                            if this.font_picker_open && !was_open { this.close_font_picker(window, false, cx); }
                            this.font_picker_target = target;
                            this.picker_input().update(cx, |input, cx| input.focus(window, cx));
                            if was_open { this.close_font_picker(window, false, cx); } else { this.open_font_picker(cx); }
                            cx.stop_propagation();
                            cx.notify();
                        })))
                    .aria_label(language.text(match target {
                        FontPickerTarget::Terminal => crate::i18n::Message::SettingsFontEnglish,
                        FontPickerTarget::Cjk => crate::i18n::Message::SettingsFontChinese,
                        FontPickerTarget::Tab => crate::i18n::Message::SettingsFontTabFamily,
                    })),
            )
            // 弹层仍须取输入框的真实窗口坐标，才能正确处理滚动、缩放与 DPI。
            .child(
                gpui::canvas(
                    move |bounds, _, cx| {
                        let _ = picker.update(cx, |picker, cx| {
                            let slot = match target {
                                FontPickerTarget::Terminal => &mut picker.font_picker_trigger_bounds,
                                FontPickerTarget::Cjk => &mut picker.font_picker_cjk_bounds,
                                FontPickerTarget::Tab => &mut picker.font_picker_tab_bounds,
                            };
                            if *slot == Some(bounds) { return; }
                            *slot = Some(bounds);
                            // anchored() 消费的是上一轮 render 拿到的窗口坐标；
                            // 聚焦自动滚动后补一帧，弹层才会继续贴住输入框。
                            if picker.font_picker_open && picker.font_picker_target == target {
                                cx.notify();
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0()
                .size_full(),
            );
        if self.active_section == 1 {
            return control.into_any_element();
        }
        let (label, help_text): (&'static str, SettingHelp) = match target {
            FontPickerTarget::Terminal => (
                language.text(crate::i18n::Message::SettingsFontEnglish),
                help("font_family", language),
            ),
            FontPickerTarget::Cjk => (
                language.text(crate::i18n::Message::SettingsFontChinese),
                language.text(crate::i18n::Message::SettingsFontChineseDescription).into(),
            ),
            FontPickerTarget::Tab => (
                language.text(crate::i18n::Message::SettingsFontTabFamily),
                language.text(crate::i18n::Message::SettingsFontTabFamilyDescription).into(),
            ),
        };
        self.row(label, help_text, control, cx).into_any_element()
    }

    fn chain_action(
        id: SharedString,
        icon: IconName,
        tooltip: &'static str,
        disabled: bool,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .icon(icon)
            .ghost()
            .xsmall()
            .tooltip(tooltip)
            .disabled(disabled)
            .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
    }

    /// 弹层先列当前顺序，再列输入框末段的建议。整行点击补全，加号则追加到
    /// 字体组末尾，两种常用操作互不抢占。
    fn font_picker_panel(
        &mut self,
        width: gpui::Pixels,
        list_height: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let muted = cx.theme().muted_foreground;
        let hover_bg = cx.theme().list_hover;
        let selected_bg = cx.theme().list_active;
        let warning = cx.theme().warning;
        let border = cx.theme().border;
        let families = self.configured_font_families(cx);
        let family_count = families.len();
        let input_value = self.picker_input().read(cx).value().to_string();
        let current = self.picker_font_chain(cx);
        let input_normalized = crate::font_install::normalize_font_family_chain(&input_value);
        let current_normalized = crate::font_install::normalize_font_family_chain(&current);
        // 初次聚焦默认展开完整目录；开始编辑后只拿最后一个逗号段过滤，
        // 这样键入 `Font A,` 会重新显示全部候选供追加 fallback。
        let query = if input_normalized == current_normalized {
            ""
        } else {
            input_value.rsplit(',').next().unwrap_or_default().trim()
        };
        let system = self.font_system.clone().unwrap_or_default();
        let catalog =
            crate::font_install::font_catalog(&system, &self.font_imported, true, query, "");

        let badge = |text: &'static str, color: Hsla| {
            div()
                .flex_shrink_0()
                .px(px(5.0))
                .py(px(1.0))
                .rounded_sm()
                .text_xs()
                .text_color(color)
                .border_1()
                .border_color(color.opacity(0.4))
                .child(text)
        };

        let selected_rows: Vec<_> = families
            .iter()
            .enumerate()
            .map(|(index, family)| {
                let family_name: SharedString = family.clone().into();
                let display_name: SharedString = font_display_name(family).into();
                let tooltip_name = display_name.clone();
                h_flex()
                    .id(SharedString::from(format!("font-chain-row-{index}")))
                    .h(px(38.0))
                    .w_full()
                    .min_w_0()
                    .px_1()
                    .gap_1()
                    .items_center()
                    .rounded_md()
                    .bg(selected_bg)
                    .child(
                        div()
                            .w(px(16.0))
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(muted)
                            .child((index + 1).to_string()),
                    )
                    .child(
                        div()
                            .id(("font-chain-name", index))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .font_family(family_name)
                            .tooltip(move |window, cx| {
                                gpui_component::tooltip::Tooltip::new(tooltip_name.clone())
                                    .build(window, cx)
                            })
                            .child(display_name),
                    )
                    .when(index == 0, |row| {
                        row.child(badge(
                            language.text(crate::i18n::Message::SettingsFontPrimary),
                            muted,
                        ))
                    })
                    .child(Self::chain_action(
                        SharedString::from(format!("font-chain-up-{index}")),
                        IconName::ArrowUp,
                        language.text(crate::i18n::Message::SettingsFontMoveUp),
                        index == 0,
                        move |this, window, cx| {
                            this.move_font_family(index, -1, window, cx);
                        },
                        cx,
                    ))
                    .child(Self::chain_action(
                        SharedString::from(format!("font-chain-down-{index}")),
                        IconName::ArrowDown,
                        language.text(crate::i18n::Message::SettingsFontMoveDown),
                        index + 1 == family_count,
                        move |this, window, cx| {
                            this.move_font_family(index, 1, window, cx);
                        },
                        cx,
                    ))
                    .child(Self::chain_action(
                        SharedString::from(format!("font-chain-delete-{index}")),
                        IconName::Delete,
                        language.text(crate::i18n::Message::SettingsFontRemoveFromFontGroup),
                        family_count == 1,
                        move |this, window, cx| this.remove_font_family(index, window, cx),
                        cx,
                    ))
            })
            .collect();

        let available_rows: Vec<_> = catalog
            .into_iter()
            .enumerate()
            .map(|(index, entry)| {
                let select_name = entry.name.clone();
                let append_name = entry.name.clone();
                let family: SharedString = entry.name.clone().into();
                let display_name: SharedString = font_display_name(&entry.name).into();
                let tooltip_name = display_name.clone();
                let bundled = entry.source == FontSource::Bundled;
                let already_selected =
                    families.iter().any(|family| family.eq_ignore_ascii_case(&entry.name));
                h_flex()
                    .id(SharedString::from(format!("font-available-row-{index}")))
                    .debug_selector(move || format!("font-available-{index}"))
                    .h(px(38.0))
                    .w_full()
                    .min_w_0()
                    .px_2()
                    .gap_1()
                    .items_center()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|row| row.bg(hover_bg))
                    .child(
                        div()
                            .id(("font-available-name", index))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .font_family(family)
                            .tooltip(move |window, cx| {
                                gpui_component::tooltip::Tooltip::new(tooltip_name.clone())
                                    .build(window, cx)
                            })
                            .child(display_name),
                    )
                    .when(bundled, |row| {
                        row.child(badge(
                            language.text(crate::i18n::Message::SettingsFontBundled),
                            muted,
                        ))
                    })
                    .when(entry.source == FontSource::Imported, |row| {
                        row.child(badge(
                            language.text(crate::i18n::Message::SettingsFontImported),
                            muted,
                        ))
                    })
                    .when(!entry.monospaced, |row| {
                        row.child(badge(
                            language.text(crate::i18n::Message::SettingsFontProportional),
                            warning,
                        ))
                    })
                    .child(
                        Button::new(SharedString::from(format!("font-add-{index}")))
                            .icon(IconName::Plus)
                            .ghost()
                            .xsmall()
                            .tooltip(
                                language.text(crate::i18n::Message::SettingsFontAddToFontGroup),
                            )
                            .disabled(already_selected)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.append_font_family(append_name.clone(), window, cx);
                            })),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_font_family(select_name.clone(), window, cx);
                    }))
            })
            .collect();
        let available_empty = available_rows.is_empty();

        v_flex()
            // The menu needs room for family names and ordering actions; the trigger
            // remains aligned with the other settings fields.
            .debug_selector(|| "font-picker-panel".to_owned())
            .w(width)
            .max_w_full()
            .p_3()
            .gap_2()
            .popover_style(cx)
            .occlude()
            .overflow_hidden()
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(language.text(crate::i18n::Message::SettingsFontFontGroup)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(language.tr("settings.font.group_description")),
            )
            .when(self.font_loading, |panel| {
                panel.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(Spinner::new().xsmall())
                        .child(
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child(language.tr("settings.font.loading")),
                        ),
                )
            })
            .when(!self.font_loading, |panel| {
                panel.child(v_flex().debug_selector(|| "font-picker-list".to_owned()).h(list_height).min_h_0().overflow_y_scrollbar().child(
                    v_flex()
                        .w_full()
                        // gpui-component 的纵向滚动条以 16px 绝对定位覆盖在
                        // 内容右侧，不会自动挤出布局空间。这里预留同宽安全区，
                        // 避免排序操作与候选行落进滚动条命中区而发生误触。
                        .pr(px(16.0))
                        .gap_1()
                        .child(
                            div()
                                .px_1()
                                .text_xs()
                                .text_color(muted)
                                .child(language.text(crate::i18n::Message::SettingsFontCurrentOrder)),
                        )
                        .children(selected_rows)
                        .child(
                            div()
                                .mt_2()
                                .pt_2()
                                .border_t_1()
                                .border_color(border.opacity(0.5))
                                .px_1()
                                .text_xs()
                                .text_color(muted)
                                .child(language.text(crate::i18n::Message::SettingsFontAvailableFonts)),
                        )
                        .children(available_rows)
                        .when(available_empty, |list| {
                            list.child(
                                div()
                                    .py_2()
                                    .text_sm()
                                    .text_color(muted)
                                    .child(language.tr("settings.font.no_matches")),
                            )
                        }),
                ))
            })
    }

    /// Keep the menu inside the window, preferring the side with room for candidates.
    pub(super) fn font_picker_dropdown(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        self.font_picker_dropdown_for(FontPickerTarget::Terminal, window, cx)
    }

    pub(super) fn cjk_font_picker_dropdown(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        self.font_picker_dropdown_for(FontPickerTarget::Cjk, window, cx)
    }

    pub(super) fn tab_font_picker_dropdown(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        self.font_picker_dropdown_for(FontPickerTarget::Tab, window, cx)
    }

    fn font_picker_dropdown_for(
        &mut self,
        target: FontPickerTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let row = self.font_picker_row(target, window, cx);
        let trigger_bounds = match target {
            FontPickerTarget::Terminal => self.font_picker_trigger_bounds,
            FontPickerTarget::Cjk => self.font_picker_cjk_bounds,
            FontPickerTarget::Tab => self.font_picker_tab_bounds,
        };
        let viewport = window.viewport_size();
        let panel_width = px(FONT_PICKER_PANEL_WIDTH
            .min((f32::from(viewport.width) - 2.0 * FONT_PICKER_WINDOW_MARGIN).max(0.0)));
        let (open_above, list_height) = trigger_bounds
            .as_ref()
            .map(|bounds| {
                let below = (f32::from(viewport.height - bounds.bottom_right().y)
                    - FONT_PICKER_OFFSET_Y
                    - FONT_PICKER_WINDOW_MARGIN)
                    .max(0.0);
                let above =
                    (f32::from(bounds.origin.y) - FONT_PICKER_OFFSET_Y - FONT_PICKER_WINDOW_MARGIN)
                        .max(0.0);
                let preferred = FONT_PICKER_PANEL_CHROME_HEIGHT + FONT_PICKER_LIST_PREFERRED_HEIGHT;
                let open_above = below < preferred && above > below;
                let available = if open_above { above } else { below };
                (
                    open_above,
                    px((available - FONT_PICKER_PANEL_CHROME_HEIGHT)
                        .clamp(0.0, FONT_PICKER_LIST_PREFERRED_HEIGHT)),
                )
            })
            .unwrap_or((false, px(FONT_PICKER_LIST_PREFERRED_HEIGHT)));
        let panel = (self.font_picker_open && self.font_picker_target == target)
            .then(|| self.font_picker_panel(panel_width, list_height, cx));

        div().relative().w(px(SETTINGS_SELECT_WIDTH)).flex_shrink_0().child(row).when_some(
            panel.zip(trigger_bounds),
            |anchor, (panel, trigger_bounds)| {
                anchor.child(
                    deferred(
                        anchored()
                            .anchor(if open_above {
                                gpui::Anchor::BottomRight
                            } else {
                                gpui::Anchor::TopRight
                            })
                            .position(if open_above {
                                trigger_bounds.top_right()
                            } else {
                                trigger_bounds.bottom_right()
                            })
                            .offset(gpui::point(
                                px(0.0),
                                px(if open_above {
                                    -FONT_PICKER_OFFSET_Y
                                } else {
                                    FONT_PICKER_OFFSET_Y
                                }),
                            ))
                            .snap_to_window_with_margin(px(FONT_PICKER_WINDOW_MARGIN))
                            .child(panel),
                    )
                    .with_priority(2),
                )
            },
        )
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod interaction_tests {
    use super::*;
    use gpui::{TestAppContext, point};
    use gpui_component::Root;

    #[gpui::test]
    fn cjk_dropdown_selection_preserves_the_english_font_chain(cx: &mut TestAppContext) {
        use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};
        let _lock = lock_theme_studio();
        let _settings = SettingsBytesGuard::capture();
        nebula_settings::persist_keys(&[
            ("font_family", REQUIRED_FONT_FAMILY.into()),
            ("font_family_cjk", "Consolas".into()),
        ])
        .unwrap();
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_reduce_motion(true);
            cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
        });
        let mut pane = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| SettingsPane::new(window, cx));
            view.update(cx, |pane, _| {
                pane.active_section = 1;
                pane.font_system = Some(Vec::new());
            });
            pane = Some(view.clone());
            Root::new(view, window, cx)
        });
        let pane = pane.unwrap();
        window.simulate_resize(gpui::size(px(1280.0), px(1600.0)));
        window.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let arrow = window.debug_bounds("font-picker-cjk-chevron").unwrap();
        window.simulate_click(arrow.center(), gpui::Modifiers::default());
        window.run_until_parked();
        window.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(pane.read_with(window, |pane, _| pane.font_picker_open
            && pane.font_picker_target == FontPickerTarget::Cjk));
        let expected = crate::font_install::font_catalog(&[], &[], true, "", "")[0].name.clone();
        let candidate = window.debug_bounds("font-available-0").unwrap();
        window.simulate_click(
            point(candidate.left() + px(40.0), candidate.center().y),
            gpui::Modifiers::default(),
        );
        window.run_until_parked();
        let saved = RuntimeSettings::load();
        assert_eq!(saved.font_family.as_deref(), Some(REQUIRED_FONT_FAMILY));
        assert_eq!(saved.font_family_cjk.as_deref(), Some(expected.as_str()));
        window.simulate_keystrokes("escape");
        window.run_until_parked();
        assert!(!pane.read_with(window, |pane, _| pane.font_picker_open));
        assert_eq!(
            pane.read_with(window, |pane, cx| pane.font_family_input.read(cx).value().to_string()),
            REQUIRED_FONT_FAMILY
        );
    }

    #[gpui::test]
    fn tab_dropdown_selection_persists_tab_font_family(cx: &mut TestAppContext) {
        use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};
        let _lock = lock_theme_studio();
        let _settings = SettingsBytesGuard::capture();
        // 未设置标签字体时，输入框显示生效链（ui_font_family 覆盖 Geist）。
        // 本机 settings 可能已有 tab_font_family 残留；显式清空保证初始值确定。
        nebula_settings::persist_keys(&[
            ("ui_font_family", "Inter".into()),
            ("tab_font_family", String::new()),
        ])
        .unwrap();
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_reduce_motion(true);
            cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
        });
        let mut pane = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| SettingsPane::new(window, cx));
            view.update(cx, |pane, _| {
                pane.active_section = 1;
                pane.font_system = Some(Vec::new());
            });
            pane = Some(view.clone());
            Root::new(view, window, cx)
        });
        let pane = pane.unwrap();
        // 界面组在设置页靠后；加高窗口让标签字体行落在视口内。
        window.simulate_resize(gpui::size(px(1280.0), px(2400.0)));
        window.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(
            pane.read_with(window, |pane, cx| pane
                .tab_font_family_input
                .read(cx)
                .value()
                .to_string()),
            "Inter"
        );
        let arrow = window.debug_bounds("font-picker-tab-chevron").unwrap();
        window.simulate_click(arrow.center(), gpui::Modifiers::default());
        window.run_until_parked();
        window.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(pane.read_with(window, |pane, _| pane.font_picker_open
            && pane.font_picker_target == FontPickerTarget::Tab));
        let expected = crate::font_install::font_catalog(&[], &[], true, "", "")[0].name.clone();
        let candidate = window.debug_bounds("font-available-0").unwrap();
        window.simulate_click(
            point(candidate.left() + px(40.0), candidate.center().y),
            gpui::Modifiers::default(),
        );
        window.run_until_parked();
        let saved = RuntimeSettings::load();
        assert_eq!(saved.tab_font_family.as_deref(), Some(expected.as_str()));
        window.simulate_keystrokes("escape");
        window.run_until_parked();
        assert!(!pane.read_with(window, |pane, _| pane.font_picker_open));
    }

    #[gpui::test]
    fn review_regression_font_fields_align_and_dropdown_toggles_with_search(
        cx: &mut TestAppContext,
    ) {
        use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};
        let _lock = lock_theme_studio();
        let _settings = SettingsBytesGuard::capture();
        cx.update(|cx| {
            gpui_component::init(cx);
            let mut settings =
                crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord);
            // This layout fixture controls its font chain; installed/user
            // preferences must not decide its expected field contents.
            settings.font_family = REQUIRED_FONT_FAMILY.to_owned();
            cx.set_global(settings);
        });
        let mut pane = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| SettingsPane::new(window, cx));
            pane = Some(view.clone());
            Root::new(view, window, cx)
        });
        let pane = pane.unwrap();
        cx.simulate_resize(gpui::size(px(1280.0), px(1600.0)));
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let english = cx.debug_bounds("font-family-input").expect("English field is visible");
        let chinese = cx.debug_bounds("font-family-cjk-input").expect("Chinese field is visible");
        assert_eq!(english.size.width, px(SETTINGS_SELECT_WIDTH));
        assert_eq!(english.size, chinese.size, "both font fields share the same dimensions");
        assert_eq!(
            english.origin.x, chinese.origin.x,
            "both font fields align in the control column"
        );
        assert_eq!(
            pane.read_with(cx, |pane, cx| pane.font_family_input.read(cx).value().to_string()),
            REQUIRED_FONT_FAMILY
        );
        assert_eq!(
            pane.read_with(cx, |pane, cx| pane.font_family_cjk_input.read(cx).value().to_string()),
            pane.read_with(cx, |pane, _| pane
                .runtime
                .font_family_cjk
                .clone()
                .unwrap_or_else(|| REQUIRED_FONT_FAMILY.to_owned()))
        );
        let bounds =
            cx.debug_bounds("font-picker-chevron").expect("font field exposes a dropdown arrow");
        let center = point(
            bounds.origin.x + bounds.size.width * 0.5,
            bounds.origin.y + bounds.size.height * 0.5,
        );
        cx.simulate_click(center, gpui::Modifiers::default());
        cx.run_until_parked();
        assert!(pane.read_with(cx, |pane, _| pane.font_picker_open));
        cx.simulate_click(center, gpui::Modifiers::default());
        cx.run_until_parked();
        assert!(!pane.read_with(cx, |pane, _| pane.font_picker_open));
        // At the real 144-DPI window's logical height, placing the menu below
        // the font field used to hide every available family under its header.
        cx.simulate_resize(gpui::size(px(1280.0), px(735.0)));
        cx.run_until_parked();
        let bounds = cx.debug_bounds("font-picker-chevron").unwrap();
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let trigger = cx.debug_bounds("font-family-input").unwrap();
        let panel = cx.debug_bounds("font-picker-panel").unwrap();
        let list = cx.debug_bounds("font-picker-list").unwrap();
        let candidate = cx.debug_bounds("font-available-0").unwrap();
        assert!(
            panel.origin.y >= px(0.0) && panel.bottom_right().y <= px(735.0),
            "font menu stays within the window: {panel:?}"
        );
        assert_eq!(panel.size.width, px(FONT_PICKER_PANEL_WIDTH));
        assert!(
            panel.bottom_right().y <= trigger.origin.y
                || panel.origin.y >= trigger.bottom_right().y,
            "menu must not cover the field after focus scrolling: panel={panel:?}, trigger={trigger:?}"
        );
        assert!(
            candidate.origin.y >= list.origin.y
                && candidate.bottom_right().y <= list.bottom_right().y,
            "at least the first available family is fully visible without scrolling: list={list:?}, candidate={candidate:?}, trigger={trigger:?}"
        );
        cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        cx.update(|window, cx| {
            pane.update(cx, |pane, cx| {
                pane.settings_search_input
                    .update(cx, |input, cx| input.replace_all("quick terminal", window, cx));
            })
        });
        cx.run_until_parked();
        assert_eq!(
            pane.read_with(cx, |pane, cx| (
                pane.active_section,
                pane.matching_settings_sections(cx)
            )),
            (7, vec![7])
        );
        cx.update(|window, cx| {
            pane.update(cx, |pane, cx| {
                pane.settings_search_input
                    .update(cx, |input, cx| input.replace_all("", window, cx));
            })
        });
        cx.run_until_parked();
        assert_eq!(pane.read_with(cx, |pane, _| pane.active_section), 1);
    }
}
