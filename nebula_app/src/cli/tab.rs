//! Tab command arguments shared by the short and control CLI.

use super::*;

#[derive(Args, Debug)]
pub struct TabLaunchOptions {
    /// Target window; omitted uses the window policy (most recently active by default).
    #[clap(long)]
    pub window: Option<u64>,

    /// Working directory for the new terminal (relative to this CLI process).
    #[clap(long, alias = "working-directory", value_hint = ValueHint::DirPath)]
    pub cwd: Option<PathBuf>,

    /// Shell id from the settings, for example bash or wsl:Ubuntu.
    #[clap(long, conflicts_with = "command")]
    pub shell: Option<String>,

    /// Program and arguments to execute directly; must be the last option.
    #[clap(short = 'e', long, allow_hyphen_values = true, num_args = 1..)]
    pub command: Vec<String>,
}

#[derive(Args, Debug)]
pub struct TabNewOptions {
    #[clap(flatten)]
    pub launch: TabLaunchOptions,

    #[clap(flatten)]
    pub output: ShortOutput,

    #[clap(long, default_value_t = SHORT_TIMEOUT_MS)]
    pub timeout_ms: u64,
}

#[derive(Args, Debug)]
pub struct TabResourceOptions {
    #[clap(subcommand)]
    pub command: TabCommand,
}

/// Tab indices are scoped to one window; creation may select the most recent window.
#[derive(Subcommand, Debug)]
pub enum TabCommand {
    /// Open a terminal tab, optionally running a program instead of the default shell.
    New(TabNewOptions),
    /// Close an idle tab.
    Close(TabCloseOptions),
    /// Set a tab's custom name. Pass an empty name to restore the generated title.
    Rename(TabRenameOptions),
    /// Move a tab to another index in the same window.
    Move(TabMoveOptions),
}

#[derive(Args, Debug)]
pub struct TabCloseOptions {
    /// Zero-based tab index from the runtime snapshot.
    pub tab: usize,

    /// Window containing the tab.
    #[clap(long)]
    pub window: u64,

    #[clap(flatten)]
    pub output: ShortOutput,

    #[clap(long, default_value_t = SHORT_TIMEOUT_MS)]
    pub timeout_ms: u64,
}

#[derive(Args, Debug)]
pub struct TabRenameOptions {
    /// Zero-based tab index from the runtime snapshot.
    pub tab: usize,

    /// New custom name. An empty string restores the generated title.
    pub name: String,

    /// Window containing the tab.
    #[clap(long)]
    pub window: u64,

    #[clap(flatten)]
    pub output: ShortOutput,

    #[clap(long, default_value_t = SHORT_TIMEOUT_MS)]
    pub timeout_ms: u64,
}

#[derive(Args, Debug)]
pub struct TabMoveOptions {
    /// Current zero-based tab index.
    pub tab: usize,

    /// Destination zero-based tab index in the same window.
    pub to: usize,

    /// Window containing the tab.
    #[clap(long)]
    pub window: u64,

    #[clap(flatten)]
    pub output: ShortOutput,

    #[clap(long, default_value_t = SHORT_TIMEOUT_MS)]
    pub timeout_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tab_parses_a_direct_command_without_consuming_its_arguments() {
        let options = Options::try_parse_from([
            "pebrel",
            "tab",
            "new",
            "--window",
            "7",
            "--cwd",
            "project space",
            "--pretty",
            "--command",
            "some program",
            "--window",
            "8",
            "",
            "$(literal)",
        ])
        .unwrap();
        let Some(Subcommands::Tab(TabResourceOptions { command: TabCommand::New(options) })) =
            options.subcommands
        else {
            panic!("expected tab new");
        };
        assert_eq!(options.launch.window, Some(7));
        assert_eq!(options.launch.cwd, Some(PathBuf::from("project space")));
        assert!(options.output.pretty);
        assert_eq!(options.launch.command, ["some program", "--window", "8", "", "$(literal)"]);
    }

    #[test]
    fn new_tab_and_ctl_accept_default_shells_and_reject_conflicting_launches() {
        for prefix in [vec!["pebrel", "tab", "new"], vec!["pebrel", "ctl", "new-tab"]] {
            assert!(Options::try_parse_from(&prefix).is_ok());
            let mut args = prefix.clone();
            args.extend(["--shell", "bash", "--cwd", "."]);
            assert!(Options::try_parse_from(&args).is_ok());
            args.extend(["--command", "sh"]);
            assert!(Options::try_parse_from(&args).is_err());
            let mut args = prefix;
            args.push("--command");
            assert!(Options::try_parse_from(&args).is_err());
        }
        let options = Options::try_parse_from([
            "pebrel",
            "ctl",
            "new-tab",
            "--working-directory",
            ".",
            "-e",
            "sh",
            "-c",
            "printf ok",
        ])
        .unwrap();
        let Some(Subcommands::Ctl(ControlOptions {
            command: ControlCommand::NewTab(launch), ..
        })) = options.subcommands
        else {
            panic!("expected ctl new-tab");
        };
        assert_eq!(launch.command, ["sh", "-c", "printf ok"]);
    }
}
