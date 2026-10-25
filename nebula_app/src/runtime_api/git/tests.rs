use super::*;
use std::path::Path;

fn git(cwd: &Path, args: &[&str]) -> String {
    let mut command = std::process::Command::new("git");
    crate::platform::process::hidden_command(&mut command);
    let output = command.arg("-C").arg(cwd).args(args).output().expect("run Git");
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

fn repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "--initial-branch=main"]);
    git(dir.path(), &["config", "user.name", "Mobile Git Test"]);
    git(dir.path(), &["config", "user.email", "mobile@example.invalid"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    dir
}

fn request(cwd: &Path, operation: &str, mut params: Value) -> Request {
    params["window_id"] = json!(1);
    params["pane_id"] = json!(2);
    params["expected_cwd"] = json!(cwd.to_str().unwrap());
    let command =
        command(&ApiRequest::new("test".into(), format!("git.{operation}"), params)).unwrap();
    let RuntimeCommand::Git { request, .. } = command else { panic!("Git command") };
    request
}

fn call(cwd: &Path, operation: &str, params: Value) -> Result<Value, ApiError> {
    let options = nebula_terminal::tty::Options {
        working_directory: Some(cwd.to_path_buf()),
        ..Default::default()
    };
    execute(
        PaneExecContext::from_pty_options(&options),
        cwd.to_str().unwrap().into(),
        &request(cwd, operation, params),
    )
}

fn revision(cwd: &Path) -> Value {
    call(cwd, "status", json!({})).unwrap()["revision"].clone()
}

#[test]
fn literal_paths_unborn_unstage_and_stale_commit() {
    let dir = repository();
    let path = dir.path();
    let name = "-中文 [draft].txt";
    std::fs::write(path.join(name), "first\n").unwrap();
    let status = call(path, "status", json!({})).unwrap();
    assert_eq!(status["status"]["entries"][0]["path"], name);
    assert!(
        call(path, "diff", json!({"path": name})).unwrap()["text"]
            .as_str()
            .unwrap()
            .contains("+first")
    );
    call(path, "stage", json!({"path": name, "revision": status["revision"]})).unwrap();
    call(path, "unstage", json!({"path": name, "revision": revision(path)})).unwrap();
    assert!(git(path, &["ls-files"]).is_empty());
    call(path, "stage", json!({"all": true, "revision": revision(path)})).unwrap();
    let stale = revision(path);
    std::fs::write(path.join(name), "restaged\n").unwrap();
    git(path, &["add", "--", name]);
    assert_eq!(
        call(path, "commit", json!({"message": "stale", "revision": stale})).unwrap_err().code,
        "git_stale"
    );
    call(path, "commit", json!({"message": "initial", "revision": revision(path)})).unwrap();
    let history = call(path, "history", json!({})).unwrap();
    assert_eq!(history["commits"][0]["subject"], "initial");
    assert!(
        call(path, "diff", json!({"commit": history["commits"][0]["hash"]})).unwrap()["text"]
            .as_str()
            .unwrap()
            .contains("+restaged")
    );
    git(path, &["mv", "--", name, "renamed.txt"]);
    let status = call(path, "status", json!({})).unwrap();
    assert_eq!(status["status"]["entries"][0]["original"], name);
    call(path, "unstage", json!({"path": "renamed.txt", "revision": status["revision"]})).unwrap();
    assert!(git(path, &["diff", "--cached", "--name-only"]).is_empty());
    assert!(path.join("renamed.txt").is_file());
}

#[test]
fn local_remote_fetch_pull_push_and_nested_cwd() {
    let dir = repository();
    let path = dir.path();
    std::fs::create_dir(path.join("src")).unwrap();
    std::fs::write(path.join("src/file.txt"), "one\n").unwrap();
    call(path, "stage", json!({"all": true, "revision": revision(path)})).unwrap();
    call(path, "commit", json!({"message": "initial", "revision": revision(path)})).unwrap();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare", "--initial-branch=main"]);
    git(path, &["remote", "add", "origin", remote.path().to_str().unwrap()]);
    git(path, &["push", "--set-upstream", "origin", "main"]);
    std::fs::write(path.join("src/file.txt"), "two\n").unwrap();
    let nested = path.join("src");
    call(&nested, "stage", json!({"path": "src/file.txt", "revision": revision(&nested)})).unwrap();
    call(&nested, "commit", json!({"message": "from phone", "revision": revision(&nested)}))
        .unwrap();
    call(&nested, "push", json!({"revision": revision(&nested)})).unwrap();
    assert_eq!(git(path, &["rev-parse", "HEAD"]), git(remote.path(), &["rev-parse", "main"]));
    call(path, "fetch", json!({"revision": revision(path)})).unwrap();
    call(path, "pull", json!({"revision": revision(path)})).unwrap();
    assert!(
        call(path, "status", json!({})).unwrap()["status"]["entries"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rejects_wrong_target_and_unknown_methods_before_execution() {
    let dir = repository();
    let options = nebula_terminal::tty::Options::default();
    let error = execute(
        PaneExecContext::from_pty_options(&options),
        "another cwd".into(),
        &request(dir.path(), "status", json!({})),
    )
    .unwrap_err();
    assert_eq!(error.code, "git_target_changed");
    assert!(command(&ApiRequest::new("test".into(), "git.reset", json!({}))).is_err());
    assert!(
        command(&ApiRequest::new(
            "test".into(),
            "git.stage",
            json!({"window_id":1,"pane_id":2,"expected_cwd":"/tmp"})
        ))
        .is_err()
    );
}
