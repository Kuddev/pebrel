//! Own WinRT notification objects until their bounded retention expires.
//! A successful Show RPC alone does not keep Activated callbacks alive.

use std::collections::VecDeque;
use std::sync::{OnceLock, mpsc};

use ::windows::Data::Xml::Dom::XmlDocument;
use ::windows::Foundation::TypedEventHandler;
use ::windows::UI::Notifications::{
    ToastActivatedEventArgs, ToastNotification, ToastNotificationManager,
};
use ::windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use ::windows::core::{IInspectable, Interface};

use super::{ToastAction, ToastActivation, win};

struct Request {
    title: String,
    body: String,
    open: Option<ToastActivation>,
    actions: Vec<ToastAction>,
}

pub(super) fn enqueue(
    title: &str,
    body: &str,
    open: Option<ToastActivation>,
    actions: Vec<ToastAction>,
) {
    static WORKER: OnceLock<Option<mpsc::SyncSender<Request>>> = OnceLock::new();
    let worker = WORKER.get_or_init(|| {
        let (sender, receiver) = mpsc::sync_channel(32);
        std::thread::Builder::new()
            .name("pebrel-native-notifications".into())
            .spawn(move || run(receiver))
            .map(|_| sender)
            .map_err(|error| log::warn!("notify: could not start native worker: {error}"))
            .ok()
    });
    let Some(worker) = worker else { return };
    if let Err(error) =
        worker.try_send(Request { title: title.to_owned(), body: body.to_owned(), open, actions })
    {
        log::warn!("notify: native queue unavailable: {error}");
    }
}

fn run(receiver: mpsc::Receiver<Request>) {
    // All WinRT objects live on this MTA thread; callbacks only enqueue app events.
    if let Err(error) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
        log::warn!("notify: COM initialization failed: {error}");
        return;
    }
    let mut live = VecDeque::new();
    for request in receiver {
        match show(request) {
            Ok(notification) => {
                if live.len() == 64 {
                    live.pop_front();
                }
                live.push_back(notification);
            },
            Err(error) => log::warn!("notify: native delivery failed: {error}"),
        }
    }
    drop(live);
    unsafe { CoUninitialize() };
}

fn show(request: Request) -> ::windows::core::Result<ToastNotification> {
    win::ensure_aumid();
    let document = XmlDocument::new()?;
    document.LoadXml(&xml(&request).into())?;
    let notification = ToastNotification::CreateToastNotification(&document)?;
    notification.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(
        move |_, args| {
            let argument = args
                .as_ref()
                .and_then(|args| args.cast::<ToastActivatedEventArgs>().ok())
                .and_then(|args| args.Arguments().ok())
                .map(|value| value.to_string())
                .unwrap_or_default();
            if argument.is_empty() {
                if let Some(open) = &request.open {
                    open();
                }
            } else if let Some(index) =
                argument.strip_prefix("choice-").and_then(|id| id.parse::<usize>().ok())
                && let Some(action) = request.actions.get(index)
            {
                (action.activate)();
            }
            Ok(())
        },
    ))?;
    ToastNotificationManager::CreateToastNotifierWithId(&win::AUMID.into())?.Show(&notification)?;
    Ok(notification)
}

fn xml(request: &Request) -> String {
    let mut xml = format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text>",
        escape(&request.title),
        escape(&request.body),
    );
    if let Some(path) = win::icon_path() {
        xml.push_str(&format!(
            "<image placement=\"appLogoOverride\" src=\"{}\"/>",
            escape(&path.to_string_lossy())
        ));
    }
    xml.push_str("</binding></visual>");
    if !request.actions.is_empty() {
        xml.push_str("<actions>");
        for (index, action) in request.actions.iter().take(5).enumerate() {
            xml.push_str(&format!("<action content=\"{}\" arguments=\"choice-{index}\" activationType=\"foreground\"/>", escape(&action.label)));
        }
        xml.push_str("</actions>");
    }
    xml.push_str("</toast>");
    xml
}

fn escape(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_controls_and_markup_cannot_break_the_toast_document() {
        assert_eq!(escape("<tool>\u{1b}\u{0}\"&'"), "&lt;tool&gt;&quot;&amp;&apos;");
    }
}
