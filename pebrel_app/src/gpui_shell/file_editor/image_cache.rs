//! Document-owned decoded images. Virtual rows must not populate the process-wide
//! retain-all asset map. Eviction releases both pixel buffers and atlas entries.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui::{
    App, AppContext, Asset, AssetLogger, Entity, EntityId, ImageAssetLoader, ImageCache,
    ImageCacheError, RenderImage, Resource, Task, WeakEntity, Window,
};

use crate::render_cache::RenderCache;

const IMAGE_BYTES: usize = 64 * 1024 * 1024;
const IMAGE_ENTRIES: usize = 128;
const MAX_LOADING: usize = 2;
const WORKING_BYTES: usize = 16 * 1024 * 1024;

type ImageResult = Result<Arc<RenderImage>, ImageCacheError>;

pub(super) struct DocumentImageCache {
    owner: WeakEntity<Self>,
    ready: RenderCache<Resource, ImageResult>,
    loading: HashMap<Resource, Task<()>>,
    view: EntityId,
    frame_images: HashSet<Resource>,
    byte_limit: usize,
    active: bool,
    generation: u64,
}

impl DocumentImageCache {
    pub(super) fn new(view: EntityId, cx: &mut App) -> Entity<Self> {
        Self::with_limits(view, IMAGE_BYTES, IMAGE_ENTRIES, cx)
    }

    fn with_limits(view: EntityId, bytes: usize, entries: usize, cx: &mut App) -> Entity<Self> {
        let cache = cx.new(|cx| Self {
            owner: cx.entity().downgrade(),
            ready: RenderCache::new(bytes, entries),
            loading: HashMap::new(),
            view,
            frame_images: HashSet::new(),
            byte_limit: bytes,
            active: true,
            generation: 0,
        });
        cx.observe_release(&cache, |cache, cx| {
            cache.clear(None, cx);
        })
        .detach();
        cache
    }

    pub(super) fn begin_frame(&mut self) {
        self.frame_images.clear();
    }

    fn clear(&mut self, window: Option<&mut Window>, cx: &mut App) {
        self.generation = self.generation.wrapping_add(1);
        self.loading.clear();
        self.frame_images.clear();
        let images: Vec<_> = self.ready.drain().flatten().collect();
        if let Some(window) = window {
            for image in images {
                cx.drop_image(image, Some(window));
            }
        } else if !images.is_empty() {
            // Release observers may run during a window update, when that window
            // is temporarily absent from App.windows. Defer until it is restored.
            cx.defer(move |cx| {
                for image in images {
                    cx.drop_image(image, None);
                }
            });
        }
    }

    pub(super) fn set_active(&mut self, active: bool, window: &mut Window, cx: &mut App) {
        if self.active == active {
            return;
        }
        self.active = active;
        if !active {
            self.clear(Some(window), cx);
        }
    }

    pub(super) fn finish_frame(&mut self, window: &mut Window, cx: &mut App) {
        self.ready.trim_with_eviction(
            WORKING_BYTES,
            |source| self.frame_images.contains(source),
            |old| {
                if let Ok(image) = old {
                    cx.drop_image(image, Some(window));
                }
            },
        );
    }
}

impl ImageCache for DocumentImageCache {
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<ImageResult> {
        if !self.active {
            return None;
        }
        self.frame_images.insert(resource.clone());
        if let Some(image) = self.ready.get(resource) {
            return Some(image.clone());
        }
        if self.loading.contains_key(resource) || self.loading.len() >= MAX_LOADING {
            return None;
        }

        // Call the loader directly: fetch_asset/use_asset would also retain its
        // completed result globally, defeating this owner's eviction policy.
        let future = AssetLogger::<ImageAssetLoader>::load(resource.clone(), cx);
        let background = cx.background_executor().spawn(future);
        let owner = self.owner.clone();
        let source = resource.clone();
        let generation = self.generation;
        let task = window.spawn(cx, async move |cx| {
            let mut result = background.await;
            let _ = owner.update_in(cx, |cache, window, cx| {
                if !cache.active || cache.generation != generation {
                    return;
                }
                cache.loading.remove(&source);
                let mut bytes = result.as_ref().map_or(1, |image| {
                    (0..image.frame_count()).fold(0usize, |total, frame| {
                        total.saturating_add(image.as_bytes(frame).map_or(0, <[u8]>::len))
                    })
                });
                if bytes > cache.byte_limit {
                    // The decoded cache limit is not a bound on transient decoder
                    // storage. Keep a small failure entry to avoid repeated decode.
                    result = Err(image::ImageError::Limits(image::error::LimitError::from_kind(
                        image::error::LimitErrorKind::InsufficientMemory,
                    ))
                    .into());
                    bytes = 1;
                }
                cache.ready.insert_with_eviction(source, result, bytes, |old| {
                    if let Ok(image) = old {
                        cx.drop_image(image, Some(window));
                    }
                });
                App::notify(cx, cache.view);
            });
        });
        self.loading.insert(resource.clone(), task);
        None
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use gpui::{ImageSource, TestAppContext};

    #[gpui::test]
    fn suspension_drops_decodes_cancels_jobs_and_reloads_only_on_resume(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let document = directory.path().join("inactive.md");
        std::fs::write(&document, "").unwrap();
        let (file, mut cx) = super::super::tests::open(document, cx);
        let cache =
            cx.update(|_, cx| DocumentImageCache::with_limits(file.entity_id(), 128, 8, cx));
        let path = directory.path().join("image.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255])).save(&path).unwrap();
        let source = Resource::Path(Arc::from(path));
        cx.update(|window, cx| {
            cache.update(cx, |cache, cx| assert!(cache.load(&source, window, cx).is_none()));
        });
        cx.run_until_parked();
        let old = cx.update(|window, cx| {
            cache.update(cx, |cache, cx| {
                let image = cache.load(&source, window, cx).unwrap().unwrap();
                let weak = Arc::downgrade(&image);
                cache.set_active(false, window, cx);
                assert_eq!(cache.ready.used_bytes(), 0);
                assert!(cache.load(&source, window, cx).is_none());
                assert!(cache.loading.is_empty());
                weak
            })
        });
        assert!(old.upgrade().is_none());
        cx.update(|window, cx| {
            cache.update(cx, |cache, cx| {
                cache.set_active(true, window, cx);
                assert!(cache.load(&source, window, cx).is_none());
                assert_eq!(cache.loading.len(), 1);
                cache.set_active(false, window, cx);
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            cache.update(cx, |cache, cx| {
                assert_eq!(cache.ready.used_bytes(), 0);
                assert!(cache.loading.is_empty());
                cache.set_active(true, window, cx);
                assert!(cache.load(&source, window, cx).is_none());
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            cache.update(cx, |cache, cx| {
                assert!(cache.load(&source, window, cx).unwrap().is_ok());
                assert_eq!(cache.ready.used_bytes(), 64);
            });
        });
    }

    #[gpui::test]
    fn decoding_reuses_hot_images_evicts_cold_pixels_and_releases_with_document(
        cx: &mut TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let document = directory.path().join("empty.md");
        std::fs::write(&document, "").unwrap();
        let (file, mut cx) = super::super::tests::open(document, cx);
        let cache =
            cx.update(|_, cx| DocumentImageCache::with_limits(file.entity_id(), 128, 8, cx));
        let resources: Vec<_> = (0..3)
            .map(|index| {
                let path = directory.path().join(format!("image-{index}.png"));
                image::RgbaImage::from_pixel(4, 4, image::Rgba([index, 0, 0, 255]))
                    .save(&path)
                    .unwrap();
                Resource::Path(Arc::from(path))
            })
            .collect();
        let mut weak_images = Vec::new();
        for resource in &resources {
            cx.update(|window, cx| {
                assert!(cache.update(cx, |cache, cx| cache.load(resource, window, cx)).is_none());
            });
            cx.run_until_parked();
            let image = cx.update(|window, cx| {
                cache.update(cx, |cache, cx| cache.load(resource, window, cx)).unwrap().unwrap()
            });
            weak_images.push(Arc::downgrade(&image));
            cx.update(|window, cx| {
                let reused = cache.update(cx, |cache, cx| cache.load(resource, window, cx));
                assert_eq!(reused.unwrap().unwrap().id, image.id);
                assert!(!ImageSource::Resource(resource.clone()).is_asset_cached(cx));
                assert!(cache.read(cx).ready.used_bytes() <= 128);
            });
        }
        assert!(weak_images[0].upgrade().is_none());
        assert!(weak_images[1].upgrade().is_some());
        assert!(weak_images[2].upgrade().is_some());
        cx.update(|_, _| drop(cache));
        cx.run_until_parked();
        assert!(weak_images.iter().all(|image| image.upgrade().is_none()));
    }

    #[gpui::test]
    fn large_decodes_are_not_retained_or_retried_and_pending_work_has_an_owner(
        cx: &mut TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let document = directory.path().join("empty.md");
        std::fs::write(&document, "").unwrap();
        let (file, mut cx) = super::super::tests::open(document, cx);
        let path = directory.path().join("large.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 0, 255])).save(&path).unwrap();
        let source = Resource::Path(Arc::from(path));
        let cache = cx.update(|_, cx| DocumentImageCache::with_limits(file.entity_id(), 16, 8, cx));
        cx.update(|window, cx| {
            assert!(cache.update(cx, |cache, cx| cache.load(&source, window, cx)).is_none());
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(
                cache.update(cx, |cache, cx| cache.load(&source, window, cx)).unwrap().is_err()
            );
            assert_eq!(cache.read(cx).ready.used_bytes(), 1);
            assert!(cache.read(cx).loading.is_empty());
            for index in 0..10 {
                let source =
                    Resource::Path(Arc::from(directory.path().join(format!("{index}.png"))));
                cache.update(cx, |cache, cx| cache.load(&source, window, cx));
            }
            assert_eq!(cache.read(cx).loading.len(), MAX_LOADING);
        });
        let owner = cache.downgrade();
        cx.update(|_, _| drop(cache));
        cx.run_until_parked();
        assert!(owner.upgrade().is_none());
        cx.update(|_, cx| assert!(!ImageSource::Resource(source).is_asset_cached(cx)));
    }
}
