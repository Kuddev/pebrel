//! Graphics wire/replay integration: real parser, grid ownership and protocol replies.

use super::*;
use crate::term::{Config, test::TermSize};

#[derive(Clone, Default)]
struct Gfx(std::sync::Arc<std::sync::Mutex<Vec<Event>>>);

impl Gfx {
    fn replies(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                Event::PtyWrite(reply) => Some(reply.clone()),
                _ => None,
            })
            .collect()
    }
}

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

fn resource(terminal: &Term<Gfx>, row: i32, column: usize) -> u64 {
    terminal.grid()[Line(row)][Column(column)]
        .image()
        .unwrap()
        .placement
        .as_ref()
        .unwrap()
        .resource
        .id
}

#[test]
fn kitty_query_validates_data_quiet_flags_and_preserves_existing_image() {
    let listener = Gfx::default();
    let mut terminal = Term::new(Config::default(), &TermSize::new(20, 8), listener.clone());
    let mut stream = StreamProcessor::default();
    stream.feed(&mut terminal, &listener, &kitty_command("a=T,i=7,s=1,v=1,C=1", "AQIDBA=="));
    let original = resource(&terminal, 0, 0);
    listener.0.lock().unwrap().clear();
    stream.feed(&mut terminal, &listener, b"\x1b[?2026h");
    for (control, data) in [
        ("a=q,i=7,s=1,v=1,f=24,q=1", "AAAA"),
        ("a=q,i=7,s=0,v=1,f=24,q=1", "AAAA"),
        ("a=q,i=8,s=1,v=1,f=24,q=0", "AA"),
        ("a=q,i=9,s=1,v=1,t=f,q=2", "AAAA"),
        ("a=q,i=10,s=1,v=1,U=1", "AQIDBA=="),
        ("a=q,i=11,s=1,v=1,f=24", "AAAA"),
    ] {
        stream.feed(&mut terminal, &listener, &kitty_command(control, data));
    }
    assert!(stream.next_sync_timeout().is_some(), "queries do not prematurely end a frame");
    assert_eq!(
        listener.replies(),
        [
            "\x1b_Gi=7;EINVAL\x1b\\",
            "\x1b_Gi=8;EINVAL\x1b\\",
            "\x1b_Gi=10;ENOTSUP\x1b\\",
            "\x1b_Gi=11;OK\x1b\\",
        ]
    );
    stream.stop_sync(&mut terminal);
    assert_eq!(resource(&terminal, 0, 0), original);
    stream.feed(&mut terminal, &listener, &kitty_command("a=p,i=7,P=999,Q=1,C=1", ""));
    assert_eq!(listener.replies().last().unwrap(), "\x1b_Gi=7;ENOTSUP\x1b\\");
    assert_eq!(resource(&terminal, 0, 0), original);
}

#[test]
fn kitty_chunked_ack_keeps_header_identity_and_placement_until_final_chunk() {
    let listener = Gfx::default();
    let mut terminal = Term::new(Config::default(), &TermSize::new(20, 8), listener.clone());
    let mut stream = StreamProcessor::default();
    stream.feed(&mut terminal, &listener, &kitty_command("a=T,i=9,p=3,s=1,v=1,m=1,C=1", "AQID"));
    assert!(listener.replies().is_empty());
    stream.feed(&mut terminal, &listener, &kitty_command("m=0", "BA=="));
    stream.feed(&mut terminal, &listener, &kitty_command("a=p,i=9,p=4,c=1,r=1,C=1,q=1", ""));
    stream.feed(&mut terminal, &listener, &kitty_command("a=p,i=99,p=4,q=1", ""));
    assert_eq!(listener.replies(), ["\x1b_Gi=9,p=3;OK\x1b\\", "\x1b_Gi=99,p=4;ENOENT\x1b\\"]);
}

#[test]
fn kitty_named_data_is_independent_between_primary_and_alternate_screens() {
    let listener = Gfx::default();
    let mut terminal = Term::new(Config::default(), &TermSize::new(20, 8), listener.clone());
    let mut stream = StreamProcessor::default();
    stream.feed(&mut terminal, &listener, &kitty_command("a=T,i=7,s=1,v=1,C=1", "AQIDBA=="));
    let primary = resource(&terminal, 0, 0);
    stream.feed(&mut terminal, &listener, b"\x1b[?1049h");
    stream.feed(&mut terminal, &listener, &kitty_command("a=T,i=7,s=1,v=1,C=1", "BQYHCA=="));
    assert_ne!(resource(&terminal, 0, 0), primary);
    stream.feed(&mut terminal, &listener, &kitty_command("a=d,d=I,i=7", ""));
    assert!(terminal.grid()[Line(0)][Column(0)].image().is_none());
    stream.feed(&mut terminal, &listener, b"\x1b[?1049l");
    assert_eq!(resource(&terminal, 0, 0), primary);
}

#[test]
fn kitty_selective_delete_keeps_the_other_placement_and_releases_the_last_one() {
    let listener = Gfx::default();
    let mut terminal = Term::new(Config::default(), &TermSize::new(20, 8), listener.clone());
    let mut stream = StreamProcessor::default();
    stream.feed(&mut terminal, &listener, &kitty_command("a=T,i=7,p=1,s=1,v=1,C=1", "AQIDBA=="));
    let original = resource(&terminal, 0, 0);
    stream.feed(&mut terminal, &listener, b"\x1b[1;6H");
    stream.feed(&mut terminal, &listener, &kitty_command("a=p,i=7,p=2,C=1", ""));
    assert_eq!(resource(&terminal, 0, 5), original);
    stream.feed(&mut terminal, &listener, &kitty_command("a=d,d=I,i=7,p=1", ""));
    assert!(terminal.grid()[Line(0)][Column(0)].image().is_none());
    assert_eq!(resource(&terminal, 0, 5), original);
    stream.feed(&mut terminal, &listener, &kitty_command("a=d,d=I,i=7,p=2", ""));
    stream.feed(&mut terminal, &listener, &kitty_command("a=p,i=7", ""));
    assert_eq!(listener.replies().last().unwrap(), "\x1b_Gi=7;ENOENT\x1b\\");
}

#[test]
fn kitty_default_transmit_anonymous_display_and_cursor_movement_are_distinct() {
    let listener = Gfx::default();
    let mut terminal = Term::new(Config::default(), &TermSize::new(20, 8), listener.clone());
    let mut stream = StreamProcessor::default();
    stream.feed(&mut terminal, &listener, &kitty_command("i=42,s=1,v=1", "AQIDBA=="));
    assert!(terminal.grid()[Line(0)][Column(0)].image().is_none());
    stream.feed(&mut terminal, &listener, &kitty_command("a=p,i=42,c=2,r=2", ""));
    assert_eq!(terminal.grid().cursor.point, crate::index::Point::new(Line(2), Column(2)));
    stream.feed(&mut terminal, &listener, &kitty_command("a=T,s=1,v=1,c=1,r=1,C=1", "AQIDBA=="));
    assert!(terminal.grid()[Line(2)][Column(2)].image().is_some());
    assert_eq!(terminal.grid().cursor.point, crate::index::Point::new(Line(2), Column(2)));
}

#[test]
fn kitty_reset_replays_before_a_following_transfer_in_the_same_frame() {
    let listener = Gfx::default();
    let mut terminal = Term::new(Config::default(), &TermSize::new(20, 8), listener.clone());
    let mut stream = StreamProcessor::default();
    stream.feed(&mut terminal, &listener, &kitty_command("a=T,i=7,s=1,v=1,C=1", "AQIDBA=="));
    let before = resource(&terminal, 0, 0);
    let mut wire = b"\x1b[?2026h\x1bc".to_vec();
    wire.extend(kitty_command("a=T,i=7,s=1,v=1,C=1", "BQYHCA=="));
    wire.extend(b"\x1b[?2026l");
    stream.feed(&mut terminal, &listener, &wire);
    assert_ne!(resource(&terminal, 0, 0), before);
    assert!(stream.pending_images.is_empty());
}

#[test]
fn kitty_synchronized_placement_uses_the_replayed_cursor_and_timeout_publishes_it() {
    let listener = Gfx::default();
    let mut terminal = Term::new(Config::default(), &TermSize::new(20, 8), listener.clone());
    let mut stream = StreamProcessor::default();
    let mut wire = b"\x1b[?2026h\x1b[3;5H".to_vec();
    wire.extend(kitty_command("a=T,i=7,f=32,s=2,v=1,c=2,r=1,C=1", &rgba2x1()));
    stream.feed(&mut terminal, &listener, &wire);
    assert!(listener.0.lock().unwrap().is_empty());
    assert!(terminal.grid()[Line(2)][Column(4)].image().is_none());
    assert!(stream.next_sync_timeout().is_some());
    stream.stop_sync(&mut terminal);
    assert!(terminal.grid()[Line(2)][Column(4)].image().is_some());
    assert_eq!(
        listener
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|event| matches!(event, Event::KittyImage { .. }))
            .count(),
        1
    );
    assert!(stream.pending_images.is_empty());
}

#[test]
fn kitty_sync_accepts_more_than_sixteen_small_chunks_without_ending_the_frame() {
    use base64::Engine as _;
    let listener = Gfx::default();
    let mut terminal = Term::new(Config::default(), &TermSize::new(20, 8), listener.clone());
    let mut stream = StreamProcessor::default();
    let encoded = base64::engine::general_purpose::STANDARD.encode([7u8; 2400]);
    stream.feed(&mut terminal, &listener, b"\x1b[?2026h\x1b[3;5H");
    for (index, chunk) in encoded.as_bytes().chunks(8).enumerate() {
        let control = if index == 0 {
            "a=T,i=7,f=32,s=30,v=20,c=2,r=2,C=1,m=1"
        } else if (index + 1) * 8 == encoded.len() {
            "m=0"
        } else {
            "m=1"
        };
        stream.feed(
            &mut terminal,
            &listener,
            &kitty_command(control, std::str::from_utf8(chunk).unwrap()),
        );
        assert!(stream.next_sync_timeout().is_some());
        assert!(listener.0.lock().unwrap().is_empty());
    }
    stream.feed(&mut terminal, &listener, b"\x1b[?2026l");
    assert!(stream.pending_images.is_empty());
    assert!(terminal.grid()[Line(2)][Column(4)].image().is_some());
    assert_eq!(
        listener
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|event| matches!(event, Event::KittyImage { .. }))
            .count(),
        1
    );
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
    wire.extend_from_slice(&kitty_command("a=p,i=7,p=1,x=0,y=0,w=1,h=1,c=1,r=1,C=1", ""));
    wire.extend_from_slice(b"\x1b[3;6H");
    wire.extend_from_slice(&kitty_command("a=p,i=7,p=2,x=1,y=0,w=1,h=1,c=1,r=1,C=1", ""));
    stream.feed(&mut terminal, &listener, &wire);

    let events = listener.0.lock().unwrap();
    let images: Vec<&Event> =
        events.iter().filter(|event| matches!(event, Event::KittyImage { .. })).collect();
    // One publication per image, not one per placement: a preview arrives as
    // thousands of placements.
    assert_eq!(images.len(), 1);
    let Event::KittyImage { data, channels, placement } = images[0] else { unreachable!() };
    assert_eq!(data.upgrade().unwrap().len(), 8);
    assert_eq!(*channels, 4);
    assert_eq!(placement.layout.width, 2.0);
    assert_eq!(placement.layout.height, 1.0);
    assert_eq!((placement.layout.columns, placement.layout.rows), (1, 1));
    // The cells the cursor visited carry the image, keyed to the tile the
    // application sampled, so a later scroll re-samples the right pixels.
    let cell = terminal.grid()[Line(2)][Column(4)].image().expect("placed at the cursor");
    assert_eq!(cell.placement.as_ref().unwrap().resource.id, placement.id);
    assert_eq!((cell.column, cell.row), (0, 0));
    let next = terminal.grid()[Line(2)][Column(5)].image().expect("second cell");
    assert_eq!((next.column, next.row), (0, 0));
    assert_eq!(next.placement.as_ref().unwrap().source_x, 1.0);
    assert_eq!(next.placement.as_ref().unwrap().resource.id, placement.id);
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
    assert_eq!((data.upgrade().unwrap().len(), *channels), (2 * 2 * 3, 3));
    assert_eq!((placement.layout.columns, placement.layout.rows), (2, 2));
}

#[test]
fn kitty_declared_cells_scale_the_full_bitmap_even_with_real_cell_metrics() {
    let listener = Gfx::default();
    let mut terminal = Term::new(Config::default(), &TermSize::new(20, 8), listener.clone());
    let mut stream = StreamProcessor::default();
    stream.resize(WindowSize { num_cols: 20, num_lines: 8, cell_width: 9, cell_height: 20 });
    stream.feed(
        &mut terminal,
        &listener,
        &kitty_command("a=T,i=7,f=32,s=2,v=1,c=4,r=2", &rgba2x1()),
    );
    let events = listener.0.lock().unwrap();
    let layout = events
        .iter()
        .find_map(|event| match event {
            Event::KittyImage { placement, .. } => Some(placement.layout),
            _ => None,
        })
        .unwrap();
    assert_eq!(layout.width / layout.cell_width, 4.0);
    assert_eq!(layout.height / layout.cell_height, 2.0);
    assert_eq!((layout.columns, layout.rows), (4, 2));
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
    assert_eq!(
        listener
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|event| matches!(event, Event::KittyImage { .. }))
            .count(),
        1
    );

    stream.feed(&mut terminal, &listener, b"\x1b[5;3H");
    stream.feed(&mut terminal, &listener, &kitty_command("a=t,i=4,f=32,s=2,v=1,m=0", &rgba2x1()));
    stream.feed(&mut terminal, &listener, &kitty_command("a=p,i=4,c=2,r=1", ""));
    assert!(terminal.grid()[Line(4)][Column(2)].image().is_some());
    // A second image under the same id must reach the frontend again.
    assert_eq!(
        listener
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|event| matches!(event, Event::KittyImage { .. }))
            .count(),
        2
    );
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
