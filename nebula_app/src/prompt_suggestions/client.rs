//! Bounded background inference; the selected provider never changes a CLI's model.
use std::sync::{
    LazyLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::{INSTRUCTIONS, Message, context, decode, voice_examples};
use crate::ai_providers::{AiProvider, ProviderKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Failure {
    Cancelled,
    Busy,
    Unavailable,
    Network,
    InvalidResponse,
}

type Prediction = Result<Option<String>, Failure>;

fn agent(local: bool) -> ureq::Agent {
    let config = ureq::config::Config::builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .timeout_connect(Some(Duration::from_millis(1500)))
        .max_redirects(0);
    (if local { config.proxy(None) } else { config }).build().new_agent()
}

static CLIENT: LazyLock<ureq::Agent> = LazyLock::new(|| agent(false));
static LOCAL_CLIENT: LazyLock<ureq::Agent> = LazyLock::new(|| agent(true));
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

struct Slot;
impl Slot {
    fn acquire() -> Option<Self> {
        ACTIVE
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < 2).then_some(n + 1))
            .ok()?;
        Some(Self)
    }
}
impl Drop for Slot {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::AcqRel);
    }
}

pub fn supported(provider: &AiProvider) -> bool {
    provider.enabled
        && !provider.model.trim().is_empty()
        && !matches!(provider.kind, ProviderKind::Google | ProviderKind::AzureOpenAi)
        && matches!(provider.wire_api.as_str(), "" | "responses" | "chat" | "chat_completions")
}

/// An immutable configuration/credential snapshot owned by the completion service.
/// Prepare off the UI thread on enable/configuration change, then reuse through Arc.
/// The owner must cancel old jobs and replace this snapshot when settings or keys change.
/// No disk, keychain, transcript reads or persistence occur on the request path.
pub struct PreparedClient {
    provider: AiProvider,
    key: Zeroizing<Vec<u8>>,
}

impl PreparedClient {
    pub fn prepare(provider: AiProvider) -> Result<Self, Failure> {
        if !supported(&provider) {
            return Err(Failure::Unavailable);
        }
        let key = Zeroizing::new(
            crate::ai_providers::load_api_key(&provider.id)
                .map_err(|_| Failure::Unavailable)?
                .unwrap_or_default(),
        );
        std::str::from_utf8(&key).map_err(|_| Failure::Unavailable)?;
        if key.is_empty() && provider.kind.requires_api_key() {
            return Err(Failure::Unavailable);
        }
        Ok(Self { provider, key })
    }

    /// Blocking; run on a background worker. A missing prediction is optional.
    pub fn request(
        &self,
        language: &str,
        messages: &[Message],
        cancelled: &AtomicBool,
    ) -> Prediction {
        if cancelled.load(Ordering::Acquire) {
            return Err(Failure::Cancelled);
        }
        let _slot = Slot::acquire().ok_or(Failure::Busy)?;
        request_with_key(
            &self.provider,
            std::str::from_utf8(&self.key).map_err(|_| Failure::Unavailable)?,
            language,
            messages,
            cancelled,
        )
    }
}

fn request_with_key(
    provider: &AiProvider,
    key: &str,
    language: &str,
    messages: &[Message],
    cancelled: &AtomicBool,
) -> Prediction {
    if cancelled.load(Ordering::Acquire) {
        return Err(Failure::Cancelled);
    }
    if key.is_empty() && provider.kind.requires_api_key() {
        return Err(Failure::Unavailable);
    }
    if !messages.iter().any(|message| message.role == "user" && !message.text.trim().is_empty()) {
        return Err(Failure::Unavailable);
    }
    let (url, body) = payload(provider, language, messages).ok_or(Failure::Unavailable)?;
    let uri = url.parse::<ureq::http::Uri>().map_err(|_| Failure::Unavailable)?;
    if !matches!(uri.scheme_str(), Some("http" | "https"))
        || uri.authority().ok_or(Failure::Unavailable)?.as_str().contains('@')
    {
        return Err(Failure::Unavailable);
    }
    let local = uri.host().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    let client = if local { &*LOCAL_CLIENT } else { &*CLIENT };
    let mut request = client.post(&url);
    let bearer = Zeroizing::new(format!("Bearer {key}"));
    request = match provider.kind {
        ProviderKind::Anthropic => {
            request.header("x-api-key", key).header("anthropic-version", "2023-06-01")
        },
        _ if key.is_empty() => request,
        _ => request.header("Authorization", bearer.as_str()),
    };
    let mut response = request.send_json(&body).map_err(|_| Failure::Network)?;
    if !response.status().is_success() {
        return Err(Failure::Network);
    }
    let value: Value = response
        .body_mut()
        .with_config()
        .limit(64 * 1024)
        .read_json()
        .map_err(|_| Failure::InvalidResponse)?;
    if cancelled.load(Ordering::Acquire) {
        return Err(Failure::Cancelled);
    }
    decode(&response_text(provider, &value).ok_or(Failure::InvalidResponse)?)
        .map_err(|_| Failure::InvalidResponse)
}

fn response_text(provider: &AiProvider, value: &Value) -> Option<String> {
    match provider.kind {
        ProviderKind::Anthropic => {
            if value["stop_reason"] != "end_turn" {
                return None;
            }
            let content = value["content"].as_array()?;
            let mut text = content.iter().filter(|v| v["type"] == "text");
            let result = text.next()?["text"].as_str()?.to_owned();
            text.next().is_none().then_some(result)
        },
        _ if provider.wire_api == "responses" => {
            if value["status"] != "completed" {
                return None;
            }
            let output = value["output"].as_array()?;
            let mut messages = output.iter().filter(|v| v["type"] == "message");
            let message = messages.next()?;
            if messages.next().is_some() {
                return None;
            }
            let content = message["content"].as_array()?;
            if content.len() != 1 || content[0]["type"] != "output_text" {
                return None;
            }
            Some(content[0]["text"].as_str()?.to_owned())
        },
        _ => {
            let choices = value["choices"].as_array()?;
            if choices.len() != 1
                || choices[0]["finish_reason"] != "stop"
                || !choices[0]["message"]["refusal"].is_null()
            {
                return None;
            }
            Some(choices[0]["message"]["content"].as_str()?.to_owned())
        },
    }
}

fn payload(provider: &AiProvider, language: &str, messages: &[Message]) -> Option<(String, Value)> {
    if !supported(provider) {
        return None;
    }
    let input = serde_json::to_string(&json!({
        "language": language,
        "conversation": context(messages),
        "user_voice_examples": voice_examples(messages),
    }))
    .ok()?;
    let reasoning_model = provider.model.starts_with("gpt-5")
        || provider.model.starts_with("gpt-6")
        || provider.model.starts_with("o3")
        || provider.model.starts_with("o4");
    let (route, mut body) = if provider.kind == ProviderKind::Anthropic {
        (
            "messages",
            json!({"model":provider.model,"max_tokens":512,"system":INSTRUCTIONS,
            "messages":[{"role":"user","content":input}]}),
        )
    } else if provider.wire_api == "responses" {
        (
            "responses",
            json!({"model":provider.model,"store":false,"max_output_tokens":512,
            "instructions":INSTRUCTIONS,"input":input}),
        )
    } else {
        let mut body = json!({"model":provider.model,
            "messages":[{"role":"system","content":INSTRUCTIONS},{"role":"user","content":input}]});
        body[if reasoning_model { "max_completion_tokens" } else { "max_tokens" }] = json!(512);
        ("chat/completions", body)
    };
    if reasoning_model && provider.kind != ProviderKind::Anthropic {
        if provider.wire_api == "responses" {
            body["reasoning"] = json!({"effort":"low"});
        } else {
            body["reasoning_effort"] = json!("low");
        }
    }
    let base = provider.base_url.trim().trim_end_matches('/');
    let url = if provider.full_url { base.to_owned() } else { format!("{base}/{route}") };
    Some((url, body))
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
