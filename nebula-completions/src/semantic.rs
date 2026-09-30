//! Command/argument semantics. This module describes sources, never performs I/O.

use crate::command_context::{CommandContext, ShellSyntax};
use crate::{CandidateMatcher, CompletionOptions, CompletionSort, MatchAlgorithm, Suggestion};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Words(&'static [&'static str]),
    Branches {
        include_busy: bool,
    },
    Revisions {
        include_busy: bool,
    },
    RevisionsAndPaths {
        include_busy: bool,
    },
    Paths {
        directories_only: bool,
    },
    ProjectScripts,
    Options,
    /// A known free-form value must not receive unrelated history/path candidates.
    None,
}

#[derive(Debug)]
pub struct Context {
    input: CommandContext,
    pub source: Source,
    pub directories: Vec<String>,
    options: &'static [OptionSpec],
    attached: Option<usize>,
    branch_guess: Option<bool>,
}

impl Context {
    pub fn parse(line: &str, cursor: usize, syntax: ShellSyntax) -> Option<Self> {
        let input = CommandContext::parse(line, cursor, syntax)?;
        let mut context = Self {
            input,
            source: Source::None,
            directories: Vec::new(),
            options: &[],
            attached: None,
            branch_guess: None,
        };
        context.source = match context.input.arguments.first()?.as_str() {
            "git" | "git.exe" => context.git()?,
            "npm" | "npm.cmd" | "pnpm" | "pnpm.cmd" | "yarn" | "yarn.cmd" => context.scripts()?,
            _ => return None,
        };
        Some(context)
    }

    pub fn input(&self) -> &CommandContext {
        &self.input
    }

    pub fn value_prefix(&self) -> &str {
        &self.input.prefix()[self.attached.unwrap_or(0)..]
    }

    pub fn guesses_branches(&self, configured: bool) -> bool {
        matches!(self.source, Source::Branches { .. } | Source::RevisionsAndPaths { .. })
            && self.branch_guess.unwrap_or(configured)
    }

    /// Prefix matches stay first; fuzzy candidates reuse the existing scorer.
    pub fn candidates<'a>(&self, values: impl IntoIterator<Item = &'a str>) -> Vec<Suggestion> {
        let options = CompletionOptions {
            match_algorithm: MatchAlgorithm::Fuzzy,
            sort: CompletionSort::Smart,
            ..Default::default()
        };
        let mut matcher = CandidateMatcher::literal(self.value_prefix(), &options, true);
        for value in values {
            let full_value =
                self.attached.map(|end| format!("{}{value}", &self.input.prefix()[..end]));
            if let Some(candidate) = self.input.candidate(full_value.as_deref().unwrap_or(value)) {
                matcher.add(value, candidate);
            }
        }
        let mut candidates: Vec<_> = matcher.results().into_iter().map(|(s, _)| s).collect();
        candidates.sort_by_key(|s| !s.display_value().starts_with(self.input.prefix()));
        candidates.truncate(256);
        candidates
    }

    pub fn static_candidates(&self) -> Vec<Suggestion> {
        match self.source {
            Source::Words(values) => self.candidates(values.iter().copied()),
            Source::Options => {
                self.candidates(self.options.iter().flat_map(|option| option.names.iter().copied()))
            },
            _ => Vec::new(),
        }
    }

    fn git(&mut self) -> Option<Source> {
        let args = &self.input.arguments;
        let mut index = 1;
        while args.get(index).is_some_and(|arg| arg == "-C") {
            let Some(directory) = args.get(index + 1) else {
                return Some(Source::Paths { directories_only: true });
            };
            self.directories.push(directory.clone());
            index += 2;
        }
        let Some(command) = args.get(index) else {
            return Some(Source::Words(if self.input.prefix().starts_with('-') {
                &["-C", "--version", "--help"]
            } else {
                &[
                    "add",
                    "bisect",
                    "branch",
                    "checkout",
                    "cherry-pick",
                    "clone",
                    "commit",
                    "diff",
                    "fetch",
                    "init",
                    "log",
                    "merge",
                    "pull",
                    "push",
                    "rebase",
                    "remote",
                    "reset",
                    "restore",
                    "revert",
                    "show",
                    "stash",
                    "status",
                    "switch",
                    "tag",
                    "worktree",
                ]
            }));
        };
        let options = match command.as_str() {
            "switch" => SWITCH_OPTIONS,
            "checkout" => CHECKOUT_OPTIONS,
            "merge" => MERGE_OPTIONS,
            "rebase" => REBASE_OPTIONS,
            // Unknown commands retain the existing path/history behavior.
            _ => return None,
        };
        self.options = options;
        let mut positional = 0;
        let mut parse_options = true;
        let mut include_busy = matches!(command.as_str(), "merge" | "rebase");
        let mut revisions = command != "switch";
        let mut reference_only = false;
        let mut terminal = false;
        let mut root = false;
        let mut no_track = false;
        index += 1;
        while let Some(arg) = args.get(index) {
            if parse_options && arg == "--" {
                if command == "checkout" {
                    return Some(Source::Paths { directories_only: false });
                }
                parse_options = false;
            } else if parse_options && arg.starts_with('-') {
                let (name, attached) =
                    arg.split_once('=').map_or((arg.as_str(), None), |(a, b)| (a, Some(b)));
                let option = options.iter().find(|option| option.names.contains(&name))?;
                include_busy |= option.include_busy;
                // 普通 switch 只接受分支；新分支起点和 detach 则接受任意 commit-ish。
                revisions |=
                    matches!(name, "-c" | "-C" | "--create" | "--force-create" | "-d" | "--detach");
                reference_only |= matches!(name, "-b" | "-B" | "-d" | "--detach");
                terminal |= option.terminal;
                root |= name == "--root";
                match name {
                    "--guess" => self.branch_guess = Some(true),
                    "--no-guess" => self.branch_guess = Some(false),
                    "--no-track" => no_track = true,
                    _ => {},
                }
                if let Some(value_source) = option.value {
                    let provided = if let Some(value) = attached {
                        value
                    } else {
                        index += 1;
                        let Some(value) = args.get(index) else {
                            return Some(value_source);
                        };
                        value.as_str()
                    };
                    if let Source::Words(values) = value_source {
                        if !values.contains(&provided) {
                            return Some(Source::None);
                        }
                    }
                } else if attached.is_some() {
                    return None;
                }
            } else {
                positional += 1;
            }
            index += 1;
        }
        if terminal {
            return Some(Source::None);
        }
        // Git 的自动建分支要求 tracking 未显式指定，--guess 不能覆盖 --no-track。
        if no_track {
            self.branch_guess = Some(false);
        }
        if parse_options && self.input.prefix().starts_with('-') {
            if let Some((name, _)) = self.input.prefix().split_once('=') {
                let option = options.iter().find(|option| option.names.contains(&name))?;
                self.attached = Some(name.len() + 1);
                return option.value;
            }
            return Some(Source::Options);
        }
        // checkout 的首个无标记参数可指向分支或路径，后续参数只接受路径。
        if command == "checkout" && !reference_only {
            return Some(if positional == 0 {
                Source::RevisionsAndPaths { include_busy }
            } else {
                Source::Paths { directories_only: false }
            });
        }
        let limit = match command.as_str() {
            "merge" => usize::MAX,
            "rebase" if root => 1,
            "rebase" => 2,
            _ => 1,
        };
        if positional >= limit {
            return Some(if command == "checkout" {
                Source::Paths { directories_only: false }
            } else {
                Source::None
            });
        }
        Some(if revisions {
            Source::Revisions { include_busy }
        } else {
            Source::Branches { include_busy }
        })
    }

    fn scripts(&mut self) -> Option<Source> {
        let args = &self.input.arguments;
        let program = args[0].trim_end_matches(".cmd");
        let directory_flag = match program {
            "npm" => "--prefix",
            "pnpm" => "--dir",
            _ => "--cwd",
        };
        let mut index = 1;
        while let Some(arg) = args.get(index) {
            if arg == directory_flag || program == "pnpm" && arg == "-C" {
                let Some(directory) = args.get(index + 1) else {
                    // 包管理器目录选项相对于命令 cwd；不像 Git -C 那样逐级进入。
                    self.directories.clear();
                    return Some(Source::Paths { directories_only: true });
                };
                self.directories.push(directory.clone());
                index += 2;
            } else if let Some(value) = arg.strip_prefix(&format!("{directory_flag}=")) {
                self.directories.push(value.to_owned());
                index += 1;
            } else {
                break;
            }
        }
        if !matches!(args.get(index).map(String::as_str), Some("run" | "run-script")) {
            return None;
        }
        if program != "npm" && args[index] == "run-script" {
            return None;
        }
        index += 1;
        while program != "yarn"
            && args
                .get(index)
                .is_some_and(|arg| matches!(arg.as_str(), "--silent" | "--if-present"))
        {
            index += 1;
        }
        if index == args.len() && !self.input.prefix().starts_with('-') {
            Some(Source::ProjectScripts)
        } else {
            None // script arguments/options are not script-name positions.
        }
    }
}

#[derive(Debug)]
struct OptionSpec {
    names: &'static [&'static str],
    value: Option<Source>,
    include_busy: bool,
    terminal: bool,
}

const fn flag(names: &'static [&'static str]) -> OptionSpec {
    OptionSpec { names, value: None, include_busy: false, terminal: false }
}
const fn value(names: &'static [&'static str], source: Source) -> OptionSpec {
    OptionSpec { value: Some(source), ..flag(names) }
}
const CONFLICT: Source = Source::Words(&["merge", "diff3", "zdiff3"]);
const REVISION: Source = Source::Revisions { include_busy: true };
const SWITCH_OPTIONS: &[OptionSpec] = &[
    OptionSpec {
        include_busy: true,
        ..value(&["-c", "-C", "--create", "--force-create"], Source::None)
    },
    OptionSpec { terminal: true, ..value(&["--orphan"], Source::None) },
    OptionSpec { include_busy: true, ..flag(&["-d", "--detach", "--ignore-other-worktrees"]) },
    value(&["--conflict"], CONFLICT),
    flag(&[
        "-q",
        "--quiet",
        "-m",
        "--merge",
        "-f",
        "--force",
        "--discard-changes",
        "--guess",
        "--no-guess",
        "--no-track",
        "--progress",
        "--no-progress",
        "--overwrite-ignore",
        "--no-overwrite-ignore",
    ]),
];
const CHECKOUT_OPTIONS: &[OptionSpec] = &[
    OptionSpec { include_busy: true, ..value(&["-b", "-B"], Source::None) },
    OptionSpec { terminal: true, ..value(&["--orphan"], Source::None) },
    OptionSpec { include_busy: true, ..flag(&["-d", "--detach", "--ignore-other-worktrees"]) },
    value(&["--conflict"], CONFLICT),
    flag(&[
        "-q",
        "--quiet",
        "-m",
        "--merge",
        "-f",
        "--force",
        "--guess",
        "--no-guess",
        "--no-track",
        "--progress",
        "--no-progress",
    ]),
];
const MERGE_OPTIONS: &[OptionSpec] = &[
    flag(&[
        "--ff",
        "--no-ff",
        "--ff-only",
        "--squash",
        "--no-squash",
        "--commit",
        "--no-commit",
        "--edit",
        "--no-edit",
        "--stat",
        "--no-stat",
        "--autostash",
        "--no-autostash",
        "--allow-unrelated-histories",
        "-q",
        "--quiet",
        "-v",
        "--verbose",
    ]),
    value(&["-m", "--message"], Source::None),
    value(
        &["-s", "--strategy"],
        Source::Words(&["ort", "recursive", "resolve", "octopus", "ours", "subtree"]),
    ),
    OptionSpec { terminal: true, ..flag(&["--abort", "--continue", "--quit"]) },
];
const REBASE_OPTIONS: &[OptionSpec] = &[
    value(&["--onto"], REVISION),
    value(&["--empty"], Source::Words(&["drop", "keep", "stop"])),
    value(&["-x", "--exec", "-C"], Source::None),
    flag(&[
        "-i",
        "--interactive",
        "--autostash",
        "--no-autostash",
        "--autosquash",
        "--no-autosquash",
        "--keep-base",
        "--root",
        "--update-refs",
        "--no-update-refs",
        "--reapply-cherry-picks",
        "-q",
        "--quiet",
        "-v",
        "--verbose",
    ]),
    OptionSpec {
        terminal: true,
        ..flag(&[
            "--abort",
            "--continue",
            "--skip",
            "--quit",
            "--edit-todo",
            "--show-current-patch",
        ])
    },
];
#[cfg(test)]
mod tests;
