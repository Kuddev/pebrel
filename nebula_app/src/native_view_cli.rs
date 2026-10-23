//! 显式内容入口；没有协商能力或输出被重定向时保留普通文本。
use std::ffi::OsString;
use std::io::{self, IsTerminal as _, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use clap::{Args, ValueEnum};
use nebula_terminal::inline_content::{MAX_CONTENT_BYTES, MAX_TITLE_BYTES, Packet};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Kind {
    Markdown,
    Table,
    Git,
}

#[derive(Debug, Args)]
pub struct Options {
    /// Native renderer: Markdown, JSON/NDJSON table, or Git progress.
    #[arg(long = "type", value_enum)]
    kind: Kind,
    /// UTF-8 input file; omit to read standard input.
    #[arg(conflicts_with = "command")]
    input: Option<PathBuf>,
    /// Force native output, for a compatible remote terminal.
    #[arg(long, conflicts_with = "plain")]
    native: bool,
    /// Print original text instead of the native view protocol.
    #[arg(long)]
    plain: bool,
    /// Run Git with separately captured stdout/stderr; e.g. -- git fetch --progress.
    #[arg(last = true, num_args = 1..)]
    command: Vec<OsString>,
}

pub fn run(options: Options) -> i32 {
    match execute(options) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("pebrel view: {error}");
            1
        },
    }
}

fn execute(options: Options) -> io::Result<i32> {
    let native = !options.plain
        && (options.native
            || (io::stdout().is_terminal()
                && std::env::var("PEBREL_NATIVE_RESULTS").as_deref() == Ok("1")));
    if !options.command.is_empty() {
        return run_git(&options, native);
    }
    let mut input: Box<dyn Read> = match &options.input {
        Some(path) => Box::new(std::fs::File::open(path)?),
        None => Box::new(io::stdin()),
    };
    if !native {
        io::copy(&mut input, &mut io::stdout())?;
        return Ok(0);
    }
    let mut bytes = Vec::new();
    input.take((MAX_CONTENT_BYTES + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_CONTENT_BYTES {
        return Err(io::Error::other(
            "native input exceeds 4 MiB; use --plain or split the dataset",
        ));
    }
    let text = String::from_utf8(bytes).map_err(|_| io::Error::other("input must be UTF-8"))?;
    let title = options
        .input
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "stdin".into());
    let mut output = Emitter::new(&options, &title)?;
    output.append(&text, false)?;
    output.finish(0)?;
    Ok(0)
}

struct Emitter {
    id: u64,
    output: io::Stdout,
}

fn bounded_title(title: &str) -> String {
    let mut result = String::new();
    for c in title.chars().filter(|c| !c.is_control()) {
        if result.len() + c.len_utf8() > MAX_TITLE_BYTES {
            break;
        }
        result.push(c);
    }
    result
}
impl Emitter {
    fn new(options: &Options, title: &str) -> io::Result<Self> {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
            ^ u64::from(std::process::id());
        let mime = match options.kind {
            Kind::Markdown => "text/markdown",
            Kind::Table => "application/json",
            Kind::Git => "text/x-git-progress",
        };
        let mut emitter = Self { id, output: io::stdout() };
        emitter.output.write_all(b"\r")?;
        emitter.send(Packet::Begin {
            wire_id: id,
            rows: 2,
            mime: mime.into(),
            title: bounded_title(title),
        })?;
        emitter.output.write_all(b"\r\n")?;
        // 发送端只占一个真实锚点行，富内容高度由视图布局决定；ConPTY 与前端仍看到同一个 CRLF。
        emitter.output.flush()?;
        Ok(emitter)
    }
    fn send(&mut self, packet: Packet) -> io::Result<()> {
        self.output.write_all(packet.encode().as_bytes())?;
        self.output.flush()
    }
    fn append(&mut self, mut text: &str, stderr: bool) -> io::Result<()> {
        while !text.is_empty() {
            let mut end = text.len().min(16 * 1024);
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            let text_chunk = text[..end].to_owned();
            self.send(if stderr {
                Packet::AppendStderr { wire_id: self.id, text: text_chunk }
            } else {
                Packet::Append { wire_id: self.id, text: text_chunk }
            })?;
            text = &text[end..];
        }
        Ok(())
    }
    fn finish(&mut self, code: i32) -> io::Result<()> {
        self.send(Packet::End { wire_id: self.id, code })
    }
}

fn run_git(options: &Options, native: bool) -> io::Result<i32> {
    let program = PathBuf::from(&options.command[0]);
    if !matches!(options.kind, Kind::Git)
        || !program.file_name().is_some_and(|name| {
            name.eq_ignore_ascii_case("git") || name.eq_ignore_ascii_case("git.exe")
        })
    {
        return Err(io::Error::other("command capture requires --type git -- git <arguments>"));
    }
    let mut command = Command::new(&options.command[0]);
    command.args(&options.command[1..]);
    if !native {
        return Ok(command.status()?.code().unwrap_or(1));
    }
    // Git 的阶段文本由 locale 决定；固定子进程语言，避免把任意日志数字猜成进度。
    command.env("LC_ALL", "C").env("LANG", "C").stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let title = options.command.iter().map(|v| v.to_string_lossy()).collect::<Vec<_>>().join(" ");
    let mut emitter = match Emitter::new(options, &title) {
        Ok(emitter) => emitter,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        },
    };
    let (tx, rx) = std::sync::mpsc::sync_channel::<io::Result<(bool, String)>>(8);
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let readers = [(Box::new(stdout) as Box<dyn Read + Send>), Box::new(stderr)];
    let handles: Vec<_> = readers
        .into_iter()
        .enumerate()
        .map(|(stream, reader)| {
            let tx = tx.clone();
            std::thread::spawn(move || {
                if let Err(error) =
                    read_utf8(reader, |text| tx.send(Ok((stream == 1, text))).is_ok())
                {
                    let _ = tx.send(Err(error));
                }
            })
        })
        .collect();
    drop(tx);
    let mut failure = None;
    for text in rx {
        let result = text.and_then(|(stderr, text)| emitter.append(&text, stderr));
        if let Err(error) = result {
            failure = Some(error);
            let _ = child.kill();
            break;
        }
    }
    // 接收器在异常路径已经释放，读线程不会卡在有界队列上。
    let status = child.wait()?;
    for handle in handles {
        let _ = handle.join();
    }
    let code = status.code().unwrap_or(1);
    emitter.finish(if failure.is_some() { 1 } else { code })?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(code)
}

fn read_utf8(
    mut reader: Box<dyn Read + Send>,
    mut send: impl FnMut(String) -> bool,
) -> io::Result<()> {
    let mut buffer = [0; 8192];
    let mut pending = Vec::new();
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            return if pending.is_empty() {
                Ok(())
            } else {
                Err(io::Error::other("incomplete UTF-8 output"))
            };
        }
        pending.extend_from_slice(&buffer[..n]);
        let valid = match std::str::from_utf8(&pending) {
            Ok(_) => pending.len(),
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Err(_) => return Err(io::Error::other("Git output must be UTF-8")),
        };
        if valid > 0 {
            let text = std::str::from_utf8(&pending[..valid]).unwrap().to_owned();
            if !send(text) {
                return Ok(());
            }
            pending.drain(..valid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn title_budget_is_in_bytes_without_splitting_unicode() {
        let title = bounded_title(&"🦀".repeat(80));
        assert_eq!(title.len(), 256);
        assert_eq!(title.chars().count(), 64);
    }
    #[test]
    fn utf8_reader_preserves_split_multibyte_characters() {
        struct Bytes(std::io::Cursor<Vec<u8>>);
        impl Read for Bytes {
            fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
                self.0.read(&mut b[..1])
            }
        }
        let mut output = String::new();
        read_utf8(Box::new(Bytes(std::io::Cursor::new("中文🦀".as_bytes().to_vec()))), |text| {
            output.push_str(&text);
            true
        })
        .unwrap();
        assert_eq!(output, "中文🦀");
    }
}
