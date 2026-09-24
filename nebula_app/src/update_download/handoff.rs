//! Two-phase platform installation handoff and the resulting restore ticket.
//! Files under one transaction are durable evidence; only commit authorizes setup.
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::platform::update_installation::{canonical, current_process_created};
use crate::session::Session;
use crate::update_check::UpdateAsset;

#[cfg(target_os = "macos")]
pub(crate) mod macos;

struct ActiveRestore {
    directory: PathBuf,
    expected_windows: usize,
}

static ACTIVE_RESTORE: std::sync::Mutex<Option<ActiveRestore>> = std::sync::Mutex::new(None);

#[derive(Serialize, Deserialize)]
struct Participant {
    pid: u32,
    created: String,
}

#[derive(Serialize, Deserialize)]
struct Plan {
    schema: u32,
    asset: UpdateAsset,
    transaction: String,
    executable: PathBuf,
    installation: PathBuf,
    config_directory: PathBuf,
    installer: PathBuf,
    sha256: String,
    bytes: u64,
    version: String,
    original_version: String,
    guard_path: PathBuf,
    participants: Vec<Participant>,
}

pub(crate) struct PreparedUpdate {
    directory: PathBuf,
    transaction: String,
    child: Child,
    committed: bool,
}

impl PreparedUpdate {
    pub(crate) fn commit(&mut self, windows: &[Session]) -> Result<(), String> {
        let snapshot = serde_json::to_vec(windows).map_err(|error| error.to_string())?;
        crate::atomic_file::write(&self.directory.join("workspace.json"), &snapshot)
            .map_err(|error| format!("Could not preserve the update workspace: {error}"))?;
        if self.child.try_wait().map_err(|error| error.to_string())?.is_some() {
            return Err("The update helper exited before commit".into());
        }
        let commit = serde_json::to_vec(&serde_json::json!({ "transaction": self.transaction }))
            .map_err(|error| error.to_string())?;
        crate::atomic_file::write(&self.directory.join("commit.json"), &commit)
            .map_err(|error| format!("Could not commit the update: {error}"))?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for PreparedUpdate {
    fn drop(&mut self) {
        if !self.committed {
            let _ = crate::atomic_file::write(&self.directory.join("cancel.json"), b"{}");
            // This is our own uncommitted helper; it has no authority to install.
            if crate::platform::Platform::current() == crate::platform::Platform::MacOS {
                return;
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn guard_base(executable: &Path) -> PathBuf {
    // All configurations of the same installed binary share this lock. A
    // settings-directory override must not bypass an installation in progress.
    // The installer never replaces or deletes this reserved sidecar.
    crate::platform::update_installation::guard_base(executable)
}

/// Called before starting any resident resources. The lock belongs to the exact
/// installation; a copied test/portable application has a different identity.
pub(crate) fn installation_in_progress() -> io::Result<bool> {
    let executable = canonical(&std::env::current_exe()?)?;
    Ok(crate::atomic_file::try_lifetime_lock(&guard_base(&executable))?.is_none())
}

/// All verification and process waiting occurs on a background executor.
pub(crate) fn prepare(asset: &UpdateAsset) -> Result<PreparedUpdate, String> {
    if crate::platform::elevation::requires_isolation() {
        return Err(
            "Install updates from an ordinary Pebrel window so privileged sessions stay isolated"
                .into(),
        );
    }
    if !crate::platform::CAPABILITIES.self_update_install {
        return Err("In-app installation is unavailable on this platform".into());
    }
    let installer = super::ready_path(asset)?;
    let executable = canonical(&std::env::current_exe().map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    let installation = crate::platform::update_installation::installation_directory(&executable)?;
    let transaction = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos()
    );
    let directory = nebula_settings::settings_dir().join("updates/handoffs").join(&transaction);
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    // Detect an unwritable target while all windows are still available.
    let probe_directory =
        if crate::platform::Platform::current() == crate::platform::Platform::MacOS {
            installation.parent().ok_or("Missing application parent")?
        } else {
            &installation
        };
    let probe = probe_directory.join(format!(".pebrel-update-{transaction}"));
    let probe_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|error| format!("Installation directory is not writable: {error}"))?;
    drop(probe_file);
    std::fs::remove_file(probe).map_err(|error| error.to_string())?;
    let created = current_process_created().map_err(|error| error.to_string())?;
    let base = guard_base(&executable);
    let plan = Plan {
        schema: 1,
        asset: asset.clone(),
        transaction: transaction.clone(),
        installation,
        executable,
        config_directory: canonical(&nebula_settings::settings_dir())
            .map_err(|error| error.to_string())?,
        installer: canonical(&installer).map_err(|error| error.to_string())?,
        sha256: asset.sha256.clone().ok_or("Missing package digest")?,
        bytes: std::fs::metadata(installer).map_err(|error| error.to_string())?.len(),
        version: asset.version.clone(),
        original_version: env!("CARGO_PKG_VERSION").into(),
        guard_path: base.with_extension("nebula-lock"),
        participants: vec![Participant { pid: std::process::id(), created: created.to_string() }],
    };
    let plan_path = directory.join("plan.json");
    crate::atomic_file::write(
        &plan_path,
        &serde_json::to_vec(&plan).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    crate::atomic_file::write(
        &nebula_settings::settings_dir().join("updates/last-handoff.json"),
        &serde_json::to_vec(&plan_path).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let child =
        crate::platform::update_installation::spawn_prepared_helper(&directory, &plan_path)?;
    let mut prepared = PreparedUpdate { directory, transaction, child, committed: false };
    let timeout = if crate::platform::Platform::current() == crate::platform::Platform::MacOS {
        600
    } else {
        30
    };
    let deadline = Instant::now() + Duration::from_secs(timeout);
    loop {
        if prepared.child.try_wait().map_err(|error| error.to_string())?.is_some() {
            let result = read_json::<serde_json::Value>(&prepared.directory.join("result.json"))
                .and_then(|result| result["error"].as_str().map(str::to_owned));
            return Err(
                result.unwrap_or_else(|| "The update helper could not prepare installation".into())
            );
        }
        if let Some(ready) = read_json::<serde_json::Value>(&prepared.directory.join("ready.json"))
            && ready["transaction"] == prepared.transaction
        {
            return Ok(prepared);
        }
        if Instant::now() >= deadline {
            return Err("The update helper did not become ready".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > 16 * 1024 * 1024 {
        return None;
    }
    serde_json::from_reader(file.take(16 * 1024 * 1024)).ok()
}

/// A ticket restores windows once, independently of the ordinary startup setting.
/// Keep its original snapshots on disk even after acknowledgement.
pub(crate) fn restore_ticket() -> Option<Vec<Session>> {
    let path = std::env::var_os("PEBREL_UPDATE_RESTORE").map(PathBuf::from).or_else(|| {
        read_json(&nebula_settings::settings_dir().join("updates/last-handoff.json"))
    })?;
    let path = canonical(&path).ok()?;
    let root = canonical(&nebula_settings::settings_dir().join("updates/handoffs")).ok()?;
    if !path.starts_with(root) || path.file_name()? != "plan.json" {
        return None;
    }
    let plan: Plan = read_json(&path)?;
    if plan.schema != 1
        || canonical(&plan.executable).ok()? != canonical(&std::env::current_exe().ok()?).ok()?
    {
        return None;
    }
    let directory = path.parent()?;
    let result: serde_json::Value = read_json(&directory.join("result.json"))?;
    let current_version = env!("CARGO_PKG_VERSION");
    let succeeded = result["success"] == true && plan.version == current_version;
    let recovered_original = result["success"] == false
        && result["recovered_original"] == true
        && plan.original_version == current_version;
    if (!succeeded && !recovered_original) || result["transaction"] != plan.transaction {
        return None;
    }
    let windows: Vec<Session> = read_json(&directory.join("workspace.json"))?;
    if windows.is_empty() || windows.len() > 32 || windows.iter().any(|session| session.version > 4)
    {
        return None;
    }
    let count: u32 = read_json(&directory.join("restore-attempts.json")).unwrap_or(0);
    if count >= 3 || directory.join("restored.json").exists() {
        return None;
    }
    crate::atomic_file::write(
        &directory.join("restore-attempts.json"),
        (count + 1).to_string().as_bytes(),
    )
    .ok()?;
    *ACTIVE_RESTORE.lock().unwrap_or_else(|error| error.into_inner()) =
        Some(ActiveRestore { directory: directory.to_owned(), expected_windows: windows.len() });
    Some(windows)
}

pub(crate) fn acknowledge_restore(restored_windows: usize) {
    let mut active = ACTIVE_RESTORE.lock().unwrap_or_else(|error| error.into_inner());
    let Some(ticket) = active.as_ref() else {
        return;
    };
    if restored_windows != ticket.expected_windows {
        return;
    }
    if let Err(error) = crate::atomic_file::write(&ticket.directory.join("restored.json"), b"{}") {
        log::warn!("Could not acknowledge update recovery: {error}");
    } else {
        *active = None;
    }
}

pub(super) fn failed_update() -> Option<(UpdateAsset, String)> {
    let path: PathBuf =
        read_json(&nebula_settings::settings_dir().join("updates/last-handoff.json"))?;
    let root = canonical(&nebula_settings::settings_dir().join("updates/handoffs")).ok()?;
    let path = canonical(&path).ok()?;
    if !path.starts_with(root) {
        return None;
    }
    let plan: Plan = read_json(&path)?;
    if canonical(&plan.executable).ok()? != canonical(&std::env::current_exe().ok()?).ok()? {
        return None;
    }
    let result_path = path.parent()?.join("result.json");
    if super::cache::supersedes(&result_path) {
        return None;
    }
    let result: serde_json::Value = read_json(&result_path)?;
    if result["success"] != false || result["transaction"] != plan.transaction {
        return None;
    }
    super::validate_asset(&plan.asset).ok()?;
    Some((plan.asset, result["error"].as_str()?.to_owned()))
}

pub(super) fn failure_unseen(prompt_state: &Path) -> bool {
    if failed_update().is_none() {
        return false;
    }
    let Some(path) =
        read_json::<PathBuf>(&nebula_settings::settings_dir().join("updates/last-handoff.json"))
    else {
        return false;
    };
    let Some(directory) = path.parent() else { return false };
    let failure =
        std::fs::metadata(directory.join("result.json")).and_then(|file| file.modified()).ok();
    let prompted = std::fs::metadata(prompt_state).and_then(|file| file.modified()).ok();
    match (failure, prompted) {
        (Some(failure), Some(prompted)) => failure > prompted,
        (Some(_), None) => true,
        _ => false,
    }
}

pub(crate) fn schedule(asset: &UpdateAsset) -> Result<(), String> {
    if crate::platform::elevation::requires_isolation() {
        return Err("Schedule updates from an ordinary Pebrel window".into());
    }
    super::validate_asset(asset)?;
    if !matches!(super::status(asset), super::DownloadStatus::Ready { .. }) {
        return Err("The package is not ready to schedule".into());
    }
    let data = serde_json::to_vec(asset).map_err(|error| error.to_string())?;
    crate::atomic_file::write(
        &nebula_settings::settings_dir().join("updates/install-next.json"),
        &data,
    )
    .map_err(|error| error.to_string())
}

/// Before resident resources/windows exist, apply an explicitly armed update.
/// A failed attempt is disarmed and normal startup remains available.
pub(crate) fn apply_scheduled() -> bool {
    let path = nebula_settings::settings_dir().join("updates/install-next.json");
    let Some(asset) = read_json::<UpdateAsset>(&path) else {
        return false;
    };
    if crate::atomic_file::write(&path, b"null").is_err() {
        return false;
    }
    if !crate::update_check::can_install_version(&asset.version).unwrap_or(false) {
        return false;
    }
    super::hydrate();
    let result = prepare(&asset).and_then(|mut prepared| {
        let windows = crate::session::load_update_windows().map_err(|error| error.to_string())?;
        prepared.commit(&windows)
    });
    match result {
        Ok(()) => true,
        Err(error) => {
            log::warn!("Scheduled update failed; opening the application: {error}");
            let _ = super::cache::save(&asset, &super::DownloadStatus::InstallFailed(error));
            false
        },
    }
}

#[cfg(test)]
mod tests;
