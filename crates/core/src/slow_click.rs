//! Slow click to rename, the way File Explorer's list view does it.
//!
//! Clicking an item that was already the only selection, and then not clicking again for the
//! double-click time, starts renaming it. The delay is the system double-click time
//! (`GetDoubleClickTime`, 500 ms by default) so the first click of a double-click never starts a
//! rename: "a click on a selected item with no other clicks within the double-click time, either
//! before or after, is treated as a rename" (Raymond Chen, The Old New Thing, 2008-11-10 and
//! 2012-06-25; the list view sets a `WM_TIMER` instead of editing at once). Finder ties its
//! click-to-rename delay to the double-click speed too.
//!
//! The pointer has to stay on the item while waiting; leaving it, a key press, a drag, another
//! click (a double-click), a right click or a selection change cancels.
//!
//! Pure state: the caller passes the clock and runs the timer, so this is tested without a UI.

use std::time::{Duration, Instant};

/// Wait used when the system double-click time can't be read.
pub const FALLBACK_DELAY: Duration = Duration::from_millis(500);
/// Bounds for the system value (Windows caps it at 5000 ms).
const MIN_DELAY: Duration = Duration::from_millis(200);
const MAX_DELAY: Duration = Duration::from_millis(5000);

/// The rename delay for a system double-click time in milliseconds (`None` when unknown).
pub fn delay_for(double_click_ms: Option<u32>) -> Duration {
    match double_click_ms {
        Some(ms) if ms > 0 => Duration::from_millis(u64::from(ms)).clamp(MIN_DELAY, MAX_DELAY),
        _ => FALLBACK_DELAY,
    }
}

/// What the press under the pointer looked like.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts about one press, read once by `SlowClick::press`"
)]
pub struct Press {
    /// 1 for a single click, 2 for the second click of a double-click, …
    pub click_count: usize,
    /// Ctrl, Shift, Alt or Win held.
    pub modifiers: bool,
    /// The item was the only selected item before this press.
    pub was_sole_selection: bool,
    /// The list already had keyboard focus before this press (a click that activates the
    /// window or the pane doesn't rename, like Explorer).
    pub had_focus: bool,
    /// Something else rules it out (single-click-to-open mode, renaming already, read-only
    /// listing such as search results).
    pub blocked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum State<K> {
    Idle,
    /// Pressed on the sole selected item; waiting for the release.
    Pressed(K),
    /// Released without a drag; renames at `due` unless cancelled first.
    Waiting {
        item: K,
        due: Instant,
        ticket: u64,
    },
}

/// The slow-click state machine. `K` names an item (its file name).
#[derive(Clone, Debug)]
pub struct SlowClick<K> {
    state: State<K>,
    ticket: u64,
}

impl<K> Default for SlowClick<K> {
    fn default() -> Self {
        Self {
            state: State::Idle,
            ticket: 0,
        }
    }
}

impl<K: PartialEq + Clone> SlowClick<K> {
    /// Left button down on `item`. Arms only for a plain single click on what was already the
    /// only selected item; anything else (the selecting click, the second click of a
    /// double-click, Ctrl/Shift clicks) cancels.
    pub fn press(&mut self, item: K, press: Press) {
        let arms = press.click_count == 1
            && !press.modifiers
            && press.was_sole_selection
            && press.had_focus
            && !press.blocked;
        self.state = if arms {
            State::Pressed(item)
        } else {
            State::Idle
        };
    }

    /// Left button released on `item` without a drag (a click). Starts the wait and returns the
    /// ticket the timer hands back to [`SlowClick::fire`], or `None` when nothing is armed.
    pub fn release(&mut self, item: &K, now: Instant, delay: Duration) -> Option<u64> {
        match &self.state {
            State::Pressed(pressed) if pressed == item => {
                self.ticket = self.ticket.wrapping_add(1);
                self.state = State::Waiting {
                    item: item.clone(),
                    due: now + delay,
                    ticket: self.ticket,
                };
                Some(self.ticket)
            }
            _ => {
                self.state = State::Idle;
                None
            }
        }
    }

    /// The pointer left `item` (or the list). Cancels a wait for that item.
    pub fn leave(&mut self, item: &K) {
        let on_it = match &self.state {
            State::Pressed(k) | State::Waiting { item: k, .. } => k == item,
            State::Idle => false,
        };
        if on_it {
            self.state = State::Idle;
        }
    }

    /// A key press, a drag, a right click, a scroll, a selection change or focus loss.
    pub fn cancel(&mut self) {
        self.state = State::Idle;
    }

    /// Whether a rename is pending (pressed or waiting).
    pub const fn is_armed(&self) -> bool {
        !matches!(self.state, State::Idle)
    }

    /// The timer for `ticket` ran out. Returns the item to rename when the wait is still on,
    /// it's due, and `still_sole` says the item is still the only selection.
    pub fn fire(
        &mut self,
        ticket: u64,
        now: Instant,
        still_sole: impl FnOnce(&K) -> bool,
    ) -> Option<K> {
        let State::Waiting {
            item,
            due,
            ticket: waiting,
        } = &self.state
        else {
            return None;
        };
        if *waiting != ticket || now < *due {
            return None;
        }
        let item = item.clone();
        self.state = State::Idle;
        still_sole(&item).then_some(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: Duration = Duration::from_millis(500);

    fn plain() -> Press {
        Press {
            click_count: 1,
            modifiers: false,
            was_sole_selection: true,
            had_focus: true,
            blocked: false,
        }
    }

    #[test]
    fn delay_follows_the_system_double_click_time() {
        assert_eq!(delay_for(Some(500)), Duration::from_millis(500));
        assert_eq!(delay_for(Some(900)), Duration::from_millis(900));
        assert_eq!(delay_for(None), FALLBACK_DELAY);
        assert_eq!(delay_for(Some(0)), FALLBACK_DELAY);
        assert_eq!(delay_for(Some(50)), MIN_DELAY);
        assert_eq!(delay_for(Some(60_000)), MAX_DELAY);
    }

    #[test]
    fn click_on_the_selected_item_then_wait_renames() {
        let t0 = Instant::now();
        let mut s = SlowClick::default();
        s.press("a", plain());
        let ticket = s.release(&"a", t0, D).unwrap();
        // Too early: nothing yet, still waiting.
        assert_eq!(s.fire(ticket, t0 + D / 2, |_| true), None);
        assert!(s.is_armed());
        assert_eq!(s.fire(ticket, t0 + D, |_| true), Some("a"));
        assert!(!s.is_armed());
        // Fires once.
        assert_eq!(s.fire(ticket, t0 + D * 2, |_| true), None);
    }

    #[test]
    fn the_selecting_click_does_not_arm() {
        let t0 = Instant::now();
        let mut s = SlowClick::default();
        s.press(
            "a",
            Press {
                was_sole_selection: false,
                ..plain()
            },
        );
        assert_eq!(s.release(&"a", t0, D), None);
        assert!(!s.is_armed());
    }

    #[test]
    fn part_of_a_multi_selection_does_not_arm() {
        let mut s = SlowClick::default();
        // The caller reports `was_sole_selection: false` for an item among several.
        s.press(
            "a",
            Press {
                was_sole_selection: false,
                ..plain()
            },
        );
        assert!(!s.is_armed());
    }

    #[test]
    fn double_click_cancels() {
        let t0 = Instant::now();
        let mut s = SlowClick::default();
        s.press("a", plain());
        let ticket = s.release(&"a", t0, D).unwrap();
        // The second click of a double-click arrives inside the double-click time.
        s.press(
            "a",
            Press {
                click_count: 2,
                ..plain()
            },
        );
        assert_eq!(s.fire(ticket, t0 + D, |_| true), None);
    }

    #[test]
    fn modifier_clicks_do_not_arm() {
        let mut s = SlowClick::default();
        s.press(
            "a",
            Press {
                modifiers: true,
                ..plain()
            },
        );
        assert!(!s.is_armed());
    }

    #[test]
    fn unfocused_or_blocked_does_not_arm() {
        let mut s = SlowClick::default();
        s.press(
            "a",
            Press {
                had_focus: false,
                ..plain()
            },
        );
        assert!(!s.is_armed());
        s.press(
            "a",
            Press {
                blocked: true,
                ..plain()
            },
        );
        assert!(!s.is_armed());
    }

    #[test]
    fn leaving_the_item_cancels() {
        let t0 = Instant::now();
        let mut s = SlowClick::default();
        s.press("a", plain());
        let ticket = s.release(&"a", t0, D).unwrap();
        // Leaving another item changes nothing.
        s.leave(&"b");
        assert!(s.is_armed());
        s.leave(&"a");
        assert_eq!(s.fire(ticket, t0 + D, |_| true), None);
    }

    #[test]
    fn key_drag_or_right_click_cancels() {
        let t0 = Instant::now();
        let mut s = SlowClick::default();
        s.press("a", plain());
        let ticket = s.release(&"a", t0, D).unwrap();
        s.cancel();
        assert_eq!(s.fire(ticket, t0 + D, |_| true), None);
    }

    #[test]
    fn drag_never_releases_as_a_click() {
        // A drag ends without a click, so the press is dropped by the next press or cancel.
        let mut s = SlowClick::default();
        s.press("a", plain());
        s.cancel();
        assert_eq!(s.release(&"a", Instant::now(), D), None);
    }

    #[test]
    fn release_on_another_item_does_not_arm() {
        let mut s = SlowClick::default();
        s.press("a", plain());
        assert_eq!(s.release(&"b", Instant::now(), D), None);
        assert!(!s.is_armed());
    }

    #[test]
    fn stale_timer_is_ignored() {
        let t0 = Instant::now();
        let mut s = SlowClick::default();
        s.press("a", plain());
        let first = s.release(&"a", t0, D).unwrap();
        s.press("a", plain());
        let second = s.release(&"a", t0 + D / 4, D).unwrap();
        assert_ne!(first, second);
        // The first timer runs out but belongs to the older click.
        assert_eq!(s.fire(first, t0 + D, |_| true), None);
        assert_eq!(s.fire(second, t0 + D / 4 + D, |_| true), Some("a"));
    }

    #[test]
    fn selection_changed_meanwhile_does_not_rename() {
        let t0 = Instant::now();
        let mut s = SlowClick::default();
        s.press("a", plain());
        let ticket = s.release(&"a", t0, D).unwrap();
        assert_eq!(s.fire(ticket, t0 + D, |_| false), None);
        assert!(!s.is_armed());
    }
}
