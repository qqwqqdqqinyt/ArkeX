//! Embedded UI icons (simple SVGs drawn for this project).
//!
//! Each SVG is a single-color shape; GPUI draws it as a mask and `theme` picks the color.
//! Multi-color icons are stacked layers: folders and file pages (see `explorer::kind_icon`) and
//! colored place icons, a tinted fill under the line (see [`tinted`]).

use std::borrow::Cow;

use arke_core::filetype::FileKind;
use gpui::{
    AnyElement, AssetSource, Hsla, IntoElement, ParentElement, Pixels, Result, SharedString,
    Styled, div, svg,
};

use crate::theme;

pub(crate) struct Assets;

macro_rules! icons {
    ($($variant:ident => $name:literal),* $(,)?) => {
        const ICONS: &[(&str, &[u8])] =
            &[$(($name, include_bytes!(concat!("../assets/icons/", $name, ".svg")))),*];

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(crate) enum Icon {
            $($variant),*
        }

        impl Icon {
            pub(crate) const fn path(self) -> &'static str {
                match self {
                    $(Icon::$variant => concat!("icons/", $name, ".svg")),*
                }
            }
        }
    };
}

icons![
    ArrowLeft => "arrow-left",
    ArrowRight => "arrow-right",
    ArrowUp => "arrow-up",
    Refresh => "refresh",
    Bolt => "bolt",
    ChevronRight => "chevron-right",
    Drive => "drive",
    NetDrive => "net-drive",
    Close => "close",
    Plus => "plus",
    Search => "search",
    Subfolders => "subfolders",
    FolderPlus => "folder-plus",
    Cut => "cut",
    Copy => "copy",
    Paste => "paste",
    Rename => "rename",
    Trash => "trash",
    Restore => "restore",
    ViewList => "view-list",
    ViewGrid => "view-grid",
    ViewColumns => "view-columns",
    Split => "split",
    Density => "density",
    Preview => "preview",
    Cloud => "cloud",
    CloudLocal => "cloud-local",
    CloudPinned => "cloud-pinned",
    CloudSyncing => "cloud-syncing",
    CloudError => "cloud-error",
    Shelf => "shelf",
    // Symlink/junction badge over an item's icon (fill, frame, arrow)
    LinkBadgeFill => "link-badge-fill",
    LinkBadgeFrame => "link-badge-frame",
    LinkBadgeArrow => "link-badge-arrow",
    TabsTop => "tabs-top",
    TabsSide => "tabs-side",
    // File icon layers
    FolderBack => "folder-back",
    FolderFront => "folder-front",
    PageFill => "page-fill",
    PageLine => "page-line",
    // Type glyphs on the file page
    FtImage => "ft-image",
    FtVideo => "ft-video",
    FtAudio => "ft-audio",
    FtArchive => "ft-archive",
    FtCode => "ft-code",
    FtText => "ft-text",
    FtPdf => "ft-pdf",
    FtDoc => "ft-doc",
    FtSheet => "ft-sheet",
    FtSlides => "ft-slides",
    FtApp => "ft-app",
    FtShortcut => "ft-shortcut",
    FtFont => "ft-font",
    FtDisk => "ft-disk",
    // Known folders (favorites tiles)
    KfHome => "kf-home",
    KfUser => "kf-user",
    KfDesktop => "kf-desktop",
    KfDownload => "kf-download",
    KfDocuments => "kf-documents",
    KfPictures => "kf-pictures",
    KfMusic => "kf-music",
    KfVideos => "kf-videos",
    Folder => "folder",
    // Tint layers under colored place icons (`theme::place_tint`), see `tinted`
    KfHomeFill => "kf-home-fill",
    KfUserFill => "kf-user-fill",
    KfDesktopFill => "kf-desktop-fill",
    KfDownloadFill => "kf-download-fill",
    KfDocumentsFill => "kf-documents-fill",
    KfPicturesFill => "kf-pictures-fill",
    KfMusicFill => "kf-music-fill",
    KfVideosFill => "kf-videos-fill",
    CloudFill => "cloud-fill",
    DriveFill => "drive-fill",
    NetDriveFill => "net-drive-fill",
    TerminalFill => "terminal-fill",
    // Large folder art (grid, preview): back panel, paper, front flap, lit edge
    FolderLgBack => "folder-lg-back",
    FolderLgPaper => "folder-lg-paper",
    FolderLgFront => "folder-lg-front",
    FolderLgLip => "folder-lg-lip",
    // Custom window buttons
    WinMin => "win-min",
    WinMax => "win-max",
    WinRestore => "win-restore",
    WinClose => "win-close",
    // Menu check mark
    Check => "check",
    Settings => "settings",
    Menu => "menu",
    // Settings card icons
    Theme => "theme",
    Accent => "accent",
    // Context menu
    Terminal => "terminal",
    Pin => "pin",
    External => "external",
    Open => "open",
    // "Open with" / "Open with <editor>" when the app's own icon can't be read. Not `FtApp`: that
    // is a glyph drawn small inside a file page and shrinks to a dot on its own.
    OpenWith => "open-with",
    Zip => "zip",
    EyeOff => "eye-off",
    FileExt => "file-ext",
    Image => "image",
    Sort => "sort",
    // Two-tone menu icons, one layer each, stacked in one cell (see `explorer_menu::menu_glyph`)
    SortUp => "sort-up",
    SortDown => "sort-down",
    FolderPlusBody => "folder-plus-body",
    FolderPlusMark => "folder-plus-mark",
    Globe => "globe",
    Power => "power",
    Info => "info",
    Window => "window",
    // One metaphor per command, so no two neighbors in a menu or settings page look alike
    Undo => "undo",
    Redo => "redo",
    CopyPath => "copy-path",
    Font => "font",
    Update => "update",
    Feedback => "feedback",
    Tray => "tray",
    TabNew => "tab-new",
    TabOpen => "tab-open",
    // Dual pane: synchronized browsing, folder compare; saved workspaces
    Link => "link",
    Compare => "compare",
    Workspaces => "workspaces",
    Explorer => "explorer",
    WindowStart => "window-start",
    // Sidebar icons (Phosphor regular, as in Atlas; 256-unit grid)
    SbKfHome => "sb-kf-home",
    SbKfUser => "sb-kf-user",
    SbKfDesktop => "sb-kf-desktop",
    SbKfDownload => "sb-kf-download",
    SbKfDocuments => "sb-kf-documents",
    SbKfPictures => "sb-kf-pictures",
    SbKfMusic => "sb-kf-music",
    SbKfVideos => "sb-kf-videos",
    SbFolder => "sb-folder",
    SbDrive => "sb-drive",
    SbNetDrive => "sb-net-drive",
    SbCloud => "sb-cloud",
    SbTerminal => "sb-terminal",
    SbPin => "sb-pin",
    SbPlus => "sb-plus",
    SbClose => "sb-close",
    SbTabNew => "sb-tab-new",
    SbTabsTop => "sb-tabs-top",
    SbTabsSide => "sb-tabs-side",
    SbChevronRight => "sb-chevron-right",
];

impl Icon {
    /// The fill drawn under a colored place icon, if it has one.
    const fn tint_layer(self) -> Option<Self> {
        Some(match self {
            Self::KfHome => Self::KfHomeFill,
            Self::KfUser => Self::KfUserFill,
            Self::KfDesktop => Self::KfDesktopFill,
            Self::KfDownload => Self::KfDownloadFill,
            Self::KfDocuments => Self::KfDocumentsFill,
            Self::KfPictures => Self::KfPicturesFill,
            Self::KfMusic => Self::KfMusicFill,
            Self::KfVideos => Self::KfVideosFill,
            Self::Cloud => Self::CloudFill,
            Self::Drive => Self::DriveFill,
            Self::NetDrive => Self::NetDriveFill,
            Self::Terminal => Self::TerminalFill,
            _ => return None,
        })
    }
}

impl Icon {
    /// Atlas's glyph for `self` in the sidebar, when it has one.
    pub(crate) const fn sidebar(self) -> Self {
        match self {
            Self::KfHome => Self::SbKfHome,
            Self::KfUser => Self::SbKfUser,
            Self::KfDesktop => Self::SbKfDesktop,
            Self::KfDownload => Self::SbKfDownload,
            Self::KfDocuments => Self::SbKfDocuments,
            Self::KfPictures => Self::SbKfPictures,
            Self::KfMusic => Self::SbKfMusic,
            Self::KfVideos => Self::SbKfVideos,
            Self::Folder => Self::SbFolder,
            Self::Drive => Self::SbDrive,
            Self::NetDrive => Self::SbNetDrive,
            Self::Cloud => Self::SbCloud,
            Self::Terminal => Self::SbTerminal,
            Self::Pin => Self::SbPin,
            Self::Plus => Self::SbPlus,
            Self::Close => Self::SbClose,
            Self::TabNew => Self::SbTabNew,
            Self::TabsTop => Self::SbTabsTop,
            Self::TabsSide => Self::SbTabsSide,
            Self::ChevronRight => Self::SbChevronRight,
            other => other,
        }
    }
}

/// A sidebar icon as Atlas draws it: one-color Phosphor glyph in `color`.
pub(crate) fn sidebar_icon(icon: Icon, size: Pixels, color: impl Into<Hsla>) -> AnyElement {
    svg()
        .path(icon.sidebar().path())
        .size(size)
        .flex_none()
        .text_color(color.into())
        .into_any_element()
}

/// A place icon in its own color, like the known folders in File Explorer and Files: a soft
/// tinted fill under the line, both from [`theme::place_tint`]. A plain folder is the amber
/// folder; any other icon is a plain line in `color`. For the sidebar, Home and Ctrl+T; menus and
/// settings keep one-color line icons.
pub(crate) fn tinted(icon: Icon, size: Pixels, color: impl Into<Hsla>) -> AnyElement {
    if icon == Icon::Folder {
        return crate::explorer::kind_icon(FileKind::Folder, size);
    }
    let layer = |icon: Icon, color: Hsla| {
        svg()
            .absolute()
            .top_0()
            .left_0()
            .size(size)
            .path(icon.path())
            .text_color(color)
    };
    match (theme::place_tint(icon), icon.tint_layer()) {
        (Some((line, fill)), Some(under)) => div()
            .relative()
            .size(size)
            .flex_none()
            .child(layer(under, fill.into()))
            .child(layer(icon, line.into()))
            .into_any_element(),
        _ => svg()
            .path(icon.path())
            .size(size)
            .flex_none()
            .text_color(color)
            .into_any_element(),
    }
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let name = path
            .strip_prefix("icons/")
            .and_then(|p| p.strip_suffix(".svg"));
        Ok(name.and_then(|name| {
            ICONS
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, bytes)| Cow::Borrowed(*bytes))
        }))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .map(|(n, _)| SharedString::from(format!("icons/{n}.svg")))
            .filter(|p| p.starts_with(path))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every icon is drawn on the same 16-unit grid, so icons line up at any size.
    #[test]
    fn icons_share_the_16_unit_grid() {
        for (name, bytes) in ICONS {
            let svg = String::from_utf8_lossy(bytes);
            // Sidebar glyphs are Atlas's Phosphor set, drawn on its 256-unit grid.
            let grid = if name.starts_with("sb-") {
                r#"viewBox="0 0 256 256""#
            } else {
                r#"viewBox="0 0 16 16""#
            };
            assert!(svg.contains(grid), "{name}");
        }
    }

    /// A place gets a color exactly when it has a fill layer to show it with.
    #[test]
    fn tinted_places_have_a_fill_layer() {
        let places = [
            Icon::KfHome,
            Icon::KfUser,
            Icon::KfDesktop,
            Icon::KfDownload,
            Icon::KfDocuments,
            Icon::KfPictures,
            Icon::KfMusic,
            Icon::KfVideos,
            Icon::Cloud,
            Icon::Drive,
            Icon::NetDrive,
            Icon::Terminal,
            Icon::Folder,
            Icon::Copy,
        ];
        for icon in places {
            assert_eq!(
                theme::place_tint(icon).is_some(),
                icon.tint_layer().is_some(),
                "{icon:?}"
            );
        }
    }
}
