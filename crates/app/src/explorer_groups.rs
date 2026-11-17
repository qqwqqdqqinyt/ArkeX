//! Group-by view (off by default). The details view splits into sections by the sort column,
//! each with a header row.
//!
//! - Date modified uses relative buckets (today, yesterday, earlier this week, ...), then years.
//!   Size uses the same buckets as Windows. Name groups by first letter. Folders sorted first
//!   get their own section.
//! - Grouping by the sort column means sections are already contiguous, so one linear pass is
//!   enough (a few ms for tens of thousands of items). Cached by (list, sort, today).
//! - Rows mix headers and items. Selection, cursor and keyboard only see items; rows are
//!   converted only for scrolling and marquee selection. Clicking a header selects its section.
//! - Grid view groups too: its sections go to the Icons view layout (`explorer_grid.rs`), which
//!   gives each header and row of tiles its own height in thin uniform slots.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::SystemTime;

use arke_core::{Entry, SortColumn};
use gpui::{AnyElement, ClickEvent, Context, SharedString, div, prelude::*, px};
use rust_i18n::t;

use super::{Explorer, ViewMode};
use crate::a11y::A11yExt as _;
use crate::theme;

/// One row in the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Row {
    /// Section header: label, first item and item count.
    Header {
        label: SharedString,
        start: usize,
        count: usize,
    },
    /// An item, by index in the list.
    Item(usize),
}

pub(crate) struct Groups {
    key: (usize, usize, SortColumn, bool, i64),
    pub(crate) rows: Vec<Row>,
    /// Row index of each item.
    row_of: Vec<u32>,
}

/// Cached layouts, queried on every render.
#[derive(Default)]
pub(crate) struct GroupCache(
    RefCell<Option<Rc<Groups>>>,
    RefCell<Option<Rc<GridSections>>>,
);

/// Grid sections: (label, first item, count), with what they were built from.
pub(crate) type GridSections = (GridKey, Vec<(SharedString, usize, usize)>);
pub(crate) type GridKey = (usize, usize, SortColumn, bool, i64);

impl Groups {
    pub(crate) fn row_of(&self, ix: usize) -> usize {
        self.row_of.get(ix).map_or(ix, |r| *r as usize)
    }
}

/// "Today" for date buckets: local day number and the local offset from UTC in seconds.
#[derive(Clone, Copy)]
pub(crate) struct Today {
    day: i64,
    offset: i64,
}

impl Today {
    pub(crate) fn now(shell: &dyn crate::shell::Shell) -> Self {
        let now = SystemTime::now();
        let utc = unix_secs(now);
        let offset = shell.local_parts(now).map_or(0, |[y, m, d, h, mi, s]| {
            let local = days_from_civil(i64::from(y), i64::from(m), i64::from(d)) * 86_400
                + i64::from(h) * 3600
                + i64::from(mi) * 60
                + i64::from(s);
            // Round to whole minutes.
            ((local - utc) as f64 / 60.).round() as i64 * 60
        });
        Self {
            day: (utc + offset).div_euclid(86_400),
            offset,
        }
    }

    fn local_day(self, t: SystemTime) -> i64 {
        (unix_secs(t) + self.offset).div_euclid(86_400)
    }
}

fn unix_secs(t: SystemTime) -> i64 {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_secs()).unwrap_or(i64::MAX),
    }
}

/// Civil date to days since 1970-01-01 (Howard Hinnant's algorithm).
const fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Day number to (year, month).
fn year_month(days: i64) -> (i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m)
}

/// Date-modified section label.
fn date_group(t: Option<SystemTime>, today: Today) -> String {
    let Some(t) = t else {
        return t!("group.unknown").into_owned();
    };
    let day = today.local_day(t);
    let now = today.day;
    // Weeks start on Monday (0 = Monday).
    let weekday = (now + 3).rem_euclid(7);
    let week_start = now - weekday;
    let (y, m) = year_month(day);
    let (ny, nm) = year_month(now);
    let key = if day > now {
        "group.future"
    } else if day == now {
        "group.today"
    } else if day == now - 1 {
        "group.yesterday"
    } else if day >= week_start {
        "group.this_week"
    } else if day >= week_start - 7 {
        "group.last_week"
    } else if (y, m) == (ny, nm) {
        "group.this_month"
    } else if (y * 12 + m) == (ny * 12 + nm - 1) {
        "group.last_month"
    } else if y == ny {
        "group.this_year"
    } else {
        return t!("group.year", year = y).into_owned();
    };
    t!(key).into_owned()
}

/// Size section, using the same buckets as Windows.
fn size_group(entry: &Entry) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    let key = match entry.size {
        0 => "group.size_empty",
        s if s < 16 * KB => "group.size_tiny",
        s if s < MB => "group.size_small",
        s if s < 128 * MB => "group.size_medium",
        s if s < GB => "group.size_large",
        s if s < 4 * GB => "group.size_huge",
        _ => "group.size_gigantic",
    };
    t!(key).into_owned()
}

/// Name section: first ASCII letter, with one section for digits and one for everything else.
fn name_group(entry: &Entry) -> String {
    match entry.name.chars().next() {
        Some(c) if c.is_ascii_alphabetic() => c.to_ascii_uppercase().to_string(),
        Some(c) if c.is_ascii_digit() => "0–9".into(),
        _ => t!("group.other").into_owned(),
    }
}

/// Section label for an entry. `folders_apart`: folders are sorted first and form their own
/// section.
fn group_of(entry: &Entry, column: SortColumn, folders_apart: bool, today: Today) -> String {
    let dir = entry.is_dir_like();
    if dir && (folders_apart || !matches!(column, SortColumn::Modified | SortColumn::Created)) {
        // Same localized "folder" label as the Type column.
        return arke_core::format::type_label(entry);
    }
    match column {
        SortColumn::Name => name_group(entry),
        SortColumn::Modified => date_group(entry.modified, today),
        SortColumn::Type => arke_core::format::type_label(entry),
        SortColumn::Size => size_group(entry),
        SortColumn::Created => date_group(entry.created, today),
        SortColumn::Extension => match entry.extension() {
            "" => t!("group.no_extension").into_owned(),
            ext => ext.to_uppercase(),
        },
        SortColumn::Attributes => match entry.attribute_letters() {
            "" => t!("group.no_attributes").into_owned(),
            letters => letters.to_owned(),
        },
    }
}

/// Splits the list into sections, merging adjacent entries with the same label.
fn build(
    entries: &[Entry],
    column: SortColumn,
    folders_apart: bool,
    today: Today,
) -> (Vec<Row>, Vec<u32>) {
    let mut rows = Vec::with_capacity(entries.len() + 16);
    let mut row_of = Vec::with_capacity(entries.len());
    let mut current: Option<String> = None;
    let mut header_at = 0usize;
    for (ix, entry) in entries.iter().enumerate() {
        let group = group_of(entry, column, folders_apart, today);
        if current.as_deref() != Some(group.as_str()) {
            header_at = rows.len();
            rows.push(Row::Header {
                label: SharedString::from(group.clone()),
                start: ix,
                count: 0,
            });
            current = Some(group);
        }
        if let Some(Row::Header { count, .. }) = rows.get_mut(header_at) {
            *count += 1;
        }
        row_of.push(u32::try_from(rows.len()).unwrap_or(u32::MAX));
        rows.push(Row::Item(ix));
    }
    (rows, row_of)
}

impl Explorer {
    /// Whether to group: setting on, details view, not search results or column view.
    pub(super) fn grouping(&self) -> bool {
        self.group_rows
            && self.view_mode == ViewMode::Details
            && !self.search_mode
            && !self.columns_active()
            // An expanded tree folder lists its items under itself, not in sections.
            && !self.tree_expanding()
    }

    /// Sections of the grouped grid, or `None` when not grouping.
    pub(crate) fn grid_sections(&self) -> Option<Rc<GridSections>> {
        if !(self.group_rows && self.view_mode == ViewMode::Icons && !self.search_mode) {
            return None;
        }
        let view = self.view()?;
        let today = Today::now(self.services.shell.as_ref());
        let folders_apart = !self.sort.mixed;
        let key = (
            Arc::as_ptr(&view) as usize,
            view.len(),
            self.sort.column,
            folders_apart,
            today.day,
        );
        if let Some(cached) = self.group_cache.1.borrow().as_ref().filter(|g| g.0 == key) {
            return Some(Rc::clone(cached));
        }
        let (rows, _) = build(&view, self.sort.column, folders_apart, today);
        let sections = rows
            .into_iter()
            .filter_map(|row| match row {
                Row::Header {
                    label,
                    start,
                    count,
                } => Some((label, start, count)),
                Row::Item(_) => None,
            })
            .collect();
        let grid = Rc::new((key, sections));
        *self.group_cache.1.borrow_mut() = Some(Rc::clone(&grid));
        Some(grid)
    }

    /// Sections of the current list, or `None` when not grouping.
    pub(crate) fn groups(&self) -> Option<Rc<Groups>> {
        if !self.grouping() {
            return None;
        }
        let view = self.view()?;
        let today = Today::now(self.services.shell.as_ref());
        let folders_apart = !self.sort.mixed;
        let key = (
            Arc::as_ptr(&view) as usize,
            view.len(),
            self.sort.column,
            folders_apart,
            today.day,
        );
        if let Some(cached) = self
            .group_cache
            .0
            .borrow()
            .as_ref()
            .filter(|g| g.key == key)
        {
            return Some(Rc::clone(cached));
        }
        let (rows, row_of) = build(&view, self.sort.column, folders_apart, today);
        let groups = Rc::new(Groups { key, rows, row_of });
        *self.group_cache.0.borrow_mut() = Some(Rc::clone(&groups));
        Some(groups)
    }

    /// Number of rows in the list, including headers when grouping.
    pub(super) fn display_len(&self, items: usize) -> usize {
        self.groups().map_or(items, |g| g.rows.len())
    }

    /// Row of an item, for scrolling.
    pub(super) fn display_row(&self, ix: usize) -> usize {
        self.groups().map_or(ix, |g| g.row_of(ix))
    }

    /// Item at a row, for marquee selection. `None` for headers.
    pub(super) fn item_at_row(&self, row: usize) -> Option<usize> {
        match self.groups() {
            None => Some(row),
            Some(g) => match g.rows.get(row)? {
                Row::Item(ix) => Some(*ix),
                Row::Header { .. } => None,
            },
        }
    }

    /// Draws rows `range` when grouping. Headers draw themselves; runs of items use the normal
    /// rows.
    pub(super) fn render_grouped_rows(
        &mut self,
        groups: &Groups,
        range: std::ops::Range<usize>,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut out = Vec::with_capacity(range.len());
        let mut run: Option<std::ops::Range<usize>> = None;
        for row in range {
            match groups.rows.get(row) {
                Some(Row::Item(ix)) => {
                    run = Some(match run {
                        Some(r) if r.end == *ix => r.start..ix + 1,
                        Some(r) => {
                            out.extend(self.render_rows(r, focused, cx));
                            *ix..ix + 1
                        }
                        None => *ix..ix + 1,
                    });
                }
                Some(Row::Header {
                    label,
                    start,
                    count,
                }) => {
                    if let Some(r) = run.take() {
                        out.extend(self.render_rows(r, focused, cx));
                    }
                    out.push(self.group_header(row, label.clone(), *start, *count, None, cx));
                }
                None => {}
            }
        }
        if let Some(r) = run {
            out.extend(self.render_rows(r, focused, cx));
        }
        out
    }

    /// Section header with label, count and a rule. Clicking selects the section.
    pub(super) fn group_header(
        &self,
        row: usize,
        label: SharedString,
        start: usize,
        count: usize,
        height: Option<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(("group-header", row))
            .a11y_lazy(gpui::Role::Heading, || format!("{label} ({count})"))
            .aria_level(1)
            // Full width, and at least as wide as Details rows when they scroll sideways.
            .w_full()
            .when_some(self.cols, |d, c| d.min_w(c.row))
            .h(height.unwrap_or_else(|| self.row_height()))
            .px(theme::S_12)
            .flex()
            .items_end()
            .pb(theme::S_6)
            .gap(theme::S_8)
            .cursor_pointer()
            .rounded(theme::R_8)
            .hover(|s| s.bg(theme::wash()))
            // A heading, not a caption: item size, semibold, full ink.
            .child(
                div()
                    .flex_none()
                    .text_size(theme::FS_14)
                    .font_weight(theme::FW_STRONG)
                    .text_color(theme::text())
                    .child(label),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(theme::FS_12)
                    .text_color(theme::text_subtle())
                    .child(arke_core::format::format_count(count)),
            )
            .child(div().flex_1().h(px(1.)).mb(px(6.)).bg(theme::line()))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _, _| {
                // Don't start a marquee from a header.
                this.row_pressed = true;
            }))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                window.focus(&this.list_focus, cx);
                let Some(view) = this.view() else {
                    return;
                };
                // Ctrl adds to the existing selection.
                let mut names: Vec<std::sync::Arc<str>> = if event.modifiers().control {
                    this.selection.names().cloned().collect()
                } else {
                    Vec::new()
                };
                let end = (start + count).min(view.len());
                names.extend(view[start.min(end)..end].iter().map(|e| Arc::clone(&e.name)));
                this.selection.select_names(names);
                this.selection
                    .set_cursor_name(view.get(start).map(|e| Arc::clone(&e.name)));
                this.cursor = Some(start);
                cx.notify();
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use arke_core::EntryKind;
    use std::time::Duration;

    fn at(today: Today, days_ago: i64) -> SystemTime {
        let secs = today.day * 86_400 - today.offset + 12 * 3600 - days_ago * 86_400;
        SystemTime::UNIX_EPOCH + Duration::from_secs(u64::try_from(secs).unwrap())
    }

    #[test]
    fn civil_dates_round_trip() {
        for (y, m, d) in [(1970, 1, 1), (2000, 2, 29), (2026, 11, 10), (2026, 12, 31)] {
            let days = days_from_civil(y, m, d);
            assert_eq!(year_month(days), (y, m));
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }

    #[test]
    fn dates_fall_into_relative_buckets() {
        rust_i18n::set_locale("en");
        // 2026-11-10 is a Tuesday.
        let today = Today {
            day: days_from_civil(2026, 11, 10),
            offset: 8 * 3600,
        };
        let g = |ago| date_group(Some(at(today, ago)), today);
        assert_eq!(g(0), t!("group.today"));
        assert_eq!(g(1), t!("group.yesterday"));
        assert_eq!(g(3), t!("group.last_week"), "Nov 7 is last Saturday");
        assert_eq!(g(9), t!("group.this_month"));
        assert_eq!(g(20), t!("group.last_month"));
        assert_eq!(g(100), t!("group.this_year"));
        assert_eq!(g(400), t!("group.year", year = 2025));
    }

    #[test]
    fn runs_become_groups_with_counts() {
        rust_i18n::set_locale("en");
        let today = Today {
            day: 20_000,
            offset: 0,
        };
        let entries = vec![
            Entry::new("docs", EntryKind::Dir, 0, None, 0),
            Entry::new("apple.txt", EntryKind::File, 1, None, 0),
            Entry::new("avocado.txt", EntryKind::File, 1, None, 0),
            Entry::new("banana.txt", EntryKind::File, 1, None, 0),
            // A non-ASCII first letter falls into "Other".
            Entry::new("報告.docx", EntryKind::File, 1, None, 0),
        ];
        let (rows, row_of) = build(&entries, SortColumn::Name, true, today);
        let headers: Vec<(String, usize)> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Header { label, count, .. } => Some((label.to_string(), *count)),
                Row::Item(_) => None,
            })
            .collect();
        assert_eq!(
            headers,
            [
                (arke_core::format::type_label(&entries[0]), 1),
                ("A".into(), 2),
                ("B".into(), 1),
                (t!("group.other").into_owned(), 1)
            ]
        );
        assert_eq!(rows.len(), entries.len() + 4);
        // Every item maps back to its row.
        for (ix, row) in row_of.iter().enumerate() {
            assert_eq!(rows[*row as usize], Row::Item(ix));
        }
    }
}
