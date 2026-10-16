use super::*;
use std::path::PathBuf;

struct Fixture {
    root: tempfile::TempDir,
    config: PathBuf,
    installation: PathBuf,
    executable: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config");
        let installation = root.path().join("app");
        std::fs::create_dir_all(&installation).unwrap();
        std::fs::create_dir_all(config.join("updates/handoffs")).unwrap();
        let executable = installation.join("pebrel.exe");
        std::fs::write(&executable, b"current executable").unwrap();
        std::fs::write(installation.join("my notes.txt"), b"user file").unwrap();
        Self {
            root,
            config: canonical(&config).unwrap(),
            installation: canonical(&installation).unwrap(),
            executable: canonical(&executable).unwrap(),
        }
    }

    fn transaction(
        &self,
        id: &str,
        version: &str,
        success: bool,
        restored: bool,
    ) -> (PathBuf, PathBuf, PathBuf) {
        let directory = self.config.join("updates/handoffs").join(id);
        let stage = directory.join("portable");
        let backup = self.installation.join(format!(".pebrel-update-{id}"));
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(stage.join("pebrel.exe"), b"staged").unwrap();
        std::fs::write(backup.join("pebrel.exe"), b"backup").unwrap();
        let arch = if std::env::consts::ARCH == "aarch64" { "arm64" } else { "x64" };
        let name = format!("Pebrel-v{version}-windows-{arch}.zip");
        let package = self.config.join("updates").join(&name);
        std::fs::write(&package, b"ZIP fixture").unwrap();
        let plan = serde_json::json!({
            "schema":1, "asset": {"version":version, "name":name,
                "download_url":format!("https://github.com/Kuddev/pebrel/releases/download/v{version}/{name}"), "size":11, "sha256":"a".repeat(64)},
            "transaction":id, "executable":self.executable, "installation":self.installation,
            "config_directory":self.config, "installer":package, "sha256":"a".repeat(64), "bytes":11,
            "version":version, "original_version":"2.2.0", "guard_path":self.installation.join(".pebrel-update.nebula-lock"),
            "participants":[], "portable":{"directory":canonical(&stage).unwrap(),
                "files":[{"path":"pebrel.exe", "bytes":6, "sha256":"a".repeat(64)}]}
        });
        Self::json(&directory.join("plan.json"), plan);
        Self::json(
            &directory.join("result.json"),
            serde_json::json!({"transaction":id, "success":success, "version":version}),
        );
        Self::json(
            &directory.join("workspace.json"),
            serde_json::json!([{"fixture":"saved workspace"}]),
        );
        if restored {
            Self::json(&directory.join("restored.json"), serde_json::json!({}));
        }
        (directory, backup, package)
    }

    fn json(path: &Path, value: serde_json::Value) {
        std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    fn run(&self) -> io::Result<()> {
        run_at(&self.config, &self.executable, "2.3.0")
    }
}

#[test]
#[cfg(windows)]
fn acknowledged_success_retires_packages_and_staging_keeps_latest_backup_and_failures() {
    let fixture = Fixture::new();
    let (old, old_backup, old_zip) = fixture.transaction("1-100", "2.2.1", true, true);
    let (latest, latest_backup, latest_zip) = fixture.transaction("1-200", "2.3.0", true, true);
    let (failed, failed_backup, failed_zip) = fixture.transaction("1-300", "2.4.0", false, false);
    fixture.run().unwrap();
    assert!(!old.join("portable").exists() && !latest.join("portable").exists());
    assert!(!old_zip.exists() && !latest_zip.exists());
    assert!(!old_backup.exists() && latest_backup.exists());
    assert!(failed.join("portable").exists() && failed_backup.exists() && failed_zip.exists());
    assert!(old.join("plan.json").exists() && latest.join("workspace.json").exists());
    assert_eq!(std::fs::read(fixture.executable).unwrap(), b"current executable");
    assert_eq!(std::fs::read(fixture.installation.join("my notes.txt")).unwrap(), b"user file");
}

#[test]
#[cfg(windows)]
fn no_restore_ack_or_active_installation_means_no_cleanup() {
    let fixture = Fixture::new();
    let (directory, backup, zip) = fixture.transaction("1-200", "2.3.0", true, false);
    fixture.run().unwrap();
    assert!(directory.join("portable").exists() && backup.exists() && zip.exists());
    Fixture::json(&directory.join("restored.json"), serde_json::json!({}));
    let guard =
        crate::atomic_file::try_lifetime_lock(&guard_base(&fixture.executable)).unwrap().unwrap();
    fixture.run().unwrap();
    assert!(directory.join("portable").exists() && zip.exists());
    drop(guard);
    fixture.run().unwrap();
    assert!(!zip.exists() && backup.exists());
}

#[test]
#[cfg(windows)]
fn packages_referenced_by_failed_or_scheduled_updates_are_retained() {
    let fixture = Fixture::new();
    let (_, _, zip) = fixture.transaction("1-200", "2.3.0", true, true);
    fixture.transaction("1-300", "2.3.0", false, false);
    fixture.run().unwrap();
    assert!(zip.exists());
    let second = Fixture::new();
    let (directory, _, zip) = second.transaction("1-200", "2.3.0", true, true);
    let plan: Plan = read_json(&directory.join("plan.json")).unwrap();
    Fixture::json(
        &second.config.join("updates/install-next.json"),
        serde_json::to_value(plan.asset).unwrap(),
    );
    second.run().unwrap();
    assert!(zip.exists());
    let newer = Fixture::new();
    let (_, _, zip) = newer.transaction("1-200", "2.3.0", true, true);
    let cache = newer.config.join("updates/download.json");
    std::fs::write(&cache, b"newer download record").unwrap();
    std::fs::File::options()
        .write(true)
        .open(cache)
        .unwrap()
        .set_times(
            std::fs::FileTimes::new()
                .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5)),
        )
        .unwrap();
    newer.run().unwrap();
    assert!(zip.exists(), "a newer download must not be retired by an older update");
}

#[test]
#[cfg(windows)]
fn unexpected_files_and_other_installations_are_never_removed() {
    let fixture = Fixture::new();
    let (old, backup, _) = fixture.transaction("1-100", "2.2.1", true, true);
    fixture.transaction("1-200", "2.3.0", true, true);
    std::fs::write(backup.join("my notes.txt"), b"keep").unwrap();
    assert!(fixture.run().is_err());
    assert!(backup.join("pebrel.exe").exists() && backup.join("my notes.txt").exists());
    // A valid-looking foreign plan must not authorize deletion in another copy.
    let foreign = Fixture::new();
    let (directory, _, zip) = foreign.transaction("1-200", "2.3.0", true, true);
    let mut plan: serde_json::Value = read_json(&directory.join("plan.json")).unwrap();
    plan["executable"] = serde_json::to_value(&fixture.executable).unwrap();
    Fixture::json(&directory.join("plan.json"), plan);
    foreign.run().unwrap();
    assert!(zip.exists() && directory.join("portable").exists());
    assert!(old.join("result.json").exists());
}

#[test]
#[cfg(windows)]
fn directory_links_cannot_redirect_cleanup() {
    use std::os::windows::process::CommandExt as _;
    let fixture = Fixture::new();
    let (directory, _, zip) = fixture.transaction("1-200", "2.3.0", true, true);
    let outside = fixture.root.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("pebrel.exe"), b"outside").unwrap();
    std::fs::remove_file(directory.join("portable/pebrel.exe")).unwrap();
    std::fs::remove_dir(directory.join("portable")).unwrap();
    // Junctions can be created without symlink elevation. Only new fixture paths
    // are supplied through environment variables, with no shell interpolation.
    let output = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command",
        "New-Item -ItemType Junction -Path $env:PEBREL_TEST_LINK -Target $env:PEBREL_TEST_TARGET | Out-Null"])
        .env("PEBREL_TEST_LINK", directory.join("portable")).env("PEBREL_TEST_TARGET", &outside).creation_flags(0x0800_0000).output().unwrap();
    assert!(output.status.success());
    assert!(fixture.run().is_err());
    assert_eq!(std::fs::read(outside.join("pebrel.exe")).unwrap(), b"outside");
    assert!(zip.exists());
}
