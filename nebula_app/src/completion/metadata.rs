//! Owned metadata execution scopes; queries never run in an interactive PTY.

use std::time::{Duration, Instant};

use crate::display::SuggestEnv;
use crate::runtime_exec::PaneExecContext;

#[derive(Clone)]
pub(crate) enum Execution {
    Process { context: PaneExecContext, scope: SuggestEnv },
    Ssh { destination: String, connection: Option<crate::ssh_session::completion::Connection> },
}

impl Execution {
    /// Freeze an authenticated transport on the worker before consulting caches.
    pub(super) fn prepare(&mut self) -> bool {
        match self {
            Self::Process { .. } => true,
            Self::Ssh { destination, connection } => {
                let Some(runtime) = crate::ssh_session::runtime().ok() else {
                    return false;
                };
                *connection = runtime.block_on(async {
                    tokio::time::timeout(
                        Duration::from_millis(100),
                        crate::ssh_session::completion::capture(destination),
                    )
                    .await
                    .ok()?
                    .ok()
                });
                connection.is_some()
            },
        }
    }

    pub(crate) fn paths_not_found(
        &self,
        cwd: &str,
        directories: &[String],
        names: &[String],
        cancelled: &dyn Fn() -> bool,
    ) -> Option<std::collections::HashSet<String>> {
        if names.is_empty() {
            return Some(Default::default());
        }
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(
            serde_json::to_vec(
                &serde_json::json!({"cwd":cwd, "directories":directories, "names":names}),
            )
            .ok()?,
        );
        let script = format!(
            "import base64,json,os\nc=json.loads(base64.b64decode('{encoded}'))\np=c['cwd']\nfor directory in c['directories']: p=os.path.join(p,directory)\nmissing=[]\nfor name in c['names']:\n try: os.lstat(os.path.join(p,name))\n except FileNotFoundError: missing.append(name)\n except OSError: pass\nprint(json.dumps(missing))\n"
        );
        let argv = ["python3", "-I", "-S", "-"].map(str::to_owned);
        let bytes = self.read(
            cwd,
            &argv,
            script.as_bytes(),
            Duration::from_secs(1),
            16 * 1024,
            false,
            cancelled,
        )?;
        serde_json::from_slice(&bytes).ok()
    }

    pub(crate) fn from_scope(
        scope: &SuggestEnv,
        execution: Option<&PaneExecContext>,
    ) -> Option<Self> {
        match scope {
            SuggestEnv::Local => Some(Self::Process {
                context: execution.cloned().unwrap_or_else(|| {
                    PaneExecContext::from_pty_options(&nebula_terminal::tty::Options::default())
                }),
                scope: scope.clone(),
            }),
            SuggestEnv::Wsl { distro } if !distro.is_empty() => {
                let context =
                    execution.and_then(|e| e.for_wsl_distribution(distro)).unwrap_or_else(|| {
                        PaneExecContext::from_pty_options(&nebula_terminal::tty::Options {
                            shell: Some(nebula_terminal::tty::Shell::new(
                                "wsl.exe".into(),
                                vec!["--distribution".into(), distro.clone()],
                            )),
                            ..Default::default()
                        })
                    });
                Some(Self::Process { context, scope: scope.clone() })
            },
            SuggestEnv::Ssh { destination } => {
                Some(Self::Ssh { destination: destination.clone(), connection: None })
            },
            _ => None,
        }
    }

    pub(crate) fn key(&self) -> String {
        match self {
            Self::Process { context, scope } => format!("{scope:?}\0{:?}", context.wsl_user()),
            Self::Ssh { destination, connection } => format!(
                "ssh:{destination}\0{:?}",
                connection.as_ref().map(crate::ssh_session::completion::Connection::key)
            ),
        }
    }

    pub(crate) fn is_host(&self) -> bool {
        matches!(self, Self::Process { scope: SuggestEnv::Local, .. })
    }

    pub(crate) fn git(
        &self,
        cwd: &str,
        directories: &[String],
        args: &[&str],
        deadline: Instant,
        limit: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<Vec<u8>> {
        let mut argv =
            ["git", "-c", "core.warnAmbiguousRefs=true", "-c", "completion.snapshot=true"]
                .map(str::to_owned)
                .to_vec();
        for directory in directories {
            argv.extend(["-C".into(), directory.clone()]);
        }
        argv.extend(args.iter().map(|arg| (*arg).to_owned()));
        self.read(
            cwd,
            &argv,
            &[],
            deadline.saturating_duration_since(Instant::now()),
            limit,
            true,
            cancelled,
        )
    }

    pub(super) fn read(
        &self,
        cwd: &str,
        argv: &[String],
        script: &[u8],
        budget: Duration,
        limit: usize,
        git: bool,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<Vec<u8>> {
        if cancelled() || budget.is_zero() {
            return None;
        }
        let env = [
            "GIT_OPTIONAL_LOCKS=0",
            "GIT_TERMINAL_PROMPT=0",
            "GIT_NO_LAZY_FETCH=1",
            "GIT_ALLOW_PROTOCOL=",
        ];
        match self {
            Self::Process { context, scope } => {
                let guest = !scope.is_this_machine();
                if guest && !cwd.starts_with('/') {
                    return None;
                }
                let guest_argv;
                let args = if guest && git {
                    guest_argv = std::iter::once("env".to_owned())
                        .chain(env.iter().map(|s| (*s).to_owned()))
                        .chain(argv.iter().cloned())
                        .collect::<Vec<_>>();
                    &guest_argv
                } else {
                    argv
                };
                let (mut command, _) =
                    crate::runtime_exec::build_command(context, cwd, args).ok()?;
                if git {
                    for setting in env {
                        let (key, value) = setting.split_once('=').unwrap();
                        command.env(key, value);
                    }
                }
                if !script.is_empty() {
                    // Python receives a bounded, isolated program through a private temporary file.
                    let mut file = tempfile::tempfile().ok()?;
                    use std::io::{Seek as _, Write as _};
                    file.write_all(script).ok()?;
                    file.rewind().ok()?;
                    command.stdin(file);
                    return crate::platform::process_output::read_cancellable_with_stdin(
                        command, budget, limit, cancelled,
                    )
                    .ok();
                }
                crate::platform::process_output::read_cancellable(command, budget, limit, cancelled)
                    .ok()
            },
            Self::Ssh { connection, .. } => {
                if !cwd.starts_with('/') || cwd.chars().any(char::is_control) {
                    return None;
                }
                let command = if script.is_empty() {
                    let words = std::iter::once("exec".to_owned())
                        .chain(git.then(|| "env".to_owned()))
                        .chain(env.iter().filter(|_| git).map(|s| quote(s)))
                        .chain(argv.iter().map(|s| quote(s)))
                        .collect::<Vec<_>>()
                        .join(" ");
                    format!("cd -- {} && {words}", quote(cwd))
                } else {
                    "python3 -I -S -".to_owned()
                };
                crate::ssh_session::runtime()
                    .ok()?
                    .block_on(crate::ssh_session::completion::read_connection(
                        connection.as_ref()?,
                        &command,
                        script,
                        budget,
                        limit,
                        cancelled,
                    ))
                    .ok()
            },
        }
    }
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
