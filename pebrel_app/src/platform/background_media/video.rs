//! Demand-driven animated-media decoding. Decoder objects stay on one owning worker.
//! This private adapter is exercised by the media lab before product activation.
#[cfg(windows)]
#[path = "video/media_foundation.rs"]
mod media_foundation;

use anyhow::{Result, ensure};
use gpui::{StreamImageBudgets, StreamImageLease, StreamImagePreparationLease};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    thread::{self, ThreadId},
    time::Duration,
};

pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Tightly packed top-down premultiplied BGRA; native RGB32 is made opaque.
    pub pixels: Vec<u8>,
    pub delay: Duration,
    pub pts_100ns: i64,
    pub sequence: u64,
    pub loops: u64,
    /// Moves with the pixels, including while an old scene retains its CPU frame.
    pub lease: StreamImageLease,
}

type Reply = SyncSender<Result<Option<Frame>>>;

/// Closing revokes future results without waiting on a native call on the UI thread.
pub struct Cursor {
    sender: Option<SyncSender<Reply>>,
    cancelled: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
    ui_thread: ThreadId,
}

struct ExitNotice {
    permit: Option<StreamImagePreparationLease>,
    exited: Arc<AtomicBool>,
}
impl Drop for ExitNotice {
    fn drop(&mut self) {
        // A cancelled decoder still owns its job until native teardown completes.
        drop(self.permit.take());
        self.exited.store(true, Ordering::Release);
    }
}

impl Cursor {
    pub fn start(
        path: PathBuf,
        budgets: StreamImageBudgets,
        jobs: StreamImageBudgets,
        ui_thread: ThreadId,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self> {
        #[cfg(windows)]
        {
            Self::spawn(jobs, ui_thread, cancelled, move |requests, cancelled| {
                let mut reader = None;
                for reply in requests {
                    if cancelled.load(Ordering::Acquire) {
                        break;
                    }
                    let result = (|| {
                        if reader.is_none() {
                            reader = Some(media_foundation::Reader::open(&path, budgets.clone())?);
                        }
                        reader.as_mut().unwrap().next_looping(&cancelled).map(Some)
                    })();
                    if cancelled.load(Ordering::Acquire) {
                        break;
                    }
                    let failed = result.is_err();
                    if reply.send(result).is_err() || failed {
                        break;
                    }
                }
                // Reader, samples, file sharing lease and COM/MF runtime drop here.
            })
        }
        #[cfg(not(windows))]
        {
            let _ = (path, budgets, jobs, ui_thread, cancelled);
            anyhow::bail!("system video decoding is not implemented on this platform")
        }
    }

    #[cfg(feature = "gif-background")]
    pub fn start_gif(
        path: PathBuf,
        budgets: StreamImageBudgets,
        jobs: StreamImageBudgets,
        ui_thread: ThreadId,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self> {
        Self::spawn(jobs, ui_thread, cancelled, move |requests, cancelled| {
            let mut reader = None;
            let mut owned = None;
            let mut pts = 0i64;
            for reply in requests {
                if cancelled.load(Ordering::Acquire) {
                    break;
                }
                let result = (|| {
                    if reader.is_none() {
                        // 预先计入画布、patch、restore、解码器及循环切换时短暂重叠的读取器。
                        owned = Some(budgets.reserve(21 * 1024 * 1024)?);
                        reader = Some(
                            super::gif_decoder::Cursor::open(&path).map_err(anyhow::Error::msg)?,
                        );
                    }
                    let lease = budgets.reserve(super::gif_decoder::MAX_FRAME_BYTES)?;
                    let cursor = reader.as_mut().unwrap();
                    let Some(decoded) = cursor.next_looping().map_err(anyhow::Error::msg)? else {
                        return Ok(None);
                    };
                    let (width, height) = decoded.pixels.dimensions();
                    let frame = Frame {
                        width,
                        height,
                        pixels: decoded.pixels.into_raw(),
                        delay: decoded.delay,
                        pts_100ns: pts,
                        sequence: decoded.sequence,
                        loops: cursor.loops_done(),
                        lease,
                    };
                    pts = pts.saturating_add(
                        (decoded.delay.as_nanos() / 100).min(i64::MAX as u128) as i64,
                    );
                    Ok(Some(frame))
                })();
                if cancelled.load(Ordering::Acquire) {
                    break;
                }
                let stopped = result.as_ref().is_ok_and(Option::is_none) || result.is_err();
                if reply.send(result).is_err() || stopped {
                    break;
                }
            }
            drop(reader);
            drop(owned);
        })
    }

    fn spawn(
        budgets: StreamImageBudgets,
        ui_thread: ThreadId,
        cancelled: Arc<AtomicBool>,
        run: impl FnOnce(Receiver<Reply>, Arc<AtomicBool>) + Send + 'static,
    ) -> Result<Self> {
        let permit = budgets.reserve_preparation()?;
        let (sender, requests) = sync_channel(1);
        let worker_cancelled = cancelled.clone();
        let exited = Arc::new(AtomicBool::new(false));
        let notice = ExitNotice { permit: Some(permit), exited: exited.clone() };
        thread::Builder::new().name("wallpaper-media".into()).spawn(move || {
            let _notice = notice;
            run(requests, worker_cancelled);
        })?;
        Ok(Self { sender: Some(sender), cancelled, exited, ui_thread })
    }

    /// One request and one response. Call from a background executor, never during paint.
    pub fn next_looping(&mut self) -> Result<Frame> {
        self.next_frame()?.ok_or_else(|| anyhow::anyhow!("animated source reached its end"))
    }

    pub fn next_frame(&mut self) -> Result<Option<Frame>> {
        ensure!(thread::current().id() != self.ui_thread, "video decode requested on UI thread");
        ensure!(!self.cancelled.load(Ordering::Acquire), "video source closed");
        let (reply, response) = sync_channel(1);
        self.sender.as_ref().ok_or_else(|| anyhow::anyhow!("video source closed"))?.send(reply)?;
        let frame = response.recv().map_err(|_| anyhow::anyhow!("video worker exited"))??;
        ensure!(!self.cancelled.load(Ordering::Acquire), "late video frame after close");
        Ok(frame)
    }

    pub fn exit_witness(&self) -> Arc<AtomicBool> {
        self.exited.clone()
    }

    pub fn close(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.sender.take();
    }
}
impl Drop for Cursor {
    fn drop(&mut self) {
        self.close();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BgraLayout {
    pub width: u32,
    pub height: u32,
    pub storage_width: u32,
    pub storage_height: u32,
    pub x: u32,
    pub y: u32,
    pub stride: i32,
}

/// Checks complete row ranges, including negative native stride, before copying.
pub fn copy_bgra_rows(
    width: u32,
    height: u32,
    stride: i32,
    source: &[u8],
    target: &mut [u8],
) -> Result<()> {
    copy_bgra_crop(
        BgraLayout {
            width,
            height,
            storage_width: width,
            storage_height: height,
            x: 0,
            y: 0,
            stride,
        },
        source,
        target,
    )
}

pub(super) fn copy_bgra_crop(layout: BgraLayout, source: &[u8], target: &mut [u8]) -> Result<()> {
    let BgraLayout { width, height, storage_width, storage_height, x, y, stride } = layout;
    let row = (width as usize).checked_mul(4).ok_or_else(|| anyhow::anyhow!("row overflow"))?;
    let stored_row = (storage_width as usize)
        .checked_mul(4)
        .ok_or_else(|| anyhow::anyhow!("storage row overflow"))?;
    let pitch = stride.unsigned_abs() as usize;
    let output =
        row.checked_mul(height as usize).ok_or_else(|| anyhow::anyhow!("output overflow"))?;
    let needed = pitch
        .checked_mul((storage_height as usize).saturating_sub(1))
        .and_then(|bytes| bytes.checked_add(stored_row))
        .ok_or_else(|| anyhow::anyhow!("stride overflow"))?;
    ensure!(
        width > 0
            && height > 0
            && pitch >= stored_row
            && needed <= source.len()
            && x.checked_add(width).is_some_and(|right| right <= storage_width)
            && y.checked_add(height).is_some_and(|bottom| bottom <= storage_height)
            && source.len() <= 4 * 1024 * 1024
            && output == target.len(),
        "BGRA layout outside admitted buffers"
    );
    for index in 0..height as usize {
        let display_row = y as usize + index;
        let source_row =
            if stride < 0 { storage_height as usize - 1 - display_row } else { display_row };
        let offset = source_row * pitch + x as usize * 4;
        let target = &mut target[index * row..(index + 1) * row];
        target.copy_from_slice(&source[offset..offset + row]);
        for alpha in target.iter_mut().skip(3).step_by(4) {
            *alpha = 255;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::StreamImageBudget;
    use std::sync::mpsc::channel;
    use std::time::Instant;

    #[cfg(feature = "gif-background")]
    #[test]
    fn finite_gif_worker_reports_end_and_retains_only_the_last_frame_lease() {
        let name = format!(
            "pebrel-gif-worker-{}-{}.gif",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        );
        let path = std::env::temp_dir().join(name);
        {
            let file =
                std::fs::OpenOptions::new().create_new(true).write(true).open(&path).unwrap();
            let mut encoder = gif::Encoder::new(file, 2, 2, &[255, 0, 0]).unwrap();
            encoder
                .write_frame(&gif::Frame {
                    width: 2,
                    height: 2,
                    delay: 4,
                    buffer: std::borrow::Cow::Owned(vec![0; 4]),
                    ..Default::default()
                })
                .unwrap();
        }
        let local = StreamImageBudget::new(32 * 1024 * 1024);
        let global = StreamImageBudget::new(96 * 1024 * 1024);
        let worker = StreamImageBudget::with_allocation_limit(1, 1);
        let mut cursor = Cursor::start_gif(
            path.clone(),
            StreamImageBudgets::new(local.clone(), global.clone()),
            StreamImageBudgets::new(StreamImageBudget::with_allocation_limit(1, 1), worker.clone()),
            thread::current().id(),
            Arc::default(),
        )
        .unwrap();
        let exited = cursor.exit_witness();
        let frame = thread::spawn(move || {
            let frame = cursor.next_frame().unwrap().unwrap();
            assert_eq!(&frame.pixels[..4], &[0, 0, 255, 255]);
            assert!(cursor.next_frame().unwrap().is_none());
            frame
        })
        .join()
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !exited.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert_eq!(worker.preparations(), 0);
        assert_eq!(local.used(), super::super::gif_decoder::MAX_FRAME_BYTES);
        assert!(local.peak() <= 32 * 1024 * 1024);
        drop(frame);
        assert_eq!((local.used(), global.used()), (0, 0));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn cancellation_during_work_refuses_late_pixels_without_releasing_early() {
        let local = StreamImageBudget::with_allocation_limit(16, 1);
        let global = StreamImageBudget::with_allocation_limit(32, 2);
        let budgets = StreamImageBudgets::new(local.clone(), global.clone());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (started, ready) = channel();
        let (resume, resume_worker) = channel();
        let bytes = budgets.clone();
        let mut cursor = Cursor::spawn(
            budgets,
            thread::current().id(),
            cancelled.clone(),
            move |requests, _| {
                let reply = requests.recv().unwrap();
                let lease = bytes.reserve(4).unwrap();
                started.send(()).unwrap();
                resume_worker.recv().unwrap();
                let _ = reply.send(Ok(Some(Frame {
                    width: 1,
                    height: 1,
                    pixels: vec![0, 0, 0, 255],
                    delay: Duration::from_millis(33),
                    pts_100ns: 0,
                    sequence: 1,
                    loops: 0,
                    lease,
                })));
            },
        )
        .unwrap();
        let exited = cursor.exit_witness();
        let request = thread::spawn(move || cursor.next_looping());
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        cancelled.store(true, Ordering::Release);
        assert_eq!((local.used(), global.used(), local.preparations()), (4, 4, 1));
        assert!(!exited.load(Ordering::Acquire));
        resume.send(()).unwrap();
        assert!(request.join().unwrap().is_err());
        let deadline = Instant::now() + Duration::from_secs(2);
        while !exited.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert_eq!(
            (local.used(), global.used(), local.preparations(), global.preparations()),
            (0, 0, 0, 0)
        );
    }

    #[test]
    fn aligned_storage_padding_is_not_visible_for_either_stride_direction() {
        let source = [1, 2, 3, 0, 4, 5, 6, 0, 70, 80, 90, 0, 100, 110, 120, 0];
        let mut output = [0; 8];
        let layout = BgraLayout {
            width: 1,
            height: 2,
            storage_width: 1,
            storage_height: 4,
            x: 0,
            y: 0,
            stride: 4,
        };
        copy_bgra_crop(layout, &source, &mut output).unwrap();
        assert_eq!(output, [1, 2, 3, 255, 4, 5, 6, 255]);
        copy_bgra_crop(BgraLayout { stride: -4, ..layout }, &source, &mut output).unwrap();
        assert_eq!(output, [100, 110, 120, 255, 70, 80, 90, 255]);
        assert!(copy_bgra_crop(BgraLayout { y: 3, ..layout }, &source, &mut output).is_err());
    }

    #[test]
    fn native_stride_and_alpha_are_checked_before_copy() {
        let source = [1, 2, 3, 0, 4, 5, 6, 0, 99, 99, 99, 99, 7, 8, 9, 0, 10, 11, 12, 0];
        let mut result = [0; 16];
        copy_bgra_rows(2, 2, -12, &source, &mut result).unwrap();
        assert_eq!(result, [7, 8, 9, 255, 10, 11, 12, 255, 1, 2, 3, 255, 4, 5, 6, 255]);
        assert!(copy_bgra_rows(2, 2, 4, &source, &mut result).is_err());
        assert!(copy_bgra_rows(2, 2, 12, &source[..16], &mut result).is_err());
        assert!(copy_bgra_rows(0, 2, 0, &[], &mut []).is_err());
    }

    #[test]
    fn ui_decode_is_refused_and_close_releases_only_after_worker_exit() {
        let local = StreamImageBudget::with_allocation_limit(16, 1);
        let global = StreamImageBudget::with_allocation_limit(32, 2);
        let budgets = StreamImageBudgets::new(local.clone(), global.clone());
        let (started, ready) = channel();
        let (resume, resume_worker) = channel();
        let mut cursor =
            Cursor::spawn(budgets.clone(), thread::current().id(), Arc::default(), move |_, _| {
                started.send(()).unwrap();
                resume_worker.recv().unwrap();
            })
            .unwrap();
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(cursor.next_looping().err().unwrap().to_string().contains("UI thread"));
        let exited = cursor.exit_witness();
        cursor.close();
        assert_eq!((local.preparations(), global.preparations()), (1, 1));
        assert!(budgets.reserve_preparation().is_err());
        assert!(!exited.load(Ordering::Acquire));
        resume.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !exited.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert_eq!((local.preparations(), global.preparations()), (0, 0));
    }

    #[test]
    fn global_worker_refusal_rolls_back_local_admission() {
        let local = StreamImageBudget::with_allocation_limit(16, 1);
        let global = StreamImageBudget::with_allocation_limit(32, 1);
        let budgets = StreamImageBudgets::new(local.clone(), global.clone());
        let held = StreamImageBudgets::new(StreamImageBudget::new(16), global.clone())
            .reserve_preparation()
            .unwrap();
        assert!(
            Cursor::spawn(budgets, thread::current().id(), Arc::default(), |_, _| panic!(
                "not admitted"
            ))
            .is_err()
        );
        assert_eq!((local.preparations(), global.preparations()), (0, 1));
        drop(held);
    }
}
