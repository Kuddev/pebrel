//! Process-local native plugin loading, with the original CLI owning its TTY.

use std::ffi::{OsStr, OsString};
use std::io::{self, Read as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use clap::{Args, ValueEnum};

#[derive(Args, Debug)]
pub(crate) struct Options {
    #[clap(value_enum)]
    pub agent: Agent,
    /// Arguments forwarded unchanged to the native agent.
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<OsString>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum Agent {
    Claude,
    Pi,
    Opencode,
}

impl Agent {
    pub(crate) fn executable(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Pi => "pi",
            Self::Opencode => "opencode",
        }
    }
}

/// Deliberately narrow: generated start/resume/fork commands, never a shell
/// parser. Unknown wrappers, paths, aliases and compound commands are untouched.
pub(crate) fn command_agent(command: &str) -> Option<Agent> {
    if command.contains(['\n', '\r', '\0', ';', '|', '&', '<', '>', '$', '`']) {
        return None;
    }
    match command.split_ascii_whitespace().next()? {
        "claude" => Some(Agent::Claude),
        "pi" => Some(Agent::Pi),
        "opencode" => Some(Agent::Opencode),
        _ => None,
    }
}

pub(crate) fn decorate(command: &str, quoted_executable: &str) -> Option<String> {
    command_agent(command)?;
    Some(format!("{} predict {}", quoted_executable.trim_end(), command.trim_start()))
}

fn agent_command(agent: Agent) -> Command {
    #[cfg(windows)]
    {
        use crate::ai_agents::AgentKind;

        let kind = match agent {
            Agent::Claude => AgentKind::Claude,
            Agent::Pi => AgentKind::Pi,
            Agent::Opencode => AgentKind::OpenCode,
        };
        if let Some(executable) = crate::ai_hook::integrations::executable(kind) {
            if executable.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("ps1"))
            {
                // -File preserves script argument boundaries; never construct
                // a PowerShell expression from native CLI arguments.
                let powershell = std::env::var_os("SystemRoot")
                    .map(std::path::PathBuf::from)
                    .map(|root| root.join("System32/WindowsPowerShell/v1.0/powershell.exe"))
                    .unwrap_or_else(|| "powershell.exe".into());
                let mut command = Command::new(powershell);
                command.args(["-NoLogo", "-NoProfile", "-File"]).arg(executable);
                return command;
            }
            // Explicit .cmd/.bat paths use std's batch argument escaping.
            // Bare program names only receive an implicit .exe search.
            return Command::new(executable);
        }
    }
    Command::new(agent.executable())
}

fn native_command(agent: Agent, args: &[OsString]) -> Command {
    let mut command = agent_command(agent);
    command.args(args);
    command
}

fn configured_command(
    agent: Agent,
    args: &[OsString],
    bundle: &Path,
    existing_tui_config: Option<&OsStr>,
) -> Result<Command, String> {
    let mut command = agent_command(agent);
    match agent {
        Agent::Claude => {
            // The verified native CLI takes one path per --plugin-dir flag.
            command.arg("--plugin-dir").arg(bundle.join("claude"));
            command.env("CLAUDE_CODE_ENABLE_FUNCTION_HOOKS", "1");
            command.env("CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION", "false");
        },
        Agent::Pi => {
            command.arg("--extension").arg(bundle.join("pi/index.ts"));
        },
        Agent::Opencode => {
            // V1 exposes one explicit TUI config. Copying JSONC would change
            // relative plugin origins and can drop includes/interpolation. Keep
            // an existing override intact instead of silently replacing it.
            if existing_tui_config.is_some_and(|value| !value.is_empty()) {
                return Err("an existing OPENCODE_TUI_CONFIG cannot be safely overlaid".into());
            }
            command.env("OPENCODE_TUI_CONFIG", bundle.join("opencode/tui.json"));
        },
    }
    command.args(args);
    Ok(command)
}

fn compatible_version(agent: Agent, output: &str) -> bool {
    let version = output.split_ascii_whitespace().find_map(|word| {
        let components: Vec<_> = word.split('.').collect();
        if components.len() != 3 {
            return None;
        }
        Some((
            components[0].parse::<u32>().ok()?,
            components[1].parse::<u32>().ok()?,
            components[2].parse::<u32>().ok()?,
        ))
    });
    match (agent, version) {
        (Agent::Claude, Some(version)) => version.0 == 2 && version >= (2, 1, 310),
        (Agent::Pi, Some(version)) => version.0 == 1 && version >= (1, 4, 0),
        (Agent::Opencode, Some(version)) => version.0 == 1 && version.1 == 19 && version.2 >= 5,
        _ => false,
    }
}

fn read_version(agent: Agent) -> io::Result<String> {
    let mut command = agent_command(agent);
    command.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    crate::platform::process::configure_process_group(&mut command);
    let mut child = command.spawn()?;
    let group = match crate::platform::process::ProcessGroup::attach(&child) {
        Ok(group) => group,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        },
    };
    let stdout = child.stdout.take().expect("piped version stdout");
    let reader =
        match std::thread::Builder::new().name("prediction-version".into()).spawn(move || {
            let mut bytes = Vec::new();
            stdout.take(4096).read_to_end(&mut bytes).map(|_| bytes)
        }) {
            Ok(reader) => reader,
            Err(error) => {
                group.terminate(&mut child);
                let _ = child.wait();
                return Err(error);
            },
        };
    let started = Instant::now();
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break Ok(()),
            Ok(Some(_)) => break Err(io::Error::other("native version query failed")),
            Err(error) => break Err(error),
            Ok(None) if started.elapsed() >= Duration::from_secs(2) => {
                break Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "native version query timed out",
                ));
            },
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    group.terminate(&mut child);
    let _ = child.wait();
    group.finish();
    let bytes = reader.join().map_err(|_| io::Error::other("native version reader failed"))??;
    result?;
    String::from_utf8(bytes).map_err(io::Error::other)
}

fn prediction_command(options: &Options) -> Result<Command, String> {
    if !["PEBREL_PREDICTION_ENDPOINT", "PEBREL_PREDICTION_TOKEN"]
        .into_iter()
        .all(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
    {
        return Err("no local Pebrel prediction bridge is available".into());
    }
    let version = read_version(options.agent).map_err(|error| error.to_string())?;
    if !compatible_version(options.agent, &version) {
        return Err("this native agent version has no verified prediction adapter".into());
    }
    let bundle = super::install::ensure()?;
    configured_command(
        options.agent,
        &options.args,
        &bundle,
        std::env::var_os("OPENCODE_TUI_CONFIG").as_deref(),
    )
}

pub(crate) fn run(options: Options) -> i32 {
    let settings = nebula_settings::RuntimeSettings::load();
    let language = crate::i18n::LanguagePreference::from(settings.language).resolved();
    let mut command = if settings.prompt_suggestions.enabled {
        match prediction_command(&options) {
            Ok(command) => command,
            Err(error) => {
                eprintln!(
                    "{}",
                    language.format(
                        crate::i18n::Message::PromptSuggestionsLauncherFallback,
                        &[("agent", options.agent.executable()), ("error", &error)]
                    )
                );
                native_command(options.agent, &options.args)
            },
        }
    } else {
        native_command(options.agent, &options.args)
    };
    // Inherit the current TTY, cwd and environment. No shell, PTY proxy or input
    // interception sits between the user and the native process.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        let error = command.exec();
        eprintln!(
            "{}",
            language.format(
                crate::i18n::Message::PromptSuggestionsLauncherFailed,
                &[("agent", options.agent.executable()), ("error", &error.to_string())]
            )
        );
        127
    }
    #[cfg(not(unix))]
    match command.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!(
                "{}",
                language.format(
                    crate::i18n::Message::PromptSuggestionsLauncherFailed,
                    &[("agent", options.agent.executable()), ("error", &error.to_string())]
                )
            );
            127
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser as _;

    #[test]
    fn clap_preserves_native_flags_and_argument_boundaries() {
        let parsed = crate::cli::Options::try_parse_from([
            "pebrel",
            "predict",
            "claude",
            "--resume",
            "abc",
            "--append-system-prompt",
            "literal $() 空格",
        ])
        .unwrap();
        let Some(crate::cli::Subcommands::Predict(options)) = parsed.subcommands else {
            panic!("prediction command")
        };
        assert_eq!(options.agent, Agent::Claude);
        assert_eq!(
            options.args,
            ["--resume", "abc", "--append-system-prompt", "literal $() 空格"].map(OsString::from)
        );
    }

    #[test]
    fn only_exact_single_agent_commands_are_decorated() {
        for command in [
            "claude",
            "claude --resume abc --fork-session",
            "pi --session '/my file.jsonl'",
            "opencode --session ses_123",
        ] {
            assert_eq!(
                decorate(command, "'/app dir/pebrel'"),
                Some(format!("'/app dir/pebrel' predict {command}"))
            );
        }
        for command in [
            "codex",
            "qoder",
            "env claude",
            "X=1 claude",
            "npx claude",
            "./claude",
            "'claude'",
            "claudecode",
            "claude; pwd",
            "claude && pwd",
            "claude\npi",
            "claude $(pwd)",
            "pebrel predict claude",
        ] {
            assert!(decorate(command, "pebrel").is_none(), "{command}");
        }
    }

    #[test]
    fn native_loading_does_not_modify_arguments_or_global_configuration() {
        let bundle = Path::new("/runtime/bundle with spaces");
        let args = [OsString::from("--resume"), OsString::from("session with spaces")];
        let cc = configured_command(Agent::Claude, &args, bundle, None).unwrap();
        let plugin = bundle.join("claude");
        assert_eq!(
            cc.get_args().collect::<Vec<_>>(),
            [OsStr::new("--plugin-dir"), plugin.as_os_str(), &args[0], &args[1]]
        );
        assert!(cc.get_envs().any(|(key, value)| key == "CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION"
            && value == Some(OsStr::new("false"))));
        let pi = configured_command(Agent::Pi, &args, bundle, None).unwrap();
        assert_eq!(pi.get_args().next(), Some(OsStr::new("--extension")));
        assert_eq!(pi.get_envs().count(), 0);
        let oc = configured_command(Agent::Opencode, &args, bundle, None).unwrap();
        let tui = bundle.join("opencode/tui.json");
        assert_eq!(
            oc.get_args().collect::<Vec<_>>(),
            args.iter().map(OsString::as_os_str).collect::<Vec<_>>()
        );
        assert_eq!(
            oc.get_envs().collect::<Vec<_>>(),
            [(OsStr::new("OPENCODE_TUI_CONFIG"), Some(tui.as_os_str()))]
        );
        assert!(
            configured_command(Agent::Opencode, &args, bundle, Some(OsStr::new("custom.jsonc")))
                .is_err()
        );
    }

    #[test]
    fn unknown_or_incompatible_versions_keep_native_startup() {
        assert!(compatible_version(Agent::Claude, "2.1.310 (Claude Code)"));
        assert!(compatible_version(Agent::Pi, "1.4.0\n"));
        assert!(compatible_version(Agent::Opencode, "1.19.5\n"));
        for (agent, text) in [
            (Agent::Claude, "2.1.309"),
            (Agent::Pi, "0.54.0"),
            (Agent::Opencode, "2.0.0"),
            (Agent::Opencode, "1.20.0"),
            (Agent::Opencode, "unknown"),
            (Agent::Opencode, "1.19.5-beta"),
        ] {
            assert!(!compatible_version(agent, text));
        }
    }
}
