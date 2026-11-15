use super::*;
use crate::platform::ai_session_identity::UpdatePause as HandoffPause;
use std::process::Command;
use windows_sys::Win32::System::Threading::GetProcessId;

fn start_probe() -> OwnedHandle {
    // An empty identity reply still leaves a healthy native worker resident.
    assert!(probe(0, 0).is_none());
    WORKER.lock().unwrap().as_ref().unwrap().process.try_clone().unwrap()
}

fn assert_paused() {
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| assert!(probe(0, 0).is_none()));
        }
    });
    assert!(WORKER.lock().unwrap().is_none(), "background polls must not recreate the worker");
}

fn check_processes(handles: &[&OwnedHandle]) -> std::process::Output {
    let directory = tempfile::tempdir().unwrap();
    let helper = directory.path().join("handoff.ps1");
    std::fs::write(&helper, include_bytes!("../../../../update_download/handoff.ps1")).unwrap();
    // Execute the actual updater guard against the selected real processes. The
    // full test runner may have unrelated tests running from this same binary.
    let script = r#"
$ErrorActionPreference = 'Stop'
$tokens = $null
$errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile(
    $env:PEBREL_PAUSE_HELPER, [ref]$tokens, [ref]$errors)
if ($errors) { throw 'Invalid production helper' }
$functions = $ast.FindAll({ param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -in @('Same-Path', 'Check-UnpreparedProcesses')
}, $false)
if ($functions.Count -ne 2) { throw 'Missing production process guard' }
foreach ($function in $functions) { . ([scriptblock]::Create($function.Extent.Text)) }
$exe = $env:PEBREL_PAUSE_EXE
$plan = @{ participants = @(@{ pid = [int]$env:PEBREL_PAUSE_OWNER }) }
$ids = @($env:PEBREL_PAUSE_PIDS.Split(',') | ForEach-Object { [int]$_ })
$processes = @([System.Diagnostics.Process]::GetProcessesByName(
    [System.IO.Path]::GetFileNameWithoutExtension($exe)) | Where-Object { $ids -contains $_.Id })
try { Check-UnpreparedProcesses $processes }
catch { [Console]::Out.Write($_.Exception.Message); exit 1 }
"#;
    let system = std::env::var_os("SystemRoot").unwrap();
    let mut command = Command::new(
        std::path::Path::new(&system).join("System32/WindowsPowerShell/v1.0/powershell.exe"),
    );
    let mut ids = vec![std::process::id().to_string()];
    ids.extend(
        handles.iter().map(|handle| unsafe { GetProcessId(handle.as_raw_handle()).to_string() }),
    );
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("PEBREL_PAUSE_HELPER", helper)
        .env("PEBREL_PAUSE_EXE", std::env::current_exe().unwrap())
        .env("PEBREL_PAUSE_OWNER", std::process::id().to_string())
        .env("PEBREL_PAUSE_PIDS", ids.join(","));
    crate::platform::process::hidden_command(&mut command).output().unwrap()
}

fn assert_blocked(output: std::process::Output) {
    assert!(!output.status.success(), "a live unprepared process must block installation");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Another process from this installation is not prepared for update",
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn update_pause_coordinates_native_workers_and_preserves_the_process_guard() {
    // The global pause deliberately survives commit. Isolate it from other tests.
    if std::env::var_os("PEBREL_TEST_UPDATE_PAUSE").is_none() {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "platform::ai_session_identity::windows::worker::update_tests::update_pause_coordinates_native_workers_and_preserves_the_process_guard",
                "--nocapture",
            ])
            .env("PEBREL_TEST_UPDATE_PAUSE", "1");
        let output = crate::platform::process::hidden_command(&mut command).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        return;
    }

    let original = start_probe();
    assert_blocked(check_processes(&[&original]));
    let pause = HandoffPause::acquire().unwrap();
    assert_eq!(unsafe { WaitForSingleObject(original.as_raw_handle(), 0) }, WAIT_OBJECT_0);
    assert_paused();
    let output = check_processes(&[&original]);
    assert!(output.status.success(), "{output:?}");

    // An overlapping failed/cancelled attempt cannot release another attempt's gate.
    let overlapping = HandoffPause::acquire().unwrap();
    drop(pause);
    assert_paused();
    let unrelated = Worker::start(&arguments(), Instant::now() + PROBE_TIMEOUT).unwrap();
    assert_blocked(check_processes(&[&unrelated.process]));
    assert_eq!(unsafe { WaitForSingleObject(unrelated.process.as_raw_handle(), 0) }, WAIT_TIMEOUT);
    drop(overlapping);
    let resumed = start_probe();
    assert_eq!(unsafe { WaitForSingleObject(resumed.as_raw_handle(), 0) }, WAIT_TIMEOUT);

    // Failure to obtain the worker lock must roll back only its own pause.
    let slot = WORKER.lock().unwrap();
    assert!(HandoffPause::acquire().is_err());
    assert_eq!(UPDATE_PAUSES.load(Ordering::Acquire), 0);
    assert_eq!(unsafe { WaitForSingleObject(resumed.as_raw_handle(), 0) }, WAIT_TIMEOUT);

    // Preparation waits for in-flight work; its gate closes before the wait.
    let preparing = std::thread::spawn(HandoffPause::acquire);
    let deadline = Instant::now() + PROBE_TIMEOUT;
    while UPDATE_PAUSES.load(Ordering::Acquire) == 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(POLL);
    }
    assert!(probe(0, 0).is_none(), "new probes must not queue behind preparation");
    assert!(!preparing.is_finished(), "preparation must wait for the active probe's lock");
    drop(slot);
    let mut committed = preparing.join().unwrap().unwrap();
    assert_eq!(unsafe { WaitForSingleObject(resumed.as_raw_handle(), 0) }, WAIT_OBJECT_0);
    committed.commit();
    drop(committed);
    assert_paused();
}
