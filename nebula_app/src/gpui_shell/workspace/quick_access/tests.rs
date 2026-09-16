use super::quick_access_store_id;

/// 「快速访问」的 × 必须按 store 的裸 id 删。渲染键
/// （[`crate::config::ui_config::Profile::settings_id`]）带 `profile:<shell>|`
/// 前缀，是设置页引用默认 shell 的键——递进 `remove` 只会静默返回 false，
/// 也就是「× 点得动但删不掉」的成因。这条测试把两个键的分工钉住。
#[test]
fn quick_access_delete_is_keyed_by_the_store_id() {
    let mut store = crate::terminal_profiles::TerminalProfiles::default();
    store
        .add(crate::terminal_profiles::TerminalProfile {
            id: r"qa-d:\tools\demo".to_owned(),
            name: "demo".to_owned(),
            // `validate` 只要求绝对路径，不要求这个文件真的存在。
            command: std::env::temp_dir().join("wsl.exe"),
            args: vec!["-d".to_owned(), "Ubuntu".to_owned()],
            cwd: None,
            shell_id: "wsl:Ubuntu".to_owned(),
        })
        .unwrap();

    let rows = store.as_config_profiles();
    let row = &rows[0];
    let render_key = row.settings_id().expect("导入的 profile 一定有设置键");
    let delete_id = quick_access_store_id(row).expect("store id").to_owned();

    // 反例：渲染键删不掉任何东西——修之前 × 走的就是这条路。
    assert_ne!(render_key, delete_id, "渲染键与删除键不是同一个东西");
    assert!(!store.remove(&render_key), "渲染键不是删除键");

    // 正例：store 自己的裸 id 才认。
    assert_eq!(delete_id, r"qa-d:\tools\demo");
    assert!(store.remove(&delete_id));
    assert!(store.profiles().is_empty());
}

use super::quick_access_profile_for;
use crate::session::LaunchSession;
use crate::terminal_profiles::TerminalProfiles;
use std::path::Path;

fn fixture_shell() -> Option<(String, LaunchSession)> {
    Some((
        "sh".into(),
        LaunchSession::Shell {
            name: "Shell".into(),
            program: std::env::temp_dir().join("sh").to_string_lossy().into_owned(),
            args: vec!["-l".into()],
        },
    ))
}

#[test]
fn differently_cased_directories_survive_store_round_trip() {
    let root = tempfile::tempdir().unwrap();
    let upper =
        quick_access_profile_for(&root.path().join("Project"), fixture_shell(), None).unwrap();
    let lower =
        quick_access_profile_for(&root.path().join("project"), fixture_shell(), None).unwrap();
    assert_ne!(upper.id, lower.id);
    assert_eq!(upper.args, ["-l"]);
    let mut store = TerminalProfiles::default();
    store.add(upper).unwrap();
    store.add(lower).unwrap();
    assert_eq!(store.profiles().len(), 2);
    // Re-adding the same path still replaces its own entry.
    store
        .add(quick_access_profile_for(&root.path().join("Project"), fixture_shell(), None).unwrap())
        .unwrap();
    assert_eq!(store.profiles().len(), 2);
}

#[test]
fn wsl_shortcut_uses_absolute_program_and_guest_directory() {
    let program = std::env::temp_dir().join("wsl.exe");
    let directory = Path::new(r"\\wsl.localhost\Ubuntu\home\dev\Project");
    let profile = quick_access_profile_for(directory, None, Some(program.clone())).unwrap();
    assert_eq!(profile.command, program);
    assert_eq!(profile.args, ["-d", "Ubuntu", "--cd", "/home/dev/Project"]);
    assert_eq!(profile.cwd, None);
    let mut store = TerminalProfiles::default();
    store.add(profile).expect("WSL entry must pass the real store's validation");
    assert!(quick_access_profile_for(directory, None, Some("wsl.exe".into())).is_none());
    assert!(quick_access_profile_for(directory, None, None).is_none());
}

#[test]
fn saved_profile_keeps_arguments_and_uses_selected_directory() {
    let directory = std::env::temp_dir().join("selected project");
    let program = std::env::temp_dir().join("shell");
    let launch = LaunchSession::Profile {
        name: "Login shell".into(),
        command: program.to_string_lossy().into_owned(),
        args: vec!["--login".into()],
        cwd: Some("old directory".into()),
        shell_id: Some("zsh".into()),
    };
    let profile =
        quick_access_profile_for(&directory, Some(("profile:zsh|login".into(), launch)), None)
            .unwrap();
    assert_eq!(profile.command, program);
    assert_eq!(profile.args, ["--login"]);
    assert_eq!(profile.cwd, Some(directory));
    assert_eq!(profile.shell_id, "zsh");
}

#[cfg(all(feature = "gpui-test-support", not(windows)))]
#[gpui::test]
fn empty_quick_access_still_opens_the_add_picker(cx: &mut gpui::TestAppContext) {
    use gpui::{Modifiers, point, px};
    let (workspace, mut window) = super::super::project::tests::open_workspace(
        nebula_settings::TabsPositionName::Sidebar,
        cx,
    );
    window.update(|window, cx| {
        workspace.update(cx, |view, cx| {
            view.quick_access.clear();
            view.quick_access_collapsed = false;
            cx.notify();
        });
        let _ = window.draw(cx);
    });
    let bounds = window
        .debug_bounds("sidebar-quick-access-add")
        .expect("empty lists must allow adding the first entry");
    window.simulate_click(bounds.origin + point(px(3.0), px(3.0)), Modifiers::default());
    window.run_until_parked();
    assert!(window.did_prompt_for_paths());
    window.simulate_path_prompt_response(|options| {
        assert!(options.directories && !options.files && !options.multiple);
        None
    });
    window.run_until_parked();
    workspace.read_with(&window, |view, _| assert!(view.quick_access.is_empty()));
}
