//! Package-manager positions and workspace selection without running a manager.

use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PackageManager {
    #[default]
    Npm,
    Pnpm,
    Yarn,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectSelection {
    pub manager: PackageManager,
    pub selectors: Vec<String>,
    pub all: bool,
    pub include_root: bool,
    pub allow_missing: bool,
}

impl Context {
    pub(super) fn scripts(&mut self) -> Option<Source> {
        let args: Vec<_> =
            self.input.arguments.iter().chain(self.input.following_arguments()).cloned().collect();
        let program = args[0].trim_end_matches(".cmd");
        self.project.manager = match program {
            "npm" => PackageManager::Npm,
            "pnpm" => PackageManager::Pnpm,
            _ => PackageManager::Yarn,
        };
        let directory_flag = match program {
            "npm" => "--prefix",
            "pnpm" => "--dir",
            _ => "--cwd",
        };
        let selector_flag = if program == "npm" { "--workspace" } else { "--filter" };
        let selector_short = if program == "npm" { "-w" } else { "-F" };
        let mut positional = Vec::new();
        let mut index = 1;
        while let Some(arg) = args.get(index) {
            let (name, attached) =
                arg.split_once('=').map_or((arg.as_str(), None), |(n, v)| (n, Some(v)));
            let directory = name == directory_flag || program == "pnpm" && name == "-C";
            let selector =
                program != "yarn" && matches!(name, n if n == selector_flag || n == selector_short);
            if directory || selector {
                let value = if let Some(value) = attached {
                    value
                } else {
                    index += 1;
                    let Some(value) = args.get(index) else {
                        if directory {
                            self.directories.clear();
                        }
                        return Some(if selector {
                            Source::Workspaces
                        } else {
                            Source::Paths { directories_only: true }
                        });
                    };
                    value
                };
                if value.is_empty() {
                    return Some(Source::None);
                }
                if directory {
                    self.directories.push(value.to_owned());
                } else {
                    self.project.selectors.push(value.to_owned());
                }
            } else if name == "--workspaces" && program == "npm"
                || matches!(name, "--recursive" | "-r") && program == "pnpm"
            {
                self.project.all = attached.is_none_or(|value| value == "true");
            } else if name == "--include-workspace-root" {
                self.project.include_root = attached.is_none_or(|v| v == "true");
            } else if name == "--if-present" {
                self.project.allow_missing = attached.is_none_or(|value| value == "true");
            } else if matches!(name, "--silent" | "--parallel" | "--stream") {
            } else if name == "--" {
                return None;
            } else if arg.starts_with('-') {
                return None;
            } else {
                positional.push(arg.as_str());
            }
            index += 1;
        }
        if let Some(value) = self.input.prefix().strip_prefix(&format!("{selector_flag}="))
            && program != "yarn"
        {
            self.attached = Some(self.input.prefix().len() - value.len());
            return Some(Source::Workspaces);
        }
        if program == "yarn" && positional.first() == Some(&"workspace") {
            if positional.len() == 1 {
                return Some(Source::Workspaces);
            }
            self.project.selectors.push(positional[1].to_owned());
            positional.drain(..2);
            if positional.is_empty() {
                return Some(Source::ProjectScripts);
            }
        }
        if positional.len() == 1
            && matches!(positional[0], "run" | "run-script")
            && (program == "npm" || positional[0] == "run")
            && !self.input.prefix().starts_with('-')
        {
            Some(Source::ProjectScripts)
        } else {
            None
        }
    }
}
