use super::{
    HostState, PersistHost, RelayAccess, Route, authentication,
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
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async_tls_with_config,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};
use tokio_util::sync::CancellationToken;

pub struct Handle {
    pub status: Arc<Mutex<Status>>,
    pub shutdown: CancellationToken,
    pub(super) invitation: Arc<Mutex<Option<String>>>,
    pub(super) pairing_code: Arc<Mutex<Option<super::pairing_code::PairingCode>>>,
    #[cfg(feature = "preview")]
    pub(super) discovery: Option<super::discovery::Advertisement>,
    host: Arc<Mutex<HostState>>,
    access: RelayAccess,
    name: String,
    route: Route,
}
impl Handle {
    pub fn invitation(&self) -> Option<String> {
        let invitation = self.invitation.lock().ok()?.clone()?;
        let value: Value = serde_json::from_str(&invitation).ok()?;
        let grant = value["secure"]["grant"].as_str()?;
        self.host.lock().ok()?.invitation_valid(grant, now().ok()?).then_some(invitation)
    }

    pub fn refresh_invitation(&self, allow_input: bool) -> io::Result<()> {
        let mut host = self.host.lock().map_err(|_| invalid())?;
        let mut slot = self.invitation.lock().map_err(|_| invalid())?;
        if let Some(previous) = slot.take() {
            if let Ok(value) = serde_json::from_str::<Value>(&previous) {
                if let Some(grant) = value["secure"]["grant"].as_str() {
                    host.cancel_invitation(grant);
                }
            }
        }
        let mut value: Value =
            serde_json::from_str(&host.issue(&self.access, &self.name, allow_input, now()?)?)
                .map_err(|_| invalid())?;
        value["mode"] = json!(match self.route {
            Route::Lan => "lan",
            Route::Relay => "relay",
        });
        let invitation = value.to_string();
        *self.pairing_code.lock().map_err(|_| invalid())? = if self.route == Route::Lan {
            Some(super::pairing_code::PairingCode::new(
                &invitation,
                value["secure"]["grant"].as_str().ok_or_else(invalid)?,
            )?)
        } else {
            None
        };
        *slot = Some(invitation);
        Ok(())
    }

    pub fn address(&self) -> &str {
        &self.access.url
    }

    pub fn pairing_code(&self) -> Option<String> {
        let host = self.host.lock().ok()?;
        let pairing_code = self.pairing_code.lock().ok()?;
        pairing_code.as_ref()?.code(&host, now().ok()?).map(str::to_owned)
    }

    #[cfg(feature = "preview")]
    pub fn discoverable(&self) -> bool {
        self.discovery.is_some()
    }

    pub(super) fn new(
        access: RelayAccess,
        host: Arc<Mutex<HostState>>,
        name: &str,
        route: Route,
        allow_input: bool,
    ) -> io::Result<Self> {
        let handle = Self {
            status: Arc::new(Mutex::new(Status::Waiting)),
            shutdown: CancellationToken::new(),
            invitation: Arc::new(Mutex::new(None)),
            pairing_code: Arc::new(Mutex::new(None)),
            #[cfg(feature = "preview")]
            discovery: None,
            host,
            access,
            name: name.to_owned(),
            route,
        };
        handle.refresh_invitation(allow_input)?;
        Ok(handle)
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.shutdown.cancel();
        let invitation = self.invitation.lock().ok().and_then(|value| value.clone());
        if let Some(grant) = invitation
            .and_then(|value| serde_json::from_str::<Value>(&value).ok())
            .and_then(|value| value["secure"]["grant"].as_str().map(str::to_owned))
        {
            if let Ok(mut host) = self.host.lock() {
                host.cancel_invitation(&grant);
            }
        }
    }
}

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
pub(super) fn status(state: &Mutex<Status>, value: Status) {
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
    let handle = Handle::new(
        RelayAccess::parse(&serde_json::to_vec(&access)?)?,
        host.clone(),
        name,
        Route::Relay,
        allow_input,
    )?;
    let invitation = handle.invitation.clone();
    let state = handle.status.clone();
    let shutdown = handle.shutdown.clone();
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
                    result=time::timeout(Duration::from_secs(150),authenticate(&mut socket,&epoch,host.clone(),persist.clone(),invitation.clone(),Route::Relay,None))=>result,
                };
                if let Ok(Ok((channel, _, session))) = result {
                    failures = 0;
                    session.mark_connected();
                    status(&state, Status::Connected);
                    tokio::select! {
                    biased;
                    _ = session.cancelled() => {},
                    _ = exchange(
                        &mut socket,
                        channel,
                        factory(session.authorization.clone()),
                        shutdown.clone(),
                        session.authorization.clone(),
                    ) => {},
                    }
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

pub(super) async fn notice(
    socket: &mut WebSocketStream<impl AsyncRead + AsyncWrite + Unpin + Send>,
    wait_paired: bool,
) -> io::Result<Option<String>> {
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

async fn next(
    socket: &mut WebSocketStream<impl AsyncRead + AsyncWrite + Unpin + Send>,
) -> io::Result<Message> {
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

pub(super) async fn send(
    socket: &mut WebSocketStream<impl AsyncRead + AsyncWrite + Unpin + Send>,
    bytes: Vec<u8>,
) -> io::Result<()> {
    if bytes.is_empty() || bytes.len() > MAX_PACKET {
        return Err(invalid());
    }
    time::timeout(Duration::from_secs(8), socket.send(Message::Binary(bytes.into())))
        .await
        .map_err(|_| invalid())?
        .map_err(|_| invalid())
}
pub(super) async fn binary(
    socket: &mut WebSocketStream<impl AsyncRead + AsyncWrite + Unpin + Send>,
) -> io::Result<Vec<u8>> {
    match next(socket).await? {
        Message::Binary(bytes) if !bytes.is_empty() => Ok(bytes.to_vec()),
        _ => Err(invalid()),
    }
}
pub(super) async fn encrypted(
    socket: &mut WebSocketStream<impl AsyncRead + AsyncWrite + Unpin + Send>,
    channel: &mut SecureChannel,
    bytes: &[u8],
) -> io::Result<()> {
    for packet in channel.seal(bytes).map_err(|_| authentication())? {
        send(socket, packet).await?;
    }
    Ok(())
}
pub(super) async fn plaintext(
    socket: &mut WebSocketStream<impl AsyncRead + AsyncWrite + Unpin + Send>,
    channel: &mut SecureChannel,
) -> io::Result<zeroize::Zeroizing<Vec<u8>>> {
    loop {
        if let Some(value) = channel.open(&binary(socket).await?).map_err(|_| authentication())? {
            return Ok(value);
        }
    }
}

pub(super) async fn authenticate(
    socket: &mut WebSocketStream<impl AsyncRead + AsyncWrite + Unpin + Send>,
    epoch: &str,
    host: Arc<Mutex<HostState>>,
    persist: PersistHost,
    invitation: Arc<Mutex<Option<String>>>,
    route: Route,
    peer: Option<String>,
) -> io::Result<(SecureChannel, bool, super::host::DeviceSession)> {
    let (hello, mut channel, request) = time::timeout(Duration::from_secs(12), async {
        let first = binary(socket).await?;
        if first.len() > 512 || first[0] != 1 {
            return Err(invalid());
        }
        let hello: Hello = serde_json::from_slice(&first[1..]).map_err(|_| invalid())?;
        let mut channel = host.lock().map_err(|_| invalid())?.handshake(&hello, epoch, now()?)?;
        channel.read_handshake(&binary(socket).await?).map_err(|_| authentication())?;
        send(socket, channel.write_handshake().map_err(|_| authentication())?).await?;
        let bytes = plaintext(socket, &mut channel).await?;
        if bytes.len() > 1024 {
            return Err(invalid());
        }
        let request: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if request["type"] != "secure.connect" {
            return Err(invalid());
        }
        Ok((hello, channel, request))
    })
    .await
    .map_err(|_| invalid())??;
    let name = request["name"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 160 && !v.chars().any(char::is_control))
        .ok_or_else(invalid)?
        .to_owned();
    let was_invite = hello.invitation;
    let mut guard = None;
    if was_invite {
        let code = channel.verification_code().ok_or_else(invalid)?.to_owned();
        let ticket = host.lock().map_err(|_| invalid())?.request_pairing(
            &hello,
            &name,
            route,
            peer,
            code.clone(),
            now()?,
        )?;
        guard = Some(ApprovalGuard { host: host.clone(), id: ticket.id.clone() });
        if request["approval"] == true {
            encrypted(socket, &mut channel, json!({"type":"secure.approval", "code":code, "expiresAt":now()?.saturating_add(120)}).to_string().as_bytes()).await?;
        }
        let approved = time::timeout(Duration::from_secs(120), async {
            let mut decision = ticket.decision;
            let mut heartbeat = time::interval(Duration::from_secs(25));
            loop {
                tokio::select! {
                    result = &mut decision => return result.map_err(|_| authentication()),
                    frame = socket.next() => match frame {
                        Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => { socket.flush().await.map_err(|_| invalid())?; },
                        _ => return Err(authentication()),
                    },
                    _ = heartbeat.tick() => { socket.send(Message::Ping(Vec::new().into())).await.map_err(|_| invalid())?; },
                }
            }
        }).await.map_err(|_| authentication())??;
        if !approved {
            return Err(authentication());
        }
    }
    let approval = guard.as_ref().map(|guard| guard.id.clone());
    let (response, allow_input, session) = tokio::task::spawn_blocking(move || {
        host.lock().map_err(|_| invalid())?.enroll(
            &hello,
            &name,
            now()?,
            &persist,
            approval.as_deref(),
            route,
        )
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
    Ok((channel, allow_input, session))
}

struct ApprovalGuard {
    host: Arc<Mutex<HostState>>,
    id: String,
}

impl Drop for ApprovalGuard {
    fn drop(&mut self) {
        if let Ok(mut host) = self.host.lock() {
            host.cancel_pairing(&self.id);
        }
    }
}

pub(super) async fn exchange(
    socket: &mut WebSocketStream<impl AsyncRead + AsyncWrite + Unpin + Send>,
    mut channel: SecureChannel,
    factory: super::runtime::RuntimeFactory,
    shutdown: CancellationToken,
    authorization: super::runtime::Authorization,
) -> io::Result<()> {
    let mut permissions = authorization.input_changes();
    let (input, mut output) = bridge(factory);
    loop {
        tokio::select! {
            _=shutdown.cancelled()=>return Ok(()),
            changed=permissions.changed()=>{
                changed.map_err(|_|invalid())?;
                let allow_input = *permissions.borrow_and_update();
                // 权限更新不等待终端输出，静止的终端也立即更新手机输入状态。
                encrypted(socket,&mut channel,json!({"type":"mobile.policy","allow_input":allow_input}).to_string().as_bytes()).await?;
            },
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
