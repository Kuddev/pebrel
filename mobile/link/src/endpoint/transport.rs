use super::{
    HostState, PersistHost, RelayAccess, authentication,
    host::Hello,
    invalid, now,
    runtime::{AuthorizedFactory, MAX_REQUEST, Status, bridge},
};
use crate::crypto::{MAX_PACKET, SecureChannel};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    io,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async_tls_with_config,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};
use tokio_util::sync::CancellationToken;

pub struct Handle {
    pub status: Arc<Mutex<Status>>,
    pub shutdown: CancellationToken,
    invitation: Arc<Mutex<Option<String>>>,
    expires_at: u64,
}
impl Handle {
    pub fn invitation(&self) -> Option<String> {
        if now().ok()? >= self.expires_at {
            return None;
        }
        self.invitation.lock().ok()?.clone()
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
fn status(state: &Mutex<Status>, value: Status) {
    if let Ok(mut state) = state.lock() {
        *state = value;
    }
}

pub async fn start_relay(
    access: RelayAccess,
    host: Arc<Mutex<HostState>>,
    persist: PersistHost,
    name: &str,
    allow_input: bool,
    factory: AuthorizedFactory,
) -> io::Result<Handle> {
    access.validate()?;
    let mut socket = connect(&access, "desktop").await?;
    let first = time::timeout(Duration::from_secs(12), notice(&mut socket, false))
        .await
        .map_err(|_| invalid())??;
    let invitation =
        host.lock().map_err(|_| invalid())?.issue(&access, name, allow_input, now()?)?;
    let expires_at =
        serde_json::from_str::<Value>(&invitation).map_err(|_| invalid())?["secure"]["expiresAt"]
            .as_u64()
            .ok_or_else(invalid)?;
    let invitation = Arc::new(Mutex::new(Some(invitation)));
    let state = Arc::new(Mutex::new(Status::Waiting));
    let shutdown = CancellationToken::new();
    let handle = Handle {
        status: state.clone(),
        shutdown: shutdown.clone(),
        invitation: invitation.clone(),
        expires_at,
    };
    tokio::spawn(async move {
        let mut first = first;
        let mut failures = 0_u32;
        loop {
            let paired = if let Some(epoch) = first.take() {
                Ok(Some(epoch))
            } else {
                tokio::select! { _=shutdown.cancelled()=>break, result=notice(&mut socket,true)=>result }
            };
            if let Ok(Some(epoch)) = paired {
                // Never open the Runtime API before authenticated enrollment
                // and its encrypted acknowledgement have both completed.
                let result = tokio::select! {
                    _=shutdown.cancelled()=>break,
                    result=time::timeout(Duration::from_secs(15),authenticate(&mut socket,&epoch,host.clone(),persist.clone(),invitation.clone()))=>result,
                };
                if let Ok(Ok((channel, grant_input))) = result {
                    failures = 0;
                    status(&state, Status::Connected);
                    let _ = exchange(
                        &mut socket,
                        channel,
                        factory(allow_input && grant_input),
                        shutdown.clone(),
                    )
                    .await;
                }
            }
            if shutdown.is_cancelled() {
                break;
            }
            let _ = time::timeout(Duration::from_secs(1), socket.close(None)).await;
            status(&state, Status::Reconnecting);
            loop {
                failures = failures.saturating_add(1);
                // Jitter with a bounded ceiling; no queued application requests
                // survive a disconnect or a change of relay epoch.
                let jitter = crate::identity::Secret::generate()
                    .map(|s| s.hash().bytes().next().unwrap_or(0) as u64)
                    .unwrap_or(0);
                let delay = Duration::from_millis(
                    ((1_u64 << failures.min(5)) * 500).min(15_000) + jitter * 4,
                );
                tokio::select! { _=shutdown.cancelled()=>{status(&state,Status::Stopped);return;},_=time::sleep(delay)=>{} }
                let result = tokio::select! { _=shutdown.cancelled()=>{status(&state,Status::Stopped);return;},result=connect(&access,"desktop")=>result };
                match result {
                    Ok(next) => {
                        socket = next;
                        status(&state, Status::Waiting);
                        break;
                    },
                    Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                        status(&state, Status::Failed);
                        return;
                    },
                    Err(_) => {},
                }
            }
        }
        status(&state, Status::Stopped);
    });
    Ok(handle)
}

pub(super) async fn connect(access: &RelayAccess, role: &str) -> io::Result<Socket> {
    let url =
        format!("{}/v2/link?device={}&role={role}", access.url.trim_end_matches('/'), access.room);
    let mut request = url.into_client_request().map_err(|_| invalid())?;
    let token = if role == "desktop" { &access.desktop_token } else { &access.mobile_token };
    request
        .headers_mut()
        .insert("Authorization", format!("Bearer {token}").parse().map_err(|_| invalid())?);
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_PACKET))
        .max_frame_size(Some(MAX_PACKET))
        .write_buffer_size(0)
        .max_write_buffer_size(MAX_PACKET * 2);
    let result = time::timeout(
        Duration::from_secs(12),
        connect_async_tls_with_config(
            request,
            Some(config),
            true,
            Some(super::tls::connector(access)?),
        ),
    )
    .await;
    match result {
        Ok(Ok((socket, _))) => Ok(socket),
        Ok(Err(tokio_tungstenite::tungstenite::Error::Http(response)))
            if matches!(response.status().as_u16(), 401 | 403) =>
        {
            Err(authentication())
        },
        Ok(Err(tokio_tungstenite::tungstenite::Error::Tls(_))) => Err(authentication()),
        _ => Err(io::Error::other("relay_connection_failed")),
    }
}

pub(super) async fn notice(socket: &mut Socket, wait_paired: bool) -> io::Result<Option<String>> {
    loop {
        match next(socket).await? {
            Message::Text(text) if text.len() <= 1024 => {
                let frame: Value = serde_json::from_str(&text).map_err(|_| invalid())?;
                if frame["version"] != 2 {
                    return Err(invalid());
                }
                match frame["type"].as_str() {
                    Some("relay.waiting") => {
                        if !wait_paired {
                            return Ok(None);
                        }
                    },
                    Some("relay.paired") => {
                        return frame["link"]
                            .as_str()
                            .filter(|v| crate::identity::valid_id(v))
                            .map(|v| Some(v.to_owned()))
                            .ok_or_else(invalid);
                    },
                    _ => return Err(invalid()),
                }
            },
            _ => return Err(invalid()),
        }
    }
}

async fn next(socket: &mut Socket) -> io::Result<Message> {
    loop {
        let frame = time::timeout(Duration::from_secs(65), socket.next())
            .await
            .map_err(|_| invalid())?
            .ok_or_else(invalid)?
            .map_err(|_| invalid())?;
        match frame {
            Message::Ping(_) | Message::Pong(_) => {
                // tungstenite queues Pong on read; flush even when application
                // traffic is idle so the server heartbeat can observe it.
                time::timeout(Duration::from_secs(8), socket.flush())
                    .await
                    .map_err(|_| invalid())?
                    .map_err(|_| invalid())?;
            },
            Message::Close(_) => return Err(io::Error::other("relay_disconnected")),
            value => return Ok(value),
        }
    }
}

pub(super) async fn send(socket: &mut Socket, bytes: Vec<u8>) -> io::Result<()> {
    if bytes.is_empty() || bytes.len() > MAX_PACKET {
        return Err(invalid());
    }
    time::timeout(Duration::from_secs(8), socket.send(Message::Binary(bytes.into())))
        .await
        .map_err(|_| invalid())?
        .map_err(|_| invalid())
}
pub(super) async fn binary(socket: &mut Socket) -> io::Result<Vec<u8>> {
    match next(socket).await? {
        Message::Binary(bytes) if !bytes.is_empty() => Ok(bytes.to_vec()),
        _ => Err(invalid()),
    }
}
pub(super) async fn encrypted(
    socket: &mut Socket,
    channel: &mut SecureChannel,
    bytes: &[u8],
) -> io::Result<()> {
    for packet in channel.seal(bytes).map_err(|_| authentication())? {
        send(socket, packet).await?;
    }
    Ok(())
}
pub(super) async fn plaintext(
    socket: &mut Socket,
    channel: &mut SecureChannel,
) -> io::Result<zeroize::Zeroizing<Vec<u8>>> {
    loop {
        if let Some(value) = channel.open(&binary(socket).await?).map_err(|_| authentication())? {
            return Ok(value);
        }
    }
}

async fn authenticate(
    socket: &mut Socket,
    epoch: &str,
    host: Arc<Mutex<HostState>>,
    persist: PersistHost,
    invitation: Arc<Mutex<Option<String>>>,
) -> io::Result<(SecureChannel, bool)> {
    let first = binary(socket).await?;
    if first.len() > 512 || first[0] != 1 {
        return Err(invalid());
    }
    let hello: Hello = serde_json::from_slice(&first[1..]).map_err(|_| invalid())?;
    let mut channel = host.lock().map_err(|_| invalid())?.handshake(&hello, epoch, now()?)?;
    channel.read_handshake(&binary(socket).await?).map_err(|_| authentication())?;
    send(socket, channel.write_handshake().map_err(|_| authentication())?).await?;
    let request: Value =
        serde_json::from_slice(&plaintext(socket, &mut channel).await?).map_err(|_| invalid())?;
    if request["type"] != "secure.connect" {
        return Err(invalid());
    }
    let name = request["name"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 160 && !v.chars().any(char::is_control))
        .ok_or_else(invalid)?
        .to_owned();
    let was_invite = hello.invitation;
    let (response, allow_input) = tokio::task::spawn_blocking(move || {
        host.lock().map_err(|_| invalid())?.enroll(&hello, &name, now()?, &persist)
    })
    .await
    .map_err(|_| invalid())??;
    if was_invite {
        *invitation.lock().map_err(|_| invalid())? = None;
    }
    encrypted(socket, &mut channel, response.as_bytes()).await?;
    let ack: Value =
        serde_json::from_slice(&plaintext(socket, &mut channel).await?).map_err(|_| invalid())?;
    let expected: Value = serde_json::from_str(&response).map_err(|_| invalid())?;
    if ack != json!({"type":"secure.ack","grant":expected["grant"]}) {
        return Err(invalid());
    }
    Ok((channel, allow_input))
}

async fn exchange(
    socket: &mut Socket,
    mut channel: SecureChannel,
    factory: super::runtime::RuntimeFactory,
    shutdown: CancellationToken,
) -> io::Result<()> {
    let (input, mut output) = bridge(factory);
    loop {
        tokio::select! {
            _=shutdown.cancelled()=>return Ok(()),
            frame=async {
                let permit = input.reserve().await.map_err(|_|invalid())?;
                let frame = next(socket).await?;
                Ok::<_,io::Error>((permit,frame))
            }=>{
                let (permit,frame) = frame?;
                match frame {
                Message::Binary(bytes)=>if let Some(plain)=channel.open(&bytes).map_err(|_|authentication())? {
                    if plain.len()>MAX_REQUEST {return Err(invalid());}
                    permit.send(plain.to_vec());
                },
                _=>return Err(invalid()),
                }
            },
            response=output.recv()=>{
                let bytes=response.ok_or_else(invalid)?;
                encrypted(socket,&mut channel,&bytes).await?;
            },
        }
    }
}
