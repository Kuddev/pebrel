//! Shared-tab addressing and bounded file-read requests. Tab order is not identity.

use super::{ApiError, ApiRequest, RuntimeCommand, parse_params};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabId(pub String);

impl Default for TabId {
    fn default() -> Self {
        use std::sync::{
            LazyLock,
            atomic::{AtomicU64, Ordering},
        };
        static NEXT: AtomicU64 = AtomicU64::new(1);
        static INSTANCE: LazyLock<u64> = LazyLock::new(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64
                ^ u64::from(std::process::id())
        });
        // 不持久化：重启、关闭后重新打开都必须获得新身份，迟到请求不得命中替身。
        Self(format!("{:016x}{:016x}", *INSTANCE, NEXT.fetch_add(1, Ordering::Relaxed)))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FileInfo {
    pub path: String,
    pub remote: bool,
    pub dirty: bool,
    pub saving: bool,
    pub ready: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
}

#[derive(Clone, Debug)]
pub enum Request {
    Focus { tab_id: String },
    Close { tab_id: String },
    Open { path: PathBuf },
    Read { tab_id: String, offset: usize, limit: usize, revision: Option<String> },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetParams {
    window_id: u64,
    tab_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenParams {
    window_id: u64,
    path: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadParams {
    window_id: u64,
    tab_id: String,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    revision: Option<String>,
}

fn default_limit() -> usize {
    MAX_CHUNK_BYTES
}

fn validate_target(window_id: u64, tab_id: &str) -> Result<(), ApiError> {
    if window_id == 0 || tab_id.len() != 32 || !tab_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ApiError::invalid_params(
            "an explicit window_id and snapshot tab_id are required",
        ));
    }
    Ok(())
}

pub(super) fn parse(request: &ApiRequest) -> Result<RuntimeCommand, ApiError> {
    let (window_id, request) = match request.method.as_str() {
        "tab.focus" | "tab.close" => {
            let params: TargetParams = parse_params(&request.params)?;
            validate_target(params.window_id, &params.tab_id)?;
            let operation = if request.method == "tab.focus" {
                Request::Focus { tab_id: params.tab_id }
            } else {
                Request::Close { tab_id: params.tab_id }
            };
            (params.window_id, operation)
        },
        "tab.open" => {
            let params: OpenParams = parse_params(&request.params)?;
            let path = params.path.to_string_lossy();
            if params.window_id == 0
                || !params.path.is_absolute()
                || path.len() > 4096
                || path.chars().any(char::is_control)
            {
                return Err(ApiError::invalid_params(
                    "window_id and an absolute file path are required",
                ));
            }
            (params.window_id, Request::Open { path: params.path })
        },
        "tab.read" => {
            let params: ReadParams = parse_params(&request.params)?;
            validate_target(params.window_id, &params.tab_id)?;
            if !(4..=MAX_CHUNK_BYTES).contains(&params.limit)
                || params.offset > MAX_FILE_BYTES
                || params.revision.as_ref().is_some_and(|s| s.is_empty() || s.len() > 128)
                || (params.offset > 0 && params.revision.is_none())
            {
                return Err(ApiError::invalid_params(
                    "invalid chunk range or missing file revision",
                ));
            }
            (
                params.window_id,
                Request::Read {
                    tab_id: params.tab_id,
                    offset: params.offset,
                    limit: params.limit,
                    revision: params.revision,
                },
            )
        },
        _ => return Err(ApiError::new("method_not_found", "unknown tab operation")),
    };
    Ok(RuntimeCommand::Tab { window_id, request })
}
