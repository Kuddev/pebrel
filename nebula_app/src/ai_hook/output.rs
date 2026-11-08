//! A bounded preview when the remote envelope cannot carry the full answer.

use serde_json::Value;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct BridgeOutput {
    pub preview: Option<Arc<str>>,
    pub original_chars: u64,
    pub truncated: bool,
}

impl BridgeOutput {
    pub(super) fn from_payload(payload: &Value) -> Option<Self> {
        let output = payload.get("pebrel_output")?;
        if output.get("reason")?.as_str()? != "envelope_limit" {
            return None;
        }
        let original_chars = output.get("original_chars")?.as_u64()?;
        // The bridge accepts at most one MiB of provider JSON. Validate its
        // preview independently; provider JSON is still untrusted data.
        if original_chars > 1024 * 1024 {
            return None;
        }
        let preview = match output.get("preview")? {
            Value::Null => None,
            Value::String(text) if text.chars().take(4_001).count() <= 4_000 => {
                Some(Arc::<str>::from(text.as_str()))
            },
            _ => return None,
        };
        let kept = preview.as_ref().map_or(0, |text| text.chars().count() as u64);
        if kept > original_chars {
            return None;
        }
        Some(Self { truncated: kept < original_chars, preview, original_chars })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bounded_bridge_output_checks_lengths_and_completeness() {
        let payload = |preview, original_chars| {
            json!({
                "pebrel_output": {
                    "reason": "envelope_limit", "preview": preview,
                    "original_chars": original_chars,
                }
            })
        };
        let output = BridgeOutput::from_payload(&payload(json!("前缀"), 30_000)).unwrap();
        assert!(output.truncated);
        assert_eq!(output.preview.as_deref(), Some("前缀"));
        assert!(!BridgeOutput::from_payload(&payload(json!("完整"), 2)).unwrap().truncated);
        assert!(BridgeOutput::from_payload(&payload(json!("too long"), 2)).is_none());
        assert!(BridgeOutput::from_payload(&payload(json!("X".repeat(4_001)), 10_000)).is_none());
        assert!(BridgeOutput::from_payload(&payload(json!("X"), u64::MAX)).is_none());
        let missing = BridgeOutput::from_payload(&payload(Value::Null, 30_000)).unwrap();
        assert!(missing.truncated);
        assert!(missing.preview.is_none());
    }
}
