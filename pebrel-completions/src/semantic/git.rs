//! Git argument roles, independent of repository I/O.

use super::*;

impl Context {
    pub(super) fn git(&mut self) -> Option<Source> {
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
        if matches!(command.as_str(), "push" | "pull" | "fetch") {
            let command = command.clone();
            return self.git_network(&command, index + 1);
        }
        let options = match command.as_str() {
            "switch" => SWITCH_OPTIONS,
            "checkout" => CHECKOUT_OPTIONS,
            "merge" => MERGE_OPTIONS,
            "rebase" => REBASE_OPTIONS,
            // Unknown commands retain the existing path/history behavior.
            _ => return None,
        };
        let before_cursor = args.len();
        let args: Vec<_> = args.iter().chain(self.input.following_arguments()).cloned().collect();
        self.options = options;
        let mut positional = 0;
        let mut parse_options = true;
        let mut include_busy = matches!(command.as_str(), "merge" | "rebase");
        let mut revisions = command != "switch";
        let mut reference_only = false;
        let mut terminal = false;
        let mut root = false;
        let mut tracking = None;
        let mut creates_branch = false;
        let mut detach = false;
        index += 1;
        while let Some(arg) = args.get(index) {
            if parse_options && arg == "--" {
                if command == "checkout" {
                    return Some(if tracking.is_some() {
                        Source::None
                    } else if index >= before_cursor {
                        Source::Revisions { include_busy }
                    } else {
                        Source::Paths { directories_only: false }
                    });
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
                creates_branch |=
                    matches!(name, "-b" | "-B" | "-c" | "-C" | "--create" | "--force-create");
                detach |= matches!(name, "-d" | "--detach");
                terminal |= option.terminal;
                root |= name == "--root";
                match name {
                    "--guess" => self.branch_guess = Some(true),
                    "--no-guess" => self.branch_guess = Some(false),
                    "-t" | "--track" => {
                        tracking = Some(match attached {
                            None | Some("direct") => TrackingMode::Direct,
                            Some("inherit") => TrackingMode::Inherit,
                            _ => return Some(Source::None),
                        })
                    },
                    "--no-track" => tracking = Some(TrackingMode::Disabled),
                    _ => {},
                }
                if let Some(value_source) = option.value {
                    // --track 的可选值只能附在选项上；后面的单词仍是起点引用。
                    if option.optional_value && attached.is_none() {
                        index += 1;
                        continue;
                    }
                    let provided = if let Some(value) = attached {
                        value
                    } else {
                        index += 1;
                        let Some(value) = args.get(index) else {
                            return Some(if index <= before_cursor {
                                value_source
                            } else {
                                Source::None
                            });
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
            } else if index < before_cursor {
                positional += 1;
            }
            index += 1;
        }
        if terminal {
            return Some(Source::None);
        }
        // Git 的自动建分支要求 tracking 未显式指定，--guess 不能覆盖 --no-track。
        if tracking.is_some() {
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
        if let Some(mode) = tracking {
            return Some(if detach || positional != 0 {
                Source::None
            } else {
                Source::Tracking { mode, infer_name: !creates_branch }
            });
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

    fn git_network(&mut self, command: &str, mut index: usize) -> Option<Source> {
        let options = match command {
            "push" => PUSH_OPTIONS,
            "pull" => PULL_OPTIONS,
            _ => FETCH_OPTIONS,
        };
        self.options = options;
        let before_cursor = self.input.arguments.len();
        let args: Vec<_> =
            self.input.arguments.iter().chain(self.input.following_arguments()).cloned().collect();
        let mut remote = None;
        let mut parse_options = true;
        let mut multiple = false;
        let mut delete = false;
        let mut terminal = false;
        while let Some(arg) = args.get(index) {
            if parse_options && arg == "--" {
                parse_options = false;
            } else if parse_options && arg.starts_with('-') {
                let (name, attached) =
                    arg.split_once('=').map_or((arg.as_str(), None), |(n, v)| (n, Some(v)));
                let option = options.iter().find(|o| o.names.contains(&name))?;
                delete |= name == "--delete" || command == "push" && name == "-d";
                multiple |= name == "--multiple";
                terminal |= option.terminal;
                if let Some(source) = option.value {
                    if option.optional_value && attached.is_none() {
                        index += 1;
                        continue;
                    }
                    let provided = if let Some(value) = attached {
                        value
                    } else {
                        index += 1;
                        let Some(value) = args.get(index) else {
                            return Some(if index <= before_cursor {
                                source
                            } else {
                                Source::None
                            });
                        };
                        value
                    };
                    if name == "--repo" {
                        remote = Some(provided.to_owned());
                    }
                    if let Source::Words(values) = source
                        && !values.contains(&provided)
                    {
                        return Some(Source::None);
                    }
                } else if attached.is_some() {
                    return None;
                }
            } else if index < before_cursor && remote.is_none() {
                remote = Some(arg.clone());
            }
            index += 1;
        }
        if parse_options && self.input.prefix().starts_with('-') {
            if let Some((name, _)) = self.input.prefix().split_once('=') {
                let option = options.iter().find(|o| o.names.contains(&name))?;
                self.attached = Some(name.len() + 1);
                return option.value;
            }
            return Some(Source::Options);
        }
        if terminal {
            return Some(Source::None);
        }
        if remote.is_none() || command == "fetch" && multiple {
            return Some(Source::Remotes);
        }
        self.remote = remote;
        if self.input.prefix().is_empty() && self.input.full_word().starts_with(['+', ':']) {
            return Some(Source::None);
        }
        if !delete
            && !self.input.prefix().contains(':')
            && let Some((_, destination)) = self.input.full_word().split_once(':')
        {
            self.preserved_tail = format!(":{destination}");
        }
        if command == "push" && !delete {
            let destination = if let Some((source, _)) = self.input.prefix().split_once(':') {
                self.attached = Some(source.len() + 1);
                true
            } else {
                if self.input.prefix().starts_with('+') {
                    self.attached = Some(1);
                }
                false
            };
            Some(Source::PushRefs { destination })
        } else if self.input.prefix().contains(':')
            || delete && self.input.full_word().contains(':')
        {
            Some(Source::None)
        } else {
            if self.input.prefix().starts_with('+') {
                self.attached = Some(1);
            }
            Some(Source::RemoteBranches)
        }
    }
}

const CONFLICT: Source = Source::Words(&["merge", "diff3", "zdiff3"]);
const REVISION: Source = Source::Revisions { include_busy: true };
const TRACK: OptionSpec = OptionSpec {
    optional_value: true,
    ..value(&["--track"], Source::Words(&["direct", "inherit"]))
};
const SWITCH_OPTIONS: &[OptionSpec] = &[
    TRACK,
    flag(&["-t"]),
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
    TRACK,
    flag(&["-t"]),
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

const PUSH_OPTIONS: &[OptionSpec] = &[
    flag(&[
        "-u",
        "--set-upstream",
        "-f",
        "--force",
        "-d",
        "--delete",
        "--dry-run",
        "-n",
        "--atomic",
        "--porcelain",
        "--follow-tags",
        "--no-verify",
        "-q",
        "--quiet",
        "-v",
        "--verbose",
    ]),
    OptionSpec { terminal: true, ..flag(&["--all", "--mirror", "--tags"]) },
    OptionSpec { optional_value: true, ..value(&["--force-with-lease"], Source::None) },
    value(&["--repo"], Source::Remotes),
    value(&["--receive-pack", "--exec", "--push-option", "-o"], Source::None),
];
const FETCH_OPTIONS: &[OptionSpec] = &[
    flag(&[
        "--multiple",
        "-p",
        "--prune",
        "--prune-tags",
        "-t",
        "--tags",
        "--no-tags",
        "-f",
        "--force",
        "--dry-run",
        "--write-fetch-head",
        "--no-write-fetch-head",
        "--recurse-submodules",
        "-q",
        "--quiet",
        "-v",
        "--verbose",
    ]),
    OptionSpec { terminal: true, ..flag(&["--all"]) },
    value(
        &[
            "--depth",
            "--deepen",
            "--shallow-since",
            "--shallow-exclude",
            "--upload-pack",
            "--refmap",
            "-j",
            "--jobs",
        ],
        Source::None,
    ),
];
const PULL_OPTIONS: &[OptionSpec] = &[
    flag(&[
        "--ff",
        "--ff-only",
        "--no-ff",
        "--no-rebase",
        "--no-commit",
        "--commit",
        "--no-edit",
        "--edit",
        "--squash",
        "--autostash",
        "--no-autostash",
        "--allow-unrelated-histories",
        "-q",
        "--quiet",
        "-v",
        "--verbose",
        "--tags",
        "--no-tags",
        "--prune",
    ]),
    OptionSpec {
        optional_value: true,
        ..value(&["-r", "--rebase"], Source::Words(&["false", "true", "merges", "interactive"]))
    },
    value(
        &["--depth", "--deepen", "--upload-pack", "--strategy", "-s", "--strategy-option", "-X"],
        Source::None,
    ),
];
