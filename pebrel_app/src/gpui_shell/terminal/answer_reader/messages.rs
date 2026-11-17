use super::{IMAGE_BUDGET, Message, UiLanguage, document};

pub(super) enum ImageFailure {
    MissingDirectory,
    TotalBudget,
    Read(document::ImageError),
    Decode(String),
}

impl ImageFailure {
    pub(super) fn localized(&self, language: UiLanguage) -> String {
        match self {
            Self::MissingDirectory => {
                language.text(Message::ReaderImageMissingDirectory).to_owned()
            },
            Self::TotalBudget => language.format(
                Message::ReaderImageTotalBudget,
                &[("limit", &(IMAGE_BUDGET / 1024 / 1024).to_string())],
            ),
            Self::Read(error) => error.localized(language).to_owned(),
            Self::Decode(error) => {
                language.format(Message::ReaderImageDecodeFailed, &[("error", error)])
            },
        }
    }
}
