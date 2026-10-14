//! Clipboard feedback for the two plain-text saved-command editor fields.
//!
//! The pinned InputState's copy/cut methods are private and emit no event. Its
//! public selection and EntityInputHandler edit APIs retain selection, IME,
//! validation and undo ownership in the component. These fields have no
//! completion provider, so a public replacement has the same edit behavior as
//! the component's silent cut. No clipboard content is read for confirmation.

use super::*;
use gpui::EntityInputHandler as _;
use gpui_component::input::{Copy, Cut};

enum Operation {
    Copy,
    Cut,
}

pub(super) fn editor_clipboard_input(input: Input, state: &Entity<InputState>) -> gpui::Div {
    let copy = state.clone();
    let cut = state.clone();
    div()
        .w_full()
        .capture_action(move |_: &Copy, window, cx| {
            cx.stop_propagation();
            clipboard_action(&copy, Operation::Copy, window, cx);
        })
        .capture_action(move |_: &Cut, window, cx| {
            cx.stop_propagation();
            clipboard_action(&cut, Operation::Cut, window, cx);
        })
        .child(input)
}

fn clipboard_action(
    input: &Entity<InputState>,
    operation: Operation,
    window: &mut Window,
    cx: &mut App,
) {
    let selected = input.read(cx).selected_value();
    if selected.is_empty() {
        return;
    }
    let before = matches!(operation, Operation::Cut).then(|| input.read(cx).value());
    // GPUI's clipboard API accepts the write synchronously; it has no OS error result.
    cx.write_to_clipboard(ClipboardItem::new_string(selected.to_string()));
    if let Some(before) = before {
        input.update(cx, |state, cx| state.replace_text_in_range(None, "", window, cx));
        if input.read(cx).value() == before {
            return;
        }
    }
    let language = crate::gpui_shell::config::ui_language(cx);
    let message = match operation {
        Operation::Copy => crate::i18n::Message::CommandsInputCopied,
        Operation::Cut => crate::i18n::Message::CommandsInputCut,
    };
    crate::gpui_shell::toast::toast(
        window,
        cx,
        crate::gpui_shell::toast::ToastKind::Info,
        language.text(message),
    );
}
