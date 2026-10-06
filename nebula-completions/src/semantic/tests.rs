use super::*;

#[test]
fn caret_roles_follow_later_options_without_consuming_later_positionals() {
    for (line, prefix, expected) in [
        ("git switch rel-old --detach", "git switch rel", Source::Revisions { include_busy: true }),
        (
            "git checkout main-old -- README",
            "git checkout ma",
            Source::Revisions { include_busy: false },
        ),
        ("git push origin topic-old --delete", "git push origin to", Source::RemoteBranches),
        ("git switch topic-old --conflict", "git switch to", Source::None),
    ] {
        assert_eq!(
            Context::parse(line, prefix.len(), ShellSyntax::Posix).unwrap().source,
            expected,
            "{line}"
        );
    }
    let line = "git switch auto/in-old --no-track";
    let context = Context::parse(line, "git switch auto/in".len(), ShellSyntax::Posix).unwrap();
    assert_eq!(context.source, Source::Tracking { mode: TrackingMode::Disabled, infer_name: true });
    assert!(!context.guesses_branches(true));
    for line in ["npm run bu-old --prefix ~/project", "npm run bu-old --workspace 'unterminated"] {
        assert!(Context::parse(line, "npm run bu".len(), ShellSyntax::Posix).is_none());
    }
}

#[test]
fn refspec_caret_edits_keep_the_explicit_destination_and_force_marker() {
    for syntax in [ShellSyntax::Posix, ShellSyntax::PowerShell] {
        for command in ["push", "fetch"] {
            let line =
                format!("git {command} origin \"+feature/中-old:refs/heads/publish\" --quiet");
            let cursor = format!("git {command} origin \"+feature/中").len();
            let context = Context::parse(&line, cursor, syntax).unwrap();
            let candidate = context.candidates(["feature/中文"]).remove(0);
            assert_eq!(
                format!(
                    "{}{}{}",
                    &line[..candidate.span.start],
                    candidate.value,
                    &line[candidate.span.end..]
                ),
                format!("git {command} origin \"+feature/中文:refs/heads/publish\" --quiet")
            );
        }
    }
}

#[test]
fn network_commands_keep_remote_refspec_and_option_roles_distinct() {
    use crate::command_context::ShellSyntax;
    for syntax in [ShellSyntax::Posix, ShellSyntax::PowerShell, ShellSyntax::Cmd] {
        let parse = |line: &str| Context::parse(line, line.len(), syntax).unwrap();
        for line in ["git push or", "git fetch --multiple or", "git pull --rebase or"] {
            assert!(matches!(parse(line).source, Source::Remotes), "{line}");
        }
        let context = parse("git push -u origin feat:to");
        assert!(matches!(context.source, Source::PushRefs { destination: true }));
        assert_eq!(context.remote.as_deref(), Some("origin"));
        assert_eq!(context.candidate("topic").unwrap().value, "feat:topic");
        assert!(matches!(
            parse("git push origin fe").source,
            Source::PushRefs { destination: false }
        ));
        for line in
            ["git push --delete origin to", "git fetch origin to", "git pull --ff-only origin to"]
        {
            assert!(matches!(parse(line).source, Source::RemoteBranches), "{line}");
        }
        assert!(matches!(parse("git fetch --all to").source, Source::None));
        assert!(matches!(parse("git push --force-with-lease=to").source, Source::None));
    }
}

#[test]
fn workspace_flags_and_script_positions_share_literal_edits() {
    use crate::command_context::ShellSyntax;
    for syntax in [ShellSyntax::Posix, ShellSyntax::PowerShell, ShellSyntax::Cmd] {
        let parse = |line: &str| Context::parse(line, line.len(), syntax).unwrap();
        for line in [
            "npm -w app run bu",
            "npm run --workspace app bu",
            "pnpm --filter app run bu",
            "yarn workspace app bu",
            "yarn workspace app run bu",
        ] {
            let context = parse(line);
            assert!(matches!(context.source, Source::ProjectScripts), "{line}");
            assert_eq!(context.project.selectors, ["app"]);
        }
        let context = parse("npm --workspace=ap");
        assert!(matches!(context.source, Source::Workspaces));
        assert_eq!(context.candidate("app").unwrap().value, "--workspace=app");
        assert!(matches!(parse("yarn workspace ap").source, Source::Workspaces));
        assert!(matches!(parse("pnpm --filter ap").source, Source::Workspaces));
        let context = parse("npm --workspaces --if-present run bu");
        assert!(context.project.all && context.project.allow_missing);
    }
}

#[test]
fn cursor_edits_preserve_suffix_arguments_and_utf8_boundaries() {
    for syntax in [ShellSyntax::Posix, ShellSyntax::PowerShell, ShellSyntax::Cmd] {
        let line = "git switch fe-old --quiet";
        let context = Context::parse(line, "git switch fe".len(), syntax).unwrap();
        let candidate = context.candidate("feature/new").unwrap();
        assert_eq!(
            format!(
                "{}{}{}",
                &line[..candidate.span.start],
                candidate.value,
                &line[candidate.span.end..]
            ),
            "git switch feature/new --quiet"
        );
        let line = "npm run bu-old --workspace app";
        let context = Context::parse(line, "npm run bu".len(), syntax).unwrap();
        assert_eq!(context.project.selectors, ["app"]);
        assert!(matches!(context.source, Source::ProjectScripts));
    }
    let line = "git switch \"feature/中-old\" --quiet";
    let cursor = "git switch \"feature/中".len();
    let context = Context::parse(line, cursor, ShellSyntax::PowerShell).unwrap();
    let candidate = context.candidate("feature/中文").unwrap();
    assert_eq!(
        format!(
            "{}{}{}",
            &line[..candidate.span.start],
            candidate.value,
            &line[candidate.span.end..]
        ),
        "git switch \"feature/中文\" --quiet"
    );
    assert!(Context::parse(line, cursor - 1, ShellSyntax::PowerShell).is_none());
}

fn context(line: &str) -> Context {
    Context::parse(line, line.len(), ShellSyntax::Posix).unwrap()
}

#[test]
fn ssh_completion_preserves_login_jump_and_option_values() {
    for syntax in [ShellSyntax::Posix, ShellSyntax::PowerShell, ShellSyntax::Cmd] {
        for (line, expected) in [
            ("ssh pro", "prod"),
            ("ssh me@pro", "me@prod"),
            ("ssh ssh://me@pro", "ssh://me@prod"),
            ("ssh -J first,me@pro", "first,me@prod"),
            ("ssh -vJfirst,me@pro", "-vJfirst,me@prod"),
        ] {
            let context = Context::parse(line, line.len(), syntax).unwrap();
            assert!(matches!(context.source, Source::SshHosts { .. }), "{line}");
            assert_eq!(context.value_prefix(), "pro");
            let candidate = context.candidates(["prod"]).pop().unwrap();
            let decoded =
                CommandContext::parse(&candidate.value, candidate.value.len(), syntax).unwrap();
            assert_eq!(decoded.prefix(), expected, "{syntax:?}: {line}");
        }
        for line in ["ssh -p ", "ssh -p22", "ssh prod ", "ssh prod cat ", "ssh -o "] {
            assert_eq!(
                Context::parse(line, line.len(), syntax).unwrap().source,
                Source::None,
                "{line}"
            );
        }
        let line = "ssh -Fconfig -iidentity pro";
        assert_eq!(
            Context::parse(line, line.len(), syntax).unwrap().ssh_config.as_deref(),
            Some("config")
        );
        let line = "ssh -viidentity";
        let c = Context::parse(line, line.len(), syntax).unwrap();
        assert_eq!(c.source, Source::Paths { directories_only: false });
        let expected =
            if syntax == ShellSyntax::PowerShell { "'-viidentity.pem'" } else { "-viidentity.pem" };
        assert_eq!(c.candidate("identity.pem").unwrap().value, expected);
    }
    assert!(context("ssh -F ~/.ssh/config pro").ssh_config_expands_home);
    assert!(!context("ssh -F '~/.ssh/config' pro").ssh_config_expands_home);
    assert_eq!(context("ssh -F=literal pro").ssh_config.as_deref(), Some("=literal"));
    assert_eq!(context("ssh -F first -F second pro").ssh_config.as_deref(), Some("second"));
}

#[test]
fn powershell_attached_paths_are_single_native_arguments() {
    for (line, path, expected) in [
        ("ssh -Fqa-ssh.c", "qa-ssh.conf", "'-Fqa-ssh.conf'"),
        ("ssh -iC:/ke", "C:/keys/id", "'-iC:/keys/id'"),
        ("ssh -viid", "identity.pem", "'-viidentity.pem'"),
        ("ssh -Fconf", "config", "-Fconfig"),
    ] {
        let c = Context::parse(line, line.len(), ShellSyntax::PowerShell).unwrap();
        assert_eq!(c.candidate(path).unwrap().value, expected);
    }
    for (line, expected) in
        [("git switch --qui", "--quiet"), ("Get-Content -LiteralP", "-LiteralPath")]
    {
        let c = Context::parse(line, line.len(), ShellSyntax::PowerShell).unwrap();
        assert_eq!(c.static_candidates()[0].value, expected);
    }
}

#[test]
fn wsl_completion_separates_registered_names_paths_and_guest_commands() {
    for syntax in [ShellSyntax::Posix, ShellSyntax::PowerShell, ShellSyntax::Cmd] {
        for line in [
            "wsl -d ",
            "wsl.exe --distribution De",
            "wsl --export ",
            "wsl --set-version ",
            "wsl --terminate De",
        ] {
            let c = Context::parse(line, line.len(), syntax).unwrap();
            assert_eq!(c.source, Source::WslDistributions);
            assert_eq!(c.candidates(["Debian"])[0].value, "Debian");
        }
        for line in [
            "wsl -e cat ",
            "wsl -- cat ",
            "wsl -d Debian cat ",
            "wsl --cd /",
            "wsl --user ",
            "wsl --install ",
            "wsl --install Ubuntu -d ",
        ] {
            assert_eq!(
                Context::parse(line, line.len(), syntax).unwrap().source,
                Source::None,
                "{line}"
            );
        }
        for (line, directories_only) in [
            ("wsl --export Debian ", false),
            ("wsl --import New ", true),
            ("wsl --import New root ", false),
        ] {
            assert_eq!(
                Context::parse(line, line.len(), syntax).unwrap().source,
                Source::Paths { directories_only },
                "{line}"
            );
        }
        let line = "wsl --set-version Debian ";
        assert_eq!(Context::parse(line, line.len(), syntax).unwrap().static_candidates().len(), 2);
        for line in ["wsl -dDebian ", "wsl --distribution=Debian ", "wsl -lv "] {
            assert_eq!(
                Context::parse(line, line.len(), syntax).unwrap().source,
                Source::None,
                "{line}"
            );
        }
    }
}

#[test]
fn common_commands_select_the_correct_cli_and_argument_roles() {
    assert!(
        Context::parse("cat fi", 6, ShellSyntax::Literal).unwrap().candidate("file@host").is_none()
    );
    for syntax in [ShellSyntax::Posix, ShellSyntax::Literal] {
        for line in ["ls -al fi", "cp -R src fi", "grep -e pattern fi", "grep pattern fi"] {
            assert_eq!(
                Context::parse(line, line.len(), syntax).unwrap().source,
                Source::Paths { directories_only: false },
                "{line}"
            );
        }
        for line in ["grep ", "grep -e ", "mkdir -m "] {
            assert_eq!(Context::parse(line, line.len(), syntax).unwrap().source, Source::None);
        }
        assert!(!Context::parse("ls -", 4, syntax).unwrap().static_candidates().is_empty());
    }
    for line in [
        "Get-ChildItem -LiteralPath fi",
        "Copy-Item -Destination fi",
        "Get-Content -literalpath fi",
    ] {
        assert_eq!(
            Context::parse(line, line.len(), ShellSyntax::PowerShell).unwrap().source,
            Source::Paths { directories_only: false }
        );
    }
    assert_eq!(
        Context::parse("Get-Content -Tail ", "Get-Content -Tail ".len(), ShellSyntax::PowerShell)
            .unwrap()
            .source,
        Source::None
    );
    for line in ["DIR /S fi", "copy /Y src fi", "type fi"] {
        assert_eq!(
            Context::parse(line, line.len(), ShellSyntax::Cmd).unwrap().source,
            Source::Paths { directories_only: false }
        );
    }
    assert_eq!(
        Context::parse("cd /d fi", 8, ShellSyntax::Cmd).unwrap().source,
        Source::Paths { directories_only: true }
    );
}

#[test]
fn branches_respect_argument_roles_directories_and_worktrees() {
    for syntax in [ShellSyntax::Posix, ShellSyntax::PowerShell, ShellSyntax::Cmd] {
        for line in [
            "git switch ",
            "git switch --quiet fe",
            "git switch -- fe",
            "git -C \"中文 repo\" switch \"fe\"",
        ] {
            let context = Context::parse(line, line.len(), syntax).unwrap();
            assert!(matches!(context.source, Source::Branches { .. }));
            let candidate = context.candidates(["feature/中文"]).pop().unwrap();
            assert!(line.is_char_boundary(candidate.span.start));
            assert_eq!(candidate.span.end, line.len());
            assert!(candidate.value.contains("feature/中文"));
            if line.contains("-C") {
                assert_eq!(context.directories, ["中文 repo"]);
            }
        }
    }
    for line in [
        "git switch --detach ma",
        "git switch -c new fe",
        "git switch -C new ma",
        "git merge ma",
        "git rebase --onto ma",
        "git rebase main fe",
        "git checkout -b new ma",
    ] {
        assert_eq!(context(line).source, Source::Revisions { include_busy: true }, "{line}");
    }
    for line in [
        "git switch -c ",
        "git switch --orphan new",
        "git switch main ",
        "git switch --conflict invalid fe",
        "git rebase --root main ",
        "git merge -m ",
        "git rebase --exec ",
        "git rebase --abort ",
        "git rebase main topic ",
    ] {
        assert_eq!(context(line).source, Source::None, "{line}");
    }
    for line in [
        "echo git switch fe",
        "git -c alias.switch=x switch fe",
        "git switch $(echo fe)",
        "git switch fe; pwd",
        "git switch fe | cat",
    ] {
        assert!(Context::parse(line, line.len(), ShellSyntax::Posix).is_none(), "{line}");
    }
    let middle = Context::parse("git switch feat", 13, ShellSyntax::Posix).unwrap();
    assert_eq!(middle.value_prefix(), "fe");
    assert_eq!(middle.candidate("feature").unwrap().span.end, 15);
}

#[test]
fn automatic_branch_creation_respects_explicit_flags_in_each_shell() {
    for syntax in [ShellSyntax::Posix, ShellSyntax::PowerShell, ShellSyntax::Cmd] {
        for (line, configured, expected) in [
            ("git switch topic", true, true),
            ("git checkout topic", true, true),
            ("git switch topic", false, false),
            ("git switch --guess topic", false, true),
            ("git switch --guess --no-guess topic", true, false),
            ("git checkout --no-guess --guess topic", false, true),
            ("git switch --no-track --guess topic", true, false),
            ("git checkout --no-track topic", true, false),
            ("git switch --detach topic", true, false),
            ("git switch -c new topic", true, false),
            ("git checkout -b new topic", true, false),
            ("git checkout -- topic", true, false),
            ("git merge topic", true, false),
        ] {
            assert_eq!(
                Context::parse(line, line.len(), syntax).unwrap().guesses_branches(configured),
                expected,
                "{syntax:?}: {line}"
            );
        }
    }
}

#[test]
fn explicit_tracking_uses_optional_values_and_new_branch_start_roles() {
    for syntax in [ShellSyntax::Posix, ShellSyntax::PowerShell, ShellSyntax::Cmd] {
        for (line, mode, infer_name) in [
            ("git switch --track origin/", TrackingMode::Direct, true),
            ("git switch --track -- origin/", TrackingMode::Direct, true),
            ("git checkout -t origin/", TrackingMode::Direct, true),
            ("git switch --track=inherit local/", TrackingMode::Inherit, true),
            ("git switch --no-track origin/", TrackingMode::Disabled, true),
            ("git switch -c new --track main", TrackingMode::Direct, false),
            ("git switch -c new --track -- main", TrackingMode::Direct, false),
            ("git checkout --track=inherit -b new local/", TrackingMode::Inherit, false),
            ("git switch --no-track --track origin/", TrackingMode::Direct, true),
            ("git switch --track --no-track origin/", TrackingMode::Disabled, true),
        ] {
            let context = Context::parse(line, line.len(), syntax).unwrap();
            assert_eq!(context.source, Source::Tracking { mode, infer_name }, "{syntax:?}: {line}");
            assert!(!context.guesses_branches(true));
        }
        let line = "git switch --track=inh";
        assert_eq!(
            Context::parse(line, line.len(), syntax).unwrap().static_candidates()[0].value,
            "--track=inherit"
        );
    }
    for line in [
        "git switch --track=wrong origin/",
        "git switch --track --detach origin/",
        "git checkout --track -- file",
        "git checkout --track -b new -- origin/",
        "git checkout --track origin/main -- file",
        "git switch --track origin/main next",
        "git switch --track direct origin/",
    ] {
        assert_eq!(context(line).source, Source::None, "{line}");
    }
}

#[test]
fn paths_and_ambiguous_arguments_retain_directory_scope() {
    for line in ["git checkout -- src", "git checkout main -- src", "git checkout main src"] {
        assert_eq!(context(line).source, Source::Paths { directories_only: false });
    }
    assert_eq!(
        context("git checkout src").source,
        Source::RevisionsAndPaths { include_busy: false }
    );
    for line in
        ["git -C repo", "git -C one -C two", "npm --prefix repo", "pnpm -C repo", "yarn --cwd repo"]
    {
        assert_eq!(context(line).source, Source::Paths { directories_only: true }, "{line}");
    }
    assert_eq!(context("git -C one -C two").directories, ["one"]);
    assert!(context("npm --prefix one --prefix two").directories.is_empty());
    assert_eq!(
        context("git checkout --ignore-other-worktrees fe").source,
        Source::RevisionsAndPaths { include_busy: true }
    );
    assert_eq!(context("git -C one checkout -- file").directories, ["one"]);
}

#[test]
fn path_quotes_roundtrip_literal_characters_and_home_intent() {
    for syntax in [ShellSyntax::Posix, ShellSyntax::PowerShell] {
        for value in ["中文 repo/file", "'quote", "a'b/child", "~literal", ".../file", "$var`file"]
        {
            let input = CommandContext::parse("cat a", 5, syntax).unwrap();
            let candidate = input.candidate(value).unwrap();
            let line = format!("cat {}", candidate.value);
            let decoded = CommandContext::parse(&line, line.len(), syntax).unwrap();
            assert_eq!(decoded.prefix(), value, "{syntax:?}: {line}");
        }
        let input = CommandContext::parse("cat ~/", 6, syntax).unwrap();
        assert!(input.expands_home());
        let input = CommandContext::parse("cat '~/", 7, syntax).unwrap();
        assert!(!input.expands_home());
    }
    let input = CommandContext::parse("git -C re", 9, ShellSyntax::Cmd).unwrap();
    assert_eq!(input.candidate("repo 中文/").unwrap().value, "\"repo 中文/\"");
    assert!(input.candidate("repo%PATH%/").is_none());
    assert!(input.candidate("repo!/").is_none());
    assert!(input.candidate("repo name\\").is_none());
}

#[test]
fn subcommands_options_and_option_values_use_the_same_context() {
    for (line, expected) in [
        ("git sw", "switch"),
        ("git switch --qui", "--quiet"),
        ("git switch --conflict zd", "zdiff3"),
        ("git rebase --empty k", "keep"),
        ("git rebase --empty=k", "--empty=keep"),
    ] {
        assert_eq!(context(line).static_candidates()[0].value, expected, "{line}");
    }
    assert_eq!(context("git rebase --onto=ma").candidates(["main"])[0].value, "--onto=main");
    assert_eq!(context("git -C one -C two sw").directories, ["one", "two"]);
}

#[test]
fn scripts_only_occupy_the_name_position_and_keep_explicit_directory_scope() {
    for line in ["npm run ", "npm.cmd run bu", "npm run-script bu", "pnpm run bu", "yarn run bu"] {
        assert_eq!(context(line).source, Source::ProjectScripts, "{line}");
    }
    for line in [
        "npm --prefix '中文 repo' run bu",
        "pnpm -C '中文 repo' run bu",
        "yarn --cwd '中文 repo' run bu",
    ] {
        assert_eq!(context(line).directories, ["中文 repo"]);
    }
    for line in [
        "npm run build --wa",
        "npm exec bu",
        "npm run build -- argument",
        "pnpm --filter other exec bu",
        "yarn run --top-level bu",
    ] {
        assert!(Context::parse(line, line.len(), ShellSyntax::Posix).is_none(), "{line}");
    }
}

#[test]
fn matching_and_quoting_preserve_literal_utf8_arguments() {
    for (syntax, expected) in
        [(ShellSyntax::Posix, "'feat'\\''$x'"), (ShellSyntax::PowerShell, "'feat''$x'")]
    {
        let line = "git switch 'fe";
        let context = Context::parse(line, line.len(), syntax).unwrap();
        assert_eq!(context.candidates(["feat'$x"])[0].value, expected);
    }
    let line = "npm run \"build:中\"";
    for syntax in [ShellSyntax::Posix, ShellSyntax::PowerShell, ShellSyntax::Cmd] {
        let candidates =
            Context::parse(line, line.len(), syntax).unwrap().candidates(["build:中文"]);
        assert_eq!(candidates[0].value, "\"build:中文\"");
    }
    assert_eq!(context("npm run bld").candidates(["build"])[0].value, "build");
    let line = "npm run bu";
    let ctx = Context::parse(line, line.len(), ShellSyntax::Cmd).unwrap();
    assert!(ctx.candidates(["build%PATH%", "build\nunsafe"]).is_empty());
    assert_eq!(context("git -C repo\\ name switch fe").directories, ["repo name"]);
    let line = "git -C \"D:\\repo name\" switch fe";
    assert_eq!(
        Context::parse(line, line.len(), ShellSyntax::PowerShell).unwrap().directories,
        ["D:\\repo name"]
    );
    let line = "git -C repo`name switch fe";
    assert!(Context::parse(line, line.len(), ShellSyntax::PowerShell).is_none());
    assert!(Context::parse(&"x".repeat(4097), 4097, ShellSyntax::Posix).is_none());
}
