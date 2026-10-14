//! Exercise the component itself, without a pairing manager or SSH endpoint.
use super::*;
use gpui::{Modifiers, TestAppContext, VisualTestContext, point};

#[test]
fn isolated_local_files_component() {
    let directory = tempfile::tempdir().unwrap();
    files_fixture::prepare(directory.path(), "en-US").unwrap();
    let output = files_fixture::child_command(directory.path())
        .args([
            "--exact",
            "gpui_shell::workspace::local_files_drawer::tests::owners_keep_independent_controls_and_cancel_stale_work",
            "--ignored",
            "--nocapture",
        ])
        .env("PEBREL_FILES_COMPONENT_DIR", directory.path())
        .output()
        .unwrap();
    let evidence = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success() && evidence.contains("1 passed;"), "{evidence}");
}

struct Host {
    owners: [Entity<TerminalView>; 2],
    drawers: [Entity<LocalFilesDrawer>; 2],
    selected: Option<usize>,
}

impl Host {
    fn select(&mut self, selected: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        for (index, drawer) in self.drawers.iter().enumerate() {
            drawer.update(cx, |drawer, cx| drawer.set_visible(selected == Some(index), window, cx));
        }
        self.selected = selected;
        cx.notify();
    }
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(
                h_flex().children(
                    [("source-a", Some(0)), ("source-b", Some(1)), ("hide-source", None)]
                        .into_iter()
                        .map(|(label, selected)| {
                            Button::new(label)
                                .debug_selector(move || label.into())
                                .label(label)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.select(selected, window, cx)
                                }))
                        }),
                ),
            )
            .when_some(self.selected, |panel, index| {
                panel.child(div().flex_1().min_h_0().child(self.drawers[index].clone()))
            })
    }
}

type State = (Option<PathBuf>, String, Option<PathBuf>, Vec<PathBuf>, usize, bool);

fn draw(window: &mut VisualTestContext) {
    window.run_until_parked();
    window.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn click(window: &mut VisualTestContext, selector: impl Into<String>) {
    let selector = Box::leak(selector.into().into_boxed_str());
    let center =
        window.debug_bounds(selector).unwrap_or_else(|| panic!("missing {selector}")).center();
    window.simulate_mouse_move(center, None, Modifiers::default());
    window.simulate_click(center, Modifiers::default());
    draw(window);
}

fn state(drawer: &Entity<LocalFilesDrawer>, window: &mut VisualTestContext) -> State {
    drawer.read_with(window, |drawer, cx| drawer.files_test_state(cx))
}

fn focus(drawer: &Entity<LocalFilesDrawer>, window: &mut VisualTestContext) -> (bool, bool) {
    window.update(|window, cx| drawer.read(cx).files_test_focus(window, cx))
}

fn wait(
    drawer: &Entity<LocalFilesDrawer>,
    window: &mut VisualTestContext,
    ready: impl Fn(&State) -> bool,
) -> State {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        window.executor().advance_clock(Duration::from_millis(100));
        draw(window);
        let current = state(drawer, window);
        if ready(&current) {
            return current;
        }
        assert!(std::time::Instant::now() < deadline, "component did not settle: {current:?}");
        // SidePanel snapshots/search run on real owned filesystem workers.
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn replace_input(window: &mut VisualTestContext, text: &str) {
    window.simulate_keystrokes(
        if crate::platform::Platform::current() == crate::platform::Platform::MacOS {
            "cmd-a"
        } else {
            "ctrl-a"
        },
    );
    window.simulate_input(text);
    draw(window);
}

#[gpui::test]
#[ignore = "launched by isolated_local_files_component with owned settings and filesystem"]
fn owners_keep_independent_controls_and_cancel_stale_work(cx: &mut TestAppContext) {
    let directory = PathBuf::from(std::env::var_os("PEBREL_FILES_COMPONENT_DIR").unwrap());
    files_fixture::assert_isolated(&directory);
    let roots = [directory.join("owner-a"), directory.join("owner-b")];
    let browsed = [roots[0].join("中文 folder a"), roots[1].join("中文 folder b")];
    for (index, prefix) in ["alpha", "beta"].into_iter().enumerate() {
        std::fs::create_dir_all(&browsed[index]).unwrap();
        std::fs::write(roots[index].join(format!("{prefix}-root-only.txt")), b"root").unwrap();
        for item in 0..100 {
            std::fs::write(browsed[index].join(format!("{prefix}-{item:03}.txt")), b"fixture")
                .unwrap();
        }
    }
    let roots = roots.map(|root| root.canonicalize().unwrap());
    let browsed = browsed.map(|root| PathBuf::from(display_path(&root.canonicalize().unwrap())));
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord));
        cx.set_reduce_motion(true);
    });
    let mut host = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let owners = std::array::from_fn(|index| {
            let owner = cx.new(|cx| {
                TerminalView::new(
                    index as u64 + 1,
                    (80, 24),
                    files_fixture::local_launch(&roots[index]),
                    window,
                    cx,
                )
            });
            owner.update(cx, |owner, _| {
                let _ = owner.install_completion_test_session();
            });
            owner
        });
        let drawers = std::array::from_fn(|index| {
            cx.new(|cx| LocalFilesDrawer::new(owners[index].downgrade(), window, cx))
        });
        let entity = cx.new(|_| Host { owners, drawers, selected: None });
        host = Some(entity.clone());
        Root::new(entity, window, cx)
    });
    let host = host.unwrap();
    let drawers = host.read_with(window, |host, _| host.drawers.clone());
    window.update(|window, _| window.activate_window());
    window.simulate_resize(size(px(700.0), px(500.0)));
    draw(window);
    assert!(window.debug_bounds("local-files-browser").is_none());
    let mut saved = Vec::new();
    for (index, label, query, delta) in
        [(0, "source-a", "alpha", -250.0), (1, "source-b", "beta", -500.0)]
    {
        click(window, label);
        let initial = wait(&drawers[index], window, |state| {
            state.0.as_ref() == Some(&roots[index]) && !state.5 && !state.3.is_empty()
        });
        assert!(initial.1.is_empty() && initial.2.is_none() && initial.4 == 0);
        assert!(initial.3.contains(&roots[index].join(format!("{query}-root-only.txt"))));
        assert!(!initial.3.iter().any(|path| path.starts_with(&roots[1 - index])));
        click(window, "local-files-path");
        assert_eq!(focus(&drawers[index], window), (true, false));
        replace_input(window, browsed[index].file_name().unwrap().to_str().unwrap());
        window.simulate_keystrokes("enter");
        wait(&drawers[index], window, |state| {
            state.0.as_ref() == Some(&browsed[index]) && !state.5 && state.3.len() >= 100
        });
        assert_eq!(focus(&drawers[index], window), (false, true));
        click(window, "local-files-path");
        replace_input(window, "missing-owned-directory");
        window.simulate_keystrokes("enter");
        draw(window);
        assert!(
            drawers[index]
                .read_with(window, |drawer, _| drawer.error.is_some() && drawer.editing.is_some())
        );
        assert_eq!(state(&drawers[index], window).0, Some(browsed[index].clone()));
        window.simulate_keystrokes("escape");
        draw(window);
        assert_eq!(focus(&drawers[index], window), (false, true));
        assert!(
            drawers[index]
                .read_with(window, |drawer, _| drawer.error.is_none() && drawer.editing.is_none())
        );
        click(window, "local-files-search");
        replace_input(window, query);
        let found = wait(&drawers[index], window, |state| {
            state.1 == query && !state.5 && state.3.len() == 100
        });
        assert!(found.3.iter().all(|path| path.parent() == Some(browsed[index].as_path())));
        let position = window.debug_bounds("local-files-list").unwrap().center();
        window.simulate_mouse_move(position, None, Modifiers::default());
        window.simulate_event(gpui::ScrollWheelEvent {
            position,
            delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(delta))),
            modifiers: Modifiers::default(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        draw(window);
        let scrolled = state(&drawers[index], window);
        assert!(scrolled.4 > 0);
        let row = scrolled.4 + 1;
        click(window, format!("local-file-row-{row}"));
        let selected = state(&drawers[index], window);
        assert_eq!(selected.2.as_ref(), Some(&selected.3[row]));
        saved.push(selected);
    }
    assert_ne!(saved[0].4, saved[1].4);
    for (index, label) in [(0, "source-a"), (1, "source-b")] {
        click(window, label);
        assert_eq!(wait(&drawers[index], window, |state| !state.5), saved[index]);
    }
    click(window, "local-files-search");
    click(window, "hide-source");
    assert_eq!(focus(&drawers[1], window), (false, false));
    window.simulate_input("hidden-search-input");
    draw(window);
    assert_eq!(state(&drawers[1], window), saved[1]);
    click(window, "source-b");
    click(window, "local-files-path");
    click(window, "hide-source");
    assert_eq!(focus(&drawers[1], window), (false, false));
    window.simulate_input("hidden-path-input");
    click(window, "source-b");
    click(window, "local-files-path-go");
    assert!(window.debug_bounds("local-files-path").is_some());
    assert_eq!(focus(&drawers[1], window), (false, true));
    assert_eq!(state(&drawers[1], window).0, saved[1].0);

    // Dispatch actual Enter and hide-button events without draining async work
    // between them: a real pending validation must be cancelled on hide.
    click(window, "source-a");
    click(window, "local-files-path");
    replace_input(window, roots[1].to_str().unwrap());
    window.update(|window, cx| {
        window.dispatch_keystroke(gpui::Keystroke::parse("enter").unwrap(), cx);
    });
    assert!(drawers[0].read_with(window, |drawer, _| drawer.validation.is_some()));
    let position = window.debug_bounds("hide-source").unwrap().center();
    window.update(|window, cx| {
        window.dispatch_event(
            gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                position,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                position,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
                click_count: 1,
            }),
            cx,
        );
    });
    assert!(drawers[0].read_with(window, |drawer, _| !drawer.model.open
        && drawer.validation.is_none()
        && drawer.polling.is_none()));
    draw(window);
    assert_eq!(state(&drawers[0], window).0, saved[0].0);
    click(window, "source-a");
    click(window, "local-files-path-input");
    window.simulate_keystrokes("escape");
    click(window, "local-files-path");
    replace_input(window, roots[1].to_str().unwrap());
    let next = directory.join("new-owner-cwd");
    std::fs::create_dir(&next).unwrap();
    std::fs::write(next.join("alpha-next.txt"), b"new origin").unwrap();
    let next = next.canonicalize().unwrap();
    window.update(|_, cx| {
        host.read(cx).owners[0].clone().update(cx, |owner, cx| {
            owner.cwd = next.display().to_string();
            cx.notify();
        })
    });
    wait(&drawers[0], window, |state| state.0.as_ref() == Some(&next) && !state.5);
    assert_eq!(focus(&drawers[0], window), (false, true));
    assert!(
        drawers[0]
            .read_with(window, |drawer, _| drawer.editing.is_none() && drawer.validation.is_none())
    );
    window.simulate_keystrokes("enter");
    draw(window);
    assert_eq!(state(&drawers[0], window).0, Some(next));
    click(window, "source-b");
    assert_eq!(state(&drawers[1], window).0, saved[1].0);
}
