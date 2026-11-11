//! Streaming filename enumeration. Only one application search walks at a time.

use super::*;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::mpsc;

pub(super) const VISIT_LIMIT: usize = 500_000;
const DEPTH_LIMIT: usize = 64;

#[derive(Default)]
pub(super) struct WalkOutcome {
    pub visited: usize,
    pub limited: bool,
    pub error: Option<String>,
}

pub(super) fn local(
    root: &Path,
    cancelled: impl Fn() -> bool,
    mut directory: impl FnMut(&Path),
    mut visit: impl FnMut(IndexedPath),
) -> WalkOutcome {
    let mut outcome = WalkOutcome::default();
    directory(root);
    let read = match std::fs::read_dir(root) {
        Ok(read) => read,
        Err(error) => {
            outcome.error = Some(error.to_string());
            return outcome;
        },
    };
    // Keep directory iterators, not every pending descendant path. A wide
    // node_modules or build tree therefore cannot fill an unbounded BFS queue.
    let mut stack = vec![read];
    while let Some(read) = stack.last_mut() {
        if cancelled() {
            break;
        }
        let Some(next) = read.next() else {
            stack.pop();
            continue;
        };
        let child = match next {
            Ok(child) => child,
            Err(_) => {
                outcome.limited = true;
                continue;
            },
        };
        if outcome.visited >= VISIT_LIMIT {
            outcome.limited = true;
            break;
        }
        let Ok(kind) = child.file_type() else {
            outcome.limited = true;
            continue;
        };
        if kind.is_symlink() || child.file_name() == ".git" {
            continue;
        }
        let path = child.path();
        if path.as_os_str().as_encoded_bytes().len() > memory::PATH_LIMIT {
            outcome.limited = true;
            continue;
        }
        if let Some(entry) = indexed_local_path(root, &path, kind.is_dir()) {
            outcome.visited += 1;
            visit(entry);
        }
        if kind.is_dir() {
            if stack.len() >= DEPTH_LIMIT {
                outcome.limited = true;
                continue;
            }
            directory(&path);
            match std::fs::read_dir(path) {
                Ok(read) => stack.push(read),
                Err(_) => outcome.limited = true,
            }
        }
    }
    outcome
}

fn read_field(reader: &mut impl BufRead) -> std::io::Result<Option<Vec<u8>>> {
    let mut field = Vec::new();
    let count = reader.take(memory::PATH_LIMIT as u64 + 1).read_until(0, &mut field)?;
    if count == 0 {
        return Ok(None);
    }
    if field.last() != Some(&0) || count > memory::PATH_LIMIT {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "File search received an incomplete or oversized path",
        ));
    }
    field.pop();
    Ok(Some(field))
}

pub(super) fn wsl(
    located: &crate::shell_detect::WslCwd,
    cancelled: impl Fn() -> bool,
    mut visit: impl FnMut(IndexedPath),
) -> WalkOutcome {
    let mut outcome = WalkOutcome::default();
    let root = normalize_wsl_guest_path(&located.guest);
    let mut command = Command::new("wsl.exe");
    // `--` still goes through the distro's default shell, which interprets
    // find's parentheses and path metacharacters. Direct exec also preserves
    // the single backslash required by GNU find's NUL format.
    command.args(["-d", &located.distro, "--exec", "find", &root]);
    command.args([
        "-mindepth",
        "1",
        "-maxdepth",
        "64",
        "(",
        "-type",
        "d",
        "-name",
        ".git",
        "-prune",
        ")",
        "-o",
        "(",
        "!",
        "-type",
        "l",
        "-printf",
        r"%y\0%p\0",
        ")",
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            outcome.error = Some(error.to_string());
            return outcome;
        },
    };
    let stdout = child.stdout.take().expect("piped search stdout");
    let stderr = child.stderr.take().expect("piped search stderr");
    let (sender, receiver) = mpsc::sync_channel::<std::io::Result<(u8, Vec<u8>)>>(4);
    let reader = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let pair = (|| {
                let Some(kind) = read_field(&mut reader)? else { return Ok(None) };
                let path = read_field(&mut reader)?.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Incomplete find record")
                })?;
                Ok(Some((kind.first().copied().unwrap_or_default(), path)))
            })();
            match pair {
                Ok(None) => break,
                Ok(Some(pair)) => {
                    if sender.send(Ok(pair)).is_err() {
                        break;
                    }
                },
                Err(error) => {
                    let _ = sender.send(Err(error));
                    break;
                },
            }
        }
    });
    // Retain a small diagnostic prefix, then drain the rest without buffering.
    let errors = std::thread::spawn(move || {
        let mut stderr = stderr;
        let mut buffer = [0; 4096];
        let mut diagnostic = Vec::new();
        while let Ok(count) = stderr.read(&mut buffer) {
            if count == 0 {
                break;
            }
            let keep = count.min(2048usize.saturating_sub(diagnostic.len()));
            diagnostic.extend_from_slice(&buffer[..keep]);
        }
        String::from_utf8_lossy(&diagnostic).trim().to_owned()
    });
    let deadline = Instant::now() + WSL_COMMAND_TIMEOUT;
    let mut abort = false;
    loop {
        if cancelled() || outcome.visited >= VISIT_LIMIT || Instant::now() >= deadline {
            outcome.limited = !cancelled();
            abort = true;
            break;
        }
        match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(Ok((kind, bytes))) => {
                let guest_path = String::from_utf8_lossy(&bytes).into_owned();
                let Some(relative) = guest_path.strip_prefix(&root) else { continue };
                if root != "/" && !relative.starts_with('/') {
                    continue;
                }
                let key = relative.trim_start_matches('/').to_owned();
                if kind == b'd' && key.split('/').count() >= DEPTH_LIMIT {
                    outcome.limited = true;
                }
                let name = key.rsplit('/').next().unwrap_or(&key).to_owned();
                let entry = IndexedPath {
                    path: PathBuf::from(&guest_path),
                    guest_path: Some(guest_path),
                    name_folded: name.to_lowercase(),
                    key_folded: key.to_lowercase(),
                    name,
                    key,
                    is_dir: kind == b'd',
                };
                outcome.visited += 1;
                visit(entry);
            },
            Ok(Err(error)) => {
                outcome.error = Some(error.to_string());
                abort = true;
                break;
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {},
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    if abort {
        let _ = child.kill();
    }
    drop(receiver);
    let status = child.wait();
    let _ = reader.join();
    let diagnostic = errors.join().unwrap_or_default();
    if !abort && !matches!(status, Ok(status) if status.success()) {
        if outcome.visited == 0 {
            outcome.error = Some(if diagnostic.is_empty() {
                "Unable to read the WSL search directory".to_owned()
            } else {
                diagnostic
            });
        } else {
            outcome.limited = true;
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nul_reader_bounds_long_or_incomplete_paths_without_losing_newlines() {
        let mut valid = std::io::Cursor::new(b"name\nwith newline\0");
        assert_eq!(read_field(&mut valid).unwrap(), Some(b"name\nwith newline".to_vec()));
        assert_eq!(read_field(&mut valid).unwrap(), None);
        let mut incomplete = std::io::Cursor::new(b"incomplete");
        assert!(read_field(&mut incomplete).is_err());
        let mut huge = std::io::Cursor::new(vec![b'x'; memory::PATH_LIMIT * 2]);
        assert!(read_field(&mut huge).is_err());
        assert_eq!(huge.position(), memory::PATH_LIMIT as u64 + 1);
    }
}

#[cfg(windows)]
#[test]
#[ignore = "Requires Debian WSL; searches only an owned temporary directory"]
fn wsl_search_streams_and_cancels_its_owned_find() {
    let owner = tempfile::tempdir().unwrap();
    let fixture = owner.path().join("中文 space & (literal)");
    std::fs::create_dir(&fixture).unwrap();
    for number in 0..256 {
        std::fs::write(fixture.join(format!("needle-中文-{number}.txt")), b"").unwrap();
    }
    let windows = fixture.to_string_lossy().replace('\\', "/");
    let (drive, rest) = windows.split_once(":/").expect("Windows drive-backed fixture");
    let located = crate::shell_detect::WslCwd {
        distro: "Debian".to_owned(),
        guest: format!("/mnt/{}/{}", drive.to_ascii_lowercase(), rest),
    };
    let mut count = 0;
    let complete = wsl(
        &located,
        || false,
        |entry| {
            assert!(entry.name.starts_with("needle-中文-"));
            assert!(entry.guest_path.as_ref().unwrap().starts_with(&located.guest));
            count += 1;
        },
    );
    assert!(complete.error.is_none(), "{:?}", complete.error);
    assert!(!complete.limited);
    assert_eq!(count, 256);
    let visits = std::cell::Cell::new(0);
    let cancelled = wsl(&located, || visits.get() > 0, |_| visits.set(visits.get() + 1));
    assert_eq!(cancelled.visited, 1);
    assert_eq!(visits.get(), 1, "cancellation must stop enumeration before the next record");
}
