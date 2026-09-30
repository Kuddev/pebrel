use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Subcommand};
use serde_json::{Value, json};

use super::{Error, MAX_JSON_BYTES, Package};

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
    /// Validate package metadata and entry paths without executing Lua.
    Check { path: PathBuf },
    /// Invoke one declared native or Lua command; no resident plugin is started.
    Run {
        path: PathBuf,
        command: String,
        /// JSON object overriding this command's default parameters.
        #[clap(long, default_value = "{}")]
        args: String,
        #[clap(long, default_value_t = 3000, value_parser = clap::value_parser!(u64).range(1..=30_000))]
        timeout_ms: u64,
    },
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
    let path = match command {
        Command::Check { path } | Command::Run { path, .. } => path,
    };
    let package = Package::open(path)?;
    match command {
        Command::Check { .. } => Ok(json!({
            "manifest": package.manifest,
            "lua_executed": false,
            "validation": "manifest_and_entry_paths",
        })),
        Command::Run { command, args, timeout_ms, .. } => {
            if args.len() > MAX_JSON_BYTES {
                return Err(Error::new("arguments_too_large", "arguments exceed 64 KiB"));
            }
            let args = serde_json::from_str(args)
                .map_err(|error| Error::new("invalid_arguments", error))?;
            let execution = super::execute(
                &package,
                command,
                args,
                Duration::from_millis(*timeout_ms),
                &mut |method, params, timeout| {
                    let response =
                        crate::runtime_api::request_once_bounded(method, params, timeout)
                            .map_err(runtime_error)?;
                    if !response.ok {
                        let error = response.error.ok_or_else(|| {
                            Error::new("invalid_runtime_response", "missing error")
                        })?;
                        return Err(runtime_error(error));
                    }
                    response
                        .result
                        .ok_or_else(|| Error::new("invalid_runtime_response", "missing result"))
                },
            )?;
            Ok(json!({ "plugin": package.manifest.id, "command": command, "execution": execution }))
        },
    }
}

fn runtime_error(error: crate::runtime_api::ApiError) -> Error {
    let mut result = Error::new(error.code, error.message);
    // 部分完成与清理回执属于 Runtime 的结果，插件适配层不得将它们折叠成字符串。
    result.details = error.details;
    result
}
