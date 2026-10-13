//! Command/argument semantics. This module describes sources, never performs I/O.

use crate::command_context::{CommandContext, ShellSyntax};
use crate::{CandidateMatcher, CompletionOptions, CompletionSort, MatchAlgorithm, Suggestion};

mod common;
mod git;
mod scripts;
pub use scripts::{PackageManager, ProjectSelection};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackingMode {
    Direct,
    Inherit,
    Disabled,
}

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
    Tracking {
        mode: TrackingMode,
        infer_name: bool,
    },
    Paths {
        directories_only: bool,
    },
    Remotes,
    RemoteBranches,
    PushRefs {
        destination: bool,
    },
    Workspaces,
    ProjectScripts,
    SshHosts {
        jump: bool,
    },
    WslDistributions,
    Options,
    /// A known free-form value must not receive unrelated history/path candidates.
    None,
}

#[derive(Debug)]
pub struct Context {
    input: CommandContext,
    pub source: Source,
    pub directories: Vec<String>,
    pub remote: Option<String>,
    pub project: ProjectSelection,
    pub ssh_config: Option<String>,
    pub ssh_config_expands_home: bool,
    options: &'static [OptionSpec],
    attached: Option<usize>,
    preserved_tail: String,
    branch_guess: Option<bool>,
}

impl Context {
    pub fn parse(line: &str, cursor: usize, syntax: ShellSyntax) -> Option<Self> {
        let input = CommandContext::parse(line, cursor, syntax).or_else(|| {
            // SSH 的身份文件不改变 cwd；只对其已知参数保留 home 意图给宿主适配层。
            let input = CommandContext::parse_with_home(line, cursor, syntax)?;
            let name = input.arguments.first()?;
            (matches!(name.as_str(), "ssh" | "ssh.exe")
                || matches!(syntax, ShellSyntax::PowerShell | ShellSyntax::Cmd)
                    && (name.eq_ignore_ascii_case("ssh") || name.eq_ignore_ascii_case("ssh.exe")))
            .then_some(input)
        })?;
        let mut context = Self {
            input,
            source: Source::None,
            directories: Vec::new(),
            remote: None,
            project: ProjectSelection::default(),
            ssh_config: None,
            ssh_config_expands_home: false,
            options: &[],
            attached: None,
            preserved_tail: String::new(),
            branch_guess: None,
        };
        context.source = match context.input.arguments.first()?.as_str() {
            "git" | "git.exe" => context.git()?,
            "npm" | "npm.cmd" | "pnpm" | "pnpm.cmd" | "yarn" | "yarn.cmd" => context.scripts()?,
            _ => context.common(syntax)?,
        };
        if let Source::SshHosts { jump } = context.source {
            let base = context.attached.unwrap_or(0);
            let prefix = context.value_prefix();
            let start = if jump { prefix.rfind(',').map_or(0, |n| n + 1) } else { 0 };
            let login = prefix[start..]
                .rfind('@')
                .map(|n| start + n + 1)
                .or_else(|| prefix.starts_with("ssh://").then_some(6))
                .unwrap_or(start);
            context.attached = Some(base + login);
        }
        Some(context)
    }

    pub fn input(&self) -> &CommandContext {
        &self.input
    }

    pub fn value_prefix(&self) -> &str {
        &self.input.prefix()[self.attached.unwrap_or(0)..]
    }

    pub fn candidate(&self, value: &str) -> Option<Suggestion> {
        let full = (self.attached.is_some() || !self.preserved_tail.is_empty()).then(|| {
            format!(
                "{}{value}{}",
                &self.input.prefix()[..self.attached.unwrap_or(0)],
                self.preserved_tail
            )
        });
        self.input.candidate(full.as_deref().unwrap_or(value))
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
            if let Some(candidate) = self.candidate(value) {
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
}

#[derive(Debug)]
struct OptionSpec {
    names: &'static [&'static str],
    value: Option<Source>,
    include_busy: bool,
    terminal: bool,
    optional_value: bool,
}

const fn flag(names: &'static [&'static str]) -> OptionSpec {
    OptionSpec { names, value: None, include_busy: false, terminal: false, optional_value: false }
}
const fn value(names: &'static [&'static str], source: Source) -> OptionSpec {
    OptionSpec { value: Some(source), ..flag(names) }
}
#[cfg(test)]
mod tests;
