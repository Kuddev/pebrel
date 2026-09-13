//! GPUI 更新安装包的下载、校验与启动。
//!
//! 更新检查只负责提供 release 元数据；本模块再次收紧资产合同，并把大文件
//! 流式写入同目录 `.part` 文件。只有长度、PE 文件头与 SHA-256 全部通过后，
//! 才原子替换为可启动的安装包，避免中断下载或错误响应变成可执行文件。

use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use sha2::{Digest as _, Sha256};

use crate::i18n::UiLanguage;
use crate::update_check::UpdateAsset;

const RELEASE_DOWNLOAD_PREFIX: &str = "https://github.com/Kuddev/pebrel/releases/download/";
const LEGACY_RELEASE_DOWNLOAD_PREFIX: &str = "https://github.com/Kuddev/nebula/releases/download/";
const MAX_INSTALLER_BYTES: u64 = 512 * 1024 * 1024;
const DOWNLOAD_CHUNK_BYTES: usize = 64 * 1024;

static DOWNLOAD_SESSION: Mutex<Option<DownloadSession>> = Mutex::new(None);

#[derive(Clone, Debug)]
pub(crate) enum DownloadStatus {
    Idle,
    Downloading { downloaded: u64, total: Option<u64> },
    Ready { path: PathBuf, bytes: u64 },
    Failed(String),
}

impl DownloadStatus {
    pub(crate) fn is_terminal(&self) -> bool {
        matches!(self, Self::Ready { .. } | Self::Failed(_))
    }
}

#[derive(Clone, Debug)]
struct DownloadSession {
    asset: UpdateAsset,
    status: DownloadStatus,
}

fn session() -> MutexGuard<'static, Option<DownloadSession>> {
    DOWNLOAD_SESSION.lock().unwrap_or_else(|poison| poison.into_inner())
}

pub(crate) fn status(asset: &UpdateAsset) -> DownloadStatus {
    session()
        .as_ref()
        .filter(|current| current.asset == *asset)
        .map(|current| current.status.clone())
        .unwrap_or(DownloadStatus::Idle)
}

/// 将当前资产切换到下载态。`false` 表示同一资产已经在下载或已经校验完成。
pub(crate) fn begin(asset: &UpdateAsset) -> Result<bool, String> {
    validate_asset(asset)?;
    let mut current = session();
    if let Some(existing) = current.as_ref().filter(|existing| existing.asset == *asset)
        && matches!(
            existing.status,
            DownloadStatus::Downloading { .. } | DownloadStatus::Ready { .. }
        )
    {
        return Ok(false);
    }
    *current = Some(DownloadSession {
        asset: asset.clone(),
        status: DownloadStatus::Downloading { downloaded: 0, total: asset.size },
    });
    Ok(true)
}

/// 在后台执行器线程调用；进度直接写入进程内会话，UI 以低频轮询刷新。
pub(crate) fn run(asset: UpdateAsset) {
    let outcome = download_and_verify(&asset);
    let mut current = session();
    let Some(current) = current.as_mut().filter(|current| current.asset == asset) else {
        return;
    };
    current.status = match outcome {
        Ok((path, bytes)) => DownloadStatus::Ready { path, bytes },
        Err(error) => DownloadStatus::Failed(error),
    };
}

pub(crate) fn launch_ready(asset: &UpdateAsset) -> Result<(), String> {
    let path = match status(asset) {
        DownloadStatus::Ready { path, .. } => path,
        _ => return Err(UiLanguage::current().tr("update.installer_not_verified").to_owned()),
    };
    let (_, expected_path) = download_paths(asset)?;
    if path != expected_path || !path.is_file() {
        return Err(UiLanguage::current().tr("update.verified_installer_missing").to_owned());
    }
    // Ready 只表示下载完成时通过过校验；安装前再读一遍，避免缓存文件在
    // 弹窗等待用户确认期间被替换后仍直接执行。
    verify_file(&path, asset).map_err(|error| UiLanguage::current().tr_args("update.preinstall_verify_failed", &[("error", error.as_str())]))?;

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;

        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        Command::new(&path)
            // 安装向导必须可见；这里只切断旧进程的标准流并让安装器独立存活。
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NEW_PROCESS_GROUP)
            .spawn()
            .map_err(|error| UiLanguage::current().tr_args("update.installer_launch_failed", &[("error", &error.to_string())]))?;
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = path;
        Err(UiLanguage::current().tr("update.in_app_install_unsupported").to_owned())
    }
}

fn download_and_verify(asset: &UpdateAsset) -> Result<(PathBuf, u64), String> {
    validate_asset(asset)?;
    let (partial_path, final_path) = download_paths(asset)?;
    let _download_lock = crate::atomic_file::try_lifetime_lock(&final_path)
        .map_err(|error| UiLanguage::current().tr_args("update.download_lock_failed", &[("error", &error.to_string())]))?
        .ok_or_else(|| UiLanguage::current().tr("update.download_busy").to_owned())?;

    if final_path.is_file()
        && let Ok(bytes) = verify_file(&final_path, asset)
    {
        return Ok((final_path, bytes));
    }

    let result = download_to_partial(asset, &partial_path).and_then(|bytes| {
        crate::atomic_file::replace(&partial_path, &final_path)
            .map_err(|error| UiLanguage::current().tr_args("update.save_verified_installer_failed", &[("error", &error.to_string())]))?;
        Ok((final_path.clone(), bytes))
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&partial_path);
    }
    result
}

fn download_to_partial(asset: &UpdateAsset, partial_path: &Path) -> Result<u64, String> {
    let agent = ureq::config::Config::builder()
        .timeout_global(Some(Duration::from_secs(15 * 60)))
        .build()
        .new_agent();
    let mut response = agent
        .get(&asset.download_url)
        .header("User-Agent", "pebrel-updater")
        .header("Accept", "application/octet-stream")
        .header("Accept-Encoding", "identity")
        .call()
        .map_err(|error| UiLanguage::current().tr_args("update.installer_download_failed", &[("error", &error.to_string())]))?;

    let response_size = response.body().content_length();
    if let (Some(expected), Some(actual)) = (asset.size, response_size)
        && expected != actual
    {
        return Err(UiLanguage::current().tr_args("update.installer_length_mismatch", &[("actual", &actual.to_string()), ("expected", &expected.to_string())]));
    }
    let total = asset.size.or(response_size);
    if total.is_some_and(|bytes| bytes > MAX_INSTALLER_BYTES) {
        return Err(UiLanguage::current().tr("update.installer_too_large").to_owned());
    }

    let mut output = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(partial_path)
        .map_err(|error| UiLanguage::current().tr_args("update.temp_file_create_failed", &[("error", &error.to_string())]))?;
    let mut reader = response.body_mut().as_reader();
    let mut hasher = Sha256::new();
    let mut downloaded = 0_u64;
    let mut pe_header = Vec::with_capacity(2);
    let mut buffer = vec![0_u8; DOWNLOAD_CHUNK_BYTES];
    loop {
        let read = reader.read(&mut buffer).map_err(|error| UiLanguage::current().tr_args("update.installer_read_failed", &[("error", &error.to_string())]))?;
        if read == 0 {
            break;
        }
        downloaded = downloaded.saturating_add(read as u64);
        if downloaded > MAX_INSTALLER_BYTES {
            return Err(UiLanguage::current().tr("update.installer_too_large").to_owned());
        }
        if pe_header.len() < 2 {
            let take = (2 - pe_header.len()).min(read);
            pe_header.extend_from_slice(&buffer[..take]);
        }
        hasher.update(&buffer[..read]);
        output
            .write_all(&buffer[..read])
            .map_err(|error| UiLanguage::current().tr_args("update.temp_file_write_failed", &[("error", &error.to_string())]))?;
        set_progress(asset, downloaded, total);
    }
    output.sync_all().map_err(|error| UiLanguage::current().tr_args("update.temp_file_sync_failed", &[("error", &error.to_string())]))?;

    verify_download(downloaded, &pe_header, hasher.finalize(), asset)?;
    Ok(downloaded)
}

fn verify_file(path: &Path, asset: &UpdateAsset) -> Result<u64, String> {
    let mut file = File::open(path).map_err(|error| UiLanguage::current().tr_args("update.cache_read_failed", &[("error", &error.to_string())]))?;
    let metadata = file.metadata().map_err(|error| UiLanguage::current().tr_args("update.cache_size_read_failed", &[("error", &error.to_string())]))?;
    let bytes = metadata.len();
    if bytes > MAX_INSTALLER_BYTES {
        return Err(UiLanguage::current().tr("update.cache_too_large").to_owned());
    }
    let mut hasher = Sha256::new();
    let mut pe_header = Vec::with_capacity(2);
    let mut buffer = vec![0_u8; DOWNLOAD_CHUNK_BYTES];
    loop {
        let read = file.read(&mut buffer).map_err(|error| UiLanguage::current().tr_args("update.cache_content_read_failed", &[("error", &error.to_string())]))?;
        if read == 0 {
            break;
        }
        if pe_header.len() < 2 {
            let take = (2 - pe_header.len()).min(read);
            pe_header.extend_from_slice(&buffer[..take]);
        }
        hasher.update(&buffer[..read]);
    }
    verify_download(bytes, &pe_header, hasher.finalize(), asset)?;
    Ok(bytes)
}

fn verify_download(
    bytes: u64,
    pe_header: &[u8],
    digest: impl AsRef<[u8]>,
    asset: &UpdateAsset,
) -> Result<(), String> {
    if bytes == 0 || asset.size.is_some_and(|expected| expected != bytes) {
        return Err(UiLanguage::current().tr_args("update.installer_length_verify_failed", &[("bytes", &bytes.to_string())]));
    }
    if pe_header != b"MZ" {
        return Err(UiLanguage::current().tr("update.not_windows_installer").to_owned());
    }
    let expected = asset.sha256.as_deref().ok_or_else(|| UiLanguage::current().tr("update.release_missing_sha256").to_owned())?;
    let mut actual = String::with_capacity(64);
    for byte in digest.as_ref() {
        let _ = write!(&mut actual, "{byte:02x}");
    }
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(UiLanguage::current().tr_args("update.sha256_verify_failed", &[("actual", actual.as_str())]));
    }
    Ok(())
}

fn set_progress(asset: &UpdateAsset, downloaded: u64, total: Option<u64>) {
    let mut current = session();
    if let Some(current) = current.as_mut().filter(|current| current.asset == *asset) {
        current.status = DownloadStatus::Downloading { downloaded, total };
    }
}

fn download_paths(asset: &UpdateAsset) -> Result<(PathBuf, PathBuf), String> {
    let directory = nebula_settings::settings_dir().join("updates");
    std::fs::create_dir_all(&directory)
        .map_err(|error| UiLanguage::current().tr_args("update.download_directory_create_failed", &[("error", &error.to_string())]))?;
    let final_path = directory.join(&asset.name);
    let partial_path = directory.join(format!("{}.part", asset.name));
    Ok((partial_path, final_path))
}

fn validate_asset(asset: &UpdateAsset) -> Result<(), String> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err(UiLanguage::current().tr("update.no_installer_for_platform").to_owned());
    }
    validate_windows_asset_contract(asset)
}

fn validate_windows_asset_contract(asset: &UpdateAsset) -> Result<(), String> {
    if asset.version.is_empty()
        || !asset
            .version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
    {
        return Err(UiLanguage::current().tr("update.invalid_release_version").to_owned());
    }
    if !crate::update_check::windows_x64_installer_names(&asset.version).contains(&asset.name) {
        return Err(UiLanguage::current().tr("update.wrong_platform_asset").to_owned());
    }
    let trusted_url = [RELEASE_DOWNLOAD_PREFIX, LEGACY_RELEASE_DOWNLOAD_PREFIX]
        .iter()
        .any(|prefix| asset.download_url == format!("{prefix}v{}/{}", asset.version, asset.name));
    if !trusted_url {
        return Err(UiLanguage::current().tr("update.untrusted_installer_url").to_owned());
    }
    if asset.size.is_some_and(|bytes| bytes == 0 || bytes > MAX_INSTALLER_BYTES) {
        return Err(UiLanguage::current().tr("update.invalid_installer_size").to_owned());
    }
    let hash = asset.sha256.as_deref().ok_or_else(|| {
        UiLanguage::current().tr("update.unverifiable_sha256").to_owned()
    })?;
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(UiLanguage::current().tr("update.invalid_sha256_format").to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sha2::{Digest as _, Sha256};

    use super::{
        LEGACY_RELEASE_DOWNLOAD_PREFIX, MAX_INSTALLER_BYTES, RELEASE_DOWNLOAD_PREFIX, UpdateAsset,
        validate_windows_asset_contract, verify_download,
    };

    fn asset(url: &str, sha256: Option<&str>) -> UpdateAsset {
        UpdateAsset {
            version: "1.4.0".to_owned(),
            name: "NebulaTerminal-1.4.0-windows-x64-setup.exe".to_owned(),
            download_url: url.to_owned(),
            size: Some(42),
            sha256: sha256.map(str::to_owned),
        }
    }

    #[test]
    fn accepts_exact_official_asset_contract() {
        let url = "https://github.com/Kuddev/nebula/releases/download/v1.4.0/NebulaTerminal-1.4.0-windows-x64-setup.exe";
        let hash = "a".repeat(64);
        assert!(validate_windows_asset_contract(&asset(url, Some(hash.as_str()))).is_ok());
    }

    #[test]
    fn rejects_untrusted_url_or_missing_digest() {
        let official = "https://github.com/Kuddev/nebula/releases/download/v1.4.0/NebulaTerminal-1.4.0-windows-x64-setup.exe";
        let untrusted = "https://example.invalid/NebulaTerminal-1.4.0-windows-x64-setup.exe";
        let hash = "a".repeat(64);

        assert!(validate_windows_asset_contract(&asset(untrusted, Some(hash.as_str()))).is_err());
        assert!(validate_windows_asset_contract(&asset(official, None)).is_err());
    }

    fn branded_asset(brand: &str) -> UpdateAsset {
        let name = format!("{brand}-1.6.0-windows-x64-setup.exe");
        UpdateAsset {
            version: "1.6.0".to_owned(),
            download_url: format!("{RELEASE_DOWNLOAD_PREFIX}v1.6.0/{name}"),
            name,
            size: Some(42),
            sha256: Some("b".repeat(64)),
        }
    }

    #[test]
    fn both_brand_names_require_the_same_exact_version_and_url_contract() {
        for brand in ["Pebrel", "NebulaTerminal"] {
            let original = branded_asset(brand);
            assert!(validate_windows_asset_contract(&original).is_ok());
            let mut legacy_url = original.clone();
            legacy_url.download_url =
                format!("{LEGACY_RELEASE_DOWNLOAD_PREFIX}v{}/{}", original.version, original.name);
            assert!(validate_windows_asset_contract(&legacy_url).is_ok());
            for url in [
                original.download_url.replace("/v1.6.0/", "/v1.5.0/"),
                original.download_url.replace("/v1.6.0/", "/v1.6.0/extra/"),
                original.download_url.replace("github.com/", "github.com.evil.invalid/"),
                original.download_url.replace("https://", "http://"),
                format!("{}?download=1", original.download_url),
                original.download_url.replace("Kuddev/pebrel/", "elsewhere/pebrel/"),
            ] {
                let mut candidate = original.clone();
                candidate.download_url = url;
                assert!(validate_windows_asset_contract(&candidate).is_err(), "{candidate:?}");
            }
            let mut candidate = original;
            candidate.version = "1.5.0".to_owned();
            assert!(validate_windows_asset_contract(&candidate).is_err());
        }
    }

    #[test]
    fn rejects_non_windows_x64_names_invalid_versions_sizes_and_hashes() {
        for name in [
            "Pebrel-1.6.0-windows-arm64-setup.exe",
            "Pebrel-v1.6.0-windows-x64.zip",
            "Pebrel-v1.6.0-linux-x86_64.AppImage",
            "../Pebrel-1.6.0-windows-x64-setup.exe",
        ] {
            let mut candidate = branded_asset("Pebrel");
            candidate.name = name.to_owned();
            assert!(validate_windows_asset_contract(&candidate).is_err());
        }
        for version in ["", "../1.6.0", "1.6.0?download=1", "1.6.0\n"] {
            let mut candidate = branded_asset("Pebrel");
            candidate.version = version.to_owned();
            assert!(validate_windows_asset_contract(&candidate).is_err());
        }
        for size in [0, MAX_INSTALLER_BYTES + 1] {
            let mut candidate = branded_asset("Pebrel");
            candidate.size = Some(size);
            assert!(validate_windows_asset_contract(&candidate).is_err());
        }
        for hash in [None, Some("a".repeat(63)), Some("g".repeat(64))] {
            let mut candidate = branded_asset("Pebrel");
            candidate.sha256 = hash;
            assert!(validate_windows_asset_contract(&candidate).is_err());
        }
    }

    #[test]
    fn both_brands_reject_corrupt_or_non_executable_downloads() {
        let bytes = b"MZinstaller fixture";
        let digest = Sha256::digest(bytes);
        let hash: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        for brand in ["Pebrel", "NebulaTerminal"] {
            let mut candidate = branded_asset(brand);
            candidate.size = Some(bytes.len() as u64);
            candidate.sha256 = Some(hash.clone());
            assert!(verify_download(bytes.len() as u64, b"MZ", digest, &candidate).is_ok());
            assert!(verify_download(0, b"MZ", digest, &candidate).is_err());
            assert!(verify_download(bytes.len() as u64 - 1, b"MZ", digest, &candidate).is_err());
            assert!(verify_download(bytes.len() as u64, b"<!", digest, &candidate).is_err());
            assert!(
                verify_download(bytes.len() as u64, b"MZ", Sha256::digest(b"changed"), &candidate)
                    .is_err()
            );
        }
    }
}
