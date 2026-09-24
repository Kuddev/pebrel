//! Native acceptance uses disposable signed apps, never the user's installation.
use super::*;
use std::fs;

struct Fixture {
    _root: tempfile::TempDir,
    plan: Plan,
    directory: PathBuf,
    old: Child,
    helper: Option<Child>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(child) = &mut self.helper {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = self.old.kill();
        let _ = self.old.wait();
        // Only processes executing this disposable fixture path are ours.
        for pid in native::running_copies(&self.plan.executable).unwrap_or_default() {
            unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        }
    }
}

fn app(path: &Path, version: &str, fails: bool) {
    fs::create_dir_all(path.join("Contents/MacOS")).unwrap();
    let source = path.parent().unwrap().join("fixture.c");
    fs::write(
        &source,
        format!(
            r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
int main(int argc, char **argv) {{
    if (argc > 1 && strcmp(argv[1], "--version") == 0) {{
        puts("pebrel {version}"); return 0;
    }}
    char path[4096];
    snprintf(path, sizeof(path), "%s/launched.txt", getenv("PEBREL_CONFIG_DIR"));
    FILE *file = fopen(path, "a");
    if (file) {{ fprintf(file, "{version}\n"); fclose(file); }}
    if ({fails}) return 42;
    sleep(600); return 0;
}}
"#,
            fails = u8::from(fails)
        ),
    )
    .unwrap();
    native::run(
        Command::new("/usr/bin/cc").arg(&source).arg("-o").arg(path.join("Contents/MacOS/pebrel")),
    )
    .unwrap();
    fs::remove_file(source).unwrap();
    fs::write(path.join("Contents/Info.plist"), format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>io.github.kuddev.pebrel</string>
<key>CFBundleExecutable</key><string>pebrel</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>{version}</string>
</dict></plist>"#)).unwrap();
    native::run(Command::new("/usr/bin/codesign").args(["--force", "--sign", "-"]).arg(path))
        .unwrap();
}

impl Fixture {
    fn new(binary: &Path, fails: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = fs::canonicalize(root.path()).unwrap();
        let installation = path.join("Pebrel.app");
        let config = path.join("config");
        let transaction = "native-acceptance".to_owned();
        let directory = config.join("updates/handoffs").join(&transaction);
        fs::create_dir_all(&directory).unwrap();
        let version = "99.0.0";
        app(&installation, env!("CARGO_PKG_VERSION"), false);
        let source = path.join("image");
        fs::create_dir(&source).unwrap();
        app(&source.join("Pebrel.app"), version, fails);
        let name = crate::update_check::assets::native_names(version).remove(0);
        let installer = config.join("updates").join(&name);
        native::run(
            Command::new("/usr/bin/hdiutil")
                .args([
                    "create",
                    "-quiet",
                    "-fs",
                    "HFS+",
                    "-volname",
                    "PebrelUpdateTest",
                    "-srcfolder",
                ])
                .arg(&source)
                .args(["-format", "UDZO"])
                .arg(&installer),
        )
        .unwrap();
        let hash: String =
            file_digest(&installer).unwrap().iter().map(|b| format!("{b:02x}")).collect();
        let executable = installation.join("Contents/MacOS/pebrel");
        let old = Command::new(&executable).env("PEBREL_CONFIG_DIR", &config).spawn().unwrap();
        let created = native::process_created(old.id()).unwrap().unwrap();
        let plan = Plan {
            schema: 1,
            asset: UpdateAsset {
                version: version.into(),
                name: name.clone(),
                download_url: format!(
                    "https://github.com/Kuddev/pebrel/releases/download/v{version}/{name}"
                ),
                size: Some(fs::metadata(&installer).unwrap().len()),
                sha256: Some(hash.clone()),
            },
            transaction,
            guard_path: guard_base(&executable).with_extension("nebula-lock"),
            executable,
            installation,
            config_directory: config,
            bytes: fs::metadata(&installer).unwrap().len(),
            installer,
            sha256: hash,
            version: version.into(),
            original_version: env!("CARGO_PKG_VERSION").into(),
            participants: vec![Participant { pid: old.id(), created: created.to_string() }],
        };
        fs::copy(binary, directory.join("handoff")).unwrap();
        Self { _root: root, plan, directory, old, helper: None }
    }

    fn start(&mut self) {
        let path = self.directory.join("plan.json");
        fs::write(&path, serde_json::to_vec(&self.plan).unwrap()).unwrap();
        let log = fs::File::create(self.directory.join("helper.log")).unwrap();
        self.helper = Some(
            Command::new(self.directory.join("handoff"))
                .arg("--internal-macos-update")
                .arg(path)
                .env("PEBREL_CONFIG_DIR", &self.plan.config_directory)
                .env("NEBULA_CONFIG_DIR", &self.plan.config_directory)
                .stdout(Stdio::null())
                .stderr(log)
                .spawn()
                .unwrap(),
        );
    }

    fn ready(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !self.directory.join("ready.json").exists() {
            assert!(self.helper.as_mut().unwrap().try_wait().unwrap().is_none(), "{}", self.log());
            assert!(Instant::now() < deadline, "helper did not prepare: {}", self.log());
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(
            native::property(&self.plan.installation, "CFBundleShortVersionString").unwrap(),
            self.plan.original_version
        );
        assert!(
            crate::atomic_file::try_lifetime_lock(&guard_base(&self.plan.executable))
                .unwrap()
                .is_none()
        );
    }

    fn commit(&mut self) {
        fs::write(self.directory.join("workspace.json"), b"[]").unwrap();
        write_value(
            &self.directory.join("commit.json"),
            &serde_json::json!({"transaction": self.plan.transaction}),
        )
        .unwrap();
        self.old.kill().unwrap();
        self.old.wait().unwrap();
    }

    fn finish(&mut self, success: bool) -> serde_json::Value {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(status) = self.helper.as_mut().unwrap().try_wait().unwrap() {
                assert_eq!(status.success(), success, "{}", self.log());
                break;
            }
            assert!(Instant::now() < deadline, "helper timed out: {}", self.log());
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!self.directory.join("mount").exists(), "image must be detached");
        read_json(&self.directory.join("result.json")).unwrap()
    }

    fn log(&self) -> String {
        fs::read_to_string(self.directory.join("helper.log")).unwrap_or_default()
    }
}

#[test]
#[ignore = "needs macOS signing/mount permissions and PEBREL_MACOS_TEST_EXECUTABLE from a fresh product build"]
fn native_macos_update_installs_cancels_rejects_and_rolls_back() {
    let binary = PathBuf::from(
        std::env::var_os("PEBREL_MACOS_TEST_EXECUTABLE").expect("fresh product executable"),
    );
    for case in ["success", "rollback", "cancel", "digest", "other-instance"] {
        let mut fixture = Fixture::new(&binary, case == "rollback");
        if case == "digest" {
            fixture.plan.sha256 = "0".repeat(64);
            fixture.plan.asset.sha256 = Some(fixture.plan.sha256.clone());
        }
        let mut other = (case == "other-instance").then(|| {
            Command::new(&fixture.plan.executable)
                .env("PEBREL_CONFIG_DIR", &fixture.plan.config_directory)
                .spawn()
                .unwrap()
        });
        fixture.start();
        if case == "digest" || case == "other-instance" {
            let result = fixture.finish(false);
            assert_eq!(result["success"], false);
            assert!(!fixture.directory.join("ready.json").exists());
        } else {
            fixture.ready();
            if case == "cancel" {
                fs::write(fixture.directory.join("cancel.json"), b"{}").unwrap();
            } else {
                fixture.commit();
            }
            let result = fixture.finish(case == "success");
            assert_eq!(result["success"], case == "success");
            assert_eq!(result["recovered_original"], case == "rollback");
        }
        if let Some(child) = &mut other {
            let _ = child.kill();
            let _ = child.wait();
        }
        let expected = if case == "success" { "99.0.0" } else { env!("CARGO_PKG_VERSION") };
        assert_eq!(
            native::property(&fixture.plan.installation, "CFBundleShortVersionString").unwrap(),
            expected,
            "{case}"
        );
        if case == "success" {
            let backup = fixture
                .plan
                .installation
                .parent()
                .unwrap()
                .join(".pebrel-update-native-acceptance.app");
            native::verify_bundle(&backup, env!("CARGO_PKG_VERSION")).unwrap();
            assert!(
                fs::read_to_string(fixture.plan.config_directory.join("launched.txt"))
                    .unwrap()
                    .contains("99.0.0")
            );
        }
        println!("native macOS handoff: {case} passed");
    }
}
