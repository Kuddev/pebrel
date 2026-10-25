use std::sync::{Arc, Mutex};

use nebula_terminal::Term;
use nebula_terminal::event::{Event, EventListener};
use nebula_terminal::event_loop::StreamProcessor;
use nebula_terminal::osc_cwd::{CwdSniffer, OscEvent};
use nebula_terminal::program_status::{FEATURE_REPLY, State};
use nebula_terminal::term::test::TermSize;
use vte::ansi::Handler;

#[derive(Clone, Default)]
struct Listener(Arc<Mutex<Vec<Event>>>);
impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        self.0.lock().unwrap().push(event);
    }
}

#[test]
fn every_chunk_boundary_and_terminator_preserves_one_complete_report() {
    for end in ["\x07", "\x1b\\"] {
        let wire =
            format!("before\x1b]7501;state=blocked:id=task:kind=permission:msg=VGVzdA=={end}after");
        for split in 0..=wire.len() {
            let mut sniffer = CwdSniffer::default();
            let mut events = sniffer.feed(&wire.as_bytes()[..split]);
            assert!(sniffer.feed(b"").is_empty());
            events.extend(sniffer.feed(&wire.as_bytes()[split..]));
            assert_eq!(events.len(), 1, "{split}");
            assert!(matches!(events[0].1, OscEvent::ProgramStatus(_)));
        }
        let mut sniffer = CwdSniffer::default();
        assert_eq!(wire.bytes().flat_map(|b| sniffer.feed(&[b])).count(), 1);
    }
    assert!(CwdSniffer::default().feed(b"\x1b]7501;state=working").is_empty());
}

#[test]
fn whole_sequence_budget_includes_both_terminators_and_drops_overflow() {
    for end in ["\x07", "\x1b\\"] {
        let prefix = "\x1b]7501;state=working:extension=";
        for size in [4095, 4096, 4097, 100_000] {
            let wire = format!("{prefix}{}{end}", "a".repeat(size - prefix.len() - end.len()));
            let mut sniffer = CwdSniffer::default();
            assert_eq!(sniffer.feed(wire.as_bytes()).len(), usize::from(size <= 4096));
            assert_eq!(sniffer.feed(b"\x1b]7501;state=done\x07").len(), 1);
        }
    }
}

#[test]
fn cancelled_report_cannot_reappear_after_ris_at_any_chunk_boundary() {
    for end in ["\x07", "\x1b\\"] {
        let wire = format!("\x1b]7501;state=working:ignored=\x1bc{end}");
        for split in 0..=wire.len() {
            let listener = Listener::default();
            let mut term = Term::new(Default::default(), &TermSize::new(80, 24), listener.clone());
            let mut stream = StreamProcessor::default();
            stream.feed(&mut term, &listener, b"\x1b]7501;state=blocked:id=old\x07");
            stream.feed(&mut term, &listener, &wire.as_bytes()[..split]);
            stream.feed(&mut term, &listener, &wire.as_bytes()[split..]);
            assert_eq!(term.program_statuses().iter().count(), 0, "{split}, {end:?}");
            assert!(!term.program_statuses().seen(), "{split}, {end:?}");
            stream.feed(&mut term, &listener, b"\x1b]7501;state=done:id=fresh\x07");
            let record = term.program_statuses().iter().next().unwrap();
            assert_eq!(record.id, "fresh");
            assert_eq!(record.state, State::Done);
        }
    }
}

#[test]
fn cancelled_reports_neither_apply_nor_swallow_following_reports() {
    for cancel in ["\x18", "\x1a", "\x1b[0m", "\x1b", "\x1b\x00"] {
        for following in ["", "\x1b]7501;state=done:id=fresh\x1b\\"] {
            // A later terminator must not revive the aborted report. With no
            // terminator in between, a new ESC ] must start its own report.
            for separator in ["", "\x07", "\x1b\\"] {
                let wire = format!(
                    "\x1b]7501;state=working:id=aborted:ignored={cancel}{separator}{following}"
                );
                for split in 0..=wire.len() {
                    let listener = Listener::default();
                    let mut term =
                        Term::new(Default::default(), &TermSize::new(80, 24), listener.clone());
                    let mut stream = StreamProcessor::default();
                    stream.feed(&mut term, &listener, &wire.as_bytes()[..split]);
                    stream.feed(&mut term, &listener, &wire.as_bytes()[split..]);
                    let records = term.program_statuses().iter().collect::<Vec<_>>();
                    let expected = usize::from(!following.is_empty());
                    assert_eq!(
                        records.len(),
                        expected,
                        "{cancel:?}, {separator:?}, {following:?}, split {split}"
                    );
                    assert_eq!(term.program_statuses().seen(), expected != 0);
                    if let Some(record) = records.first() {
                        assert_eq!(record.id, "fresh");
                        assert_eq!(record.state, State::Done);
                    }
                }
            }
        }
    }
}

#[test]
fn wire_order_reset_prompt_and_pty_exit_own_record_lifetime() {
    let listener = Listener::default();
    let mut term = Term::new(Default::default(), &TermSize::new(80, 24), listener.clone());
    let mut stream = StreamProcessor::default();
    stream.feed(&mut term, &listener, b"\x1b]7501;state=working\x07\x1b[?1049h\x1b[!p\x1b[?1049l");
    assert_eq!(term.program_statuses().iter().next().unwrap().state, State::Working);
    stream.feed(&mut term, &listener, b"\x1b]7501;state=done:id=result\x07\x1b]133;A\x07");
    assert_eq!(term.program_statuses().iter().count(), 1);
    stream.feed(&mut term, &listener, b"\x1b]7501;state=blocked:id=child\x07");
    term.exit();
    assert_eq!(term.program_statuses().iter().next().unwrap().id, "result");
    assert_eq!(term.program_statuses().iter().count(), 1);
    stream.feed(&mut term, &listener, b"\x1bc\x1b]7501;state=error:id=new\x07");
    assert_eq!(term.program_statuses().iter().next().unwrap().id, "new");
    stream.feed(&mut term, &listener, b"\x1bc");
    assert!(!term.program_statuses().seen());
    assert_eq!(term.program_statuses().iter().count(), 0);
}

#[test]
fn synchronized_ris_preserves_report_order_at_every_chunk_boundary() {
    for reset in ["\x1bc", "\x1b]7501;state=error:id=aborted:ignored=\x1bc"] {
        for reset_first in [true, false] {
            for end in ["\x1b[?2026l", ""] {
                let report = "\x1b]7501;state=working:app=codex:id=fresh\x07";
                let body = if reset_first {
                    format!("{reset}{report}")
                } else {
                    format!("{report}{reset}")
                };
                let wire = format!("\x1b[?2026h{body}{end}");
                for split in 0..=wire.len() {
                    let listener = Listener::default();
                    let mut term =
                        Term::new(Default::default(), &TermSize::new(80, 24), listener.clone());
                    let mut stream = StreamProcessor::default();
                    stream.feed(&mut term, &listener, b"\x1b]7501;state=blocked:id=old\x07");
                    listener.0.lock().unwrap().clear();
                    stream.feed(&mut term, &listener, &wire.as_bytes()[..split]);
                    stream.feed(&mut term, &listener, &wire.as_bytes()[split..]);
                    let assert_records = |term: &Term<Listener>| {
                        let records = term.program_statuses().iter().collect::<Vec<_>>();
                        assert_eq!(records.len(), usize::from(reset_first), "{wire:?}, {split}");
                        assert_eq!(
                            term.program_statuses().seen(),
                            reset_first,
                            "{wire:?}, {split}"
                        );
                        if let Some(record) = records.first() {
                            assert_eq!(record.id, "fresh");
                            assert_eq!(record.state, State::Working);
                            assert_eq!(record.app.as_deref(), Some("codex"));
                        }
                    };
                    assert_records(&term);
                    if end.is_empty() {
                        // Do not flush or end synchronized rendering just to
                        // order status events. This also covers the timeout path.
                        assert!(stream.next_sync_timeout().is_some(), "{wire:?}, {split}");
                        assert!(stream.sync_bytes_count() > 0, "{wire:?}, {split}");
                        stream.stop_sync(&mut term);
                        assert_records(&term);
                    }
                    assert!(stream.next_sync_timeout().is_none());
                    assert_eq!(stream.sync_bytes_count(), 0);
                    assert_eq!(
                        listener
                            .0
                            .lock()
                            .unwrap()
                            .iter()
                            .filter(|event| matches!(event, Event::ProgramStatusChanged))
                            .count(),
                        2,
                        "{wire:?}, {split}"
                    );
                }
            }
        }
    }
}

#[test]
fn incomplete_cancelled_and_intermediate_escapes_do_not_reset_records() {
    for escape in ["\x1b", "\x1b\x18c", "\x1b\x1ac", "\x1b#c", "\x1b[!p"] {
        for split in 0..=escape.len() {
            let listener = Listener::default();
            let mut term = Term::new(Default::default(), &TermSize::new(80, 24), listener.clone());
            let mut stream = StreamProcessor::default();
            stream.feed(&mut term, &listener, b"\x1b]7501;state=done\x07");
            stream.feed(&mut term, &listener, &escape.as_bytes()[..split]);
            stream.feed(&mut term, &listener, &escape.as_bytes()[split..]);
            assert_eq!(term.program_statuses().iter().next().unwrap().state, State::Done);
            // Direct reset still owns full Term reset semantics, independently
            // of StreamProcessor's protection around VT buffer replay.
            term.reset_state();
            assert!(!term.program_statuses().seen());
            assert_eq!(term.program_statuses().iter().count(), 0);
        }
    }
}

#[test]
fn detection_reply_is_fixed_precedes_da_and_cannot_loop() {
    let listener = Listener::default();
    let mut term = Term::new(Default::default(), &TermSize::new(80, 24), listener.clone());
    let mut stream = StreamProcessor::default();
    stream.feed(&mut term, &listener, b"\x1b]7501;?\x1b\\\x1b[c");
    let replies = listener
        .0
        .lock()
        .unwrap()
        .iter()
        .filter_map(
            |event| {
                if let Event::PtyWrite(text) = event { Some(text.clone()) } else { None }
            },
        )
        .collect::<Vec<_>>();
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0], FEATURE_REPLY);
    listener.0.lock().unwrap().clear();
    stream.feed(&mut term, &listener, FEATURE_REPLY.as_bytes());
    stream.feed(&mut term, &listener, b"\x1b]7501;?:state=working:msg=VGVzdA==\x07");
    assert!(listener.0.lock().unwrap().is_empty());
    assert!(!term.program_statuses().seen());
}

#[test]
fn osc94_cannot_clobber_real_status_until_ris() {
    let listener = Listener::default();
    let mut term = Term::new(Default::default(), &TermSize::new(80, 24), listener.clone());
    let mut stream = StreamProcessor::default();
    stream.feed(&mut term, &listener, b"\x1b]7501;?\x07\x1b]9;4;1;20\x07\x1b]7501;state=working\x07\x1b]9;4;0\x07\x1b]7501;state=clear\x07\x1b]9;4;3\x07\x1bc\x1b]9;4;1;90\x07");
    let progress = listener
        .0
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| {
            if let Event::Progress { state, value } = event { Some((*state, *value)) } else { None }
        })
        .collect::<Vec<_>>();
    assert_eq!(progress, [(1, Some(20)), (1, Some(90))]);
}

#[test]
fn malformed_updates_and_another_pty_cannot_replace_a_record() {
    let listener = Listener::default();
    let mut first = Term::new(Default::default(), &TermSize::new(80, 24), listener.clone());
    let mut second = Term::new(Default::default(), &TermSize::new(80, 24), listener.clone());
    let mut first_stream = StreamProcessor::default();
    let mut second_stream = StreamProcessor::default();
    first_stream.feed(&mut first, &listener, b"\x1b]7501;state=blocked:app=codex\x07");
    second_stream.feed(&mut second, &listener, b"\x1b]7501;state=working:app=cargo\x07");
    first_stream.feed(&mut first, &listener, b"\x1b]7501;state=done:msg=Cg==\x07\x1b]7501;state=clear:id=/\x07\x1b]7501;state=future\x07");
    assert_eq!(first.program_statuses().iter().next().unwrap().state, State::Blocked);
    second_stream.feed(&mut second, &listener, b"\x1bc");
    assert!(first.program_statuses().seen());
    assert_eq!(first.program_statuses().iter().next().unwrap().app.as_deref(), Some("codex"));
    assert_eq!(second.program_statuses().iter().count(), 0);
}
