//! Durable download metadata. A record grants no installation authority.
use super::*;
use serde::{Deserialize, Serialize};

static WRITER: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize)]
struct Record {
    asset: UpdateAsset,
    failure: Option<String>,
    #[serde(default)]
    installation: bool,
}

fn path() -> PathBuf {
    nebula_settings::settings_dir().join("updates/download.json")
}

pub(super) fn save(asset: &UpdateAsset, status: &DownloadStatus) -> std::io::Result<()> {
    let failure = match status {
        DownloadStatus::Failed(error) | DownloadStatus::InstallFailed(error) => Some(error.clone()),
        DownloadStatus::Ready { .. } => None,
        _ => return Ok(()),
    };
    let record = Record {
        asset: asset.clone(),
        failure,
        installation: matches!(status, DownloadStatus::InstallFailed(_)),
    };
    let data = serde_json::to_vec(&record)?;
    crate::atomic_file::write(&path(), &data)
}

/// Serializes only background writers. A cancelled completion cannot survive a
/// restart, and a later generation always writes after an older writer finishes.
pub(super) fn save_job(job: &DownloadJob, status: &DownloadStatus) -> std::io::Result<()> {
    let _writer = WRITER.lock().unwrap_or_else(|error| error.into_inner());
    if !job.is_current() {
        return Ok(());
    }
    save(&job.asset, status)?;
    if !job.is_current() {
        // WRITER excludes newer writers until the stale record is removed.
        std::fs::remove_file(path())?;
    }
    Ok(())
}

pub(super) fn supersedes(result: &Path) -> bool {
    let modified = |path: &Path| std::fs::metadata(path).and_then(|file| file.modified()).ok();
    matches!((modified(&path()), modified(result)), (Some(cache), Some(result)) if cache > result)
}

pub(super) fn failure_unseen(prompt_state: &Path) -> bool {
    let failure = std::fs::metadata(path()).and_then(|file| file.modified()).ok();
    let prompted = std::fs::metadata(prompt_state).and_then(|file| file.modified()).ok();
    match (failure, prompted) {
        (Some(failure), Some(prompted)) => failure > prompted,
        (_, None) => true,
        _ => false,
    }
}

pub(super) fn load() -> Option<(UpdateAsset, DownloadStatus)> {
    let file = File::open(path()).ok()?;
    if file.metadata().ok()?.len() > 64 * 1024 {
        return None;
    }
    let record: Record = serde_json::from_reader(file.take(64 * 1024)).ok()?;
    validate_asset(&record.asset).ok()?;
    let status = if let Some(error) = record.failure {
        if record.installation {
            DownloadStatus::InstallFailed(error)
        } else {
            DownloadStatus::Failed(error)
        }
    } else {
        let (_, path) = download_paths(&record.asset).ok()?;
        match verify_file(&path, &record.asset) {
            Ok(bytes) => DownloadStatus::Ready { path, bytes },
            Err(error) => DownloadStatus::Failed(error),
        }
    };
    Some((record.asset, status))
}
