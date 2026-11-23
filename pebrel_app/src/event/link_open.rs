//! The legacy event loop delegates potentially blocking path metadata/openers.
//! Completion is scoped to the requesting window; a closed window drops it.

use winit::{event_loop::EventLoopProxy, window::WindowId};

use super::{Event, EventType};
use crate::config::ui_config::Program;
use crate::file_uri::LegacyHintOutcome;
use crate::i18n::{Message, UiLanguage};

pub(super) fn open(
    command: Program,
    text: String,
    language: UiLanguage,
    proxy: EventLoopProxy<Event>,
    window: WindowId,
) {
    let errors = proxy.clone();
    let worker = std::thread::Builder::new().name("link-opener".into()).spawn(move || {
        let result =
            match crate::file_uri::handle_legacy_hint_command(&text, command.args(), language) {
                LegacyHintOutcome::Handled => Ok(()),
                LegacyHintOutcome::Failed(message) => Err(message),
                LegacyHintOutcome::SpawnCommand(args) => {
                    crate::daemon::spawn_detached(command.program(), &args).map_err(|error| {
                        language.format(
                            Message::CommonLinkOpenUrlFailed,
                            &[("error", &error.to_string())],
                        )
                    })
                },
            };
        if let Err(message) = result {
            let _ = proxy.send_event(Event::new(EventType::LinkOpenFailed(message), window));
        }
    });
    if let Err(error) = worker {
        let message =
            language.format(Message::CommonLinkOpenUrlFailed, &[("error", &error.to_string())]);
        let _ = errors.send_event(Event::new(EventType::LinkOpenFailed(message), window));
    }
}
