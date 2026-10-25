use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::ops::Deref;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use nebula_terminal::event::{Event as TerminalEvent, WindowSize};
use nebula_terminal::event_loop::{Msg, StreamProcessor};
use nebula_terminal::sync::FairMutex;
use nebula_terminal::term::Term;
use russh::{Channel, ChannelMsg, ChannelReadHalf, ChannelWriteHalf, client};
use tokio::sync::{mpsc, watch};

use super::{
    ResumeTarget, SessionError, SharedSession, SshDestination, SshEventHost, SshStage, resume,
};

pub(super) const NETWORK_TIMEOUT: Duration = Duration::from_secs(20);
const AUTH_RESPONSE_TIMEOUT: Duration = Duration::from_secs(300);

pub(super) async fn authentication<T, Error>(
    operation: &str,
    future: impl Future<Output = Result<T, Error>>,
) -> Result<T, SessionError>
where
    Error: Into<SessionError>,
{
    network_with_budget(operation, AUTH_RESPONSE_TIMEOUT, future).await
}

pub(super) async fn network<T, Error>(
    operation: &str,
    future: impl Future<Output = Result<T, Error>>,
) -> Result<T, SessionError>
where
    Error: Into<SessionError>,
{
    network_with_budget(operation, NETWORK_TIMEOUT, future).await
}

async fn network_with_budget<T, Error>(
    operation: &str,
    budget: Duration,
    future: impl Future<Output = Result<T, Error>>,
) -> Result<T, SessionError>
where
    Error: Into<SessionError>,
{
    tokio::time::timeout(budget, future)
        .await
        .map_err(|_| timed_out(operation))?
        .map_err(Into::into)
}

fn timed_out(operation: &str) -> SessionError {
    io::Error::new(io::ErrorKind::TimedOut, format!("SSH {operation} timed out")).into()
}

async fn cancelled(receiver: &mut watch::Receiver<bool>) {
    while !*receiver.borrow_and_update() {
        if receiver.changed().await.is_err() {
            return;
        }
    }
}

struct CancelOnDrop(Option<watch::Sender<bool>>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(sender) = &self.0 {
            sender.send_replace(true);
        }
    }
}

#[derive(Clone)]
pub(super) struct Handshake {
    prompting: watch::Sender<bool>,
    cancelled: watch::Sender<bool>,
}

impl Default for Handshake {
    fn default() -> Self {
        Self { prompting: watch::channel(false).0, cancelled: watch::channel(false).0 }
    }
}

impl Handshake {
    pub(super) async fn confirm(&self, future: impl Future<Output = bool>) -> bool {
        let mut cancellation = self.cancelled.subscribe();
        self.prompting.send_replace(true);
        let result = tokio::select! {
            biased;
            _ = cancelled(&mut cancellation) => false,
            accepted = future => accepted,
        };
        self.prompting.send_replace(false);
        result
    }

    pub(super) async fn connect<T>(
        &self,
        future: impl Future<Output = Result<T, russh::Error>>,
    ) -> Result<T, SessionError> {
        self.connect_with_budget(future, NETWORK_TIMEOUT).await
    }

    async fn connect_with_budget<T>(
        &self,
        future: impl Future<Output = Result<T, russh::Error>>,
        mut remaining: Duration,
    ) -> Result<T, SessionError> {
        let mut guard = CancelOnDrop(Some(self.cancelled.clone()));
        let mut prompting = self.prompting.subscribe();
        tokio::pin!(future);
        loop {
            let paused = *prompting.borrow_and_update();
            let started = tokio::time::Instant::now();
            tokio::select! {
                biased;
                result = &mut future => {
                    guard.0 = None;
                    return result.map_err(Into::into);
                },
                _ = prompting.changed() => {},
                _ = tokio::time::sleep(remaining), if !paused => return Err(timed_out("handshake")),
            }
            if !paused {
                remaining = remaining.saturating_sub(started.elapsed());
            }
        }
    }
}

fn input_bridge(receiver: Receiver<Msg>) -> (mpsc::UnboundedReceiver<Msg>, watch::Receiver<bool>) {
    let (input_tx, input_rx) = mpsc::unbounded_channel();
    let (cancel_tx, cancel_rx) = watch::channel(false);
    tokio::task::spawn_blocking(move || {
        while !input_tx.is_closed() {
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                    let _ = cancel_tx.send(true);
                    break;
                },
                Ok(message) => {
                    if input_tx.send(message).is_err() {
                        break;
                    }
                },
                Err(RecvTimeoutError::Timeout) => {},
            }
        }
    });
    (input_rx, cancel_rx)
}

pub(super) async fn run<H: SshEventHost>(
    destination: String,
    initial_remote_cwd: Option<String>,
    resume_target: Option<ResumeTarget>,
    initial_size: WindowSize,
    terminal: Arc<FairMutex<Term<H>>>,
    event_proxy: H,
    receiver: Receiver<Msg>,
) {
    let (mut input, mut cancellation) = input_bridge(receiver);
    let work = async {
        super::report_stage(Some(&event_proxy), SshStage::Resolve);
        let raw = destination.clone();
        let profiles_path = crate::display::nebula_data_dir().join("ssh_profiles.json");
        let (resolved, profile) = network(
            "address resolution",
            tokio::task::spawn_blocking(move || {
                let resolved = SshDestination::resolve(&raw)?;
                let profiles = crate::ssh_profiles::SshProfiles::load(&profiles_path)?;
                Ok::<_, io::Error>((resolved, profiles.for_destination(&raw)))
            }),
        )
        .await??;
        let mut acquired =
            super::authenticated_session_at(&resolved, &profile, Some(&event_proxy), 0).await?;
        let (mut channel, hook_token) = match open_shell(
            &acquired,
            initial_size,
            initial_remote_cwd.as_deref(),
            resume_target.as_ref(),
            &event_proxy,
        )
        .await
        {
            Ok(opened) => opened,
            Err(first_error) if acquired.reused => {
                super::evict_pooled_session(&acquired.key, &acquired.session).await;
                log::info!("SSH pooled channel failed; reconnecting: {first_error}");
                acquired =
                    super::authenticated_session_at(&resolved, &profile, Some(&event_proxy), 0)
                        .await?;
                open_shell(
                    &acquired,
                    initial_size,
                    initial_remote_cwd.as_deref(),
                    resume_target.as_ref(),
                    &event_proxy,
                )
                .await?
            },
            Err(error) => return Err(error),
        };
        let (_pane_connection_scope, connection) =
            super::pane_connection::PaneConnectionScope::new(&acquired.session);
        event_proxy.ssh_pane_connection(Some(connection));
        super::report_stage(Some(&event_proxy), SshStage::Ready);
        let identity = channel.identity.clone();
        let result = tokio::select! {
            biased;
            result = pump(&mut channel, hook_token, initial_size, &terminal, &event_proxy, &mut input) => result,
            never = resume::monitor(&acquired.session, identity, resume_target.as_ref(), &event_proxy) => match never {},
        };
        if result.is_err() || acquired.session.is_closed() {
            super::evict_pooled_session(&acquired.key, &acquired.session).await;
        }
        result
    };
    drive(work, &mut cancellation, &terminal, &event_proxy).await;
}

async fn drive<H: SshEventHost>(
    work: impl Future<Output = Result<(), SessionError>>,
    cancellation: &mut watch::Receiver<bool>,
    terminal: &Arc<FairMutex<Term<H>>>,
    event_proxy: &H,
) {
    let result = tokio::select! {
        biased;
        _ = cancelled(cancellation) => Ok(()),
        result = work => result,
    };
    finish(result, terminal, event_proxy);
}

fn finish<H: SshEventHost>(
    result: Result<(), SessionError>,
    terminal: &Arc<FairMutex<Term<H>>>,
    event_proxy: &H,
) {
    event_proxy.ssh_pane_connection(None);
    match result {
        Ok(()) => terminal.lock().exit(),
        Err(error) => {
            terminal.lock().end_program_status_activity();
            log::info!("SSH session stopped: {error}");
            super::report_stage(Some(event_proxy), SshStage::Failed(error.to_string()));
            super::render_error(terminal, event_proxy, &format!("SSH connection stopped: {error}"));
        },
    }
    event_proxy.send_event(TerminalEvent::Wakeup);
}

struct Opening {
    key: String,
    session: SharedSession,
    finished: bool,
}

impl Drop for Opening {
    fn drop(&mut self) {
        if !self.finished {
            let key = self.key.clone();
            let session = self.session.clone();
            tokio::spawn(async move {
                super::evict_pooled_session(&key, &session).await;
            });
        }
    }
}

pub(super) struct ShellChannel {
    channel: Option<Arc<ChannelWriteHalf<client::Msg>>>,
    reader: ChannelReadHalf,
    pending: VecDeque<ChannelMsg>,
    pending_bytes: usize,
    identity: Arc<std::sync::Mutex<Option<resume::PtyIdentity>>>,
    identity_input: Option<resume::IdentityInput>,
}

/// The same close-on-drop ownership applies to shell and short-lived exec channels.
pub(super) fn own_channel(channel: Channel<client::Msg>) -> ShellChannel {
    let (reader, writer) = channel.split();
    ShellChannel {
        channel: Some(Arc::new(writer)),
        reader,
        pending: VecDeque::new(),
        pending_bytes: 0,
        identity: Default::default(),
        identity_input: None,
    }
}

impl ShellChannel {
    pub(super) async fn wait(&mut self) -> Option<ChannelMsg> {
        self.reader.wait().await
    }

    async fn next_message(&mut self) -> Option<ChannelMsg> {
        match self.pending.pop_front() {
            Some(message) => Some(message),
            None => self.wait().await,
        }
    }

    /// Poll only what is already available; never wait while the grid is locked.
    fn ready_message(&mut self) -> Option<ChannelMsg> {
        if let Some(message) = self.pending.pop_front() {
            return Some(message);
        }
        let mut future = std::pin::pin!(self.wait());
        match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(message) => message,
            Poll::Pending => None,
        }
    }

    pub(super) async fn finish(&mut self) -> Result<(), SessionError> {
        if let Some(channel) = &self.channel {
            // Retain ownership across await: cancellation still runs close-on-drop.
            network("channel close", channel.close()).await?;
        }
        self.channel.take();
        Ok(())
    }
}

impl Deref for ShellChannel {
    type Target = ChannelWriteHalf<client::Msg>;
    fn deref(&self) -> &Self::Target {
        self.channel.as_ref().expect("SSH channel exists until drop")
    }
}

impl Drop for ShellChannel {
    fn drop(&mut self) {
        if let Some(channel) = self.channel.take() {
            tokio::spawn(async move {
                let _ = network("channel close", channel.close()).await;
            });
        }
    }
}

async fn open_shell<H: SshEventHost>(
    acquired: &super::AcquiredSession,
    size: WindowSize,
    remote_cwd: Option<&str>,
    target: Option<&ResumeTarget>,
    progress: &H,
) -> Result<(ShellChannel, String), SessionError> {
    open_shell_with_budget(acquired, size, remote_cwd, target, progress, NETWORK_TIMEOUT).await
}

async fn open_shell_with_budget<H: SshEventHost>(
    acquired: &super::AcquiredSession,
    size: WindowSize,
    remote_cwd: Option<&str>,
    target: Option<&ResumeTarget>,
    progress: &H,
    budget: Duration,
) -> Result<(ShellChannel, String), SessionError> {
    super::report_stage(Some(progress), SshStage::OpenShell);
    let mut opening =
        Opening { key: acquired.key.clone(), session: acquired.session.clone(), finished: false };
    let result = network_with_budget(
        "shell channel",
        budget,
        open_shell_channel(&acquired.session, size, remote_cwd, target),
    )
    .await;
    if result.is_ok() {
        opening.finished = true;
    }
    result
}

async fn open_shell_channel(
    session: &SharedSession,
    size: WindowSize,
    remote_cwd: Option<&str>,
    target: Option<&ResumeTarget>,
) -> Result<(ShellChannel, String), SessionError> {
    let hook_token = super::remote_hook_token()?;
    let bootstrap = super::integration::prepare(session, &hook_token).await;
    let mut channel = own_channel(session.channel_open_session().await?);
    channel
        .request_pty(
            true,
            "xterm-256color",
            u32::from(size.num_cols),
            u32::from(size.num_lines),
            u32::from(size.cell_width) * u32::from(size.num_cols),
            u32::from(size.cell_height) * u32::from(size.num_lines),
            &[],
        )
        .await?;
    wait_request_success(&mut channel, "PTY").await?;
    if bootstrap.is_some() || target.is_some() || resume::supported(session).await {
        let command = resume::startup(&hook_token, bootstrap.as_deref(), target, remote_cwd);
        match super::integration::start(&mut channel, &command).await {
            Ok(()) => {
                channel.identity_input =
                    Some(resume::IdentityInput::new(&hook_token, channel.identity.clone()));
                return Ok((channel, hook_token));
            },
            Err(error) => {
                log::debug!("SSH integrated shell rejected; opening ordinary shell: {error}");
                channel.finish().await?;
                channel = own_channel(session.channel_open_session().await?);
                channel
                    .request_pty(
                        true,
                        "xterm-256color",
                        u32::from(size.num_cols),
                        u32::from(size.num_lines),
                        u32::from(size.cell_width) * u32::from(size.num_cols),
                        u32::from(size.cell_height) * u32::from(size.num_lines),
                        &[],
                    )
                    .await?;
                wait_request_success(&mut channel, "PTY").await?;
            },
        }
    }
    channel.set_env(false, "NEBULA_REMOTE_HOOK_TOKEN", hook_token.clone()).await?;
    let _ = channel.set_env(false, "NEBULA_PANE_REMOTE", "1").await;
    channel.request_shell(true).await?;
    wait_request_success(&mut channel, "shell").await?;
    if let Some(command) = super::initial_remote_cd_command(remote_cwd) {
        channel.data_bytes(command).await?;
    }
    Ok((channel, hook_token))
}

pub(super) async fn wait_request_success(
    channel: &mut ShellChannel,
    request: &str,
) -> Result<(), SessionError> {
    loop {
        match channel.wait().await {
            Some(ChannelMsg::Success) => return Ok(()),
            Some(ChannelMsg::WindowAdjusted { .. }) => {},
            Some(message @ (ChannelMsg::Data { .. } | ChannelMsg::ExtendedData { .. })) => {
                let length = match &message {
                    ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => data.len(),
                    _ => unreachable!(),
                };
                channel.pending_bytes = channel.pending_bytes.saturating_add(length);
                if channel.pending_bytes > 1024 * 1024 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "SSH server sent too much data before shell confirmation",
                    )
                    .into());
                }
                channel.pending.push_back(message)
            },
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    format!("SSH server did not accept {request} request"),
                )
                .into());
            },
        }
    }
}

async fn pump<H: SshEventHost>(
    channel: &mut ShellChannel,
    hook_token: String,
    initial_size: WindowSize,
    terminal: &FairMutex<Term<H>>,
    event_proxy: &H,
    input: &mut mpsc::UnboundedReceiver<Msg>,
) -> Result<(), SessionError> {
    let mut stream = StreamProcessor::default();
    stream.resize(initial_size);
    stream.set_remote_hook_token(hook_token);
    let result = pump_stream(channel, &mut stream, terminal, event_proxy, input).await;
    if let Some(filter) = &mut channel.identity_input {
        let trailing = filter.finish();
        if !trailing.is_empty() {
            stream.feed(&mut *terminal.lock(), event_proxy, &trailing);
            event_proxy.send_event(TerminalEvent::Wakeup);
        }
    }
    // An exit or network error can arrive before the synchronized frame ends.
    // Publish bytes already received before the pane reports its final state.
    if stream.next_sync_timeout().is_some() {
        stream.stop_sync(&mut *terminal.lock());
        event_proxy.send_event(TerminalEvent::Wakeup);
    }
    result
}

async fn pump_stream<H: SshEventHost>(
    channel: &mut ShellChannel,
    stream: &mut StreamProcessor,
    terminal: &FairMutex<Term<H>>,
    event_proxy: &H,
    input: &mut mpsc::UnboundedReceiver<Msg>,
) -> Result<(), SessionError> {
    while matches!(
        channel.pending.front(),
        Some(ChannelMsg::Data { .. } | ChannelMsg::ExtendedData { .. })
    ) {
        let message = channel.pending.pop_front().unwrap();
        feed_ready_output(channel, message, stream, terminal, event_proxy);
    }
    let mut eof_deadline = None;
    // A blocked SSH send must not stop draining output. The peer may need that
    // receive space before it can consume input and return window credit.
    let writer = channel.channel.as_ref().expect("open shell writer").clone();
    let mut writing: Option<Pin<Box<dyn Future<Output = Result<(), SessionError>> + Send>>> = None;
    loop {
        let sync_deadline = stream.next_sync_timeout();
        tokio::select! {
            result = async { writing.as_mut().expect("pending write").await }, if writing.is_some() => {
                result?;
                writing = None;
            },
            message = input.recv(), if writing.is_none() => match message {
                Some(Msg::Input(bytes)) => {
                    let writer = writer.clone();
                    writing = Some(Box::pin(async move {
                        network("channel write", writer.data_bytes(bytes.into_owned())).await
                    }));
                },
                Some(Msg::Resize(size)) => {
                    // SSH has no local EventLoop to apply the grid half of a
                    // resize. Keep the terminal model in lockstep with the
                    // stream and the remote PTY, just as the local event loop
                    // does before calling ResizePseudoConsole.
                    terminal.lock().resize(size);
                    stream.resize(size);
                    event_proxy.send_event(TerminalEvent::Wakeup);
                    let writer = writer.clone();
                    writing = Some(Box::pin(async move {
                        network("channel resize", writer.window_change(
                            u32::from(size.num_cols), u32::from(size.num_lines),
                            u32::from(size.cell_width) * u32::from(size.num_cols),
                            u32::from(size.cell_height) * u32::from(size.num_lines),
                        )).await
                    }));
                },
                Some(Msg::ResizeGrid(size)) => {
                    terminal.lock().resize(size);
                    stream.resize(size);
                    event_proxy.send_event(TerminalEvent::Wakeup);
                },
                Some(Msg::Shutdown) | None => return Ok(()),
            },
            message = channel.next_message() => match message {
                Some(message @ (ChannelMsg::Data { .. } | ChannelMsg::ExtendedData { .. })) => {
                    feed_ready_output(channel, message, stream, terminal, event_proxy);
                },
                Some(ChannelMsg::ExitStatus { .. }) => return Ok(()),
                Some(ChannelMsg::Eof) => eof_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(1)),
                Some(ChannelMsg::Close | ChannelMsg::ExitSignal { .. }) | None => {
                    return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "SSH connection ended without an exit status").into());
                },
                _ => {},
            },
            _ = wait_for_sync(sync_deadline), if sync_deadline.is_some() => {
                stream.stop_sync(&mut *terminal.lock());
                event_proxy.send_event(TerminalEvent::Wakeup);
            },
            _ = async { tokio::time::sleep_until(eof_deadline.expect("EOF deadline exists")).await }, if eof_deadline.is_some() => {
                return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "SSH connection ended without an exit status").into());
            },
        }
    }
}

/// Match the local PTY's bounded reads: consume queued output under one grid
/// lock and publish once. A packet boundary is not a terminal frame boundary.
fn feed_ready_output<H: SshEventHost>(
    channel: &mut ShellChannel,
    mut message: ChannelMsg,
    stream: &mut StreamProcessor,
    terminal: &FairMutex<Term<H>>,
    event_proxy: &H,
) {
    const MAX_BYTES: usize = 64 * 1024;
    const MAX_MESSAGES: usize = 128;
    let mut bytes = 0;
    let mut messages = 0;
    {
        let mut terminal = terminal.lock();
        loop {
            match message {
                ChannelMsg::Data { data } => {
                    bytes += data.len();
                    messages += 1;
                    let filtered = match &mut channel.identity_input {
                        Some(filter) => filter.feed(data.as_ref()),
                        None => std::borrow::Cow::Borrowed(data.as_ref()),
                    };
                    stream.feed(&mut terminal, event_proxy, &filtered);
                },
                ChannelMsg::ExtendedData { data, .. } => {
                    bytes += data.len();
                    messages += 1;
                    stream.feed(&mut terminal, event_proxy, data.as_ref());
                },
                // Preserve EOF, exit status and other control messages in order.
                control => {
                    channel.pending.push_front(control);
                    break;
                },
            }
            if bytes >= MAX_BYTES || messages >= MAX_MESSAGES {
                break;
            }
            let Some(next) = channel.ready_message() else { break };
            message = next;
        }
    }
    event_proxy.send_event(TerminalEvent::Wakeup);
}

async fn wait_for_sync(deadline: Option<std::time::Instant>) {
    if let Some(deadline) = deadline {
        tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;
    }
}

#[cfg(test)]
mod tests;
