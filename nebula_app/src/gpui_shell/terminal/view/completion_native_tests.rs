//! Native window + real shell/PTY + Git, using the product's input and paint paths.

use super::*;
use gpui::{EntityInputHandler as _, WindowBounds, WindowOptions, size};
use gpui_component::ActiveTheme as _;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

mod editor;
mod remote;

struct CompletionSurface(gpui::Entity<TerminalView>);

impl gpui::Render for CompletionSurface {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        // 终端视图由正式卡片容器提供底色；独立验收窗口也必须补齐这个组合职责。
        gpui::div().size_full().bg(cx.theme().background).child(self.0.clone())
    }
}

async fn wait_for(
    cx: &mut gpui::AsyncApp,
    window: gpui::AnyWindowHandle,
    view: &gpui::Entity<TerminalView>,
    check: impl Fn(&TerminalView) -> bool,
) -> Result<(), String> {
    for _ in 0..500 {
        let ready = cx
            .update_window(window, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
                check(view.read(cx))
            })
            .map_err(|error| error.to_string())?;
        if ready {
            return Ok(());
        }
        cx.background_executor().timer(Duration::from_millis(20)).await;
    }
    cx.update_window(window, |_, _, cx| {
        let view = view.read(cx);
        format!(
            "completion timeout: line={:?} ghost={:?} items={:?} error={:?} exited={:?} editor={:?} mode={:?}",
            view.suggest.screen_line,
            view.suggest.suggestion,
            view.suggest.completion_items,
            view.error,
            view.exited,
            view.completion_editor,
            view.term_mode()
        )
    })
    .map_err(|error| error.to_string())
    .and_then(Err)
}

async fn type_demo_line(
    cx: &mut gpui::AsyncApp,
    window: gpui::AnyWindowHandle,
    view: &gpui::Entity<TerminalView>,
    text: &str,
) -> Result<(), String> {
    for character in text.chars() {
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.replace_text_in_range(None, &character.to_string(), window, cx);
            })
        })
        .map_err(|error| error.to_string())?;
        cx.background_executor().timer(Duration::from_millis(110)).await;
    }
    Ok(())
}

#[test]
#[ignore = "requires a native desktop, Node/npm, and fresh isolated PEBREL_COMPLETION_QA_DIR/config"]
fn git_completion_native_shell_end_to_end() {
    run_native_completion_fixture(false);
}

#[test]
#[ignore = "requires a native desktop with the PowerShell QA editor and isolated config"]
fn editor_completion_native_shell_end_to_end() {
    run_native_completion_fixture(true);
}

fn run_native_completion_fixture(editor_only: bool) {
    let output = PathBuf::from(std::env::var_os("PEBREL_COMPLETION_QA_DIR").expect("QA directory"));
    let demo = std::env::var("PEBREL_COMPLETION_DEMO").ok();
    assert!(output.is_absolute());
    assert_eq!(
        std::env::var_os("PEBREL_CONFIG_DIR").map(PathBuf::from),
        Some(output.join("config"))
    );
    assert!(!output.join("result.json").exists(), "use a fresh QA directory");
    if demo.is_some() {
        // Powerline 由 shell 读取共享持久化设置；只写本次录制的隔离配置。
        nebula_settings::persist_keys(&[("powerline", "1".to_owned())]).unwrap();
    }
    let repository = crate::git_completion::tests::repository();
    for name in
        ["qa inline 文件.txt", "qa popup 文件.txt", "qa hybrid 文件.txt", "qa right 文件.txt"]
    {
        std::fs::write(repository.path().join(name), "executed").unwrap();
    }
    crate::git_completion::tests::git(repository.path(), &["add", "."]);
    crate::git_completion::tests::git(
        repository.path(),
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-m",
            "Path completion fixture",
        ],
    );
    for branch in [
        "qa/inline",
        "qa/popup",
        "qa/hybrid",
        "qa/right",
        "qa/subcommand",
        "qa/option",
        "qa/value",
        "feature/search-panel",
        "feature/settings-sync",
        "qa/editor-inline-中文😀",
        "qa/editor-inline-basic-中文",
        "qa/editor-popup-中文😀",
        "qa/editor-popup-basic-中文",
        "qa/editor-hybrid-中文😀",
        "qa/editor-hybrid-basic-中文",
        "qa/rapid-inline",
        "qa/rapid-popup",
        "qa/rapid-hybrid",
        "qa/prediction-inline",
        "qa/prediction-popup",
        "qa/prediction-hybrid",
    ] {
        crate::git_completion::tests::git(repository.path(), &["branch", branch]);
    }
    for tag in ["release/inline", "release/popup", "release/hybrid", "release/detach"] {
        crate::git_completion::tests::git(repository.path(), &["tag", tag]);
    }
    crate::git_completion::tests::git(
        repository.path(),
        &["update-ref", "refs/remotes/origin/native-topic", "HEAD"],
    );
    for remote in ["origin", "upstream", "custom"] {
        crate::git_completion::tests::git(
            repository.path(),
            &["remote", "add", remote, "https://example.invalid/native-completion"],
        );
    }
    for name in ["inline", "popup", "hybrid", "right", "checkout", "preferred"] {
        crate::git_completion::tests::git(
            repository.path(),
            &["update-ref", &format!("refs/remotes/origin/auto/{name}"), "HEAD"],
        );
    }
    crate::git_completion::tests::git(
        repository.path(),
        &["update-ref", "refs/remotes/upstream/auto/preferred", "HEAD"],
    );
    crate::git_completion::tests::git(
        repository.path(),
        &["config", "checkout.defaultRemote", "origin"],
    );
    crate::git_completion::tests::git(
        repository.path(),
        &["config", "remote.custom.fetch", "+refs/heads/custom/*:refs/vendor/pre-*-post"],
    );
    crate::git_completion::tests::git(
        repository.path(),
        &["update-ref", "refs/vendor/pre-native-post", "HEAD"],
    );
    for mode in ["inline", "popup", "hybrid"] {
        crate::git_completion::tests::git(
            repository.path(),
            &["update-ref", &format!("refs/remotes/origin/track-{mode}"), "HEAD"],
        );
    }
    crate::git_completion::tests::git(
        repository.path(),
        &["update-ref", "refs/remotes/origin/no-track", "HEAD"],
    );
    let bare = tempfile::tempdir().unwrap();
    crate::git_completion::tests::git(bare.path(), &["init", "--bare"]);
    crate::git_completion::tests::git(
        repository.path(),
        &["remote", "add", "fixture", bare.path().to_str().unwrap()],
    );
    crate::git_completion::tests::git(repository.path(), &["push", "fixture", "main:network-seed"]);
    let revision = std::fs::read_to_string(repository.path().join(".git/refs/heads/main")).unwrap();
    let mut scripts: serde_json::Map<_, _> = ["inline", "popup", "hybrid", "right"]
        .into_iter()
        .map(|mode| {
            (
                format!("qa:{mode}"),
                serde_json::Value::String(format!(
                    "node -e \"require('fs').writeFileSync('.qa-{mode}', 'executed')\""
                )),
            )
        })
        .collect();
    scripts.insert("build:desktop".into(), serde_json::Value::String("node build.cjs".into()));
    std::fs::write(repository.path().join("build.cjs"), "require('fs').writeFileSync('.qa-desktop', 'executed'); console.log('Desktop build completed');\n").unwrap();
    std::fs::write(
        repository.path().join("package.json"),
        serde_json::to_vec(&serde_json::json!({"name":"native-completion-root", "scripts": scripts, "workspaces":["packages/*"]})).unwrap(),
    )
    .unwrap();
    for mode in ["inline", "popup", "hybrid"] {
        let directory = repository.path().join("packages").join(mode);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("package.json"), serde_json::to_vec(&serde_json::json!({
            "name":format!("native-workspace-{mode}"),
            "scripts":{"qa:workspace":format!("node -e \"require('fs').writeFileSync('../../.qa-workspace-{mode}', 'executed')\"")}
        })).unwrap()).unwrap();
    }
    let ssh_config = repository.path().join("qa-ssh-included.conf");
    std::fs::write(&ssh_config, "Host native-inline native-popup native-hybrid native-right production-eu production-us\n  HostName completion.example.invalid\n").unwrap();
    crate::platform::shell::completion_qa_ssh_config_permissions(&ssh_config);
    std::fs::write(
        repository.path().join("qa-ssh.conf"),
        format!("Include \"{}\"\n", ssh_config.to_string_lossy().replace('\\', "/")),
    )
    .unwrap();
    std::fs::copy(repository.path().join("qa-ssh.conf"), repository.path().join("ssh.conf"))
        .unwrap();
    std::fs::write(repository.path().join("qa common source.txt"), "executed").unwrap();
    std::fs::write(repository.path().join("release notes.txt"), "Release checklist ready.\n")
        .unwrap();
    std::fs::write(repository.path().join("release plan.md"), "executed").unwrap();
    for mode in ["inline", "popup", "hybrid"] {
        std::fs::write(repository.path().join(format!("qa move {mode}.txt")), "executed").unwrap();
    }
    let shell = crate::platform::shell::completion_qa_shell(&output);
    let powershell = pebrel_completions::command_context::ShellSyntax::for_program(shell.program())
        == pebrel_completions::command_context::ShellSyntax::PowerShell;
    assert!(!editor_only || powershell, "editor-only acceptance requires the PowerShell QA shell");
    let result = Arc::new(Mutex::new(None));
    let after = result.clone();
    gpui_platform::application().with_assets(crate::gpui_shell::assets::NebulaAssets).run(move |cx| {
        crate::gpui_shell::register_bundled_fonts(cx);
        gpui_component::init(cx);
        crate::gpui_shell::scientific_render::init(cx);
        let mut settings = Settings::load(nebula_settings::ThemeName::Nord);
        settings.ghost = true;
        if demo.is_some() {
            settings.font_size_px = 18.0;
        }
        cx.set_global(settings);
        crate::gpui_shell::theme::apply_chrome_theme(cx);
        let mut terminal = None;
        let cwd = repository.path().to_owned();
        let window = cx.open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(80.0), px(80.0)), size(px(1000.0), px(600.0))))),
            focus: false,
            ..Default::default()
        }, |window, cx| {
            if demo.is_some() { window.set_window_title("Pebrel Completion Demo"); }
            let view = cx.new(|cx| TerminalView::new(9001, (100, 30), TerminalLaunch::Local {
                cwd: Some(cwd), shell: Some(shell), shell_name: None,
            }, window, cx));
            window.focus(&view.read(cx).focus_handle.clone(), cx);
            terminal = Some(view.clone());
            let surface = cx.new(|_| CompletionSurface(view));
            cx.new(|cx| gpui_component::Root::new(surface, window, cx))
        }).unwrap();
        let terminal = terminal.unwrap();
        cx.spawn(async move |cx| {
            let run = async {
                if demo.is_some() {
                    wait_for(cx, window.into(), &terminal, |view| view.session.as_ref().is_some_and(|session| {
                        let term = session.term.lock();
                        crate::display::nebula_shell_ready_from_raw_grid(&term, &view.suggest.suggest_env)
                    })).await?;
                    std::fs::write(output.join("ready"), b"ready").map_err(|e| e.to_string())?;
                    for _ in 0..600 {
                        if output.join("record").exists() { break; }
                        cx.background_executor().timer(Duration::from_millis(50)).await;
                    }
                    if !output.join("record").exists() { return Err("recorder did not start".into()); }
                }
                let mut reports = Vec::new();
                let mut previous = "ref: refs/heads/main".to_owned();
                let marker_complete = |name: &str| {
                    if name.starts_with(".qa-push-") { return bare.path().join("refs/heads").join(format!("network-{}", name.trim_start_matches(".qa-push-"))).is_file(); }
                    if name == ".qa-fetch" { return std::fs::read_to_string(repository.path().join(".git/FETCH_HEAD")).is_ok_and(|text| text.contains("network-inline")); }
                    std::fs::read_to_string(repository.path().join(name)).is_ok_and(|text| {
                    if name.starts_with(".qa-push-") { return bare.path().join("refs/heads").join(format!("network-{}", name.trim_start_matches(".qa-push-"))).is_file(); }
                    if name == ".qa-fetch" { return std::fs::read_to_string(repository.path().join(".git/FETCH_HEAD")).is_ok_and(|text| text.contains("network-inline")); }
                    if name == ".qa-pull" { return !text.trim().is_empty(); }
                    if name.starts_with(".qa-ssh-") { text.lines().any(|line| line == "hostname completion.example.invalid") }
                    else if name.starts_with(".qa-wsl") || name.starts_with(".qa-cat") { text.trim_start_matches('\u{feff}').trim() == "executed" }
                    else { text == "executed" }
                }) };
                let distro = crate::platform::shell::registered_wsl_distros(&|| false).into_iter().next();
                let wsl_case = distro.as_deref().map(|name| {
                    let prefix: String = name.chars().take(2).collect();
                    (format!("wsl -d \"{prefix}"), format!("wsl -d \"{name}\""))
                });
                let config_expected = if powershell { "ssh -G '-Fqa-ssh.conf'" } else { "ssh -G -Fqa-ssh.conf" };
                let jump_expected = if powershell { "ssh -G -F qa-ssh.conf '-Jnative-inline,me@native-popup'" } else { "ssh -G -F qa-ssh.conf -Jnative-inline,me@native-popup" };
                // Every case starts from a real prompt, types through EntityInputHandler,
                // paints candidates, accepts through the keyboard handler, then executes.
                let mut cases = vec![
                    (crate::display::CompletionStyle::Inline, "git switch qa/in", "git switch qa/inline", "", Some("qa/inline"), None, false),
                    (crate::display::CompletionStyle::Hybrid, "git switch qa/inline", "git switch qa/inline", "", Some("qa/inline"), None, false),
                    (crate::display::CompletionStyle::Popup, "git switch \"qa/po\"", "git switch \"qa/popup\"", "", Some("qa/popup"), None, false),
                    (crate::display::CompletionStyle::Hybrid, "git switch \"qa/hy", "git switch \"qa/hybrid\"", "", Some("qa/hybrid"), None, false),
                    (crate::display::CompletionStyle::Hybrid, "git switch qa/ri", "git switch qa/right", "", Some("qa/right"), None, true),
                    (crate::display::CompletionStyle::Inline, "git sw", "git switch", " qa/subcommand", Some("qa/subcommand"), None, false),
                    (crate::display::CompletionStyle::Popup, "git switch --qui", "git switch --quiet", " qa/option", Some("qa/option"), None, false),
                    (crate::display::CompletionStyle::Hybrid, "git switch --conflict zd", "git switch --conflict zdiff3", " qa/value", Some("qa/value"), None, true),
                    (crate::display::CompletionStyle::Inline, "git switch --track origin/track-in", "git switch --track origin/track-inline", "", Some("track-inline"), None, false),
                    (crate::display::CompletionStyle::Popup, "git switch --track=direct origin/track-po", "git switch --track=direct origin/track-popup", "", Some("track-popup"), None, false),
                    (crate::display::CompletionStyle::Hybrid, "git switch --track origin/track-hy", "git switch --track origin/track-hybrid", "", Some("track-hybrid"), None, false),
                    (crate::display::CompletionStyle::Popup, "git switch -c native-inherit --track=inherit track-in", "git switch -c native-inherit --track=inherit track-inline", "", Some("native-inherit"), None, false),
                    (crate::display::CompletionStyle::Hybrid, "git switch --no-track origin/no-tr", "git switch --no-track origin/no-track", "", Some("no-track"), None, true),
                    (crate::display::CompletionStyle::Inline, "git push fi", "git push fixture", " qa/inline:network-inline", None, Some(".qa-push-inline"), false),
                    (crate::display::CompletionStyle::Popup, "git push fixture qa/po", "git push fixture qa/popup", ":network-popup", None, Some(".qa-push-popup"), false),
                    (crate::display::CompletionStyle::Hybrid, "git push fixture qa/hy", "git push fixture qa/hybrid", ":network-hybrid", None, Some(".qa-push-hybrid"), false),
                    (crate::display::CompletionStyle::Popup, "git fetch fixture network-in", "git fetch fixture network-inline", "", None, Some(".qa-fetch"), false),
                    (crate::display::CompletionStyle::Hybrid, "git pull --ff-only fixture network-in", "git pull --ff-only fixture network-inline", " > .qa-pull", None, Some(".qa-pull"), false),
                    (crate::display::CompletionStyle::Inline, "npm -w native-workspace-inline run qa:wor", "npm -w native-workspace-inline run qa:workspace", "", None, Some(".qa-workspace-inline"), false),
                    (crate::display::CompletionStyle::Popup, "npm run --workspace native-workspace-popup qa:wor", "npm run --workspace native-workspace-popup qa:workspace", "", None, Some(".qa-workspace-popup"), false),
                    (crate::display::CompletionStyle::Hybrid, "npm --workspace=native-workspace-hybrid run qa:wor", "npm --workspace=native-workspace-hybrid run qa:workspace", "", None, Some(".qa-workspace-hybrid"), false),
                    (crate::display::CompletionStyle::Inline, "npm run qa:in", "npm run qa:inline", "", None, Some(".qa-inline"), false),
                    (crate::display::CompletionStyle::Popup, "npm run \"qa:po\"", "npm run \"qa:popup\"", "", None, Some(".qa-popup"), false),
                    (crate::display::CompletionStyle::Hybrid, "npm run \"qa:hy", "npm run \"qa:hybrid\"", "", None, Some(".qa-hybrid"), false),
                    (crate::display::CompletionStyle::Hybrid, "npm run qa:ri", "npm run qa:right", "", None, Some(".qa-right"), true),
                    (crate::display::CompletionStyle::Inline, "git checkout -- \"qa in", "git checkout -- \"qa inline 文件.txt\"", "", None, Some("qa inline 文件.txt"), false),
                    (crate::display::CompletionStyle::Popup, "git checkout -- \"qa po\"", "git checkout -- \"qa popup 文件.txt\"", "", None, Some("qa popup 文件.txt"), false),
                    (crate::display::CompletionStyle::Hybrid, "git checkout -- \"qa hy", "git checkout -- \"qa hybrid 文件.txt\"", "", None, Some("qa hybrid 文件.txt"), false),
                    (crate::display::CompletionStyle::Hybrid, "git checkout -- \"qa ri\"", "git checkout -- \"qa right 文件.txt\"", "", None, Some("qa right 文件.txt"), true),
                    (crate::display::CompletionStyle::Inline, "git switch -c qa/tag-inline release/in", "git switch -c qa/tag-inline release/inline", "", Some("qa/tag-inline"), None, false),
                    (crate::display::CompletionStyle::Popup, "git switch -c qa/tag-popup \"release/po\"", "git switch -c qa/tag-popup \"release/popup\"", "", Some("qa/tag-popup"), None, false),
                    (crate::display::CompletionStyle::Hybrid, "git switch -c qa/tag-hybrid \"release/hy", "git switch -c qa/tag-hybrid \"release/hybrid\"", "", Some("qa/tag-hybrid"), None, false),
                    (crate::display::CompletionStyle::Hybrid, "git switch -c qa/remote origin/native", "git switch -c qa/remote origin/native-topic", "", Some("qa/remote"), None, true),
                    (crate::display::CompletionStyle::Popup, "git switch --detach release/de", "git switch --detach release/detach", "", None, None, false),
                    (crate::display::CompletionStyle::Inline, "git switch auto/in", "git switch auto/inline", "", Some("auto/inline"), None, false),
                    (crate::display::CompletionStyle::Popup, "git switch \"auto/po\"", "git switch \"auto/popup\"", "", Some("auto/popup"), None, false),
                    (crate::display::CompletionStyle::Hybrid, "git switch \"auto/hy", "git switch \"auto/hybrid\"", "", Some("auto/hybrid"), None, false),
                    (crate::display::CompletionStyle::Hybrid, "git switch auto/ri", "git switch auto/right", "", Some("auto/right"), None, true),
                    (crate::display::CompletionStyle::Popup, "git checkout auto/ch", "git checkout auto/checkout", "", Some("auto/checkout"), None, false),
                    (crate::display::CompletionStyle::Inline, "git switch auto/pre", "git switch auto/preferred", "", Some("auto/preferred"), None, false),
                    (crate::display::CompletionStyle::Hybrid, "git switch custom/na", "git switch custom/native", "", Some("custom/native"), None, true),
                    (crate::display::CompletionStyle::Inline, "ssh -G -F qa-ssh.conf native-in", "ssh -G -F qa-ssh.conf native-inline", " > .qa-ssh-inline", None, Some(".qa-ssh-inline"), false),
                    (crate::display::CompletionStyle::Popup, "ssh -G -F qa-ssh.conf native-po", "ssh -G -F qa-ssh.conf native-popup", " > .qa-ssh-popup", None, Some(".qa-ssh-popup"), false),
                    (crate::display::CompletionStyle::Hybrid, "ssh -G -F qa-ssh.conf native-hy", "ssh -G -F qa-ssh.conf native-hybrid", " > .qa-ssh-hybrid", None, Some(".qa-ssh-hybrid"), false),
                    (crate::display::CompletionStyle::Hybrid, "ssh -G -F qa-ssh.conf me@native-ri", "ssh -G -F qa-ssh.conf me@native-right", " > .qa-ssh-right", None, Some(".qa-ssh-right"), true),
                    (crate::display::CompletionStyle::Inline, "cp \"qa common so", "cp \"qa common source.txt\"", " qa-copied-inline", None, Some("qa-copied-inline"), false),
                    (crate::display::CompletionStyle::Popup, "cp \"qa common so", "cp \"qa common source.txt\"", " qa-copied-popup", None, Some("qa-copied-popup"), false),
                    (crate::display::CompletionStyle::Hybrid, "cp \"qa common so", "cp \"qa common source.txt\"", " qa-copied-hybrid", None, Some("qa-copied-hybrid"), true),
                    (crate::display::CompletionStyle::Inline, "cat \"qa common so", "cat \"qa common source.txt\"", " > .qa-cat-inline", None, Some(".qa-cat-inline"), false),
                    (crate::display::CompletionStyle::Popup, "cat \"qa common so", "cat \"qa common source.txt\"", " > .qa-cat-popup", None, Some(".qa-cat-popup"), false),
                    (crate::display::CompletionStyle::Hybrid, "cat \"qa common so", "cat \"qa common source.txt\"", " > .qa-cat-hybrid", None, Some(".qa-cat-hybrid"), true),
                    (crate::display::CompletionStyle::Inline, "mv \"qa move in", "mv \"qa move inline.txt\"", " qa-moved-inline", None, Some("qa-moved-inline"), false),
                    (crate::display::CompletionStyle::Popup, "mv \"qa move po", "mv \"qa move popup.txt\"", " qa-moved-popup", None, Some("qa-moved-popup"), false),
                    (crate::display::CompletionStyle::Hybrid, "mv \"qa move hy", "mv \"qa move hybrid.txt\"", " qa-moved-hybrid", None, Some("qa-moved-hybrid"), true),
                    (crate::display::CompletionStyle::Popup, "ssh -G -Fqa-ssh.c", config_expected, " native-popup > .qa-ssh-config", None, Some(".qa-ssh-config"), false),
                    (crate::display::CompletionStyle::Hybrid, "ssh -G -F qa-ssh.conf -Jnative-inline,me@native-po", jump_expected, " native-right > .qa-ssh-jump", None, Some(".qa-ssh-jump"), true),
                ];
                if let Some((prefix, expected)) = &wsl_case {
                    cases.push((crate::display::CompletionStyle::Popup, prefix.as_str(), expected.as_str(), " --exec /bin/printf executed > .qa-wsl", None, Some(".qa-wsl"), false));
                }
                if demo.as_deref() == Some("smart") {
                    let ssh_preview = if powershell { " | Select-String '^hostname '" } else { " | grep '^hostname '" };
                    cases = vec![
                        (crate::display::CompletionStyle::Popup, "git switch feature/se", "git switch feature/search-panel", "", Some("feature/search-panel"), None, false),
                        (crate::display::CompletionStyle::Inline, "npm run build:d", "npm run build:desktop", "", None, Some(".qa-desktop"), false),
                        (crate::display::CompletionStyle::Hybrid, "ssh -G -F ssh.conf dev@pro", "ssh -G -F ssh.conf dev@production-eu", ssh_preview, None, None, true),
                    ];
                    if let Some((prefix, expected)) = &wsl_case {
                        cases.push((crate::display::CompletionStyle::Popup, prefix, expected, " --exec /bin/cat /etc/os-release", None, None, false));
                    }
                    cases.push((crate::display::CompletionStyle::Hybrid, "cat \"release no", "cat \"release notes.txt\"", "", None, None, true));
                } else if demo.as_deref() == Some("history") {
                    wait_for(cx, window.into(), &terminal, |view| view.session.as_ref().is_some_and(|session| {
                        let term = session.term.lock();
                        crate::display::nebula_shell_ready_from_raw_grid(&term, &view.suggest.suggest_env)
                    })).await?;
                    type_demo_line(cx, window.into(), &terminal, "echo deployment finished").await?;
                    cx.update_window(window.into(), |_, window, cx| terminal.update(cx, |view, cx| {
                        view.on_key_down(&KeyDownEvent { keystroke: gpui::Keystroke::parse("enter").unwrap(), is_held: false, prefer_character_input: false }, window, cx);
                    })).map_err(|e| e.to_string())?;
                    wait_for(cx, window.into(), &terminal, |_| crate::completion::history_hint_for_test(&crate::nebula_history::HistoryScope::Local, "echo dep").is_some()).await?;
                    assert_eq!(crate::completion::history_hint_for_test(&crate::nebula_history::HistoryScope::Local, "echo dep").as_deref(), Some("loyment finished"));
                    cases = vec![(crate::display::CompletionStyle::Inline, "echo dep", "echo deployment finished", "", None, None, false)];
                }
                if demo.is_some() {
                    std::fs::write(output.join("case-count"), cases.len().to_string()).map_err(|e| e.to_string())?;
                }
                if editor_only { cases.clear(); }
                for (mode, prefix, expected, suffix, branch, marker, right) in cases {
                    crate::gpui_shell::try_write_stderr(format_args!("native completion case: {mode:?} {prefix}"));
                    // Windows PowerShell 默认重定向为 UTF-16；证据文件统一显式 UTF-8。
                    let suffix = crate::platform::shell::completion_qa_redirect(suffix);
                    if prefix.starts_with("git checkout --") {
                        std::fs::write(repository.path().join(marker.unwrap()), "modified").map_err(|error| error.to_string())?;
                    }
                    let launcher = format!("{} ", crate::platform::shell::completion_qa_package_manager());
                    let prefix = prefix.replacen("npm ", &launcher, 1);
                    let expected = expected.replacen("npm ", &launcher, 1);
                    if demo.as_deref().is_some_and(|mode| mode != "history") {
                        // 逐场景查真实历史，避免前一条演示意外把后一条变成历史回放。
                        assert!(crate::completion::history_hint_for_test(&crate::nebula_history::HistoryScope::Local, &prefix).is_none(), "first-use candidate: {prefix}");
                    }
                    wait_for(cx, window.into(), &terminal, |view| view.session.as_ref().is_some_and(|session| {
                        let term = session.term.lock();
                        crate::display::nebula_shell_ready_from_raw_grid(&term, &view.suggest.suggest_env)
                    })).await?;
                    cx.update_window(window.into(), |_, window, cx| terminal.update(cx, |view, cx| {
                        view.completion_style = mode;
                        if demo.is_none() { view.replace_text_in_range(None, &prefix, window, cx); }
                    })).map_err(|error| error.to_string())?;
                    if demo.is_some() { type_demo_line(cx, window.into(), &terminal, &prefix).await?; }
                    let start = std::time::Instant::now();
                    if mode == crate::display::CompletionStyle::Hybrid && !right && prefix == expected {
                        wait_for(cx, window.into(), &terminal, |view| view.suggest.screen_line.trim() == prefix).await?;
                    } else {
                        wait_for(cx, window.into(), &terminal, |view| if mode == crate::display::CompletionStyle::Popup { !view.suggest.completion_items.is_empty() } else { !view.suggest.suggestion.is_empty() }).await?;
                    }
                    let candidate_ms = start.elapsed().as_secs_f64() * 1000.0;
                    cx.update_window(window.into(), |_, window, cx| terminal.update(cx, |view, cx| {
                        assert_eq!(view.suggest.screen_line.trim(), prefix, "candidates must wait for the entire typed prefix");
                        if right {
                            view.on_key_down(&KeyDownEvent { keystroke: gpui::Keystroke::parse("right").unwrap(), is_held: false, prefer_character_input: false }, window, cx);
                        } else {
                            view.on_terminal_tab(&TerminalTab, window, cx);
                        }
                    })).map_err(|error| error.to_string())?;
                    if mode == crate::display::CompletionStyle::Hybrid && !right {
                        wait_for(cx, window.into(), &terminal, |view| !view.suggest.completion_items.is_empty()).await?;
                        cx.update_window(window.into(), |_, window, cx| terminal.update(cx, |view, cx| {
                            assert!(view.completion_popup_geometry().is_some(), "real popup layout");
                            assert!(view.suggest.screen_line.trim() == prefix, "Tab must not write");
                            view.on_key_down(&KeyDownEvent { keystroke: gpui::Keystroke::parse("enter").unwrap(), is_held: false, prefer_character_input: false }, window, cx);
                        })).map_err(|error| error.to_string())?;
                    }
                    wait_for(cx, window.into(), &terminal, |view| view.suggest.screen_line.trim() == expected).await?;
                    crate::gpui_shell::try_write_stderr(format_args!("native completion accepted: {expected}"));
                    let head = std::fs::read_to_string(repository.path().join(".git/HEAD")).map_err(|error| error.to_string())?;
                    if head.trim() != previous {
                        return Err("accepting completion executed the command".to_owned());
                    }
                    if marker.is_some_and(marker_complete) {
                        return Err("accepting completion executed a script or restored a file".to_owned());
                    }
                    if !suffix.is_empty() {
                        if demo.is_some() {
                            type_demo_line(cx, window.into(), &terminal, &suffix).await?;
                        } else {
                            cx.update_window(window.into(), |_, window, cx| terminal.update(cx, |view, cx| {
                                view.replace_text_in_range(None, &suffix, window, cx);
                            })).map_err(|error| error.to_string())?;
                        }
                        wait_for(cx, window.into(), &terminal, |view| view.suggest.screen_line.trim() == format!("{expected}{suffix}")).await?;
                    }
                    cx.update_window(window.into(), |_, window, cx| terminal.update(cx, |view, cx| {
                        view.on_key_down(&KeyDownEvent { keystroke: gpui::Keystroke::parse("enter").unwrap(), is_held: false, prefer_character_input: false }, window, cx);
                    })).map_err(|error| error.to_string())?;
                    let expected_head = branch.map(|branch| format!("ref: refs/heads/{branch}"))
                        .or_else(|| prefix.starts_with("git switch --detach").then(|| revision.trim().to_owned()));
                    if let Some(expected_head) = expected_head {
                        wait_for(cx, window.into(), &terminal, |_| std::fs::read_to_string(repository.path().join(".git/HEAD")).is_ok_and(|head| head.trim() == expected_head)).await?;
                        previous = expected_head;
                    }
                    if let Some(marker) = marker {
                        wait_for(cx, window.into(), &terminal, |_| marker_complete(marker)).await.map_err(|error| format!("{error}; marker {marker}: {:?}", std::fs::read(repository.path().join(marker)).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())))?;
                    }
                    if demo.as_deref() == Some("smart") {
                        let visible_output = if prefix.starts_with("ssh ") { Some("hostname completion.example.invalid") }
                            else if prefix.starts_with("wsl ") { Some("NAME=") }
                            else if prefix.starts_with("cat ") { Some("Release checklist ready.") } else { None };
                        if let Some(expected) = visible_output {
                            // 演示直接检查真实终端输出，不把测试标记文件命令录进画面。
                            wait_for(cx, window.into(), &terminal, |view| view.session.as_ref().is_some_and(|session| {
                                session.term.lock().grid().display_iter().map(|cell| cell.cell.c).collect::<String>().contains(expected)
                            })).await?;
                        }
                    }
                    if let Some(branch) = branch.filter(|name| name.starts_with("auto/") || name.starts_with("custom/")) {
                        let cwd = repository.path().to_owned();
                        let upstream = cx.background_executor().spawn(async move {
                            crate::git_completion::tests::git_output(&cwd, &["rev-parse", "--symbolic-full-name", &format!("{branch}@{{upstream}}")])
                        }).await;
                        let expected = if branch == "custom/native" { "refs/vendor/pre-native-post".to_owned() } else { format!("refs/remotes/origin/{branch}") };
                        if upstream.trim() != expected { return Err(format!("wrong upstream: {upstream:?}, expected {expected:?}")); }
                    }
                    reports.push(serde_json::json!({"mode": format!("{mode:?}"), "input": prefix, "accepted": expected, "branch": branch, "script_marker": marker, "candidate_ms": candidate_ms, "right": right}));
                    crate::gpui_shell::try_write_stderr(format_args!("native completion executed: {expected}"));
                    if demo.is_some() {
                        // 只等实际命令结束，不为录制安排人为停顿。
                        wait_for(cx, window.into(), &terminal, |view| view.session.as_ref().is_some_and(|session| {
                            let term = session.term.lock();
                            crate::display::nebula_shell_ready_from_raw_grid(&term, &view.suggest.suggest_env)
                        })).await?;
                    }
                }
                if demo.is_none() && !editor_only {
                    for mode in ["inline", "popup", "hybrid"] {
                        let actual = crate::git_completion::tests::git_output(repository.path(), &["rev-parse", "--symbolic-full-name", &format!("track-{mode}@{{upstream}}")]);
                        assert_eq!(actual.trim(), format!("refs/remotes/origin/track-{mode}"));
                    }
                    assert_eq!(crate::git_completion::tests::git_output(repository.path(), &["rev-parse", "--symbolic-full-name", "native-inherit@{upstream}"]).trim(), "refs/remotes/origin/track-inline");
                    assert!(crate::git_completion::tests::git_output(repository.path(), &["for-each-ref", "--format=%(upstream)", "refs/heads/no-track"]).trim().is_empty());
                }
                if powershell && demo.is_none() {
                    reports.extend(editor::run(cx, window.into(), &terminal, repository.path()).await?);
                }
                if demo.is_some() {
                    // 录制方先关闭编码器再释放窗口，避免把窗口关闭后的桌面收进末帧。
                    std::fs::write(output.join("recording-complete"), b"complete").map_err(|e| e.to_string())?;
                    for _ in 0..600 {
                        if output.join("recording-stopped").exists() { break; }
                        cx.background_executor().timer(Duration::from_millis(50)).await;
                    }
                    if !output.join("recording-stopped").exists() { return Err("recorder did not stop".into()); }
                }
                Ok::<_, String>(reports)
            }.await;
            std::fs::write(output.join("result.json"), serde_json::to_vec_pretty(&run).unwrap()).unwrap();
            *result.lock().unwrap() = Some(run);
            let _ = cx.update_window(window.into(), |_, window, cx| { terminal.update(cx, |view, _| view.shutdown()); window.remove_window(); });
            cx.update(|cx| cx.quit());
        }).detach();
    });
    let outcome = after.lock().unwrap().take();
    assert!(outcome.as_ref().is_some_and(Result::is_ok), "{outcome:?}");
}
