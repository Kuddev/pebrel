use base64::Engine as _;
use serde_json::{Value, json};

use super::{Capture, HEAD_BYTES, PAGE_BYTES, changed, io_failure};
use crate::runtime_api::ApiError;
use crate::ssh_session::TranscriptReader;

const SCRIPT: &str = include_str!("remote_read.py");

pub(super) fn capture(
    reader: &TranscriptReader,
    path: &str,
    before: Option<u64>,
) -> Result<Capture, ApiError> {
    let base64 = base64::engine::general_purpose::STANDARD;
    let argument = base64.encode(json!({"path": path, "before": before}).to_string());
    let script = format!("{SCRIPT}\nmain(json.loads(base64.b64decode('{argument}')))\n");
    let raw = crate::ssh_session::runtime()
        .map_err(io_failure)?
        .block_on(reader.capture(script.as_bytes()))
        .map_err(io_failure)?;
    decode(&raw, before)
}

fn decode(raw: &str, before: Option<u64>) -> Result<Capture, ApiError> {
    if raw.len() > 2 * 1024 * 1024 {
        return Err(io_failure("response budget"));
    }
    let line = raw
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("PEBREL_TRANSCRIPT="))
        .ok_or_else(|| io_failure("response"))?;
    let value: Value = serde_json::from_str(line).map_err(io_failure)?;
    if value["version"] != 1 {
        return Err(io_failure("version"));
    }
    if value["error"] == "conversation_changed" {
        return Err(changed());
    }
    if value.get("error").is_some() {
        return Err(io_failure("remote file"));
    }
    let end = value["end"].as_u64().ok_or_else(|| io_failure("end"))?;
    let start = value["start"].as_u64().ok_or_else(|| io_failure("start"))?;
    if before.is_some_and(|before| end != before) || start != end.saturating_sub(PAGE_BYTES as u64)
    {
        return Err(changed());
    }
    let base64 = base64::engine::general_purpose::STANDARD;
    let head = base64
        .decode(value["head"].as_str().ok_or_else(|| io_failure("head"))?)
        .map_err(io_failure)?;
    let bytes = base64
        .decode(value["bytes"].as_str().ok_or_else(|| io_failure("bytes"))?)
        .map_err(io_failure)?;
    let stamp = value["stamp"]
        .as_str()
        .filter(|stamp| stamp.len() <= 256)
        .ok_or_else(|| io_failure("stamp"))?
        .to_owned();
    if head.len() > HEAD_BYTES
        || bytes.len() > PAGE_BYTES + 1
        || bytes.len() as u64 != end - start.saturating_sub(1)
    {
        return Err(io_failure("byte range"));
    }
    Ok(Capture { head, bytes, start, end, stamp })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_capture_checks_revision_shape_and_does_not_accept_unbounded_bytes() {
        let response = |value: Value| format!("banner\nPEBREL_TRANSCRIPT={value}\n");
        let valid =
            json!({"version":1,"start":0,"end":3,"head":"YWJj","bytes":"YWJj","stamp":"owner"});
        assert_eq!(decode(&response(valid.clone()), None).unwrap().bytes, b"abc");
        assert!(decode(&response(valid.clone()), Some(4)).is_err());
        let mut invalid = valid;
        invalid["end"] = json!(5);
        assert!(decode(&response(invalid), None).is_err());
        assert_eq!(
            decode(&response(json!({"version":1,"error":"conversation_changed"})), None)
                .err()
                .unwrap()
                .code,
            "conversation_changed"
        );
    }
}
