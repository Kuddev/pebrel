//! Resolve native Pi recovery targets in their original execution environment.
//! Reads only metadata headers; ambiguous or missing targets never select a chat.
use std::io::{BufRead as _, Read as _};
use std::path::{Path, PathBuf};

use crate::session::AgentSession;

const MAX_HEADER: u64 = 16 * 1024;
const MAX_ENTRIES: usize = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolveError {
    Missing,
    Ambiguous,
    Invalid,
    Unavailable,
}

fn native_id(saved: &str) -> Option<&str> {
    let uuid = |id: &str| {
        id.len() == 36
            && id.bytes().enumerate().all(|(i, b)| {
                if matches!(i, 8 | 13 | 18 | 23) { b == b'-' } else { b.is_ascii_hexdigit() }
            })
    };
    if uuid(saved) {
        return Some(saved);
    }
    // Compatibility for the old bridge's timestamp_uuid basename fallback.
    let (timestamp, id) = saved.rsplit_once('_')?;
    (uuid(id)
        && timestamp.contains('T')
        && timestamp.bytes().all(|b| b.is_ascii_digit() || b"TZ.:-".contains(&b)))
    .then_some(id)
}

fn header_id(header: &str) -> Option<String> {
    let header: serde_json::Value = serde_json::from_str(header).ok()?;
    (header.get("type")?.as_str()? == "session").then_some(())?;
    let id = header.get("id")?.as_str()?;
    (native_id(id) == Some(id)).then(|| id.to_owned())
}

fn read_header(path: &Path) -> Option<String> {
    let input = std::fs::File::open(path).ok()?.take(MAX_HEADER);
    let mut header = String::new();
    std::io::BufReader::new(input).read_line(&mut header).ok()?;
    if header.len() as u64 >= MAX_HEADER {
        return None;
    }
    header_id(&header)
}

fn resolve_in(root: &Path, saved: &AgentSession) -> Result<AgentSession, ResolveError> {
    let expected = saved.session_id.as_deref().and_then(native_id).ok_or(ResolveError::Invalid)?;
    if let Some(file) = &saved.session_file {
        if !crate::session::valid_native_session_file(file) {
            return Err(ResolveError::Invalid);
        }
        match std::fs::metadata(file) {
            Ok(_) => {
                let found = read_header(Path::new(file)).ok_or(ResolveError::Invalid)?;
                if found != expected {
                    return Err(ResolveError::Invalid);
                }
                return Ok(AgentSession {
                    source: "pi".into(),
                    session_id: Some(found),
                    session_file: Some(file.clone()),
                });
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => return Err(ResolveError::Unavailable),
        }
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut dirs = vec![(root.to_owned(), 0)];
    let mut matches = Vec::new();
    let mut visited = 0;
    while let Some((dir, depth)) = dirs.pop() {
        let entries = std::fs::read_dir(dir).map_err(|_| ResolveError::Unavailable)?;
        for entry in entries {
            let entry = entry.map_err(|_| ResolveError::Unavailable)?;
            visited += 1;
            if visited > MAX_ENTRIES || std::time::Instant::now() >= deadline {
                return Err(ResolveError::Unavailable);
            }
            let kind = entry.file_type().map_err(|_| ResolveError::Unavailable)?;
            if kind.is_dir() && depth == 0 {
                dirs.push((entry.path(), depth + 1));
            }
            if !kind.is_file()
                || entry.path().extension().is_none_or(|extension| extension != "jsonl")
            {
                continue;
            }
            if read_header(&entry.path()).as_deref() == Some(expected) {
                matches.push(entry.path());
                if matches.len() > 1 {
                    return Err(ResolveError::Ambiguous);
                }
            }
        }
    }
    let file = matches.pop().ok_or(ResolveError::Missing)?;
    Ok(AgentSession {
        source: "pi".into(),
        session_id: Some(expected.to_owned()),
        session_file: Some(file.to_str().ok_or(ResolveError::Invalid)?.to_owned()),
    })
}

pub(crate) fn resolve(
    saved: &AgentSession,
    context: Option<&crate::runtime_exec::PaneExecContext>,
    cwd: &str,
) -> Result<AgentSession, ResolveError> {
    if let Some(context) = context.filter(|context| context.wsl_distribution().is_some()) {
        return resolve_guest(saved, context, cwd);
    }
    if let Some(file) = &saved.session_file
        && Path::new(file).is_file()
    {
        return resolve_in(Path::new(""), saved);
    }
    let home = super::dirs::home_dir().ok_or(ResolveError::Unavailable)?;
    let cwd = Path::new(cwd);
    let config = std::env::var_os("PI_CODING_AGENT_DIR")
        .map(|path| expand_directory(Path::new(&path), cwd, &home))
        .transpose()?
        .unwrap_or_else(|| home.join(".pi/agent"));
    let configured = match std::env::var_os("PI_CODING_AGENT_SESSION_DIR") {
        Some(path) => Some(PathBuf::from(path)),
        None => match read_session_directory(&cwd.join(".pi/settings.json"))? {
            Some(directory) => Some(directory),
            None => read_session_directory(&config.join("settings.json"))?,
        },
    };
    let root = configured
        .map(|path| expand_directory(&path, cwd, &home))
        .transpose()?
        .unwrap_or_else(|| config.join("sessions"));
    resolve_in(&root, saved)
}

fn expand_directory(path: &Path, cwd: &Path, home: &Path) -> Result<PathBuf, ResolveError> {
    let text = path.to_str().ok_or(ResolveError::Invalid)?;
    if text.is_empty() || text.chars().any(char::is_control) {
        return Err(ResolveError::Invalid);
    }
    if text == "~" {
        return Ok(home.to_owned());
    }
    if let Some(suffix) = text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\")) {
        return Ok(home.join(suffix));
    }
    if path.is_absolute() {
        Ok(path.to_owned())
    } else if cwd.is_absolute() {
        Ok(cwd.join(path))
    } else {
        Err(ResolveError::Unavailable)
    }
}

fn read_session_directory(path: &Path) -> Result<Option<PathBuf>, ResolveError> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ResolveError::Unavailable),
    };
    let settings: serde_json::Value =
        serde_json::from_reader(file.take(64 * 1024)).map_err(|_| ResolveError::Invalid)?;
    match settings.get("sessionDir") {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(path)) => Ok(Some(PathBuf::from(path))),
        _ => Err(ResolveError::Invalid),
    }
}

fn resolve_guest(
    saved: &AgentSession,
    context: &crate::runtime_exec::PaneExecContext,
    cwd: &str,
) -> Result<AgentSession, ResolveError> {
    let expected = saved.session_id.as_deref().and_then(native_id).ok_or(ResolveError::Invalid)?;
    let file = saved.session_file.as_deref().unwrap_or("");
    if !file.is_empty() && !crate::session::valid_native_session_file(file) {
        return Err(ResolveError::Invalid);
    }
    let result = crate::runtime_exec::execute(
        context.clone(),
        cwd.to_owned(),
        ["sh", "-c", include_str!("pi_session.sh"), "pebrel-session-restore", expected, file]
            .map(str::to_owned)
            .to_vec(),
        3000,
        64 * 1024,
    )
    .map_err(|_| ResolveError::Unavailable)?;
    if result["success"] != true || result["capture"]["stdout"]["truncated"] != false {
        return Err(ResolveError::Unavailable);
    }
    let output = result["stdout"].as_str().ok_or(ResolveError::Unavailable)?;
    let mut candidate = None;
    for line in output.lines() {
        let (path, header) = line.split_once('\t').ok_or(ResolveError::Invalid)?;
        if !crate::session::valid_native_session_file(path) {
            return Err(ResolveError::Invalid);
        }
        if header_id(header).as_deref() != Some(expected) {
            continue;
        }
        if candidate.is_some() {
            return Err(ResolveError::Ambiguous);
        }
        candidate = Some(AgentSession {
            source: "pi".into(),
            session_id: Some(expected.into()),
            session_file: Some(path.into()),
        });
    }
    candidate.ok_or(ResolveError::Missing)
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "01a09dda-29d9-729f-8eb0-d0a20d866fc5";

    #[test]
    fn exact_native_metadata_required_and_ambiguity_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(format!("timestamp_{ID}.jsonl"));
        let saved =
            AgentSession { source: "pi".into(), session_id: Some(ID.into()), session_file: None };
        assert_eq!(resolve_in(dir.path(), &saved), Err(ResolveError::Missing));
        let header = format!("{{\"type\":\"session\",\"id\":\"{ID}\"}}\n");
        std::fs::write(&file, &header).unwrap();
        let found = resolve_in(dir.path(), &saved).unwrap();
        assert_eq!(found.session_file.as_deref(), file.to_str());
        std::fs::write(dir.path().join(format!("duplicate_{ID}.jsonl")), &header).unwrap();
        assert_eq!(resolve_in(dir.path(), &saved), Err(ResolveError::Ambiguous));
        // An exact path disambiguates two files bearing the same native ID.
        assert!(resolve_in(dir.path(), &found).is_ok());
        std::fs::write(file, "{\"type\":\"session\",\"id\":\"different\"}\n").unwrap();
        assert_eq!(resolve_in(dir.path(), &found), Err(ResolveError::Invalid));
    }

    #[test]
    fn moved_file_is_found_by_native_header_in_custom_directory() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("custom-history-name.jsonl");
        std::fs::write(&file, format!("{{\"type\":\"session\",\"id\":\"{ID}\"}}\n")).unwrap();
        let saved = AgentSession {
            source: "pi".into(),
            session_id: Some(ID.into()),
            session_file: Some(dir.path().join("old-location.jsonl").to_str().unwrap().into()),
        };
        let found = resolve_in(dir.path(), &saved).unwrap();
        assert_eq!(found.session_file.as_deref(), file.to_str());
        assert_eq!(found.session_id, saved.session_id);
    }

    #[test]
    fn custom_roots_expand_relative_paths_and_home_without_changing_process_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().join("project");
        let home = dir.path().join("user");
        assert_eq!(
            expand_directory(Path::new(".pi/sessions"), &cwd, &home).unwrap(),
            cwd.join(".pi/sessions")
        );
        assert_eq!(
            expand_directory(Path::new("~/sessions"), &cwd, &home).unwrap(),
            home.join("sessions")
        );
        assert_eq!(expand_directory(Path::new(""), &cwd, &home), Err(ResolveError::Invalid));
    }

    #[test]
    fn legacy_timestamp_identity_is_normalized_but_process_badges_are_rejected() {
        assert_eq!(native_id(&format!("2026-09-15T10-22-03-123Z_{ID}")), Some(ID));
        assert!(native_id("pid-1234").is_none());
        assert!(native_id(&format!("arbitrary_{ID}")).is_none());
    }
}
