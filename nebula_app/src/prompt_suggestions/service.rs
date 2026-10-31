//! Per-runtime inference ownership. Native clients keep all editor state.
use std::collections::VecDeque;
use std::sync::{
    Arc, LazyLock, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};

use super::{Message, client::PreparedClient};

static CONFIG_REVISION: LazyLock<Arc<AtomicU64>> = LazyLock::new(|| Arc::new(AtomicU64::new(1)));

/// Called after application settings/provider/credential mutations succeed.
/// The next background IPC request refreshes the snapshot; UI never opens a keychain.
pub fn invalidate_config() {
    CONFIG_REVISION.fetch_add(1, Ordering::AcqRel);
}

#[derive(Clone, Serialize)]
pub struct Config {
    pub enabled: bool,
    pub model: String,
    pub language: String,
    pub loading_text: String,
}

struct Snapshot {
    revision: u64,
    config: Config,
    client: Option<PreparedClient>,
    cancelled: AtomicBool,
}

impl Snapshot {
    fn load(revision: u64) -> Self {
        let runtime = nebula_settings::RuntimeSettings::load();
        let settings = runtime.prompt_suggestions;
        let language = crate::i18n::LanguagePreference::from(runtime.language).resolved();
        let client = settings
            .enabled
            .then(|| {
                let mut provider = crate::ai_providers::load()
                    .providers
                    .into_iter()
                    .find(|provider| provider.id == settings.provider_id)?;
                provider.model.clone_from(&settings.model);
                PreparedClient::prepare(provider).ok()
            })
            .flatten();
        let config = Config {
            enabled: client.is_some(),
            loading_text: language.format(
                crate::i18n::Message::PromptSuggestionsPredicting,
                &[("model", &settings.model)],
            ),
            model: settings.model,
            language: language.code().to_owned(),
        };
        Self { revision, config, client, cancelled: AtomicBool::new(false) }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub agent: String,
    pub session: String,
    pub turn: String,
    pub revision: u64,
    pub messages: Vec<Message>,
}

impl Request {
    pub fn valid(&self) -> bool {
        matches!(self.agent.as_str(), "claude" | "pi" | "opencode")
            && [&self.session, &self.turn]
                .iter()
                .all(|id| !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))
            && !self.messages.is_empty()
            && self.messages.len() <= 24
            && self.messages.iter().all(|message| {
                matches!(message.role.as_str(), "user" | "assistant")
                    && message.text.len() <= 24_000
            })
    }
}

#[derive(Serialize)]
pub struct Response {
    pub suggestion: Option<String>,
    pub model: String,
}

#[derive(Default)]
struct Inner {
    snapshot: Option<Arc<Snapshot>>,
    turns: VecDeque<(String, String, String)>,
}

pub struct Service {
    /// A prediction-only token, never accepted by runtime control methods.
    pub token: String,
    inner: Mutex<Inner>,
    revision: Arc<AtomicU64>,
}

impl Default for Service {
    fn default() -> Self {
        let mut bytes = [0u8; 32];
        let token = if getrandom::fill(&mut bytes).is_ok() {
            bytes.iter().map(|byte| format!("{byte:02x}")).collect()
        } else {
            String::new() // Fail closed if OS entropy is unavailable.
        };
        Self { token, inner: Mutex::new(Inner::default()), revision: CONFIG_REVISION.clone() }
    }
}

impl Service {
    fn snapshot(&self) -> Arc<Snapshot> {
        let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        let revision = self.revision.load(Ordering::Acquire);
        if inner.snapshot.as_ref().is_none_or(|snapshot| snapshot.revision != revision) {
            if let Some(old) = &inner.snapshot {
                old.cancelled.store(true, Ordering::Release);
            }
            inner.snapshot = Some(Arc::new(Snapshot::load(revision)));
        }
        inner.snapshot.as_ref().unwrap().clone()
    }

    pub fn config(&self) -> Config {
        self.snapshot().config.clone()
    }

    pub fn predict(&self, request: Request) -> Response {
        let snapshot = self.snapshot();
        let mut response = Response { suggestion: None, model: snapshot.config.model.clone() };
        let Some(client) = &snapshot.client else { return response };
        if !request.valid() {
            return response;
        }
        {
            let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
            let turn = (request.agent, request.session, request.turn);
            // Native adapters invalidate input revisions. A changed revision must
            // never cause another paid prediction of the same completed turn.
            let _input_revision = request.revision;
            if inner.turns.contains(&turn) {
                return response;
            }
            inner.turns.push_back(turn);
            if inner.turns.len() > 128 {
                inner.turns.pop_front();
            }
        }
        let candidate = client
            .request(&snapshot.config.language, &request.messages, &snapshot.cancelled)
            .ok()
            .flatten();
        if snapshot.revision == self.revision.load(Ordering::Acquire)
            && !snapshot.cancelled.load(Ordering::Acquire)
        {
            response.suggestion = candidate;
        }
        response
    }
}

#[cfg(test)]
impl Service {
    pub(crate) fn test_disabled() -> Self {
        Self::test_snapshot(None)
    }

    fn test_snapshot(client: Option<PreparedClient>) -> Self {
        let service = Self { revision: Arc::new(AtomicU64::new(1)), ..Self::default() };
        service.inner.lock().unwrap().snapshot = Some(Arc::new(Snapshot {
            revision: service.revision.load(Ordering::Acquire),
            config: Config {
                enabled: client.is_some(),
                model: "fixture-model".into(),
                language: "zh-CN".into(),
                loading_text: "fixture-model 正在预测".into(),
            },
            client,
            cancelled: AtomicBool::new(false),
        }));
        service
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
