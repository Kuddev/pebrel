//! Title state and stack; private image replay tokens never become titles.

use super::*;

impl<T: EventListener> Term<T> {
    #[inline]
    pub(super) fn apply_title(&mut self, title: Option<String>) {
        if title.as_deref().is_some_and(|title| self.nebula_dispatch_inline_image(title)) {
            return;
        }
        trace!("Setting title to '{title:?}'");

        self.title.clone_from(&title);

        let title_event = match title {
            Some(title) => Event::Title(title),
            None => Event::ResetTitle,
        };

        self.event_proxy.send_event(title_event);
    }

    #[inline]
    pub(super) fn save_title(&mut self) {
        trace!("Pushing '{:?}' onto title stack", self.title);

        if self.title_stack.len() >= TITLE_STACK_MAX_DEPTH {
            let removed = self.title_stack.remove(0);
            trace!(
                "Removing '{removed:?}' from bottom of title stack that exceeds its maximum depth"
            );
        }

        self.title_stack.push(self.title.clone());
    }

    #[inline]
    pub(super) fn restore_title(&mut self) {
        trace!("Attempting to pop title from stack...");

        if let Some(popped) = self.title_stack.pop() {
            trace!("Title '{popped:?}' popped from stack");
            self.apply_title(popped);
        }
    }
}
