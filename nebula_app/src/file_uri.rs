//! Translate `file:` URIs and filesystem links into native paths across platforms.
//!
//! The shells Nebula integrates with emit clickable entries as OSC 8 hyperlinks or
//! terminal output paths:
//!
//! - **Windows**:
//!   - PowerShell (`Nebula-List`): `file:///C:/Users/me/a%20b.txt` — empty host,
//!     real Windows drive path, percent-encoded.
//!   - Raw paths: `D:/work/project/spec.md`, `D:\work\...`, `/D:/...`
//!   - Quoted paths: `"C:\Program Files\App\app.exe"`, `'D:\My Documents\notes.txt'`
//!   - Git-bash / MSYS (`ls --hyperlink`): `file://HOST/d/temp/x` — MSYS drive form.
//!   - WSL (`ls --hyperlink`): `file://HOST/mnt/c/x` or `/cygdrive/c/x`.
//!   - UNC shares: `\\server\share\file.txt` or `file://fileserver/share/...`.
//! - **macOS / Linux**:
//!   - Standard file URIs: `file:///Users/me/a%20b.txt`, `file:///home/me/...`
//!   - Home-relative paths: `~/docs/spec.md`, `~`
//! - **Security & Boundaries**:
//!   - Naked POSIX paths without `file:` or `~` (e.g. `/etc/passwd`, `/usr/bin/gcc`)
//!     are strictly NOT opened as local files to prevent accidental system access.
//!   - Relative paths (`./README.md`) are rejected with user-visible guidance when
//!     pane working directory context is unavailable.
//! - **Sentence punctuation**: links emitted at sentence ends often capture
//!   trailing punctuation (e.g. `[link](path)。` or `[link](path))`).
//!
//! When launching:
//! - Dispatches via [`crate::platform::file_manager::open`].
//!   - On Windows: launches default associated application or Explorer.
//!   - On macOS: `open <path>`.
//!   - On Linux: `xdg-open <path>`.
//!   - Falls back to revealing in the system file manager ([`crate::platform::file_manager::reveal`]) on open failure.

use std::path::{Path, PathBuf};

use crate::platform::{Platform, local_paths};

/// Decode a file URI or local path into a native filesystem PathBuf.
///
/// Returns `None` when the input is not a local file path or recognized `file:`
/// URI (allowing the caller to fall back to the default web browser handler).
pub fn file_uri_to_local_path(uri: &str) -> Option<PathBuf> {
    file_uri_to_local_path_with(
        uri,
        local_paths::drive_exists,
        home::home_dir,
        Platform::current() == Platform::Windows,
    )
}

/// Open a local path (file or directory) using the platform's default handler.
///
/// Dispatches via [`crate::platform::file_manager::open`]. If opening directly
/// fails to launch its platform helper, falls back to revealing
/// the item in the system file manager ([`crate::platform::file_manager::reveal`]).
/// Success means the helper process was launched, not that an associated app
/// finished opening the file. Later helper failures are not reported by this API.
pub fn open_local_path(path: &Path) -> std::io::Result<()> {
    match crate::platform::file_manager::open(path) {
        Ok(()) => {
            log::debug!("open_local_path helper launched for {}", path.display());
            Ok(())
        },
        Err(open_err) => {
            log::debug!(
                "open_local_path open failed for {}: {open_err}; falling back to reveal",
                path.display()
            );
            match crate::platform::file_manager::reveal(path) {
                Ok(()) => {
                    log::debug!("open_local_path fallback reveal succeeded for {}", path.display());
                    Ok(())
                },
                Err(reveal_err) => {
                    log::debug!(
                        "open_local_path fallback reveal also failed for {}: {reveal_err}",
                        path.display()
                    );
                    Err(open_err)
                },
            }
        },
    }
}

/// Detailed error outcomes for local link opening attempts.
#[derive(Debug, PartialEq, Eq)]
pub enum LocalLinkError {
    MissingCwd(String),
    FileNotFound(PathBuf),
    OpenFailed(String),
}

impl LocalLinkError {
    pub fn localized_message(&self, language: crate::i18n::UiLanguage) -> String {
        match self {
            Self::MissingCwd(rel) => {
                language.format(crate::i18n::Message::CommonLinkMissingCwd, &[("path", rel)])
            },
            Self::FileNotFound(path) => language.format(
                crate::i18n::Message::CommonLinkFileNotFound,
                &[("path", &path.display().to_string())],
            ),
            Self::OpenFailed(err) => {
                language.format(crate::i18n::Message::CommonLinkOpenFailed, &[("error", err)])
            },
        }
    }
}

/// Classification of a potential link target extracted from the terminal or Markdown.
#[derive(Debug, PartialEq, Eq)]
pub enum LinkTargetKind {
    /// A local file/directory path that actually exists on the filesystem.
    LocalExisting(PathBuf),
    /// A local file/directory path that does NOT exist on the filesystem.
    LocalMissing(PathBuf),
    /// A relative file path (e.g. `./README.md` or `../docs/spec.md`), which cannot be safely resolved without pane cwd.
    RelativePath(String),
    /// A recognized web or protocol URI (e.g. `https://...`, `mailto:...`).
    ProtocolUri(String),
    /// Unrecognized format.
    Unrecognized(String),
}

/// Classify a link target to enforce strict gatekeeping before opening.
#[allow(dead_code)]
pub fn classify_link_target(raw: &str) -> LinkTargetKind {
    classify_link_target_with_cwd(raw, None)
}

/// Classify a link target with an optional working directory for relative path resolution.
pub fn classify_link_target_with_cwd(raw: &str, cwd: Option<&Path>) -> LinkTargetKind {
    let target = extract_link_target(raw);
    if target.is_empty() || target.starts_with('#') {
        return LinkTargetKind::Unrecognized(raw.to_string());
    }

    // 1. Try to resolve as a local absolute, UNC, home-relative path, or explicit file: URI
    if let Some(path) = file_uri_to_local_path(target) {
        // UNC paths (\\server\share\...): skip synchronous exists() check to prevent blocking UI thread.
        let is_unc = path.to_str().map_or(false, |s| s.starts_with(r"\\"));
        if is_unc || path.exists() {
            return LinkTargetKind::LocalExisting(path);
        } else {
            return LinkTargetKind::LocalMissing(path);
        }
    }

    // 2. Check if it's an explicit relative path (e.g. ./x, ../x)
    if is_explicit_relative_path(target) {
        if let Some(base) = cwd {
            let clean = clean_relative_prefix(target);
            let resolved = base.join(clean);
            if resolved.exists() {
                return LinkTargetKind::LocalExisting(resolved);
            } else {
                return LinkTargetKind::LocalMissing(resolved);
            }
        }
        return LinkTargetKind::RelativePath(target.to_string());
    }

    // 3. Check if it's a recognized web/network protocol URI
    if is_web_or_protocol_uri(target) {
        return LinkTargetKind::ProtocolUri(target.to_string());
    }

    // 4. Relative paths without explicit ./ prefix (e.g. docs/spec.md, README.md)
    if !target.starts_with('/')
        && !target.starts_with('\\')
        && !is_drive_prefixed(target)
        && !has_scheme_prefix(target)
    {
        if let Some(base) = cwd {
            let resolved = base.join(target);
            if resolved.exists() {
                return LinkTargetKind::LocalExisting(resolved);
            } else {
                return LinkTargetKind::LocalMissing(resolved);
            }
        } else {
            return LinkTargetKind::RelativePath(target.to_string());
        }
    }

    LinkTargetKind::Unrecognized(target.to_string())
}

/// Whether the string starts with a recognized network or protocol URI scheme.
pub fn is_web_or_protocol_uri(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    // file: URIs are local filesystem targets, handled by file_uri_to_local_path, not protocol URIs.
    if lower.starts_with("file:") {
        return false;
    }
    if lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:")
        || lower.starts_with("gemini://")
        || lower.starts_with("gopher://")
        || lower.starts_with("news:")
        || lower.starts_with("git://")
        || lower.starts_with("ssh://")
        || lower.starts_with("ssh:")
        || lower.starts_with("ftp://")
        || lower.starts_with("ipfs:")
        || lower.starts_with("ipns:")
        || lower.starts_with("magnet:")
        || lower.starts_with("tel:")
        || lower.starts_with("sms:")
    {
        return true;
    }
    // Generic scheme URI check (e.g. vscode://, obsidian://, idea://, etc.):
    // Scheme must start with an ASCII letter, followed by letters/digits/+/-/. (at least 2 chars total), then "://"
    if let Some(colon_pos) = s.find("://") {
        let scheme = &s[..colon_pos];
        if scheme.len() >= 2
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
        {
            return true;
        }
    }
    false
}

fn is_explicit_relative_path(s: &str) -> bool {
    s.starts_with("./") || s.starts_with(r".\") || s.starts_with("../") || s.starts_with(r"..\")
}

fn clean_relative_prefix(s: &str) -> &str {
    if let Some(tail) = s.strip_prefix("./").or_else(|| s.strip_prefix(r".\")) { tail } else { s }
}

fn has_scheme_prefix(s: &str) -> bool {
    if let Some(colon) = s.find(':') {
        let scheme = &s[..colon];
        return scheme.len() >= 2
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.');
    }
    false
}

/// Try to extract and open an existing local file/directory path from raw text.
#[allow(dead_code)]
pub fn try_open_local_link(text: &str) -> Option<Result<(), LocalLinkError>> {
    try_open_local_link_with_cwd(text, None)
}

/// Try to extract and open an existing local file/directory path from raw text, resolving
/// relative paths against the provided pane cwd if present.
pub fn try_open_local_link_with_cwd(
    text: &str,
    cwd: Option<&Path>,
) -> Option<Result<(), LocalLinkError>> {
    match classify_link_target_with_cwd(text, cwd) {
        LinkTargetKind::LocalExisting(path) => {
            Some(open_local_path(&path).map_err(|e| LocalLinkError::OpenFailed(e.to_string())))
        },
        LinkTargetKind::LocalMissing(path) => Some(Err(LocalLinkError::FileNotFound(path))),
        LinkTargetKind::RelativePath(rel) => Some(Err(LocalLinkError::MissingCwd(rel))),
        LinkTargetKind::ProtocolUri(_) | LinkTargetKind::Unrecognized(_) => None,
    }
}

/// Outcome of handling a hint command in legacy event contexts.
#[cfg(any(test, feature = "legacy-shell"))]
#[derive(Debug, PartialEq, Eq)]
pub enum LegacyHintOutcome {
    Handled,
    SpawnCommand(Vec<std::ffi::OsString>),
    Failed(String),
}

/// Dispatch a hint command string in legacy event contexts with user-visible notification on failure.
#[cfg(any(test, feature = "legacy-shell"))]
pub fn handle_legacy_hint_command(
    text: &str,
    default_args: &[String],
    language: crate::i18n::UiLanguage,
) -> LegacyHintOutcome {
    let target = extract_link_target(text);
    if let Some(result) = try_open_local_link(target) {
        match result {
            Ok(()) => {
                log::debug!("trigger_hint opened local link from {target:?}");
                LegacyHintOutcome::Handled
            },
            Err(err) => {
                let msg = err.localized_message(language);
                log::debug!("trigger_hint local link failed for {target:?}: {msg}");
                LegacyHintOutcome::Failed(msg)
            },
        }
    } else if is_web_or_protocol_uri(target) {
        let mut args: Vec<std::ffi::OsString> = default_args.iter().map(|s| s.into()).collect();
        args.push(target.into());
        LegacyHintOutcome::SpawnCommand(args)
    } else {
        let msg =
            language.format(crate::i18n::Message::CommonLinkUnrecognized, &[("target", target)]);
        log::debug!("trigger_hint ignored non-protocol target: {target:?}");
        LegacyHintOutcome::Failed(msg)
    }
}

/// Extract the link target from potential Markdown link syntax `[title](target)`.
///
/// For Markdown links, extracts the `target` URL or path, strips surrounding
/// `<...>` or quotes, and trims any trailing sentence punctuation.
///
/// For naked URLs or paths (e.g. `https://en.wikipedia.org/wiki/Rust_(programming_language)`),
/// leaves the target untouched without stripping legal characters like closing parentheses.
pub fn extract_link_target(raw: &str) -> &str {
    let trimmed = raw.trim();
    let unpunct = trim_outer_punctuation(trimmed);
    if unpunct.starts_with('[') {
        if let Some(open_paren) = unpunct.rfind("](") {
            let inside = &unpunct[open_paren + 2..];
            if let Some(target) = inside.strip_suffix(')') {
                let is_quote_or_bracket = |c: char| {
                    matches!(c, '<' | '>' | '"' | '\'' | '‘' | '’' | '“' | '”' | '《' | '》')
                };
                let clean = target.trim().trim_matches(is_quote_or_bracket);
                return trim_target_punctuation(clean);
            }
        }
    }
    // If wrapped in angle brackets `<url>`, strip them.
    if unpunct.starts_with('<') && unpunct.ends_with('>') && unpunct.len() >= 2 {
        let inside = &unpunct[1..unpunct.len() - 1];
        return trim_target_punctuation(inside);
    }
    trim_target_punctuation(trimmed)
}

/// Strip sentence-ending punctuation from outer strings while preserving structural parentheses/brackets.
fn trim_outer_punctuation(s: &str) -> &str {
    s.trim_end_matches(|c: char| {
        matches!(
            c,
            '。' | '，'
                | '、'
                | '；'
                | '！'
                | '？'
                | '）'
                | '】'
                | '》'
                | '”'
                | '’'
                | ';'
                | ','
                | '.'
                | ':'
        )
    })
}

/// Strip trailing sentence punctuation while strictly preserving legal closing parentheses
/// in URLs when parentheses are balanced (e.g. `https://en.wikipedia.org/wiki/Rust_(programming_language)`).
fn trim_target_punctuation(s: &str) -> &str {
    let mut end = s.len();
    while end > 0 {
        let c = s[..end].chars().next_back().unwrap();
        // Always strip trailing CJK sentence delimiters
        if matches!(c, '。' | '，' | '、' | '；' | '！' | '？' | '）' | '】' | '》' | '”' | '’')
        {
            end -= c.len_utf8();
            continue;
        }
        if matches!(c, ';' | ',') {
            end -= 1;
            continue;
        }
        // For closing parenthesis, only strip if unmatched (e.g. `(http://...)`)
        if c == ')' {
            let open_count = s[..end].chars().filter(|&ch| ch == '(').count();
            let close_count = s[..end].chars().filter(|&ch| ch == ')').count();
            if close_count > open_count {
                end -= 1;
                continue;
            }
        }
        break;
    }
    &s[..end]
}

/// Test seam for [`file_uri_to_local_path`]; drives and home directory are injected
/// so unit tests can verify both Windows and Unix behavior deterministically.
fn file_uri_to_local_path_with(
    uri: &str,
    drive_exists: impl Fn(char) -> bool,
    home_dir: impl Fn() -> Option<PathBuf>,
    is_windows: bool,
) -> Option<PathBuf> {
    let trimmed = extract_link_target(uri);
    if trimmed.is_empty() {
        return None;
    }

    // 1. Direct home-relative path: "~" or "~/..." or (on Windows) "~\..."
    if trimmed == "~" {
        return home_dir();
    }
    if let Some(tail) = trimmed.strip_prefix("~/").or_else(|| trimmed.strip_prefix(r"~\")) {
        let home = home_dir()?;
        return Some(join_home(home, tail, is_windows));
    }

    // 2. Direct Windows drive path without scheme, e.g.:
    // "D:/work/project/file.md" or "D:\work\project\file.md"
    if is_drive_prefixed(trimmed) {
        return if is_windows { Some(to_windows(trimmed)) } else { Some(PathBuf::from(trimmed)) };
    }

    // 3. Windows drive path with a stray leading slash, e.g. "/D:/work/..."
    if trimmed.starts_with('/') && is_drive_prefixed(&trimmed[1..]) {
        let rest = &trimmed[1..];
        return if is_windows { Some(to_windows(rest)) } else { Some(PathBuf::from(rest)) };
    }

    // 4. Direct UNC path on Windows, e.g. "\\server\share\file.txt"
    if is_windows
        && trimmed.starts_with(r"\\")
        && !trimmed.to_ascii_lowercase().starts_with("file:")
    {
        return Some(PathBuf::from(trimmed.replace('/', "\\")));
    }

    // 5. Scheme-prefixed `file:` URI
    if let Some(rest) = strip_scheme(trimmed) {
        if let Some(after_slashes) = rest.strip_prefix("//") {
            // 5a. Home-relative URI: "file://~/..." or "file:///~/..."
            if let Some(tail) =
                after_slashes.strip_prefix("~/").or_else(|| after_slashes.strip_prefix("/~/"))
            {
                let home = home_dir()?;
                let decoded = percent_decode_utf8(tail);
                return Some(join_home(home, &decoded, is_windows));
            }

            // 5b. Two-slash drive path: "file://C:/..." or "file://C:\..."
            if is_drive_prefixed(after_slashes) {
                let path = percent_decode_utf8(after_slashes);
                return if is_windows {
                    Some(to_windows(&path))
                } else {
                    Some(PathBuf::from(path))
                };
            }

            // 5c. Split `HOST/PATH`
            if let Some(slash) = after_slashes.find('/') {
                let host = &after_slashes[..slash];
                let path = percent_decode_utf8(&after_slashes[slash..]);

                // Host that is actually a drive letter (e.g. "C:" from "file://C:/...")
                if is_drive_prefixed(host) {
                    let full = format!("{host}{path}");
                    return if is_windows {
                        Some(to_windows(&full))
                    } else {
                        Some(PathBuf::from(full))
                    };
                }

                if host_is_local(host) {
                    return translate_local_path_impl(&path, &drive_exists, &home_dir, is_windows);
                } else if is_windows {
                    // Remote host → UNC share on Windows (`\\HOST\path`)
                    let host = percent_decode_utf8(host);
                    let tail = path.trim_start_matches('/').replace('/', "\\");
                    return (!host.is_empty())
                        .then(|| PathBuf::from(format!("\\\\{host}\\{tail}")));
                } else {
                    return None;
                }
            } else {
                return None;
            }
        } else if rest.starts_with('/') {
            // Single slash: "file:/home/user/..." or "file:/C:/..."
            let decoded = percent_decode_utf8(rest);
            return translate_local_path_impl(&decoded, &drive_exists, &home_dir, is_windows);
        } else if is_drive_prefixed(rest) {
            // "file:C:/..."
            let decoded = percent_decode_utf8(rest);
            return if is_windows {
                Some(to_windows(&decoded))
            } else {
                Some(PathBuf::from(decoded))
            };
        }
    }

    // Naked POSIX paths (e.g. /etc/passwd or /usr/bin/gcc) are strictly NOT opened
    // as local files unless prefixed with file: or ~ to prevent accidental access.
    None
}

/// Strip a case-insensitive `file:` scheme prefix.
fn strip_scheme(uri: &str) -> Option<&str> {
    let bytes = uri.as_bytes();
    (bytes.len() >= 5 && bytes[..5].eq_ignore_ascii_case(b"file:")).then(|| &uri[5..])
}

/// Whether a URI host names this machine (so the path is local, not a share).
fn host_is_local(host: &str) -> bool {
    if host.is_empty() || host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if host == "127.0.0.1" || host == "::1" || host == "[::1]" {
        return true;
    }
    local_paths::matches_hostname(host)
}

/// Convert a decoded, local, posix-looking URI path into a platform-appropriate PathBuf.
fn translate_local_path_impl(
    path: &str,
    drive_exists: &impl Fn(char) -> bool,
    home_dir: &impl Fn() -> Option<PathBuf>,
    is_windows: bool,
) -> Option<PathBuf> {
    if let Some(tail) = path.strip_prefix("/~/").or_else(|| path.strip_prefix("~/")) {
        let home = home_dir()?;
        return Some(join_home(home, tail, is_windows));
    }

    if let Some(without_slash) = path.strip_prefix('/') {
        if is_drive_prefixed(without_slash) {
            return if is_windows {
                Some(to_windows(without_slash))
            } else {
                Some(PathBuf::from(without_slash))
            };
        }
    } else if is_drive_prefixed(path) {
        return if is_windows { Some(to_windows(path)) } else { Some(PathBuf::from(path)) };
    }

    let trimmed = path.trim_start_matches('/');

    if is_windows {
        // `/mnt/c/…` (WSL) or `/cygdrive/c/…` (Cygwin/some MSYS builds).
        for mount in ["mnt/", "cygdrive/"] {
            if let Some(after) = trimmed.strip_prefix(mount) {
                if let Some(win) = mount_drive_path(after) {
                    return Some(win);
                }
            }
        }

        // `/c/temp/…` — MSYS drive form. Only when the drive really exists.
        let mut segments = trimmed.splitn(2, '/');
        if let Some(first) = segments.next() {
            if let Some(drive) = single_drive_letter(first) {
                if drive_exists(drive) {
                    let tail = segments.next().unwrap_or("");
                    return Some(to_windows(&format!("{drive}:/{tail}")));
                }
            }
        }

        // Pure POSIX path with no drive mapping isn't reachable on Windows.
        None
    } else {
        // On Unix platforms (macOS, Linux), any leading slash indicates a valid absolute path.
        Some(PathBuf::from(format!("/{}", trimmed)))
    }
}

/// Build a Windows path from a `<drive>/<rest>` mount tail (`c/x` → `C:\x`).
fn mount_drive_path(after: &str) -> Option<PathBuf> {
    let mut parts = after.splitn(2, '/');
    let drive = single_drive_letter(parts.next()?)?;
    let tail = parts.next().unwrap_or("");
    Some(to_windows(&format!("{drive}:/{tail}")))
}

/// `true` for a `X:` / `X:/…` drive-letter prefix.
fn is_drive_prefixed(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// The uppercase drive letter if `s` is exactly one ascii letter, else `None`.
fn single_drive_letter(s: &str) -> Option<char> {
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => Some(c.to_ascii_uppercase()),
        _ => None,
    }
}

/// Join a relative path to the home directory with platform-appropriate separators.
fn join_home(home: PathBuf, sub: &str, is_windows: bool) -> PathBuf {
    let clean = sub.trim_start_matches(|c| c == '/' || c == '\\');
    if is_windows {
        let home_str = home.to_string_lossy();
        let home_trimmed = home_str.trim_end_matches(|c| c == '/' || c == '\\');
        let sub_win = clean.replace('/', "\\");
        PathBuf::from(format!("{home_trimmed}\\{sub_win}"))
    } else {
        let home_str = home.to_string_lossy();
        let home_trimmed = home_str.trim_end_matches('/');
        let sub_unix = clean.replace('\\', "/");
        PathBuf::from(format!("{home_trimmed}/{sub_unix}"))
    }
}

/// Normalize forward slashes to backslashes and uppercase the drive letter.
fn to_windows(path: &str) -> PathBuf {
    let mut out = path.replace('/', "\\");
    if is_drive_prefixed(&out) {
        // SAFETY: `is_drive_prefixed` guarantees a leading ascii byte.
        out[..1].make_ascii_uppercase();
    }
    PathBuf::from(out)
}

/// UTF-8 aware percent-decoding.
fn percent_decode_utf8(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex_val(b[i + 1]), hex_val(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_home() -> Option<PathBuf> {
        Some(PathBuf::from("/mock/home/user"))
    }

    /// Resolve simulating Windows environment with mounted drives 'C' and 'D'.
    fn t_win(uri: &str) -> Option<String> {
        file_uri_to_local_path_with(
            uri,
            |d| matches!(d, 'C' | 'D'),
            || Some(PathBuf::from(r"C:\Users\testuser")),
            true,
        )
        .map(|p| p.to_string_lossy().into_owned())
    }

    /// Resolve simulating Unix environment (macOS / Linux).
    fn t_unix(uri: &str) -> Option<String> {
        file_uri_to_local_path_with(uri, |_| false, mock_home, false)
            .map(|p| p.to_string_lossy().into_owned())
    }

    #[test]
    fn powershell_drive_uri() {
        assert_eq!(t_win("file:///C:/Users/me/file.txt"), Some(r"C:\Users\me\file.txt".into()));
    }

    #[test]
    fn drive_letter_uppercased() {
        assert_eq!(t_win("file:///d:/temp/x"), Some(r"D:\temp\x".into()));
    }

    #[test]
    fn percent_encoded_space() {
        assert_eq!(t_win("file:///C:/a%20b/c.txt"), Some(r"C:\a b\c.txt".into()));
    }

    #[test]
    fn utf8_filename_roundtrips() {
        assert_eq!(t_win("file:///D:/%E6%96%87%E6%A1%A3/x.md"), Some("D:\\文档\\x.md".into()));
    }

    #[test]
    fn scheme_is_case_insensitive() {
        assert_eq!(t_win("FILE:///C:/x"), Some(r"C:\x".into()));
    }

    #[test]
    fn wsl_mount_path() {
        assert_eq!(t_win("file://localhost/mnt/c/work/a.rs"), Some(r"C:\work\a.rs".into()));
    }

    #[test]
    fn cygdrive_mount_path() {
        assert_eq!(t_win("file://localhost/cygdrive/d/proj/b.rs"), Some(r"D:\proj\b.rs".into()));
    }

    #[test]
    fn msys_drive_form_when_drive_exists() {
        assert_eq!(t_win("file://localhost/d/temp_build/x"), Some(r"D:\temp_build\x".into()));
    }

    #[test]
    fn msys_drive_form_root() {
        assert_eq!(t_win("file://localhost/c/"), Some(r"C:\".into()));
    }

    #[test]
    fn leading_segment_that_is_not_a_drive_is_not_mangled_on_windows() {
        assert_eq!(t_win("file://localhost/z/opt/thing"), None);
    }

    #[test]
    fn pure_posix_path_falls_back_on_windows() {
        assert_eq!(t_win("file://localhost/home/user/.bashrc"), None);
    }

    #[test]
    fn remote_host_becomes_unc_on_windows() {
        assert_eq!(
            t_win("file://fileserver/share/doc.txt"),
            Some(r"\\fileserver\share\doc.txt".into())
        );
    }

    #[test]
    fn non_file_scheme_falls_back() {
        assert_eq!(t_win("https://example.com/a"), None);
        assert_eq!(t_unix("https://example.com/a"), None);
        assert_eq!(t_win("mailto:x@y.z"), None);
    }

    #[test]
    fn raw_windows_drive_paths() {
        assert_eq!(
            t_win("D:/work/git/gt_project_git_extra/docs/brainstorms/2026-09-18-global-tutorial-framework-requirements.md"),
            Some(r"D:\work\git\gt_project_git_extra\docs\brainstorms\2026-09-18-global-tutorial-framework-requirements.md".into())
        );
        assert_eq!(t_win(r"D:\work\git\proj\file.txt"), Some(r"D:\work\git\proj\file.txt".into()));
        assert_eq!(t_win("/C:/Users/me/file.txt"), Some(r"C:\Users\me\file.txt".into()));
        assert_eq!(
            t_win(r"C:\work\report%20final.docx"),
            Some(r"C:\work\report%20final.docx".into())
        );
    }

    #[test]
    fn file_uri_two_slashes_drive() {
        assert_eq!(t_win("file://D:/work/project/doc.md"), Some(r"D:\work\project\doc.md".into()));
    }

    #[test]
    fn raw_path_with_trailing_punctuation() {
        assert_eq!(t_win("D:/work/notes/spec.md。"), Some(r"D:\work\notes\spec.md".into()));
        assert_eq!(t_win("D:/work/notes/spec.md)"), Some(r"D:\work\notes\spec.md".into()));
        assert_eq!(t_win("<D:/work/notes/spec.md>"), Some(r"D:\work\notes\spec.md".into()));
    }

    #[test]
    fn home_path_expansion() {
        assert_eq!(t_unix("~/docs/spec.md"), Some("/mock/home/user/docs/spec.md".into()));
        assert_eq!(t_unix("~"), Some("/mock/home/user".into()));
        assert_eq!(t_unix("file://~/notes.txt"), Some("/mock/home/user/notes.txt".into()));
        assert_eq!(t_unix("file:///~/notes.txt"), Some("/mock/home/user/notes.txt".into()));
        assert_eq!(t_win(r"~\notes.txt"), Some(r"C:\Users\testuser\notes.txt".into()));
    }

    #[test]
    fn unix_posix_paths() {
        // Naked POSIX paths are NOT recognized as local file targets without file: or ~
        assert_eq!(t_unix("/Users/alice/projects/demo/src/main.rs"), None);
        assert_eq!(t_unix("/home/bob/work/repo/README.md"), None);
        assert_eq!(t_unix("/etc/passwd"), None);
        assert_eq!(t_unix("/usr/bin/gcc"), None);

        // Explicit file: URIs resolve properly
        assert_eq!(
            t_unix("file:///Users/alice/projects/demo/src/main.rs"),
            Some("/Users/alice/projects/demo/src/main.rs".into())
        );
        assert_eq!(
            t_unix("file:/home/bob/work/repo/README.md"),
            Some("/home/bob/work/repo/README.md".into())
        );
        assert_eq!(
            t_unix("file://localhost/home/bob/work/repo/README.md"),
            Some("/home/bob/work/repo/README.md".into())
        );
        assert_eq!(
            t_unix("file:///home/bob/docs/需求规格.md。"),
            Some("/home/bob/docs/需求规格.md".into())
        );
    }

    #[test]
    fn markdown_link_resolution() {
        assert_eq!(
            extract_link_target(
                "[全局新手引导框架需求规格](D:/work/git/gt_project_git_extra/docs/brainstorms/2026-09-18-global-tutorial-framework-requirements.md)。"
            ),
            "D:/work/git/gt_project_git_extra/docs/brainstorms/2026-09-18-global-tutorial-framework-requirements.md"
        );
        assert_eq!(extract_link_target("[Web](https://github.com)"), "https://github.com");
        assert_eq!(
            extract_link_target("[nested [brackets]](D:/path/file.txt)"),
            "D:/path/file.txt"
        );
        assert_eq!(extract_link_target("[spec](<D:/my files/spec.md>)"), "D:/my files/spec.md");

        assert_eq!(
            t_win("[全局新手引导框架需求规格](D:/work/git/gt_project_git_extra/docs/brainstorms/2026-09-18-global-tutorial-framework-requirements.md)。"),
            Some(r"D:\work\git\gt_project_git_extra\docs\brainstorms\2026-09-18-global-tutorial-framework-requirements.md".into())
        );
        assert_eq!(t_win("[link](file:///D:/notes/todo.txt)"), Some(r"D:\notes\todo.txt".into()));
        // Naked POSIX in Markdown does NOT resolve without file: or ~
        assert_eq!(t_unix("[docs](/home/bob/work/repo/README.md)"), None);
        assert_eq!(
            t_unix("[docs](file:///home/bob/work/repo/README.md)"),
            Some("/home/bob/work/repo/README.md".into())
        );
        assert_eq!(t_unix("[home](~/docs/spec.md)"), Some("/mock/home/user/docs/spec.md".into()));
        assert_eq!(
            extract_link_target("https://en.wikipedia.org/wiki/Rust_(programming_language)"),
            "https://en.wikipedia.org/wiki/Rust_(programming_language)"
        );
        assert_eq!(
            extract_link_target(
                "[Rust](https://en.wikipedia.org/wiki/Rust_(programming_language))。"
            ),
            "https://en.wikipedia.org/wiki/Rust_(programming_language)"
        );
        assert_eq!(extract_link_target("<https://example.com/docs>"), "https://example.com/docs");
        assert_eq!(extract_link_target("<D:/x.md>。"), "D:/x.md");
        assert_eq!(extract_link_target("<https://example.com/docs>。"), "https://example.com/docs");
        // Relative paths cannot be resolved without pane cwd context and must fall back.
        assert_eq!(t_win("./README.md"), None);
        assert_eq!(t_unix("./README.md"), None);
        assert_eq!(t_unix("docs/spec.md"), None);
    }

    #[test]
    fn local_link_contract_gates() {
        let lang = crate::i18n::UiLanguage::ZhCn;

        // Gate 1: Relative paths are locked down and rejected with user-visible guidance.
        assert_eq!(
            classify_link_target("./README.md"),
            LinkTargetKind::RelativePath("./README.md".into())
        );
        assert_eq!(
            classify_link_target("[Local](./doc.md)"),
            LinkTargetKind::RelativePath("./doc.md".into())
        );
        let rel_res = try_open_local_link("./README.md").unwrap();
        assert_eq!(rel_res, Err(LocalLinkError::MissingCwd("./README.md".into())));
        assert!(rel_res.unwrap_err().localized_message(lang).contains("缺少工作目录"));

        // Gate 2: Nonexistent local paths are locked down and reported as missing without spawning opener.
        let missing = PathBuf::from(r"C:\nonexistent_12345_nebula_test\file.md");
        assert_eq!(
            classify_link_target(missing.to_str().unwrap()),
            LinkTargetKind::LocalMissing(missing.clone())
        );
        let missing_res = try_open_local_link(missing.to_str().unwrap()).unwrap();
        assert_eq!(missing_res, Err(LocalLinkError::FileNotFound(missing.clone())));
        assert!(missing_res.unwrap_err().localized_message(lang).contains("文件不存在"));

        // Gate 3: Naked POSIX paths like /etc/passwd or /usr/bin/ls are Unrecognized (never LocalExisting).
        assert_eq!(
            classify_link_target("/etc/passwd"),
            LinkTargetKind::Unrecognized("/etc/passwd".into())
        );
        assert_eq!(
            classify_link_target("[docs](/etc/passwd)"),
            LinkTargetKind::Unrecognized("/etc/passwd".into())
        );
        assert_eq!(
            classify_link_target("/usr/bin/gcc"),
            LinkTargetKind::Unrecognized("/usr/bin/gcc".into())
        );
        assert_eq!(try_open_local_link("/etc/passwd"), None);
        assert_eq!(try_open_local_link("[docs](/etc/passwd)"), None);

        // Gate 4: Protocol URIs (https, wiki, ssh, file:/) are properly classified.
        let wiki = "https://en.wikipedia.org/wiki/Rust_(programming_language)";
        assert_eq!(classify_link_target(wiki), LinkTargetKind::ProtocolUri(wiki.into()));
        assert_eq!(try_open_local_link(wiki), None);

        let ssh_uri = "ssh:git@github.com:user/repo.git";
        assert_eq!(classify_link_target(ssh_uri), LinkTargetKind::ProtocolUri(ssh_uri.into()));
        assert_eq!(try_open_local_link(ssh_uri), None);

        let vscode_uri = "vscode://file/D:/project/main.rs";
        assert_eq!(
            classify_link_target(vscode_uri),
            LinkTargetKind::ProtocolUri(vscode_uri.into())
        );

        // Gate 4a: file:// URIs must be classified as local paths, NEVER ProtocolUri
        assert!(!is_web_or_protocol_uri("file:///C:/Windows"));
        #[cfg(windows)]
        {
            assert_eq!(
                classify_link_target("file:///C:/Windows"),
                LinkTargetKind::LocalExisting(PathBuf::from(r"C:\Windows"))
            );
            assert_eq!(
                classify_link_target("file:///C:/Users/me/a%20b.txt"),
                LinkTargetKind::LocalMissing(PathBuf::from(r"C:\Users\me\a b.txt"))
            );
            // UNC paths should be classified as LocalExisting without blocking exists() checks
            assert_eq!(
                classify_link_target(r"\\server\share\file.txt"),
                LinkTargetKind::LocalExisting(PathBuf::from(r"\\server\share\file.txt"))
            );
        }

        // Gate 4b: Relative paths resolve when cwd is supplied (with or without ./, and clean of ./)
        let repo_root = std::env::current_dir().unwrap();
        assert_eq!(
            classify_link_target_with_cwd("./Cargo.toml", Some(&repo_root)),
            LinkTargetKind::LocalExisting(repo_root.join("Cargo.toml"))
        );
        assert_eq!(
            classify_link_target_with_cwd("[spec](Cargo.toml)", Some(&repo_root)),
            LinkTargetKind::LocalExisting(repo_root.join("Cargo.toml"))
        );
        assert_eq!(
            classify_link_target_with_cwd(
                "[missing](nonexistent_nebula_file.xyz)",
                Some(&repo_root)
            ),
            LinkTargetKind::LocalMissing(repo_root.join("nonexistent_nebula_file.xyz"))
        );
        assert_eq!(
            classify_link_target_with_cwd("docs/nonexistent.md", Some(&repo_root)),
            LinkTargetKind::LocalMissing(repo_root.join("docs/nonexistent.md"))
        );
        assert_eq!(
            classify_link_target_with_cwd("Cargo.toml", None),
            LinkTargetKind::RelativePath("Cargo.toml".into())
        );
        assert_eq!(
            classify_link_target_with_cwd("[call](tel:123456)", Some(&repo_root)),
            LinkTargetKind::ProtocolUri("tel:123456".into())
        );
        assert_eq!(
            classify_link_target_with_cwd("[x](custom:foo)", Some(&repo_root)),
            LinkTargetKind::Unrecognized("custom:foo".into())
        );
        assert_eq!(
            classify_link_target_with_cwd("#section", Some(&repo_root)),
            LinkTargetKind::Unrecognized("#section".into())
        );
        assert_eq!(t_win("//cdn.example.com/lib.js"), None);
        assert_eq!(t_unix("//cdn.example.com/lib.js"), None);

        // Gate 5: Single-slash file:/ is recognized as local URI on both platforms
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing.txt");
        let native = missing.to_string_lossy().replace('\\', "/");
        let file_single = format!("file:/{}", native.trim_start_matches('/'));
        assert_eq!(classify_link_target(&file_single), LinkTargetKind::LocalMissing(missing));
        assert_eq!(t_unix("file:/home/user/whatever"), Some("/home/user/whatever".into()));
        assert_eq!(t_win("file:/C:/test.txt"), Some(r"C:\test.txt".into()));

        // Gate 6: Legacy handler reports failure outcome
        let outcome = handle_legacy_hint_command("./README.md", &[], lang);
        match outcome {
            LegacyHintOutcome::Failed(msg) => assert!(msg.contains("缺少工作目录")),
            other => panic!("expected failed outcome, got: {other:?}"),
        }

        let outcome = handle_legacy_hint_command("/etc/passwd", &[], lang);
        match outcome {
            LegacyHintOutcome::Failed(msg) => assert!(msg.contains("无法识别")),
            other => panic!("expected failed outcome, got: {other:?}"),
        }
    }

    #[test]
    fn probe_round2_review_coverage() {
        // Test all cases from review probe to guarantee zero regressions:
        // 1. file:// URIs must classify into local paths, never ProtocolUri
        #[cfg(windows)]
        {
            assert!(matches!(
                classify_link_target("file:///C:/Windows/"),
                LinkTargetKind::LocalExisting(_)
            ));
            assert_eq!(
                classify_link_target("file:///C:/Users/me/a%20b.txt"),
                LinkTargetKind::LocalMissing(PathBuf::from(r"C:\Users\me\a b.txt"))
            );
            assert_eq!(
                classify_link_target("file://localhost/mnt/c/work/a.rs"),
                LinkTargetKind::LocalMissing(PathBuf::from(r"C:\work\a.rs"))
            );
            assert_eq!(
                classify_link_target("file://fileserver/share/doc.txt"),
                LinkTargetKind::LocalExisting(PathBuf::from(r"\\fileserver\share\doc.txt"))
            );
            assert_eq!(
                classify_link_target("FILE:///C:/x"),
                LinkTargetKind::LocalMissing(PathBuf::from(r"C:\x"))
            );
            assert_eq!(
                classify_link_target("file:/C:/x"),
                LinkTargetKind::LocalMissing(PathBuf::from(r"C:\x"))
            );
            assert_eq!(
                classify_link_target(r"\\server\share\file.txt"),
                LinkTargetKind::LocalExisting(PathBuf::from(r"\\server\share\file.txt"))
            );
        }

        // 2. Generic protocol URIs must classify into ProtocolUri
        assert_eq!(
            classify_link_target("vscode://file/D:/x.rs"),
            LinkTargetKind::ProtocolUri("vscode://file/D:/x.rs".into())
        );

        // 3. Raw and markdown disk paths
        #[cfg(windows)]
        {
            assert_eq!(
                classify_link_target("D:/work/spec.md"),
                LinkTargetKind::LocalMissing(PathBuf::from(r"D:\work\spec.md"))
            );
            assert_eq!(
                classify_link_target("[x](D:/a/b)。"),
                LinkTargetKind::LocalMissing(PathBuf::from(r"D:\a\b"))
            );
            assert_eq!(
                classify_link_target("<D:/x.md>。"),
                LinkTargetKind::LocalMissing(PathBuf::from(r"D:\x.md"))
            );
            assert_eq!(
                classify_link_target("[app](C:/Program Files (x86)/foo.exe)"),
                LinkTargetKind::LocalMissing(PathBuf::from(r"C:\Program Files (x86)\foo.exe"))
            );
            assert_eq!(
                classify_link_target("D:/docs/需求（草案）.md"),
                LinkTargetKind::LocalMissing(PathBuf::from(r"D:\docs\需求（草案）.md"))
            );
        }
    }
}
