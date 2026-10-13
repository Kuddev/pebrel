//! 由本机串口直接驱动的终端会话。
//!
//! 与 SSH 窗格一样不创建伪终端：字节流经 `StreamProcessor` 进入 grid，
//! 输入、缩放和关闭沿用统一的 `Msg` 协议。

use std::io::{self, Read, Write};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nebula_terminal::event::{Event as TerminalEvent, EventListener, WindowSize};
use nebula_terminal::event_loop::{EventLoopSender, Msg, StreamProcessor};
use nebula_terminal::sync::FairMutex;
use nebula_terminal::term::Term;

pub(crate) const SCHEME: &str = "serial:";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Parity {
    None,
    Even,
    Odd,
}

/// `serial:<port>[,<baud>[,<data><parity><stop>]]`，如 `serial:COM3,115200,8N1`；缺省 115200 8N1。
/// 不含空白和 shell 元字符，因此可以直接作为 SSH 主机列表里的一个条目保存。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SerialConfig {
    pub port: String,
    pub baud: u32,
    pub data_bits: u8,
    pub parity: Parity,
    pub stop_bits: u8,
}

impl FromStr for SerialConfig {
    type Err = io::Error;

    fn from_str(destination: &str) -> io::Result<Self> {
        let invalid = |message: String| io::Error::new(io::ErrorKind::InvalidInput, message);
        let rest = destination
            .strip_prefix(SCHEME)
            .ok_or_else(|| invalid(format!("not a serial destination: {destination}")))?;
        let mut parts = rest.split(',');
        let port = parts.next().unwrap_or_default();
        if port.is_empty() {
            return Err(invalid("serial port name is empty".into()));
        }
        let mut config = Self {
            port: port.into(),
            baud: 115_200,
            data_bits: 8,
            parity: Parity::None,
            stop_bits: 1,
        };
        if let Some(baud) = parts.next() {
            config.baud = baud
                .parse()
                .ok()
                .filter(|baud| *baud > 0)
                .ok_or_else(|| invalid(format!("invalid baud rate: {baud}")))?;
        }
        if let Some(frame) = parts.next() {
            let bad = || invalid(format!("invalid frame format: {frame}"));
            let &[data, parity, stop] = frame.as_bytes() else { return Err(bad()) };
            config.data_bits = match data {
                b'5'..=b'8' => data - b'0',
                _ => return Err(bad()),
            };
            config.parity = match parity.to_ascii_uppercase() {
                b'N' => Parity::None,
                b'E' => Parity::Even,
                b'O' => Parity::Odd,
                _ => return Err(bad()),
            };
            config.stop_bits = match stop {
                b'1' | b'2' => stop - b'0',
                _ => return Err(bad()),
            };
        }
        if parts.next().is_some() {
            return Err(invalid(format!("unexpected serial option in {destination}")));
        }
        Ok(config)
    }
}

/// 标签和提示里展示的端口名，不含参数。
pub(crate) fn display_name(destination: &str) -> &str {
    let rest = destination.strip_prefix(SCHEME).unwrap_or(destination);
    rest.split(',').next().unwrap_or(rest)
}

pub(crate) fn spawn_session<H>(
    destination: &str,
    terminal: Arc<FairMutex<Term<H>>>,
    event_proxy: H,
) -> io::Result<EventLoopSender>
where
    H: EventListener + Clone + Send + Sync + 'static,
{
    let config: SerialConfig = destination.parse()?;
    let mut writer = crate::platform::serial_port::open(&config)?;
    let mut reader = writer.try_clone()?;
    let (sender, receiver) = EventLoopSender::standalone()?;
    let stream = Arc::new(Mutex::new(StreamProcessor::default()));
    let stopped = Arc::new(AtomicBool::new(false));

    let read_terminal = Arc::clone(&terminal);
    let read_proxy = event_proxy.clone();
    let read_stream = Arc::clone(&stream);
    let read_stopped = Arc::clone(&stopped);
    std::thread::Builder::new().name("serial-read".into()).spawn(move || {
        let mut buf = [0u8; 4096];
        while !read_stopped.load(Ordering::Acquire) {
            match reader.read(&mut buf) {
                // 读超时：借机结束超时的同步更新，保持与 SSH 路径一致。
                Ok(0) => {
                    let mut stream = read_stream.lock().unwrap();
                    if stream.next_sync_timeout().is_some_and(|t| t <= std::time::Instant::now()) {
                        stream.stop_sync(&mut *read_terminal.lock());
                        read_proxy.send_event(TerminalEvent::Wakeup);
                    }
                },
                Ok(n) => {
                    let mut stream = read_stream.lock().unwrap();
                    stream.feed(&mut *read_terminal.lock(), &read_proxy, &buf[..n]);
                    read_proxy.send_event(TerminalEvent::Wakeup);
                },
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {},
                Err(error) => {
                    if !read_stopped.swap(true, Ordering::AcqRel) {
                        let message = format!("Serial port stopped: {error}");
                        render_error(&read_terminal, &read_proxy, &message);
                        read_terminal.lock().exit();
                    }
                    return;
                },
            }
        }
    })?;

    std::thread::Builder::new().name("serial-write".into()).spawn(move || {
        while !stopped.load(Ordering::Acquire) {
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(Msg::Input(bytes)) => {
                    if let Err(error) = writer.write_all(&bytes).and_then(|()| writer.flush()) {
                        if !stopped.swap(true, Ordering::AcqRel) {
                            let message = format!("Serial write failed: {error}");
                            render_error(&terminal, &event_proxy, &message);
                            terminal.lock().exit();
                        }
                        return;
                    }
                },
                Ok(Msg::Resize(size) | Msg::ResizeGrid(size)) => {
                    resize(&terminal, &stream, &event_proxy, size);
                },
                Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(_) | Err(RecvTimeoutError::Timeout) => {},
            }
        }
        stopped.store(true, Ordering::Release);
    })?;
    Ok(sender)
}

fn resize<H: EventListener>(
    terminal: &Arc<FairMutex<Term<H>>>,
    stream: &Mutex<StreamProcessor>,
    event_proxy: &H,
    size: WindowSize,
) {
    terminal.lock().resize(size);
    stream.lock().unwrap().resize(size);
    event_proxy.send_event(TerminalEvent::Wakeup);
}

fn render_error<H: EventListener>(
    terminal: &Arc<FairMutex<Term<H>>>,
    event_proxy: &H,
    message: &str,
) {
    let text = format!("\r\n\x1b[31m{message}\x1b[0m\r\n");
    StreamProcessor::default().feed(&mut *terminal.lock(), event_proxy, text.as_bytes());
    event_proxy.send_event(TerminalEvent::Wakeup);
}

#[cfg(test)]
mod tests;
