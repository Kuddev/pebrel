//! 常用命令的参数角色；数据发现留给应用层，shell 方言只决定已知别名的语义。

use super::*;

const FILES: Source = Source::Paths { directories_only: false };
const DIRS: Source = Source::Paths { directories_only: true };
const SSH: &[OptionSpec] = &[
    flag(&[
        "-4", "-6", "-A", "-a", "-C", "-f", "-G", "-g", "-K", "-k", "-M", "-N", "-n", "-q", "-s",
        "-T", "-t", "-V", "-v", "-X", "-x", "-Y", "-y",
    ]),
    value(&["-F", "-i", "-E", "-S"], FILES),
    value(&["-J"], Source::SshHosts { jump: true }),
    value(
        &["-B", "-b", "-c", "-D", "-I", "-L", "-l", "-m", "-O", "-o", "-p", "-Q", "-R", "-W", "-w"],
        Source::None,
    ),
];
const WSL: &[OptionSpec] = &[
    flag(&[
        "--list",
        "-l",
        "--verbose",
        "-v",
        "--quiet",
        "-q",
        "--all",
        "--running",
        "--online",
        "-o",
        "--status",
        "--version",
        "--help",
        "--shutdown",
        "--update",
        "--vhd",
    ]),
    value(
        &[
            "--distribution",
            "-d",
            "--set-default",
            "-s",
            "--terminate",
            "-t",
            "--unregister",
            "--export",
            "--set-version",
        ],
        Source::WslDistributions,
    ),
    value(&["--user", "-u", "--cd", "--install", "--import"], Source::None),
    value(&["--shell-type"], Source::Words(&["standard", "login", "none"])),
    value(&["--set-default-version"], Source::Words(&["1", "2"])),
    flag(&["--exec", "-e", "--"]),
];
const POSIX_LS: &[OptionSpec] =
    &[flag(&["-a", "-A", "-l", "-h", "-R", "-d", "-F", "-t", "-r", "-S", "-1"])];
const POSIX_CAT: &[OptionSpec] = &[flag(&["-b", "-e", "-n", "-s", "-t", "-u", "-v"])];
const POSIX_COPY: &[OptionSpec] = &[flag(&["-R", "-r", "-f", "-i", "-p", "-v"])];
const POSIX_MOVE: &[OptionSpec] = &[flag(&["-f", "-i", "-n", "-v"])];
const POSIX_REMOVE: &[OptionSpec] = &[flag(&["-f", "-i", "-r", "-R", "-v"])];
const POSIX_MKDIR: &[OptionSpec] = &[flag(&["-p", "-v"]), value(&["-m"], Source::None)];
const POSIX_GREP: &[OptionSpec] = &[
    flag(&["-i", "-n", "-r", "-R", "-v", "-l", "-c", "-E", "-F", "-w", "-x", "-q"]),
    value(&["-e"], Source::None),
    value(&["-f"], FILES),
];
const PS_LIST: &[OptionSpec] = &[
    value(&["-Path", "-LiteralPath"], FILES),
    value(&["-Filter", "-Include", "-Exclude", "-Depth"], Source::None),
    flag(&["-Directory", "-File", "-Force", "-Recurse", "-Name", "-Hidden"]),
];
const PS_CONTENT: &[OptionSpec] = &[
    value(&["-Path", "-LiteralPath"], FILES),
    value(&["-TotalCount", "-Tail", "-ReadCount", "-Encoding", "-Delimiter"], Source::None),
    flag(&["-Raw", "-Wait", "-Force"]),
];
const PS_COPY: &[OptionSpec] = &[
    value(&["-Path", "-LiteralPath", "-Destination"], FILES),
    flag(&["-Recurse", "-Force", "-PassThru", "-WhatIf", "-Confirm"]),
];
const PS_MOVE: &[OptionSpec] = &[
    value(&["-Path", "-LiteralPath", "-Destination"], FILES),
    flag(&["-Force", "-PassThru", "-WhatIf", "-Confirm"]),
];
const PS_REMOVE: &[OptionSpec] = &[
    value(&["-Path", "-LiteralPath"], FILES),
    flag(&["-Force", "-Recurse", "-WhatIf", "-Confirm"]),
];
const PS_CD: &[OptionSpec] = &[value(&["-Path", "-LiteralPath"], DIRS), flag(&["-PassThru"])];
const CMD_CD: &[OptionSpec] = &[flag(&["/d"])];
const CMD_DIR: &[OptionSpec] =
    &[flag(&["/a", "/b", "/s", "/p", "/w", "/d", "/n", "/o", "/q", "/r", "/x"])];
const CMD_COPY: &[OptionSpec] = &[flag(&["/y", "/-y", "/v", "/b", "/a", "/z"])];
const CMD_MOVE: &[OptionSpec] = &[flag(&["/y", "/-y"])];
const CMD_DEL: &[OptionSpec] = &[flag(&["/p", "/f", "/s", "/q", "/a"])];

impl Context {
    pub(super) fn common(&mut self, syntax: ShellSyntax) -> Option<Source> {
        let name = &self.input.arguments[0];
        let normalized = if matches!(syntax, ShellSyntax::PowerShell | ShellSyntax::Cmd) {
            name.to_ascii_lowercase()
        } else {
            name.clone()
        };
        let program = normalized.strip_suffix(".exe").unwrap_or(&normalized);
        let ssh = program == "ssh";
        let wsl = program == "wsl";
        let powershell = syntax == ShellSyntax::PowerShell;
        let cmd = syntax == ShellSyntax::Cmd && !ssh && !wsl;
        let program_lower = program.to_ascii_lowercase();
        let (options, mut source, limit) = if ssh {
            (SSH, Source::SshHosts { jump: false }, 1)
        } else if wsl {
            (WSL, Source::None, usize::MAX)
        } else if cmd {
            match program {
                "cd" | "chdir" => (CMD_CD, DIRS, 1),
                "dir" => (CMD_DIR, FILES, usize::MAX),
                "copy" => (CMD_COPY, FILES, usize::MAX),
                "move" => (CMD_MOVE, FILES, usize::MAX),
                "del" | "erase" => (CMD_DEL, FILES, usize::MAX),
                "type" => (&[][..], FILES, usize::MAX),
                "mkdir" | "md" => (&[][..], DIRS, usize::MAX),
                _ => return None,
            }
        } else if powershell {
            match program_lower.as_str() {
                "cd" | "chdir" | "set-location" => (PS_CD, DIRS, 1),
                "dir" | "gci" | "get-childitem" => (PS_LIST, FILES, usize::MAX),
                "gc" | "type" | "get-content" => (PS_CONTENT, FILES, usize::MAX),
                "copy" | "cpi" | "copy-item" => (PS_COPY, FILES, usize::MAX),
                "move" | "mi" | "move-item" => (PS_MOVE, FILES, usize::MAX),
                "del" | "erase" | "ri" | "remove-item" => (PS_REMOVE, FILES, usize::MAX),
                // Unix PowerShell 保留这些名字给本机程序，不能给 /bin/ls 补 cmdlet 参数。
                "ls" if cfg!(windows) => (PS_LIST, FILES, usize::MAX),
                "cat" if cfg!(windows) => (PS_CONTENT, FILES, usize::MAX),
                "cp" if cfg!(windows) => (PS_COPY, FILES, usize::MAX),
                "mv" if cfg!(windows) => (PS_MOVE, FILES, usize::MAX),
                "rm" if cfg!(windows) => (PS_REMOVE, FILES, usize::MAX),
                "ls" => (POSIX_LS, FILES, usize::MAX),
                "cat" => (POSIX_CAT, FILES, usize::MAX),
                "cp" => (POSIX_COPY, FILES, usize::MAX),
                "mv" => (POSIX_MOVE, FILES, usize::MAX),
                "rm" => (POSIX_REMOVE, FILES, usize::MAX),
                _ => return None,
            }
        } else {
            match program {
                "cd" => (&[][..], DIRS, 1),
                "ls" if matches!(syntax, ShellSyntax::Posix | ShellSyntax::Literal) => {
                    (POSIX_LS, FILES, usize::MAX)
                },
                "cat" if matches!(syntax, ShellSyntax::Posix | ShellSyntax::Literal) => {
                    (POSIX_CAT, FILES, usize::MAX)
                },
                "cp" if matches!(syntax, ShellSyntax::Posix | ShellSyntax::Literal) => {
                    (POSIX_COPY, FILES, usize::MAX)
                },
                "mv" if matches!(syntax, ShellSyntax::Posix | ShellSyntax::Literal) => {
                    (POSIX_MOVE, FILES, usize::MAX)
                },
                "rm" if matches!(syntax, ShellSyntax::Posix | ShellSyntax::Literal) => {
                    (POSIX_REMOVE, FILES, usize::MAX)
                },
                "mkdir" if matches!(syntax, ShellSyntax::Posix | ShellSyntax::Literal) => {
                    (POSIX_MKDIR, DIRS, usize::MAX)
                },
                "grep" if matches!(syntax, ShellSyntax::Posix | ShellSyntax::Literal) => {
                    (POSIX_GREP, Source::None, usize::MAX)
                },
                _ => return None,
            }
        };
        self.options = options;
        let mut index = 1;
        let mut positional = 0;
        let mut parse_options = true;
        let mut grep_pattern = false;
        let mut installing = false;
        while let Some(arg) = self.input.arguments.get(index) {
            if parse_options && arg == "--" {
                if wsl {
                    return Some(Source::None);
                }
                parse_options = false;
            } else if parse_options && arg.starts_with(if cmd { '/' } else { '-' }) {
                if wsl && matches!(arg.as_str(), "--exec" | "-e") {
                    return Some(Source::None);
                }
                if let Some((option, name, attached)) =
                    resolve(options, arg, cmd || powershell && !ssh && !wsl, !wsl)
                {
                    if wsl && name == "--install" {
                        installing = true;
                    }
                    let import_version = wsl
                        && name == "--version"
                        && self.input.arguments.iter().any(|a| a == "--import");
                    if import_version {
                        index += 1;
                        if self.input.arguments.get(index).is_none() {
                            return Some(Source::Words(&["1", "2"]));
                        }
                    }
                    if let Some(value_source) = option.value {
                        let value = if let Some(offset) = attached {
                            &arg[offset..]
                        } else {
                            index += 1;
                            let Some(value) = self.input.arguments.get(index) else {
                                return Some(if installing { Source::None } else { value_source });
                            };
                            value.as_str()
                        };
                        if ssh && name == "-F" {
                            self.ssh_config = Some(value.to_owned());
                            self.ssh_config_expands_home =
                                attached.is_none() && self.input.argument_expands_home(index);
                        }
                        if program == "grep" && matches!(name, "-e" | "-f") {
                            grep_pattern = true;
                        }
                        if wsl {
                            source = match name {
                                "--export" => FILES,
                                "--set-version" => Source::Words(&["1", "2"]),
                                "--import" => DIRS,
                                _ => source,
                            };
                        }
                    } else if attached.is_some() {
                        return Some(Source::None);
                    }
                } else if !wsl
                    && !cmd
                    && !arg.starts_with("--")
                    && arg[1..].chars().all(|ch| {
                        options.iter().any(|option| {
                            option.value.is_none()
                                && option
                                    .names
                                    .iter()
                                    .any(|name| name.len() == 2 && name.ends_with(ch))
                        })
                    })
                {
                    // 常见 -al/-vvv 只在每个成员均为无值选项时成立。
                } else {
                    return Some(Source::None);
                }
            } else {
                positional += 1;
                if ssh || positional >= limit || wsl && source == Source::None {
                    return Some(Source::None);
                }
                if program == "grep" {
                    grep_pattern = true;
                }
                if wsl {
                    source = if matches!(source, Source::Paths { directories_only: true }) {
                        FILES
                    } else {
                        Source::None
                    };
                }
            }
            index += 1;
        }
        if parse_options && self.input.prefix().starts_with(if cmd { '/' } else { '-' }) {
            if let Some((option, _, Some(offset))) =
                resolve(options, self.input.prefix(), cmd || powershell && !ssh && !wsl, !wsl)
            {
                self.attached = Some(offset);
                return option.value;
            }
            return Some(Source::Options);
        }
        if program == "grep" && grep_pattern {
            source = FILES;
        }
        Some(source)
    }
}

fn resolve<'a>(
    options: &'a [OptionSpec],
    arg: &str,
    case_insensitive: bool,
    short_values: bool,
) -> Option<(&'a OptionSpec, &'static str, Option<usize>)> {
    for option in options {
        for name in option.names {
            if arg == *name || case_insensitive && arg.eq_ignore_ascii_case(name) {
                return Some((option, name, None));
            }
        }
    }
    // OpenSSH/getopt 的短选项值不剥离 '='；-F=file 指的是名为 =file 的文件。
    if short_values && !case_insensitive && arg.starts_with('-') && !arg.starts_with("--") {
        for (offset, ch) in arg.char_indices().skip(1) {
            let option = options.iter().find(|option| {
                option.names.iter().any(|name| name.len() == 2 && name.ends_with(ch))
            })?;
            let name = option.names.iter().find(|name| name.len() == 2 && name.ends_with(ch))?;
            if option.value.is_some() {
                let end = offset + ch.len_utf8();
                return Some((option, name, (end < arg.len()).then_some(end)));
            }
        }
    }
    None
}
