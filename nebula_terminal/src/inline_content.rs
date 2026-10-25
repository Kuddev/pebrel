//! 有界原生内容传输与网格锚点；这里只保存 MIME 和字节，不依赖 GUI 或工具类型。
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

pub const PREFIX: &str = "777;pebrel-view;1;";
pub const MAX_FRAME_BYTES: usize = 48 * 1024;
pub const MAX_CONTENT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_TITLE_BYTES: usize = 256;
const MAX_RETAINED_BYTES: usize = 16 * 1024 * 1024;
const MAX_SURFACES: usize = 32;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

mod command;
pub(crate) use command::CommandCapture;

/// Renderer metadata selected by the host; the terminal never interprets its MIME.
pub struct CommandView {
    pub mime: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    Begin { wire_id: u64, rows: u16, mime: String, title: String },
    Place { wire_id: u64, row: u16 },
    Append { wire_id: u64, text: String },
    AppendStderr { wire_id: u64, text: String },
    End { wire_id: u64, code: i32 },
}

impl Packet {
    pub fn parse(payload: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(payload).ok()?.strip_prefix(PREFIX)?;
        let mut fields = text.split(';');
        let wire_id = fields.next()?.parse().ok()?;
        let decode = |s: &str| String::from_utf8(STANDARD.decode(s).ok()?).ok();
        let packet = match fields.next()? {
            "B" => {
                let rows = fields.next()?.parse().ok()?;
                let mime = decode(fields.next()?)?;
                let title = decode(fields.next()?)?;
                if !(2..=40).contains(&rows)
                    || mime.len() > 96
                    || title.len() > MAX_TITLE_BYTES
                    || mime.chars().any(char::is_control)
                    || title.chars().any(char::is_control)
                {
                    return None;
                }
                Self::Begin { wire_id, rows, mime, title }
            },
            "A" => Self::Append { wire_id, text: decode(fields.next()?)? },
            "P" => {
                let row = fields.next()?.parse().ok()?;
                if row >= 40 {
                    return None;
                }
                Self::Place { wire_id, row }
            },
            "S" => Self::AppendStderr { wire_id, text: decode(fields.next()?)? },
            "E" => Self::End { wire_id, code: fields.next()?.parse().ok()? },
            _ => return None,
        };
        fields.next().is_none().then_some(packet)
    }

    pub fn encode(&self) -> String {
        let fields = match self {
            Self::Begin { wire_id, rows, mime, title } => {
                format!("{wire_id};B;{rows};{};{}", STANDARD.encode(mime), STANDARD.encode(title))
            },
            Self::Append { wire_id, text } => format!("{wire_id};A;{}", STANDARD.encode(text)),
            Self::Place { wire_id, row } => format!("{wire_id};P;{row}"),
            Self::AppendStderr { wire_id, text } => {
                format!("{wire_id};S;{}", STANDARD.encode(text))
            },
            Self::End { wire_id, code } => format!("{wire_id};E;{code}"),
        };
        format!("\x1b]{PREFIX}{fields}\x07")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Receiving,
    Finished(i32),
    Interrupted,
    Limited,
    Evicted,
}

#[derive(Debug)]
struct Content {
    chunks: Vec<Arc<Chunk>>,
    bytes: usize,
    revision: u64,
    status: Status,
}

#[derive(Debug)]
pub struct Surface {
    pub id: u64,
    rows: AtomicU16,
    max_rows: u16,
    pub mime: Arc<str>,
    pub title: Arc<str>,
    content: Mutex<Content>,
    source_hashes: Vec<u64>,
}

impl PartialEq for Surface {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for Surface {}

#[derive(Debug)]
pub struct Chunk {
    pub stderr: bool,
    pub text: String,
}

pub struct Update {
    pub chunks: Vec<Arc<Chunk>>,
    pub chunk_count: usize,
    pub revision: u64,
    pub status: Status,
}

impl Surface {
    pub fn rows(&self) -> u16 {
        self.rows.load(Ordering::Relaxed)
    }

    fn new(rows: u16, mime: String, title: String) -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            rows: AtomicU16::new(1),
            max_rows: rows,
            mime: mime.into(),
            title: title.into(),
            content: Mutex::new(Content {
                chunks: Vec::new(),
                bytes: 0,
                revision: 1,
                status: Status::Receiving,
            }),
            source_hashes: Vec::new(),
        }
    }

    pub fn has_grid_source(&self) -> bool {
        !self.source_hashes.is_empty()
    }

    pub fn source_row_matches(&self, row: u16, cells: &[crate::term::cell::Cell]) -> bool {
        self.source_hashes
            .get(usize::from(row))
            .is_some_and(|expected| *expected == source_hash(cells))
    }

    pub fn revision(&self) -> u64 {
        self.content.lock().unwrap().revision
    }

    /// 只克隆新增块的引用，鼠标移动和重复绘制不复制完整内容。
    pub fn update_since(&self, chunk: usize) -> Update {
        let content = self.content.lock().unwrap();
        Update {
            chunks: content.chunks.get(chunk..).unwrap_or_default().to_vec(),
            chunk_count: content.chunks.len(),
            revision: content.revision,
            status: content.status,
        }
    }

    fn append(&self, text: String, stderr: bool) {
        if text.is_empty() {
            return;
        }
        let mut content = self.content.lock().unwrap();
        if content.status != Status::Receiving {
            return;
        }
        content.revision += 1;
        if content.bytes.saturating_add(text.len()) > MAX_CONTENT_BYTES
            || content.chunks.len() >= 4096
        {
            content.status = Status::Limited;
            return;
        }
        content.bytes += text.len();
        content.chunks.push(Arc::new(Chunk { text, stderr }));
    }

    fn finish(&self, status: Status) {
        let mut content = self.content.lock().unwrap();
        if content.status == Status::Receiving {
            content.status = status;
            content.revision += 1;
        }
    }

    fn evict(&self) {
        let mut content = self.content.lock().unwrap();
        content.chunks = Vec::new();
        content.bytes = 0;
        content.status = Status::Evicted;
        content.revision += 1;
    }

    fn bytes(&self) -> usize {
        self.content.lock().unwrap().bytes
    }
}

/// 只在预留行首分配。标记随 Cell 移动，重排不再依赖会失效的裸绝对行号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub surface: Arc<Surface>,
    pub row: u16,
}

#[derive(Default)]
pub(crate) struct Registry {
    retained: VecDeque<(u64, Weak<Surface>)>,
}

impl Registry {
    fn capture(
        &mut self,
        view: CommandView,
        text: String,
        hashes: Vec<u64>,
        code: i32,
    ) -> Arc<Surface> {
        let rows = hashes.len() as u16;
        let mut surface = Surface::new(rows, view.mime, view.title);
        surface.rows.store(rows, Ordering::Relaxed);
        surface.source_hashes = hashes;
        surface.append(text, false);
        surface.finish(Status::Finished(code));
        let surface = Arc::new(surface);
        self.retained.push_back((surface.id | (1 << 63), Arc::downgrade(&surface)));
        self.trim();
        surface
    }

    pub(crate) fn apply(&mut self, packet: Packet) -> Option<Arc<Surface>> {
        // 注册表不延长源的寿命；最后一个网格标记消失即可释放原文。
        self.retained.retain(|(_, surface)| surface.strong_count() > 0);
        let result = match packet {
            Packet::Begin { wire_id, rows, mime, title } => {
                if self.retained.iter().any(|(id, _)| *id == wire_id) {
                    return None;
                }
                let surface = Arc::new(Surface::new(rows, mime, title));
                self.retained.push_back((wire_id, Arc::downgrade(&surface)));
                Some(surface)
            },
            Packet::Append { wire_id, text } => {
                if let Some(surface) = self
                    .retained
                    .iter()
                    .find(|(id, _)| *id == wire_id)
                    .and_then(|(_, surface)| surface.upgrade())
                {
                    surface.append(text, false);
                }
                None
            },
            Packet::Place { wire_id, row } => self
                .retained
                .iter()
                .find(|(id, _)| *id == wire_id)
                .and_then(|(_, surface)| surface.upgrade())
                .filter(|surface| row < surface.max_rows && row == surface.rows())
                .inspect(|surface| {
                    // 只能逐行增长，跳号和重复标记不能让高度领先于真实占位。
                    surface.rows.store(row + 1, Ordering::Relaxed);
                    surface.content.lock().unwrap().revision += 1;
                }),
            Packet::AppendStderr { wire_id, text } => {
                if let Some(surface) = self
                    .retained
                    .iter()
                    .find(|(id, _)| *id == wire_id)
                    .and_then(|(_, surface)| surface.upgrade())
                {
                    surface.append(text, true);
                }
                None
            },
            Packet::End { wire_id, code } => {
                if let Some(surface) = self
                    .retained
                    .iter()
                    .find(|(id, _)| *id == wire_id)
                    .and_then(|(_, surface)| surface.upgrade())
                {
                    surface.finish(Status::Finished(code));
                }
                None
            },
        };
        self.trim();
        result
    }

    fn trim(&mut self) {
        self.retained.retain(|(_, surface)| surface.strong_count() > 0);
        let mut total: usize = self
            .retained
            .iter()
            .filter_map(|(_, surface)| surface.upgrade())
            .map(|surface| surface.bytes())
            .sum();
        while self.retained.len() > MAX_SURFACES || total > MAX_RETAINED_BYTES {
            let Some((_, surface)) = self.retained.pop_front() else { break };
            let Some(surface) = surface.upgrade() else { continue };
            total = total.saturating_sub(surface.bytes());
            // 历史仍有锚点时明确显示“已释放”，不能把不完整数据当成功结果。
            surface.evict();
        }
    }

    pub(crate) fn finish_pending(&self) {
        for (_, surface) in &self.retained {
            if let Some(surface) = surface.upgrade() {
                surface.finish(Status::Interrupted);
            }
        }
    }
}

fn source_hash(cells: &[crate::term::cell::Cell]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    // 窗口加宽补的空白不是输出内容；软折行标记仍参与校验，避免把重排后的旧跨度当真。
    let end = cells
        .iter()
        .rposition(|cell| {
            cell.c != ' '
                || cell.zerowidth().is_some_and(|z| !z.is_empty())
                || !cell.flags.is_empty()
        })
        .map_or(0, |index| index + 1);
    for cell in &cells[..end] {
        cell.c.hash(&mut hash);
        cell.zerowidth().unwrap_or_default().hash(&mut hash);
        cell.flags.bits().hash(&mut hash);
    }
    hash.finish()
}

impl Drop for Registry {
    fn drop(&mut self) {
        self.finish_pending();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_rows_grow_only_for_contiguous_places_within_cap() {
        let mut registry = Registry::default();
        let surface = registry
            .apply(Packet::Begin {
                wire_id: 9,
                rows: 4,
                mime: "text/plain".into(),
                title: "compact".into(),
            })
            .unwrap();
        assert_eq!(surface.rows(), 1);
        for row in [0, 2, 4] {
            assert!(registry.apply(Packet::Place { wire_id: 9, row }).is_none());
            assert_eq!(surface.rows(), 1);
        }
        for row in 1..4 {
            assert!(registry.apply(Packet::Place { wire_id: 9, row }).is_some());
            assert_eq!(surface.rows(), row + 1);
            assert!(registry.apply(Packet::Place { wire_id: 9, row }).is_none());
        }
        assert!(registry.apply(Packet::Place { wire_id: 9, row: 4 }).is_none());
        assert_eq!(surface.rows(), 4);
    }
    #[test]
    fn frame_roundtrip_and_invalid_input() {
        let packet = Packet::Begin {
            wire_id: 7,
            rows: 14,
            mime: "text/markdown".into(),
            title: "中文结果".into(),
        };
        let encoded = packet.encode();
        assert_eq!(Packet::parse(&encoded.as_bytes()[2..encoded.len() - 1]), Some(packet));
        assert!(Packet::parse(b"777;pebrel-view;1;7;A;/w==").is_none());
        assert!(Packet::parse(b"777;pebrel-view;1;7;E;0;extra").is_none());
    }
    #[test]
    fn incremental_chunks_limits_and_lifetime() {
        let mut registry = Registry::default();
        let surface = registry
            .apply(Packet::Begin {
                wire_id: 1,
                rows: 8,
                mime: "text/plain".into(),
                title: "test".into(),
            })
            .unwrap();
        registry.apply(Packet::Append { wire_id: 1, text: "第一块".into() });
        assert_eq!(surface.update_since(0).chunks[0].text, "第一块");
        assert!(surface.update_since(1).chunks.is_empty());
        registry.apply(Packet::Append { wire_id: 1, text: "x".repeat(MAX_CONTENT_BYTES) });
        registry.apply(Packet::End { wire_id: 1, code: 0 });
        assert_eq!(surface.update_since(1).status, Status::Limited);
        drop(surface);
        registry.apply(Packet::End { wire_id: 2, code: 0 });
        assert!(registry.retained.is_empty());
    }
    #[test]
    fn registry_does_not_own_content_and_ending_stream_marks_interrupted() {
        let mut registry = Registry::default();
        let surface = registry
            .apply(Packet::Begin {
                wire_id: 2,
                rows: 8,
                mime: "text/plain".into(),
                title: "test".into(),
            })
            .unwrap();
        registry.apply(Packet::AppendStderr { wire_id: 2, text: "stderr".into() });
        let update = surface.update_since(0);
        assert!(update.chunks[0].stderr);
        assert_eq!(update.chunks[0].text, "stderr");
        drop(update);
        drop(registry);
        assert_eq!(surface.update_since(0).status, Status::Interrupted);
        let weak = Arc::downgrade(&surface);
        drop(surface);
        assert!(weak.upgrade().is_none());
    }
}
