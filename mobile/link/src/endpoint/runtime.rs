//! Bounded adapter to the existing Runtime API. This module does not interpret
//! RPC methods or duplicate the application's input-authorization policy.
use std::{io, sync::Arc};
use tokio::sync::mpsc;

pub const MAX_REQUEST: usize = 40 * 1024;
pub type Reply = Arc<dyn Fn(Vec<u8>) -> io::Result<()> + Send + Sync>;
pub trait RuntimeSession: Send {
    fn request(&mut self, bytes: &[u8]) -> io::Result<()>;
}
pub type RuntimeFactory = Arc<dyn Fn(Reply) -> io::Result<Box<dyn RuntimeSession>> + Send + Sync>;
pub type AuthorizedFactory = Arc<dyn Fn(bool) -> RuntimeFactory + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Starting,
    Waiting,
    Connected,
    Reconnecting,
    Stopped,
    Failed,
}

pub(crate) fn bridge(
    factory: RuntimeFactory,
) -> (std::sync::mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
    let (input, requests) = std::sync::mpsc::sync_channel::<Vec<u8>>(8);
    let (output, replies) = mpsc::channel::<Vec<u8>>(8);
    std::thread::spawn(move || {
        let reply: Reply = Arc::new(move |bytes| {
            if bytes.len() > crate::crypto::MAX_MESSAGE {
                return Err(io::Error::other("frame_too_large"));
            }
            output.try_send(bytes).map_err(|_| io::Error::other("mobile_output_closed"))
        });
        let Ok(mut runtime) = factory(reply) else { return };
        while let Ok(bytes) = requests.recv() {
            if runtime.request(&bytes).is_err() {
                break;
            }
        }
    });
    (input, replies)
}
