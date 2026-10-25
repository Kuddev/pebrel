use super::*;
use crate::{
    event::OnResize,
    term::{Config, test::TermSize},
};
use std::{os::fd::AsRawFd, sync::Mutex};
#[derive(Clone, Default)]
struct Listener(Arc<Mutex<Vec<Event>>>);
impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        self.0.lock().unwrap().push(event);
    }
}
struct Pty(File);
impl tty::EventedReadWrite for Pty {
    type Reader = File;
    type Writer = File;
    unsafe fn register(&mut self, p: &Arc<Poller>, e: PollingEvent, m: PollMode) -> io::Result<()> {
        unsafe { p.add_with_mode(&self.0, e, m) }
    }
    fn reregister(&mut self, p: &Arc<Poller>, e: PollingEvent, m: PollMode) -> io::Result<()> {
        p.modify_with_mode(&self.0, e, m)
    }
    fn deregister(&mut self, p: &Arc<Poller>) -> io::Result<()> {
        p.delete(&self.0)
    }
    fn reader(&mut self) -> &mut File {
        &mut self.0
    }
    fn writer(&mut self) -> &mut File {
        &mut self.0
    }
}
impl tty::EventedPty for Pty {
    fn next_child_event(&mut self) -> Option<tty::ChildEvent> {
        None
    }
}
impl OnResize for Pty {
    fn on_resize(&mut self, _: WindowSize) {}
}
fn image() -> &'static str {
    "\x1b]1337;File=inline=1;width=1;height=1:iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==\x07"
}
fn setup() -> (EventLoop<Pty, Listener>, State, Listener, File) {
    let pair = rustix_openpty::openpty(None, None).unwrap();
    let mut termios = rustix_openpty::rustix::termios::tcgetattr(&pair.user).unwrap();
    termios.make_raw();
    rustix_openpty::rustix::termios::tcsetattr(
        &pair.user,
        rustix_openpty::rustix::termios::OptionalActions::Now,
        &termios,
    )
    .unwrap();
    let flags = rustix_openpty::rustix::fs::fcntl_getfl(&pair.controller).unwrap();
    rustix_openpty::rustix::fs::fcntl_setfl(
        &pair.controller,
        flags | rustix_openpty::rustix::fs::OFlags::NONBLOCK,
    )
    .unwrap();
    let listener = Listener::default();
    let terminal = Arc::new(FairMutex::new(Term::new(
        Config::default(),
        &TermSize::new(20, 8),
        listener.clone(),
    )));
    let event_loop =
        EventLoop::new(terminal, listener.clone(), Pty(File::from(pair.controller)), false, false)
            .unwrap();
    let mut state = State::default();
    state.stream.resize(WindowSize { num_cols: 20, num_lines: 8, cell_width: 10, cell_height: 20 });
    (event_loop, state, listener, File::from(pair.user))
}
fn read(
    loop_: &mut EventLoop<Pty, Listener>,
    state: &mut State,
    slave: &mut File,
    bytes: &[u8],
) -> usize {
    slave.write_all(bytes).unwrap();
    let mut event = libc::pollfd { fd: loop_.pty.0.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    assert_eq!(unsafe { libc::poll(&mut event, 1, 1000) }, 1);
    let consumed = loop_.pty_read::<io::Sink>(state, &mut [0; 4096], None).unwrap();
    assert_eq!(consumed, bytes.len());
    consumed
}
fn observe(prefix: &str, images: usize, closed: bool) -> usize {
    let (mut loop_, mut state, listener, mut slave) = setup();
    let bytes = format!(
        "{prefix}\x1b[?2026h{}{}",
        image().repeat(images),
        if closed { "\x1b[?2026l" } else { "" }
    );
    let wire = read(&mut loop_, &mut state, &mut slave, bytes.as_bytes());
    if prefix.contains('A') {
        assert_eq!(loop_.terminal.lock().grid()[Line(0)][Column(0)].c, 'A');
    }
    let events = listener.0.lock().unwrap();
    let wakes = events.iter().filter(|e| matches!(e, Event::Wakeup)).count();
    let placements = events.iter().filter(|e| matches!(e, Event::InlineImage { .. })).count();
    assert_eq!(wire, bytes.len());
    assert_eq!(placements, if closed { images } else { 0 });
    wakes
}
#[test]
fn committed_prefix_has_actual_wakeup_before_open_image_batch() {
    assert_eq!(observe("A", 1, false), 1);
}
#[test]
fn completed_frame_has_actual_wakeup_before_next_open_image_batch() {
    assert_eq!(observe("\x1b[?2026hA\x1b[?2026l", 1, false), 1);
}
#[test]
fn committed_prefix_refreshes_with_multiple_images() {
    assert_eq!(observe("A", 3, false), 1);
    assert_eq!(observe("A", 1, true), 1);
}
#[test]
fn synchronized_image_waits_for_commit_at_every_split() {
    let bytes = image().as_bytes();
    for split in 0..=bytes.len() {
        let (mut loop_, mut state, listener, mut slave) = setup();
        read(&mut loop_, &mut state, &mut slave, b"\x1b[?2026h");
        listener.0.lock().unwrap().clear();
        for part in [&bytes[..split], &bytes[split..]] {
            if !part.is_empty() {
                read(&mut loop_, &mut state, &mut slave, part);
            }
            assert!(listener.0.lock().unwrap().is_empty(), "split={split}");
        }
        read(&mut loop_, &mut state, &mut slave, b"\x1b[?2026l");
        let events = listener.0.lock().unwrap();
        assert_eq!(events.iter().filter(|e| matches!(e, Event::Wakeup)).count(), 1);
        assert_eq!(events.iter().filter(|e| matches!(e, Event::InlineImage { .. })).count(), 1);
    }
}
#[test]
fn separately_read_prefix_and_final_commit_both_refresh() {
    let (mut loop_, mut state, listener, mut slave) = setup();
    read(&mut loop_, &mut state, &mut slave, b"A");
    assert_eq!(listener.0.lock().unwrap().iter().filter(|e| matches!(e, Event::Wakeup)).count(), 1);
    listener.0.lock().unwrap().clear();
    read(&mut loop_, &mut state, &mut slave, format!("\x1b[?2026h{}", image()).as_bytes());
    listener.0.lock().unwrap().clear();
    read(&mut loop_, &mut state, &mut slave, b"\x1b[?2026l");
    assert_eq!(listener.0.lock().unwrap().iter().filter(|e| matches!(e, Event::Wakeup)).count(), 1);
}
