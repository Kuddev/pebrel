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

    pub(super) fn project(
        &self,
        cwd: &str,
        context: &pebrel_completions::semantic::Context,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<Vec<String>> {
        use base64::Engine as _;
        use pebrel_completions::semantic::Source;
        let workspace = context.project.all
            || !context.project.selectors.is_empty()
            || matches!(context.source, Source::Workspaces);
        let deadline = Instant::now() + Duration::from_secs(3);
        let probe = |config: serde_json::Value| -> Option<serde_json::Value> {
            let encoded =
                base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&config).ok()?);
            let script = format!("INPUT = '{encoded}'\n{}", include_str!("project_probe.py"));
            let argv = ["python3", "-I", "-S", "-"].map(str::to_owned);
            let bytes = self.read(
                cwd,
                &argv,
                script.as_bytes(),
                deadline.saturating_duration_since(Instant::now()),
                4 * 1024 * 1024,
                false,
                cancelled,
            )?;
            if cancelled() {
                return None;
            }
            serde_json::from_slice(&bytes).ok()
        };
        let header = probe(
            serde_json::json!({"mode":"root", "cwd":cwd, "directories":context.directories, "workspace":workspace, "manager":format!("{:?}", context.project.manager).to_ascii_lowercase()}),
        )?;
        if !workspace {
            return Some(
                header.get("manifest")?.get("scripts")?.as_object()?.keys().cloned().collect(),
            );
        }
        let patterns = super::project_scripts::workspace::patterns(
            header.get("manifest")?,
            header.get("yaml").and_then(serde_json::Value::as_str),
            context.project.manager,
        )?;
        let prefixes: Vec<_> = patterns
            .iter()
            .filter(|pattern| !pattern.starts_with('!'))
            .map(|pattern| {
                pattern.split(['*', '?', '[', '{']).next().unwrap_or_default().trim_end_matches('/')
            })
            .collect();
        let mut catalog = probe(
            serde_json::json!({"mode":"packages", "cwd":header.get("root")?, "prefixes":prefixes, "remaining":header.get("remaining").unwrap_or(&serde_json::Value::from(4 * 1024 * 1024))}),
        )?;
        let packages = catalog.as_array_mut()?;
        packages.retain(|package| {
            package.get("path").and_then(serde_json::Value::as_str).is_some_and(|path| {
                patterns.iter().filter(|p| !p.starts_with('!')).any(|p| {
                    glob::Pattern::new(p).is_ok_and(|p| {
                        p.matches_with(
                            path,
                            glob::MatchOptions {
                                require_literal_separator: true,
                                require_literal_leading_dot: true,
                                case_sensitive: true,
                            },
                        )
                    })
                }) && !patterns.iter().filter_map(|p| p.strip_prefix('!')).any(|p| {
                    glob::Pattern::new(p).is_ok_and(|p| {
                        p.matches_with(
                            path,
                            glob::MatchOptions {
                                require_literal_separator: true,
                                require_literal_leading_dot: true,
                                case_sensitive: true,
                            },
                        )
                    })
                })
            })
        });
        if context.project.include_root
            || context.project.manager == pebrel_completions::semantic::PackageManager::Pnpm
                && header.get("yaml").is_none_or(serde_json::Value::is_null)
        {
            packages.push(serde_json::json!({"path":".", "manifest":header.get("manifest")?}));
        }
        super::project_scripts::workspace::finish_catalog(&catalog, context)
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::completion::{Cancellation, Session};
    use crate::display::CompletionStyle;

    #[test]
    #[ignore = "requires WSL, PEBREL_COMPLETION_QA_MANAGERS and PEBREL_COMPLETION_QA_GUEST_NODE"]
    fn real_wsl_workspace_scripts_execute_the_accepted_pnpm_and_yarn_candidates() {
        let distro = crate::platform::shell::registered_wsl_distros(&|| false)
            .into_iter()
            .find(|name| !name.starts_with("docker-desktop"))
            .expect("WSL distro");
        let managers = std::env::var("PEBREL_COMPLETION_QA_MANAGERS")
            .expect("isolated package manager directory");
        let node = std::env::var("PEBREL_COMPLETION_QA_GUEST_NODE").expect("guest Node binary");
        assert!(managers.starts_with('/') && node.starts_with('/'));
        let script = r#"import tempfile,pathlib,json,sys,shlex
p=pathlib.Path(tempfile.mkdtemp(prefix='pebrel-completion-managers-qa-'))
(p/'package.json').write_text(json.dumps({'name':'fixture-root','private':True,'workspaces':['packages/*']}))
(p/'pnpm-workspace.yaml').write_text('packages:\n  - packages/*\n')
q=p/'packages/app'; q.mkdir(parents=True)
scripts={}
for manager in ['pnpm','yarn']:
 for mode in ['inline','popup','hybrid']:
  name=manager+'-'+mode
  scripts['qa-'+name]=shlex.quote(sys.argv[1])+' -e "require(\'fs\').writeFileSync(\'../../.qa-'+name+'\', \'executed\')"'
(q/'package.json').write_text(json.dumps({'name':'@fixture/app','version':'1.0.0','scripts':scripts}))
print(p)
"#;
        let output = std::process::Command::new("wsl.exe")
            .args(["--distribution", &distro, "--exec", "python3", "-I", "-S", "-c", script, &node])
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let cwd = String::from_utf8(output.stdout).unwrap().trim().to_owned();
        let env = SuggestEnv::Wsl { distro: distro.clone() };
        let session = Session::default();
        let execution = Execution::from_scope(&env, None).unwrap();
        let markers: Vec<_> = ["pnpm", "yarn"]
            .into_iter()
            .flat_map(|manager| {
                ["inline", "popup", "hybrid"].map(move |mode| format!(".qa-{manager}-{mode}"))
            })
            .collect();
        for manager in ["pnpm", "yarn"] {
            for (style, mode) in [
                (CompletionStyle::Inline, "inline"),
                (CompletionStyle::Popup, "popup"),
                (CompletionStyle::Hybrid, "hybrid"),
            ] {
                let prefix = if manager == "pnpm" {
                    format!("pnpm --filter app run qa-pnpm-{}", &mode[..2])
                } else {
                    format!("yarn workspace @fixture/app run qa-yarn-{}", &mode[..2])
                };
                let result = session
                    .request_with_syntax(
                        cwd.clone(),
                        env.clone(),
                        prefix.clone(),
                        prefix.len(),
                        style,
                        None,
                        Some(pebrel_completions::command_context::ShellSyntax::Posix),
                    )
                    .calculate(&Cancellation::default());
                let edit = if style == CompletionStyle::Popup {
                    result.completion_items.first().expect("workspace candidate")
                } else {
                    result.suggestion_edit.as_ref().expect("workspace candidate")
                };
                let head: String =
                    prefix.chars().take(prefix.chars().count() - edit.replace_chars).collect();
                let accepted = format!("{head}{}", edit.insert);
                assert!(accepted.ends_with(&format!("qa-{manager}-{mode}")), "{accepted}");
                let marker = format!(".qa-{manager}-{mode}");
                assert!(
                    execution
                        .paths_not_found(&cwd, &[], std::slice::from_ref(&marker), &|| false)
                        .unwrap()
                        .contains(&marker),
                    "candidate discovery never executes the script"
                );
                let launcher = if manager == "pnpm" {
                    format!("{managers}/node_modules/pnpm/bin/pnpm.cjs")
                } else {
                    format!("{managers}/node_modules/yarn/bin/yarn.js")
                };
                let output = std::process::Command::new("wsl.exe")
                    .args(["--distribution", &distro, "--cd", &cwd, "--exec", &node, &launcher])
                    .args(accepted.split_whitespace().skip(1))
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{accepted}: {} {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(
                    !execution
                        .paths_not_found(&cwd, &[], std::slice::from_ref(&marker), &|| false)
                        .unwrap()
                        .contains(&marker)
                );
            }
        }
        assert!(execution.paths_not_found(&cwd, &[], &markers, &|| false).unwrap().is_empty());
        let cleanup = std::process::Command::new("wsl.exe")
            .args([
                "--distribution",
                &distro,
                "--exec",
                "python3",
                "-I",
                "-S",
                "-c",
                "import shutil,sys; shutil.rmtree(sys.argv[1])",
                &cwd,
            ])
            .status()
            .unwrap();
        assert!(cleanup.success());
    }

    #[test]
    #[ignore = "requires a runnable WSL distribution and guest git/python"]
    fn real_wsl_metadata_and_accepted_git_command_end_to_end() {
        let distro = crate::platform::shell::registered_wsl_distros(&|| false)
            .into_iter()
            .find(|name| !name.starts_with("docker-desktop"))
            .expect("WSL distro");
        let script = r#"import tempfile, pathlib, json, subprocess
p=pathlib.Path(tempfile.mkdtemp(prefix='pebrel-completion-qa-'))
(p/'package.json').write_text(json.dumps({'name':'fixture','workspaces':['packages/*'],'scripts':{'guest-task':'touch forbidden'}}))
q=p/'packages/app'; q.mkdir(parents=True); (q/'package.json').write_text(json.dumps({'name':'app','scripts':{'workspace-task':'touch forbidden'}}))
for args in [['init','-b','main'],['add','.'],['-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-m','fixture'],['branch','feature/guest']]:
 subprocess.run(['git',*args],cwd=p,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
print(p)
"#;
        let output = std::process::Command::new("wsl.exe")
            .args(["--distribution", &distro, "--exec", "python3", "-I", "-S", "-c", script])
            .output()
            .unwrap();
        assert!(output.status.success(), "guest fixture creation");
        let cwd = String::from_utf8(output.stdout).unwrap().trim().to_owned();
        let env = SuggestEnv::Wsl { distro: distro.clone() };
        let session = Session::default();
        for style in [CompletionStyle::Inline, CompletionStyle::Popup, CompletionStyle::Hybrid] {
            for (line, expected) in [
                ("git switch feature/gue", "git switch feature/guest"),
                ("npm run gue", "npm run guest-task"),
                ("npm -w app run work", "npm -w app run workspace-task"),
            ] {
                let result = session
                    .request(cwd.clone(), env.clone(), line.into(), style, None)
                    .calculate(&Cancellation::default());
                let edit = if style == CompletionStyle::Popup {
                    result.completion_items.first().unwrap()
                } else {
                    result.suggestion_edit.as_ref().unwrap()
                };
                let head: String =
                    line.chars().take(line.chars().count() - edit.replace_chars).collect();
                assert_eq!(format!("{head}{}", edit.insert), expected, "{style:?}: {line}");
            }
        }
        let before = std::process::Command::new("wsl.exe")
            .args([
                "--distribution",
                &distro,
                "--cd",
                &cwd,
                "--exec",
                "git",
                "branch",
                "--show-current",
            ])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(before.stdout).unwrap().trim(), "main");
        let applied = std::process::Command::new("wsl.exe")
            .args([
                "--distribution",
                &distro,
                "--cd",
                &cwd,
                "--exec",
                "git",
                "switch",
                "feature/guest",
            ])
            .status()
            .unwrap();
        assert!(applied.success());
        let after = std::process::Command::new("wsl.exe")
            .args([
                "--distribution",
                &distro,
                "--cd",
                &cwd,
                "--exec",
                "git",
                "branch",
                "--show-current",
            ])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(after.stdout).unwrap().trim(), "feature/guest");
        let absent = std::process::Command::new("wsl.exe")
            .args([
                "--distribution",
                &distro,
                "--exec",
                "test",
                "!",
                "-e",
                &format!("{cwd}/forbidden"),
            ])
            .status()
            .unwrap();
        assert!(absent.success(), "discovery never executes project scripts");
        std::process::Command::new("wsl.exe")
            .args([
                "--distribution",
                &distro,
                "--exec",
                "python3",
                "-I",
                "-S",
                "-c",
                "import shutil,sys; shutil.rmtree(sys.argv[1])",
                &cwd,
            ])
            .status()
            .unwrap();
    }
}
