//! Noise NKpsk0: pinned host identity plus a per-device, out-of-band secret.
//! Relay routing credentials are deliberately NOT this secret. Ordered Noise
//! transport nonces reject replay/reordering; every connection starts a new
//! ephemeral handshake. Any cryptographic/frame error permanently closes the link.

use snow::{Builder, HandshakeState, TransportState};
use zeroize::Zeroizing;

use crate::identity::Secret;

const PATTERN: &str = "Noise_NKpsk0_25519_ChaChaPoly_SHA256";
pub const MAX_PACKET: usize = 65_535;
pub const MAX_MESSAGE: usize = 2 * 1024 * 1024;
const CHUNK: usize = 60 * 1024;
const HEADER: usize = 8;

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("invalid_credential")]
    Credential,
    #[error("secure_random_unavailable")]
    Random,
    #[error("invalid_link_state")]
    State,
    #[error("invalid_secure_frame")]
    Frame,
    #[error("secure_authentication_failed")]
    Authentication,
}

pub struct HostKey {
    secret: Secret,
    public: [u8; 32],
}

impl HostKey {
    pub fn generate() -> Result<Self, LinkError> {
        let keys = builder()?.generate_keypair().map_err(|_| LinkError::Random)?;
        let private = Zeroizing::new(keys.private);
        Ok(Self {
            secret: Secret(Zeroizing::new(
                private.as_slice().try_into().map_err(|_| LinkError::Credential)?,
            )),
            public: keys.public.try_into().map_err(|_| LinkError::Credential)?,
        })
    }

    pub fn restore(secret: Secret, public: [u8; 32]) -> Self {
        Self { secret, public }
    }

    pub fn public(&self) -> [u8; 32] {
        self.public
    }

    pub fn export_secret(&self) -> Zeroizing<String> {
        self.secret.expose_encoded()
    }
}

fn builder<'a>() -> Result<Builder<'a>, LinkError> {
    Ok(Builder::new(PATTERN.parse().map_err(|_| LinkError::State)?))
}

enum State {
    Handshake(Box<HandshakeState>),
    Transport(Box<TransportState>),
    Closed,
}

pub struct SecureChannel {
    state: State,
    assembly: Zeroizing<Vec<u8>>,
    expected_size: usize,
}

impl SecureChannel {
    /// Context includes the protocol version, host/grant and current relay epoch.
    pub fn initiator(host: &[u8; 32], secret: &Secret, context: &[u8]) -> Result<Self, LinkError> {
        let state = builder()?
            .remote_public_key(host)
            .map_err(|_| LinkError::Credential)?
            .psk(0, &secret.0)
            .map_err(|_| LinkError::Credential)?
            .prologue(context)
            .map_err(|_| LinkError::State)?
            .build_initiator()
            .map_err(|_| LinkError::Credential)?;
        Ok(Self::handshake(state))
    }

    pub fn responder(host: &HostKey, secret: &Secret, context: &[u8]) -> Result<Self, LinkError> {
        let state = builder()?
            .local_private_key(host.secret.0.as_ref())
            .map_err(|_| LinkError::Credential)?
            .psk(0, &secret.0)
            .map_err(|_| LinkError::Credential)?
            .prologue(context)
            .map_err(|_| LinkError::State)?
            .build_responder()
            .map_err(|_| LinkError::Credential)?;
        Ok(Self::handshake(state))
    }

    fn handshake(state: HandshakeState) -> Self {
        Self {
            state: State::Handshake(Box::new(state)),
            assembly: Zeroizing::new(Vec::new()),
            expected_size: 0,
        }
    }

    pub fn established(&self) -> bool {
        matches!(self.state, State::Transport(_))
    }

    pub fn close(&mut self) {
        self.state = State::Closed;
        self.assembly = Zeroizing::new(Vec::new());
        self.expected_size = 0;
    }

    pub fn write_handshake(&mut self) -> Result<Vec<u8>, LinkError> {
        let mut bytes = vec![0; 128];
        let result = match &mut self.state {
            State::Handshake(state) => {
                state.write_message(&[], &mut bytes).map_err(|_| LinkError::Authentication)
            },
            _ => Err(LinkError::State),
        };
        match result {
            Ok(size) => {
                bytes.truncate(size);
                self.finish_handshake()?;
                Ok(bytes)
            },
            Err(error) => {
                self.close();
                Err(error)
            },
        }
    }

    pub fn read_handshake(&mut self, bytes: &[u8]) -> Result<(), LinkError> {
        let result = match &mut self.state {
            State::Handshake(state) if bytes.len() <= 128 => {
                state.read_message(bytes, &mut []).map_err(|_| LinkError::Authentication)
            },
            _ => Err(LinkError::State),
        };
        if result.is_err() {
            self.close();
            return Err(LinkError::Authentication);
        }
        self.finish_handshake()
    }

    fn finish_handshake(&mut self) -> Result<(), LinkError> {
        if matches!(&self.state, State::Handshake(state) if state.is_handshake_finished()) {
            let State::Handshake(state) = std::mem::replace(&mut self.state, State::Closed) else {
                unreachable!()
            };
            self.state = State::Transport(Box::new(
                state.into_transport_mode().map_err(|_| LinkError::Authentication)?,
            ));
        }
        Ok(())
    }

    pub fn seal(&mut self, message: &[u8]) -> Result<Vec<Vec<u8>>, LinkError> {
        let result = self.seal_inner(message);
        if result.is_err() {
            self.close();
        }
        result
    }

    fn seal_inner(&mut self, message: &[u8]) -> Result<Vec<Vec<u8>>, LinkError> {
        if message.is_empty() || message.len() > MAX_MESSAGE {
            return Err(LinkError::Frame);
        }
        let State::Transport(state) = &mut self.state else { return Err(LinkError::State) };
        let mut packets = Vec::with_capacity(message.len().div_ceil(CHUNK));
        for (index, chunk) in message.chunks(CHUNK).enumerate() {
            let mut plain = Zeroizing::new(Vec::with_capacity(HEADER + chunk.len()));
            plain.extend_from_slice(&(message.len() as u32).to_be_bytes());
            plain.extend_from_slice(&((index * CHUNK) as u32).to_be_bytes());
            plain.extend_from_slice(chunk);
            let mut packet = vec![0; plain.len() + 16];
            let size =
                state.write_message(&plain, &mut packet).map_err(|_| LinkError::Authentication)?;
            packet.truncate(size);
            packets.push(packet);
        }
        Ok(packets)
    }

    pub fn open(&mut self, packet: &[u8]) -> Result<Option<Zeroizing<Vec<u8>>>, LinkError> {
        let result = self.open_inner(packet);
        if result.is_err() {
            self.close();
        }
        result
    }

    fn open_inner(&mut self, packet: &[u8]) -> Result<Option<Zeroizing<Vec<u8>>>, LinkError> {
        if !(HEADER + 17..=MAX_PACKET).contains(&packet.len()) {
            return Err(LinkError::Frame);
        }
        let State::Transport(state) = &mut self.state else { return Err(LinkError::State) };
        let mut plain = Zeroizing::new(vec![0; packet.len()]);
        let size = state.read_message(packet, &mut plain).map_err(|_| LinkError::Authentication)?;
        if size <= HEADER {
            return Err(LinkError::Frame);
        }
        let total = u32::from_be_bytes(plain[..4].try_into().unwrap()) as usize;
        let offset = u32::from_be_bytes(plain[4..HEADER].try_into().unwrap()) as usize;
        let payload = &plain[HEADER..size];
        if total == 0
            || total > MAX_MESSAGE
            || offset != self.assembly.len()
            || payload.len() > CHUNK
            || offset + payload.len() > total
            || (offset > 0 && total != self.expected_size)
        {
            return Err(LinkError::Frame);
        }
        if offset == 0 {
            self.expected_size = total;
        }
        self.assembly.extend_from_slice(payload);
        if self.assembly.len() == total {
            self.expected_size = 0;
            Ok(Some(std::mem::replace(&mut self.assembly, Zeroizing::new(Vec::new()))))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (SecureChannel, SecureChannel) {
        let host = HostKey::generate().unwrap();
        let secret = Secret::generate().unwrap();
        let mut phone =
            SecureChannel::initiator(&host.public(), &secret, b"test:host:grant:epoch").unwrap();
        let mut desktop =
            SecureChannel::responder(&host, &secret, b"test:host:grant:epoch").unwrap();
        desktop.read_handshake(&phone.write_handshake().unwrap()).unwrap();
        phone.read_handshake(&desktop.write_handshake().unwrap()).unwrap();
        assert!(phone.established() && desktop.established());
        (phone, desktop)
    }

    #[test]
    fn large_frames_are_bounded_and_round_trip() {
        let (mut phone, mut desktop) = pair();
        let message = vec![b'x'; MAX_MESSAGE];
        let packets = phone.seal(&message).unwrap();
        let mut received = None;
        for packet in packets {
            assert!(packet.len() <= MAX_PACKET);
            assert!(received.is_none());
            received = desktop.open(&packet).unwrap();
        }
        assert_eq!(received.unwrap().as_slice(), message);
    }

    #[test]
    fn replay_and_tampering_close_the_channel() {
        let (mut phone, mut desktop) = pair();
        let packet = phone.seal(b"command").unwrap().remove(0);
        assert!(desktop.open(&packet).unwrap().is_some());
        assert!(desktop.open(&packet).is_err());
        assert!(!desktop.established());
        let (mut phone, mut desktop) = pair();
        let mut packet = phone.seal(b"command").unwrap().remove(0);
        packet[0] ^= 1;
        assert!(desktop.open(&packet).is_err());
        assert!(!desktop.established());
    }

    #[test]
    fn wrong_invite_and_wrong_epoch_cannot_authenticate() {
        for different_epoch in [false, true] {
            let host = HostKey::generate().unwrap();
            let secret = Secret::generate().unwrap();
            let wrong = Secret::generate().unwrap();
            let mut phone = SecureChannel::initiator(&host.public(), &secret, b"epoch1").unwrap();
            let mut desktop = SecureChannel::responder(
                &host,
                if different_epoch { &secret } else { &wrong },
                if different_epoch { b"epoch2" } else { b"epoch1" },
            )
            .unwrap();
            assert!(desktop.read_handshake(&phone.write_handshake().unwrap()).is_err());
        }
    }

    #[test]
    fn order_direction_and_previous_connections_are_authenticated() {
        let (mut phone, mut desktop) = pair();
        let mut packets = phone.seal(&vec![7; CHUNK + 1]).unwrap();
        assert!(desktop.open(&packets.remove(1)).is_err());
        let (mut phone, mut desktop) = pair();
        let packet = phone.seal(b"input").unwrap().remove(0);
        assert!(phone.open(&packet).is_err());
        let (_, mut next) = pair();
        assert!(next.open(&packet).is_err());
        assert!(desktop.open(&packet).unwrap().is_some());
    }
}
