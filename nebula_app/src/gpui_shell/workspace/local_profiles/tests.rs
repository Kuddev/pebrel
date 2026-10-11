use super::*;
use gpui::{TestAppContext, VisualTestContext};

struct Surface;
impl Render for Surface {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().children(Root::render_dialog_layer(window, cx))
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}
#[track_caller]
fn click(cx: &mut VisualTestContext, id: &'static str) {
    draw(cx);
    let bounds = cx.debug_bounds(id).unwrap_or_else(|| panic!("real editor control bounds: {id}"));
    cx.simulate_click(bounds.center(), gpui::Modifiers::default());
    draw(cx);
}
fn type_field(
    editor: &Entity<LocalProfiles>,
    field: fn(&LocalProfiles) -> &Entity<InputState>,
    text: &str,
    cx: &mut VisualTestContext,
) {
    cx.update(|window, cx| {
        field(editor.read(cx)).clone().update(cx, |input, cx| input.focus(window, cx))
    });
    cx.simulate_keystrokes(match crate::platform::Platform::current() {
        crate::platform::Platform::MacOS => "cmd-a",
        _ => "ctrl-a",
    });
    cx.simulate_input(text);
    draw(cx);
}

#[gpui::test]
fn local_profile_editor_real_controls_save_duplicate_and_reject_reserved_variables(
    cx: &mut TestAppContext,
) {
    if crate::platform::profile_test_isolation::run_exact_child(
        "gpui_shell::workspace::local_profiles::tests::local_profile_editor_real_controls_save_duplicate_and_reject_reserved_variables",
        None,
    ) {
        return;
    }
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("profiles.json");
    let (_, mut window) =
        cx.add_window_view(|window, cx| Root::new(cx.new(|_| Surface), window, cx));
    let editor = window.update(|window, cx| {
        let editor = cx.new(|cx| LocalProfiles::new(path.clone(), window, cx));
        let child = editor.clone();
        window.open_dialog(cx, move |dialog, _, _| dialog.w(px(680.0)).child(child.clone()));
        editor
    });
    draw(&mut window);
    click(&mut window, "local-profile-new");
    type_field(&editor, |e| &e.name, "Alpha", &mut window);
    type_field(
        &editor,
        |e| &e.program,
        std::env::current_exe().unwrap().to_str().unwrap(),
        &mut window,
    );

    type_field(&editor, |e| &e.args, "--first\nargument with spaces", &mut window);
    click(&mut window, "local-profile-add-variable");
    type_field(&editor, |e| &e.variables[0].name, "QA_LOCAL_VALUE", &mut window);
    type_field(&editor, |e| &e.variables[0].value, "synthetic-secret-alpha", &mut window);
    click(&mut window, "local-profile-save");
    let store = TerminalProfiles::load_from(&path).unwrap();
    assert_eq!(store.profiles().len(), 1);
    let alpha = store.profiles()[0].clone();
    assert_eq!(alpha.args, ["--first", "argument with spaces"]);
    assert_eq!(alpha.env.0["QA_LOCAL_VALUE"], "synthetic-secret-alpha");
    assert!(!format!("{alpha:?}").contains("synthetic-secret-alpha"));
    click(&mut window, "local-profile-duplicate-0");
    type_field(&editor, |e| &e.name, "Beta", &mut window);
    type_field(&editor, |e| &e.variables[0].value, "beta-value", &mut window);
    click(&mut window, "local-profile-save");
    let store = TerminalProfiles::load_from(&path).unwrap();
    assert_eq!(store.profiles().len(), 2);
    assert_eq!(store.profiles()[0].command, store.profiles()[1].command);
    assert_ne!(store.profiles()[0].id, store.profiles()[1].id);
    assert_eq!(store.profiles()[0], alpha);
    type_field(&editor, |e| &e.variables[0].name, "PEBREL_PANE_ID", &mut window);
    click(&mut window, "local-profile-save");
    editor.read_with(window, |editor, _| {
        assert!(editor.error.as_deref().unwrap().contains("owned by Pebrel"))
    });
    assert_eq!(TerminalProfiles::load_from(&path).unwrap(), store);
    click(&mut window, "local-profile-cancel");
    assert_eq!(
        TerminalProfiles::load_from(&path).unwrap(),
        store,
        "cancel discards unsaved form edits"
    );
}

#[gpui::test]
fn local_profile_editor_duplicate_keeps_unchanged_multiline_and_empty_arguments(
    cx: &mut TestAppContext,
) {
    if crate::platform::profile_test_isolation::run_exact_child(
        "gpui_shell::workspace::local_profiles::tests::local_profile_editor_duplicate_keeps_unchanged_multiline_and_empty_arguments",
        None,
    ) {
        return;
    }
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("profiles.json");
    let original = TerminalProfile {
        id: "synthetic-original".into(),
        name: "Original".into(),
        command: std::env::current_exe().unwrap(),
        args: vec!["-c".into(), "printf first\nprintf second".into(), String::new()],
        cwd: None,
        shell_id: None,
        env: Default::default(),
    };
    TerminalProfiles::edit_at(&path, None, Some(original.clone())).unwrap();
    let (_, mut window) =
        cx.add_window_view(|window, cx| Root::new(cx.new(|_| Surface), window, cx));
    let editor = window.update(|window, cx| {
        let editor = cx.new(|cx| LocalProfiles::new(path.clone(), window, cx));
        let child = editor.clone();
        window.open_dialog(cx, move |dialog, _, _| dialog.w(px(680.0)).child(child.clone()));
        editor
    });
    draw(&mut window);
    click(&mut window, "local-profile-duplicate-0");
    type_field(&editor, |editor| &editor.name, "Copy", &mut window);
    click(&mut window, "local-profile-save");
    let store = TerminalProfiles::load_from(&path).unwrap();
    assert_eq!(store.profiles().len(), 2);
    assert_eq!(store.profiles()[0], original);
    assert_ne!(store.profiles()[1].id, original.id);
    assert_eq!(store.profiles()[1].args, original.args);
    // Once the user changes the displayed arguments, the one-argument-per-line UI applies.
    type_field(&editor, |editor| &editor.args, "--new\ntwo words", &mut window);
    click(&mut window, "local-profile-save");
    let store = TerminalProfiles::load_from(&path).unwrap();
    assert_eq!(store.profiles()[1].args, ["--new", "two words"]);
    assert_eq!(store.profiles()[0], original);
}

#[gpui::test]
fn detected_preset_editor_real_controls_prefill_save_and_keep_presets_immutable(
    cx: &mut TestAppContext,
) {
    if crate::platform::profile_test_isolation::run_exact_child(
        "gpui_shell::workspace::local_profiles::tests::detected_preset_editor_real_controls_prefill_save_and_keep_presets_immutable",
        None,
    ) {
        return;
    }
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("profiles.json");
    let program = directory.path().join("wsl.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &program).unwrap();
    let presets = vec![crate::shell_detect::DetectedShell {
        name: "Synthetic WSL".into(),
        id: "wsl:Team Linux".into(),
        program: program.to_string_lossy().into_owned(),
        args: ["-d", "Team Linux", "--user", "guest"].map(String::from).to_vec(),
    }];
    let (_, window) = cx.add_window_view(|window, cx| Root::new(cx.new(|_| Surface), window, cx));
    let mut window = window.clone();
    let editor = window.update(|window, cx| {
        let editor = cx.new(|cx| LocalProfiles::new(path.clone(), window, cx));
        let child = editor.clone();
        window.open_dialog(cx, move |dialog, _, _| dialog.w(px(680.0)).child(child.clone()));
        editor
    });
    draw(&mut window);
    editor.update(&mut window, |editor, cx| {
        editor.presets = presets.clone();
        cx.notify();
    });
    click(&mut window, "local-profile-preset-0");
    editor.read_with(&window, |editor, cx| {
        assert!(editor.original.is_none(), "preset selection is a new saved session");
        assert_eq!(editor.name.read(cx).value(), "Synthetic WSL");
        assert_eq!(editor.program.read(cx).value(), program.to_string_lossy().as_ref());
        assert_eq!(editor.args.read(cx).value(), "-d\nTeam Linux\n--user\nguest");
        assert!(editor.cwd.read(cx).value().is_empty());
    });
    type_field(&editor, |editor| &editor.name, "WSL custom", &mut window);
    type_field(
        &editor,
        |editor| &editor.args,
        "-d\nTeam Linux\n--user\nguest\n--shell-type=login",
        &mut window,
    );
    click(&mut window, "local-profile-save");
    let store = TerminalProfiles::load_from(&path).unwrap();
    assert_eq!(store.profiles().len(), 1);
    assert_eq!(
        store.profiles()[0].args,
        ["-d", "Team Linux", "--user", "guest", "--shell-type=login"]
    );
    assert_eq!(store.profiles()[0].shell_id.as_deref(), Some("wsl:Team Linux"));
    assert!(store.profiles()[0].cwd.is_none());
    editor.read_with(&window, |editor, _| assert_eq!(editor.presets, presets));
    click(&mut window, "local-profile-preset-0");
    click(&mut window, "local-profile-save");
    let second = TerminalProfiles::load_from(&path).unwrap();
    assert_eq!(second.profiles().len(), 2);
    assert_eq!(second.profiles()[0], store.profiles()[0]);
    assert_ne!(second.profiles()[0].id, second.profiles()[1].id);
    assert_eq!(second.profiles()[1].args, presets[0].args);
}
