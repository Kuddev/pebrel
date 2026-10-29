//! Next-message prediction contracts. Suggestions never own or reconstruct a CLI draft.
//! Inspired by Codex's September 2026 TUI prompt suggestions; see the ownership note.

use serde::{Deserialize, Serialize};

pub mod client;

pub const MAX_SUGGESTION_CHARS: usize = 240;
const MAX_CONTEXT_CHARS: usize = 12_000;
const MAX_MESSAGE_CHARS: usize = 3_000;

// OpenAI Codex, Apache-2.0; source and immutable revision in the ownership note.
// Preserve the upstream prompt verbatim. The schema suffix is likewise from
// tui/src/app/prompt_suggestions.rs, not a local voice/personality policy.
pub const INSTRUCTIONS: &str = concat!(
    include_str!("../res/prompts/codex_prompt_suggestion.txt"),
    "\n\nReturn only JSON matching this schema: {\"type\":\"object\",\"properties\":{\"suggestion\":{\"type\":[\"string\",\"null\"]}},\"required\":[\"suggestion\"],\"additionalProperties\":false}."
);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Message {
    pub role: String,
    pub text: String,
}

/// Keep user wording as well as the answer; never send tools, hidden reasoning or whole logs.
pub fn context(messages: &[Message]) -> Vec<Message> {
    let mut remaining = MAX_CONTEXT_CHARS;
    let mut result = Vec::new();
    for message in messages.iter().rev().filter(|m| matches!(m.role.as_str(), "user" | "assistant"))
    {
        if result.len() == 8 || remaining == 0 {
            break;
        }
        let limit = remaining.min(MAX_MESSAGE_CHARS);
        let text = excerpt(&message.text, limit);
        if text.trim().is_empty() {
            continue;
        }
        remaining -= text.chars().count();
        result.push(Message { role: message.role.clone(), text });
    }
    result.reverse();
    result
}

/// Long answers often put results and remaining work at the end. Preserve both ends.
fn excerpt(text: &str, limit: usize) -> String {
    // Inspect at most limit + 1 characters; a giant pasted message must not cost
    // a full scan merely to keep a small head/tail excerpt.
    if text.char_indices().nth(limit).is_none() {
        return text.to_owned();
    }
    const GAP: &str = "\n[... omitted ...]\n";
    let available = limit.saturating_sub(GAP.chars().count());
    if available < 2 {
        return text.chars().rev().take(limit).collect::<Vec<_>>().into_iter().rev().collect();
    }
    let head = available / 2;
    let tail: String =
        text.chars().rev().take(available - head).collect::<Vec<_>>().into_iter().rev().collect();
    format!("{}{GAP}{tail}", text.chars().take(head).collect::<String>())
}

/// Separate bounded wording evidence so long assistant answers cannot crowd out
/// the user's voice. Native adapters supply actual submitted messages in memory.
/// This is transient request context, not a persisted personality/profile.
fn voice_examples(messages: &[Message]) -> Vec<String> {
    let mut remaining = 1600;
    let mut examples = Vec::new();
    for message in messages.iter().rev().filter(|message| message.role == "user") {
        if examples.len() == 6 || remaining == 0 {
            break;
        }
        let text = excerpt(&message.text, remaining.min(360));
        if text.trim().is_empty() || examples.contains(&text) {
            continue;
        }
        remaining -= text.chars().count();
        examples.push(text);
    }
    examples.reverse();
    examples
}

pub fn parse(response: &str) -> Option<String> {
    decode(response).ok().flatten()
}

fn decode(response: &str) -> Result<Option<String>, ()> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Prediction {
        Text(String),
        Abstain(()),
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Response {
        suggestion: Prediction,
    }
    // A bounded full next-message response, not a suffix: surrounding spaces may be trimmed.
    if response.len() > 4096 {
        return Err(());
    }
    let response: Response = serde_json::from_str(response).map_err(|_| ())?;
    let Prediction::Text(text) = response.suggestion else { return Ok(None) };
    if text.chars().any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}')) {
        return Err(());
    }
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_SUGGESTION_CHARS {
        return Err(());
    }
    Ok(Some(text.to_owned()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub agent: String,
    pub session: String,
    pub turn: String,
    pub input_revision: u64,
    pub provider: String,
    pub language: String,
}

#[derive(Default)]
pub struct State {
    generation: u64,
    handled_turn: Option<(String, String, String)>,
    current: Option<Identity>,
    candidate: Option<String>,
}

impl State {
    /// One request per completed turn. Dismissal does not turn into repeated paid requests.
    pub fn begin(&mut self, identity: Identity) -> Option<u64> {
        let turn = (identity.agent.clone(), identity.session.clone(), identity.turn.clone());
        if self.handled_turn.as_ref() == Some(&turn) {
            return None;
        }
        self.invalidate();
        self.handled_turn = Some(turn);
        self.current = Some(identity);
        Some(self.generation)
    }

    pub fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.current = None;
        self.candidate = None;
    }

    pub fn finish(&mut self, generation: u64, identity: &Identity, response: Option<String>) {
        if self.generation == generation && self.current.as_ref() == Some(identity) {
            self.candidate = response;
        }
    }

    pub fn candidate(&self, identity: &Identity, empty_editable: bool) -> Option<&str> {
        (empty_editable && self.current.as_ref() == Some(identity))
            .then_some(self.candidate.as_deref())
            .flatten()
    }

    pub fn take(&mut self, identity: &Identity, empty_editable: bool) -> Option<String> {
        self.candidate(identity, empty_editable)?;
        let text = self.candidate.take();
        self.invalidate();
        text
    }
}

#[cfg(test)]
#[path = "prompt_suggestions/tests.rs"]
mod tests;
