use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use axum::{extract::ws::Message, http::StatusCode};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::RoomConfig;
use crate::identity::Secret;

pub const QUEUE_PACKETS: usize = 8;

#[derive(Clone, Copy)]
pub enum Role {
    Desktop = 0,
    Mobile = 1,
}

impl Role {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "desktop" => Some(Self::Desktop),
            "mobile" => Some(Self::Mobile),
            _ => None,
        }
    }
    fn peer(self) -> usize {
        1 - self as usize
    }
}

struct Peer {
    generation: u64,
    send: mpsc::Sender<Message>,
    cancel: CancellationToken,
    active: bool,
}

struct Room {
    hashes: [String; 2],
    peers: [Option<Peer>; 2],
}

pub struct Registry {
    rooms: HashMap<String, Room>,
    generation: u64,
}

pub type SharedRegistry = Arc<Mutex<Registry>>;

pub struct Lease {
    registry: SharedRegistry,
    room: String,
    role: Role,
    generation: u64,
    pub cancel: CancellationToken,
}

impl Registry {
    pub fn new(configs: Vec<RoomConfig>) -> SharedRegistry {
        Arc::new(Mutex::new(Self {
            rooms: configs
                .into_iter()
                .map(|room| {
                    (
                        room.id,
                        Room {
                            hashes: [room.desktop_token_hash, room.mobile_token_hash],
                            peers: [None, None],
                        },
                    )
                })
                .collect(),
            generation: 0,
        }))
    }

    pub fn reserve(
        registry: &SharedRegistry,
        room: &str,
        role: Role,
        token: &str,
    ) -> Result<(Lease, mpsc::Receiver<Message>), StatusCode> {
        let secret = Secret::decode(token).map_err(|_| StatusCode::UNAUTHORIZED)?;
        let mut state = registry.lock().map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let entry = state.rooms.get(room).ok_or(StatusCode::UNAUTHORIZED)?;
        if !secret.matches_hash(&entry.hashes[role as usize]) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        if entry.peers[role as usize].is_some() {
            return Err(StatusCode::CONFLICT);
        }
        state.generation =
            state.generation.checked_add(1).ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        let generation = state.generation;
        let (send, receive) = mpsc::channel(QUEUE_PACKETS);
        let cancel = CancellationToken::new();
        state.rooms.get_mut(room).unwrap().peers[role as usize] =
            Some(Peer { generation, send, cancel: cancel.clone(), active: false });
        Ok((
            Lease { registry: registry.clone(), room: room.to_owned(), role, generation, cancel },
            receive,
        ))
    }
}

impl Lease {
    pub fn announce(&self) -> Result<(), ()> {
        let mut state = self.registry.lock().map_err(|_| ())?;
        let room = state.rooms.get_mut(&self.room).ok_or(())?;
        let own = room.peers[self.role as usize].as_mut().ok_or(())?;
        if own.generation != self.generation || own.active || self.cancel.is_cancelled() {
            return Err(());
        }
        own.active = true;
        let paired = room
            .peers
            .iter()
            .all(|peer| peer.as_ref().is_some_and(|p| p.active && !p.cancel.is_cancelled()));
        let message = if paired {
            let epoch = Secret::generate().map_err(|_| ())?.hash();
            serde_json::json!({"type": "relay.paired", "version": 2, "link": epoch})
        } else {
            serde_json::json!({"type": "relay.waiting", "version": 2})
        };
        for (index, peer) in room.peers.iter().enumerate() {
            let Some(peer) = peer else { continue };
            if !paired && index != self.role as usize {
                continue;
            }
            peer.send.try_send(Message::Text(message.to_string().into())).map_err(|_| ())?;
        }
        Ok(())
    }

    pub fn forward(&self, packet: axum::body::Bytes) -> Result<(), ()> {
        let state = self.registry.lock().map_err(|_| ())?;
        let room = state.rooms.get(&self.room).ok_or(())?;
        if self.cancel.is_cancelled() {
            return Err(());
        }
        let target = room.peers[self.role.peer()].as_ref().ok_or(())?;
        if !target.active || target.cancel.is_cancelled() {
            return Err(());
        }
        target.send.try_send(Message::Binary(packet)).map_err(|_| {
            target.cancel.cancel();
        })
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let Ok(mut state) = self.registry.lock() else { return };
        let Some(room) = state.rooms.get_mut(&self.room) else { return };
        if room.peers[self.role as usize]
            .as_ref()
            .is_some_and(|peer| peer.generation == self.generation)
        {
            room.peers[self.role as usize] = None;
            // No store-and-forward and no reuse of a live socket for a new peer.
            // Both ends reconnect and establish new Noise keys after any loss.
            if let Some(peer) = &room.peers[self.role.peer()] {
                peer.cancel.cancel();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_upgrades_must_finish_and_a_slow_peer_cannot_grow_the_queue() {
        let desktop = Secret::generate().unwrap();
        let mobile = Secret::generate().unwrap();
        let registry = Registry::new(vec![RoomConfig {
            id: "host".into(),
            desktop_token_hash: desktop.hash(),
            mobile_token_hash: mobile.hash(),
        }]);
        let (a, mut a_queue) =
            Registry::reserve(&registry, "host", Role::Desktop, &desktop.expose_encoded()).unwrap();
        let (b, mut b_queue) =
            Registry::reserve(&registry, "host", Role::Mobile, &mobile.expose_encoded()).unwrap();
        a.announce().unwrap();
        assert!(a_queue.try_recv().unwrap().to_text().unwrap().contains("relay.waiting"));
        assert!(b_queue.try_recv().is_err());
        assert!(a.forward(axum::body::Bytes::from_static(b"before upgrade")).is_err());
        b.announce().unwrap();
        assert!(a_queue.try_recv().unwrap().to_text().unwrap().contains("relay.paired"));
        assert!(b_queue.try_recv().unwrap().to_text().unwrap().contains("relay.paired"));
        for _ in 0..QUEUE_PACKETS {
            a.forward(axum::body::Bytes::from_static(b"ciphertext")).unwrap();
        }
        assert!(a.forward(axum::body::Bytes::from_static(b"over budget")).is_err());
        assert!(b.cancel.is_cancelled());
        drop(b);
        assert!(a.cancel.is_cancelled());
    }
}
