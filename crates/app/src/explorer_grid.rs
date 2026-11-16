//! Icons view layout, File Explorer style: one algorithm that drawing, scrolling, keyboard steps,
//! clicks and the marquee all read, so a tile is hit exactly where it is drawn.
//!
//! - Columns ([`Cols`]): `n = ⌊avail ÷ cell⌋`, then `gap = (avail − n·cell) ÷ (n + 1)`. The same gap
//!   sits before the first column, between columns and after the last, so a row always fills the
//!   width evenly. When the gap would be under [`GAP_MIN`], one column fewer and again.
//! - Names wrap onto up to [`MAX_LINES`] lines (the last one ellipsized). Each row is as tall as its
//!   tallest tile; a tile itself is only as tall as its own name, so its highlight hugs it.
//! - The list is a `uniform_list` of thin [`SLOT`]-high items: a row (or a section header when
//!   grouping) takes whole slots and is drawn in its first slot, overflowing downward. A row whose
//!   first slot is above the visible range is drawn from the first visible slot, shifted up.
//! - The layout is built once per listing, icon size, width and grouping, and reused on every
//!   scroll frame ([`super::Explorer::grid_layout`]). The wrapped line count of each name is cached
//!   separately, since it doesn't depend on the window width.

use std::ops::Range;
use std::sync::Arc;

use gpui::{Bounds, Pixels, SharedString, TextSystem, point, px, size};

use crate::theme;

/// Height of one list item: rows and headers take whole slots.
pub(crate) const SLOT: Pixels = px(4.);
/// Narrowest gap between tiles (and before the first, after the last).
pub(crate) const GAP_MIN: f32 = 2.;
/// Most lines a name wraps onto; the last one ends in "…" when the name is longer.
pub(crate) const MAX_LINES: u8 = 3;
/// Font size and line height of tile names.
pub(crate) const LABEL_SIZE: Pixels = theme::FS_13;
pub(crate) const LABEL_LINE: Pixels = px(17.);
/// Space above the image, between image and name, and under the name.
pub(crate) const PAD_TOP: Pixels = theme::S_4;
pub(crate) const PAD_MID: Pixels = theme::S_4;
pub(crate) const PAD_BOTTOM: Pixels = theme::S_6;
/// Side padding inside a tile; names wrap at the tile width less twice this.
pub(crate) const PAD_SIDE: Pixels = theme::S_4;
/// Height of a section header in the grouped grid.
pub(crate) const HEADER_H: Pixels = px(36.);
/// Space between the list's top and the first row.
pub(crate) const TOP: Pixels = theme::S_4;

/// Columns across the list: how many, how wide, and the even gap around them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Cols {
    pub(crate) n: usize,
    pub(crate) cell: Pixels,
    pub(crate) gap: Pixels,
}

impl Cols {
    /// Fits tiles `cell` wide into `avail`, File Explorer style (see the module docs).
    pub(crate) fn fit(avail: Pixels, cell: Pixels) -> Self {
        let (avail, w) = (f32::from(avail).max(0.), f32::from(cell).max(1.));
        let mut n = ((avail / w).floor() as usize).max(1);
        loop {
            let gap = (n as f32).mul_add(-w, avail) / (n as f32 + 1.);
            if gap >= GAP_MIN || n == 1 {
                return Self {
                    n,
                    cell,
                    // A single tile wider than the list still starts at the minimum gap.
                    gap: px(gap.max(GAP_MIN)),
                };
            }
            n -= 1;
        }
    }

    /// Distance from one column's left edge to the next.
    pub(crate) fn pitch(self) -> Pixels {
        self.cell + self.gap
    }

    /// Left edge of column `c`.
    pub(crate) fn x(self, c: usize) -> Pixels {
        self.gap + self.pitch() * c as f32
    }

    /// The column whose tile covers `x`, if any (the gaps belong to no column).
    #[cfg(test)]
    pub(crate) fn at(self, x: Pixels) -> Option<usize> {
        let rel = x - self.gap;
        if rel < px(0.) {
            return None;
        }
        let c = (rel / self.pitch()).floor() as usize;
        (c < self.n && (rel - self.pitch() * c as f32) < self.cell).then_some(c)
    }

    /// Columns whose tiles overlap `x0..=x1`.
    pub(crate) fn span(self, x0: Pixels, x1: Pixels) -> Option<(usize, usize)> {
        let first = (0..self.n).find(|&c| self.x(c) + self.cell >= x0)?;
        let last = (0..self.n).rev().find(|&c| self.x(c) <= x1)?;
        (first <= last).then_some((first, last))
    }
}

/// What a band of slots holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Band {
    Header {
        label: SharedString,
        start: usize,
        count: usize,
    },
    /// A row of tiles (consecutive items).
    Row(Range<usize>),
}

/// A band and where it sits.
#[derive(Clone, Debug)]
pub(crate) struct Placed {
    pub(crate) band: Band,
    /// First slot.
    pub(crate) slot: usize,
    /// Slots it takes.
    pub(crate) slots: usize,
}

impl Placed {
    pub(crate) fn height(&self) -> Pixels {
        SLOT * self.slots as f32
    }

    /// Top edge in list content coordinates.
    pub(crate) fn top(&self) -> Pixels {
        TOP + SLOT * self.slot as f32
    }
}

/// What a layout was built from; a new one is built when any of it changes.
#[derive(Clone, PartialEq)]
pub(crate) struct LayoutKey {
    /// The listing: its address and length.
    pub(crate) listing: (usize, usize),
    pub(crate) image: Pixels,
    pub(crate) cols: Cols,
    /// Sections when grouping (see `explorer_groups.rs`).
    pub(crate) groups: Option<super::groups::GridKey>,
    /// Address of the line counts it used.
    pub(crate) lines: usize,
}

/// What the line counts were measured for.
#[derive(Clone, PartialEq)]
pub(crate) struct LinesKey {
    pub(crate) listing: (usize, usize),
    pub(crate) width: Pixels,
    pub(crate) show_ext: bool,
    pub(crate) font: SharedString,
}

/// The whole Icons view, laid out.
pub(crate) struct GridLayout {
    pub(crate) key: LayoutKey,
    pub(crate) cols: Cols,
    pub(crate) tile: theme::Tile,
    pub(crate) bands: Vec<Placed>,
    /// Band of each item.
    band_of: Vec<u32>,
    /// Wrapped lines of each item's name.
    lines: Arc<[u8]>,
    pub(crate) total_slots: usize,
}

/// Height of a tile whose name takes `lines` lines.
pub(crate) fn tile_height(tile: theme::Tile, lines: u8) -> Pixels {
    PAD_TOP + tile.image + PAD_MID + LABEL_LINE * f32::from(lines.max(1)) + PAD_BOTTOM
}

fn slots_for(height: Pixels) -> usize {
    (f32::from(height) / f32::from(SLOT)).ceil().max(1.) as usize
}

impl GridLayout {
    /// Lays out `lines.len()` items. `sections`: when grouping, each section as (label, first
    /// item, count), in order and covering every item.
    pub(crate) fn new(
        key: LayoutKey,
        tile: theme::Tile,
        lines: Arc<[u8]>,
        sections: Option<&[(SharedString, usize, usize)]>,
    ) -> Self {
        let cols = key.cols;
        let len = lines.len();
        let mut bands = Vec::new();
        let mut band_of = vec![0u32; len];
        let mut slot = 0usize;
        let mut push_rows = |start: usize, end: usize, bands: &mut Vec<Placed>, slot: &mut usize| {
            let mut at = start;
            while at < end {
                let stop = (at + cols.n).min(end);
                let tallest = lines[at..stop].iter().copied().max().unwrap_or(1);
                let slots = slots_for(tile_height(tile, tallest));
                let b = u32::try_from(bands.len()).unwrap_or(u32::MAX);
                band_of[at..stop].fill(b);
                bands.push(Placed {
                    band: Band::Row(at..stop),
                    slot: *slot,
                    slots,
                });
                *slot += slots;
                at = stop;
            }
        };
        match sections {
            Some(sections) => {
                let header = slots_for(HEADER_H);
                for (label, start, count) in sections {
                    bands.push(Placed {
                        band: Band::Header {
                            label: label.clone(),
                            start: *start,
                            count: *count,
                        },
                        slot,
                        slots: header,
                    });
                    slot += header;
                    push_rows(*start, (start + count).min(len), &mut bands, &mut slot);
                }
            }
            None => push_rows(0, len, &mut bands, &mut slot),
        }
        Self {
            key,
            cols,
            tile,
            bands,
            band_of,
            lines,
            total_slots: slot,
        }
    }

    /// Wrapped lines of item `ix`'s name.
    pub(crate) fn lines(&self, ix: usize) -> u8 {
        self.lines.get(ix).copied().unwrap_or(1)
    }

    /// Band holding slot `slot` (the last band past the end).
    pub(crate) fn band_at_slot(&self, slot: usize) -> Option<usize> {
        let after = self.bands.partition_point(|b| b.slot <= slot);
        after.checked_sub(1)
    }

    /// Band (row) of item `ix`.
    pub(crate) fn band_of(&self, ix: usize) -> Option<usize> {
        self.band_of.get(ix).map(|b| *b as usize)
    }

    /// First slot of item `ix`'s row, for scrolling to it.
    pub(crate) fn slot_of(&self, ix: usize) -> usize {
        self.band_of(ix)
            .and_then(|b| self.bands.get(b))
            .map_or(0, |b| b.slot)
    }

    /// Tile rows only, in order: their bands' indexes.
    fn rows(&self) -> impl DoubleEndedIterator<Item = usize> + '_ {
        self.bands
            .iter()
            .enumerate()
            .filter(|(_, b)| matches!(b.band, Band::Row(_)))
            .map(|(i, _)| i)
    }

    fn row_items(&self, band: usize) -> Range<usize> {
        match self.bands.get(band).map(|b| &b.band) {
            Some(Band::Row(r)) => r.clone(),
            _ => 0..0,
        }
    }

    /// `delta` rows down (up when negative) from item `ix`, in the same column (or the last item
    /// of a shorter row).
    pub(crate) fn step(&self, ix: usize, delta: isize) -> usize {
        let Some(band) = self.band_of(ix) else {
            return ix;
        };
        let rows: Vec<usize> = self.rows().collect();
        let Some(at) = rows.iter().position(|&b| b == band) else {
            return ix;
        };
        let col = ix - self.row_items(band).start;
        let target = at
            .saturating_add_signed(delta)
            .min(rows.len().saturating_sub(1));
        let range = self.row_items(rows[target]);
        (range.start + col).min(range.end.saturating_sub(1))
    }

    /// Rows that fit in `height` from the row of `ix`, for Page Up/Down (at least one).
    pub(crate) fn page_rows(&self, height: Pixels) -> usize {
        let typical = self
            .rows()
            .next()
            .and_then(|b| self.bands.get(b))
            .map_or(px(100.), Placed::height);
        ((height / typical).floor() as usize).max(1)
    }

    /// Item `ix`'s tile in list content coordinates (the scroll offset not applied).
    pub(crate) fn tile_bounds(&self, ix: usize) -> Option<Bounds<Pixels>> {
        let band = self.bands.get(self.band_of(ix)?)?;
        let Band::Row(range) = &band.band else {
            return None;
        };
        let c = ix - range.start;
        Some(Bounds {
            origin: point(self.cols.x(c), band.top()),
            size: size(self.cols.cell, tile_height(self.tile, self.lines(ix))),
        })
    }

    /// The item drawn under `p` (list content coordinates), if any. Clicks reach tiles through
    /// their elements, which sit at these same bounds; tests check the two agree.
    #[cfg(test)]
    pub(crate) fn hit(&self, p: gpui::Point<Pixels>) -> Option<usize> {
        let c = self.cols.at(p.x)?;
        if p.y < TOP {
            return None;
        }
        let slot = ((p.y - TOP) / SLOT).floor() as usize;
        let band = self.bands.get(self.band_at_slot(slot)?)?;
        let Band::Row(range) = &band.band else {
            return None;
        };
        let ix = range.start + c;
        (ix < range.end && self.tile_bounds(ix)?.contains(&p)).then_some(ix)
    }

    /// Items whose tiles overlap the box `x0..=x1` × `y0..=y1` (list content coordinates).
    pub(crate) fn hits_in(&self, x0: Pixels, x1: Pixels, y0: Pixels, y1: Pixels) -> Vec<usize> {
        let Some((c0, c1)) = self.cols.span(x0, x1) else {
            return Vec::new();
        };
        let slot = |y: Pixels| (((y - TOP) / SLOT).floor().max(0.)) as usize;
        let (Some(b0), Some(b1)) = (
            self.band_at_slot(slot(y0)),
            self.band_at_slot(slot(y1.max(TOP))),
        ) else {
            return Vec::new();
        };
        let mut hits = Vec::new();
        for band in &self.bands[b0..=b1.min(self.bands.len().saturating_sub(1))] {
            let Band::Row(range) = &band.band else {
                continue;
            };
            for ix in (range.start + c0..=range.start + c1).filter(|ix| range.contains(ix)) {
                if let Some(b) = self.tile_bounds(ix)
                    && b.top() <= y1
                    && b.bottom() >= y0
                {
                    hits.push(ix);
                }
            }
        }
        hits
    }
}

/// Lines each name wraps onto at `width` (1 to [`MAX_LINES`]), measured with the same wrapper
/// gpui draws the names with.
pub(crate) fn measure_lines<S: AsRef<str>>(
    text: &Arc<TextSystem>,
    font: gpui::Font,
    width: Pixels,
    names: impl Iterator<Item = S>,
) -> Arc<[u8]> {
    let mut wrapper = text.line_wrapper(font, LABEL_SIZE);
    names
        .map(|name| {
            let fragments = [gpui::LineFragment::text(name.as_ref())];
            let breaks = wrapper
                .wrap_line(&fragments, width, gpui::IndentAdjustment::default())
                .take(usize::from(MAX_LINES))
                .count();
            u8::try_from(breaks + 1)
                .unwrap_or(MAX_LINES)
                .min(MAX_LINES)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(cols: Cols) -> LayoutKey {
        LayoutKey {
            listing: (0, 0),
            image: px(88.),
            cols,
            groups: None,
            lines: 0,
        }
    }

    #[test]
    fn gap_spreads_evenly_and_drops_a_column_below_the_minimum() {
        // 4 × 100 in 450: gap (450 − 400) ÷ 5 = 10 on every side.
        let c = Cols::fit(px(450.), px(100.));
        assert_eq!((c.n, c.gap), (4, px(10.)));
        assert_eq!(c.x(0), px(10.));
        assert_eq!(c.x(3) + c.cell + c.gap, px(450.));
        // 4 × 100 in 405: gap 1 < 2, so 3 columns: (405 − 300) ÷ 4.
        let c = Cols::fit(px(405.), px(100.));
        assert_eq!((c.n, c.gap), (3, px(26.25)));
        // Narrower than one tile: still one column.
        assert_eq!(Cols::fit(px(50.), px(100.)).n, 1);
    }

    #[test]
    fn hit_testing_counts_the_gap() {
        let c = Cols::fit(px(450.), px(100.));
        assert_eq!(c.at(px(5.)), None);
        assert_eq!(c.at(px(10.)), Some(0));
        assert_eq!(c.at(px(109.)), Some(0));
        assert_eq!(c.at(px(115.)), None);
        assert_eq!(c.at(px(120.)), Some(1));
        assert_eq!(c.span(px(112.), px(125.)), Some((1, 1)));
        assert_eq!(c.span(px(0.), px(449.)), Some((0, 3)));
    }

    #[test]
    fn rows_are_as_tall_as_their_longest_name() {
        let cols = Cols::fit(px(450.), px(100.));
        let lines: Arc<[u8]> = Arc::from(vec![1u8, 3, 1, 1, 1, 2]);
        let tile = theme::TILE_LARGE;
        let grid = GridLayout::new(key(cols), tile, lines, None);
        assert_eq!(grid.bands.len(), 2);
        assert_eq!(grid.bands[0].height(), SLOT * slots_for(tile_height(tile, 3)) as f32);
        assert_eq!(grid.bands[1].height(), SLOT * slots_for(tile_height(tile, 2)) as f32);
        assert_eq!(grid.bands[1].slot, grid.bands[0].slots);
        // Drawn tile 5 (row 1, column 1) is hit where it is drawn.
        let b = grid.tile_bounds(5).expect("tile");
        assert_eq!(grid.hit(b.center()), Some(5));
        assert_eq!(grid.hit(point(b.left() - px(1.), b.top() + px(4.))), None);
        // A short name leaves blank space under it in a tall row.
        let short = grid.tile_bounds(0).expect("tile");
        assert_eq!(grid.hit(point(short.left() + px(4.), short.bottom() + px(2.))), None);
        assert_eq!(grid.hits_in(px(0.), px(449.), px(0.), px(1.)), vec![0, 1, 2, 3]);
        assert_eq!(grid.step(1, 1), 5);
        assert_eq!(grid.step(2, 1), 5);
    }

    #[test]
    fn sections_start_new_rows_under_their_header() {
        let cols = Cols::fit(px(330.), px(100.));
        assert_eq!(cols.n, 3);
        let lines: Arc<[u8]> = Arc::from(vec![1u8; 8]);
        let sections = [
            (SharedString::from("Folders"), 0, 1),
            (SharedString::from("A"), 1, 5),
            (SharedString::from("B"), 6, 2),
        ];
        let grid = GridLayout::new(key(cols), theme::TILE_LARGE, lines, Some(&sections));
        let rows: Vec<_> = grid
            .bands
            .iter()
            .filter_map(|b| match &b.band {
                Band::Row(r) => Some(r.clone()),
                Band::Header { .. } => None,
            })
            .collect();
        assert_eq!(rows, [0..1, 1..4, 4..6, 6..8]);
        // Down from a3 (column 2): the next row has two items, so a5; then column 2 of B, b2.
        assert_eq!(grid.step(3, 1), 5);
        assert_eq!(grid.step(5, 1), 7);
        assert_eq!(grid.step(7, -1), 5);
        assert_eq!(grid.step(0, -1), 0);
        assert_eq!(grid.slot_of(0), slots_for(HEADER_H));
    }
}
