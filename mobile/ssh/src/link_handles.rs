use pebrel_mobile_link::{
    crypto::{LinkError, SecureChannel},
    identity::Secret,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

type Channel = Arc<Mutex<SecureChannel>>;
#[derive(Default)]
struct Handles {
    next: i64,
    channels: HashMap<i64, Channel>,
}

fn handles() -> &'static Mutex<Handles> {
    static HANDLES: OnceLock<Mutex<Handles>> = OnceLock::new();
    HANDLES.get_or_init(|| Mutex::new(Handles::default()))
}

pub(crate) fn channel(id: i64) -> Result<Channel, LinkError> {
    handles()
        .lock()
        .map_err(|_| LinkError::State)?
        .channels
        .get(&id)
        .cloned()
        .ok_or(LinkError::State)
}

pub(crate) fn create(host: &[u8], secret: &Secret, context: &[u8]) -> Result<i64, LinkError> {
    let channel = SecureChannel::initiator(
        host.try_into().map_err(|_| LinkError::Credential)?,
        secret,
        context,
    )?;
    let mut handles = handles().lock().map_err(|_| LinkError::State)?;
    if handles.channels.len() >= 32 {
        return Err(LinkError::State);
    }
    handles.next = handles.next.checked_add(1).ok_or(LinkError::State)?;
    let id = handles.next;
    handles.channels.insert(id, Arc::new(Mutex::new(channel)));
    Ok(id)
}

pub(crate) fn close(id: i64) -> Result<(), LinkError> {
    let owner = handles().lock().map_err(|_| LinkError::State)?.channels.remove(&id);
    if let Some(owner) = owner {
        owner.lock().map_err(|_| LinkError::State)?.close();
    }
    Ok(())
}
