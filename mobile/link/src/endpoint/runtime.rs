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

pub(crate) fn bridge(factory: RuntimeFactory) -> (mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
    let (input, mut requests) = mpsc::channel::<Vec<u8>>(8);
    let (output, replies) = mpsc::channel::<Vec<u8>>(8);
    std::thread::spawn(move || {
        let reply: Reply = Arc::new(move |bytes| {
            if bytes.len() > crate::crypto::MAX_MESSAGE {
                return Err(io::Error::other("frame_too_large"));
            }
            // This callback runs only on owned Runtime/stream worker threads.
            // Backpressure must not turn an ordinary burst into a disconnect;
            // dropping the socket receiver unblocks these writers on teardown.
            output.blocking_send(bytes).map_err(|_| io::Error::other("mobile_output_closed"))
        });
        let Ok(mut runtime) = factory(reply) else { return };
        while let Some(bytes) = requests.blocking_recv() {
            if runtime.request(&bytes).is_err() {
                break;
            }
        }
    });
    (input, replies)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    struct Echo(Reply, Arc<AtomicUsize>);
    impl RuntimeSession for Echo {
        fn request(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.1.fetch_add(1, Ordering::SeqCst);
            (self.0)(bytes.to_vec())
        }
    }

    #[tokio::test]
    async fn mobile_latency_runtime_burst_backpressures_without_dropping_or_reordering() {
        let started = Arc::new(AtomicUsize::new(0));
        let entered = started.clone();
        let (input, mut output) =
            bridge(Arc::new(move |reply| Ok(Box::new(Echo(reply, entered.clone())))));
        time_limit(async {
            for byte in 0..16 {
                input.send(vec![byte]).await.unwrap();
            }
            while started.load(Ordering::SeqCst) < 9 {
                tokio::task::yield_now().await;
            }
            // The ninth reply waits behind eight, without closing the session.
            assert!(!input.is_closed());
            for byte in 0..16 {
                assert_eq!(output.recv().await.unwrap(), vec![byte]);
            }
            drop(input);
            assert!(output.recv().await.is_none());
        })
        .await;
    }

    #[tokio::test]
    async fn mobile_latency_runtime_disconnect_releases_a_backpressured_writer() {
        let started = Arc::new(AtomicUsize::new(0));
        let entered = started.clone();
        let (input, output) =
            bridge(Arc::new(move |reply| Ok(Box::new(Echo(reply, entered.clone())))));
        time_limit(async {
            for byte in 0..9 {
                input.send(vec![byte]).await.unwrap();
            }
            while started.load(Ordering::SeqCst) < 9 {
                tokio::task::yield_now().await;
            }
            drop(output);
            input.closed().await;
        })
        .await;
    }

    async fn time_limit(task: impl std::future::Future<Output = ()>) {
        tokio::time::timeout(Duration::from_secs(3), task).await.unwrap();
    }
}
