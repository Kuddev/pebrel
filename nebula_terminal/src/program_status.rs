//! OSC 7501 revision 0.4: bounded, per-terminal program facts.
//! Presentation and Agent identity remain the consumer's responsibility.

use std::collections::VecDeque;

use base64::Engine as _;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

pub const FEATURE_REPLY: &str =
    "\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth\x1b\\";
const MAX_RECORDS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockedKind {
    Permission,
    Question,
    Auth,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Empty is the root; all other ids are validated hierarchical paths.
    pub id: String,
    pub state: State,
    pub app: Option<String>,
    pub kind: Option<BlockedKind>,
    pub progress: Option<u8>,
    pub title: Option<String>,
    pub message: Option<String>,
    /// The terminal may stop presenting a result after user acknowledgement.
    /// Keep the record so children can still inherit its app identity.
    pub result_acknowledged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    Query,
    Clear(String),
    Replace(Record),
}

/// Oldest update first. Reading a record does not refresh its lifetime or LRU.
#[derive(Debug, Default)]
pub struct Records {
    entries: VecDeque<Record>,
    seen: bool,
}

impl Records {
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Record> {
        self.entries.iter()
    }

    /// OSC 9;4 must not overwrite richer reports, even after an explicit clear.
    pub fn seen(&self) -> bool {
        self.seen
    }

    pub fn app_for<'a>(&'a self, record: &'a Record) -> Option<&'a str> {
        if let Some(app) = record.app.as_deref() {
            return Some(app);
        }
        let mut id = record.id.as_str();
        while !id.is_empty() {
            id = id.rsplit_once('/').map_or("", |(parent, _)| parent);
            if let Some(app) = self
                .entries
                .iter()
                .find(|entry| entry.id == id)
                .and_then(|entry| entry.app.as_deref())
            {
                return Some(app);
            }
        }
        None
    }

    pub(crate) fn apply(&mut self, report: Report) -> bool {
        if report == Report::Query {
            return false;
        }
        let first = !std::mem::replace(&mut self.seen, true);
        match report {
            Report::Clear(id) => {
                let count = self.entries.len();
                self.entries.retain(|entry| {
                    !(id.is_empty()
                        || entry.id == id
                        || entry.id.strip_prefix(&id).is_some_and(|tail| tail.starts_with('/')))
                });
                first || count != self.entries.len()
            },
            Report::Replace(record) => {
                if self.entries.back() == Some(&record) {
                    return first;
                }
                if let Some(index) = self.entries.iter().position(|entry| entry.id == record.id) {
                    self.entries.remove(index);
                } else if self.entries.len() == MAX_RECORDS {
                    self.entries.pop_front();
                }
                self.entries.push_back(record);
                true
            },
            Report::Query => unreachable!(),
        }
    }

    pub(crate) fn command_ended(&mut self) -> bool {
        let count = self.entries.len();
        self.entries.retain(|entry| matches!(entry.state, State::Done | State::Error));
        count != self.entries.len()
    }

    pub(crate) fn acknowledge_results(&mut self) -> bool {
        let mut changed = false;
        for entry in &mut self.entries {
            if matches!(entry.state, State::Done | State::Error) && !entry.result_acknowledged {
                entry.result_acknowledged = true;
                changed = true;
            }
        }
        changed
    }

    pub(crate) fn reset(&mut self) -> bool {
        let changed = self.seen;
        *self = Self::default();
        changed
    }
}

fn identifier(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.+-".contains(&b))
}

fn text(value: &str, encoded_limit: usize, decoded_limit: usize) -> Option<String> {
    if value.len() > encoded_limit {
        return None;
    }
    let engine = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
    );
    let bytes = engine.decode(value).ok()?;
    if bytes.len() > decoded_limit {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    (!text.chars().any(char::is_control)).then_some(text)
}

/// Validate every pair before publishing anything, including overwritten keys.
pub(crate) fn parse(body: &[u8]) -> Option<Report> {
    if body.trim_ascii() == b"?" {
        return Some(Report::Query);
    }
    // A reflected detection reply must never create a record or another reply.
    if body.trim_ascii_start().starts_with(b"?") {
        return None;
    }
    let (mut state, mut id, mut app, mut kind, mut progress) = (None, "", None, None, None);
    let (mut message, mut title) = (None, None);
    for pair in body.split(|byte| *byte == b':') {
        let Some(eq) = pair.iter().position(|byte| *byte == b'=') else { continue };
        let key = pair[..eq].trim_ascii();
        let raw = pair[eq + 1..].trim_ascii();
        if key.len() > 16 {
            return None;
        }
        if key.is_empty() || !key.iter().all(u8::is_ascii_lowercase) {
            continue;
        }
        let value = std::str::from_utf8(raw).ok();
        // Never let a malformed id address the root record.
        if key == b"id"
            && !value.is_some_and(|id| {
                id.len() <= 128
                    && id.split('/').count() <= 8
                    && id.split('/').all(|part| part.len() <= 32 && identifier(part))
            })
        {
            return None;
        }
        let limit = match key {
            b"app" => 32,
            b"msg" => 2732,
            b"title" => 256,
            _ => usize::MAX,
        };
        if raw.len() > limit {
            return None;
        }
        if !raw.iter().all(|b| b.is_ascii_alphanumeric() || b"_.,+/=-".contains(b)) {
            continue;
        }
        let value = value?;
        match key {
            b"state" => state = Some(value),
            b"id" => id = value,
            b"app" => app = identifier(value).then(|| value.to_owned()),
            b"kind" => {
                kind = match value {
                    "permission" => Some(BlockedKind::Permission),
                    "question" => Some(BlockedKind::Question),
                    "auth" => Some(BlockedKind::Auth),
                    _ => None,
                }
            },
            b"progress" => {
                progress = value
                    .bytes()
                    .all(|b| b.is_ascii_digit())
                    .then(|| value.parse::<u8>().ok().filter(|v| *v <= 100))
                    .flatten()
            },
            b"msg" => message = Some(text(value, 2732, 2048)?),
            b"title" => title = Some(text(value, 256, 192)?),
            _ => {},
        }
    }
    let state = match state? {
        "idle" => State::Idle,
        "working" => State::Working,
        "done" => State::Done,
        "blocked" => State::Blocked,
        "error" => State::Error,
        "clear" => return Some(Report::Clear(id.to_owned())),
        _ => return None,
    };
    Some(Report::Replace(Record {
        id: id.to_owned(),
        state,
        app,
        title,
        message,
        result_acknowledged: false,
        kind: (state == State::Blocked).then_some(kind).flatten(),
        progress: matches!(state, State::Working | State::Blocked).then_some(progress).flatten(),
    }))
}

#[cfg(test)]
mod tests;
