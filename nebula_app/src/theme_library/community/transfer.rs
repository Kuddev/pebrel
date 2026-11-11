//! Bounded HTTP transfers for the community catalog. Each operation builds its
//! own agent through the updater's proxy resolution and runs on a caller-owned
//! background thread; there is no shared client, cache or resident service.

use super::{CATALOG_URL, Entry, Listing, MAX_CATALOG_BYTES, MAX_PREVIEW_BYTES};
use crate::i18n::{Message, UiLanguage};
use crate::theme_library::package::CheckedPackage;
use crate::theme_library::{ThemeDocument, ThemeLibraryStore};
use crate::update_proxy::{NetworkFailure, classify};
use sha2::{Digest, Sha256};
use std::fmt;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const USER_AGENT: &str = "pebrel-community";
const PREVIEW_EDGE: u32 = 480;

#[derive(Debug)]
pub(crate) enum Error {
    Network(NetworkFailure),
    TooLarge,
    /// The bytes differ from the size or SHA-256 pinned by the catalog.
    Integrity,
    Catalog(String),
    Package(String),
    Cancelled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(NetworkFailure::Status(status)) => write!(f, "HTTP {status}"),
            Self::Network(failure) => write!(f, "network error: {failure:?}"),
            Self::TooLarge => f.write_str("download exceeds its size limit"),
            Self::Integrity => f.write_str("download does not match the catalog size and SHA-256"),
            Self::Catalog(detail) | Self::Package(detail) => f.write_str(detail),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    pub(crate) fn describe(&self, language: UiLanguage) -> String {
        let message = match self {
            Self::Network(NetworkFailure::Status(status)) => {
                return language
                    .format(Message::ThemeCommunityErrorHttp, &[("status", &status.to_string())]);
            },
            Self::Network(NetworkFailure::Dns) => Message::ThemeCommunityErrorDns,
            Self::Network(NetworkFailure::Tls) => Message::ThemeCommunityErrorTls,
            Self::Network(NetworkFailure::Timeout) => Message::ThemeCommunityErrorTimeout,
            Self::Network(NetworkFailure::Proxy) => Message::ThemeCommunityErrorProxy,
            Self::Network(_) => Message::ThemeCommunityErrorNetwork,
            Self::TooLarge | Self::Integrity => Message::ThemeCommunityErrorIntegrity,
            Self::Catalog(detail) => {
                return language.format(Message::ThemeCommunityErrorCatalog, &[("detail", detail)]);
            },
            Self::Package(detail) => {
                return language.format(Message::ThemeCommunityErrorPackage, &[("detail", detail)]);
            },
            Self::Cancelled => Message::ThemeCommunityErrorNetwork,
        };
        language.text(message).to_owned()
    }
}

impl From<ureq::Error> for Error {
    fn from(error: ureq::Error) -> Self {
        Self::Network(classify(&error))
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        ureq::Error::from(error).into()
    }
}

fn agent(url: &str, timeout: Duration) -> Result<ureq::Agent, Error> {
    Ok(crate::update_proxy::agent(url, timeout)?)
}

pub(crate) fn fetch_catalog() -> Result<Listing, Error> {
    catalog_with(&agent(CATALOG_URL, Duration::from_secs(30))?, CATALOG_URL)
}

/// Download a preview and reduce it to a card-sized RGBA thumbnail, so the
/// view keeps only small decoded frames.
pub(crate) fn fetch_preview(url: &str, cancel: &AtomicBool) -> Result<image::RgbaImage, Error> {
    check_cancel(cancel)?;
    let agent = agent(url, Duration::from_secs(30))?;
    let bytes = get_small(&agent, url, "image/*", MAX_PREVIEW_BYTES, cancel)?;
    check_cancel(cancel)?;
    decode_preview(bytes)
}

/// Download the pinned archive, verify its size and digest while streaming,
/// then install it through the ordinary package checks without applying it.
pub(crate) fn install(
    entry: &Entry,
    store: &ThemeLibraryStore,
    cancel: &AtomicBool,
) -> Result<ThemeDocument, Error> {
    install_with_progress(entry, store, cancel, &mut |_| {})
}

pub(crate) fn install_with_progress(
    entry: &Entry,
    store: &ThemeLibraryStore,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64),
) -> Result<ThemeDocument, Error> {
    check_cancel(cancel)?;
    let agent = agent(&entry.archive.url, Duration::from_secs(10 * 60))?;
    install_with(&agent, &entry.archive.url, entry, store, cancel, progress)
}

pub(super) fn catalog_with(agent: &ureq::Agent, url: &str) -> Result<Listing, Error> {
    Listing::parse(&get_small(
        agent,
        url,
        "application/json",
        MAX_CATALOG_BYTES,
        &AtomicBool::new(false),
    )?)
    .map_err(Error::Catalog)
}

pub(super) fn decode_preview(bytes: Vec<u8>) -> Result<image::RgbaImage, Error> {
    let invalid = |error: image::ImageError| Error::Package(error.to_string());
    let format = image::guess_format(&bytes).map_err(invalid)?;
    if !matches!(
        format,
        image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP
    ) {
        return Err(Error::Package("unsupported preview format".into()));
    }
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(invalid)?;
    Ok(if image.width().max(image.height()) > PREVIEW_EDGE {
        image.thumbnail(PREVIEW_EDGE, PREVIEW_EDGE).into_rgba8()
    } else {
        image.into_rgba8()
    })
}

pub(super) fn install_with(
    agent: &ureq::Agent,
    url: &str,
    entry: &Entry,
    store: &ThemeLibraryStore,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64),
) -> Result<ThemeDocument, Error> {
    let mut response = request(agent, url, "application/octet-stream")?;
    if response.body().content_length().is_some_and(|length| length != entry.archive.bytes) {
        return Err(Error::Integrity);
    }
    let mut file = tempfile::Builder::new().prefix("pebrel-theme-").suffix(".zip").tempfile()?;
    copy_verified(
        &mut response.body_mut().as_reader(),
        file.as_file_mut(),
        entry.archive.bytes,
        &entry.archive.sha256,
        cancel,
        progress,
    )?;
    file.as_file().sync_all()?;
    check_cancel(cancel)?;
    install_file(file.path(), entry, store)
}

pub(crate) fn install_file(
    path: &Path,
    entry: &Entry,
    store: &ThemeLibraryStore,
) -> Result<ThemeDocument, Error> {
    let package = CheckedPackage::open(path).map_err(|error| Error::Package(error.to_string()))?;
    package
        .install_from(store, Some(&entry.origin()))
        .map_err(|error| Error::Package(error.to_string()))
}

/// Copy exactly `expected` bytes whose SHA-256 is `sha256`; anything longer,
/// shorter or different fails before the package reader sees it.
pub(super) fn copy_verified(
    reader: &mut impl Read,
    writer: &mut impl Write,
    expected: u64,
    sha256: &str,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64),
) -> Result<(), Error> {
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    loop {
        check_cancel(cancel)?;
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > expected {
            return Err(Error::Integrity);
        }
        digest.update(&buffer[..read]);
        writer.write_all(&buffer[..read]).map_err(|error| Error::Package(error.to_string()))?;
        progress(total);
    }
    let actual: String = digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    if total != expected || actual != sha256 {
        return Err(Error::Integrity);
    }
    Ok(())
}

fn request(
    agent: &ureq::Agent,
    url: &str,
    accept: &str,
) -> Result<ureq::http::Response<ureq::Body>, Error> {
    Ok(agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", accept)
        .header("Accept-Encoding", "identity")
        .call()?)
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), Error> {
    if cancel.load(Ordering::Relaxed) { Err(Error::Cancelled) } else { Ok(()) }
}

fn get_small(
    agent: &ureq::Agent,
    url: &str,
    accept: &str,
    limit: u64,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, Error> {
    check_cancel(cancel)?;
    let mut response = request(agent, url, accept)?;
    if response.body().content_length().is_some_and(|length| length > limit) {
        return Err(Error::TooLarge);
    }
    let mut bytes = Vec::new();
    let mut reader = response.body_mut().as_reader().take(limit + 1);
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        check_cancel(cancel)?;
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    if bytes.len() as u64 > limit {
        return Err(Error::TooLarge);
    }
    Ok(bytes)
}
