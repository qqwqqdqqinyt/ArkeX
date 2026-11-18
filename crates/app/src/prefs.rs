//! List display preferences (Sort and view settings): short or full dates, visible columns, their
//! widths and their order, the width the preview pane was dragged to, and the column view's
//! column widths.
//!
//! App-wide like theme and accent: changes are stored here and all windows redraw (`theme::look`
//! bumps). Rows read from here while drawing, so tabs don't each keep a copy.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use gpui::{Pixels, px};
use parking_lot::RwLock;

use crate::settings::Settings;
use crate::theme;

static FULL_DATES: AtomicBool = AtomicBool::new(false);
static SHOW_MODIFIED: AtomicBool = AtomicBool::new(true);
static SHOW_CREATED: AtomicBool = AtomicBool::new(false);
static SHOW_TYPE: AtomicBool = AtomicBool::new(true);
static SHOW_SIZE: AtomicBool = AtomicBool::new(true);
static SHOW_EXTENSION: AtomicBool = AtomicBool::new(false);
static SHOW_ATTRIBUTES: AtomicBool = AtomicBool::new(false);
static SHOW_LINK_TARGET: AtomicBool = AtomicBool::new(false);
static SINGLE_CLICK: AtomicBool = AtomicBool::new(false);
/// Files and folders get the Windows Shell's type icons instead of Arke's own.
static SYSTEM_TYPE_ICONS: AtomicBool = AtomicBool::new(true);
/// Shell thumbnails for formats Arke can't decode (PSD, RAW, HEIC).
static SHELL_THUMBNAILS: AtomicBool = AtomicBool::new(true);
/// Typing in the search box searches subfolders.
static SEARCH_SUBFOLDERS: AtomicBool = AtomicBool::new(true);

#[cfg(test)]
thread_local! {
    static TEST_SEARCH_SUBFOLDERS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Settings → Search's subfolder option for the calling test's thread only.
#[cfg(test)]
pub(crate) fn set_search_subfolders_for_test(on: bool) {
    TEST_SEARCH_SUBFOLDERS.with(|c| c.set(on));
}
/// Icons view tile width in percent of the icon size.
static TILE_RATIO: AtomicU32 = AtomicU32::new(DEFAULT_TILE_RATIO);
/// Context bar size in percent.
static BAR_SCALE: AtomicU32 = AtomicU32::new(DEFAULT_BAR_SCALE);
/// Defaults of [`TILE_RATIO`] and [`BAR_SCALE`] (tile = icon × 1.3; context bar at 1.3×).
pub(crate) const DEFAULT_TILE_RATIO: u32 = 130;
pub(crate) const DEFAULT_BAR_SCALE: u32 = 130;
/// Dragged column widths (px, 0 = default), in [`Col`] order
static WIDTHS: [AtomicU32; Col::ALL.len()] = [const { AtomicU32::new(0) }; Col::ALL.len()];
/// Dragged preview pane width (px, 0 = automatic).
static PREVIEW_W: AtomicU32 = AtomicU32::new(0);
/// Sidebar width in pixels (0: the default).
static SIDEBAR_W: AtomicU32 = AtomicU32::new(0);
/// Column view widths (px, 0 = default), in [`ViewColumn`] order.
static VIEW_COLUMN_W: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
/// The columns after Name, left to right.
static ORDER: RwLock<[Col; Col::MOVABLE.len()]> = RwLock::new(Col::MOVABLE);

/// Columns in Details view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Col {
    /// The name column, always first. Its width covers the checkbox, the tree indent and the icon
    /// too; until the user sizes it, it follows the pane (see `explorer_layout::columns`).
    Name,
    Modified,
    Created,
    Type,
    Size,
    /// The extension alone ("jpg"), for when Type names it in words.
    Extension,
    /// R/H/S/A letters.
    Attributes,
    /// Where a symlink or junction points.
    LinkTarget,
}

impl Col {
    pub(crate) const ALL: [Self; 8] = [
        Self::Name,
        Self::Modified,
        Self::Created,
        Self::Type,
        Self::Size,
        Self::Extension,
        Self::Attributes,
        Self::LinkTarget,
    ];

    /// The columns after Name in their default order. Size comes last, where it has always been;
    /// the newer ones sit between Type and Size.
    pub(crate) const MOVABLE: [Self; 7] = [
        Self::Modified,
        Self::Created,
        Self::Type,
        Self::Extension,
        Self::Attributes,
        Self::LinkTarget,
        Self::Size,
    ];

    fn slot(self) -> &'static AtomicU32 {
        &WIDTHS[self as usize]
    }

    /// Key in `settings.json` (`column_order`).
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Modified => "modified",
            Self::Created => "created",
            Self::Type => "type",
            Self::Size => "size",
            Self::Extension => "extension",
            Self::Attributes => "attributes",
            Self::LinkTarget => "link_target",
        }
    }

    pub(crate) fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == key)
    }

    /// Header label.
    pub(crate) fn label(self) -> std::borrow::Cow<'static, str> {
        use rust_i18n::t;
        match self {
            Self::Name => t!("col.name"),
            Self::Modified => t!("col.modified"),
            Self::Created => t!("col.created"),
            Self::Type => t!("col.type"),
            Self::Size => t!("col.size"),
            Self::Extension => t!("col.extension"),
            Self::Attributes => t!("col.attributes"),
            Self::LinkTarget => t!("col.link_target"),
        }
    }

    /// The sort order this column's header picks; `None` for Link target (its values are read in
    /// the background, so there's nothing to sort the listing by yet).
    pub(crate) const fn sort_column(self) -> Option<arke_core::SortColumn> {
        use arke_core::SortColumn as S;
        Some(match self {
            Self::Name => S::Name,
            Self::Modified => S::Modified,
            Self::Created => S::Created,
            Self::Type => S::Type,
            Self::Size => S::Size,
            Self::Extension => S::Extension,
            Self::Attributes => S::Attributes,
            Self::LinkTarget => return None,
        })
    }

    /// Whether the column is switched on (Settings or the header menu). Name always is.
    pub(crate) fn shown(self) -> bool {
        match self {
            Self::Name => true,
            Self::Modified => show_modified(),
            Self::Created => show_created(),
            Self::Type => show_type(),
            Self::Size => show_size(),
            Self::Extension => SHOW_EXTENSION.load(Ordering::Relaxed),
            Self::Attributes => SHOW_ATTRIBUTES.load(Ordering::Relaxed),
            Self::LinkTarget => SHOW_LINK_TARGET.load(Ordering::Relaxed),
        }
    }

    /// Whether `settings` switch the column on (what [`Self::shown`] reads once applied).
    pub(crate) const fn enabled_in(self, settings: &Settings) -> bool {
        match self {
            Self::Name => true,
            Self::Modified => settings.show_modified,
            Self::Created => settings.show_created,
            Self::Type => settings.show_type,
            Self::Size => settings.show_size,
            Self::Extension => settings.show_extension,
            Self::Attributes => settings.show_attributes,
            Self::LinkTarget => settings.show_link_target,
        }
    }

    /// The setting behind [`Self::shown`]; `None` for Name, which can't be hidden.
    pub(crate) const fn setting(self, settings: &mut Settings) -> Option<&mut bool> {
        Some(match self {
            Self::Name => return None,
            Self::Modified => &mut settings.show_modified,
            Self::Created => &mut settings.show_created,
            Self::Type => &mut settings.show_type,
            Self::Size => &mut settings.show_size,
            Self::Extension => &mut settings.show_extension,
            Self::Attributes => &mut settings.show_attributes,
            Self::LinkTarget => &mut settings.show_link_target,
        })
    }

    fn default_width(self) -> Pixels {
        match self {
            Self::Name => theme::COL_NAME_MAX,
            // Full date-times are longer.
            Self::Modified | Self::Created if full_dates() => px(180.),
            Self::Modified | Self::Created => theme::COL_MODIFIED,
            Self::Type => theme::COL_TYPE,
            Self::Size => theme::COL_SIZE,
            Self::Extension => theme::COL_EXTENSION,
            Self::Attributes => theme::COL_ATTRIBUTES,
            Self::LinkTarget => theme::COL_LINK_TARGET,
        }
    }

    /// Allowed width range
    pub(crate) fn clamp(self, width: Pixels) -> Pixels {
        let (min, max) = match self {
            Self::Name => (160., 1600.),
            _ => (56., 480.),
        };
        width.clamp(px(min), px(max))
    }
}

/// Column width (dragged value or default; Name's real default depends on the pane, see
/// [`user_width`]).
pub(crate) fn width(col: Col) -> Pixels {
    user_width(col).unwrap_or_else(|| col.default_width())
}

/// The width the user gave `col` (`None`: its default). Name has no fixed default, so layout
/// reads it from here.
pub(crate) fn user_width(col: Col) -> Option<Pixels> {
    match col.slot().load(Ordering::Relaxed) {
        0 => None,
        w => Some(px(w as f32)),
    }
}

/// Set a width while dragging (`None` resets); redraws all windows on change. Saving is up to the
/// caller (on release).
pub(crate) fn set_width(col: Col, width: Option<Pixels>) {
    let value = width.map_or(0, |w| f32::from(col.clamp(w)).round() as u32);
    if col.slot().swap(value, Ordering::Relaxed) != value {
        theme::touch();
    }
}

/// Current width for saving (`None` if never dragged).
pub(crate) fn stored_width(col: Col) -> Option<u16> {
    u16::try_from(col.slot().load(Ordering::Relaxed))
        .ok()
        .filter(|&w| w != 0)
}

/// Width the preview pane was dragged to (`None`: automatic, follows the window).
pub(crate) fn preview_width() -> Option<Pixels> {
    match PREVIEW_W.load(Ordering::Relaxed) {
        0 => None,
        w => Some(px(w as f32)),
    }
}

/// Set the preview pane width while dragging (`None` goes back to automatic); redraws all windows
/// on change. The caller clamps it and saves it on release.
pub(crate) fn set_preview_width(width: Option<Pixels>) {
    let value = width.map_or(0, |w| f32::from(w).round().max(1.) as u32);
    if PREVIEW_W.swap(value, Ordering::Relaxed) != value {
        theme::touch();
    }
}

/// Sidebar width: what it was dragged to (Atlas lets its sidebar be resized), else the default.
pub(crate) fn sidebar_width() -> Pixels {
    match SIDEBAR_W.load(Ordering::Relaxed) {
        0 => theme::SIDEBAR_W,
        w => px(w as f32),
    }
}

/// Set the sidebar width while dragging, clamped to its bounds (`None` goes back to the default);
/// redraws all windows on change. The caller saves it on release.
pub(crate) fn set_sidebar_width(width: Option<Pixels>) {
    let value = width.map_or(0, |w| {
        f32::from(w.clamp(theme::SIDEBAR_MIN, theme::SIDEBAR_MAX)).round() as u32
    });
    if SIDEBAR_W.swap(value, Ordering::Relaxed) != value {
        theme::touch();
    }
}

/// Current sidebar width for saving (`None` if never dragged or reset).
pub(crate) fn stored_sidebar_width() -> Option<u16> {
    u16::try_from(SIDEBAR_W.load(Ordering::Relaxed))
        .ok()
        .filter(|&w| w != 0)
}

/// Current preview pane width for saving (`None` if automatic).
pub(crate) fn stored_preview_width() -> Option<u16> {
    u16::try_from(PREVIEW_W.load(Ordering::Relaxed))
        .ok()
        .filter(|&w| w != 0)
}

/// The column view's resizable columns (the third one, the preview, takes what they leave).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewColumn {
    Parent,
    Current,
}

/// Width the user gave a column view column (`None`: its default).
pub(crate) fn view_column_width(col: ViewColumn) -> Option<Pixels> {
    match VIEW_COLUMN_W[col as usize].load(Ordering::Relaxed) {
        0 => None,
        w => Some(px(w as f32)),
    }
}

/// Set a column view width while dragging or fitting (the caller clamps it); redraws all windows
/// on change. Saving is up to the caller.
pub(crate) fn set_view_column_width(col: ViewColumn, width: Option<Pixels>) {
    let value = width.map_or(0, |w| f32::from(w).round().max(1.) as u32);
    if VIEW_COLUMN_W[col as usize].swap(value, Ordering::Relaxed) != value {
        theme::touch();
    }
}

/// Current column view widths for saving.
pub(crate) fn stored_view_column_widths() -> crate::settings::ColumnViewWidths {
    let get = |col: ViewColumn| {
        u16::try_from(VIEW_COLUMN_W[col as usize].load(Ordering::Relaxed))
            .ok()
            .filter(|&w| w != 0)
    };
    crate::settings::ColumnViewWidths {
        parent: get(ViewColumn::Parent),
        current: get(ViewColumn::Current),
    }
}

/// The columns after Name, left to right (shown or not).
pub(crate) fn order() -> [Col; Col::MOVABLE.len()] {
    *ORDER.read()
}

/// Reads `column_order` from settings: known keys in their saved order, then any column the list
/// doesn't mention at its default place (right after the default neighbor that precedes it).
pub(crate) fn parse_order(keys: &[String]) -> [Col; Col::MOVABLE.len()] {
    let mut order: Vec<Col> = Vec::with_capacity(Col::MOVABLE.len());
    for col in keys.iter().filter_map(|k| Col::from_key(k)) {
        if col != Col::Name && !order.contains(&col) {
            order.push(col);
        }
    }
    for (ix, col) in Col::MOVABLE.into_iter().enumerate() {
        if order.contains(&col) {
            continue;
        }
        let after = Col::MOVABLE[..ix]
            .iter()
            .rev()
            .find_map(|prev| order.iter().position(|c| c == prev));
        order.insert(after.map_or(0, |at| at + 1), col);
    }
    let mut out = Col::MOVABLE;
    out.copy_from_slice(&order);
    out
}

/// `order` with `moved` placed where `target` is: before it when moving left, after it when moving
/// right (what dragging a header onto another does). `target` Name means the first place.
pub(crate) fn reordered(
    order: [Col; Col::MOVABLE.len()],
    moved: Col,
    target: Col,
) -> [Col; Col::MOVABLE.len()] {
    let Some(from) = order.iter().position(|c| *c == moved) else {
        return order;
    };
    let to = if target == Col::Name {
        0
    } else {
        match order.iter().position(|c| *c == target) {
            Some(to) => to,
            None => return order,
        }
    };
    let mut list = order.to_vec();
    let col = list.remove(from);
    list.insert(to, col);
    let mut out = order;
    out.copy_from_slice(&list);
    out
}

/// Settings value for `order`.
pub(crate) fn order_keys(order: &[Col]) -> Vec<String> {
    order.iter().map(|c| c.key().to_owned()).collect()
}

/// Apply settings; redraws all windows on change.
pub(crate) fn apply(settings: &Settings) {
    let mut changed = false;
    for (slot, value) in [
        (&FULL_DATES, settings.full_dates),
        (&SHOW_MODIFIED, settings.show_modified),
        (&SHOW_CREATED, settings.show_created),
        (&SHOW_TYPE, settings.show_type),
        (&SHOW_SIZE, settings.show_size),
        (&SHOW_EXTENSION, settings.show_extension),
        (&SHOW_ATTRIBUTES, settings.show_attributes),
        (&SHOW_LINK_TARGET, settings.show_link_target),
        (&SYSTEM_TYPE_ICONS, settings.system_type_icons),
        (&SHELL_THUMBNAILS, settings.shell_thumbnails),
    ] {
        changed |= slot.swap(value, Ordering::Relaxed) != value;
    }
    SINGLE_CLICK.store(settings.single_click, Ordering::Relaxed);
    SEARCH_SUBFOLDERS.store(settings.search_subfolders, Ordering::Relaxed);
    let ratio = u32::from(settings.tile_width_ratio).clamp(100, 250);
    changed |= TILE_RATIO.swap(ratio, Ordering::Relaxed) != ratio;
    let bar = u32::from(settings.toolbar_scale).clamp(100, 200);
    changed |= BAR_SCALE.swap(bar, Ordering::Relaxed) != bar;
    crate::win_fast_recycle::set_enabled(settings.fast_recycle);
    for col in Col::ALL {
        let value = settings.column_widths.get(col).map_or(0, u32::from);
        changed |= col.slot().swap(value, Ordering::Relaxed) != value;
    }
    let preview = settings.preview_width.map_or(0, u32::from);
    changed |= PREVIEW_W.swap(preview, Ordering::Relaxed) != preview;
    let sidebar = settings.sidebar_width.map_or(0, |w| {
        f32::from(px(f32::from(w)).clamp(theme::SIDEBAR_MIN, theme::SIDEBAR_MAX)) as u32
    });
    changed |= SIDEBAR_W.swap(sidebar, Ordering::Relaxed) != sidebar;
    let view_columns = settings.column_view_widths;
    for (col, width) in [
        (ViewColumn::Parent, view_columns.parent),
        (ViewColumn::Current, view_columns.current),
    ] {
        let value = width.map_or(0, u32::from);
        changed |= VIEW_COLUMN_W[col as usize].swap(value, Ordering::Relaxed) != value;
    }
    let order = parse_order(&settings.column_order);
    if *ORDER.read() != order {
        *ORDER.write() = order;
        changed = true;
    }
    // Size units are set in core, where the formatting lives.
    changed |= arke_core::format::set_decimal_sizes(settings.decimal_sizes);
    if changed {
        theme::touch();
    }
}

/// Whether file and folder items show the Windows Shell's type icons (what Explorer shows for
/// the extension or folder) instead of Arke's drawn kind icons.
pub(crate) fn system_type_icons() -> bool {
    SYSTEM_TYPE_ICONS.load(Ordering::Relaxed)
}

/// Whether formats Arke can't decode get the Windows Shell's thumbnail.
pub(crate) fn shell_thumbnails() -> bool {
    SHELL_THUMBNAILS.load(Ordering::Relaxed)
}

/// Whether typing in the search box searches subfolders (else it filters the current folder).
pub(crate) fn search_subfolders() -> bool {
    // Tests run in parallel in one process and keep upstream's "this folder" typing default per
    // thread, so a test that creates a workspace (which applies the settings) can't flip another.
    #[cfg(test)]
    let on = TEST_SEARCH_SUBFOLDERS.with(std::cell::Cell::get);
    #[cfg(not(test))]
    let on = SEARCH_SUBFOLDERS.load(Ordering::Relaxed);
    on
}

/// Icons view tile width per pixel of icon size (1.3 by default).
pub(crate) fn tile_ratio() -> f32 {
    TILE_RATIO.load(Ordering::Relaxed) as f32 / 100.
}

/// Scale of the context bar under the path bar (1.3 by default).
pub(crate) fn bar_scale() -> f32 {
    BAR_SCALE.load(Ordering::Relaxed) as f32 / 100.
}

/// Show full date-times for Modified (instead of short forms like "Today 07:49").
pub(crate) fn full_dates() -> bool {
    FULL_DATES.load(Ordering::Relaxed)
}

pub(crate) fn show_modified() -> bool {
    SHOW_MODIFIED.load(Ordering::Relaxed)
}

pub(crate) fn show_created() -> bool {
    SHOW_CREATED.load(Ordering::Relaxed)
}

pub(crate) fn show_type() -> bool {
    SHOW_TYPE.load(Ordering::Relaxed)
}

pub(crate) fn show_size() -> bool {
    SHOW_SIZE.load(Ordering::Relaxed)
}

/// Open on single click (on release; select with Ctrl/Shift click, marquee or keyboard).
pub(crate) fn single_click() -> bool {
    SINGLE_CLICK.load(Ordering::Relaxed)
}

/// Modified column width: dragged value or default (full date-times are longer).
pub(crate) fn modified_width() -> Pixels {
    width(Col::Modified)
}

pub(crate) fn size_width() -> Pixels {
    width(Col::Size)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|&k| k.to_owned()).collect()
    }

    /// Saved orders load as saved; unknown keys, Name and repeats are dropped, and columns the file
    /// doesn't know yet appear at their default place.
    #[test]
    fn column_order_loads_leniently() {
        assert_eq!(parse_order(&[]), Col::MOVABLE);
        let order = parse_order(&keys(&[
            "size", "type", "name", "hologram", "type", "modified",
        ]));
        assert_eq!(
            order,
            [
                Col::Size,
                Col::Type,
                Col::Extension,
                Col::Attributes,
                Col::LinkTarget,
                Col::Modified,
                Col::Created,
            ]
        );
        let all = order_keys(&order);
        assert_eq!(parse_order(&all), order, "round trip");
    }

    /// Dragging right lands after the target, dragging left before it, onto Name goes first.
    #[test]
    fn reordering_moves_one_column() {
        let start = Col::MOVABLE;
        let right = reordered(start, Col::Modified, Col::Type);
        assert_eq!(&right[..3], &[Col::Created, Col::Type, Col::Modified]);
        let left = reordered(start, Col::Size, Col::Created);
        assert_eq!(&left[..3], &[Col::Modified, Col::Size, Col::Created]);
        let first = reordered(start, Col::LinkTarget, Col::Name);
        assert_eq!(first[0], Col::LinkTarget);
        assert_eq!(reordered(start, Col::Type, Col::Type), start);
    }
}
