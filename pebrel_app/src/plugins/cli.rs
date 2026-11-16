use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Subcommand};
use serde_json::{Value, json};

use super::install::Library;
use super::package::{Grant, Output, identifier};
use super::{Error, MAX_JSON_BYTES, Package, themes};

#[derive(Args, Debug)]
pub(crate) struct Options {
    #[clap(subcommand)]
    pub command: Command,
    /// Pretty-print the JSON result.
    #[clap(long, global = true)]
    pub pretty: bool,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// Validate package metadata and entry paths without executing Luau.
    Check { path: PathBuf },
    /// Invoke one declared native or Luau command; no resident plugin is started.
    Run {
        /// A package directory, or the ID of an installed plugin when no such path exists.
        path: PathBuf,
        command: String,
        /// JSON object overriding this command's default parameters.
        #[clap(long, default_value = "{}")]
        args: String,
        #[clap(long, default_value_t = 3000, value_parser = clap::value_parser!(u64).range(1..=30_000))]
        timeout_ms: u64,
    },
    /// Copy a package into the plugin library and record the grants given here.
    Install {
        path: PathBuf,
        /// Grant a capability the manifest requests: `theme-library`.
        #[clap(long = "grant", value_parser = parse_grant)]
        grants: Vec<Grant>,
    },
    /// List installed plugins, their grants and theme commands.
    List,
    /// Let an installed plugin run again.
    Enable { id: String },
    /// Stop an installed plugin from running; its files and themes stay.
    Disable { id: String },
    /// Remove an installed plugin. Themes it added stay in the theme library.
    Uninstall { id: String },
}

fn parse_grant(value: &str) -> Result<Grant, String> {
    Grant::parse(value).ok_or_else(|| format!("unknown grant: {value} (expected theme-library)"))
}

pub(crate) fn run(options: Options) -> i32 {
    let result = execute(&options.command);
    let success = result.is_ok();
    let output = match result {
        Ok(value) => json!({ "ok": true, "result": value }),
        Err(error) => json!({ "ok": false, "error": error }),
    };
    let mut stdout = std::io::stdout().lock();
    let written = if options.pretty {
        serde_json::to_writer_pretty(&mut stdout, &output)
    } else {
        serde_json::to_writer(&mut stdout, &output)
    };
    if let Err(error) = written {
        eprintln!("Plugin output failed: {error}");
        return 1;
    }
    if stdout.write_all(b"\n").and_then(|()| stdout.flush()).is_err() {
        return 1;
    }
    i32::from(!success)
}

fn execute(command: &Command) -> super::Result<Value> {
    let library = Library::default();
    match command {
        Command::Check { path } => {
            let package = Package::open(path)?;
            Ok(json!({
                "manifest": package.manifest,
                "requested_grants": package.manifest.requested_grants(),
                "lua_executed": false,
                "validation": "manifest_and_entry_paths",
                "host_runtime_methods": super::boundary::RUNTIME_METHODS,
            }))
        },
        Command::Run { path, command, args, timeout_ms } => {
            if args.len() > MAX_JSON_BYTES {
                return Err(Error::new("arguments_too_large", "arguments exceed 64 KiB"));
            }
            let args = serde_json::from_str(args)
                .map_err(|error| Error::new("invalid_arguments", error))?;
            // 只有已安装插件带着安装时的授权；直接运行包目录永远没有授权。
            let (package, grants) = match path.to_str() {
                Some(id) if !path.exists() && identifier(id) => {
                    let (package, grants) = library.open_enabled(id)?;
                    (package, Some(grants))
                },
                _ => (Package::open(path)?, None),
            };
            let mut execution = super::execute(
                &package,
                command,
                args,
                Duration::from_millis(*timeout_ms),
                &mut super::runtime_call,
            )?;
            if package
                .manifest
                .commands
                .iter()
                .any(|item| item.id == *command && item.output == Output::Theme)
            {
                execution.result = themes::finish_for_user(
                    execution.result,
                    &package,
                    command,
                    grants.as_deref(),
                )?;
            }
            Ok(json!({ "plugin": package.manifest.id, "command": command, "execution": execution }))
        },
        Command::Install { path, grants } => {
            let plugin = library.install(path, grants)?;
            let themes = themes::install_bundled_for_user(&library, &plugin.id)?;
            Ok(json!({ "plugin": plugin, "themes": themes }))
        },
        Command::List => Ok(serde_json::to_value(library.list()?).expect("listing is JSON")),
        Command::Enable { id } => Ok(json!({ "plugin": library.set_enabled(id, true)? })),
        Command::Disable { id } => Ok(json!({ "plugin": library.set_enabled(id, false)? })),
        Command::Uninstall { id } => {
            library.uninstall(id)?;
            Ok(json!({ "uninstalled": id, "themes_kept": true }))
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../examples/plugins/native-overview/plugin.toml");

    #[test]
    fn install_takes_explicit_known_grants_and_library_verbs_parse() {
        use crate::cli::{Options as Cli, Subcommands};
        use clap::Parser as _;
        let install = Cli::try_parse_from([
            "pebrel",
            "plugin",
            "install",
            "sample",
            "--grant",
            "theme-library",
        ])
        .unwrap();
        assert!(matches!(
            install.subcommands,
            Some(Subcommands::Plugin(Options { command: Command::Install { ref grants, .. }, .. }))
                if *grants == [Grant::ThemeLibrary]
        ));
        assert!(
            Cli::try_parse_from(["pebrel", "plugin", "install", "sample", "--grant", "all"])
                .is_err()
        );
        for verb in [&["list"][..], &["enable", "a.b"], &["disable", "a.b"], &["uninstall", "a.b"]]
        {
            let arguments = ["pebrel", "plugin"].iter().chain(verb).copied();
            assert!(Cli::try_parse_from(arguments).is_ok(), "{verb:?}");
        }
    }

    #[test]
    fn check_reports_the_host_boundary_without_running_a_plugin() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("plugin.toml"), EXAMPLE).unwrap();
        let result = execute(&Command::Check { path: dir.path().to_owned() }).unwrap();
        assert_eq!(result["lua_executed"], false);
        assert_eq!(result["host_runtime_methods"], json!(["runtime.describe"]));
    }

    #[test]
    fn check_and_run_reject_self_granted_core_control_before_transport() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("plugin.toml"),
            EXAMPLE.replace("runtime.describe", "window.close"),
        )
        .unwrap();
        for command in [
            Command::Check { path: dir.path().to_owned() },
            Command::Run {
                path: dir.path().to_owned(),
                command: "describe".to_owned(),
                args: "{}".to_owned(),
                timeout_ms: 1000,
            },
        ] {
            assert_eq!(execute(&command).unwrap_err().code, "unsupported_plugin_capability");
        }
    }
}
