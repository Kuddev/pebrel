//! JNI contains no protocol state and never exposes a Rust allocation as a pointer.
use std::panic::{AssertUnwindSafe, catch_unwind};

use jni::JNIEnv;
use jni::objects::{JByteArray, JObject, JString};
use jni::sys::{jboolean, jint, jlong, jstring};
use zeroize::Zeroizing;

use crate::session::{self, Failure, Geometry, Open, Options, Result, runtime};

fn invoke<T: Default>(env: &mut JNIEnv, call: impl FnOnce(&mut JNIEnv) -> Result<T>) -> T {
    match catch_unwind(AssertUnwindSafe(|| call(env))) {
        Ok(Ok(value)) => value,
        failure => {
            let code = match failure {
                Ok(Err(error)) => error.0,
                _ => "INTERNAL",
            };
            let _ = env.throw_new("io/github/kuddev/pebrel/ssh/NativeSshException", code);
            T::default()
        },
    }
}

fn string(env: &mut JNIEnv, value: &JString) -> Result<String> {
    env.get_string(value).map(Into::into).map_err(|_| Failure("INTERNAL"))
}

fn bounds(
    env: &JNIEnv,
    bytes: &JByteArray,
    offset: jint,
    count: jint,
    maximum: jint,
) -> Result<()> {
    let length = env.get_array_length(bytes).map_err(|_| Failure("INTERNAL"))?;
    if offset < 0 || count <= 0 || count > maximum || offset > length - count {
        Err(Failure("INVALID_INPUT"))
    } else {
        Ok(())
    }
}

fn geometry(columns: jint, rows: jint, width: jint, height: jint) -> Result<Geometry> {
    if !(1..=4096).contains(&columns) || !(1..=4096).contains(&rows) || width < 0 || height < 0 {
        return Err(Failure("INVALID_INPUT"));
    }
    Ok(Geometry {
        columns: columns as u32,
        rows: rows as u32,
        width: width as u32,
        height: height as u32,
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_kuddev_pebrel_ssh_NativeSsh_create(
    mut env: JNIEnv,
    _object: JObject,
    host: JString,
    port: jint,
    user: JString,
    password: JByteArray,
    fingerprint: JString,
) -> jlong {
    invoke(&mut env, |env| {
        let size = env.get_array_length(&password).map_err(|_| Failure("INTERNAL"))?;
        if !(1..=65535).contains(&port) || size > 4096 {
            return Err(Failure("INVALID_INPUT"));
        }
        let password =
            Zeroizing::new(env.convert_byte_array(&password).map_err(|_| Failure("INTERNAL"))?);
        let options = Options {
            host: string(env, &host)?,
            port: port as u16,
            user: string(env, &user)?,
            password,
            fingerprint: string(env, &fingerprint)?,
        };
        if options.host.len() > 1024 || options.user.len() > 1024 || options.fingerprint.len() > 256
        {
            return Err(Failure("INVALID_INPUT"));
        }
        session::start(options)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_kuddev_pebrel_ssh_NativeSsh_nextEvent(
    mut env: JNIEnv,
    _object: JObject,
    id: jlong,
) -> jstring {
    invoke(&mut env, |env| {
        let owner = session::get(id)?;
        let event = runtime().block_on(owner.event())?;
        env.new_string(event).map(|value| value.into_raw()).map_err(|_| Failure("INTERNAL"))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_kuddev_pebrel_ssh_NativeSsh_answerHostKey(
    mut env: JNIEnv,
    _object: JObject,
    id: jlong,
    accepted: jboolean,
) {
    invoke(&mut env, |_| session::get(id)?.answer(accepted != 0));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_kuddev_pebrel_ssh_NativeSsh_openShell(
    mut env: JNIEnv,
    _object: JObject,
    id: jlong,
    columns: jint,
    rows: jint,
    width: jint,
    height: jint,
) {
    invoke(&mut env, |_| {
        let owner = session::get(id)?;
        runtime().block_on(owner.open(Open::Shell(geometry(columns, rows, width, height)?)))
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_kuddev_pebrel_ssh_NativeSsh_openExec(
    mut env: JNIEnv,
    _object: JObject,
    id: jlong,
    command: JString,
) {
    invoke(&mut env, |env| {
        let command = string(env, &command)?;
        if command.len() > 8192 {
            return Err(Failure("INVALID_INPUT"));
        }
        let owner = session::get(id)?;
        runtime().block_on(owner.open(Open::Exec(command)))
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_kuddev_pebrel_ssh_NativeSsh_read(
    mut env: JNIEnv,
    _object: JObject,
    id: jlong,
    target: JByteArray,
    offset: jint,
    count: jint,
    stderr: jboolean,
) -> jint {
    invoke(&mut env, |env| {
        bounds(env, &target, offset, count, 16 * 1024)?;
        let owner = session::get(id)?;
        let bytes = runtime().block_on(owner.read(stderr != 0, count as usize))?;
        if bytes.is_empty() {
            return Ok(-1);
        }
        let signed: Vec<i8> = bytes.iter().map(|byte| *byte as i8).collect();
        env.set_byte_array_region(&target, offset, &signed).map_err(|_| Failure("INTERNAL"))?;
        Ok(bytes.len() as jint)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_kuddev_pebrel_ssh_NativeSsh_write(
    mut env: JNIEnv,
    _object: JObject,
    id: jlong,
    source: JByteArray,
    offset: jint,
    count: jint,
) {
    invoke(&mut env, |env| {
        bounds(env, &source, offset, count, 8192)?;
        let mut bytes = vec![0i8; count as usize];
        env.get_byte_array_region(&source, offset, &mut bytes).map_err(|_| Failure("INTERNAL"))?;
        let owner = session::get(id)?;
        runtime().block_on(owner.write(bytes.into_iter().map(|byte| byte as u8).collect()))
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_kuddev_pebrel_ssh_NativeSsh_resize(
    mut env: JNIEnv,
    _object: JObject,
    id: jlong,
    columns: jint,
    rows: jint,
    width: jint,
    height: jint,
) {
    invoke(&mut env, |_| {
        let owner = session::get(id)?;
        runtime().block_on(owner.resize(geometry(columns, rows, width, height)?))
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_kuddev_pebrel_ssh_NativeSsh_awaitExit(
    mut env: JNIEnv,
    _object: JObject,
    id: jlong,
) -> jint {
    invoke(&mut env, |_| {
        let owner = session::get(id)?;
        runtime().block_on(owner.exit())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_kuddev_pebrel_ssh_NativeSsh_close(
    mut env: JNIEnv,
    _object: JObject,
    id: jlong,
) {
    invoke(&mut env, |_| {
        session::close(id);
        Ok(())
    });
}
