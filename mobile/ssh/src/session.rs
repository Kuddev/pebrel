//! Owned connection handles and blocking boundaries for the Android IO workers.
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tokio::runtime::{Builder, Runtime};
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

pub(crate) type Result<T> = std::result::Result<T, Failure>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Failure(pub &'static str);

impl From<russh::Error> for Failure {
    fn from(error: russh::Error) -> Self {
        use russh::Error::*;
        Self(match error {
            ConnectionTimeout | KeepaliveTimeout | InactivityTimeout | Elapsed(_) => "TIMEOUT",
            ChannelOpenFailure(_) | RequestDenied | WrongChannel => "CHANNEL",
            IO(ref io) if io.kind() == std::io::ErrorKind::ConnectionRefused => "REFUSED",
            IO(_) | Disconnect | HUP | SendError | RecvError => "NETWORK",
            _ => "NEGOTIATION",
        })
    }
}

pub(crate) fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("pebrel-ssh")
            .enable_all()
            .build()
            .expect("SSH runtime")
    })
}

pub(crate) struct Options {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Zeroizing<Vec<u8>>,
    pub fingerprint: String,
}

#[derive(Clone, Copy)]
pub(crate) struct Geometry {
    pub columns: u32,
    pub rows: u32,
    pub width: u32,
    pub height: u32,
}

pub(crate) enum Open {
    Shell(Geometry),
    Exec(String),
}

pub(crate) enum Command {
    Open(Open, oneshot::Sender<Result<()>>),
    Resize(Geometry),
}

pub(crate) struct Reader {
    rx: mpsc::Receiver<Vec<u8>>,
    pending: Vec<u8>,
    offset: usize,
}

impl Reader {
    fn new(rx: mpsc::Receiver<Vec<u8>>) -> Self {
        Self { rx, pending: Vec::new(), offset: 0 }
    }

    async fn read(&mut self, limit: usize, owner: &Session) -> Result<Vec<u8>> {
        if self.offset == self.pending.len() {
            let next = tokio::select! {
                biased;
                _ = owner.cancel.cancelled() => return Err(Failure("CLOSED")),
                next = self.rx.recv() => next,
            };
            match next {
                Some(bytes) => {
                    self.pending = bytes;
                    self.offset = 0;
                },
                None => return owner.outcome.borrow().unwrap_or(Ok(-1)).map(|_| Vec::new()),
            }
        }
        let end = (self.offset + limit).min(self.pending.len());
        let bytes = self.pending[self.offset..end].to_vec();
        self.offset = end;
        Ok(bytes)
    }
}

pub(crate) struct Session {
    pub cancel: CancellationToken,
    pub commands: mpsc::Sender<Command>,
    pub input: mpsc::Sender<Vec<u8>>,
    pub trust: Mutex<Option<oneshot::Sender<bool>>>,
    pub identity_failure: Mutex<Option<Failure>>,
    pub socket: Mutex<Option<std::net::TcpStream>>,
    events: AsyncMutex<mpsc::Receiver<String>>,
    stdout: AsyncMutex<Reader>,
    stderr: AsyncMutex<Reader>,
    outcome: watch::Receiver<Option<Result<i32>>>,
}

pub(crate) struct Worker {
    pub events: mpsc::Sender<String>,
    pub commands: mpsc::Receiver<Command>,
    pub input: mpsc::Receiver<Vec<u8>>,
    pub stdout: mpsc::Sender<Vec<u8>>,
    pub stderr: mpsc::Sender<Vec<u8>>,
    pub outcome: watch::Sender<Option<Result<i32>>>,
}

impl Session {
    pub fn new() -> (Arc<Self>, Worker) {
        let (events, event_rx) = mpsc::channel(16);
        let (commands, command_rx) = mpsc::channel(16);
        let (input, input_rx) = mpsc::channel(16);
        let (stdout, stdout_rx) = mpsc::channel(8);
        let (stderr, stderr_rx) = mpsc::channel(8);
        let (outcome, outcome_rx) = watch::channel(None);
        (
            Arc::new(Self {
                cancel: CancellationToken::new(),
                commands,
                input,
                trust: Mutex::new(None),
                identity_failure: Mutex::new(None),
                socket: Mutex::new(None),
                events: AsyncMutex::new(event_rx),
                stdout: AsyncMutex::new(Reader::new(stdout_rx)),
                stderr: AsyncMutex::new(Reader::new(stderr_rx)),
                outcome: outcome_rx,
            }),
            Worker { events, commands: command_rx, input: input_rx, stdout, stderr, outcome },
        )
    }

    pub fn shutdown_socket(&self) {
        if let Some(socket) = self.socket.lock().unwrap().take() {
            let _ = socket.shutdown(std::net::Shutdown::Both);
        }
    }

    pub fn close(&self) {
        self.cancel.cancel();
        self.trust.lock().unwrap().take();
        self.shutdown_socket();
    }

    pub async fn event(&self) -> Result<String> {
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(Failure("CLOSED")),
            event = async { self.events.lock().await.recv().await } => event.ok_or_else(|| self.failure()),
        }
    }

    pub fn answer(&self, accepted: bool) -> Result<()> {
        self.trust
            .lock()
            .unwrap()
            .take()
            .ok_or(Failure("CLOSED"))?
            .send(accepted)
            .map_err(|_| Failure("CLOSED"))
    }

    pub async fn open(&self, mode: Open) -> Result<()> {
        let (reply, wait) = oneshot::channel();
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(Failure("CLOSED")),
            result = async {
                self.commands.send(Command::Open(mode, reply)).await.map_err(|_| self.failure())?;
                wait.await.map_err(|_| self.failure())?
            } => result,
        }
    }

    pub async fn read(&self, stderr: bool, limit: usize) -> Result<Vec<u8>> {
        let stream = if stderr { &self.stderr } else { &self.stdout };
        stream.lock().await.read(limit, self).await
    }

    pub async fn write(&self, bytes: Vec<u8>) -> Result<()> {
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(Failure("CLOSED")),
            result = self.input.send(bytes) => result.map_err(|_| self.failure()),
        }
    }

    pub async fn resize(&self, size: Geometry) -> Result<()> {
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(Failure("CLOSED")),
            result = self.commands.send(Command::Resize(size)) => result.map_err(|_| self.failure()),
        }
    }

    pub async fn exit(&self) -> Result<i32> {
        let mut outcome = self.outcome.clone();
        loop {
            if let Some(result) = *outcome.borrow_and_update() {
                return result;
            }
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => return Err(Failure("CLOSED")),
                changed = outcome.changed() => changed.map_err(|_| Failure("NETWORK"))?,
            }
        }
    }

    fn failure(&self) -> Failure {
        self.outcome.borrow().and_then(|r| r.err()).unwrap_or(Failure("CLOSED"))
    }
}

fn registry() -> &'static Mutex<HashMap<i64, Arc<Session>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<i64, Arc<Session>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn start(options: Options) -> Result<i64> {
    static NEXT: AtomicI64 = AtomicI64::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    if id <= 0 {
        return Err(Failure("INTERNAL"));
    }
    let (session, worker) = Session::new();
    registry().lock().unwrap().insert(id, session.clone());
    runtime().spawn(crate::transport::run(session, worker, options));
    Ok(id)
}

pub(crate) fn get(id: i64) -> Result<Arc<Session>> {
    registry().lock().unwrap().get(&id).cloned().ok_or(Failure("CLOSED"))
}

pub(crate) fn close(id: i64) {
    if let Some(session) = registry().lock().unwrap().remove(&id) {
        session.close();
    }
}
