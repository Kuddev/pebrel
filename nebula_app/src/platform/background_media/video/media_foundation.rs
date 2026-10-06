//! Software H.264 source reader. Every COM object is created and released on its worker.
use super::{BgraLayout, Frame, copy_bgra_crop};
use anyhow::{Result, bail, ensure};
use gpui::StreamImageBudgets;
use std::{
    fs::{File, OpenOptions},
    os::windows::fs::OpenOptionsExt,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use windows::{
    Win32::{
        Media::MediaFoundation::*,
        System::{
            Com::{
                COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize,
                StructuredStorage::{
                    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
                },
            },
            Variant::VT_I8,
        },
    },
    core::{GUID, HSTRING},
};

const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
const ALL: u32 = MF_SOURCE_READER_ALL_STREAMS.0 as u32;
const FRAME_LIMIT: usize = 4 * 1024 * 1024;

struct Runtime;
impl Runtime {
    fn start() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        }
        if let Err(error) = unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) } {
            unsafe {
                CoUninitialize();
            }
            return Err(error.into());
        }
        Ok(Self)
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            CoUninitialize();
        }
    }
}

pub(super) struct Reader {
    reader: IMFSourceReader,
    // Field order keeps the file and runtime alive until the reader is released.
    _source: File,
    _runtime: Runtime,
    budgets: StreamImageBudgets,
    width: u32,
    height: u32,
    layout: BgraLayout,
    period: Duration,
    last_pts: Option<i64>,
    eos: bool,
    sequence: u64,
    loops: u64,
}
impl Reader {
    pub(super) fn open(path: &Path, budgets: StreamImageBudgets) -> Result<Self> {
        let source = OpenOptions::new().read(true).share_mode(1).open(path)?;
        let bytes = source.metadata()?.len();
        ensure!(
            source.metadata()?.is_file() && bytes > 0 && bytes <= 32 * 1024 * 1024,
            "video file exceeds 32 MiB admission before COM/MF startup"
        );
        let runtime = Runtime::start()?;
        let mut attributes = None;
        unsafe {
            MFCreateAttributes(&mut attributes, 3)?;
        }
        let attributes = attributes.unwrap();
        unsafe {
            attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
            attributes.SetUINT32(&MF_SOURCE_READER_DISABLE_DXVA, 1)?;
            attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 0)?;
        }
        let reader = unsafe {
            MFCreateSourceReaderFromURL(
                &HSTRING::from(path.to_string_lossy().as_ref()),
                &attributes,
            )?
        };
        unsafe {
            reader.SetStreamSelection(ALL, false)?;
            reader.SetStreamSelection(VIDEO, true)?;
        }
        let native = unsafe { reader.GetNativeMediaType(VIDEO, 0)? };
        let dimensions = unsafe { native.GetUINT64(&MF_MT_FRAME_SIZE)? };
        let (width, height) = ((dimensions >> 32) as u32, dimensions as u32);
        ensure!(
            width > 0
                && height > 0
                && width <= 1280
                && height <= 720
                && u64::from(width) * u64::from(height) * 4 <= FRAME_LIMIT as u64,
            "video canvas exceeds 720p admission before sample decode"
        );
        ensure!(
            unsafe { native.GetGUID(&MF_MT_SUBTYPE)? } == MFVideoFormat_H264,
            "system video adapter currently supports H.264 only"
        );
        let rate = unsafe { native.GetUINT64(&MF_MT_FRAME_RATE)? };
        let (numerator, denominator) = (rate >> 32, rate as u32 as u64);
        ensure!(
            numerator > 0 && denominator > 0 && numerator <= denominator * 60,
            "video frame rate exceeds 60 fps admission"
        );
        let period = Duration::from_nanos(1_000_000_000 * denominator / numerator);
        let requested = unsafe { MFCreateMediaType()? };
        unsafe {
            requested.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            requested.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
            reader.SetCurrentMediaType(VIDEO, None, &requested)?;
        }
        let layout = Self::admit_output(&reader, width, height, None)?;
        Ok(Self {
            reader,
            _source: source,
            _runtime: runtime,
            budgets,
            width,
            height,
            layout,
            period,
            last_pts: None,
            eos: false,
            sequence: 0,
            loops: 0,
        })
    }

    fn admit_output(
        reader: &IMFSourceReader,
        width: u32,
        height: u32,
        previous: Option<i32>,
    ) -> Result<BgraLayout> {
        let current = unsafe { reader.GetCurrentMediaType(VIDEO)? };
        let actual_size = unsafe { current.GetUINT64(&MF_MT_FRAME_SIZE)? };
        let actual_format = unsafe { current.GetGUID(&MF_MT_SUBTYPE)? };
        let (storage_width, storage_height) = ((actual_size >> 32) as u32, actual_size as u32);
        ensure!(
            actual_format == MFVideoFormat_RGB32
                && (storage_width == width || storage_width == width.div_ceil(16) * 16)
                && (storage_height == height || storage_height == height.div_ceil(16) * 16),
            "converted media type exceeds admitted visible canvas/alignment: expected {width}x{height} RGB32, got {storage_width}x{storage_height} {actual_format:?}"
        );
        let (mut x, mut y) = (0, 0);
        for key in [MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_GEOMETRIC_APERTURE] {
            if let Ok(length) = unsafe { current.GetBlobSize(&key) } {
                ensure!(
                    length as usize == std::mem::size_of::<MFVideoArea>(),
                    "invalid display aperture size"
                );
                let mut bytes = [0u8; std::mem::size_of::<MFVideoArea>()];
                unsafe {
                    current.GetBlob(&key, &mut bytes, None)?;
                }
                let area = unsafe { bytes.as_ptr().cast::<MFVideoArea>().read_unaligned() };
                ensure!(
                    area.Area.cx == width as i32
                        && area.Area.cy == height as i32
                        && area.OffsetX.fract == 0
                        && area.OffsetY.fract == 0
                        && area.OffsetX.value >= 0
                        && area.OffsetY.value >= 0,
                    "unsupported video display aperture"
                );
                x = area.OffsetX.value as u32;
                y = area.OffsetY.value as u32;
                break;
            }
        }
        let stride = unsafe {
            current.GetUINT32(&MF_MT_DEFAULT_STRIDE).map(|s| s as i32).or_else(|_| {
                MFGetStrideForBitmapInfoHeader(MFVideoFormat_RGB32.data1, storage_width)
            })?
        };
        let pitch = stride.unsigned_abs() as usize;
        let needed = pitch
            .checked_mul(storage_height as usize - 1)
            .and_then(|n| n.checked_add(storage_width as usize * 4));
        ensure!(
            pitch >= storage_width as usize * 4
                && needed.is_some_and(|n| n <= FRAME_LIMIT)
                && x + width <= storage_width
                && y + height <= storage_height
                && previous.is_none_or(|value| value == stride),
            "output stride/aperture exceeds existing admission"
        );
        Ok(BgraLayout { width, height, storage_width, storage_height, x, y, stride })
    }

    fn rewind(&mut self) -> Result<()> {
        let zero = PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_I8,
                    Anonymous: PROPVARIANT_0_0_0 { hVal: 0 },
                    ..Default::default()
                }),
            },
        };
        unsafe {
            self.reader.Flush(ALL)?;
            self.reader.SetCurrentPosition(&GUID::zeroed(), &zero)?;
        }
        self.last_pts = None;
        self.eos = false;
        self.loops += 1;
        Ok(())
    }

    pub(super) fn next_looping(&mut self, cancelled: &AtomicBool) -> Result<Frame> {
        let mut rewound = false;
        for _ in 0..1024 {
            ensure!(!cancelled.load(Ordering::Acquire), "video decode cancelled");
            if self.eos {
                ensure!(!rewound, "video contains no decodable frames");
                self.rewind()?;
                rewound = true;
            }
            // Reserve before ReadSample/contiguous conversion: original and contiguous
            // sample envelopes are each at most 4 MiB. Codec internal memory is opaque.
            let _samples = self.budgets.reserve((2 * FRAME_LIMIT) as u64)?;
            let output_bytes = self.width as usize * self.height as usize * 4;
            let lease = self.budgets.reserve(output_bytes as u64)?;
            let mut flags = 0;
            let mut pts = 0;
            let mut sample = None;
            unsafe {
                self.reader.ReadSample(
                    VIDEO,
                    0,
                    None,
                    Some(&mut flags),
                    Some(&mut pts),
                    Some(&mut sample),
                )?;
            }
            ensure!(!cancelled.load(Ordering::Acquire), "video decode cancelled after native read");
            ensure!(
                flags & MF_SOURCE_READERF_ERROR.0 as u32 == 0,
                "native video decoder reported an error"
            );
            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                self.layout = Self::admit_output(
                    &self.reader,
                    self.width,
                    self.height,
                    Some(self.layout.stride),
                )?;
            }
            self.eos = flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0;
            let Some(sample) = sample else {
                continue;
            };
            ensure!(self.last_pts.is_none_or(|last| pts > last), "nonmonotonic video PTS");
            self.last_pts = Some(pts);
            ensure!(
                unsafe { sample.GetTotalLength()? } as usize <= FRAME_LIMIT,
                "native sample exceeds envelope"
            );
            let buffer = unsafe { sample.ConvertToContiguousBuffer()? };
            let length = unsafe { buffer.GetCurrentLength()? } as usize;
            let needed = self.layout.stride.unsigned_abs() as usize
                * (self.layout.storage_height as usize - 1)
                + self.layout.storage_width as usize * 4;
            ensure!(
                (needed..=FRAME_LIMIT).contains(&length),
                "sample byte length exceeds row admission"
            );
            let mut pointer = std::ptr::null_mut();
            let mut locked_length = 0;
            unsafe {
                buffer.Lock(&mut pointer, None, Some(&mut locked_length))?;
            }
            struct Locked<'a>(&'a IMFMediaBuffer);
            impl Drop for Locked<'_> {
                fn drop(&mut self) {
                    unsafe {
                        let _ = self.0.Unlock();
                    }
                }
            }
            let _locked = Locked(&buffer);
            ensure!(
                !pointer.is_null() && locked_length as usize == length,
                "locked video buffer changed"
            );
            let mut pixels = vec![0; output_bytes];
            copy_bgra_crop(
                self.layout,
                unsafe { std::slice::from_raw_parts(pointer, length) },
                &mut pixels,
            )?;
            let duration = unsafe { sample.GetSampleDuration().ok() };
            let delay = if let Some(duration) = duration {
                ensure!((1..=100_000_000).contains(&duration), "invalid video sample duration");
                Duration::from_nanos(duration as u64 * 100)
            } else {
                self.period
            };
            ensure!(
                !delay.is_zero() && delay <= Duration::from_secs(10),
                "invalid video sample duration"
            );
            self.sequence += 1;
            return Ok(Frame {
                width: self.width,
                height: self.height,
                pixels,
                delay,
                pts_100ns: pts,
                sequence: self.sequence,
                loops: self.loops,
                lease,
            });
        }
        bail!("video control packets exceeded bounded read admission")
    }
}
