use super::*;

fn repository() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    // 测试二进制没有控制台：不压掉这个 flag，每次 `git init` 都会在用户屏幕上
    // 弹一个终端窗口（见 `platform::process`）。
    let mut command = std::process::Command::new("git");
    command.args(["init", "-q"]).arg(directory.path());
    assert!(crate::platform::process::hidden_command(&mut command).status().unwrap().success());
    directory
}

fn file(root: &Path, path: &str) -> PathBuf {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "fixture").unwrap();
    path
}

/// 分支读取的四种形态：普通 ref、嵌套分支名、detached、linked worktree 的
/// `.git` 指针文件（相对 gitdir 以根为基准）。读不到（无 `.git`）为 `None`。
#[test]
fn head_file_yields_the_branch_for_grouping() {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path().join("repo");
    std::fs::create_dir_all(root.join(".git")).unwrap();

    std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    assert_eq!(super::git_head_branch(&root).as_deref(), Some("main"));
    std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/feature/x\n").unwrap();
    assert_eq!(super::git_head_branch(&root).as_deref(), Some("feature/x"));
    std::fs::write(root.join(".git/HEAD"), "0123456789abcdef0123456789abcdef01234567\n").unwrap();
    assert_eq!(
        super::git_head_branch(&root).as_deref(),
        Some("HEAD"),
        "detached 与 --abbrev-ref 同口径"
    );

    // linked worktree：`.git` 是 gitdir 指针，HEAD 在真 gitdir 里。
    let base = repo.path().join("main/.git/worktrees/wt");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(&base.join("HEAD"), "ref: refs/heads/wt-branch\n").unwrap();
    let worktree = repo.path().join("main/wt");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(worktree.join(".git"), "gitdir: ../.git/worktrees/wt\n").unwrap();
    assert_eq!(super::git_head_branch(&worktree).as_deref(), Some("wt-branch"));

    let plain = tempfile::tempdir().unwrap();
    assert_eq!(super::git_head_branch(plain.path()), None, "非仓库没有分支");
}

#[test]
fn remove_exact_rule_preserves_comments_crlf_and_other_paths() {
    let repo = repository();
    let root = repo.path();
    let path = file(root, "a[1].txt");
    let other = file(root, "other.txt");
    let ignore = root.join(".gitignore");
    std::fs::write(&ignore, "# keep\r\n/a\\[1\\].txt\r\n/other.txt").unwrap();
    assert!(matching_rule(root, &path).unwrap().is_some());
    assert!(matches!(remove_from_gitignore(&path, false).unwrap(), IgnoreOutcome::Removed { .. }));
    assert_eq!(std::fs::read_to_string(ignore).unwrap(), "# keep\r\n/other.txt");
    assert!(matching_rule(root, &path).unwrap().is_none());
    assert!(matching_rule(root, &other).unwrap().is_some());
    assert!(matches!(
        remove_from_gitignore(&path, false).unwrap(),
        IgnoreOutcome::AlreadyVisible { .. }
    ));
}

#[test]
fn wildcard_exceptions_and_parent_exceptions_keep_siblings_ignored() {
    for pattern in ["*.log\n", "/build/\n", "*\n"] {
        let repo = repository();
        let root = repo.path();
        let target = file(root, "build/deep/keep.log");
        let sibling = file(root, "build/deep/other.log");
        let cousin = file(root, "build/other.log");
        std::fs::write(root.join(".gitignore"), pattern).unwrap();
        remove_from_gitignore(&target, false).unwrap();
        assert!(matching_rule(root, &target).unwrap().is_none(), "{pattern}");
        assert!(matching_rule(root, &sibling).unwrap().is_some(), "{pattern}");
        assert!(matching_rule(root, &cousin).unwrap().is_some(), "{pattern}");
        assert!(std::fs::read_to_string(root.join(".gitignore")).unwrap().starts_with(pattern));
    }
}

#[test]
fn nested_rules_and_repository_excludes_are_handled_in_the_right_scope() {
    for source in ["sub/.gitignore", ".git/info/exclude"] {
        let repo = repository();
        let root = repo.path();
        let target = file(root, "sub/keep.log");
        let sibling = file(root, "sub/other.log");
        std::fs::write(root.join(source), "*.log\n").unwrap();
        remove_from_gitignore(&target, false).unwrap();
        assert!(matching_rule(root, &target).unwrap().is_none());
        assert!(matching_rule(root, &sibling).unwrap().is_some());
        assert!(std::fs::read_to_string(root.join(source)).unwrap().starts_with("*.log\n"));
    }
}

#[test]
fn exact_directory_rule_removal_restores_children() {
    let repo = repository();
    let root = repo.path();
    let child = file(root, "build/result.txt");
    append_to_gitignore(&root.join("build"), true).unwrap();
    remove_from_gitignore(&root.join("build"), true).unwrap();
    assert!(matching_rule(root, &child).unwrap().is_none());
}
