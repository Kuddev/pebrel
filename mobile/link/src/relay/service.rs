//! Linux systemd lifecycle. No shell interpolation, recursive deletion, user
//! account creation, firewall changes or package manager. systemd credentials
//! let the unprivileged dynamic user read only its three runtime inputs.

use std::{
    collections::BTreeMap,
    fs, io,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::setup;

mod process;

const UNIT: &str = "pebrel-relay.service";
const OWNED: [&str; 6] = [
    "opt/pebrel-relay/pebrel-relay",
    "etc/systemd/system/pebrel-relay.service",
    "etc/pebrel-relay/relay.json",
    "etc/pebrel-relay/access.json",
    "etc/pebrel-relay/certificate.pem",
    "etc/pebrel-relay/private-key.pem",
];
const MANIFEST: &str = "opt/pebrel-relay/installation.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    product: String,
    version: u32,
    files: BTreeMap<String, String>,
}

pub trait ServiceControl {
    fn run(&self, args: &[&str]) -> io::Result<()>;
    fn ready(&self) -> io::Result<()>;

    /// Only stable stage identifiers may leave this boundary, never command
    /// output, access credentials or a guessed completion percentage.
    fn progress(&self, _stage: ServiceStage) {}

    fn pause_before_probe(&self) {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

pub struct Systemd;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStage {
    Checking,
    Initializing,
    Installing,
    Starting,
    Verifying,
    Ready,
    Stopping,
    Stopped,
    Removing,
    Uninstalled,
}

#[derive(Serialize)]
pub struct ServiceStatus {
    pub installed: bool,
    pub running: bool,
    pub ready: bool,
    pub configuration_retained: bool,
}

pub fn status(control: &impl ServiceControl) -> ServiceStatus {
    let root = Path::new("/");
    let owned = manifest(root).is_ok();
    let installed = owned && root.join(OWNED[0]).is_file() && root.join(OWNED[1]).is_file();
    let running = installed && control.run(&["is-active", "--quiet", UNIT]).is_ok();
    ServiceStatus {
        installed,
        running,
        ready: running && control.ready().is_ok(),
        configuration_retained: owned && root.join(OWNED[2]).is_file(),
    }
}

pub fn change_running(start: bool, control: &impl ServiceControl) -> io::Result<()> {
    let root = Path::new("/");
    let record = manifest(root)?;
    verify(root, &record, &OWNED, false)?;
    control.progress(if start { ServiceStage::Starting } else { ServiceStage::Stopping });
    control.run(&[if start { "start" } else { "stop" }, UNIT])?;
    if start {
        wait_until_ready(control)?;
    } else {
        control.progress(ServiceStage::Stopped);
    }
    Ok(())
}

impl ServiceControl for Systemd {
    fn run(&self, args: &[&str]) -> io::Result<()> {
        if args == ["--version"] {
            let output = process::run("/usr/bin/systemctl", &["--version"], true)?;
            let version = std::str::from_utf8(&output)
                .ok()
                .and_then(|text| text.split_whitespace().nth(1))
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(0);
            return if version >= 247 {
                Ok(())
            } else {
                Err(io::Error::other("systemd_247_required"))
            };
        }
        process::run("/usr/bin/systemctl", args, false).map(|_| ())
    }
    fn ready(&self) -> io::Result<()> {
        process::run(
            "/opt/pebrel-relay/pebrel-relay",
            &["probe", "--config", "/etc/pebrel-relay/relay.json"],
            false,
        )
        .map(|_| ())
    }
    fn progress(&self, stage: ServiceStage) {
        // A broken SSH progress pipe must not panic halfway through installation.
        // Completion/failure is still determined by the command exit status.
        use io::Write;
        let _ = writeln!(
            io::stdout().lock(),
            "{}",
            serde_json::json!({"event": "progress", "stage": stage})
        );
    }
}

fn wait_until_ready(control: &impl ServiceControl) -> io::Result<()> {
    control.progress(ServiceStage::Verifying);
    // Type=simple signals process creation, not listener readiness. Each probe
    // has its own deadline; retries are bounded and are not a background daemon.
    let started = std::time::Instant::now();
    for attempt in 0..10 {
        if started.elapsed() >= std::time::Duration::from_secs(30) {
            break;
        }
        if attempt > 0 {
            control.pause_before_probe();
        }
        if control.ready().is_ok() {
            control.progress(ServiceStage::Ready);
            return Ok(());
        }
    }
    Err(io::Error::other("service_not_ready"))
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn safe(root: &Path, relative: &str) -> io::Result<PathBuf> {
    if relative != MANIFEST && !OWNED.contains(&relative) {
        return Err(io::Error::other("unowned_path"));
    }
    let path = root.join(relative);
    for ancestor in path.ancestors() {
        if fs::symlink_metadata(ancestor).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(io::Error::other("symlink_installation_path"));
        }
    }
    Ok(path)
}

fn manifest(root: &Path) -> io::Result<Manifest> {
    let path = safe(root, MANIFEST)?;
    let value: Manifest = serde_json::from_slice(&setup::read_bounded(&path, 8192)?)
        .map_err(|_| io::Error::other("invalid_ownership_manifest"))?;
    if value.product != "pebrel-relay"
        || value.version != 1
        || value.files.len() != OWNED.len()
        || value.files.keys().any(|path| !OWNED.contains(&path.as_str()))
    {
        return Err(io::Error::other("invalid_ownership_manifest"));
    }
    Ok(value)
}

fn verify(
    root: &Path,
    record: &Manifest,
    selected: &[&str],
    allow_missing: bool,
) -> io::Result<()> {
    for relative in selected {
        let path = safe(root, relative)?;
        match setup::read_bounded(&path, 64 * 1024 * 1024) {
            Ok(bytes) if record.files.get(*relative) == Some(&digest(&bytes)) => {},
            Err(error) if allow_missing && error.kind() == io::ErrorKind::NotFound => {},
            _ => return Err(io::Error::other("managed_file_changed")),
        }
    }
    Ok(())
}

pub fn unit() -> &'static str {
    "[Unit]\nDescription=Pebrel encrypted mobile relay\nAfter=network-online.target\nWants=network-online.target\n\n\
[Service]\nType=simple\nDynamicUser=yes\nUMask=0077\n\
LoadCredential=relay.json:/etc/pebrel-relay/relay.json\n\
LoadCredential=certificate.pem:/etc/pebrel-relay/certificate.pem\n\
LoadCredential=private-key.pem:/etc/pebrel-relay/private-key.pem\n\
ExecStart=/opt/pebrel-relay/pebrel-relay serve --config %d/relay.json\n\
Restart=on-failure\nRestartSec=3\nTimeoutStopSec=10\n\
NoNewPrivileges=yes\nProtectSystem=strict\nProtectHome=yes\nPrivateTmp=yes\nPrivateDevices=yes\n\
ProtectKernelTunables=yes\nProtectKernelModules=yes\nProtectControlGroups=yes\nRestrictSUIDSGID=yes\n\
CapabilityBoundingSet=CAP_NET_BIND_SERVICE\nAmbientCapabilities=CAP_NET_BIND_SERVICE\n\
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX\nLimitNOFILE=1024\nTasksMax=64\n\
LogRateLimitIntervalSec=30s\nLogRateLimitBurst=20\n\n[Install]\nWantedBy=multi-user.target\n"
}

/// `expected_sha256` must come from the app's verified release manifest, not
/// from the same unauthenticated download as the executable.
pub fn install(
    source: &Path,
    expected_sha256: &str,
    address: &str,
    listen: SocketAddr,
    control: &impl ServiceControl,
) -> io::Result<()> {
    install_under(Path::new("/"), source, expected_sha256, address, listen, control)
}

fn install_under(
    root: &Path,
    source: &Path,
    expected_sha256: &str,
    address: &str,
    listen: SocketAddr,
    control: &impl ServiceControl,
) -> io::Result<()> {
    control.progress(ServiceStage::Checking);
    let bytes = setup::read_bounded(source, 64 * 1024 * 1024)?;
    if bytes.is_empty() || bytes.len() > 64 * 1024 * 1024 || digest(&bytes) != expected_sha256 {
        return Err(io::Error::other("binary_integrity_failed"));
    }
    let executable = safe(root, OWNED[0])?;
    let service = safe(root, OWNED[1])?;
    let record_path = safe(root, MANIFEST)?;
    if record_path.exists() {
        let previous = manifest(root)?;
        verify(root, &previous, &OWNED[2..], false)?;
        verify(root, &previous, &OWNED[..2], true)?;
        // A repeated install is idempotent, not an implicit replacement/update.
        if previous.files[OWNED[0]] != expected_sha256 {
            return Err(io::Error::other("explicit_update_required"));
        }
        control.progress(ServiceStage::Installing);
        if !executable.exists() {
            setup::write_new_private(&executable, &bytes)?;
            executable_permissions(&executable)?;
        }
        if !service.exists() {
            setup::write_new_private(&service, unit().as_bytes())?;
        }
        control.run(&["daemon-reload"])?;
        control.progress(ServiceStage::Starting);
        control.run(&["enable", "--now", UNIT])?;
        return wait_until_ready(control);
    }
    for relative in OWNED {
        if safe(root, relative)?.exists() {
            return Err(io::Error::other("installation_conflict"));
        }
    }
    control.run(&["--version"])?;
    let config_dir = root.join("etc/pebrel-relay");
    if config_dir.exists() && fs::read_dir(&config_dir)?.next().is_some() {
        return Err(io::Error::other("configuration_directory_not_empty"));
    }
    fs::create_dir_all(executable.parent().unwrap())?;
    fs::create_dir_all(service.parent().unwrap())?;
    fs::create_dir_all(&config_dir)?;
    control.progress(ServiceStage::Initializing);
    setup::initialize(&config_dir, address, listen)?;
    control.progress(ServiceStage::Installing);
    setup::write_new_private(&executable, &bytes)?;
    executable_permissions(&executable)?;
    setup::write_new_private(&service, unit().as_bytes())?;
    let mut record =
        Manifest { product: "pebrel-relay".into(), version: 1, files: BTreeMap::new() };
    for relative in OWNED {
        record.files.insert(relative.into(), digest(&fs::read(safe(root, relative)?)?));
    }
    setup::write_new_private(
        &record_path,
        &serde_json::to_vec(&record).map_err(io::Error::other)?,
    )?;
    control.run(&["daemon-reload"])?;
    control.progress(ServiceStage::Starting);
    control.run(&["enable", "--now", UNIT])?;
    wait_until_ready(control)
}

fn executable_permissions(_path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(_path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

pub fn uninstall(purge: bool, control: &impl ServiceControl) -> io::Result<()> {
    uninstall_under(Path::new("/"), purge, control)
}

fn uninstall_under(root: &Path, purge: bool, control: &impl ServiceControl) -> io::Result<()> {
    control.progress(ServiceStage::Checking);
    let record = manifest(root)?;
    let targets = if purge { &OWNED[..] } else { &OWNED[..2] };
    // Verify every selected file before changing anything, including stop state.
    verify(root, &record, targets, true)?;
    if safe(root, OWNED[1])?.exists() {
        control.progress(ServiceStage::Stopping);
        control.run(&["disable", "--now", UNIT])?;
    }
    control.progress(ServiceStage::Removing);
    for relative in targets {
        match fs::remove_file(safe(root, relative)?) {
            Ok(()) => {},
            Err(error) if error.kind() == io::ErrorKind::NotFound => {},
            Err(error) => return Err(error),
        }
    }
    control.run(&["daemon-reload"])?;
    if purge {
        fs::remove_file(safe(root, MANIFEST)?)?;
        // Empty directories only. Unknown files are preserved, never recursed.
        let _ = fs::remove_dir(root.join("etc/pebrel-relay"));
        let _ = fs::remove_dir(root.join("opt/pebrel-relay"));
    }
    control.progress(ServiceStage::Uninstalled);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Default)]
    struct FakeControl(Mutex<Vec<Vec<String>>>);
    impl ServiceControl for FakeControl {
        fn run(&self, args: &[&str]) -> io::Result<()> {
            self.0.lock().unwrap().push(args.iter().map(|v| v.to_string()).collect());
            Ok(())
        }
        fn ready(&self) -> io::Result<()> {
            Ok(())
        }
    }

    fn fixture() -> (PathBuf, PathBuf) {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp/link-tests");
        fs::create_dir_all(&fixtures).unwrap();
        let root = fixtures.join(format!(
            "pebrel-owned-service-{}",
            crate::identity::Secret::generate().unwrap().hash()
        ));
        fs::create_dir(&root).unwrap();
        let binary = root.join("download");
        setup::write_new_private(&binary, b"fixture-executable").unwrap();
        (root, binary)
    }

    #[test]
    fn install_is_idempotent_and_uninstall_preserves_configuration_and_unknown_files() {
        let (root, binary) = fixture();
        let control = FakeControl::default();
        for _ in 0..2 {
            install_under(
                &root,
                &binary,
                &digest(b"fixture-executable"),
                "localhost",
                "0.0.0.0:443".parse().unwrap(),
                &control,
            )
            .unwrap();
        }
        let config = fs::read(root.join(OWNED[2])).unwrap();
        let unrelated = root.join("etc/pebrel-relay/user-note");
        fs::write(&unrelated, b"user content").unwrap();
        uninstall_under(&root, false, &control).unwrap();
        assert_eq!(fs::read(root.join(OWNED[2])).unwrap(), config);
        assert!(!root.join(OWNED[0]).exists());
        install_under(
            &root,
            &binary,
            &digest(b"fixture-executable"),
            "localhost",
            "0.0.0.0:443".parse().unwrap(),
            &control,
        )
        .unwrap();
        assert_eq!(fs::read(root.join(OWNED[2])).unwrap(), config);
        uninstall_under(&root, true, &control).unwrap();
        assert_eq!(fs::read(&unrelated).unwrap(), b"user content");
        assert!(!root.join(OWNED[2]).exists());
        // Fixture cleanup only, after validating the independently owned root.
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("pebrel-owned-service-"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn integrity_conflicts_and_modified_units_never_overwrite_or_delete() {
        let (root, binary) = fixture();
        let control = FakeControl::default();
        assert!(
            install_under(
                &root,
                &binary,
                "wrong",
                "localhost",
                "0.0.0.0:443".parse().unwrap(),
                &control
            )
            .is_err()
        );
        assert!(control.0.lock().unwrap().is_empty());
        install_under(
            &root,
            &binary,
            &digest(b"fixture-executable"),
            "localhost",
            "0.0.0.0:443".parse().unwrap(),
            &control,
        )
        .unwrap();
        fs::write(root.join(OWNED[1]), b"user-managed replacement").unwrap();
        let count = control.0.lock().unwrap().len();
        assert!(uninstall_under(&root, true, &control).is_err());
        assert_eq!(control.0.lock().unwrap().len(), count);
        assert_eq!(fs::read(root.join(OWNED[1])).unwrap(), b"user-managed replacement");
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("pebrel-owned-service-"));
        fs::remove_dir_all(root).unwrap();
    }

    struct StartingControl {
        probes: AtomicUsize,
        fail_probes: usize,
        stages: Mutex<Vec<ServiceStage>>,
    }

    impl StartingControl {
        fn new(fail_probes: usize) -> Self {
            Self { probes: AtomicUsize::new(0), fail_probes, stages: Mutex::default() }
        }
    }

    impl ServiceControl for StartingControl {
        fn run(&self, _args: &[&str]) -> io::Result<()> {
            Ok(())
        }
        fn ready(&self) -> io::Result<()> {
            if self.probes.fetch_add(1, Ordering::SeqCst) < self.fail_probes {
                Err(io::Error::other("not_listening_yet"))
            } else {
                Ok(())
            }
        }
        fn progress(&self, stage: ServiceStage) {
            self.stages.lock().unwrap().push(stage);
        }
        fn pause_before_probe(&self) {}
    }

    #[test]
    fn slow_start_has_real_stages_and_never_reports_ready_early() {
        let (root, binary) = fixture();
        let control = StartingControl::new(2);
        install_under(
            &root,
            &binary,
            &digest(b"fixture-executable"),
            "localhost",
            "0.0.0.0:443".parse().unwrap(),
            &control,
        )
        .unwrap();
        assert_eq!(control.probes.load(Ordering::SeqCst), 3);
        assert_eq!(
            *control.stages.lock().unwrap(),
            [
                ServiceStage::Checking,
                ServiceStage::Initializing,
                ServiceStage::Installing,
                ServiceStage::Starting,
                ServiceStage::Verifying,
                ServiceStage::Ready,
            ]
        );
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("pebrel-owned-service-"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn readiness_failure_is_bounded_and_retry_keeps_existing_keys() {
        let (root, binary) = fixture();
        let control = StartingControl::new(usize::MAX);
        let error = install_under(
            &root,
            &binary,
            &digest(b"fixture-executable"),
            "localhost",
            "0.0.0.0:443".parse().unwrap(),
            &control,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "service_not_ready");
        assert_eq!(control.probes.load(Ordering::SeqCst), 10);
        assert!(!control.stages.lock().unwrap().contains(&ServiceStage::Ready));
        let access = fs::read(root.join(OWNED[3])).unwrap();
        let key = fs::read(root.join(OWNED[5])).unwrap();
        let recovered = StartingControl::new(0);
        install_under(
            &root,
            &binary,
            &digest(b"fixture-executable"),
            "localhost",
            "0.0.0.0:443".parse().unwrap(),
            &recovered,
        )
        .unwrap();
        assert_eq!(access, fs::read(root.join(OWNED[3])).unwrap());
        assert_eq!(key, fs::read(root.join(OWNED[5])).unwrap());
        assert!(!recovered.stages.lock().unwrap().contains(&ServiceStage::Initializing));
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("pebrel-owned-service-"));
        fs::remove_dir_all(root).unwrap();
    }
}
