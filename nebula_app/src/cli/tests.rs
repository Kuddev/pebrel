use super::*;

use std::fs::File;
use std::io::{Read, Write};

use clap::CommandFactory;
use clap_complete::Shell;
use toml::Table;

#[test]
fn ordinary_gui_launch_has_no_login_intent() {
    let options = Options::try_parse_from(["pebrel", "--gpui"]).unwrap();
    assert!(!options.launched_at_login);
    let terminal = &options.window_options.terminal_options;
    assert!(terminal.directory.is_empty());
    assert!(terminal.command.is_empty());
}

#[test]
fn dynamic_title_ignoring_options_by_default() {
    let mut config = UiConfig::default();
    let old_dynamic_title = config.window.dynamic_title;

    Options::default().override_config(&mut config);

    assert_eq!(old_dynamic_title, config.window.dynamic_title);
}

#[test]
fn dynamic_title_not_overridden_by_config() {
    let mut config = UiConfig::default();

    config.window.identity.title = "foo".to_owned();
    Options::default().override_config(&mut config);

    assert!(config.window.dynamic_title);
}

#[test]
fn valid_option_as_value() {
    // Test with a single field.
    let value: Value = toml::from_str("field=true").unwrap();

    let mut table = Table::new();
    table.insert(String::from("field"), Value::Boolean(true));

    assert_eq!(value, Value::Table(table));

    // Test with nested fields
    let value: Value = toml::from_str("parent.field=true").unwrap();

    let mut parent_table = Table::new();
    parent_table.insert(String::from("field"), Value::Boolean(true));
    let mut table = Table::new();
    table.insert(String::from("parent"), Value::Table(parent_table));

    assert_eq!(value, Value::Table(table));
}

#[test]
fn invalid_option_as_value() {
    let value = toml::from_str::<Value>("}");
    assert!(value.is_err());
}

#[test]
fn parses_config_check_and_init_subcommands() {
    let check =
        Options::try_parse_from(["pebrel", "config", "check", "--config-file", "sample.lua"])
            .unwrap();
    assert!(matches!(
        check.subcommands,
        Some(Subcommands::Config(ConfigOptions { command: ConfigCommand::Check(_) }))
    ));

    let init =
        Options::try_parse_from(["pebrel", "config", "init", "--language", "zh-CN"]).unwrap();
    assert!(matches!(
        init.subcommands,
        Some(Subcommands::Config(ConfigOptions {
            command: ConfigCommand::Init(ConfigInitOptions {
                language: ConfigLanguage::ZhCn,
                ..
            })
        }))
    ));
}

#[test]
fn parses_plugin_commands_without_selecting_the_gui() {
    use crate::plugins::cli::Command;
    let check =
        Options::try_parse_from(["pebrel", "plugin", "check", "sample", "--pretty"]).unwrap();
    assert!(matches!(
        check.subcommands,
        Some(Subcommands::Plugin(crate::plugins::cli::Options {
            command: Command::Check { .. },
            pretty: true
        }))
    ));
    let run = Options::try_parse_from([
        "pebrel",
        "plugin",
        "run",
        "sample",
        "summary",
        "--args",
        "{\"label\":\"中文\"}",
        "--timeout-ms",
        "1000",
    ])
    .unwrap();
    #[cfg(feature = "gpui-shell")]
    assert!(!crate::wants_gpui_shell(&run));
    assert!(matches!(
        run.subcommands,
        Some(Subcommands::Plugin(crate::plugins::cli::Options {
            command: Command::Run { .. },
            ..
        }))
    ));
    for timeout in ["0", "30001"] {
        assert!(
            Options::try_parse_from([
                "pebrel",
                "plugin",
                "run",
                "sample",
                "summary",
                "--timeout-ms",
                timeout
            ])
            .is_err()
        );
    }
}

#[test]
fn send_options_after_text_are_not_swallowed_by_the_positional() {
    let agent = Options::try_parse_from([
        "pebrel",
        "agent",
        "send",
        "codex",
        "fix",
        "login",
        "--no-submit",
        "--pretty",
    ])
    .unwrap();
    let Some(Subcommands::Agent(AgentOptions { command: AgentCommand::Send(agent) })) =
        agent.subcommands
    else {
        panic!("expected agent send");
    };
    assert_eq!(agent.text, ["fix", "login"]);
    assert!(agent.no_submit);
    assert!(agent.output.pretty);

    let pane = Options::try_parse_from([
        "pebrel",
        "pane",
        "send",
        "17",
        "cargo",
        "test",
        "--no-submit",
        "--pretty",
    ])
    .unwrap();
    let Some(Subcommands::Pane(PaneOptions { command: PaneCommand::Send(pane) })) =
        pane.subcommands
    else {
        panic!("expected pane send");
    };
    assert_eq!(pane.text, ["cargo", "test"]);
    assert!(pane.no_submit);
    assert!(pane.output.pretty);
}

#[test]
fn send_double_dash_preserves_hyphen_prefixed_text() {
    let parsed = Options::try_parse_from([
        "pebrel",
        "agent",
        "send",
        "codex",
        "--no-submit",
        "--",
        "--fix",
        "--pretty",
    ])
    .unwrap();
    let Some(Subcommands::Agent(AgentOptions { command: AgentCommand::Send(options) })) =
        parsed.subcommands
    else {
        panic!("expected agent send");
    };
    assert_eq!(options.text, ["--fix", "--pretty"]);
    assert!(options.no_submit);
    assert!(!options.output.pretty);
}

#[test]
fn paste_sources_are_explicit_and_mutually_exclusive() {
    let parsed = Options::try_parse_from([
        "pebrel",
        "pane",
        "paste",
        "17",
        "first",
        "line",
        "--no-submit",
    ])
    .unwrap();
    let Some(Subcommands::Pane(PaneOptions { command: PaneCommand::Paste(options) })) =
        parsed.subcommands
    else {
        panic!("expected pane paste");
    };
    assert_eq!(options.source.text, ["first", "line"]);
    assert!(!options.source.stdin);
    assert_eq!(options.source.from_file, None);
    assert!(options.no_submit);

    let parsed =
        Options::try_parse_from(["pebrel", "agent", "paste", "codex", "--stdin", "--wait"])
            .unwrap();
    let Some(Subcommands::Agent(AgentOptions { command: AgentCommand::Paste(options) })) =
        parsed.subcommands
    else {
        panic!("expected agent paste");
    };
    assert!(options.source.stdin);
    assert!(options.source.text.is_empty());
    assert!(options.wait);

    assert!(
        Options::try_parse_from([
            "pebrel",
            "pane",
            "paste",
            "17",
            "literal",
            "--from-file",
            "task.txt",
        ])
        .is_err()
    );
    assert!(
        Options::try_parse_from([
            "pebrel",
            "agent",
            "paste",
            "codex",
            "--stdin",
            "--from-file",
            "task.txt",
        ])
        .is_err()
    );
}

#[test]
fn layout_resource_commands_keep_ids_and_explicit_zoom_state() {
    let parsed = Options::try_parse_from([
        "pebrel", "tab", "move", "2", "0", "--window", "7", "--pretty",
    ])
    .unwrap();
    assert!(matches!(
        parsed.subcommands,
        Some(Subcommands::Tab(TabResourceOptions {
            command: TabCommand::Move(TabMoveOptions {
                tab: 2,
                to: 0,
                window: 7,
                output: ShortOutput { pretty: true },
                ..
            })
        }))
    ));

    let parsed = Options::try_parse_from([
        "pebrel", "pane", "zoom", "17", "--zoomed", "false", "--window", "7",
    ])
    .unwrap();
    assert!(matches!(
        parsed.subcommands,
        Some(Subcommands::Pane(PaneOptions {
            command: PaneCommand::Zoom(PaneZoomOptions {
                pane: 17,
                window: Some(7),
                zoomed: false,
                ..
            })
        }))
    ));

    // 省略状态不是“默认关闭”：调用方必须明确表达期望状态，才能安全重试。
    assert!(Options::try_parse_from(["pebrel", "pane", "zoom", "17"]).is_err());
}

#[test]
fn default_terminal_options_preserve_platform_pty_defaults() {
    assert_eq!(PtyOptions::from(TerminalOptions::default()), PtyOptions::default());
}

#[test]
fn float_option_as_value() {
    let value: Value = toml::from_str("float=3.4").unwrap();

    let mut expected = Table::new();
    expected.insert(String::from("float"), Value::Float(3.4));

    assert_eq!(value, Value::Table(expected));
}

#[test]
fn parse_instance_class() {
    let class = parse_class("one").unwrap();
    assert_eq!(class.general, "one");
    assert_eq!(class.instance, "one");
}

#[test]
fn parse_general_class() {
    let class = parse_class("one,two").unwrap();
    assert_eq!(class.general, "one");
    assert_eq!(class.instance, "two");
}

#[test]
fn parse_invalid_class() {
    let class = parse_class("one,two,three");
    assert!(class.is_err());
}

#[cfg(windows)]
#[test]
fn repairs_drive_root_mangled_by_explorer_quoting() {
    // `--working-directory "D:\"` reaches us as `D:"` because the trailing
    // backslash escaped the closing quote (issue #36); restore the root.
    assert_eq!(repair_context_menu_dir(PathBuf::from("D:\"")), PathBuf::from("D:\\"));
    assert_eq!(
        repair_context_menu_dir(PathBuf::from("\\\\server\\share\"")),
        PathBuf::from("\\\\server\\share\\")
    );
    // A normal folder (no trailing backslash, so no mangling) is untouched.
    assert_eq!(
        repair_context_menu_dir(PathBuf::from("D:\\temp_build")),
        PathBuf::from("D:\\temp_build")
    );
}

/// 右键菜单那条命令行的形状：`--shell` 必须排在 `--working-directory` **之前**。
///
/// 这不是风格问题。`--working-directory "D:\"` 的收尾反斜杠会吃掉它的收尾
/// 引号（issue #36），若它排在前头，`CommandLineToArgvW` 会把 `--shell` 整段
/// 并进那个参数——目录变成垃圾、shell 被吞掉，而 clap 依然认为解析成功，
/// 最后开出一个没有 cwd 的默认 shell 标签。安装器里的书写顺序由
/// `scripts/tests/installer.tests.ps1` 的断言钉住，这里钉住解析侧。
#[test]
fn shell_flag_parses_ahead_of_the_working_directory() {
    let options = Options::try_parse_from([
        "pebrel",
        "--gpui",
        "--shell",
        "wsl:Ubuntu",
        "--working-directory",
        "D:\"",
    ])
    .expect("context-menu argv parses");
    let terminal = &options.window_options.terminal_options;
    assert_eq!(terminal.shell_id().as_deref(), Some("wsl:Ubuntu"));
    let expected = if cfg!(windows) { "D:\\" } else { "D:\"" };
    assert_eq!(terminal.resolved_working_directory(), Some(PathBuf::from(expected)));
}

/// `--shell` selects a saved shell identity; `-e` supplies a command directly.
/// Reject their combination instead of silently ignoring either explicit request.
#[test]
fn shell_and_command_conflict() {
    assert!(
        Options::try_parse_from(["pebrel", "--gpui", "--shell", "pwsh", "-e", "cmd"]).is_err()
    );
}

#[test]
fn desktop_directory_keeps_command_and_subcommand_parsing() {
    let options = Options::try_parse_from(["pebrel", "/tmp/a folder"]).unwrap();
    assert_eq!(
        options.window_options.terminal_options.resolved_working_directory(),
        Some(PathBuf::from("/tmp/a folder"))
    );
    // A multi-select verb passes the clicked folder through
    // `--working-directory` and the rest as positionals, so both shapes must
    // survive parsing and come back in order.
    let multi = Options::try_parse_from([
        "pebrel",
        "/tmp/third",
        "--working-directory",
        "/tmp/first",
        "/tmp/second",
    ])
    .unwrap();
    assert_eq!(
        multi.window_options.terminal_options.startup_directories(),
        ["/tmp/first", "/tmp/third", "/tmp/second"].map(PathBuf::from).to_vec()
    );
    let command = Options::try_parse_from(["pebrel", "-e", "sh", "-c", "echo ok"]).unwrap();
    assert!(command.window_options.terminal_options.directory.is_empty());
    assert_eq!(command.window_options.terminal_options.command, ["sh", "-c", "echo ok"]);
    assert!(
        Options::try_parse_from(["pebrel", "config", "--help"]).unwrap_err().use_stderr()
            == false
    );
    let legacy = serde_json::json!({"working_directory": null, "shell": null, "hold": false, "command": []});
    assert!(serde_json::from_value::<TerminalOptions>(legacy).unwrap().directory.is_empty());
    // A saved option set from before multi-select carries one plain string.
    let older = serde_json::json!({"directory": "/tmp/only", "hold": false, "command": []});
    assert_eq!(
        serde_json::from_value::<TerminalOptions>(older).unwrap().directory,
        [PathBuf::from("/tmp/only")]
    );
}

#[test]
fn startup_directories_deduplicate_and_drop_blanks() {
    let options = Options::try_parse_from([
        "pebrel",
        "/tmp/a",
        "/tmp/a",
        "--working-directory",
        "/tmp/a",
        "/tmp/b",
    ])
    .unwrap();
    assert_eq!(
        options.window_options.terminal_options.startup_directories(),
        ["/tmp/a", "/tmp/b"].map(PathBuf::from).to_vec()
    );
}

/// 空白/空的 `--shell` 当作没给：脚本传了空串时不该去解析一个空 id，
/// 更不该因此报错挡住启动。
#[test]
fn blank_shell_id_counts_as_absent() {
    for value in ["", "   "] {
        let options = Options::try_parse_from(["pebrel", "--gpui", "--shell", value])
            .expect("blank shell parses");
        assert_eq!(options.window_options.terminal_options.shell_id(), None);
    }
}

#[test]
fn valid_decimal() {
    let value = parse_hex_or_decimal("10485773");
    assert_eq!(value, Some(10485773));
}

#[test]
fn valid_hex_to_decimal() {
    let value = parse_hex_or_decimal("0xa0000d");
    assert_eq!(value, Some(10485773));
}

#[test]
fn invalid_hex_to_decimal() {
    let value = parse_hex_or_decimal("0xa0xx0d");
    assert_eq!(value, None);
}

#[test]
fn completions() {
    let mut clap = Options::command();

    for (shell, file) in
        &[(Shell::Bash, "pebrel.bash"), (Shell::Fish, "pebrel.fish"), (Shell::Zsh, "_pebrel")]
    {
        let mut generated = Vec::new();
        clap_complete::generate(*shell, &mut clap, "pebrel", &mut generated);
        let generated = String::from_utf8_lossy(&generated);

        let mut completion = String::new();
        let mut file = File::open(completion_directory().join(file)).unwrap();
        file.read_to_string(&mut completion).unwrap();

        assert_eq!(generated, completion);
    }
}

fn completion_directory() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../extra/completions");
    if crate::platform::Platform::current() == crate::platform::Platform::Windows {
        root.join("windows")
    } else {
        root
    }
}

#[test]
#[ignore = "maintenance command: rewrites checked-in shell completions"]
fn regenerate_completions() {
    let mut clap = Options::command();
    let directory = std::env::var_os("PEBREL_COMPLETION_OUTPUT")
        .or_else(|| std::env::var_os("NEBULA_COMPLETION_OUTPUT"))
        .map(PathBuf::from)
        .unwrap_or_else(completion_directory);
    std::fs::create_dir_all(&directory).expect("create completion directory");
    for (shell, file) in
        &[(Shell::Bash, "pebrel.bash"), (Shell::Fish, "pebrel.fish"), (Shell::Zsh, "_pebrel")]
    {
        let mut generated = Vec::new();
        clap_complete::generate(*shell, &mut clap, "pebrel", &mut generated);
        File::create(directory.join(file))
            .and_then(|mut file| file.write_all(&generated))
            .expect("write generated completion");
    }
}
