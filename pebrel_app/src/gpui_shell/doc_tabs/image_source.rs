//! Image sources share one bounded decoder and a view-owned asynchronous load.

use super::*;
use crate::ssh_sftp::{
    document::{DocumentOperation, RemoteLocation},
    preview::{self, IMAGE_MAX_BYTES, IMAGE_MAX_DECODED_BYTES, PreviewKind, ReadProgress},
};
use image::ImageDecoder as _;
use std::io::{self, Cursor, Read as _};
use tokio::sync::watch;

static DECODE_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

#[derive(Clone)]
pub(super) enum ImageSource {
    Local(PathBuf),
    Remote(RemoteLocation),
}

impl ImageSource {
    pub(super) fn path(&self) -> &Path {
        match self {
            Self::Local(path) => path,
            Self::Remote(location) => Path::new(&location.path),
        }
    }

    pub(super) fn display(&self) -> String {
        match self {
            Self::Local(path) => path.display().to_string(),
            Self::Remote(location) => location.display(),
        }
    }

    async fn read(
        self,
        operation: DocumentOperation,
        progress: watch::Sender<ReadProgress>,
    ) -> io::Result<Arc<[u8]>> {
        match self {
            Self::Remote(location) => {
                preview::load(location, operation, PreviewKind::Image, Some(progress))
                    .await
                    .map(|read| read.into_bytes())
            },
            Self::Local(path) => {
                let file = std::fs::File::open(path)?;
                let metadata = file.metadata()?;
                if !metadata.is_file() || metadata.len() > IMAGE_MAX_BYTES as u64 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Expected an image file up to 64 MiB",
                    ));
                }
                let mut bytes = Vec::new();
                file.take(IMAGE_MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
                if bytes.len() > IMAGE_MAX_BYTES {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "Image exceeds 64 MiB"));
                }
                Ok(bytes.into())
            },
        }
    }
}

impl ImageTabView {
    pub(crate) fn is_local_path(&self, path: &Path) -> bool {
        matches!(&self.source, ImageSource::Local(local) if local == path)
    }

    pub(crate) fn is_remote_location(&self, location: &RemoteLocation) -> bool {
        matches!(&self.source, ImageSource::Remote(remote) if remote == location)
    }

    pub(crate) fn reader_is_remote(&self) -> bool {
        matches!(self.source, ImageSource::Remote(_))
    }

    pub(crate) fn reader_ready(&self) -> bool {
        !self.loading && self.error.is_none() && self.image.is_some()
    }

    pub(crate) fn reader_snapshot(&self) -> Option<(Arc<[u8]>, u64)> {
        self.reader_ready().then(|| self.encoded.clone().map(|bytes| (bytes, self.generation)))?
    }

    pub(super) fn spawn_decode(&mut self, cx: &mut Context<Self>) {
        if let Some(operation) = self.operation.take() {
            operation.cancel();
        }
        self.load_task = None;
        self.progress_task = None;
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.loading = true;
        self.load_received = 0;
        self.load_total = None;
        self.encoded = None;
        self.image = None;
        self.error = None;
        let operation = DocumentOperation::default();
        self.operation = Some(operation.clone());
        let (send, receive) = watch::channel(ReadProgress::default());
        if self.reader_is_remote() {
            self.observe_progress(generation, receive, operation.clone(), cx);
        }
        let source = self.source.clone();
        let task = cx.background_executor().spawn(async move {
            let bytes = source.read(operation.clone(), send).await.map_err(read_error)?;
            let _permit = DECODE_SLOTS
                .acquire()
                .await
                .map_err(|error| (Message::ImageDecodeFailed, error.to_string()))?;
            if operation.is_cancelled() {
                return Err((Message::TransferCancelled, String::new()));
            }
            let image = decode_bytes(&bytes)?;
            if operation.is_cancelled() {
                return Err((Message::TransferCancelled, String::new()));
            }
            Ok((bytes, image))
        });
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let decoded = task.await;
            let _ = this.update(cx, |view, cx| {
                if view.generation != generation {
                    return;
                }
                let cancelled =
                    view.operation.as_ref().is_some_and(DocumentOperation::is_cancelled);
                view.operation = None;
                view.loading = false;
                view.progress_task = None;
                let decoded = if cancelled {
                    Err((Message::TransferCancelled, String::new()))
                } else {
                    decoded
                };
                match decoded {
                    Ok((bytes, image)) => {
                        view.set_image(image);
                        // Only remote readers need a retained encoded snapshot. Local
                        // Runtime reads continue to validate current disk metadata.
                        if view.reader_is_remote() {
                            view.encoded = Some(bytes);
                        }
                    },
                    Err(error) => view.error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn observe_progress(
        &mut self,
        generation: u64,
        mut progress: watch::Receiver<ReadProgress>,
        operation: DocumentOperation,
        cx: &mut Context<Self>,
    ) {
        let executor = cx.background_executor().clone();
        self.progress_task = Some(cx.spawn(async move |this, cx| {
            let mut prefix_len = 0;
            while progress.changed().await.is_ok() {
                let snapshot = progress.borrow_and_update().clone();
                let active = this
                    .update(cx, |view, cx| {
                        if !view.loading || view.generation != generation {
                            return false;
                        }
                        view.load_received = snapshot.received;
                        view.load_total = snapshot.total;
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !active || operation.is_cancelled() {
                    break;
                }
                if let Some(prefix) = snapshot.prefix.filter(|bytes| bytes.len() > prefix_len) {
                    prefix_len = prefix.len();
                    let control = operation.clone();
                    let image = executor
                        .spawn(async move {
                            let _permit = DECODE_SLOTS.acquire().await.ok()?;
                            if control.is_cancelled() {
                                return None;
                            }
                            // Some formats can decode a prefix. Incomplete-data errors are
                            // provisional; only decoding the verified full file can fail UI.
                            decode_bytes(&prefix).ok()
                        })
                        .await;
                    if operation.is_cancelled() {
                        break;
                    }
                    if let Some(image) = image {
                        let _ = this.update(cx, |view, cx| {
                            if view.loading && view.generation == generation {
                                view.set_image(image);
                                cx.notify();
                            }
                        });
                    }
                }
            }
        }));
    }

    fn set_image(&mut self, image: Arc<RenderImage>) {
        let size = image.size(0);
        self.geometry.set_decoded_dimensions((size.width.into(), size.height.into()));
        self.image = Some(image);
    }
}

fn read_error(error: io::Error) -> (Message, String) {
    if error.kind() == io::ErrorKind::Interrupted {
        (Message::TransferCancelled, String::new())
    } else {
        (Message::ImageReadFailed, error.to_string())
    }
}

/// All allocations and decoding happen on the background executor, including prefixes.
fn decode_bytes(bytes: &[u8]) -> Result<Arc<RenderImage>, (Message, String)> {
    let decode = || -> Result<_, Box<dyn std::error::Error>> {
        let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(IMAGE_MAX_DECODED_BYTES);
        reader.limits(limits);
        let decoder = reader.into_decoder()?;
        let (width, height) = decoder.dimensions();
        if u64::from(width) * u64::from(height) > IMAGE_MAX_DECODED_BYTES / 4
            || decoder.total_bytes() > IMAGE_MAX_DECODED_BYTES
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Decoded image exceeds 128 MiB",
            )
            .into());
        }
        let mut rgba = image::DynamicImage::from_decoder(decoder)?.into_rgba8();
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        Ok(Arc::new(RenderImage::new([image::Frame::new(rgba)])))
    };
    decode().map_err(|error| (Message::ImageDecodeFailed, error.to_string()))
}

impl Drop for ImageTabView {
    fn drop(&mut self) {
        if let Some(operation) = &self.operation {
            operation.cancel();
        }
    }
}
