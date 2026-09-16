//! GPUI 更新安装包的下载、校验与启动。
//!
//! 更新检查只负责提供 release 元数据；本模块再次收紧资产合同，并把大文件
//! 流式写入同目录 `.part` 文件。只有长度、PE 文件头与 SHA-256 全部通过后，
//! 才原子替换为可启动的安装包，避免中断下载或错误响应变成可执行文件。

use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

mod cache;
pub(crate) mod handoff;
use std::time::Duration;

use sha2::{Digest as _, Sha256};

use crate::i18n::{Message, UiLanguage};
use crate::update_check::UpdateAsset;

const RELEASE_DOWNLOAD_PREFIX: &str = "https://github.com/Kuddev/pebrel/releases/download/";
const LEGACY_RELEASE_DOWNLOAD_PREFIX: &str = "https://github.com/Kuddev/nebula/releases/download/";
const MAX_INSTALLER_BYTES: u64 = 512 * 1024 * 1024;
const DOWNLOAD_CHUNK_BYTES: usize = 64 * 1024;

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

static DOWNLOAD_SESSION: Mutex<Option<DownloadSession>> = Mutex::new(None);

#[derive(Clone, Debug)]
pub(crate) enum DownloadStatus {
    Idle,
    Downloading { downloaded: u64, total: Option<u64> },
    Ready { path: PathBuf, bytes: u64 },
    Failed(String),
    InstallFailed(String),
}

impl DownloadStatus {
    pub(crate) fn is_terminal(&self) -> bool {
        !matches!(self, Self::Downloading { .. })
    }
}

#[derive(Clone, Debug)]
struct DownloadSession {
    generation: u64,
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

/// A task owns one generation. Cancellation or a new asset invalidates every
/// progress/completion write from the old task, even for the same version.
#[derive(Clone)]
pub(crate) struct DownloadJob {
    asset: UpdateAsset,
    generation: u64,
}

impl DownloadJob {
    pub(crate) fn is_current(&self) -> bool {
        session().as_ref().is_some_and(|current| {
            current.generation == self.generation && current.asset == self.asset
        })
    }
}

pub(crate) fn begin(asset: &UpdateAsset) -> Result<Option<DownloadJob>, String> {
    validate_asset(asset)?;
    Ok(begin_download_session(asset))
}

/// Session ownership is independent of installer availability. The public
/// entry point validates the platform and asset before reaching this state.
fn begin_download_session(asset: &UpdateAsset) -> Option<DownloadJob> {
    let mut current = session();
    if let Some(existing) = current.as_ref().filter(|existing| existing.asset == *asset)
        && matches!(
            existing.status,
            DownloadStatus::Downloading { .. } | DownloadStatus::Ready { .. }
        )
    {
        return None;
    }
    let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
    *current = Some(DownloadSession {
        generation,
        asset: asset.clone(),
        status: DownloadStatus::Downloading { downloaded: 0, total: asset.size },
    });
    Some(DownloadJob { asset: asset.clone(), generation })
}

pub(crate) fn cancel(asset: &UpdateAsset) {
    let mut current = session();
    if current.as_ref().is_some_and(|current| current.asset == *asset) {
        *current = None;
    }
}

/// Runs off the UI thread. Cached files are always reverified before Ready.
pub(crate) fn run(job: DownloadJob, language: UiLanguage) {
    if !job.is_current() {
        return;
    }
    let outcome = download_and_verify(&job.asset, language, Some(&job));
    let status = match outcome {
        Ok((path, bytes)) => DownloadStatus::Ready { path, bytes },
        Err(error) => DownloadStatus::Failed(error),
    };
    {
        let mut current = session();
        let Some(current) = current.as_mut().filter(|current| current.generation == job.generation)
        else {
            return;
        };
        current.status = status.clone();
    }
    // File sync can take seconds on a busy disk. UI status polling never waits
    // for it; the cache writer rechecks ownership separately.
    if let Err(error) = cache::save_job(&job, &status) {
        log::warn!("Could not persist update download state: {error}");
    }
}

/// Restore local update state without requiring a successful network check.
/// Call once on a background executor; a user-started task always takes priority.
pub(crate) fn hydrate() {
    let cached = handoff::failed_update()
        .map(|(asset, error)| (asset, DownloadStatus::InstallFailed(error)))
        .or_else(cache::load);
    let Some((asset, status)) = cached else {
        return;
    };
    let mut current = session();
    if current.is_none() {
        *current = Some(DownloadSession {
            generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
            asset,
            status,
        });
    }
}

pub(crate) fn cached_asset() -> Option<UpdateAsset> {
    session().as_ref().map(|current| current.asset.clone())
}

/// A failure that happened after "later" is new information. Once its details
/// were viewed/dismissed, normal reminder suppression applies again.
pub(crate) fn installation_failure_unseen(prompt_state: &Path) -> bool {
    handoff::failure_unseen(prompt_state) || cache::failure_unseen(prompt_state)
}

pub(crate) fn ready_path(asset: &UpdateAsset) -> Result<PathBuf, String> {
    let path = match status(asset) {
        DownloadStatus::Ready { path, .. } => path,
        _ => return Err("安装包尚未下载并通过校验".to_owned()),
    };
    let (_, expected_path) = download_paths(asset)?;
    if path != expected_path || !path.is_file() {
        return Err("已校验的安装包不存在或路径已改变".to_owned());
    }
    // Ready 只表示下载完成时通过过校验；安装前再读一遍，避免缓存文件在
    // 弹窗等待用户确认期间被替换后仍直接执行。
    verify_file(&path, asset).map_err(|error| format!("安装前重新校验失败：{error}"))?;

    Ok(path)
}

fn download_and_verify(
    asset: &UpdateAsset,
    language: UiLanguage,
    job: Option<&DownloadJob>,
) -> Result<(PathBuf, u64), String> {
    validate_asset(asset)?;
    let (partial_path, final_path) = download_paths(asset)?;
    let _download_lock = crate::atomic_file::try_lifetime_lock(&final_path)
        .map_err(|error| format!("无法锁定更新下载目录：{error}"))?
        .ok_or_else(|| "另一个 Pebrel 进程正在下载这项更新".to_owned())?;

    if final_path.is_file()
        && let Ok(bytes) = verify_file(&final_path, asset)
    {
        return Ok((final_path, bytes));
    }

    let result = download_to_partial(asset, &partial_path, language, job).and_then(|bytes| {
        if job.is_some_and(|job| !job.is_current()) {
            return Err("Download cancelled".into());
        }
        crate::atomic_file::replace(&partial_path, &final_path)
            .map_err(|error| format!("无法保存已校验的更新安装包：{error}"))?;
        Ok((final_path.clone(), bytes))
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&partial_path);
    }
    result
}

fn download_to_partial(
    asset: &UpdateAsset,
    partial_path: &Path,
    language: UiLanguage,
    job: Option<&DownloadJob>,
) -> Result<u64, String> {
    #[cfg(feature = "update-test-source")]
    if crate::update_check::test_source::origin()?.is_some() {
        let agent = crate::update_check::test_source::agent(Duration::from_secs(15 * 60));
        return download_with_job(asset, partial_path, language, &agent, job);
    }
    let agent = crate::update_proxy::agent(&asset.download_url, Duration::from_secs(15 * 60));
    download_with_job(asset, partial_path, language, &agent, job)
}

fn download_with_job(
    asset: &UpdateAsset,
    partial_path: &Path,
    language: UiLanguage,
    agent: &ureq::Agent,
    job: Option<&DownloadJob>,
) -> Result<u64, String> {
    if job.is_some_and(|job| !job.is_current()) {
        return Err("Download cancelled".into());
    }
    let download_url = asset.download_url.clone();
    #[cfg(feature = "update-test-source")]
    let download_url = crate::update_check::test_source::origin()?
        .map(|origin| format!("{origin}/{}", asset.name))
        .unwrap_or(download_url);
    let mut response = agent
        .get(&download_url)
        .header("User-Agent", "pebrel-updater")
        .header("Accept", "application/octet-stream")
        .header("Accept-Encoding", "identity")
        .call()
        .map_err(|error| network_error_text(error, language))?;

    let response_size = response.body().content_length();
    if let (Some(expected), Some(actual)) = (asset.size, response_size)
        && expected != actual
    {
        return Err(format!("安装包长度与 release 元数据不一致（{actual} / {expected} 字节）"));
    }
    let total = asset.size.or(response_size);
    if total.is_some_and(|bytes| bytes > MAX_INSTALLER_BYTES) {
        return Err("安装包超过 512 MiB 安全上限".to_owned());
    }

    let mut output = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(partial_path)
        .map_err(|error| format!("无法创建更新临时文件：{error}"))?;
    let mut reader = response.body_mut().as_reader();
    let mut hasher = Sha256::new();
    let mut downloaded = 0_u64;
    let mut pe_header = Vec::with_capacity(2);
    let mut buffer = vec![0_u8; DOWNLOAD_CHUNK_BYTES];
    loop {
        if job.is_some_and(|job| !job.is_current()) {
            return Err("Download cancelled".into());
        }
        let read =
            reader.read(&mut buffer).map_err(|error| network_error_text(error.into(), language))?;
        if read == 0 {
            break;
        }
        downloaded = downloaded.saturating_add(read as u64);
        if downloaded > MAX_INSTALLER_BYTES {
            return Err("安装包超过 512 MiB 安全上限".to_owned());
        }
        if pe_header.len() < 2 {
            let take = (2 - pe_header.len()).min(read);
            pe_header.extend_from_slice(&buffer[..take]);
        }
        hasher.update(&buffer[..read]);
        output
            .write_all(&buffer[..read])
            .map_err(|error| format!("写入更新临时文件失败：{error}"))?;
        set_progress(job, downloaded, total);
    }
    output.sync_all().map_err(|error| format!("同步更新临时文件失败：{error}"))?;

    verify_download(downloaded, &pe_header, hasher.finalize(), asset)?;
    Ok(downloaded)
}

#[cfg(test)]
fn download_with_agent(
    asset: &UpdateAsset,
    partial_path: &Path,
    language: UiLanguage,
    agent: &ureq::Agent,
) -> Result<u64, String> {
    download_with_job(asset, partial_path, language, agent, None)
}

/// 网络错误用稳定类别解释；不把代理 URL、认证信息或 CDN 查询串拼进 UI。
fn network_error_text(error: ureq::Error, language: UiLanguage) -> String {
    use std::io::ErrorKind;
    use ureq::Error;

    let message = match error {
        Error::HostNotFound => Message::UpdateDownloadDns,
        Error::Tls(_) | Error::Rustls(_) | Error::TlsRequired => Message::UpdateDownloadTls,
        Error::Timeout(_) => Message::UpdateDownloadTimeout,
        Error::Io(ref io) if io.kind() == ErrorKind::ConnectionRefused => {
            Message::UpdateDownloadRefused
        },
        Error::Io(ref io) if io.kind() == ErrorKind::TimedOut => Message::UpdateDownloadTimeout,
        Error::Io(ref io)
            if matches!(
                io.kind(),
                ErrorKind::ConnectionReset | ErrorKind::UnexpectedEof | ErrorKind::BrokenPipe
            ) =>
        {
            Message::UpdateDownloadInterrupted
        },
        Error::ConnectProxyFailed(_) | Error::InvalidProxyUrl => Message::UpdateDownloadProxy,
        Error::StatusCode(status) => {
            return language
                .format(Message::UpdateDownloadHttp, &[("status", &status.to_string())]);
        },
        _ => Message::UpdateDownloadNetwork,
    };
    language.text(message).to_owned()
}

fn verify_file(path: &Path, asset: &UpdateAsset) -> Result<u64, String> {
    let mut file = File::open(path).map_err(|error| format!("无法读取更新缓存：{error}"))?;
    let metadata = file.metadata().map_err(|error| format!("无法读取更新缓存大小：{error}"))?;
    let bytes = metadata.len();
    if bytes > MAX_INSTALLER_BYTES {
        return Err("更新缓存超过 512 MiB 安全上限".to_owned());
    }
    let mut hasher = Sha256::new();
    let mut pe_header = Vec::with_capacity(2);
    let mut buffer = vec![0_u8; DOWNLOAD_CHUNK_BYTES];
    loop {
        let read = file.read(&mut buffer).map_err(|error| format!("读取更新缓存失败：{error}"))?;
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
        return Err(format!("安装包长度校验失败（实际 {bytes} 字节）"));
    }
    if pe_header != b"MZ" {
        return Err("下载内容不是 Windows PE 安装包".to_owned());
    }
    let expected = asset.sha256.as_deref().ok_or_else(|| "release 未提供 SHA-256".to_owned())?;
    let mut actual = String::with_capacity(64);
    for byte in digest.as_ref() {
        let _ = write!(&mut actual, "{byte:02x}");
    }
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(format!("安装包 SHA-256 校验失败（实际 {actual}）"));
    }
    Ok(())
}

fn set_progress(job: Option<&DownloadJob>, downloaded: u64, total: Option<u64>) {
    let Some(job) = job else {
        return;
    };
    let mut current = session();
    if let Some(current) = current.as_mut().filter(|current| current.generation == job.generation) {
        current.status = DownloadStatus::Downloading { downloaded, total };
    }
}

fn download_paths(asset: &UpdateAsset) -> Result<(PathBuf, PathBuf), String> {
    let directory = nebula_settings::settings_dir().join("updates");
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("无法创建更新下载目录：{error}"))?;
    let final_path = directory.join(&asset.name);
    let partial_path = directory.join(format!("{}.part", asset.name));
    Ok((partial_path, final_path))
}

fn validate_asset(asset: &UpdateAsset) -> Result<(), String> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err("当前平台没有可用的自动更新安装包".to_owned());
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
        return Err("release 版本号不符合安装包命名规则".to_owned());
    }
    if !crate::update_check::windows_x64_installer_names(&asset.version).contains(&asset.name) {
        return Err("release 资产不是当前平台的精确安装包".to_owned());
    }
    let trusted_url = [RELEASE_DOWNLOAD_PREFIX, LEGACY_RELEASE_DOWNLOAD_PREFIX]
        .iter()
        .any(|prefix| asset.download_url == format!("{prefix}v{}/{}", asset.version, asset.name));
    if !trusted_url {
        return Err("release 安装包 URL 不属于 Pebrel 官方仓库".to_owned());
    }
    if asset.size.is_some_and(|bytes| bytes == 0 || bytes > MAX_INSTALLER_BYTES) {
        return Err("release 安装包大小无效".to_owned());
    }
    let hash = asset.sha256.as_deref().ok_or_else(|| {
        "release 未提供可验证的 SHA-256；为避免执行未知安装包，已停止自动下载".to_owned()
    })?;
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("release 提供的 SHA-256 格式无效".to_owned());
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

    #[test]
    fn cancel_then_retry_rejects_old_progress_and_old_completion() {
        let asset = branded_asset("Pebrel");
        validate_windows_asset_contract(&asset).unwrap();
        super::cancel(&asset);
        let old = super::begin_download_session(&asset).unwrap();
        assert!(
            super::begin_download_session(&asset).is_none(),
            "duplicate click owns no second task"
        );
        super::cancel(&asset);
        let current = super::begin_download_session(&asset).unwrap();
        super::set_progress(Some(&old), 100, Some(200));
        super::run(old.clone(), crate::i18n::UiLanguage::EnUs);
        assert!(!old.is_current());
        assert!(current.is_current());
        assert!(matches!(
            super::status(&asset),
            super::DownloadStatus::Downloading { downloaded: 0, .. }
        ));
        super::set_progress(Some(&current), 25, Some(200));
        assert!(matches!(
            super::status(&asset),
            super::DownloadStatus::Downloading { downloaded: 25, .. }
        ));
        super::cancel(&asset);
    }

    #[test]
    fn begin_preserves_platform_and_asset_validation() {
        let mut asset = branded_asset("Pebrel");
        assert_eq!(
            super::validate_asset(&asset).is_ok(),
            cfg!(all(windows, target_arch = "x86_64"))
        );
        if super::validate_asset(&asset).is_err() {
            assert!(super::begin(&asset).is_err());
        }
        asset.download_url = "https://example.invalid/untrusted.exe".into();
        assert!(super::begin(&asset).is_err(), "the session must not bypass asset validation");
    }

    #[test]
    fn proxy_download_follows_redirect_and_verifies_the_streamed_installer() {
        use crate::i18n::UiLanguage;
        use crate::update_proxy::test_support::{Server, response};

        let body = "MZinstaller over a proxy";
        let server = Server::start(vec![
            response("302 Found", "Location: http://cdn.update.invalid/installer\r\n", ""),
            response("200 OK", "", body),
        ]);
        let mut asset = branded_asset("Pebrel");
        asset.download_url = "http://release.update.invalid/asset".into();
        asset.size = Some(body.len() as u64);
        asset.sha256 = Some(
            Sha256::digest(body.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect(),
        );
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("installer.part");
        let bytes = super::download_with_agent(&asset, &path, UiLanguage::EnUs, &server.agent(&[]))
            .unwrap();
        assert_eq!(bytes, body.len() as u64);
        assert_eq!(std::fs::read(&path).unwrap(), body.as_bytes());
        assert!(super::verify_file(&path, &asset).is_ok());
        let requests = server.finish();
        assert!(requests[0].0.starts_with("CONNECT release.update.invalid:80 "));
        assert!(requests[1].0.starts_with("CONNECT cdn.update.invalid:80 "));
        assert!(requests[1].1.to_ascii_lowercase().contains("accept-encoding: identity"));
    }

    #[test]
    fn redirect_to_an_excluded_host_connects_directly() {
        use crate::i18n::UiLanguage;
        use crate::update_proxy::test_support::{Server, response};

        let body = "MZdirect CDN fixture";
        let origin = Server::start(vec![response("200 OK", "", body)]);
        let proxy = Server::start(vec![response(
            "302 Found",
            &format!("Location: http://{}/installer\r\n", origin.address),
            "",
        )]);
        let mut asset = branded_asset("Pebrel");
        asset.download_url = "http://release.update.invalid/asset".into();
        asset.size = Some(body.len() as u64);
        asset.sha256 = Some(
            Sha256::digest(body.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect(),
        );
        let directory = tempfile::tempdir().unwrap();
        super::download_with_agent(
            &asset,
            &directory.path().join("installer.part"),
            UiLanguage::EnUs,
            &proxy.agent(&["127.0.0.1"]),
        )
        .unwrap();
        assert!(proxy.finish()[0].0.starts_with("CONNECT release.update.invalid:80 "));
        let requests = origin.finish();
        assert!(requests[0].0.is_empty());
        assert!(requests[0].1.starts_with("GET /installer "));
    }

    #[test]
    fn proxy_download_rejects_http_errors_truncated_bodies_and_bad_digests() {
        use crate::i18n::UiLanguage;
        use crate::update_proxy::test_support::{Server, response};

        for reply in [
            response("503 Service Unavailable", "", "unavailable"),
            "HTTP/1.1 200 OK\r\nContent-Length: 42\r\nConnection: close\r\n\r\nMZshort".into(),
            response("200 OK", "", &format!("MZ{}", "x".repeat(40))),
        ] {
            let server = Server::start(vec![reply]);
            let mut asset = branded_asset("Pebrel");
            asset.download_url = "http://release.update.invalid/asset".into();
            let directory = tempfile::tempdir().unwrap();
            let result = super::download_with_agent(
                &asset,
                &directory.path().join("installer.part"),
                UiLanguage::EnUs,
                &server.agent(&[]),
            );
            assert!(result.is_err());
            server.finish();
        }
    }

    #[test]
    fn network_failures_have_localized_actionable_messages_without_credentials() {
        use crate::i18n::{Message, UiLanguage};
        use std::io::{Error as IoError, ErrorKind};
        use ureq::Error;

        for (error, message) in [
            (Error::HostNotFound, Message::UpdateDownloadDns),
            (Error::Tls("invalid certificate"), Message::UpdateDownloadTls),
            (
                Error::Io(IoError::from(ErrorKind::ConnectionRefused)),
                Message::UpdateDownloadRefused,
            ),
            (Error::Io(IoError::from(ErrorKind::TimedOut)), Message::UpdateDownloadTimeout),
            (
                Error::from(Error::Timeout(ureq::Timeout::Global).into_io()),
                Message::UpdateDownloadTimeout,
            ),
            (
                Error::Io(IoError::from(ErrorKind::UnexpectedEof)),
                Message::UpdateDownloadInterrupted,
            ),
            (
                Error::ConnectProxyFailed("http://user:secret@proxy.local".into()),
                Message::UpdateDownloadProxy,
            ),
            (Error::ConnectionFailed, Message::UpdateDownloadNetwork),
        ] {
            let text = super::network_error_text(error, UiLanguage::ZhCn);
            assert_eq!(text, UiLanguage::ZhCn.text(message));
            assert!(!text.contains("secret"));
            assert_ne!(UiLanguage::ZhCn.text(message), UiLanguage::EnUs.text(message));
        }
        assert!(
            super::network_error_text(Error::StatusCode(503), UiLanguage::EnUs)
                .contains("HTTP 503")
        );
    }

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
