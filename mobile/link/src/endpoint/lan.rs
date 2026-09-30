//! LAN 与中转使用同一 v2 握手和逐设备授权，区别只在 socket 的建立方式。

use super::{
    Handle, HostState, PersistHost, Route,
    runtime::{AuthorizedFactory, Status},
    transport,
};
use crate::{crypto::MAX_PACKET, identity::Secret, preview::LanCredentials};
use futures_util::{SinkExt, StreamExt};
use std::{
    io,
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{net::TcpListener, sync::Semaphore, time};
use tokio_tungstenite::{
    accept_hdr_async_with_config,
    tungstenite::{
        Message,
        handshake::server::{ErrorResponse, Request, Response},
        http::StatusCode,
        protocol::WebSocketConfig,
    },
};

pub async fn start_lan(
    credentials: &mut LanCredentials,
    host: Arc<Mutex<HostState>>,
    persist: PersistHost,
    name: &str,
    allow_input: bool,
    factory: AuthorizedFactory,
) -> io::Result<Handle> {
    let acceptor = credentials.acceptor()?;
    let address = SocketAddr::new(credentials.address, credentials.port);
    let mut attempts = 0;
    let listener = loop {
        match TcpListener::bind(address).await {
            Ok(listener) => break listener,
            Err(error) if error.kind() == io::ErrorKind::AddrInUse && attempts < 20 => {
                attempts += 1;
                time::sleep(Duration::from_millis(25)).await;
            },
            Err(error) => return Err(error),
        }
    };
    credentials.port = listener.local_addr()?.port();
    let access = credentials.access()?;
    let pin = access.tls_pin.clone();
    let path = format!("/v2/link?device={}&role=mobile", access.room);
    let hash = Secret::decode(&access.mobile_token).map_err(io::Error::other)?.hash();
    let mut handle = Handle::new(access, host.clone(), name, Route::Lan, allow_input)?;
    let host_id = host.lock().map_err(|_| super::invalid())?.id();
    handle.discovery = super::discovery::Advertisement::register(
        credentials.address,
        credentials.port,
        &host_id,
        name,
        &pin,
    );
    let pairing_code = handle.pairing_code.clone();
    let (shutdown, status, invitation) =
        (handle.shutdown.clone(), handle.status.clone(), handle.invitation.clone());
    tokio::spawn(async move {
        let admission = Arc::new(Semaphore::new(8));
        let online = Arc::new(AtomicUsize::new(0));
        let mut connections = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = connections.join_next(), if !connections.is_empty() => {},
                accepted = listener.accept() => {
                    let Ok((stream, peer)) = accepted else { transport::status(&status, Status::Failed); break };
                    if stream.set_nodelay(true).is_err() { continue; }
                    let Ok(permit) = admission.clone().try_acquire_owned() else { continue };
                    let (acceptor, host, persist, factory, shutdown, status, invitation, path, hash, online, pairing_code) =
                        (acceptor.clone(), host.clone(), persist.clone(), factory.clone(), shutdown.clone(), status.clone(), invitation.clone(), path.clone(), hash.clone(), online.clone(), pairing_code.clone());
                    connections.spawn(async move {
                        let _permit = permit;
                        let connect = async {
                            let stream = acceptor.accept(stream).await.map_err(io::Error::other)?;
                            let mut manual = false;
                            let callback = |request: &Request, response: Response| {
                                if request.uri().path_and_query().map(|v| v.as_str()) == Some("/v2/pair") && !request.headers().contains_key("origin") {
                                    manual = true;
                                    return Ok(response);
                                }
                                let token = request.headers().get("authorization").and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
                                if request.uri().path_and_query().map(|v| v.as_str()) != Some(path.as_str())
                                    || request.headers().contains_key("origin")
                                    || !token.and_then(|v| Secret::decode(v).ok()).is_some_and(|s| s.matches_hash(&hash)) {
                                    let mut denied = ErrorResponse::new(None);
                                    *denied.status_mut() = StatusCode::UNAUTHORIZED;
                                    return Err(denied);
                                }
                                Ok(response)
                            };
                            let config = WebSocketConfig::default().max_message_size(Some(MAX_PACKET)).max_frame_size(Some(MAX_PACKET))
                                .write_buffer_size(0).max_write_buffer_size(MAX_PACKET * 2);
                            let socket = accept_hdr_async_with_config(stream, callback, Some(config)).await.map_err(io::Error::other)?;
                            Ok::<_, io::Error>((socket, manual))
                        };
                        let (mut socket, manual) = tokio::select! {
                            _ = shutdown.cancelled() => return,
                            result = time::timeout(Duration::from_secs(10), connect) => match result { Ok(Ok(socket)) => socket, _ => return },
                        };
                        if manual {
                            let request = tokio::select! {
                                _ = shutdown.cancelled() => return,
                                result = time::timeout(Duration::from_secs(5), socket.next()) => match result {
                                    Ok(Some(Ok(Message::Text(text)))) if text.len() <= 128 => text,
                                    _ => return,
                                },
                            };
                            #[derive(serde::Deserialize)]
                            #[serde(deny_unknown_fields)]
                            struct Lookup { code: String }
                            let invitation = serde_json::from_str::<Lookup>(&request).ok().and_then(|request| {
                                let host = host.lock().ok()?;
                                let mut code = pairing_code.lock().ok()?;
                                code.as_mut()?.redeem(&request.code, &host, super::now().ok()?).ok()
                            });
                            let response = match invitation {
                                Some(invitation) => serde_json::json!({"invitation": invitation}),
                                None => serde_json::json!({"error":"pairing_code_invalid"}),
                            };
                            let _ = time::timeout(Duration::from_secs(5), socket.send(Message::Text(response.to_string().into()))).await;
                            let _ = time::timeout(Duration::from_secs(1), socket.close(None)).await;
                            return;
                        }
                        let Ok(epoch) = Secret::generate().map(|v| v.hash()) else { return };
                        if socket.send(Message::Text(serde_json::json!({"type":"relay.paired","version":2,"link":epoch}).to_string().into())).await.is_err() { return; }
                        let result = tokio::select! {
                            _ = shutdown.cancelled() => return,
                            result = time::timeout(Duration::from_secs(150), transport::authenticate(&mut socket, &epoch, host, persist, invitation, Route::Lan, Some(peer.ip().to_string()))) => result,
                        };
                        let Ok(Ok((channel, _, session))) = result else { return };
                        session.mark_connected();
                        online.fetch_add(1, Ordering::AcqRel);
                        transport::status(&status, Status::Connected);
                        tokio::select! {
                            biased;
                            _ = session.cancelled() => {},
                            _ = transport::exchange(&mut socket, channel, factory(session.authorization.clone()), shutdown.clone(), session.authorization.clone()) => {},
                        }
                        if online.fetch_sub(1, Ordering::AcqRel) == 1 && !shutdown.is_cancelled() {
                            transport::status(&status, Status::Waiting);
                        }
                    });
                }
            }
        }
        drop(listener);
        connections.shutdown().await;
        if shutdown.is_cancelled() {
            transport::status(&status, Status::Stopped);
        }
    });
    Ok(handle)
}
