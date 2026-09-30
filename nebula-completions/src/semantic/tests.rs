use super::*;

fn context(line: &str) -> Context {
    Context::parse(line, line.len(), ShellSyntax::Posix).unwrap()
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
    assert!(Context::parse("git switch feat", 13, ShellSyntax::Posix).is_none());
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
        "npm --workspace missing run bu",
        "pnpm --filter other run bu",
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
