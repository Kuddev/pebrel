//! ZMODEM session cancellation through the real stream processor (issue #806).

use super::*;
use crate::term::{Config, test::TermSize};
use crate::zmodem::{ABORT_SEQUENCE, ZmodemOffer};

#[derive(Clone, Default)]
struct Recorder(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl EventListener for Recorder {
    fn send_event(&self, event: Event) {
        let label = match event {
            Event::PtyWrite(reply) => format!("write:{}", reply.escape_default()),
            Event::ZmodemCancelled(offer) => format!("cancelled:{offer:?}"),
            _ => return,
        };
        self.0.lock().unwrap().push(label);
    }
}

fn screen_text<T: EventListener>(terminal: &Term<T>) -> String {
    let snapshot = crate::render::RenderSnapshot::capture(
        terminal,
        &crate::render::SnapshotConfig { rows: 24, cols: 80 },
    );
    snapshot
        .segments
        .iter()
        .flat_map(|segment| &segment.cells)
        .map(|cell| cell.text.as_str())
        .collect()
}

fn expected(offer: ZmodemOffer) -> Vec<String> {
    vec![format!("write:{}", ABORT_SEQUENCE.escape_default()), format!("cancelled:{offer:?}")]
}

#[test]
fn remote_rz_is_cancelled_once_at_every_chunk_boundary() {
    let wire = b"[root@host ~]# rz -y\r\nrz waiting to receive.**\x18B0100000023be50\r\x8a\x11";
    for split in 0..=wire.len() {
        let recorder = Recorder::default();
        let mut terminal = Term::new(Config::default(), &TermSize::new(80, 24), recorder.clone());
        let mut stream = StreamProcessor::default();
        stream.feed(&mut terminal, &recorder, &wire[..split]);
        stream.feed(&mut terminal, &recorder, &wire[split..]);
        assert_eq!(*recorder.0.lock().unwrap(), expected(ZmodemOffer::Upload), "split at {split}");
        // The snapshot omits blank cells; this is the text from the issue screenshot.
        let text = screen_text(&terminal);
        assert!(text.contains("receive.**B0100000023be50"), "output stays visible: {text:?}");
    }
}

#[test]
fn remote_sz_and_conpty_rendered_headers_are_cancelled() {
    for (wire, offer) in [
        (b"rz\r**\x18B00000000000000\r\x8a\x11".as_slice(), ZmodemOffer::Download),
        (b"rz waiting to receive.**B0100000023be50\r\n", ZmodemOffer::Upload),
    ] {
        let recorder = Recorder::default();
        let mut terminal = Term::new(Config::default(), &TermSize::new(80, 24), recorder.clone());
        StreamProcessor::default().feed(&mut terminal, &recorder, wire);
        assert_eq!(*recorder.0.lock().unwrap(), expected(offer));
    }
}

#[test]
fn ordinary_output_is_untouched_and_each_new_session_is_cancelled() {
    let recorder = Recorder::default();
    let mut terminal = Term::new(Config::default(), &TermSize::new(80, 24), recorder.clone());
    let mut stream = StreamProcessor::default();
    stream.feed(&mut terminal, &recorder, b"**bold** ***B0100000023be51 \x18\x18\r\n");
    assert!(recorder.0.lock().unwrap().is_empty());
    for _ in 0..2 {
        stream.feed(&mut terminal, &recorder, b"**\x18B0100000023be50\r\x8a\x11");
    }
    let mut twice = expected(ZmodemOffer::Upload);
    twice.extend(expected(ZmodemOffer::Upload));
    assert_eq!(*recorder.0.lock().unwrap(), twice);
}
