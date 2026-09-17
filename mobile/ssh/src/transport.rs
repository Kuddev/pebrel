//! russh protocol ownership, explicit request acknowledgements and byte streams.
use std::sync::Arc;
use std::time::Duration;

use russh::{Channel, ChannelMsg, client};
use tokio::net::{TcpStream, lookup_host};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;
use zeroize::{Zeroize, Zeroizing};

use crate::session::{Command, Failure, Open, Options, Result, Session, Worker};

struct HostVerifier {
    owner: Arc<Session>,
    known: String,
    events: mpsc::Sender<String>,
}

impl client::Handler for HostVerifier {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &russh::keys::ssh_key::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        let fingerprint = key.fingerprint(russh::keys::ssh_key::HashAlg::Sha256).to_string();
        if !self.known.is_empty() {
            if self.known == fingerprint {
                return Ok(true);
            }
            *self.owner.identity_failure.lock().unwrap() = Some(Failure("HOST_KEY_CHANGED"));
            return Ok(false);
        }
        let (reply, wait) = oneshot::channel();
        *self.owner.trust.lock().unwrap() = Some(reply);
        let answer = tokio::select! {
            biased;
            _ = self.owner.cancel.cancelled() => Err(Failure("CLOSED")),
            result = timeout(Duration::from_secs(60), async {
                self.events.send(format!("verify:{fingerprint}")).await.map_err(|_| Failure("CLOSED"))?;
                wait.await.map_err(|_| Failure("CLOSED"))
            }) => result.map_err(|_| Failure("TIMEOUT")).and_then(|r| r),
        };
        let failure = match answer {
            Ok(true) => return Ok(true),
            Ok(false) => Failure("TRUST_REJECTED"),
            Err(error) => error,
        };
        *self.owner.identity_failure.lock().unwrap() = Some(failure);
        Ok(false)
    }
}

pub(crate) async fn run(owner: Arc<Session>, mut worker: Worker, options: Options) {
    let result = tokio::select! {
        biased;
        _ = owner.cancel.cancelled() => Err(Failure("CLOSED")),
        result = operate(&owner, &mut worker, options) => result,
    };
    // russh spawns its protocol reader during KEX. Dropping its Handle alone does
    // not stop it; shutdown also covers cancellation before connect_stream returns.
    owner.shutdown_socket();
    worker.outcome.send_replace(Some(result));
    if let Err(error) = result {
        let _ = worker.events.try_send(format!("error:{}", error.0));
    }
    // Dropping Worker closes output queues; readers can drain already received data.
}

async fn operate(owner: &Arc<Session>, worker: &mut Worker, mut options: Options) -> Result<i32> {
    worker.events.send("stage:NETWORK".into()).await.map_err(|_| Failure("CLOSED"))?;
    let mut client = connect(owner, &worker.events, &options).await?;
    worker.events.send("stage:AUTHENTICATING".into()).await.map_err(|_| Failure("CLOSED"))?;
    let password =
        Zeroizing::new(String::from_utf8(options.password.to_vec()).map_err(|_| Failure("AUTH"))?);
    options.password.zeroize();
    let auth = timeout(Duration::from_secs(15), async {
        if client.authenticate_none(&options.user).await?.success() {
            return Ok(());
        }
        if client.authenticate_password(&options.user, password.to_string()).await?.success() {
            Ok(())
        } else {
            Err(Failure("AUTH"))
        }
    })
    .await
    .map_err(|_| Failure("TIMEOUT"))?;
    drop(password);
    auth?;
    worker.events.send("connected".into()).await.map_err(|_| Failure("CLOSED"))?;
    let (mode, reply) = loop {
        match worker.commands.recv().await.ok_or(Failure("CLOSED"))? {
            Command::Open(mode, reply) => break (mode, reply),
            Command::Resize(_) => {},
        }
    };
    let shell = matches!(mode, Open::Shell(_));
    let opened = timeout(Duration::from_secs(15), open_channel(&client, mode))
        .await
        .map_err(|_| Failure("TIMEOUT"))
        .and_then(|r| r);
    let (channel, early) = match opened {
        Ok(opened) => {
            let _ = reply.send(Ok(()));
            opened
        },
        Err(error) => {
            let _ = reply.send(Err(error));
            return Err(error);
        },
    };
    let (mut reader, writer) = channel.split();
    let Worker { stdout, stderr, commands, input, .. } = worker;
    let receive = async {
        let mut status = -1;
        for message in early {
            if forward(message, shell, stdout, stderr, &mut status).await? {
                return Ok(status);
            }
        }
        loop {
            let message = reader.wait().await.ok_or(Failure("NETWORK"))?;
            if forward(message, shell, stdout, stderr, &mut status).await? {
                return Ok(status);
            }
        }
    };
    let transmit = async {
        loop {
            tokio::select! {
                bytes = input.recv() => writer.data_bytes(bytes.ok_or(Failure("CLOSED"))?).await?,
                command = commands.recv() => match command.ok_or(Failure("CLOSED"))? {
                    Command::Resize(size) if shell => {
                        writer.window_change(size.columns, size.rows, size.width, size.height).await?;
                    }
                    Command::Resize(_) => {}
                    Command::Open(_, reply) => { let _ = reply.send(Err(Failure("CHANNEL"))); }
                }
            }
        }
        #[allow(unreachable_code)]
        Ok::<i32, Failure>(-1)
    };
    let result = tokio::select! { result = receive => result, result = transmit => result };
    let _ = timeout(
        Duration::from_secs(1),
        client.disconnect(russh::Disconnect::ByApplication, "", ""),
    )
    .await;
    result
}

async fn connect(
    owner: &Arc<Session>,
    events: &mpsc::Sender<String>,
    options: &Options,
) -> Result<client::Handle<HostVerifier>> {
    let addresses =
        timeout(Duration::from_secs(15), lookup_host((options.host.as_str(), options.port)))
            .await
            .map_err(|_| Failure("TIMEOUT"))?
            .map_err(|_| Failure("UNKNOWN_HOST"))?
            .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err(Failure("UNKNOWN_HOST"));
    }
    let socket = timeout(Duration::from_secs(15), TcpStream::connect(addresses.as_slice()))
        .await
        .map_err(|_| Failure("TIMEOUT"))?
        .map_err(|e| {
            Failure(if e.kind() == std::io::ErrorKind::ConnectionRefused {
                "REFUSED"
            } else {
                "NETWORK"
            })
        })?;
    socket.set_nodelay(true).map_err(|_| Failure("NETWORK"))?;
    let socket = socket.into_std().map_err(|_| Failure("NETWORK"))?;
    *owner.socket.lock().unwrap() = Some(socket.try_clone().map_err(|_| Failure("NETWORK"))?);
    if owner.cancel.is_cancelled() {
        owner.shutdown_socket();
        return Err(Failure("CLOSED"));
    }
    let socket = TcpStream::from_std(socket).map_err(|_| Failure("NETWORK"))?;
    let config = Arc::new(client::Config {
        window_size: 256 * 1024,
        maximum_packet_size: 32 * 1024,
        channel_buffer_size: 16,
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        nodelay: true,
        ..Default::default()
    });
    let handler = HostVerifier {
        owner: owner.clone(),
        known: options.fingerprint.clone(),
        events: events.clone(),
    };
    timeout(Duration::from_secs(90), client::connect_stream(config, socket, handler))
        .await
        .map_err(|_| Failure("TIMEOUT"))?
        .map_err(|error| owner.identity_failure.lock().unwrap().unwrap_or_else(|| error.into()))
}

async fn open_channel(
    client: &client::Handle<HostVerifier>,
    mode: Open,
) -> Result<(Channel<client::Msg>, Vec<ChannelMsg>)> {
    let mut channel = client.channel_open_session().await?;
    let mut early = Vec::new();
    match mode {
        Open::Shell(size) => {
            channel
                .request_pty(
                    true,
                    "xterm-256color",
                    size.columns,
                    size.rows,
                    size.width,
                    size.height,
                    &[],
                )
                .await?;
            acknowledged(&mut channel, &mut early).await?;
            channel.request_shell(true).await?;
        },
        Open::Exec(command) => channel.exec(true, command.into_bytes()).await?,
    }
    acknowledged(&mut channel, &mut early).await?;
    Ok((channel, early))
}

async fn acknowledged(
    channel: &mut Channel<client::Msg>,
    early: &mut Vec<ChannelMsg>,
) -> Result<()> {
    let mut bytes = early.iter().map(message_size).sum::<usize>();
    loop {
        match channel.wait().await {
            Some(ChannelMsg::Success) => return Ok(()),
            Some(ChannelMsg::Failure | ChannelMsg::Close) | None => return Err(Failure("CHANNEL")),
            Some(
                message @ (ChannelMsg::Data { .. }
                | ChannelMsg::ExtendedData { .. }
                | ChannelMsg::ExitStatus { .. }
                | ChannelMsg::Eof),
            ) => {
                bytes += message_size(&message);
                if bytes > 128 * 1024 || early.len() >= 64 {
                    return Err(Failure("CHANNEL"));
                }
                early.push(message);
            },
            _ => {},
        }
    }
}

fn message_size(message: &ChannelMsg) -> usize {
    match message {
        ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => data.len(),
        _ => 0,
    }
}

async fn forward(
    message: ChannelMsg,
    shell: bool,
    stdout: &mpsc::Sender<Vec<u8>>,
    stderr: &mpsc::Sender<Vec<u8>>,
    status: &mut i32,
) -> Result<bool> {
    let (bytes, destination) = match message {
        ChannelMsg::Data { data } => (data, stdout),
        ChannelMsg::ExtendedData { data, .. } => (data, if shell { stdout } else { stderr }),
        ChannelMsg::ExitStatus { exit_status } => {
            *status = exit_status as i32;
            return Ok(false);
        },
        ChannelMsg::Close => return Ok(true),
        _ => return Ok(false),
    };
    for chunk in bytes.chunks(16 * 1024) {
        destination.send(chunk.to_vec()).await.map_err(|_| Failure("CLOSED"))?;
    }
    Ok(false)
}
