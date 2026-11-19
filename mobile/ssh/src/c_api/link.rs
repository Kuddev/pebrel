use super::*;
use crate::link_handles;
use pebrel_mobile_link::{
    crypto::{LinkError, MAX_MESSAGE, MAX_PACKET, SecureChannel},
    identity::Secret,
};

impl From<LinkError> for Failure {
    fn from(error: LinkError) -> Self {
        Self(match error {
            LinkError::Credential => "invalid_credential",
            LinkError::Random => "secure_random_unavailable",
            LinkError::State => "invalid_link_state",
            LinkError::Frame => "invalid_secure_frame",
            LinkError::Authentication => "secure_authentication_failed",
        })
    }
}

fn with_channel<T>(
    id: i64,
    call: impl FnOnce(&mut SecureChannel) -> std::result::Result<T, LinkError>,
) -> Result<T> {
    let owner = link_handles::channel(id)?;
    let mut channel = owner.lock().map_err(|_| LinkError::State)?;
    Ok(call(&mut channel)?)
}

#[unsafe(no_mangle)]
pub extern "C" fn pebrel_link_create(
    host: Bytes,
    secret: Bytes,
    context: Bytes,
    out: *mut i64,
) -> Status {
    status(|| {
        let out = output(out)?;
        *out = link_handles::create(
            host.get(32)?,
            &Secret::from_bytes(secret.get(32)?)?,
            context.get(512)?,
        )?;
        Ok(())
    })
}

fn buffer<'a>(out: *mut u8, capacity: usize, minimum: usize) -> Result<&'a mut [u8]> {
    if out.is_null() || capacity < minimum {
        return Err(INVALID);
    }
    Ok(unsafe { slice::from_raw_parts_mut(out, minimum) })
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_link_hello(
    id: i64,
    out: *mut u8,
    capacity: usize,
    count: *mut usize,
) -> Status {
    status(|| {
        let out = buffer(out, capacity, 128)?;
        let count = output(count)?;
        let hello = with_channel(id, SecureChannel::write_handshake)?;
        out[..hello.len()].copy_from_slice(&hello);
        *count = hello.len();
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_link_finish(id: i64, response: Bytes, established: *mut bool) -> Status {
    status(|| {
        let out = output(established)?;
        let response = response.get(128)?;
        *out = with_channel(id, |channel| {
            channel.read_handshake(response)?;
            Ok(channel.established())
        })?;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_link_verification_code(id: i64, out: *mut Status) -> Status {
    status(|| {
        string_out(
            output(out)?,
            with_channel(id, |channel| {
                channel.verification_code().map(str::to_owned).ok_or(LinkError::State)
            })?,
        )
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_link_seal(
    id: i64,
    input: Bytes,
    out: *mut u8,
    capacity: usize,
    count: *mut usize,
) -> Status {
    status(|| {
        let message = input.get(MAX_MESSAGE)?;
        let out = buffer(out, capacity, message.len() + 28 * message.len().div_ceil(60 * 1024))?;
        let count = output(count)?;
        let packets = with_channel(id, |channel| channel.seal(message))?;
        let mut offset = 0;
        for packet in packets {
            out[offset..offset + 4].copy_from_slice(&(packet.len() as u32).to_be_bytes());
            offset += 4;
            out[offset..offset + packet.len()].copy_from_slice(&packet);
            offset += packet.len();
        }
        *count = offset;
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_link_open(
    id: i64,
    packet: Bytes,
    out: *mut u8,
    capacity: usize,
    count: *mut usize,
) -> Status {
    status(|| {
        let out = buffer(out, capacity, MAX_MESSAGE)?;
        let count = output(count)?;
        let packet = packet.get(MAX_PACKET)?;
        let message = with_channel(id, |channel| channel.open(packet))?;
        let message = message.as_deref().map(Vec::as_slice).unwrap_or_default();
        out[..message.len()].copy_from_slice(message);
        *count = message.len();
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_link_close(id: i64) -> Status {
    status(|| Ok(link_handles::close(id)?))
}
