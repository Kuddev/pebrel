//! Identity-bound access to the conversation already running in a pane.

use serde::{Deserialize, Serialize};

use super::{ApiError, ApiRequest, RuntimeCommand, RuntimeKey, RuntimeKeyModifiers, parse_params};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub kind: String,
    pub session_id: String,
    /// First reads resolve the epoch; all mutations require the returned value.
    pub epoch: Option<u64>,
}

impl Identity {
    pub fn matches(&self, current: &Self) -> bool {
        self.kind == current.kind
            && self.session_id == current.session_id
            && self.epoch.is_none_or(|epoch| current.epoch == Some(epoch))
    }
}

#[derive(Clone, Debug)]
pub enum Request {
    Read { identity: Identity, before: Option<u64>, revision: Option<String> },
    Send { identity: Identity, text: String },
    Choose { identity: Identity, prompt_id: String, option: usize },
    Key { identity: Identity, key: RuntimeKey, modifiers: RuntimeKeyModifiers },
}

impl Request {
    pub fn identity(&self) -> &Identity {
        match self {
            Self::Read { identity, .. }
            | Self::Send { identity, .. }
            | Self::Choose { identity, .. }
            | Self::Key { identity, .. } => identity,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    window_id: u64,
    pane_id: u64,
    identity: Identity,
    before: Option<u64>,
    revision: Option<String>,
    text: Option<String>,
    prompt_id: Option<String>,
    option: Option<usize>,
    key: Option<String>,
}

pub(super) fn parse(request: &ApiRequest) -> Result<RuntimeCommand, ApiError> {
    let p: Params = parse_params(&request.params)?;
    if p.window_id == 0
        || p.pane_id == 0
        || !matches!(p.identity.kind.as_str(), "codex" | "claude")
        || p.identity.session_id.is_empty()
        || p.identity.session_id.len() > 128
        || p.identity.session_id.chars().any(char::is_control)
        || p.revision.as_ref().is_some_and(|s| s.len() > 128)
    {
        return Err(ApiError::invalid_params(
            "a current pane and native conversation identity are required",
        ));
    }
    let read_only =
        p.text.is_none() && p.prompt_id.is_none() && p.option.is_none() && p.key.is_none();
    let request = match request.method.as_str() {
        "conversation.read" if read_only => {
            Request::Read { identity: p.identity, before: p.before, revision: p.revision }
        },
        "conversation.send"
            if p.identity.epoch.is_some()
                && p.before.is_none()
                && p.revision.is_none()
                && p.prompt_id.is_none()
                && p.option.is_none()
                && p.key.is_none() =>
        {
            let text = p.text.ok_or_else(|| ApiError::invalid_params("text is required"))?;
            super::validate_chat_message(&text)?;
            Request::Send { identity: p.identity, text }
        },
        "conversation.choose"
            if p.identity.epoch.is_some()
                && p.before.is_none()
                && p.revision.is_none()
                && p.text.is_none()
                && p.key.is_none() =>
        {
            let prompt_id = p
                .prompt_id
                .filter(|id| id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or_else(|| ApiError::invalid_params("the current prompt_id is required"))?;
            let option = p
                .option
                .filter(|index| *index < 9)
                .ok_or_else(|| ApiError::invalid_params("a visible option is required"))?;
            Request::Choose { identity: p.identity, prompt_id, option }
        },
        "conversation.key"
            if p.identity.epoch.is_some()
                && p.before.is_none()
                && p.revision.is_none()
                && p.text.is_none()
                && p.prompt_id.is_none()
                && p.option.is_none() =>
        {
            let key = match p.key.as_deref() {
                Some("Ctrl+C") => RuntimeKey::C,
                Some("Esc") => RuntimeKey::Escape,
                Some("Tab") => RuntimeKey::Tab,
                Some("←") => RuntimeKey::Left,
                Some("→") => RuntimeKey::Right,
                Some("↑") => RuntimeKey::Up,
                Some("↓") => RuntimeKey::Down,
                _ => {
                    return Err(ApiError::invalid_params(
                        "a visible conversation shortcut is required",
                    ));
                },
            };
            let modifiers =
                RuntimeKeyModifiers { control: key == RuntimeKey::C, ..Default::default() };
            Request::Key { identity: p.identity, key, modifiers }
        },
        _ => return Err(ApiError::invalid_params("invalid conversation operation fields")),
    };
    Ok(RuntimeCommand::Conversation { window_id: p.window_id, pane_id: p.pane_id, request })
}
