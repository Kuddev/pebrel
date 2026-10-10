//! Root composition and transient overlays; workspace state remains in the parent.
use super::*;

impl Render for NebulaWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme_transition = self.theme_transition.render(window, cx);
        crate::gpui_shell::theme::sync_component_focus_ring(window, cx);
        if window.is_window_active() {
            windowing::mark_active(self.runtime_window_id, cx);
        }
        if !cx.has_active_drag() {
            self.cross_window_dock = None;
        }
        // 终端卡几何取一次，布局与壳色带共用同一个实例——两处各取一次也算
        // 「各写一份」，主题在这一帧中途换掉就会出现半旧半新的卡缝。
        let card_style = crate::gpui_shell::theme::PaneCardStyle::current(cx);
        let titlebar_background = self.titlebar_background.clone();
        let draw_file_divider = self.side_panel.open;
        let sidebar_logo_target_px =
            (TAB_LABEL_ICON_SIZE * window.scale_factor()).round().max(1.0) as u32;
        if let Some(images) = logos::poll_sidebar_logo_images(self, sidebar_logo_target_px, cx) {
            // GPUI 窗口可跨不同 DPI 的显示器；原纹理只在整数物理像素尺寸
            // 变化时重建，普通 render 不重复解码 PNG。
            self.sidebar_logo_images = images;
            self.sidebar_logo_target_px = sidebar_logo_target_px;
        }
        // Some tab-open/restore paths assign `active` directly. Clear a focus
        // record tied to a different entity before deriving layout booleans.
        self.clear_stale_reader_focus(cx);
        self.sync_document_activity(window, cx);
        self.sync_terminal_activity(cx);
        self.sync_document_details(cx);
        if !self.sidebar_collapsed {
            self.sidebar_closing_width = None;
        }
        let content: Option<gpui::AnyElement> = if self.settings_open {
            self.settings_surface
                .as_ref()
                .map(|(view, _)| gpui::IntoElement::into_any_element(view.clone()))
        } else {
            match self.tabs.get(self.active) {
                Some(WorkspaceTab::Terminal { .. }) => {
                    Some(self.render_terminal_tab(self.active, cx))
                },
                Some(WorkspaceTab::Settings { view, .. }) => {
                    Some(gpui::IntoElement::into_any_element(view.clone()))
                },
                Some(WorkspaceTab::Image { view }) => {
                    Some(gpui::IntoElement::into_any_element(view.clone()))
                },
                Some(WorkspaceTab::Document { view, .. }) => {
                    Some(gpui::IntoElement::into_any_element(view.clone()))
                },
                Some(WorkspaceTab::Code { view, .. }) => {
                    Some(gpui::IntoElement::into_any_element(view.clone()))
                },
                None => None,
            }
        };
        let settings_active = self.settings_open;
        let top_tabs = self.tabs_position == nebula_settings::TabsPositionName::Top;
        let reader_focus = self.reader_focus_active(cx);
        // dock 预览：被拖 tab 悬于终端区时高亮目标半区（松手即挂到那侧）。
        let dock_preview = self
            .tab_drag
            .as_ref()
            .filter(|drag| drag.active)
            .and_then(|drag| drag.dock)
            .or(self.cross_window_dock)
            .and_then(|target| self.dock_preview_area(target))
            .map(|area| (area.x, area.y, area.w, area.h));

        div()
            .size_full()
            .flex()
            .flex_col()
            // 旧壳每个像素只画一次：整窗先透明清屏，再分别画标题栏、侧栏、
            // 卡外壳与卡底。根节点不能再铺一张半透明底。
            .bg(gpui::transparent_black())
            .text_color(cx.theme().foreground)
            .can_drop({
                let runtime_window_id = self.runtime_window_id;
                move |value, _, _| {
                    value
                        .downcast_ref::<windowing::CrossWindowTabDrag>()
                        .is_some_and(|payload| {
                            payload.source_window_id() != runtime_window_id
                        })
                }
            })
            .on_drag_move::<windowing::CrossWindowTabDrag>(cx.listener(
                |this,
                 event: &gpui::DragMoveEvent<windowing::CrossWindowTabDrag>,
                 _window,
                 cx| {
                    let payload = event.drag(cx);
                    if payload.source_window_id() == this.runtime_window_id {
                        this.cross_window_dock = None;
                        return;
                    }
                    let position = event.event.position;
                    this.cross_window_dock = this
                        .dock_nav_at(f32::from(position.x), f32::from(position.y));
                    cx.notify();
                },
            ))
            .on_drop(cx.listener(
                |this, payload: &windowing::CrossWindowTabDrag, window, cx| {
                    let dock = this.cross_window_dock.take();
                    if this.accept_cross_window_tab(payload, dock, window, cx) {
                        cx.stop_propagation();
                    }
                },
            ))
            .on_mouse_move(cx.listener(|this, event, window, cx| {
                this.update_details_panel_resize(event, window, cx);
                this.update_left_sidebar_resize(event, window, cx);
                this.continue_pending_tab_drag(event, window, cx);
                // pane 拖拽的待命态同理：罩层只在激活后才存在，越阈值那一下
                // 的 move 必须由根节点喂进去。
                this.continue_pending_pane_drag(event, cx);
            }))
            // 旧壳在窗口级 mouse-up 无条件结束 tab drag。这里必须走 capture：
            // TerminalView 可能在 bubble phase 消费释放，导致 dock 永远不提交。
            .capture_any_mouse_up(cx.listener(|this, event: &gpui::MouseUpEvent, window, cx| {
                if event.button != MouseButton::Left {
                    return;
                }
                if this.finish_details_panel_resize(cx) || this.finish_left_sidebar_resize(cx) {
                    cx.stop_propagation();
                    return;
                }
                // pane 拖拽先结算：它和 tab 拖拽互斥（起手位置不同），但待命态
                // 必须在这里清掉，否则下一次点标题条会带着上一次的按点。
                let pane_dragged = this.release_pane_drag(window, cx);
                if this.release_tab_drag_at(event.position, window, cx) || pane_dragged {
                    // 真拖拽已经完成，不能再让源 tab 的 click 或终端选择收到释放。
                    cx.stop_propagation();
                }
            }))
            .on_action(cx.listener(|this, _: &NewTerminal, window, cx| {
                this.add_terminal(window, cx);
            }))
            .on_action(cx.listener(|_, _: &NewWindow, _, cx| {
                cx.defer(|cx| {
                    if let Err(error) = windowing::open_new_window(cx, None) {
                        log::warn!("failed to open GPUI window: {error}");
                    }
                });
            }))
            .on_action(cx.listener(|this, _: &CloseActiveTerminal, window, cx| {
                this.close_active(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                if this.settings_open {
                    return;
                }
                this.sidebar_collapsed = !this.sidebar_collapsed;
                this.sidebar_fold_armed = !tab_reveal_instant(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &recipes::OpenLayoutRecipes, window, cx| {
                this.open_layout_recipes(window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                this.toggle_settings(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleCommandPalette, window, cx| {
                this.toggle_command_palette(window, cx);
            }))
            .on_action(cx.listener(|this, _: &CloseCommandPalette, window, cx| {
                this.close_command_palette(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleShellPicker, window, cx| {
                this.toggle_shell_palette(window, cx);
            }))
            .on_action(cx.listener(|this, _: &CommandPaletteUp, _, cx| {
                this.move_command_palette_selection(-1, cx);
            }))
            .on_action(cx.listener(|this, _: &CommandPaletteDown, _, cx| {
                this.move_command_palette_selection(1, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleFileTree, _, cx| {
                this.toggle_file_tree(cx);
            }))
            .on_action(cx.listener(|this, _: &SplitRight, window, cx| {
                let _ = this.split_focused(SplitDirection::LeftRight, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SplitDown, window, cx| {
                let _ = this.split_focused(SplitDirection::TopBottom, window, cx);
            }))
            .on_action(cx.listener(|this, _: &RenameActiveTab, window, cx| {
                let ix = this.active;
                this.begin_rename(ix, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleZoom, _, cx| {
                this.toggle_zoom(cx);
            }))
            .on_action(cx.listener(|this, _: &FocusPaneLeft, window, cx| {
                this.navigate_pane(SplitNav::Left, window, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusPaneRight, window, cx| {
                this.navigate_pane(SplitNav::Right, window, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusPaneUp, window, cx| {
                this.navigate_pane(SplitNav::Up, window, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusPaneDown, window, cx| {
                this.navigate_pane(SplitNav::Down, window, cx);
            }))
            .on_action(cx.listener(Self::select_tab))
            .on_action(cx.listener(|this, _: &SelectNextTab, window, cx| {
                this.select_adjacent_tab(true, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectPreviousTab, window, cx| {
                this.select_adjacent_tab(false, window, cx);
            }))
            .on_action(cx.listener(|this, _: &MoveTabLeft, window, cx| {
                this.move_active_tab(false, window, cx);
            }))
            .on_action(cx.listener(|this, _: &MoveTabRight, window, cx| {
                this.move_active_tab(true, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleGitPanel, _, cx| {
                this.toggle_git_tree(cx);
            }))
            .on_action(cx.listener(|this, _: &IncreaseFontSize, _, cx| {
                this.bump_font_size(1.0, cx);
            }))
            .on_action(cx.listener(|this, _: &DecreaseFontSize, _, cx| {
                this.bump_font_size(-1.0, cx);
            }))
            .on_action(cx.listener(|this, _: &ResetFontSize, _, cx| {
                this.bump_font_size(0.0, cx);
            }))
            .on_action(cx.listener(|this, _: &CopySelection, window, cx| {
                if !this.copy_focused_terminal(window, cx) {
                    // Copy 是条件动作：无有效选区时继续派发原始 KeyDownEvent，
                    // 让 Ctrl+C 等自定义组合键按终端原义进入 PTY。
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &PasteClipboard, window, cx| {
                this.paste_focused_terminal(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleFullscreen, window, _cx| {
                window.toggle_fullscreen();
            }))
            .on_action(cx.listener(|this, _: &OpenQuickJump, window, cx| {
                this.open_quick_jump_palette(window, cx);
            }))
            .child(
                // 用户显式配置的背景图画在 chrome 之下；系统 Mica/Aero/Acrylic
                // 位于整个 GPUI 内容层下方，由 DWM 合成，不能在这里读取壁纸仿画。
                // 拓展模式只在此绘图，壳/卡衬底在其上保留原有文字对比度。
                crate::gpui_shell::wallpaper::window_layer(cx),
            )
            .child(
                self.render_window_title_bar(
                    settings_active,
                    window,
                    cx,
                ),
            )
            .child(
                // 不用 h_flex：它默认 items_center，会把子项高度压成内容高度。
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .when(!top_tabs && !settings_active && !reader_focus, |row| {
                        row.child(self.render_sidebar_slot(window, cx))
                    })
                    .child(
                        // 终端卡（一体化外壳）：唯一的结构分界。圆角、卡缝、投影、
                        // 竖线四项全部来自 `PaneCardStyle`（主题默认叠用户覆盖），
                        // 所以「浮起的圆角卡」和「铺满到窗口边 + 一条竖线」是同一
                        // 条渲染路径的两组取值，不是两套代码。无描边——融合靠壳色
                        // 包围圆角卡本身，不靠线框。
                        //
                        // 上边距一律为零：侧栏 / 终端卡 / 右侧抽屉三列的顶边都贴
                        // chrome 下沿（用户 08-26 裁定「pane 和左侧 tab 抬到和文件树
                        // 顶部一致」），这条写在 `PaneCardStyle::resolve` 里。
                        div()
                            .flex_1()
                            .min_w_0()
                            .relative()
                            .child(
                                gpui::canvas(
                                    move |bounds, _, _| titlebar_background.record_pane(bounds),
                                    |bounds, _, window, cx| {
                                        // 卡缝、圆角、竖线全部读同一份 style 真源：
                                        // 布局的 padding 与这里的壳色带必须逐边对上，
                                        // 两边各写一份字面量就是那圈白边的来源——壳色
                                        // 若按四边对称推算，卡的上两个圆角外侧会漏
                                        // 覆盖，浅色主题下直接露出一道顶部白缝。
                                        let card =
                                            crate::gpui_shell::theme::PaneCardStyle::current(cx);
                                        crate::gpui_shell::theme::paint_shell_around_card(
                                            bounds,
                                            card.margin,
                                            window,
                                            cx,
                                        );
                                    },
                                )
                                .absolute()
                                .inset_0(),
                            )
                            .pt(px(card_style.margin.top))
                            .pr(px(card_style.margin.right))
                            .pb(px(card_style.margin.bottom))
                            .pl(px(card_style.margin.left))
                            .child(
                            div()
                                .size_full()
                                .rounded(px(card_style.radius))
                                .bg(crate::gpui_shell::theme::card_content_bg(cx))
                                .when(card_style.shadow, |card| {
                                    // 投影落在有圆角的这一层，才会跟着卡的形状走。
                                    // 父容器没有 overflow_hidden，所以 blur 可以溢出
                                    // 到卡缝之外——那正是「卡浮在壳上」的观感来源。
                                    card.shadow(vec![crate::gpui_shell::theme::card_shadow(cx)])
                                })
                                .overflow_hidden()
                                .child(
                                    // 壁纸层（卡底色之上、内容之下，覆盖整卡含
                                    // 内边距带）：卡模式按卡定位；拓展模式由
                                    // 窗口底层统一绘图，此处不覆盖原有衬底。
                                    crate::gpui_shell::wallpaper::card_layer(cx),
                                )
                                .children(content),
                        )
                            .child(
                                // Both sidebar boundaries share the same snapped line and color.
                                gpui::canvas(
                                    |_, _, _| (),
                                    move |bounds, _, window, cx| {
                                        window_titlebar::paint_pane_dividers(
                                            bounds, draw_file_divider, window, cx,
                                        );
                                    },
                                )
                                .absolute()
                                .inset_0(),
                            ),
                    )
                    .when(!reader_focus, |row| {
                        row.child(self.render_side_panel_slot(window, cx))
                    }),
            )
            .when_some(dock_preview, |root, (x, y, w, h)| {
                root.child(
                    div()
                        .absolute()
                        .left(px(x))
                        .top(px(y))
                        .w(px(w))
                        .h(px(h))
                        .rounded(crate::gpui_shell::theme::card_radius(cx))
                        // 旧壳是低透明青色水洗，不是高饱和实线框；描边只
                        // 提示落区边界，不能盖过终端内容成为视觉主体。
                        .border_1()
                        .border_color(cx.theme().primary.opacity(0.28))
                        .bg(cx.theme().primary.opacity(0.08)),
                )
            })
            .when(self.tab_drag.as_ref().is_some_and(|d| d.active), |root| {
                // 拖拽激活期间的全窗透明罩层：独占指针（occlude 挡掉下层
                // 命中），移动喂状态机、松开提交落位——等效指针捕获，指针
                // 划出侧栏甚至划到终端上都不会丢拖拽。
                root.child(
                    div()
                        .absolute()
                        .inset_0()
                        .occlude()
                        .on_mouse_move(cx.listener(|this, event, window, cx| {
                            this.update_tab_drag(event, window, cx);
                        }))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                this.finish_tab_drag(window, cx);
                            }),
                        ),
                )
            })
            .children(self.render_left_sidebar_resize_handle(cx))
            .children(self.render_details_panel_resize_handle(window, cx))
            .children(self.render_details_panel_resize_overlay(cx))
            .children(self.tabs_scrollbar_drag_overlay(cx))
            .children(self.pane_drag_overlay(cx))
            .children(self.split_drag_visual(cx))
            .children(self.render_left_sidebar_resize_overlay(cx))
            .when_some(
                self.split_drag.as_ref().map(|drag| drag.direction),
                |root, direction| {
                    // 分隔条拖拽罩层（同 tab_drag 的指针捕获模式）：整窗
                    // 保持 resize 光标，移动喂预览、松开提交/关闭。
                    root.child(
                        div()
                            .absolute()
                            .inset_0()
                            .occlude()
                            .map(|mask| match direction {
                                SplitDirection::LeftRight => mask.cursor_col_resize(),
                                SplitDirection::TopBottom => mask.cursor_row_resize(),
                            })
                            .on_mouse_move(cx.listener(|this, event, window, cx| {
                                this.update_split_drag(event, window, cx);
                            }))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, event: &gpui::MouseUpEvent, window, cx| {
                                    this.finish_split_drag(event.position, window, cx);
                                }),
                            ),
                    )
                },
            )
            .when(self.command_palette_open, |root| {
                root.child(self.render_command_palette(cx))
            })
            .when(self.command_manager_open, |root| {
                root.child(self.render_command_manager(window, cx))
            })
            .when_some(self.render_file_tree_context_menu(), |root, menu| {
                root.child(menu)
            })
            .when_some(self.render_tab_context_menu(), |root, menu| root.child(menu))
            .when_some(self.render_launcher_context_menu(), |root, menu| root.child(menu))
            .when_some(self.render_selection_context_menu(), |root, menu| {
                root.child(menu)
            })
            // 组件库的模态/通知层不会自己上屏：`Root::render` 只画宿主视图，
            // dialog/notification 两层由宿主显式挂。确认框需要晚于设置页中
            // priority 4–6 的主题浮层绘制；通知再覆盖确认框。
            //
            // 少了这两行，`window.open_dialog` 只会把模态推进 `Root` 并抢走
            // 焦点而不画任何东西——终端看着就像卡死了。
            .children(Root::render_dialog_layer(window, cx).map(|layer| {
                gpui::deferred(layer).with_priority(10)
            }))
            .children(crate::gpui_shell::toast::render_layer(window, cx).map(|layer| {
                gpui::deferred(layer).with_priority(11)
            }))
            .children(theme_transition)
    }
}
