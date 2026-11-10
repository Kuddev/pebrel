//! SFTP 面板的拖入遮罩：拖着本地文件经过面板时，压暗当前列表、铺一层中心
//! 渐隐的点阵，并用虚线框写明“松开会传到哪里”。
//!
//! 遮罩的 `drag_over` 注册普通命中区，拖走或松手时自然消失。它不阻挡
//! 下层命中区，也不接管投放；面板与各目录行原有的 `on_drop` 照常接收。
//! 落在某个目录行上时，那一行的高亮透过半透明底可见，
//! 上传目标以行为准。GPUI 没有背景模糊，用半透明窗框色压暗代替。

use super::*;
use gpui::{Bounds, canvas, fill, point, size};

const DOT_PITCH: f32 = 14.0;
const FRAME_INSET: f32 = 10.0;

impl NebulaWorkspace {
    pub(super) fn render_remote_drop_overlay(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let destination = if self.remote_browser.path.is_empty() {
            self.remote_browser.destination.clone()
        } else {
            format!("{}:{}", self.remote_browser.destination, self.remote_browser.path)
        };
        remote_drop_overlay(destination, cx)
    }
}

fn remote_drop_overlay(destination: String, cx: &App) -> gpui::AnyElement {
    let theme = cx.theme();
    let accent = theme.primary;
    let language = crate::gpui_shell::config::ui_language(cx);
    let mut veil = theme.background;
    veil.a = 0.6;
    let dot = accent.opacity(0.34);
    let lift = crate::gpui_shell::theme::raised_fill(cx);
    let lift_shadow = crate::gpui_shell::theme::settings_card_shadow(cx);

    let overlay = div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .invisible()
        .bg(veil)
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    // 点阵从中心向外渐隐：只在框内铺，边缘让给面板原有内容。
                    let center = bounds.center();
                    let reach = f32::from(bounds.size.width.max(bounds.size.height)) * 0.5;
                    let cols = (f32::from(bounds.size.width) / DOT_PITCH) as i32;
                    let rows = (f32::from(bounds.size.height) / DOT_PITCH) as i32;
                    for row in 0..=rows {
                        for col in 0..=cols {
                            let x = bounds.origin.x + px(col as f32 * DOT_PITCH + DOT_PITCH / 2.0);
                            let y = bounds.origin.y + px(row as f32 * DOT_PITCH + DOT_PITCH / 2.0);
                            let dx = f32::from(x - center.x);
                            let dy = f32::from(y - center.y);
                            let fade = 1.0 - ((dx * dx + dy * dy).sqrt() / reach).clamp(0.0, 1.0);
                            if fade <= 0.05 {
                                continue;
                            }
                            window.paint_quad(
                                fill(
                                    Bounds::new(
                                        point(x - px(1.0), y - px(1.0)),
                                        size(px(2.0), px(2.0)),
                                    ),
                                    dot.opacity(fade),
                                )
                                .corner_radii(gpui::Corners::all(px(1.0))),
                            );
                        }
                    }
                },
            )
            .absolute()
            .size_full(),
        )
        .child(
            v_flex()
                .debug_selector(|| "remote-files-drop-target".into())
                .absolute()
                .top(px(FRAME_INSET))
                .left(px(FRAME_INSET))
                .right(px(FRAME_INSET))
                .bottom(px(FRAME_INSET))
                .items_center()
                .justify_center()
                .gap(px(10.0))
                .rounded(px(12.0))
                .border_1()
                .border_dashed()
                .border_color(accent.opacity(0.7))
                .bg(accent.opacity(0.05))
                .child(
                    div()
                        .size(px(44.0))
                        .rounded(px(12.0))
                        .bg(lift)
                        .shadow(lift_shadow)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(accent)
                        .child(Icon::new(IconName::ArrowUp).size(px(22.0))),
                )
                .child(
                    div()
                        .text_color(theme.foreground)
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(language.text(crate::i18n::Message::TransferUploadHint)),
                )
                .child(
                    div()
                        .max_w_full()
                        .px(px(16.0))
                        .truncate()
                        .font_family(theme.mono_font_family.clone())
                        .text_xs()
                        .text_color(accent)
                        .child(destination),
                ),
        );
    overlay
        .drag_over::<ExternalPaths>(|style, _, _, _| style.visible())
        .drag_over::<crate::gpui_shell::file_drop::FileTreeDrag>(|style, _, _, _| style.visible())
        .into_any_element()
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use crate::gpui_shell::file_drop::{FileDragGhost, FileTreeDrag};
    use gpui::{
        AppContext as _, FileDropEvent, InteractiveElement as _, IntoElement, Modifiers,
        MouseButton, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
        TestAppContext, VisualTestContext,
    };
    use gpui_component::Root;

    struct DropHost {
        folder_drops: usize,
        background_drops: usize,
    }

    impl Render for DropHost {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            h_flex()
                .size_full()
                .child(
                    div()
                        .id("drop-source")
                        .debug_selector(|| "drop-source".into())
                        .w(px(180.0))
                        .h(px(80.0))
                        .child("example.txt")
                        .on_drag(
                            FileTreeDrag {
                                local_path: "/tmp/example.txt".into(),
                                path_text: "/tmp/example.txt".into(),
                                name: "example.txt".into(),
                            },
                            |_, _, _, cx| cx.new(|_| FileDragGhost::new("example.txt".into())),
                        ),
                )
                .child(
                    div()
                        .id("drop-panel")
                        .relative()
                        .w(px(300.0))
                        .h(px(320.0))
                        .bg(cx.theme().background)
                        .on_drop(cx.listener(|this, _: &ExternalPaths, _, cx| {
                            this.background_drops += 1;
                            cx.stop_propagation();
                        }))
                        .on_drop(cx.listener(|this, _: &FileTreeDrag, _, cx| {
                            this.background_drops += 1;
                            cx.stop_propagation();
                        }))
                        .child(
                            div()
                                .id("drop-folder")
                                .debug_selector(|| "drop-folder".into())
                                .w_full()
                                .h(px(48.0))
                                .child("folder")
                                .on_drop(cx.listener(|this, _: &ExternalPaths, _, cx| {
                                    this.folder_drops += 1;
                                    cx.stop_propagation();
                                }))
                                .on_drop(cx.listener(|this, _: &FileTreeDrag, _, cx| {
                                    this.folder_drops += 1;
                                    cx.stop_propagation();
                                })),
                        )
                        .child(remote_drop_overlay("test@example.invalid:/uploads".into(), cx)),
                )
        }
    }

    fn draw(window: &mut VisualTestContext) {
        window.run_until_parked();
        window.update(|window, cx| window.draw(cx).clear(cx));
    }

    #[test]
    #[ignore = "native drag overlay review; requires PEBREL_CONFIG_DIR and PEBREL_DROP_QA_DIR"]
    fn native_drop_overlay_visual_review() {
        use gpui::InputEvent;
        use std::time::Duration;
        let output = PathBuf::from(std::env::var_os("PEBREL_DROP_QA_DIR").expect("QA output"));
        assert!(output.is_absolute());
        std::fs::create_dir_all(&output).unwrap();
        gpui_platform::application().with_assets(crate::gpui_shell::NebulaAssets).run(move |cx| {
            crate::gpui_shell::init(cx, Default::default());
            let handle = cx
                .open_window(
                    gpui::WindowOptions {
                        window_bounds: Some(gpui::WindowBounds::Windowed(Bounds::new(
                            point(px(30.0), px(20.0)),
                            size(px(600.0), px(400.0)),
                        ))),
                        ..Default::default()
                    },
                    |window, cx| {
                        let host = cx.new(|_| DropHost { folder_drops: 0, background_drops: 0 });
                        cx.new(|cx| Root::new(host, window, cx))
                    },
                )
                .unwrap();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(Duration::from_millis(700)).await;
                cx.update_window(handle.into(), |_, window, cx| {
                    let target = window
                        .debug_bounds_for_selector("drop-folder")
                        .expect("rendered folder target")
                        .center();
                    window.dispatch_event(
                        FileDropEvent::Entered {
                            position: target,
                            paths: ExternalPaths(
                                [PathBuf::from("/tmp/example.txt")].into_iter().collect(),
                            ),
                        }
                        .to_platform_input(),
                        cx,
                    );
                    window.refresh();
                })
                .unwrap();
                cx.background_executor().timer(Duration::from_millis(500)).await;
                cx.update_window(handle.into(), |_, window, _| {
                    assert!(window.debug_bounds_for_selector("remote-files-drop-target").is_some());
                })
                .unwrap();
                std::fs::write(output.join("ready"), b"ready").unwrap();
                for _ in 0..120 {
                    if output.join("capture-complete").exists() {
                        break;
                    }
                    cx.background_executor().timer(Duration::from_millis(500)).await;
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    }

    #[gpui::test]
    fn overlay_tracks_drag_entry_exit_and_leaves_folder_drops_reachable(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let mut host = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| DropHost { folder_drops: 0, background_drops: 0 });
            host = Some(view.clone());
            Root::new(view, window, cx)
        });
        let host = host.unwrap();
        let mut window = (*window).clone();
        window.simulate_resize(size(px(600.0), px(400.0)));
        draw(&mut window);
        assert!(window.debug_bounds("remote-files-drop-target").is_none());
        let target = window.debug_bounds("drop-folder").unwrap().center();
        for external in [true, false] {
            if external {
                window.simulate_event(FileDropEvent::Entered {
                    position: target,
                    paths: ExternalPaths([PathBuf::from("/tmp/example.txt")].into_iter().collect()),
                });
            } else {
                let source = window.debug_bounds("drop-source").unwrap().center();
                window.simulate_mouse_down(source, MouseButton::Left, Modifiers::default());
                window.simulate_mouse_move(target, Some(MouseButton::Left), Modifiers::default());
            }
            draw(&mut window);
            assert!(
                window.debug_bounds("remote-files-drop-target").is_some(),
                "external={external}"
            );
            window.simulate_mouse_move(
                point(px(550.0), px(360.0)),
                Some(MouseButton::Left),
                Modifiers::default(),
            );
            draw(&mut window);
            assert!(window.debug_bounds("remote-files-drop-target").is_none());
            window.simulate_mouse_move(target, Some(MouseButton::Left), Modifiers::default());
            draw(&mut window);
            assert!(window.debug_bounds("remote-files-drop-target").is_some());
            window.simulate_mouse_up(target, MouseButton::Left, Modifiers::default());
            draw(&mut window);
            assert!(window.debug_bounds("remote-files-drop-target").is_none());
        }
        host.read_with(&window, |host, _| {
            assert_eq!(host.folder_drops, 2);
            assert_eq!(host.background_drops, 0);
        });
        window.simulate_event(FileDropEvent::Entered {
            position: target,
            paths: ExternalPaths([PathBuf::from("/tmp/example.txt")].into_iter().collect()),
        });
        draw(&mut window);
        window.simulate_event(FileDropEvent::Exited);
        draw(&mut window);
        assert!(window.debug_bounds("remote-files-drop-target").is_none());
        assert_eq!(host.read_with(&window, |host, _| host.folder_drops), 2);
    }
}
