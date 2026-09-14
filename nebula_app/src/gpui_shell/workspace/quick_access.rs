//! 侧栏「快速访问」区：常驻在标签页之上的一组 quick-launch 入口。
//!
//! 用户给项目建的 profile 不该只藏在 Ctrl+K 之后，也不该靠 `Ctrl+Shift+数字`
//! 盲选——资源管理器的「快速访问」就是常驻可见的，这里照那个心智做。
//!
//! 数据源与 Ctrl+K 选择器**同一份**（`TerminalProfiles::load()`），避免两处
//! 出现不同的口径——那正是这个仓库反复踩过的坑（见 `shell_picker.rs` 与设置页
//! 下拉的两条装载路径）。

use super::*;

/// 读一次 store，转成侧栏与选择器共用的 `Profile` 形状。
///
/// 单独成函数是为了让刷新点只有一处：渲染路径**不允许**调它（侧栏是逐帧
/// 重绘的热路径，每帧解析一遍 JSON 不可接受）。
pub(super) fn load_quick_access_profiles() -> Vec<crate::config::ui_config::Profile> {
    crate::terminal_profiles::TerminalProfiles::load()
        .map(|store| store.as_config_profiles())
        .unwrap_or_default()
}

/// 由一个选中的目录造一条 profile。
///
/// 选到 `\\wsl.localhost\<发行版>\…`（目录选择器会把 WSL 发行版钉进侧栏，
/// 见 issue #12）就造成 **WSL 入口**：`command` 是 `wsl.exe`、`args` 带
/// `-d <发行版> --cd <来宾路径>`、`shell_id` 是 `wsl:<发行版>` —— 图标因此
/// 自动落到那个发行版的品牌贴图（Ubuntu 圆标），而不是通用的企鹅或终端图标。
///
/// 其余情况用调用方给的默认 shell，`cwd` 就是选中的宿主目录。
fn quick_access_profile_for(
    directory: &std::path::Path,
    default_shell: Option<(String, std::path::PathBuf)>,
) -> Option<crate::terminal_profiles::TerminalProfile> {
    // 目录名做显示名；盘根或来宾根拿不到名字时退回整条路径，总比空着强。
    let name = directory
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| directory.to_string_lossy().into_owned());

    if let Some((distro, guest)) = crate::shell_detect::wsl_guest_path_from_unc(directory) {
        return Some(crate::terminal_profiles::TerminalProfile {
            // id 只要求非空且在本 store 内唯一；UNC 原文就是天然的稳定键，
            // 同一个目录再加一次会走 `add` 的同 id 替换而不是多出一行。
            id: format!("qa-wsl-{}", directory.to_string_lossy().to_ascii_lowercase()),
            name,
            command: std::path::PathBuf::from("wsl.exe"),
            args: vec!["-d".to_owned(), distro.clone(), "--cd".to_owned(), guest],
            // 宿主侧没有对应目录，`cwd` 留空——目录由 `--cd` 带给来宾。
            cwd: None,
            shell_id: format!("wsl:{distro}"),
        });
    }

    let (shell_id, program) = default_shell?;
    Some(crate::terminal_profiles::TerminalProfile {
        id: format!("qa-{}", directory.to_string_lossy().to_ascii_lowercase()),
        name,
        command: program,
        args: Vec::new(),
        cwd: Some(directory.to_path_buf()),
        shell_id,
    })
}

impl NebulaWorkspace {
    /// 刷新「快速访问」的行快照。初始化与 `TerminalProfilesChanged` 各调一次。
    pub(super) fn refresh_quick_access(&mut self) {
        self.quick_access = load_quick_access_profiles();
    }

    /// 删掉一条 profile 并落盘，然后让侧栏与 Ctrl+K 选择器同时刷新。
    ///
    /// 不拦截「有 tab 正在用这条 profile」的情况：`LaunchSession::Profile` 是
    /// 启动快照（内嵌而非按 id 引用），已经开出去的终端不受影响。
    pub(super) fn remove_quick_access_profile(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut store = match crate::terminal_profiles::TerminalProfiles::load() {
            Ok(store) => store,
            Err(error) => {
                log::warn!("quick access: 读取 terminal profiles 失败: {error}");
                return;
            },
        };
        if !store.remove(id) {
            return;
        }
        if let Err(error) = store.save() {
            log::warn!("quick access: 写入 terminal profiles 失败: {error}");
            return;
        }
        self.refresh_quick_access();
        self.refresh_shell_if_open(window, cx);
        cx.notify();
    }

    /// 分区标题右侧的 `+`：选一个目录，为它建一条入口。
    ///
    /// 目录选择器与设置页「导入终端目录」用的是同一支（WSL 发行版钉在侧栏），
    /// 所以用户能直接浏览进 `\\wsl.localhost\<发行版>\…` 选项目目录——
    /// [`finish_quick_access_add`] 会把它转成正经的 WSL 入口。
    pub(super) fn add_quick_access_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = crate::gpui_shell::config::ui_language(cx);
        let title = language.pick("选择项目目录", "Select a project directory");

        #[cfg(windows)]
        let picked =
            crate::gpui_shell::settings_pane::shell_picker::pick_folder_with_wsl_places(window, title);
        #[cfg(not(windows))]
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(title.into()),
        });

        cx.spawn_in(window, async move |this, cx| {
            #[cfg(windows)]
            let Ok(Some(directory)) = picked.await else {
                return;
            };
            #[cfg(not(windows))]
            let directory = {
                let Ok(Ok(Some(paths))) = picked.await else {
                    return;
                };
                let Some(directory) = paths.into_iter().next() else {
                    return;
                };
                directory
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.finish_quick_access_add(directory, window, cx);
            });
        })
        .detach();
    }

    /// 把选好的目录变成一条 profile 并落盘，然后让侧栏与选择器同时刷新。
    fn finish_quick_access_add(
        &mut self,
        directory: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 宿主目录的入口用「当前默认 shell」——用户没为这个目录特别指定过
        // 用什么终端，默认 shell 就是他的答案。
        let default_shell_id = crate::platform::shell::effective_shell_id(
            cx.try_global::<crate::gpui_shell::config::Settings>()
                .and_then(|settings| settings.shell_id.as_deref()),
        );
        let default_shell = crate::shell_detect::detect_shells()
            .into_iter()
            .find(|shell| shell.id == default_shell_id)
            .map(|shell| (shell.id, std::path::PathBuf::from(shell.program)));

        let Some(profile) = quick_access_profile_for(&directory, default_shell) else {
            log::warn!("quick access: 无法为 {directory:?} 生成 profile");
            return;
        };
        let mut store = match crate::terminal_profiles::TerminalProfiles::load() {
            Ok(store) => store,
            Err(error) => {
                log::warn!("quick access: 读取 terminal profiles 失败: {error}");
                return;
            },
        };
        if let Err(error) = store.add(profile).and_then(|()| store.save()) {
            log::warn!("quick access: 写入 terminal profiles 失败: {error}");
            return;
        }
        self.refresh_quick_access();
        self.refresh_shell_if_open(window, cx);
        cx.notify();
    }

    /// 「快速访问」区。行高与标签页行一致——两列内容上下相邻，行高不同一眼就看出来。
    pub(super) fn render_quick_access(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.quick_access.is_empty() {
            // 没有 profile 就整区不渲染：留一个空标题只占地方。
            return div().into_any_element();
        }
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let hover_bg = theme.list_hover;
        let faint = crate::gpui_shell::theme::faint_ink(cx);
        let collapsed = self.quick_access_collapsed;

        let header = h_flex()
            .id("sidebar-quick-access-toggle")
            .w_full()
            .h(px(34.0))
            .pb_1()
            .gap_1()
            .items_center()
            .cursor_pointer()
            .child(
                // 折叠槽与 TABS 标题行同宽，两行箭头对齐。共用同一个常量而不是
                // 各写一份——分叉的那天就是两行箭头错位的那天。
                div()
                    .w(px(super::sidebar::TABS_DISCLOSURE_SLOT_W))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .child(
                        Icon::new(if collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        })
                        .with_size(px(SIDEBAR_HEADER_ICON))
                        .text_color(muted),
                    ),
            )
            .child(
                div()
                    .text_size(px(14.0 * SIDEBAR_TITLE_SCALE))
                    .text_color(muted)
                    .child(workspace_ui_language().pick("快速访问", "Quick access")),
            )
            .child(
                div()
                    .ml_auto()
                    .px_1()
                    .rounded_full()
                    .bg(theme.muted)
                    .text_size(px(11.0))
                    .text_color(faint)
                    .child(SharedString::from(self.quick_access.len().to_string())),
            )
            .child(
                // 标题行里唯一的行动点，与计数同侧、靠最右。
                div()
                    .id("sidebar-quick-access-add")
                    .w(px(SIDEBAR_MENU_W))
                    .h(px(SIDEBAR_PLUS_SIZE))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(muted)
                    .hover(|button| button.bg(hover_bg).text_color(theme.foreground))
                    .tooltip(|window, cx| {
                        gpui_component::tooltip::Tooltip::new(
                            workspace_ui_language()
                                .pick("添加项目目录", "Add a project directory"),
                        )
                        .build(window, cx)
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        // 标题行本身点了要折叠，`+` 不能被那次点击吃掉。
                        cx.stop_propagation();
                        this.add_quick_access_profile(window, cx);
                    }))
                    .child(Icon::new(IconName::Plus).with_size(px(SIDEBAR_HEADER_ICON))),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.quick_access_collapsed = !this.quick_access_collapsed;
                cx.notify();
            }));

        let mut section = v_flex().w_full().flex_shrink_0().gap_1().child(header);
        if collapsed {
            return section.into_any_element();
        }

        // 先克隆一份再遍历：`cx.listener` 要可变借用 `cx`，不能与
        // `self.quick_access` 的借用同时存在。
        let mut rows: Vec<gpui::AnyElement> = Vec::new();
        for profile in self.quick_access.clone() {
            // 图标口径与 Ctrl+K 选择器一致：`shell_id` 决定品牌贴图，没有贴图时
            // 回落 Nerd Font 字形。WSL 的发行版名就在 shell_id 里
            // （`wsl:Ubuntu`），所以 Ubuntu 项目拿到的就是 Ubuntu 圆标。
            let id = profile.settings_id().unwrap_or_default();
            let icon_id = profile.shell_id.clone().unwrap_or_else(|| id.clone());
            let icon = crate::gpui_shell::widgets::shell_brand_image(
                &icon_id,
                SIDEBAR_HEADER_ICON,
                1.0,
            );
            let glyph = super::shell_picker::fallback_shell_glyph(&icon_id, icon.is_some());
            // 右侧显示目录而不是可执行文件——这一列回答的是「开在哪儿」，
            // 与选择器里的同一列保持同一口径。
            let detail = profile
                .cwd
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| profile.command.clone());

            // 每行一个独立的 hover 作用域：共用一个名字会让所有行的 × 一起显隐。
            let hover_group: SharedString = format!("qa-row-hover-{id}").into();
            let delete_id = id.clone();
            rows.push(
                h_flex()
                    .id(SharedString::from(format!("qa-row-{id}")))
                    .relative()
                    .group(hover_group.clone())
                    .w_full()
                    .h(px(TAB_ROW_H))
                    .px_1()
                    .gap_2()
                    .items_center()
                    .rounded_md()
                    .text_color(muted)
                    .hover(|row| row.bg(hover_bg).text_color(theme.foreground))
                    .child(
                        div()
                            .w(px(SIDEBAR_HEADER_ICON))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when_some(icon, |slot, image| {
                                slot.child(
                                    gpui::StyledImage::object_fit(
                                        gpui::img(image).size(px(SIDEBAR_HEADER_ICON)),
                                        gpui::ObjectFit::Contain,
                                    )
                                    .into_any_element(),
                                )
                            })
                            .when_some(glyph, |slot, glyph| {
                                // Nerd Font 字位固定走随安装包提供的 Maple，不受
                                // 用户终端字体影响——侧栏几何不能被字体换掉。
                                slot.child(
                                    div()
                                        .font_family(crate::font_install::REQUIRED_FONT_FAMILY)
                                        .text_size(px(SIDEBAR_HEADER_ICON * 0.88))
                                        .text_color(theme.foreground)
                                        .child(glyph.to_string()),
                                )
                            }),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(profile.name.clone()))
                    .child(
                        div()
                            .max_w(px(120.0))
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.0))
                            .text_color(faint)
                            .child(detail),
                    )
                    .child(
                        // 删除按钮浮在目录文字之上、靠右，只有指针停在这一行时
                        // 才出现——常驻会跟右列的文字抢注意力。
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .justify_end()
                            .items_center()
                            .pr_1()
                            .invisible()
                            .group_hover(hover_group.clone(), |slot| slot.visible())
                            .child(
                                Button::new(SharedString::from(format!("qa-del-{id}")))
                                    .icon(IconName::Close)
                                    .ghost()
                                    .xsmall()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.remove_quick_access_profile(
                                            &delete_id, window, cx,
                                        );
                                    })),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        // 走弹窗那条同一路径：`profile.cwd` 在
                        // `terminal_launch_from_session` 里优先于传入的 cwd，
                        // 所以点这一行落的是它自己配的目录，不是当前 pane 的。
                        this.launch_palette_profile(profile.clone(), window, cx);
                    }))
                    .into_any_element(),
            );
        }

        section = section.children(rows);
        section.into_any_element()
    }
}
