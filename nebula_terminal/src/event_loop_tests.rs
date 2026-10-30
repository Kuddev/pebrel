use super::*;

use crate::event::VoidListener;
use crate::inline_image::SourceRect;
use crate::term::Config;
use crate::term::test::TermSize;

#[test]
fn ordinary_command_view_preserves_grid_cursor_and_chunk_boundaries() {
    use crate::inline_content::CommandView;
    struct Lens;
    impl EventListener for Lens {
        fn command_view(&self, shell: &str, line: &str) -> Option<CommandView> {
            (shell == "pwsh:1" && line == "cat long.md")
                .then(|| CommandView { mime: "text/markdown".into(), title: line.into() })
        }
    }
    let start = b"\x1b]1337;SetUserVar=pebrel_shell=cHdzaDox\x07\x1b]1337;SetUserVar=pebrel_command=cHdzaDoxCmNhdCBsb25nLm1k\x07\x1b]133;C\x07";
    let output = "# 中文\r\n\r\nThis line is deliberately longer than the terminal grid width.\r\n1234567890123456789012345界suffix\r\n";
    // 同一字节流会同时承载状态报告与原文，合并协议入口后不能重复消费或丢失其一。
    for (previous, working, done) in [
        ("", "", ""),
        (
            "\x1b]7501;state=done:id=previous\x07",
            "\x1b]7501;state=working:id=task\x1b\\",
            "\x1b]7501;state=done:id=task\x07",
        ),
    ] {
        let wire = [
            previous.as_bytes(),
            start.as_slice(),
            working.as_bytes(),
            output.as_bytes(),
            done.as_bytes(),
            b"\x1b]133;D;0\x07\x1b]133;A\x07prompt>",
        ]
        .concat();
        for split in 0..=wire.len() {
            let mut term = Term::new(
                Config { scrolling_history: 0, ..Config::default() },
                &TermSize::new(24, 24),
                Lens,
            );
            let mut plain = Term::new(Config::default(), &TermSize::new(24, 24), VoidListener);
            let mut stream = StreamProcessor::default();
            stream.feed(&mut term, &Lens, &wire[..split]);
            stream.feed(&mut term, &Lens, &wire[split..]);
            StreamProcessor::default().feed(&mut plain, &VoidListener, &wire);
            assert_eq!(term.grid().cursor.point, plain.grid().cursor.point);
            let surface = &term.grid()[crate::index::Line(0)][crate::index::Column(0)]
                .inline_marker()
                .unwrap()
                .surface;
            assert!(surface.has_grid_source());
            // Native text markers require cleanup, not per-cell image lookup.
            assert!(!term.grid().may_have_images());
            let text = &surface.update_since(0).chunks[0].text;
            assert_eq!(text, &output.replace("\r\n", "\n"));
            assert_eq!(
                term.bounds_to_string(
                    crate::index::Point::new(crate::index::Line(0), crate::index::Column(0)),
                    term.grid().cursor.point
                ),
                plain.bounds_to_string(
                    crate::index::Point::new(crate::index::Line(0), crate::index::Column(0)),
                    plain.grid().cursor.point
                )
            );
            let records = term.program_statuses().iter().collect::<Vec<_>>();
            if previous.is_empty() {
                assert!(records.is_empty());
            } else {
                assert_eq!(records.len(), 2);
                assert!(
                    records
                        .iter()
                        .find(|record| record.id == "previous")
                        .unwrap()
                        .result_acknowledged
                );
                let task = records.iter().find(|record| record.id == "task").unwrap();
                assert_eq!(task.state, crate::program_status::State::Done);
                assert!(!task.result_acknowledged);
            }
            let lifetime = Arc::downgrade(surface);
            stream.feed(&mut term, &Lens, &b"\r\n".repeat(24));
            assert!(lifetime.upgrade().is_none());
            stream.feed(&mut term, &Lens, b"\x1bc");
            assert!(lifetime.upgrade().is_none());
            assert!(!term.grid().may_have_images());
        }
    }
}

#[test]
fn ordinary_command_view_leaves_failed_or_interactive_output_plain() {
    struct Lens;
    impl EventListener for Lens {
        fn command_view(&self, _: &str, _: &str) -> Option<crate::inline_content::CommandView> {
            Some(crate::inline_content::CommandView {
                mime: "text/markdown".into(),
                title: "cat".into(),
            })
        }
    }
    let start = b"\x1b]1337;SetUserVar=pebrel_shell=cHdzaDox\x07\x1b]1337;SetUserVar=pebrel_command=cHdzaDoxCmNhdCBsb25nLm1k\x07\x1b]133;C\x07";
    for suffix in [
        b"error\r\n\x1b]133;D;1\x07".as_slice(),
        b"\x1b[?1049hfull screen\x1b[?1049l\r\n\x1b]133;D;0\x07",
        b"partial\x1b]133;D;0\x07",
    ] {
        let mut term = Term::new(Config::default(), &TermSize::new(80, 24), Lens);
        StreamProcessor::default().feed(&mut term, &Lens, &[start.as_slice(), suffix].concat());
        assert!((term.topmost_line().0..=term.bottommost_line().0).all(|row| {
            term.grid()[crate::index::Line(row)][crate::index::Column(0)].inline_marker().is_none()
        }));
    }
}

#[test]
fn native_content_grows_in_wire_order_and_rejects_non_column_zero() {
    use crate::index::{Column, Line, Point};
    use crate::inline_content::Packet;
    let mut terminal = Term::new(Config::default(), &TermSize::new(80, 24), VoidListener);
    let mut stream = StreamProcessor::default();
    let begin =
        Packet::Begin { wire_id: 7, rows: 8, mime: "text/plain".into(), title: "compact".into() };
    stream.feed(&mut terminal, &VoidListener, begin.encode().as_bytes());
    let surface =
        terminal.grid()[Point::new(Line(0), Column(0))].inline_marker().unwrap().surface.clone();
    assert_eq!(surface.rows(), 1);
    stream.feed(&mut terminal, &VoidListener, b"\r\nx");
    stream.feed(
        &mut terminal,
        &VoidListener,
        Packet::Place { wire_id: 7, row: 1 }.encode().as_bytes(),
    );
    assert_eq!(surface.rows(), 1);
    stream.feed(&mut terminal, &VoidListener, b"\r");
    for row in 1..4 {
        let wire = format!(
            "{}\r\n{}",
            Packet::Place { wire_id: 7, row }.encode(),
            Packet::Append { wire_id: 7, text: "part".into() }.encode()
        );
        for byte in wire.as_bytes() {
            stream.feed(&mut terminal, &VoidListener, &[*byte]);
        }
        assert_eq!(surface.rows(), row + 1);
    }
    stream.feed(
        &mut terminal,
        &VoidListener,
        Packet::End { wire_id: 7, code: 0 }.encode().as_bytes(),
    );
    stream.feed(&mut terminal, &VoidListener, b"after");
    for row in 0..4 {
        assert_eq!(
            terminal.grid()[Point::new(Line(row), Column(0))].inline_marker().unwrap().row,
            row as u16
        );
    }
    assert_eq!(terminal.grid()[Point::new(Line(4), Column(0))].c, 'a');
}

#[test]
fn native_content_survives_split_frames_and_reflow_without_cell_growth() {
    use crate::index::{Column, Line, Point};
    use crate::inline_content::{Packet, Status};
    let packets = [
        Packet::Begin {
            wire_id: 55, rows: 8, mime: "text/markdown".into(), title: "结果".into()
        },
        Packet::Append { wire_id: 55, text: "# 中文\nhello".into() },
        Packet::End { wire_id: 55, code: 0 },
    ];
    let mut wire = packets[0].encode();
    for row in 1..8 {
        wire.push_str("\r\n");
        wire.push_str(&Packet::Place { wire_id: 55, row }.encode());
    }
    wire.push_str("\r\n");
    wire.extend(packets[1..].iter().map(Packet::encode));
    for (stride, synchronized) in [(1, false), (2, true), (7, true), (wire.len(), false)] {
        let mut terminal = Term::new(Config::default(), &TermSize::new(80, 24), VoidListener);
        let mut stream = StreamProcessor::default();
        if synchronized {
            stream.feed(&mut terminal, &VoidListener, b"\x1b[?2026h");
        }
        stream.feed(&mut terminal, &VoidListener, b"long prefix before content\r\n");
        for chunk in wire.as_bytes().chunks(stride) {
            stream.feed(&mut terminal, &VoidListener, chunk);
        }
        if synchronized {
            stream.feed(&mut terminal, &VoidListener, b"\x1b[?2026l");
        }
        stream.feed(&mut terminal, &VoidListener, b"after");
        let marker =
            terminal.grid()[Point::new(Line(1), Column(0))].inline_marker().unwrap().clone();
        assert_eq!(marker.surface.update_since(0).chunks[0].text, "# 中文\nhello");
        assert_eq!(marker.surface.update_since(0).status, Status::Finished(0));
        for width in [10, 120, 2, 80] {
            terminal.resize(TermSize::new(width, 24));
            let markers: Vec<_> = (terminal.topmost_line().0..=terminal.bottommost_line().0)
                .filter_map(|line| {
                    terminal.grid()[Point::new(Line(line), Column(0))].inline_marker()
                })
                .collect();
            assert_eq!(markers.len(), 8, "width {width}");
            assert!(markers.iter().all(|m| m.surface.id == marker.surface.id));
        }
        stream.feed(&mut terminal, &VoidListener, b"\x1bc");
        assert!((terminal.topmost_line().0..=terminal.bottommost_line().0).all(|line| {
            terminal.grid()[Point::new(Line(line), Column(0))].inline_marker().is_none()
        }));
    }
}

#[test]
fn animation_snapshots_cannot_observe_a_partial_synchronized_update() {
    use crate::render::{RenderSnapshot, SnapshotConfig};
    let mut term = Term::new(Config::default(), &TermSize::new(20, 2), VoidListener);
    let mut stream = StreamProcessor::default();
    let cfg = SnapshotConfig { rows: 2, cols: 20 };
    stream.feed(&mut term, &VoidListener, b"old");
    let before = RenderSnapshot::capture(&term, &cfg);
    for bytes in [b"\x1b[?20".as_slice(), b"26h\r", b"new", b"\x1b[10G", b"\x1b[?2026"] {
        stream.feed(&mut term, &VoidListener, bytes);
        // Animation frames read the grid even without a Wakeup.
        let frame = RenderSnapshot::capture(&term, &cfg);
        assert_eq!(frame.cursor.as_ref().unwrap().col, before.cursor.as_ref().unwrap().col);
        assert_eq!(term.grid()[Line(0)][Column(0)].c, 'o');
    }
    stream.feed(&mut term, &VoidListener, b"l");
    assert_eq!(RenderSnapshot::capture(&term, &cfg).cursor.unwrap().col, 9);
    assert_eq!(term.grid()[Line(0)][Column(0)].c, 'n');
    stream.feed(&mut term, &VoidListener, b"\x1b[?2026h\rtimeout");
    assert_eq!(term.grid()[Line(0)][Column(0)].c, 'n');
    stream.stop_sync(&mut term);
    assert_eq!(term.grid()[Line(0)][Column(0)].c, 't');
}

#[test]
fn synchronized_output_has_a_network_tolerant_deadline_and_bounded_buffer() {
    let mut term = Term::new(Config::default(), &TermSize::new(20, 2), VoidListener);
    let mut stream = StreamProcessor::default();
    let started = Instant::now();
    stream.feed(&mut term, &VoidListener, b"old\x1b[?2026h");
    let deadline = stream.next_sync_timeout().unwrap();
    assert!(deadline >= started + Duration::from_secs(1));
    assert!(deadline <= Instant::now() + Duration::from_secs(1));
    stream.feed(&mut term, &VoidListener, b"\rnew");
    assert_eq!(stream.next_sync_timeout(), Some(deadline));
    assert_eq!(term.grid()[Line(0)][Column(0)].c, 'o');
    stream.feed(&mut term, &VoidListener, b"\x1b[?2026l");
    assert!(stream.next_sync_timeout().is_none());
    assert_eq!(term.grid()[Line(0)][Column(0)].c, 'n');

    stream.feed(&mut term, &VoidListener, b"\x1b[?2026h");
    // The existing parser cap still forces progress when a producer never
    // ends a large update. A longer watchdog must not remove that cap.
    stream.feed(&mut term, &VoidListener, &vec![b'x'; 2 * 1024 * 1024]);
    assert!(stream.next_sync_timeout().is_none());
    assert_eq!(stream.sync_bytes_count(), 0);
    assert_eq!(term.grid()[Line(0)][Column(0)].c, 'x');
}

#[test]
fn authenticated_hook_frames_survive_every_chunk_boundary_and_reject_other_panes() {
    #[derive(Clone, Default)]
    struct Listener(std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>);
    impl EventListener for Listener {
        fn send_event(&self, event: Event) {
            if let Event::AiHookEnvelope(envelope) = event {
                self.0.lock().unwrap().push(envelope);
            }
        }
    }
    // Base64 for a small envelope; authentication is checked before delivery.
    let frame = b"\x1b]777;nebula-hook;0123456789abcdef0123456789abcdef;bmVidWxhLWhvb2svMSBzb3VyY2U9Y29kZXgKe30=\x07";
    for token in
        [None, Some("ffffffffffffffffffffffffffffffff"), Some("0123456789abcdef0123456789abcdef")]
    {
        for split in 0..=frame.len() {
            let listener = Listener::default();
            let mut terminal =
                Term::new(Config::default(), &TermSize::new(80, 24), listener.clone());
            let mut stream = StreamProcessor::default();
            if let Some(token) = token {
                stream.set_remote_hook_token(token.into());
            }
            stream.feed(&mut terminal, &listener, &frame[..split]);
            stream.feed(&mut terminal, &listener, &frame[split..]);
            let events = listener.0.lock().unwrap();
            if token == Some("0123456789abcdef0123456789abcdef") {
                assert_eq!(events.as_slice(), [b"nebula-hook/1 source=codex\n{}".to_vec()]);
            } else {
                assert!(events.is_empty());
            }
        }
    }
}

#[test]
fn shell_identity_cwd_and_title_keep_wire_order_across_chunk_boundaries() {
    #[derive(Clone, Default)]
    struct Listener(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
    impl EventListener for Listener {
        fn send_event(&self, event: Event) {
            let label = match event {
                Event::UserVar { value, .. } => format!("shell:{value}"),
                Event::CwdReport(cwd) => format!("cwd:{cwd}"),
                Event::Title(title) => format!("title:{title}"),
                _ => return,
            };
            self.0.lock().unwrap().push(label);
        }
    }
    let bytes = b"\x1b]1337;SetUserVar=pebrel_shell=cmVtb3Rl\x07\x1b]7;file://box/remote\x07\x1b]2;remote\x07\x1b]1337;SetUserVar=pebrel_shell=bG9jYWw=\x07\x1b]7;file://localhost/local\x07";
    for split in 0..=bytes.len() {
        let listener = Listener::default();
        let size = TermSize::new(80, 24);
        let mut terminal = Term::new(Config::default(), &size, listener.clone());
        let mut stream = StreamProcessor::default();
        stream.feed(&mut terminal, &listener, &bytes[..split]);
        stream.feed(&mut terminal, &listener, &bytes[split..]);
        assert_eq!(
            *listener.0.lock().unwrap(),
            ["shell:remote", "cwd:/remote", "title:remote", "shell:local", "cwd:/local"],
            "split at {split}"
        );
    }
}

#[test]
fn semantic_input_boundary_survives_chunking_and_scrolling() {
    use crate::index::{Column, Line, Point};

    let bytes = b"\x1b]133;A\x07[first]\r\n>\x1b]133;B\x07pause";
    for split in 0..=bytes.len() {
        let mut terminal = Term::new(Config::default(), &TermSize::new(20, 2), VoidListener);
        let mut stream = StreamProcessor::default();
        stream.feed(&mut terminal, &VoidListener, &bytes[..split]);
        stream.feed(&mut terminal, &VoidListener, &bytes[split..]);
        assert_eq!(terminal.nebula_prompt_input_point(), Some(Point::new(Line(1), Column(1))));
        stream.feed(&mut terminal, &VoidListener, b"\r\n");
        assert_eq!(terminal.nebula_prompt_input_point(), Some(Point::new(Line(0), Column(1))));
        stream.feed(&mut terminal, &VoidListener, b"\x1b]133;C\x07");
        assert_eq!(terminal.nebula_prompt_input_point(), None);
    }
}

#[test]
fn input_boundaries_require_a_prompt_and_do_not_survive_reflow_or_reset() {
    let mut terminal = Term::new(Config::default(), &TermSize::new(20, 2), VoidListener);
    let mut stream = StreamProcessor::default();
    stream.feed(&mut terminal, &VoidListener, b"\x1b]133;B\x07");
    assert_eq!(terminal.nebula_prompt_input_point(), None);
    for ending in [b"\x1bc".as_slice(), b"\x1b]133;D;0\x07", b"\x1b]133;A\x07"] {
        stream.feed(&mut terminal, &VoidListener, b"\x1b]133;A\x07\x1b]133;B\x07");
        assert!(terminal.nebula_prompt_input_point().is_some());
        stream.feed(&mut terminal, &VoidListener, ending);
        assert_eq!(terminal.nebula_prompt_input_point(), None);
    }
    stream.feed(&mut terminal, &VoidListener, b"\x1b]133;A\x07\x1b]133;B\x07");
    terminal.resize(TermSize::new(10, 2));
    assert_eq!(terminal.nebula_prompt_input_point(), None);
}

#[test]
fn shell_semantic_events_track_the_active_prompt() {
    let size = TermSize::new(80, 24);
    let mut terminal = Term::new(Config::default(), &size, VoidListener);
    let listener = VoidListener;
    let mut stream = StreamProcessor::default();

    stream.feed(&mut terminal, &listener, b"\x1b]133;A\x07custom prompt :: ");
    assert!(terminal.nebula_prompt_active());

    stream.feed(&mut terminal, &listener, b"\x1b]133;C\x07");
    assert!(!terminal.nebula_prompt_active());

    stream.feed(&mut terminal, &listener, b"\x1b]133;A\x07next prompt :: ");
    assert!(terminal.nebula_prompt_active());

    stream.feed(&mut terminal, &listener, b"\x1b]133;D;0\x07");
    assert!(!terminal.nebula_prompt_active());
}

#[derive(Clone, Default)]
struct Gfx(std::sync::Arc<std::sync::Mutex<Vec<Event>>>);

impl EventListener for Gfx {
    fn send_event(&self, event: Event) {
        if matches!(event, Event::KittyImage { .. } | Event::PtyWrite(_)) {
            self.0.lock().unwrap().push(event);
        }
    }
}

fn kitty_command(control: &str, payload: &str) -> Vec<u8> {
    if payload.is_empty() {
        format!("\x1b_G{control}\x1b\\").into_bytes()
    } else {
        format!("\x1b_G{control};{payload}\x1b\\").into_bytes()
    }
}

fn rgba2x1() -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode([7u8; 2 * 1 * 4])
}

/// yazi moves the cursor to each cell and places a 1×1 tile whose source
/// rectangle it computed from a cell size it guessed. Every one of those
/// placements belongs to one image, and the image is the union of the cells
/// the cursor visited — which is what keeps it inside the pane.
#[test]
fn kitty_cell_placements_publish_one_image_covering_visited_cells() {
    let listener = Gfx::default();
    let size = TermSize::new(20, 8);
    let mut terminal = Term::new(Config::default(), &size, listener.clone());
    let mut stream = StreamProcessor::default();
    let mut wire = Vec::new();
    wire.extend_from_slice(b"\x1b[3;5H");
    wire.extend_from_slice(&kitty_command("a=t,i=7,f=32,s=2,v=1,m=0", &rgba2x1()));
    wire.extend_from_slice(b"\x1b[3;5H");
    wire.extend_from_slice(&kitty_command("a=p,i=7,p=1,x=0,y=0,w=1,h=1,c=1,r=1", ""));
    wire.extend_from_slice(b"\x1b[3;6H");
    wire.extend_from_slice(&kitty_command("a=p,i=7,p=2,x=1,y=0,w=1,h=1,c=1,r=1", ""));
    stream.feed(&mut terminal, &listener, &wire);

    let events = listener.0.lock().unwrap();
    let images: Vec<&Event> =
        events.iter().filter(|event| matches!(event, Event::KittyImage { .. })).collect();
    // One publication per image, not one per placement: a preview arrives as
    // thousands of placements.
    assert_eq!(images.len(), 1);
    let Event::KittyImage { data, channels, placement } = images[0] else { unreachable!() };
    assert_eq!(data.len(), 8);
    assert_eq!(*channels, 4);
    assert_eq!(placement.layout.width, 2.0);
    assert_eq!(placement.layout.height, 1.0);
    assert_eq!((placement.layout.columns, placement.layout.rows), (2, 1));
    // The cells the cursor visited carry the image, keyed to the tile the
    // application sampled, so a later scroll re-samples the right pixels.
    let cell = terminal.grid()[Line(2)][Column(4)].image().expect("placed at the cursor");
    assert_eq!(cell.image.id, placement.id);
    assert_eq!((cell.column, cell.row), (0, 0));
    let next = terminal.grid()[Line(2)][Column(5)].image().expect("second cell");
    assert_eq!((next.column, next.row), (1, 0));
    // Placements do not move the text cursor (`C=1`), so the prompt
    // stays where the shell left it.
    assert_eq!(terminal.grid().cursor.point, crate::index::Point::new(Line(2), Column(5)));
}

/// A transfer arrives in 4096-byte chunks that carry no id; only the header
/// does. Publishing before the last chunk would paint half an image.
#[test]
fn kitty_chunked_transfer_waits_for_the_last_chunk() {
    use base64::Engine as _;
    let listener = Gfx::default();
    let size = TermSize::new(20, 8);
    let mut terminal = Term::new(Config::default(), &size, listener.clone());
    let mut stream = StreamProcessor::default();
    let encoded = base64::engine::general_purpose::STANDARD.encode([3u8; 2 * 2 * 3]);
    let (head, tail) = encoded.split_at(8);
    stream.feed(&mut terminal, &listener, b"\x1b[1;1H");
    stream.feed(&mut terminal, &listener, &kitty_command("a=t,i=9,f=24,s=2,v=2,m=1", head));
    assert!(listener.0.lock().unwrap().is_empty());
    stream.feed(&mut terminal, &listener, &kitty_command("m=0", tail));
    stream.feed(&mut terminal, &listener, &kitty_command("a=p,i=9,c=2,r=2", ""));

    let events = listener.0.lock().unwrap();
    let images: Vec<&Event> =
        events.iter().filter(|event| matches!(event, Event::KittyImage { .. })).collect();
    assert_eq!(images.len(), 1);
    let Event::KittyImage { data, channels, placement } = images[0] else { unreachable!() };
    assert_eq!((data.len(), *channels), (2 * 2 * 3, 3));
    assert_eq!((placement.layout.columns, placement.layout.rows), (2, 2));
}

/// A chunked `a=T` carries its destination cell box on the header and its
/// pixels on the final chunk, so the placement has to wait for the transfer
/// to finish: clients that stream a page image this way never send `a=p`.
#[test]
fn kitty_chunked_display_is_placed_when_the_last_chunk_arrives() {
    use base64::Engine as _;
    let listener = Gfx::default();
    let size = TermSize::new(20, 8);
    let mut terminal = Term::new(Config::default(), &size, listener.clone());
    let mut stream = StreamProcessor::default();
    let encoded = base64::engine::general_purpose::STANDARD.encode([7u8; 3 * 2 * 3]);
    let (head, tail) = encoded.split_at(8);
    stream.feed(&mut terminal, &listener, b"\x1b[3;4H");
    stream.feed(
        &mut terminal,
        &listener,
        &kitty_command("a=T,i=21,f=24,s=3,v=2,c=3,r=2,m=1", head),
    );
    assert!(listener.0.lock().unwrap().is_empty());
    stream.feed(&mut terminal, &listener, &kitty_command("m=0", tail));

    let events = listener.0.lock().unwrap();
    let images: Vec<&Event> =
        events.iter().filter(|event| matches!(event, Event::KittyImage { .. })).collect();
    assert_eq!(images.len(), 1);
    let Event::KittyImage { data, channels, placement } = images[0] else { unreachable!() };
    assert_eq!((data.len(), *channels), (3 * 2 * 3, 3));
    assert_eq!((placement.layout.columns, placement.layout.rows), (3, 2));
    assert!(terminal.grid()[Line(2)][Column(3)].image().is_some());
}

/// yazi reuses one protocol id for every preview and erases the previous image
/// before drawing the next one. Without honouring that, each file browsed
/// leaves a bitmap behind and the frontend keeps compositing all of them.
#[test]
fn kitty_delete_clears_cells_and_the_same_id_can_be_transferred_again() {
    let listener = Gfx::default();
    let size = TermSize::new(20, 8);
    let mut terminal = Term::new(Config::default(), &size, listener.clone());
    let mut stream = StreamProcessor::default();
    stream.feed(&mut terminal, &listener, b"\x1b[2;3H");
    stream.feed(&mut terminal, &listener, &kitty_command("a=t,i=4,f=32,s=2,v=1,m=0", &rgba2x1()));
    stream.feed(&mut terminal, &listener, &kitty_command("a=p,i=4,c=2,r=1", ""));
    assert!(terminal.grid()[Line(1)][Column(2)].image().is_some());

    stream.feed(&mut terminal, &listener, &kitty_command("a=d,d=I,i=4", ""));
    assert!(terminal.grid()[Line(1)][Column(2)].image().is_none());
    assert_eq!(listener.0.lock().unwrap().len(), 1);

    stream.feed(&mut terminal, &listener, b"\x1b[5;3H");
    stream.feed(&mut terminal, &listener, &kitty_command("a=t,i=4,f=32,s=2,v=1,m=0", &rgba2x1()));
    stream.feed(&mut terminal, &listener, &kitty_command("a=p,i=4,c=2,r=1", ""));
    assert!(terminal.grid()[Line(4)][Column(2)].image().is_some());
    // A second image under the same id must reach the frontend again.
    assert_eq!(listener.0.lock().unwrap().len(), 2);
}

/// The query a client sends when it wants to convert pixels into cells.
/// vte drops it, so the answer comes from the raw byte scan.
#[test]
fn cell_size_query_is_answered_once_the_pane_reports_its_geometry() {
    let listener = Gfx::default();
    let size = TermSize::new(20, 8);
    let mut terminal = Term::new(Config::default(), &size, listener.clone());
    let mut stream = StreamProcessor::default();
    stream.feed(&mut terminal, &listener, b"\x1b[16t");
    assert!(listener.0.lock().unwrap().is_empty(), "no geometry yet");
    stream.resize(WindowSize { num_cols: 20, num_lines: 8, cell_width: 14, cell_height: 36 });
    stream.feed(&mut terminal, &listener, b"");
    let events = listener.0.lock().unwrap();
    match events.as_slice() {
        [Event::PtyWrite(text)] => assert_eq!(text, "\x1b[6;36;14t"),
        other => panic!("unexpected events: {other:?}"),
    }
}

/// Shared memory transports are advertised, then refused, so the client
/// falls back to base64 instead of sending a handle this terminal cannot
/// read.
/// yazi keeps one image id for the whole process and sends the next preview
/// with the same one. If the erased picture's geometry survives, the next
/// payload decodes at the wrong stride and the preview shows diagonal
/// stripes instead of a picture.
#[test]
fn kitty_retransfer_after_delete_uses_the_new_pixel_geometry() {
    use base64::Engine as _;
    let listener = Gfx::default();
    let size = TermSize::new(20, 8);
    let mut terminal = Term::new(Config::default(), &size, listener.clone());
    let mut stream = StreamProcessor::default();

    let mut send = |stream: &mut StreamProcessor, wire: Vec<u8>| {
        stream.feed(&mut terminal, &listener, &wire);
    };
    let encoded =
        |side: usize| base64::engine::general_purpose::STANDARD.encode(vec![7u8; side * side * 4]);

    let mut first = Vec::new();
    first.extend_from_slice(&kitty_command("a=t,i=11,f=32,s=4,v=4,m=0", &encoded(4)));
    first.extend_from_slice(b"\x1b[1;1H");
    first.extend_from_slice(&kitty_command("a=p,i=11,c=2,r=2", ""));
    send(&mut stream, first);

    send(&mut stream, kitty_command("a=d,d=I,i=11", "").to_vec());

    let mut second = Vec::new();
    second.extend_from_slice(&kitty_command("a=t,i=11,f=32,s=9,v=9,m=0", &encoded(9)));
    second.extend_from_slice(b"\x1b[1;1H");
    second.extend_from_slice(&kitty_command("a=p,i=11,c=3,r=3", ""));
    send(&mut stream, second);

    let events = listener.0.lock().unwrap();
    let published: Vec<(f32, f32, usize, usize)> = events
        .iter()
        .filter_map(|event| match event {
            Event::KittyImage { placement, .. } => Some((
                placement.layout.width,
                placement.layout.height,
                placement.layout.columns,
                placement.layout.rows,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(published, vec![(4.0, 4.0, 2, 2), (9.0, 9.0, 3, 3)]);
}

#[test]
fn kitty_probe_refuses_shared_memory_and_accepts_base64() {
    let listener = Gfx::default();
    let size = TermSize::new(20, 8);
    let mut terminal = Term::new(Config::default(), &size, listener.clone());
    let mut stream = StreamProcessor::default();
    stream.feed(&mut terminal, &listener, &kitty_command("a=q,i=5,s=1,v=1,t=d,f=24", "AAAA"));
    stream.feed(&mut terminal, &listener, &kitty_command("a=q,i=6,s=1,v=1,t=s,f=24,S=3", "AAAA"));
    let events = listener.0.lock().unwrap();
    let replies: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            Event::PtyWrite(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(replies, ["\x1b_Gi=5;OK\x1b\\", "\x1b_Gi=6;ENOTSUP\x1b\\"]);
}
fn encoded(pixels: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(pixels)
}

fn kitty_images(listener: &Gfx) -> Vec<(SourceRect, usize, usize, f32, f32)> {
    listener
        .0
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            Event::KittyImage { placement, .. } => Some((
                placement.layout.source,
                placement.layout.columns,
                placement.layout.rows,
                placement.layout.width,
                placement.layout.height,
            )),
            _ => None,
        })
        .collect()
}

/// The box can be much taller than the slice it shows: a pager asks for a
/// short band inside a tall cell block while it scrolls. The slice keeps
/// the height the emitter asked for, not the height of the box.
#[test]
fn kitty_a_slice_stays_short_inside_a_tall_box() {
    let listener = Gfx::default();
    let size = TermSize::new(40, 20);
    let mut terminal = Term::new(Config::default(), &size, listener.clone());
    let mut stream = StreamProcessor::default();
    let pixels = vec![7u8; 8 * 8 * 4];

    let mut wire = b"\x1b[2;3H".to_vec();
    wire.extend_from_slice(&kitty_command("a=t,i=21,f=32,s=8,v=8,m=0", &encoded(&pixels)));
    wire.extend_from_slice(&kitty_command("a=p,i=21,c=4,r=4,x=0,y=2,w=8,h=4", ""));
    stream.feed(&mut terminal, &listener, &wire);

    let images = kitty_images(&listener);
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].0, SourceRect { x: 0, y: 2, width: 8, height: 4 });
    assert_eq!((images[0].1, images[0].2), (4, 4), "the box keeps its cells");
    assert_eq!((images[0].3, images[0].4), (8.0, 8.0), "the bitmap stays whole");
}

/// A client that repaints by re-sending the same id has to get a new
/// publication: the screen was erased in between, so the frontend has
/// nothing left to reuse and would otherwise stay blank.
#[test]
fn kitty_retransfer_of_the_same_id_publishes_again() {
    let listener = Gfx::default();
    let size = TermSize::new(20, 8);
    let mut terminal = Term::new(Config::default(), &size, listener.clone());
    let mut stream = StreamProcessor::default();
    let pixels = [7u8; 2 * 1 * 3];

    let mut wire = b"\x1b[2;2H".to_vec();
    wire.extend_from_slice(&kitty_command("a=t,i=31,f=24,s=2,v=1,m=0", &encoded(&pixels)));
    wire.extend_from_slice(&kitty_command("a=p,i=31,c=2,r=1", ""));
    stream.feed(&mut terminal, &listener, &wire);
    assert_eq!(kitty_images(&listener).len(), 1);

    let mut again = b"\x1b[2J\x1b[2;2H".to_vec();
    again.extend_from_slice(&kitty_command("a=t,i=31,f=24,s=2,v=1,m=0", &encoded(&pixels)));
    again.extend_from_slice(&kitty_command("a=p,i=31,c=2,r=1", ""));
    stream.feed(&mut terminal, &listener, &again);

    assert_eq!(kitty_images(&listener).len(), 2, "an erased picture must come back");
}

/// A pager scrolls by re-placing the same transfer with a different `y`.
/// The cell box does not change, so a publication that only watched the box
/// would keep the previous crop on screen forever.
#[test]
fn kitty_placement_publishes_its_crop_and_republishes_when_it_moves() {
    let listener = Gfx::default();
    let size = TermSize::new(20, 8);
    let mut terminal = Term::new(Config::default(), &size, listener.clone());
    let mut stream = StreamProcessor::default();
    let pixels = [9u8; 8 * 4 * 4];

    let mut wire = b"\x1b[2;3H".to_vec();
    wire.extend_from_slice(&kitty_command("a=t,i=3,f=32,s=8,v=4,m=0", &encoded(&pixels)));
    wire.extend_from_slice(&kitty_command("a=p,i=3,c=2,r=1,x=0,y=0,w=8,h=2", ""));
    stream.feed(&mut terminal, &listener, &wire);

    let mut next = b"\x1b[2;3H".to_vec();
    next.extend_from_slice(&kitty_command("a=p,i=3,c=2,r=1,x=0,y=2,w=8,h=2", ""));
    stream.feed(&mut terminal, &listener, &next);

    let images = kitty_images(&listener);
    assert_eq!(images.len(), 2, "a new crop of the same box must republish");
    assert_eq!(images[0].0, SourceRect { x: 0, y: 0, width: 8, height: 2 });
    assert_eq!(images[1].0, SourceRect { x: 0, y: 2, width: 8, height: 2 });
    // The bitmap is still the whole picture: only the slice moved.
    assert_eq!((images[1].3, images[1].4), (8.0, 4.0));
    assert_eq!((images[1].1, images[1].2), (2, 1));
}

/// Single-cell placements are tiles of one picture, so what reaches the
/// screen is the union of their slices — not the last one to be placed.
#[test]
fn kitty_single_cell_placements_union_their_slices() {
    let listener = Gfx::default();
    let size = TermSize::new(20, 8);
    let mut terminal = Term::new(Config::default(), &size, listener.clone());
    let mut stream = StreamProcessor::default();

    let mut wire = b"\x1b[3;5H".to_vec();
    wire.extend_from_slice(&kitty_command("a=t,i=7,f=32,s=2,v=1,m=0", &rgba2x1()));
    wire.extend_from_slice(&kitty_command("a=p,i=7,c=1,r=1,x=0,y=0,w=1,h=1", ""));
    stream.feed(&mut terminal, &listener, &wire);

    let mut next = b"\x1b[3;6H".to_vec();
    next.extend_from_slice(&kitty_command("a=p,i=7,c=1,r=1,x=1,y=0,w=1,h=1", ""));
    stream.feed(&mut terminal, &listener, &next);

    let images = kitty_images(&listener);
    assert_eq!(images.last().unwrap().0, SourceRect { x: 0, y: 0, width: 2, height: 1 });
}

/// `o=z` puts a zlib stream behind the base64, so the pixels arrive
/// compressed and have to be inflated to the declared size before decoding
/// would make sense.
#[test]
fn kitty_zlib_transfer_is_inflated_before_placing() {
    use std::io::Write as _;
    let listener = Gfx::default();
    let size = TermSize::new(20, 8);
    let mut terminal = Term::new(Config::default(), &size, listener.clone());
    let mut stream = StreamProcessor::default();
    let pixels: Vec<u8> = (0..3 * 2 * 3u8).collect();

    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&pixels).unwrap();
    let compressed = encoder.finish().unwrap();

    let mut wire = b"\x1b[1;1H".to_vec();
    wire.extend_from_slice(&kitty_command("a=t,i=12,f=24,s=3,v=2,o=z,m=0", &encoded(&compressed)));
    wire.extend_from_slice(&kitty_command("a=p,i=12,c=3,r=2", ""));
    stream.feed(&mut terminal, &listener, &wire);

    let events = listener.0.lock().unwrap();
    let images: Vec<&Event> =
        events.iter().filter(|event| matches!(event, Event::KittyImage { .. })).collect();
    assert_eq!(images.len(), 1);
    let Event::KittyImage { data, channels, .. } = images[0] else { unreachable!() };
    assert_eq!(&**data, pixels.as_slice());
    assert_eq!(*channels, 3);
}

/// The first chunk of a streamed transfer sometimes omits `a=` and carries
/// only the header keys the parser needs to recognise it as a transfer.
#[test]
fn kitty_transfer_without_action_is_still_a_transfer() {
    let listener = Gfx::default();
    let size = TermSize::new(20, 8);
    let mut terminal = Term::new(Config::default(), &size, listener.clone());
    let mut stream = StreamProcessor::default();
    let pixels = [5u8; 2 * 1 * 3];

    let mut wire = b"\x1b[1;1H".to_vec();
    wire.extend_from_slice(&kitty_command("i=14,f=24,s=2,v=1", &encoded(&pixels)));
    wire.extend_from_slice(&kitty_command("a=p,i=14,c=2,r=1", ""));
    stream.feed(&mut terminal, &listener, &wire);

    let events = listener.0.lock().unwrap();
    let images: Vec<&Event> =
        events.iter().filter(|event| matches!(event, Event::KittyImage { .. })).collect();
    assert_eq!(images.len(), 1);
    let Event::KittyImage { data, channels, .. } = images[0] else { unreachable!() };
    assert_eq!(&**data, pixels.as_slice());
    assert_eq!(*channels, 3);
}

/// A placement that names no cell box asks for the bitmap's natural size,
/// one image pixel per screen pixel. Dropping it leaves a blank rectangle
/// where the application believes it painted.
#[test]
fn kitty_placement_without_cell_box_uses_the_natural_size() {
    let pixels = [1u8; 20 * 10 * 4];

    let listener = Gfx::default();
    let mut terminal = Term::new(Config::default(), &TermSize::new(20, 8), listener.clone());
    let mut stream = StreamProcessor::default();
    stream.resize(WindowSize { num_cols: 20, num_lines: 8, cell_width: 10, cell_height: 10 });
    stream.feed(&mut terminal, &listener, b"");
    let mut wire = b"\x1b[1;1H".to_vec();
    wire.extend_from_slice(&kitty_command("a=t,i=15,f=32,s=20,v=10,m=0", &encoded(&pixels)));
    wire.extend_from_slice(&kitty_command("a=p,i=15", ""));
    stream.feed(&mut terminal, &listener, &wire);
    let known = kitty_images(&listener);
    assert_eq!(known.len(), 1);
    assert_eq!((known[0].1, known[0].2), (2, 1));

    // Before the pane reports its geometry there is no natural size to
    // compute, so one cell is the best answer — still visible, still there.
    let listener = Gfx::default();
    let mut terminal = Term::new(Config::default(), &TermSize::new(20, 8), listener.clone());
    let mut stream = StreamProcessor::default();
    stream.feed(&mut terminal, &listener, b"\x1b[1;1H");
    stream.feed(
        &mut terminal,
        &listener,
        &kitty_command("a=t,i=16,f=32,s=20,v=10,m=0", &encoded(&pixels)),
    );
    stream.feed(&mut terminal, &listener, &kitty_command("a=p,i=16", ""));
    let unknown = kitty_images(&listener);
    assert_eq!(unknown.len(), 1);
    assert_eq!((unknown[0].1, unknown[0].2), (1, 1));
}

/// A PDF page arrives the way a real reader sends it: one `a=T` header that
/// carries only the crop and destination cells, then 131072-byte
/// continuation chunks with no id of their own.
#[test]
fn bookokrat_style_page_transfer_publishes_when_it_completes() {
    use base64::Engine as _;
    let (width, height, crop_height) = (1036usize, 1340usize, 1150usize);
    let listener = Gfx::default();
    let mut terminal = Term::new(Config::default(), &TermSize::new(151, 40), listener.clone());
    let mut stream = StreamProcessor::default();
    let pixels = vec![9u8; width * height * 3];
    let encoded = base64::engine::general_purpose::STANDARD.encode(&pixels);

    stream.feed(&mut terminal, &listener, b"\x1b[5;2H");
    let mut rest = encoded.as_str();
    let header = format!(
        "a=T,t=d,f=24,s={width},v={height},i=1,p=1,C=1,q=1,x=0,y=0,w={width},h={crop_height},c=63,r=35,z=-1,m=1"
    );
    let (first, tail0) = rest.split_at(131072);
    rest = tail0;
    stream.feed(&mut terminal, &listener, &kitty_command(&header, first));
    while rest.len() > 4096 {
        let (piece, tail) = rest.split_at(4096);
        rest = tail;
        let data = kitty_command("m=1", piece);
        stream.feed(&mut terminal, &listener, &data);
    }
    stream.feed(&mut terminal, &listener, &kitty_command("m=0", rest));

    let images = kitty_images(&listener);
    assert_eq!(images.len(), 1, "one publication for the page");
    let (source, columns, rows, layout_width, layout_height) = images[0];
    assert_eq!((columns, rows), (63, 35));
    assert_eq!((layout_width, layout_height), (width as f32, height as f32));
    assert_eq!(source, SourceRect { x: 0, y: 0, width: width as u32, height: crop_height as u32 });
}
/// Three transfers of different depths queued back to back, the shape a
/// reader produces while it probes what the terminal accepts. Every one of
/// them has to publish, whatever its byte count is modulo three.
#[test]
fn mixed_depth_transfers_all_publish() {
    use base64::Engine as _;
    let encode = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
    let rgba = encode(&vec![7u8; 128 * 128 * 4]);
    let rgb = encode(&vec![7u8; 128 * 128 * 3]);
    let crop = encode(&vec![7u8; 256 * 200 * 3]);

    let listener = Gfx::default();
    let mut terminal = Term::new(Config::default(), &TermSize::new(120, 40), listener.clone());
    let mut stream = StreamProcessor::default();
    let mut wire = Vec::new();
    wire.extend_from_slice(&kitty_command("a=T,i=201,f=32,s=128,v=128", &rgba));
    wire.extend_from_slice(&kitty_command("a=T,i=202,f=24,s=128,v=128", &rgb));
    wire.extend_from_slice(&kitty_command(
        "a=T,t=d,f=24,i=203,s=256,v=200,c=20,r=10,x=0,y=0,w=256,h=200,z=-1",
        &crop,
    ));
    stream.feed(&mut terminal, &listener, &wire);

    let events = listener.0.lock().unwrap();
    let published = events
        .iter()
        .filter_map(|event| match event {
            Event::KittyImage { data, channels, placement } => {
                Some((placement.layout.width, placement.layout.height, *channels, data.len()))
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut published = published;
    published.sort_by(|left, right| left.partial_cmp(right).expect("no NaN geometry"));
    assert_eq!(
        published,
        vec![(128.0, 128.0, 3, 49152), (128.0, 128.0, 4, 65536), (256.0, 200.0, 3, 153600)]
    );
}
