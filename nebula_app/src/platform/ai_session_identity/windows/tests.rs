use super::*;
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::os::windows::process::CommandExt as _;
use std::process::Child;

const ID: &str = "0199a213-c2a4-7cf5-8f6b-d746fbb6e86c";
const OTHER: &str = "0199a213-c2a4-7cf5-8f6b-d746fbb6e86d";
const HOLDER: &str = "platform::ai_session_identity::windows::tests::hold_rollouts";

#[test]
fn probe_helper() {
    let Ok(parameters) = std::env::var("PEBREL_TEST_PROBE_ROOT") else { return };
    if parameters == "stall" {
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    let (pid, created) = parameters.split_once(':').unwrap();
    // Exit directly so the production parent also observes helper failure.
    std::process::exit(emit_records(pid.parse().unwrap(), created.parse().unwrap()));
}

#[test]
fn hold_rollouts() {
    let Ok(paths) = std::env::var("PEBREL_TEST_ROLLOUT_FILES") else { return };
    let paths: Vec<String> = serde_json::from_str(&paths).unwrap();
    let mut files: Vec<_> = paths
        .iter()
        .map(|path| {
            let mut file = std::fs::File::open(path).unwrap();
            file.seek(SeekFrom::Start(7)).unwrap();
            file
        })
        .collect();
    std::fs::write(
        std::env::var_os("PEBREL_TEST_ROLLOUT_READY").unwrap(),
        std::process::id().to_string(),
    )
    .unwrap();
    let mut input = [0];
    std::io::stdin().read_exact(&mut input).unwrap();
    for file in &mut files {
        assert_eq!(file.stream_position().unwrap(), 7, "probing changed the live file cursor");
    }
}

struct Fixture {
    child: Child,
    worker: OwnedHandle,
}

impl Fixture {
    fn launch(directory: &Path, executable: &Path, paths: &[&Path], powershell: bool) -> Self {
        let ready = directory.join("ready");
        let mut command = if powershell {
            let mut command = Command::new("powershell.exe");
            command.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!("& $env:PEBREL_TEST_ROLLOUT_EXE --exact {HOLDER} --nocapture | Out-Null"),
            ]);
            command
        } else {
            let mut command = Command::new("cmd.exe");
            // The product test binary is a GUI executable. Unlike real Codex,
            // CMD will not wait for it implicitly, so keep this shell alive
            // explicitly until the owned stand-in exits.
            command.args(["/d", "/s", "/c"]).raw_arg(format!(
                "start \"\" /b /wait \"{}\" --exact {HOLDER} --nocapture",
                executable.display()
            ));
            command
        };
        command
            .env("PEBREL_TEST_ROLLOUT_EXE", executable)
            .env("PEBREL_TEST_ROLLOUT_FILES", serde_json::to_string(paths).unwrap())
            .env("PEBREL_TEST_ROLLOUT_READY", &ready)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        crate::platform::process::hidden_command(&mut command);
        let mut child = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let worker = loop {
            if let Ok(pid) = std::fs::read_to_string(&ready) {
                if let Ok(pid) = pid.parse() {
                    break open_process(
                        pid,
                        windows_sys::Win32::System::Threading::PROCESS_TERMINATE,
                    )
                    .unwrap();
                }
            }
            if child.try_wait().unwrap().is_some() || Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("owned rollout fixture did not become ready");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        Self { child, worker }
    }

    fn finish(mut self) {
        self.child.stdin.take().unwrap().write_all(b"x\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "the live file cursor must be unchanged");
                return;
            }
            assert!(Instant::now() < deadline, "fixture did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::System::Threading::TerminateProcess(self.worker.0, 1) };
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn rollout(
    directory: &Path,
    filename_id: &str,
    metadata_id: &str,
    source: &str,
) -> std::path::PathBuf {
    let path = directory.join(format!("rollout-date-{filename_id}.jsonl"));
    let meta = serde_json::json!({"type":"session_meta", "payload": {
        "id": metadata_id, "cwd": directory, "source": source,
        "thread_source": "user", "private_context": "do not transport",
    }});
    std::fs::write(&path, format!("{meta}\n{{\"conversation\":\"do not read\"}}\n")).unwrap();
    path
}

#[test]
fn native_shells_bind_only_their_own_open_rollout_and_keep_the_id_and_file_cursor() {
    let temp = tempfile::tempdir_in(std::env::current_exe().unwrap().parent().unwrap()).unwrap();
    let executable = temp.path().join("codex.exe");
    std::fs::hard_link(std::env::current_exe().unwrap(), &executable).unwrap();
    let file = rollout(temp.path(), ID, ID, "cli");
    let second = tempfile::tempdir().unwrap();
    let other = rollout(second.path(), OTHER, OTHER, "cli");
    let unrelated = Fixture::launch(second.path(), &executable, &[&other], false);
    for powershell in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let fixture = Fixture::launch(root.path(), &executable, &[&file], powershell);
        let found = probe(fixture.child.id()).expect("read the owned CMD/PowerShell child rollout");
        assert_eq!(found.session_id, ID);
        assert_eq!(Path::new(&found.session_file), file);
        assert_eq!(found.cwd.as_deref(), temp.path().to_str());
        let shell = open_process(fixture.child.id(), PROCESS_QUERY_INFORMATION).unwrap();
        let output = collect_records(fixture.child.id(), creation_time(shell.0).unwrap()).unwrap();
        assert!(!output.contains("do not"));
        assert_eq!(collect_records(fixture.child.id(), 1), None, "reject recycled shell identity");
        fixture.finish();
    }
    unrelated.finish();
}

#[test]
fn native_probe_rejects_ambiguous_and_mismatched_rollouts_without_changing_them() {
    let temp = tempfile::tempdir_in(std::env::current_exe().unwrap().parent().unwrap()).unwrap();
    let executable = temp.path().join("codex.exe");
    std::fs::hard_link(std::env::current_exe().unwrap(), &executable).unwrap();
    let first = rollout(temp.path(), ID, ID, "cli");
    let second = rollout(temp.path(), OTHER, OTHER, "cli");
    let fixture = Fixture::launch(temp.path(), &executable, &[&first, &second], false);
    assert!(probe(fixture.child.id()).is_none());
    fixture.finish();
    std::fs::remove_file(temp.path().join("ready")).unwrap();
    rollout(temp.path(), ID, OTHER, "cli");
    let fixture = Fixture::launch(temp.path(), &executable, &[&first], false);
    assert!(probe(fixture.child.id()).is_none());
    fixture.finish();
}

#[test]
fn native_helper_is_terminated_at_the_probe_deadline() {
    let mut command = helper_command(0, 0).unwrap();
    command.env("PEBREL_TEST_PROBE_ROOT", "stall");
    let started = Instant::now();
    assert!(run_probe_command(command).is_none());
    assert!(started.elapsed() < Duration::from_secs(5));
}
