use super::Term;
use super::grid_content::Scrollback;

#[derive(Default)]
pub(super) struct PtyStartup {
    /// The side-loaded ConPTY host already received its initial DA1 reply on conin.
    pub(super) primed_da1: bool,
    pub(super) scrollback: Option<Scrollback>,
}

impl<T> Term<T> {
    pub(crate) fn suppress_primed_da1(&mut self) {
        self.pty_startup.primed_da1 = true;
    }

    /// ConPTY can erase saved lines before the replacement shell's first prompt.
    pub fn seed_scrollback(&mut self, scrollback: Scrollback) {
        if cfg!(windows) {
            self.pty_startup.scrollback = Some(scrollback);
        } else {
            self.install_scrollback(scrollback);
        }
    }

    pub(super) fn finish_scrollback_startup(&mut self) {
        if let Some(snapshot) = self.pty_startup.scrollback.take() {
            self.install_scrollback(snapshot);
        }
    }
}
