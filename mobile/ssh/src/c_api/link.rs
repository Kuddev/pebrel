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
        out.write(link_handles::create(
            host.get(32)?,
            &Secret::from_bytes(secret.get(32)?)?,
            context.get(512)?,
        )?);
        Ok(())
    })
}

fn buffer<'a>(out: *mut u8, capacity: usize, minimum: usize) -> Result<&'a mut [MaybeUninit<u8>]> {
    if out.is_null() || capacity < minimum {
        return Err(INVALID);
    }
    Ok(unsafe { slice::from_raw_parts_mut(out.cast::<MaybeUninit<u8>>(), minimum) })
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
        out[..hello.len()].write_copy_of_slice(&hello);
        count.write(hello.len());
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_link_finish(id: i64, response: Bytes, established: *mut bool) -> Status {
    status(|| {
        let out = output(established)?;
        let response = response.get(128)?;
        out.write(with_channel(id, |channel| {
            channel.read_handshake(response)?;
            Ok(channel.established())
        })?);
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
            out[offset..offset + 4].write_copy_of_slice(&(packet.len() as u32).to_be_bytes());
            offset += 4;
            out[offset..offset + packet.len()].write_copy_of_slice(&packet);
            offset += packet.len();
        }
        count.write(offset);
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
        out[..message.len()].write_copy_of_slice(message);
        count.write(message.len());
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn pebrel_link_close(id: i64) -> Status {
    status(|| Ok(link_handles::close(id)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pebrel_mobile_link::crypto::HostKey;

    fn bytes(value: &[u8]) -> Bytes {
        Bytes { data: value.as_ptr(), len: value.len() }
    }

    fn success(error: Status) {
        if !error.is_null() {
            let message = unsafe { CString::from_raw(error) };
            panic!("C bridge failed: {message:?}");
        }
    }

    #[test]
    fn bounded_output_preserves_noise_state_and_packet_boundaries() {
        let host = HostKey::generate().unwrap();
        let secret = Secret::from_bytes(&[42; 32]).unwrap();
        let context = b"pebrel-c-abi-test";
        let mut remote = SecureChannel::responder(&host, &secret, context).unwrap();
        let mut id = 0;
        success(pebrel_link_create(
            bytes(&host.public()),
            bytes(&[42; 32]),
            bytes(context),
            &mut id,
        ));
        let mut out = [0; 128];
        let mut count = 0;
        let error = pebrel_link_hello(id, out.as_mut_ptr(), 1, &mut count);
        assert!(!error.is_null());
        pebrel_string_free(error);
        success(pebrel_link_hello(id, out.as_mut_ptr(), out.len(), &mut count));
        remote.read_handshake(&out[..count]).unwrap();
        let mut established = false;
        success(pebrel_link_finish(
            id,
            bytes(&remote.write_handshake().unwrap()),
            &mut established,
        ));
        assert!(established);
        let mut code = ptr::null_mut();
        success(pebrel_link_verification_code(id, &mut code));
        assert_eq!(
            unsafe { CStr::from_ptr(code) }.to_str().unwrap(),
            remote.verification_code().unwrap()
        );
        pebrel_string_free(code);

        let payload = vec![7; MAX_MESSAGE];
        let mut sealed = vec![0; MAX_MESSAGE + 28 * MAX_MESSAGE.div_ceil(60 * 1024)];
        let error = pebrel_link_seal(
            id,
            bytes(&payload),
            sealed.as_mut_ptr(),
            sealed.len() - 1,
            &mut count,
        );
        assert!(!error.is_null());
        pebrel_string_free(error);
        success(pebrel_link_seal(
            id,
            bytes(&payload),
            sealed.as_mut_ptr(),
            sealed.len(),
            &mut count,
        ));
        assert_eq!(count, sealed.len());
        let mut remaining = sealed.as_slice();
        while !remaining.is_empty() {
            let size = u32::from_be_bytes(remaining[..4].try_into().unwrap()) as usize;
            let (packet, tail) = remaining[4..].split_at(size);
            let plaintext = remote.open(packet).unwrap();
            if tail.is_empty() {
                assert_eq!(*plaintext.unwrap(), payload);
            } else {
                assert!(plaintext.is_none());
            }
            remaining = tail;
        }

        let mut plain = vec![0; MAX_MESSAGE];
        let mut received = false;
        for packet in remote.seal(&payload).unwrap() {
            success(pebrel_link_open(
                id,
                bytes(&packet),
                plain.as_mut_ptr(),
                plain.len(),
                &mut count,
            ));
            if count != 0 {
                assert!(!received);
                assert_eq!(&plain[..count], payload.as_slice());
                received = true;
            }
        }
        assert!(received);
        success(pebrel_link_close(id));
        let error = pebrel_link_verification_code(id, &mut code);
        assert!(!error.is_null());
        pebrel_string_free(error);
    }
}
