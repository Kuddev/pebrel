//! Opt-in real PTY fixture. Launch only isolated QA programs; never submit a model request.
use super::*;
use gpui::{
    Entity, EntityInputHandler as _, Modifiers, PlatformInput, WindowBounds, WindowOptions, size,
};
use std::path::PathBuf;
use std::time::Duration;

#[derive(serde::Deserialize)]
struct Launch {
    program: String,
    args: Vec<String>,
}

#[derive(serde::Deserialize)]
struct Action {
    kind: String,
    #[serde(default)]
    column: f32,
    #[serde(default)]
    row: f32,
    #[serde(default)]
    shift: bool,
    #[serde(default)]
    text: String,
}

#[test]
#[ignore = "requires native desktop, isolated launch.json and PEBREL_MOUSE_QA_DIR/config"]
fn native_application_mouse_editor() {
    let output = PathBuf::from(std::env::var_os("PEBREL_MOUSE_QA_DIR").expect("QA directory"));
    assert!(output.is_absolute());
    assert_eq!(
        std::env::var_os("PEBREL_CONFIG_DIR").map(PathBuf::from),
        Some(output.join("config"))
    );
    assert!(!output.join("screen.json").exists(), "use a fresh QA directory");
    let launch: Launch =
        serde_json::from_slice(&std::fs::read(output.join("launch.json")).unwrap()).unwrap();
    assert!(std::path::Path::new(&launch.program).is_absolute());
    gpui_platform::application().with_assets(crate::gpui_shell::assets::NebulaAssets).run(move |cx| {
        crate::gpui_shell::register_bundled_fonts(cx);
        gpui_component::init(cx);
        crate::gpui_shell::terminal::init(cx);
        crate::gpui_shell::scientific_render::init(cx);
        cx.set_global(Settings::load(nebula_settings::ThemeName::Nord));
        crate::gpui_shell::theme::apply_chrome_theme(cx);
        let mut terminal = None;
        let handle = cx.open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(40.0), px(40.0)), size(px(1000.0), px(700.0)),
            ))),
            focus: true,
            ..Default::default()
        }, |window, cx| {
            let view = cx.new(|cx| TerminalView::new(
                9900, (100, 35), TerminalLaunch::Local {
                    cwd: Some(output.clone()),
                    shell: Some(nebula_terminal::tty::Shell::new(launch.program, launch.args)),
                    shell_name: None,
                }, window, cx,
            ));
            window.focus(&view.read(cx).focus_handle.clone(), cx);
            terminal = Some(view.clone());
            cx.new(|cx| gpui_component::Root::new(view, window, cx))
        }).unwrap();
        let view = terminal.unwrap();
        cx.spawn(async move |cx| {
            let mut next = 0;
            let mut completed = false;
            for _ in 0..1200 {
                cx.background_executor().timer(Duration::from_millis(500)).await;
                let path = output.join(format!("action-{next}.json"));
                let action = cx.background_executor().spawn(async move {
                    std::fs::read(path).ok().map(|data| serde_json::from_slice::<Action>(&data).unwrap())
                }).await;
                if let Some(action) = action {
                    if action.kind == "quit" {
                        completed = true;
                        break;
                    }
                    cx.update_window(handle.into(), |_, window, cx| {
                        dispatch(&view, action, window, cx);
                        window.refresh();
                    }).unwrap();
                    next += 1;
                }
                let snapshot = cx.update_window(handle.into(), |_, window, cx| {
                    let _ = window.draw(cx);
                    let view = view.read(cx);
                    let session = view.session.as_ref().expect("QA PTY must start");
                    let term = session.term.lock();
                    let rows: Vec<String> = (0..term.screen_lines()).map(|row| {
                        let mut text = String::new();
                        for column in 0..term.columns() {
                            let cell = &term.grid()[Line(row as i32)][Column(column)];
                            if cell.flags.contains(nebula_terminal::term::cell::Flags::WIDE_CHAR_SPACER) {
                                continue;
                            }
                            text.push(cell.c);
                            if let Some(zero_width) = cell.zerowidth() {
                                text.extend(zero_width.iter());
                            }
                        }
                        text.trim_end().to_owned()
                    }).collect();
                    serde_json::json!({
                        "next_action": next, "rows": rows,
                        "origin": [view.origin.x.as_f32(), view.origin.y.as_f32()],
                        "cell": [view.cell_width.as_f32(), view.line_height.as_f32()],
                        "grid": [view.cols, view.rows], "mode": term.mode().bits(),
                        "mouse_generation": term.mouse_input_generation(),
                        "cursor": [term.grid().cursor.point.column.0, term.grid().cursor.point.line.0],
                        "gesture_active": view.application_mouse.is_some(),
                    })
                }).unwrap();
                let path = output.join("screen.json");
                cx.background_executor().spawn(async move {
                    let tmp = path.with_extension("tmp");
                    std::fs::write(&tmp, serde_json::to_vec_pretty(&snapshot).unwrap()).unwrap();
                    std::fs::rename(tmp, path).unwrap();
                }).await;
            }
            cx.update_window(handle.into(), |_, _, cx| view.read(cx).shutdown()).unwrap();
            cx.background_executor().timer(Duration::from_millis(250)).await;
            assert!(completed, "native QA controller timed out");
            cx.update(|cx| cx.quit());
        }).detach();
    });
}

fn dispatch(view: &Entity<TerminalView>, action: Action, window: &mut Window, cx: &mut App) {
    let terminal = view.read(cx);
    let position = point(
        terminal.origin.x + terminal.cell_width * (action.column + 0.5),
        terminal.origin.y + terminal.line_height * (action.row + 0.5),
    );
    let modifiers = Modifiers { shift: action.shift, ..Modifiers::default() };
    let event = match action.kind.as_str() {
        "down" => PlatformInput::MouseDown(MouseDownEvent {
            position,
            modifiers,
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        }),
        "move" => PlatformInput::MouseMove(MouseMoveEvent {
            position,
            modifiers,
            pressed_button: Some(MouseButton::Left),
        }),
        "up" => PlatformInput::MouseUp(MouseUpEvent {
            position,
            modifiers,
            button: MouseButton::Left,
            click_count: 1,
        }),
        "key" => PlatformInput::KeyDown(KeyDownEvent {
            keystroke: gpui::Keystroke::parse(&action.text).unwrap(),
            is_held: false,
            prefer_character_input: false,
        }),
        "text" => {
            view.update(cx, |view, cx| view.replace_text_in_range(None, &action.text, window, cx));
            return;
        },
        _ => panic!("unknown QA action"),
    };
    window.dispatch_event(event, cx);
}
