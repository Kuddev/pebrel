//! Android invokes the exact same crypto/framing implementation as desktop.
//! Handles are bounded IDs, never raw pointers; close revokes and zeroizes them.

use jni::{
    JNIEnv,
    objects::{JByteArray, JObject},
    sys::{jboolean, jbyteArray, jlong, jobjectArray},
};
use pebrel_mobile_link::{
    crypto::{LinkError, MAX_MESSAGE, MAX_PACKET, SecureChannel},
    identity::Secret,
};
use std::{
    collections::HashMap,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex, OnceLock},
};
use zeroize::Zeroizing;

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

fn invoke<T: Default>(
    env: &mut JNIEnv,
    call: impl FnOnce(&mut JNIEnv) -> Result<T, LinkError>,
) -> T {
    match catch_unwind(AssertUnwindSafe(|| call(env))) {
        Ok(Ok(value)) => value,
        error => {
            let code = match error {
                Ok(Err(error)) => error.to_string(),
                _ => "secure_link_internal_error".into(),
            };
            let _ = env.throw_new("java/io/IOException", code);
            T::default()
        },
    }
}

fn bytes(env: &JNIEnv, array: &JByteArray, max: usize) -> Result<Zeroizing<Vec<u8>>, LinkError> {
    let size = env.get_array_length(array).map_err(|_| LinkError::Frame)?;
    if size < 0 || size as usize > max {
        return Err(LinkError::Frame);
    }
    Ok(Zeroizing::new(env.convert_byte_array(array).map_err(|_| LinkError::Frame)?))
}

fn channel(id: i64) -> Result<Channel, LinkError> {
    handles()
        .lock()
        .map_err(|_| LinkError::State)?
        .channels
        .get(&id)
        .cloned()
        .ok_or(LinkError::State)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_kuddev_pebrel_ssh_NativeLink_create(
    mut env: JNIEnv,
    _object: JObject,
    host: JByteArray,
    secret: JByteArray,
    context: JByteArray,
) -> jlong {
    invoke(&mut env, |env| {
        let host = bytes(env, &host, 32)?;
        let secret = Secret::from_bytes(&bytes(env, &secret, 32)?)?;
        let context = bytes(env, &context, 512)?;
        let channel = SecureChannel::initiator(
            host.as_slice().try_into().map_err(|_| LinkError::Credential)?,
            &secret,
            &context,
        )?;
        let mut handles = handles().lock().map_err(|_| LinkError::State)?;
        if handles.channels.len() >= 32 {
            return Err(LinkError::State);
        }
        handles.next = handles.next.checked_add(1).ok_or(LinkError::State)?;
        let id = handles.next;
        handles.channels.insert(id, Arc::new(Mutex::new(channel)));
        Ok(id)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_kuddev_pebrel_ssh_NativeLink_hello(
    mut env: JNIEnv,
    _object: JObject,
    id: jlong,
) -> jbyteArray {
    invoke(&mut env, |env| {
        let owner = channel(id)?;
        let output = owner.lock().map_err(|_| LinkError::State)?.write_handshake()?;
        env.byte_array_from_slice(&output)
            .map(|bytes| bytes.into_raw())
            .map_err(|_| LinkError::State)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_kuddev_pebrel_ssh_NativeLink_finish(
    mut env: JNIEnv,
    _object: JObject,
    id: jlong,
    response: JByteArray,
) -> jboolean {
    invoke(&mut env, |env| {
        let owner = channel(id)?;
        let mut channel = owner.lock().map_err(|_| LinkError::State)?;
        channel.read_handshake(&bytes(env, &response, 128)?)?;
        Ok(u8::from(channel.established()))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_kuddev_pebrel_ssh_NativeLink_seal(
    mut env: JNIEnv,
    _object: JObject,
    id: jlong,
    plaintext: JByteArray,
) -> jobjectArray {
    invoke(&mut env, |env| {
        let owner = channel(id)?;
        let output = owner.lock().map_err(|_| LinkError::State)?.seal(&bytes(
            env,
            &plaintext,
            MAX_MESSAGE,
        )?)?;
        let arrays = env
            .new_object_array(output.len() as i32, "[B", JObject::null())
            .map_err(|_| LinkError::State)?;
        for (index, packet) in output.iter().enumerate() {
            let bytes = env.byte_array_from_slice(packet).map_err(|_| LinkError::State)?;
            env.set_object_array_element(&arrays, index as i32, &bytes)
                .map_err(|_| LinkError::State)?;
            env.delete_local_ref(bytes).map_err(|_| LinkError::State)?;
        }
        Ok(arrays.into_raw())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_kuddev_pebrel_ssh_NativeLink_open(
    mut env: JNIEnv,
    _object: JObject,
    id: jlong,
    ciphertext: JByteArray,
) -> jbyteArray {
    invoke(&mut env, |env| {
        let owner = channel(id)?;
        let result = owner.lock().map_err(|_| LinkError::State)?.open(&bytes(
            env,
            &ciphertext,
            MAX_PACKET,
        )?)?;
        match result {
            Some(plaintext) => env
                .byte_array_from_slice(&plaintext)
                .map(|bytes| bytes.into_raw())
                .map_err(|_| LinkError::State),
            None => Ok(std::ptr::null_mut()),
        }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_kuddev_pebrel_ssh_NativeLink_close(
    mut env: JNIEnv,
    _object: JObject,
    id: jlong,
) {
    invoke(&mut env, |_| {
        let owner = handles().lock().map_err(|_| LinkError::State)?.channels.remove(&id);
        if let Some(owner) = owner {
            owner.lock().map_err(|_| LinkError::State)?.close();
        }
        Ok(())
    });
}
