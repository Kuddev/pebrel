//! Mouse negotiation and coordinate lifetime, shared by all frontends.
use super::{Event, EventListener, NamedPrivateMode, Term, TermMode};

impl<T> Term<T> {
    /// A held gesture is valid only in the generation in which its press was sent.
    /// Sampling mode bits alone misses disable/re-enable within one parser batch.
    pub fn mouse_input_generation(&self) -> u64 {
        self.mouse_input_generation
    }

    pub(super) fn invalidate_mouse_input(&mut self) {
        self.mouse_input_generation = self.mouse_input_generation.wrapping_add(1);
    }
}

impl<T: EventListener> Term<T> {
    pub(super) fn set_mouse_mode(&mut self, mode: NamedPrivateMode, enabled: bool) {
        let (flag, exclusive) = match mode {
            NamedPrivateMode::ReportMouseClicks => {
                (TermMode::MOUSE_REPORT_CLICK, TermMode::MOUSE_MODE)
            },
            NamedPrivateMode::ReportCellMouseMotion => (TermMode::MOUSE_DRAG, TermMode::MOUSE_MODE),
            NamedPrivateMode::ReportAllMouseMotion => {
                (TermMode::MOUSE_MOTION, TermMode::MOUSE_MODE)
            },
            NamedPrivateMode::SgrMouse => (TermMode::SGR_MOUSE, TermMode::UTF8_MOUSE),
            NamedPrivateMode::Utf8Mouse => (TermMode::UTF8_MOUSE, TermMode::SGR_MOUSE),
            _ => unreachable!("only mouse modes are delegated here"),
        };
        let previous = self.mode;
        if enabled {
            self.mode.remove(exclusive);
        }
        self.mode.set(flag, enabled);
        // Reasserting 1000h, 1002h, 1003h on redraw keeps an active press.
        // Leaving/re-entering mouse reporting or changing encoding expires it.
        if previous.intersects(TermMode::MOUSE_MODE) != self.mode.intersects(TermMode::MOUSE_MODE)
            || (previous ^ self.mode).intersects(TermMode::SGR_MOUSE | TermMode::UTF8_MOUSE)
        {
            self.invalidate_mouse_input();
        }
        if flag.intersects(TermMode::MOUSE_MODE) {
            self.event_proxy.send_event(Event::MouseCursorDirty);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::VoidListener;
    use crate::term::{Config, test::TermSize};
    use crate::vte::ansi::Processor;

    #[test]
    fn mouse_generation_observes_negotiation_across_parser_chunks() {
        let wire = b"\x1b[?1002h\x1b[?1006h\x1b[?1002l\x1b[?1002h";
        for chunk_size in 1..=wire.len() {
            let mut term = Term::new(Config::default(), &TermSize::new(80, 24), VoidListener);
            let mut parser: Processor = Processor::new();
            for chunk in wire.chunks(chunk_size) {
                parser.advance(&mut term, chunk);
            }
            assert_eq!(term.mouse_input_generation(), 4);
            assert!(term.mode().contains(TermMode::MOUSE_DRAG | TermMode::SGR_MOUSE));
            let generation = term.mouse_input_generation();
            parser.advance(&mut term, b"\x1b[?1002h\x1b[?1006h\x1b[?1000l");
            assert_eq!(term.mouse_input_generation(), generation, "idempotent modes are stable");
            parser.advance(&mut term, b"\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h");
            assert_eq!(
                term.mouse_input_generation(),
                generation,
                "active tracking may be reasserted"
            );
            parser.advance(&mut term, b"\x1b[?1005h");
            assert!(term.mode().contains(TermMode::UTF8_MOUSE));
            assert!(!term.mode().contains(TermMode::SGR_MOUSE));
            assert_ne!(term.mouse_input_generation(), generation);
        }
    }

    #[test]
    fn screen_reset_and_resize_expire_mouse_coordinates() {
        let mut term = Term::new(Config::default(), &TermSize::new(80, 24), VoidListener);
        let mut parser: Processor = Processor::new();
        for wire in [b"\x1b[?1049h".as_slice(), b"\x1b[?1049l", b"\x1bc"] {
            let generation = term.mouse_input_generation();
            parser.advance(&mut term, wire);
            assert_ne!(term.mouse_input_generation(), generation);
        }
        let generation = term.mouse_input_generation();
        term.resize(TermSize::new(80, 24));
        assert_eq!(term.mouse_input_generation(), generation);
        term.resize(TermSize::new(40, 12));
        assert_ne!(term.mouse_input_generation(), generation);
    }
}
