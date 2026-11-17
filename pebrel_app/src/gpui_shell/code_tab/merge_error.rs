use super::{MAX_CODE_BYTES, Message, UiLanguage};

/// Keep application-owned diagnostics semantic until the view renders them.
/// A language change can then update an error returned by an earlier task.
#[derive(Clone, Debug)]
pub(super) enum MergeError {
    Text(Message),
    Detail(Message, String),
    Document(Message, Message),
    Limit(Message),
    Raw(String),
}

impl From<Message> for MergeError {
    fn from(message: Message) -> Self {
        Self::Text(message)
    }
}

impl MergeError {
    pub(super) fn detail(message: Message, error: impl ToString) -> Self {
        Self::Detail(message, error.to_string())
    }

    pub(super) fn render(&self, language: UiLanguage) -> String {
        match self {
            Self::Text(message) => language.text(*message).to_owned(),
            Self::Detail(message, error) => language.format(*message, &[("error", error)]),
            Self::Document(message, part) => language.format(
                *message,
                &[
                    ("part", language.text(*part)),
                    ("limit", &(MAX_CODE_BYTES / 1024 / 1024).to_string()),
                ],
            ),
            Self::Limit(message) => {
                language.format(*message, &[("limit", &(MAX_CODE_BYTES / 1024 / 1024).to_string())])
            },
            Self::Raw(message) => message.clone(),
        }
    }
}
