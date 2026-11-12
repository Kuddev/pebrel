//! Community catalog inside the theme picker. The picker owns this state: the
//! catalog is fetched when the Community filter is first opened, previews and
//! installs run on the background executor, and closing the picker cancels
//! downloads and discards late results.

use super::appearance_picker::AppearanceSelection;
use super::*;
use crate::i18n::Message;
use crate::theme_library::community::{self, Entry, Installed, Listing};
use futures::StreamExt as _;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

pub(super) const COMMUNITY_FILTER: usize = 4;
const PREVIEW_CONCURRENCY: usize = 3;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

/// Production downloads from the catalog; rendered tests substitute a local archive.
pub(super) type Installer = fn(
    &Entry,
    &crate::theme_library::ThemeLibraryStore,
    &AtomicBool,
    &mut dyn FnMut(u64),
) -> Result<crate::theme_library::ThemeDocument, community::Error>;

pub(super) enum CatalogLoad {
    Loading,
    Ready(Listing),
    Failed(String),
}

pub(super) enum Preview {
    Loading,
    Ready(Arc<RenderImage>),
    Unavailable,
}

pub(super) enum InstallState {
    Running(Arc<AtomicU64>),
    Failed(String),
}

pub(super) struct Community {
    generation: u64,
    cancel: Arc<AtomicBool>,
    installer: Installer,
    pub(super) catalog: CatalogLoad,
    pub(super) previews: HashMap<String, Preview>,
    pub(super) installs: HashMap<String, InstallState>,
    /// Success text for the most recent install, shown in place.
    pub(super) notice: Option<String>,
}

impl Drop for Community {
    fn drop(&mut self) {
        // Downloads stop at the next chunk; a finished install stays in the library.
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Community {
    fn new(catalog: CatalogLoad, installer: Installer) -> Self {
        Self {
            generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
            cancel: Arc::new(AtomicBool::new(false)),
            installer,
            catalog,
            previews: HashMap::new(),
            installs: HashMap::new(),
            notice: None,
        }
    }

    pub(super) fn entry(&self, id: &str) -> Option<&Entry> {
        match &self.catalog {
            CatalogLoad::Ready(listing) => listing.entries.iter().find(|entry| entry.id == id),
            _ => None,
        }
    }

    pub(super) fn count(&self) -> usize {
        match &self.catalog {
            CatalogLoad::Ready(listing) => listing.entries.len(),
            _ => 0,
        }
    }
}

impl SettingsPane {
    fn community_mut(&mut self, generation: u64) -> Option<&mut Community> {
        self.appearance_picker
            .as_mut()
            .and_then(|picker| picker.community.as_mut())
            .filter(|community| community.generation == generation)
    }

    /// Start (or restart after a failure) the catalog request. Only an explicit
    /// visit to the Community filter or Retry reaches the network.
    pub(super) fn load_community(&mut self, retry: bool, cx: &mut Context<Self>) {
        let Some(picker) = self.appearance_picker.as_mut() else { return };
        if picker.community.is_some() && !retry {
            return;
        }
        let community = Community::new(CatalogLoad::Loading, community::install_with_progress);
        let generation = community.generation;
        let cancel = community.cancel.clone();
        picker.community = Some(community);
        let language = crate::gpui_shell::config::ui_language(cx);
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = executor
                .spawn(async move {
                    community::fetch_catalog().map_err(|error| error.describe(language))
                })
                .await;
            let previews = this
                .update(cx, |this, cx| {
                    let community = this.community_mut(generation)?;
                    let previews = match result {
                        Ok(listing) => {
                            let previews: Vec<_> = listing
                                .entries
                                .iter()
                                .filter_map(|entry| {
                                    Some((entry.id.clone(), entry.preview.clone()?))
                                })
                                .collect();
                            for (id, _) in &previews {
                                community.previews.insert(id.clone(), Preview::Loading);
                            }
                            community.catalog = CatalogLoad::Ready(listing);
                            previews
                        },
                        Err(error) => {
                            community.catalog = CatalogLoad::Failed(error);
                            Vec::new()
                        },
                    };
                    cx.notify();
                    Some(previews)
                })
                .ok()
                .flatten()
                .unwrap_or_default();
            // A slow image does not hold up the rest of the first row.
            let previews = futures::stream::iter(previews)
                .map(|(id, url)| {
                    let executor = executor.clone();
                    let cancel = cancel.clone();
                    async move {
                        let image = executor
                            .spawn(async move {
                                let mut pixels = community::fetch_preview(&url, &cancel).ok()?;
                                for pixel in pixels.pixels_mut() {
                                    pixel.0.swap(0, 2); // RenderImage takes BGRA.
                                }
                                Some(Arc::new(RenderImage::new([image::Frame::new(pixels)])))
                            })
                            .await;
                        (id, image)
                    }
                })
                .buffer_unordered(PREVIEW_CONCURRENCY);
            futures::pin_mut!(previews);
            while let Some((id, image)) = previews.next().await {
                let current = this
                    .update(cx, |this, cx| {
                        let Some(community) = this.community_mut(generation) else { return false };
                        community
                            .previews
                            .insert(id, image.map_or(Preview::Unavailable, Preview::Ready));
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !current {
                    return;
                }
            }
        })
        .detach();
        cx.notify();
    }

    pub(super) fn community_installed(&self, entry: &Entry) -> Installed {
        self.appearance_picker
            .as_ref()
            .map_or(Installed::No, |picker| community::installed(entry, &picker.custom_themes))
    }

    pub(super) fn install_community_theme(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = self.appearance_picker.as_mut() else { return };
        if picker.apply_busy || picker.custom_loading {
            return;
        }
        let Some(community) = picker.community.as_mut() else { return };
        if matches!(community.installs.get(&id), Some(InstallState::Running(_))) {
            return;
        }
        let Some(entry) = community.entry(&id).cloned() else { return };
        let progress = Arc::new(AtomicU64::new(0));
        community.installs.insert(id.clone(), InstallState::Running(progress.clone()));
        community.notice = None;
        let previous_draft = (picker.draft, picker.foreground_override);
        let generation = community.generation;
        let cancel = community.cancel.clone();
        let installer = community.installer;
        let language = crate::gpui_shell::config::ui_language(cx);
        let executor = cx.background_executor().clone();
        let timer = executor.clone();
        let watched = progress.clone();
        let progress_id = id.clone();
        cx.spawn(async move |this, cx| {
            let mut shown = 0;
            loop {
                timer.timer(PROGRESS_INTERVAL).await;
                let bytes = watched.load(Ordering::Relaxed);
                let running = this
                    .update(cx, |this, cx| {
                        let running = this.community_mut(generation).is_some_and(|community| {
                            matches!(community.installs.get(&progress_id),
                            Some(InstallState::Running(current)) if Arc::ptr_eq(current, &watched))
                        });
                        if running && bytes != shown {
                            cx.notify();
                        }
                        running
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
                shown = bytes;
            }
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            let result = executor
                .spawn(async move {
                    let store = crate::theme_library::ThemeLibraryStore::default();
                    let document = installer(&entry, &store, &cancel, &mut |bytes| {
                        progress.store(bytes, Ordering::Relaxed);
                    })
                    .map_err(|error| error.describe(language))?;
                    let definition = document.definition().map_err(|error| error.to_string())?;
                    Ok::<_, String>((document, definition))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.community_mut(generation).is_none() {
                    return;
                }
                match result {
                    Ok((document, definition)) => {
                        let name = document.name().to_owned();
                        let focus = cx.focus_handle();
                        let picker = this.appearance_picker.as_mut().unwrap();
                        let index = picker.adopt_custom_theme(document, definition, focus);
                        let community = picker.community.as_mut().unwrap();
                        community.installs.remove(&id);
                        community.notice =
                            Some(language.format(Message::ThemeCommunityAdded, &[("name", &name)]));
                        if picker.filter == COMMUNITY_FILTER
                            && (picker.draft, picker.foreground_override) == previous_draft
                        {
                            this.choose_appearance_draft(
                                AppearanceSelection::Custom(index),
                                window,
                                cx,
                            );
                        }
                    },
                    Err(error) => {
                        let community = this.community_mut(generation).unwrap();
                        let name = community.entry(&id).map(|entry| entry.name.clone());
                        community.installs.insert(
                            id.clone(),
                            InstallState::Failed(language.format(
                                Message::ThemeCommunityInstallFailed,
                                &[("name", name.as_deref().unwrap_or(&id)), ("error", &error)],
                            )),
                        );
                    },
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Select an already installed catalog theme so the preview column shows it.
    pub(super) fn preview_community_theme(
        &mut self,
        choice: AppearanceSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.appearance_picker.as_ref().is_some_and(|picker| picker.draft != choice) {
            self.choose_appearance_draft(choice, window, cx);
        }
    }
}

#[cfg(test)]
impl SettingsPane {
    /// Install a ready catalog without the network request `load_community` makes.
    pub(super) fn seed_community(&mut self, listing: Listing, installer: Installer) {
        let picker = self.appearance_picker.as_mut().expect("open theme picker");
        picker.community = Some(Community::new(CatalogLoad::Ready(listing), installer));
    }
}
