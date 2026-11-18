//! Thumbnail and system icon cache, one per app, shared by all tabs and windows.
//!
//! - Thumbnails: 3 background threads read via `Fs::read` and decode with `arke_core::thumbs`.
//! - System icons: 1 background thread (COM STA), only for `.exe`, `.lnk`, `.ico`, asked for at
//!   the size they are drawn at (one of [`SOURCE_SIZES`] in the grid, [`LIST_ICON_PX`] in lists),
//!   so big tiles don't stretch a small icon.
//! - Shell thumbnails: 2 background threads (COM STA) ask Windows for the thumbnail of any file
//!   Rust can't decode (videos, PDFs, PSDs, ... `can_shell_thumbnail`). No bytes are read here, so
//!   the 64 MB source cap doesn't apply. Files with no provider fail once; the failure is cached
//!   like any other. The Shell thumbnails setting (or `ARKE_NO_SHELL_THUMBNAILS`) turns this off.
//! - Order for a file: Arke's own thumbnail, else the Shell's, else the system or type icon, else
//!   the drawn kind icon.
//! - Folder peeks: 1 background thread finds the first picture in a folder, which the Icons view
//!   shows between the folder's back and front panels.
//! - Type icons: 1 background thread (COM STA) asks the Shell for the icon of each file type
//!   (by extension, never reading the file) and of a plain folder, once per type and size. These
//!   replace Arke's drawn kind icons while `prefs::system_type_icons` is on, on any drive.
//! - Videos: 1 background thread asks a separate process (`thumb_host.rs`) for Windows'
//!   thumbnail (a frame); Shell thumbnails above skip videos.
//! - Rust, Shell and video thumbnails are also kept compressed ([`arke_core::thumb_cache`]) in
//!   memory and on disk, so seeing one again costs a 0.1–0.2 ms decode instead of reading the
//!   original.
//! - Thumbnails and system icons only on local fixed drives (cached per drive letter; the check itself runs in the
//!   background).
//! - The visible tab calls [`MediaStore::want`] every frame with visible items first; requests
//!   scrolled away are dropped before work starts (`arke_core::loader`).
//! - Decoded images have a small budget: least recently used ones are freed and removed from the GPU
//!   atlas, but never the ones on screen or one page around it. Scrolling back decodes them again
//!   from the compressed copy, on the UI thread while it fits in a few milliseconds.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use arke_core::filetype::{
    MAX_THUMBNAIL_SOURCE, can_shell_thumbnail, can_thumbnail, can_video_thumbnail,
    shell_thumbnail_ext, wants_system_icon, wants_type_icon,
};
use arke_core::loader::Loader;
use arke_core::thumb_cache::{ThumbCache, ThumbKey};
use arke_core::thumbs::{SOURCE_SIZES, THUMBNAIL_SIZE, Thumbnail, decode_thumbnail};
use arke_core::{Entry, Fs, SortSpec};
use gpui::{App, AppContext, Context, Entity, Global, RenderImage, Task};

use crate::explorer::Services;

/// System icon size for lists and anywhere no thumbnail size is given (16 px rows, sharp up to
/// 300% display scale).
pub(crate) const LIST_ICON_PX: u32 = 48;
/// Path of a type icon's key: `*` can't be in a file name, so it never names a file.
const TYPE_ICON_PREFIX: &str = "*.";
/// Decoded pixel bytes kept beyond what is on screen and a page around it. A 192px thumbnail is
/// about 150 KB, so roughly 100 fit; the compressed copies cover the rest.
const CACHE_BUDGET: usize = 16 * 1024 * 1024;
/// Compressed thumbnails in memory: 5–15 KB each, so several thousand.
const COMPRESSED_BUDGET: usize = 48 * 1024 * 1024;
/// The disk cache folder's cap (`%LOCALAPPDATA%\Arke\thumbs`): tens of thousands of thumbnails.
const DISK_CAP: u64 = 256 * 1024 * 1024;
/// Time per frame for decoding compressed thumbnails on the UI thread (the rest go to the loader).
const SYNC_DECODE: Duration = Duration::from_millis(3);

/// Thumbnails the background loader was asked for (tests).
#[cfg(test)]
pub(crate) static THUMBNAIL_LOADS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Compressed thumbnails in memory and on disk, for every window. Tests keep them in memory only.
pub(crate) fn thumb_cache() -> &'static ThumbCache {
    static CACHE: OnceLock<ThumbCache> = OnceLock::new();
    CACHE.get_or_init(|| {
        let dir = std::env::var_os("LOCALAPPDATA")
            .filter(|_| !cfg!(test))
            .map(|d| PathBuf::from(d).join("Arke").join("thumbs"));
        ThumbCache::new(COMPRESSED_BUDGET, dir.map(|d| (d, DISK_CAP)))
    })
}

/// Folder peeks remembered before the map is cleared (each is a path and one entry).
const PEEK_CACHE_MAX: usize = 4096;

/// Whether formats Rust can't decode get the Windows Shell's thumbnail (grid, preview pane and
/// Quick Look). The setting (Settings → Files) turns it off; so does `ARKE_NO_SHELL_THUMBNAILS`,
/// for development and debugging, like `ARKE_NO_SHELL_HOST`.
pub(crate) fn shell_thumbnails_enabled() -> bool {
    static FORCED_OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    crate::prefs::shell_thumbnails()
        && !*FORCED_OFF.get_or_init(|| std::env::var_os("ARKE_NO_SHELL_THUMBNAILS").is_some())
}

/// Whether to ask the Shell for this entry's thumbnail (format, cloud and size rules; the local
/// disk check runs on the loader thread).
pub(crate) fn wants_shell_thumbnail(entry: &Entry) -> bool {
    can_shell_thumbnail(entry) && shell_thumbnails_enabled()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MediaKind {
    Thumbnail,
    SystemIcon,
    /// Thumbnail from the Windows Shell (formats Rust can't decode).
    ShellThumbnail,
    /// A video's thumbnail: a frame, from Windows, made in a separate process (`thumb_host.rs`).
    Video,
}

impl MediaKind {
    /// A picture of the content (shown like a photo), not an icon.
    pub(crate) const fn is_picture(self) -> bool {
        matches!(self, Self::Thumbnail | Self::ShellThumbnail | Self::Video)
    }
}

/// File path plus mtime and size: an edited file gets a new key and never a stale thumbnail.
/// Thumbnails also carry the size they are decoded at (one of [`SOURCE_SIZES`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MediaKey {
    pub(crate) path: PathBuf,
    modified: Option<SystemTime>,
    size: u64,
    pub(crate) kind: MediaKind,
    /// Longest side to decode a thumbnail at, or the system icon's size.
    px: u32,
}

impl MediaKey {
    /// Whether (and which) media this item needs, with thumbnails at the default size. With
    /// `thumbnails` false (Details view) only system icons.
    pub(crate) fn for_entry(dir: &Path, entry: &Entry, thumbnails: bool) -> Option<Self> {
        Self::sized(dir, entry, thumbnails.then_some(THUMBNAIL_SIZE))
    }

    /// Like [`Self::for_entry`], with thumbnails decoded at `thumbnail_px` (`None`: no
    /// thumbnails). `entry.name` may be a nested tree row's relative path.
    pub(crate) fn sized(dir: &Path, entry: &Entry, thumbnail_px: Option<u32>) -> Option<Self> {
        Self::at(dir.join(&*entry.name), entry, thumbnail_px)
    }

    /// Like [`Self::sized`], with system icons asked for at `icon_px` (the grid's drawn size even
    /// when thumbnails are off).
    pub(crate) fn sized_icons(
        dir: &Path,
        entry: &Entry,
        thumbnail_px: Option<u32>,
        icon_px: u32,
    ) -> Option<Self> {
        Self::with_icon(dir.join(&*entry.name), entry, thumbnail_px, icon_px)
    }

    /// Key for the file at `path` (whose listing entry is `entry`). System icons come at the
    /// thumbnail size, or [`LIST_ICON_PX`] without one.
    pub(crate) fn at(path: PathBuf, entry: &Entry, thumbnail_px: Option<u32>) -> Option<Self> {
        let icon_px = thumbnail_px.unwrap_or(LIST_ICON_PX);
        Self::with_icon(path, entry, thumbnail_px, icon_px)
    }

    /// The Shell's own thumbnail of a folder (the folder icon with its pictures inside, drawn by
    /// Windows the way File Explorer shows it). `entry` is the folder's listing entry.
    pub(crate) const fn shell_folder(path: PathBuf, entry: &Entry, px: u32) -> Self {
        Self {
            path,
            modified: entry.modified,
            size: entry.size,
            kind: MediaKind::ShellThumbnail,
            px,
        }
    }

    fn with_icon(
        path: PathBuf,
        entry: &Entry,
        thumbnail_px: Option<u32>,
        icon_px: u32,
    ) -> Option<Self> {
        let thumbs = thumbnail_px.is_some();
        let (kind, px) = if wants_system_icon(entry) {
            (MediaKind::SystemIcon, icon_px.max(16))
        } else if thumbs && can_thumbnail(entry) {
            (MediaKind::Thumbnail, thumbnail_px?)
        } else if thumbs && can_video_thumbnail(entry) && shell_thumbnails_enabled() {
            // Videos: Windows' frame from a separate process (a broken codec can't take Arke down).
            (MediaKind::Video, thumbnail_px?)
        } else if thumbs && wants_shell_thumbnail(entry) {
            (MediaKind::ShellThumbnail, thumbnail_px?)
        } else if !crate::prefs::system_type_icons() {
            // Windows type icons replace every drawn kind icon when on; otherwise only types the
            // theme has no icon for get the one their program registered.
            return Self::type_icon(entry);
        } else {
            return None;
        };
        Some(Self {
            path,
            modified: entry.modified,
            size: entry.size,
            kind,
            px,
        })
    }

    /// Key for the icon Windows has for `entry`'s type ([`wants_type_icon`]): one per
    /// extension, shared by every file of that type. Reads no file, so also for folders where
    /// thumbnails and file icons aren't loaded (network shares).
    pub(crate) fn type_icon(entry: &Entry) -> Option<Self> {
        wants_type_icon(entry).then(|| Self {
            path: PathBuf::from(format!("{TYPE_ICON_PREFIX}{}", entry.extension())),
            modified: None,
            size: 0,
            kind: MediaKind::SystemIcon,
            px: 0,
        })
    }

    /// The extension of a [`Self::type_icon`] key.
    fn type_icon_extension(&self) -> Option<&str> {
        self.path.to_str()?.strip_prefix(TYPE_ICON_PREFIX)
    }

    fn thumb_key(&self) -> ThumbKey {
        ThumbKey {
            path: self.path.clone(),
            modified: self.modified,
            size: self.size,
            px: self.px,
        }
    }
}

/// A folder as of its last change (adding, removing or renaming a file updates its mtime, so the
/// peek is looked up again).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PeekKey {
    pub(crate) dir: PathBuf,
    modified: Option<SystemTime>,
}

impl PeekKey {
    /// For a folder item in `parent`'s listing; `None` for files and cloud-only folders (reading
    /// them would download).
    pub(crate) fn for_entry(parent: &Path, entry: &Entry) -> Option<Self> {
        (entry.is_dir_like() && !entry.is_cloud_placeholder()).then(|| Self {
            dir: parent.join(&*entry.name),
            modified: entry.modified,
        })
    }
}

/// The first file (by name, hidden files skipped) in `dir`'s own listing that has a thumbnail.
fn first_picture(fs: &dyn Fs, dir: &Path) -> Option<Entry> {
    let shell = shell_thumbnails_enabled();
    let mut files: Vec<Entry> = fs
        .read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| {
            !e.is_hidden()
                && (can_thumbnail(e)
                    || (shell && can_shell_thumbnail(e) && shell_thumbnail_ext(e.extension())))
        })
        .collect();
    SortSpec::default().sort(&mut files);
    files.into_iter().next()
}

/// A file type (lowercase extension, `""` for none) or a plain folder, at list (32 px) or large
/// (up to 256 px) size.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TypeIconKey {
    ext: Box<str>,
    dir: bool,
    large: bool,
}

impl TypeIconKey {
    /// For a file or folder item (a link counts as its target's kind; the view draws the link
    /// badge over it).
    pub(crate) fn for_entry(entry: &Entry, large: bool) -> Self {
        use arke_core::EntryKind;
        let dir = match entry.kind {
            EntryKind::Dir => true,
            EntryKind::File => false,
            EntryKind::Link { is_dir } => is_dir,
        };
        Self {
            ext: if dir {
                "".into()
            } else {
                entry.extension().into()
            },
            dir,
            large,
        }
    }

    /// A name with this key's extension, for the Shell lookup.
    fn probe_name(&self) -> String {
        if self.dir {
            "folder".to_owned()
        } else if self.ext.is_empty() {
            "file".to_owned()
        } else {
            format!("file.{}", self.ext)
        }
    }
}

thread_local! {
    /// Images that are type icons (drawn at icon size, never stretched like thumbnails).
    static TYPE_ICON_IDS: std::cell::RefCell<HashSet<gpui::ImageId>> =
        std::cell::RefCell::new(HashSet::new());
}

/// Whether `image` is a Shell type icon from [`MediaStore::type_icon`].
pub(crate) fn is_type_icon(image: &RenderImage) -> bool {
    TYPE_ICON_IDS.with(|ids| ids.borrow().contains(&image.id))
}

enum MediaEvent {
    TypeIcon(TypeIconKey, Option<Thumbnail>),
    Loaded(MediaKey, Option<Thumbnail>),
    /// The thumbnail host couldn't be asked: nothing is kept, so the item is asked again the next
    /// time it's wanted (by then the host may start).
    Unavailable(MediaKey),
    LocalDisk(PathBuf, bool),
    Peeked(PeekKey, Option<Entry>),
}

struct GlobalMedia(Entity<MediaStore>);

impl Global for GlobalMedia {}

pub(crate) struct MediaStore {
    services: Services,
    /// `None` means loading failed (not retried until the file changes)
    cache: HashMap<MediaKey, Option<Arc<RenderImage>>>,
    /// Recency order (oldest first)
    order: VecDeque<MediaKey>,
    bytes: usize,
    thumbs: Loader<MediaKey>,
    icons: Loader<MediaKey>,
    shell_thumbs: Loader<MediaKey>,
    peeker: Loader<PeekKey>,
    type_icon_loader: Loader<TypeIconKey>,
    /// Shell icons per file type (`None`: the Shell had none). Few and small, never evicted.
    type_icons: HashMap<TypeIconKey, Option<Arc<RenderImage>>>,
    /// Each folder's first picture (`None`: it has none, or it couldn't be read)
    peeks: HashMap<PeekKey, Option<Entry>>,
    videos: Loader<MediaKey>,
    last_wanted: Vec<MediaKey>,
    /// Wanted by the main list (in priority order)
    main_wanted: Vec<MediaKey>,
    /// Wanted by the side column (column view's parent), queued after the main list
    side_wanted: Vec<MediaKey>,
    /// Wanted by the preview pane's folder pictures, queued last
    aux_wanted: Vec<MediaKey>,
    local_disk: HashMap<PathBuf, bool>,
    local_pending: HashSet<PathBuf>,
    tx: flume::Sender<MediaEvent>,
    _task: Task<()>,
}

/// Drive root (`C:\`); `None` for UNC or relative paths (never thumbnailed).
fn drive_root(path: &Path) -> Option<PathBuf> {
    match path.components().next()? {
        Component::Prefix(p) if matches!(p.kind(), std::path::Prefix::Disk(_)) => Some(
            PathBuf::from(format!("{}\\", p.as_os_str().to_string_lossy())),
        ),
        _ => None,
    }
}

pub(crate) fn to_render_image(t: Thumbnail) -> Option<Arc<RenderImage>> {
    let mut bgra = t.rgba;
    for p in bgra.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(t.width, t.height, bgra)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

impl MediaStore {
    /// Get the global media cache, creating it on first use.
    pub(crate) fn global(services: &Services, cx: &mut App) -> Entity<Self> {
        if let Some(g) = cx.try_global::<GlobalMedia>() {
            return g.0.clone();
        }
        let services = services.clone();
        let store = cx.new(|cx| Self::new(services, cx));
        cx.set_global(GlobalMedia(store.clone()));
        store
    }

    #[expect(
        clippy::too_many_lines,
        reason = "wires every loader (thumbnails, shell thumbnails, videos, icons) in one place"
    )]
    fn new(services: Services, cx: &mut Context<Self>) -> Self {
        let (tx, rx) = flume::unbounded::<MediaEvent>();
        let thumbs = {
            let fs = Arc::clone(&services.fs);
            let tx = tx.clone();
            Loader::new("thumbnail", 3, move |key: &MediaKey| {
                #[cfg(test)]
                THUMBNAIL_LOADS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let thumb = thumb_cache().load(&key.thumb_key(), || {
                    fs.read(&key.path, MAX_THUMBNAIL_SOURCE)
                        .ok()
                        .and_then(|bytes| decode_thumbnail(&bytes, key.px.max(1)).ok())
                });
                let _ = tx.send(MediaEvent::Loaded(key.clone(), thumb));
            })
        };
        let icons = {
            let shell = Arc::clone(&services.shell);
            let tx = tx.clone();
            Loader::new("system-icon", 1, move |key: &MediaKey| {
                let icon = match key.type_icon_extension() {
                    Some(ext) => shell.registered_type_icon(ext),
                    None => shell.file_icon(&key.path, key.px.max(16)),
                };
                let _ = tx.send(MediaEvent::Loaded(key.clone(), icon));
            })
        };
        let shell_thumbs = {
            let shell = Arc::clone(&services.shell);
            let tx = tx.clone();
            Loader::new("shell-thumbnail", 2, move |key: &MediaKey| {
                // Local fixed disks only, like every thumbnail: a provider reads the whole file.
                let thumb = shell
                    .is_local_disk(&key.path)
                    .then(|| {
                        thumb_cache().load(&key.thumb_key(), || {
                            shell.shell_thumbnail(&key.path, key.px.max(1))
                        })
                    })
                    .flatten();
                let _ = tx.send(MediaEvent::Loaded(key.clone(), thumb));
            })
        };
        let peeker = {
            let fs = Arc::clone(&services.fs);
            let tx = tx.clone();
            Loader::new("folder-peek", 1, move |key: &PeekKey| {
                let first = first_picture(fs.as_ref(), &key.dir);
                let _ = tx.send(MediaEvent::Peeked(key.clone(), first));
            })
        };
        let type_icon_loader = {
            let shell = Arc::clone(&services.shell);
            let tx = tx.clone();
            Loader::new("type-icon", 1, move |key: &TypeIconKey| {
                let icon = shell.type_icon(&key.probe_name(), key.dir, key.large);
                let _ = tx.send(MediaEvent::TypeIcon(key.clone(), icon));
            })
        };
        let videos = {
            let shell = Arc::clone(&services.shell);
            let tx = tx.clone();
            // Videos Windows had no thumbnail for: not asked again this run (each try can take
            // seconds, and a broken file stays broken). A changed file has a new key. Not when the
            // host couldn't be asked at all: that's not the file's doing.
            let failed: Arc<parking_lot::Mutex<HashSet<ThumbKey>>> = Arc::default();
            // One at a time: the host makes them one by one anyway.
            Loader::new("video-thumbnail", 1, move |key: &MediaKey| {
                let thumb_key = key.thumb_key();
                let thumb = if failed.lock().contains(&thumb_key) {
                    None
                } else {
                    let mut unavailable = false;
                    let made = thumb_cache().load(&thumb_key, || {
                        shell
                            .video_thumbnail(&key.path, key.px.max(1))
                            .unwrap_or_else(|_| {
                                unavailable = true;
                                None
                            })
                    });
                    if unavailable {
                        let _ = tx.send(MediaEvent::Unavailable(key.clone()));
                        return;
                    }
                    if made.is_none() {
                        failed.lock().insert(thumb_key);
                    }
                    made
                };
                let _ = tx.send(MediaEvent::Loaded(key.clone(), thumb));
            })
        };
        thumb_cache().count_in_background();
        let task = cx.spawn(async move |this, cx| {
            while let Ok(event) = rx.recv_async().await {
                if this.update(cx, |this, cx| this.handle(event, cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            services,
            cache: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            thumbs,
            icons,
            shell_thumbs,
            peeker,
            type_icon_loader,
            type_icons: HashMap::new(),
            peeks: HashMap::new(),
            videos,
            last_wanted: Vec::new(),
            main_wanted: Vec::new(),
            side_wanted: Vec::new(),
            aux_wanted: Vec::new(),
            local_disk: HashMap::new(),
            local_pending: HashSet::new(),
            tx,
            _task: task,
        }
    }

    fn handle(&mut self, event: MediaEvent, cx: &mut Context<Self>) {
        match event {
            MediaEvent::TypeIcon(key, icon) => {
                self.type_icon_loader.acknowledge(&key);
                let image = icon.and_then(to_render_image);
                if let Some(image) = &image {
                    TYPE_ICON_IDS.with(|ids| ids.borrow_mut().insert(image.id));
                }
                self.type_icons.insert(key, image);
            }
            MediaEvent::Loaded(key, thumb) => {
                // Tell the queue the result was taken (it's redone only if evicted and requested
                // again).
                self.loader(key.kind).acknowledge(&key);
                self.insert(key, thumb);
                self.evict(cx);
            }
            MediaEvent::Unavailable(key) => {
                self.videos.acknowledge(&key);
                // Asked again with the next change of what's wanted.
                self.last_wanted.clear();
            }
            MediaEvent::Peeked(key, first) => {
                self.peeker.acknowledge(&key);
                if self.peeks.len() >= PEEK_CACHE_MAX {
                    self.peeks.clear();
                }
                self.peeks.insert(key, first);
            }
            MediaEvent::LocalDisk(root, local) => {
                self.local_pending.remove(&root);
                self.local_disk.insert(root, local);
            }
        }
        cx.notify();
    }

    const fn loader(&self, kind: MediaKind) -> &Loader<MediaKey> {
        match kind {
            MediaKind::Thumbnail => &self.thumbs,
            MediaKind::SystemIcon => &self.icons,
            MediaKind::ShellThumbnail => &self.shell_thumbs,
            MediaKind::Video => &self.videos,
        }
    }

    fn insert(&mut self, key: MediaKey, thumb: Option<Thumbnail>) {
        let size = thumb.as_ref().map_or(0, |t| t.rgba.len());
        let image = thumb.and_then(to_render_image);
        if key.kind == MediaKind::SystemIcon
            && let Some(image) = &image
        {
            // Drawn at icon size like type icons, whatever its pixel size.
            TYPE_ICON_IDS.with(|ids| ids.borrow_mut().insert(image.id));
        }
        if self.cache.insert(key.clone(), image).is_none() {
            self.order.push_back(key);
            self.bytes += size;
        }
    }

    /// Frees the least recently used decoded images over the budget, except the wanted ones.
    fn evict(&mut self, cx: &mut App) {
        if self.bytes <= CACHE_BUDGET {
            return;
        }
        let wanted: HashSet<&MediaKey> = self.last_wanted.iter().collect();
        let mut kept = VecDeque::new();
        while self.bytes > CACHE_BUDGET {
            let Some(key) = self.order.pop_front() else {
                break;
            };
            if wanted.contains(&key) {
                kept.push_back(key);
                continue;
            }
            if let Some(Some(image)) = self.cache.remove(&key) {
                self.bytes = self.bytes.saturating_sub(image_bytes(&image));
                TYPE_ICON_IDS.with(|ids| ids.borrow_mut().remove(&image.id));
                // Remove from every window's GPU atlas; dropping the reference alone leaves the
                // texture in video memory.
                cx.drop_image(image, None);
            }
        }
        // Still on screen: back at the old end, in the same order.
        while let Some(key) = kept.pop_back() {
            self.order.push_front(key);
        }
    }

    /// Loaded thumbnails and icons (for the benchmark report).
    pub(crate) fn loaded_count(&self) -> usize {
        self.cache.values().filter(|v| v.is_some()).count()
    }

    /// Loaded entries (tests).
    #[cfg(test)]
    pub(crate) fn loaded(&self) -> Vec<(PathBuf, MediaKind)> {
        let mut v: Vec<_> = self
            .cache
            .iter()
            .filter(|(_, img)| img.is_some())
            .map(|(k, _)| (k.path.clone(), k.kind))
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    /// Frees every decoded image and forgets what was wanted, like scrolling far away and back.
    /// Used when the last window closes, and by tests.
    pub(crate) fn drop_decoded(&mut self, cx: &mut App) {
        for (_, image) in self.cache.drain() {
            if let Some(image) = image {
                cx.drop_image(image, None);
            }
        }
        self.cache.shrink_to_fit();
        self.order = VecDeque::new();
        self.bytes = 0;
        self.main_wanted = Vec::new();
        self.side_wanted = Vec::new();
        self.last_wanted = Vec::new();
    }

    /// Decoded bytes held (tests).
    #[cfg(test)]
    pub(crate) const fn decoded_bytes(&self) -> usize {
        self.bytes
    }

    /// With no window open: frees decoded images and the in-memory compressed thumbnails. The
    /// disk cache stays, so the next window gets its thumbnails back without decoding again.
    pub(crate) fn release_all(cx: &mut App) {
        if let Some(store) = cx.try_global::<GlobalMedia>().map(|g| g.0.clone()) {
            store.update(cx, |store, cx| store.drop_decoded(cx));
        }
        thumb_cache().release_memory();
    }

    /// Loaded image (`None` if failed or not loaded yet).
    pub(crate) fn get(&self, key: &MediaKey) -> Option<Arc<RenderImage>> {
        self.cache.get(key).cloned().flatten()
    }

    /// Whether this key finished loading: `Some(image)` loaded, `Some(None)` failed, `None` not
    /// loaded yet.
    pub(crate) fn settled(&self, key: &MediaKey) -> Option<&Option<Arc<RenderImage>>> {
        self.cache.get(key)
    }

    /// Like [`Self::get`], but while a thumbnail isn't loaded at this size yet, the same file's
    /// thumbnail at the closest other size (after an icon size change the grid keeps its pictures
    /// instead of blinking to type icons).
    pub(crate) fn get_or_nearby(&self, key: &MediaKey) -> Option<Arc<RenderImage>> {
        if let Some(found) = self.cache.get(key) {
            return found.clone();
        }
        let mut others: Vec<u32> = SOURCE_SIZES
            .into_iter()
            .chain([LIST_ICON_PX])
            .filter(|&s| s != key.px)
            .collect();
        others.sort_by_key(|&s| s.abs_diff(key.px));
        let mut probe = key.clone();
        others.into_iter().find_map(|px| {
            probe.px = px;
            self.cache.get(&probe).cloned().flatten()
        })
    }

    /// Whether this folder can get thumbnails and system icons. The first time a drive is seen this
    /// returns false, and a redraw follows once the background check finishes (drives are never
    /// probed on the UI thread).
    pub(crate) fn media_allowed(&mut self, dir: &Path) -> bool {
        let Some(root) = drive_root(dir) else {
            return false;
        };
        if let Some(&local) = self.local_disk.get(&root) {
            return local;
        }
        if self.local_pending.insert(root.clone()) {
            let shell = Arc::clone(&self.services.shell);
            let tx = self.tx.clone();
            let spawned = std::thread::Builder::new()
                .name("local-disk".into())
                .spawn(move || {
                    let local = shell.is_local_disk(&root);
                    let _ = tx.send(MediaEvent::LocalDisk(root, local));
                });
            if let Err(err) = spawned {
                log::error!("cannot spawn thread: {err}");
            }
        }
        false
    }

    /// Already probed and known to be a network location (thumbnails there load only with "Show
    /// thumbnails on network locations" on). Still-probing locations count as not remote (redraws
    /// when done).
    pub(crate) fn known_remote(&mut self, dir: &Path) -> bool {
        match drive_root(dir) {
            // Drive letter: probed and not local (mapped network drive)
            Some(root) => self.local_disk.get(&root) == Some(&false),
            // \\server\share is always remote
            None => matches!(
                dir.components().next(),
                Some(Component::Prefix(p))
                    if matches!(p.kind(), std::path::Prefix::UNC(..) | std::path::Prefix::VerbatimUNC(..))
            ),
        }
    }

    /// Items the main list needs now (in priority order). Already cached ones are skipped; ones
    /// with a compressed copy in memory are decoded right away while that stays within a few
    /// milliseconds, so scrolling back shows them in the same frame.
    pub(crate) fn want(&mut self, keys: Vec<MediaKey>, cx: &mut Context<Self>) {
        if keys == self.main_wanted {
            return;
        }
        let start = Instant::now();
        let mut decoded = false;
        for key in &keys {
            if start.elapsed() > SYNC_DECODE {
                break;
            }
            if !key.kind.is_picture() || self.cache.contains_key(key) {
                continue;
            }
            if let Some(thumb) = thumb_cache()
                .in_memory(&key.thumb_key())
                .and_then(|bytes| arke_core::thumb_cache::decode(&bytes))
            {
                self.insert(key.clone(), Some(thumb));
                decoded = true;
            }
        }
        self.main_wanted = keys;
        self.apply_wanted();
        // Not now: this runs while a window draws, and `drop_image` skips the window being
        // updated, so its GPU atlas would keep every freed thumbnail.
        if decoded {
            let this = cx.entity();
            cx.defer(move |cx| this.update(cx, |this, cx| this.evict(cx)));
        }
    }

    /// Items for the side column: queued after the main list and never cancel it.
    pub(crate) fn want_side(&mut self, keys: Vec<MediaKey>) {
        if keys == self.side_wanted {
            return;
        }
        self.side_wanted = keys;
        self.apply_wanted();
    }

    /// Items for the preview pane's folder pictures: queued after both lists.
    pub(crate) fn want_aux(&mut self, keys: Vec<MediaKey>) {
        if keys == self.aux_wanted {
            return;
        }
        self.aux_wanted = keys;
        self.apply_wanted();
    }

    /// Type icons for these items (in priority order), each `None` until loaded or when the Shell
    /// has none. Missing ones are queued.
    pub(crate) fn type_icons(
        &mut self,
        keys: &[Option<TypeIconKey>],
    ) -> Vec<Option<Arc<RenderImage>>> {
        let mut missing: Vec<TypeIconKey> = Vec::new();
        let found = keys
            .iter()
            .map(|key| {
                let key = key.as_ref()?;
                if let Some(image) = self.type_icons.get(key) {
                    return image.clone();
                }
                if !missing.contains(key) {
                    missing.push(key.clone());
                }
                None
            })
            .collect();
        if !missing.is_empty() {
            self.type_icon_loader.set_wanted(missing);
        }
        found
    }

    /// Folders whose first picture the Icons view needs now (in priority order).
    pub(crate) fn want_peeks(&mut self, keys: Vec<PeekKey>) {
        let missing: Vec<PeekKey> = keys
            .into_iter()
            .filter(|k| !self.peeks.contains_key(k))
            .collect();
        self.peeker.set_wanted(missing);
    }

    /// The folder's first picture once looked up (`None` while looking or when it has none).
    pub(crate) fn peek(&self, key: &PeekKey) -> Option<&Entry> {
        self.peeks.get(key)?.as_ref()
    }

    fn apply_wanted(&mut self) {
        let keys: Vec<MediaKey> = self
            .main_wanted
            .iter()
            .chain(self.side_wanted.iter())
            .chain(self.aux_wanted.iter())
            .cloned()
            .collect();
        if keys == self.last_wanted {
            return;
        }
        let (mut thumbs, mut icons, mut shell, mut videos) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for key in &keys {
            if self.cache.contains_key(key) {
                continue;
            }
            match key.kind {
                MediaKind::Thumbnail => thumbs.push(key.clone()),
                MediaKind::SystemIcon => icons.push(key.clone()),
                MediaKind::ShellThumbnail => shell.push(key.clone()),
                MediaKind::Video => videos.push(key.clone()),
            }
        }
        self.thumbs.set_wanted(thumbs);
        self.icons.set_wanted(icons);
        self.shell_thumbs.set_wanted(shell);
        self.videos.set_wanted(videos);
        // Move used entries to the recent end so visible thumbnails aren't evicted.
        for key in &keys {
            if self.cache.get(key).is_some_and(Option::is_some)
                && let Some(pos) = self.order.iter().position(|k| k == key)
                && let Some(k) = self.order.remove(pos)
            {
                self.order.push_back(k);
            }
        }
        self.last_wanted = keys;
    }
}

fn image_bytes(image: &RenderImage) -> usize {
    let size = image.size(0);
    (size.width.0 as usize) * (size.height.0 as usize) * 4
}
