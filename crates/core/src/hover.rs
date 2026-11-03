//! Hover state machine behind the dynamic click-through.
//!
//! The overlay window ignores the mouse almost all the time, so the web page
//! never sees hover events. A background thread polls the cursor position and
//! feeds it here; the machine decides when the island opens or closes and
//! whether the window must catch the mouse right now.

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    pub fn inflate(&self, by: f64) -> Rect {
        Rect { x: self.x - by, y: self.y - by, w: self.w + by * 2.0, h: self.h + by * 2.0 }
    }

    /// A `w`×`h` rectangle centred horizontally on the top edge of `outer`.
    pub fn top_center(outer: &Rect, w: f64, h: f64) -> Rect {
        Rect { x: outer.x + (outer.w - w) / 2.0, y: outer.y, w, h }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// Collapsed (capsule or strip) → open island.
    Expand,
    /// Open island → capsule.
    Collapse,
    /// Capsule → thin strip (idle, or a fullscreen app is in front).
    Shrink,
    /// Thin strip → capsule (something happened, fullscreen ended).
    Wake,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Open,
    Compact,
    Thin,
}

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// How long the cursor must rest on the capsule before it opens.
    pub open_delay_ms: u64,
    /// The same for the thin strip: it sits over other apps' title bars and
    /// tabs, so a short pause separates "I want the notch" from a fly-by.
    pub strip_open_delay_ms: u64,
    /// How long the cursor may be outside the open island before it closes.
    pub close_delay_ms: u64,
}

impl Default for Timing {
    fn default() -> Self {
        Self { open_delay_ms: 0, strip_open_delay_ms: 150, close_delay_ms: 350 }
    }
}

/// Everything the machine needs to know about one poll.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub now_ms: u64,
    pub cursor: (f64, f64),
    /// Capsule hitbox while collapsed (screen pixels).
    pub compact: Rect,
    /// Hitbox of the thin strip.
    pub strip: Rect,
    /// Visible island while expanded (screen pixels).
    pub expanded: Rect,
    /// Extra tolerance around the open island before it counts as "left".
    pub leave_margin: f64,
    pub left_button_down: bool,
    /// Shrink to the strip after this much inactivity (`None` = never).
    pub idle_thin_ms: Option<u64>,
    /// Stay a strip regardless of activity (a fullscreen app is in front).
    pub force_thin: bool,
}

#[derive(Debug, Clone, Default)]
pub struct HoverMachine {
    pub timing: Timing,
    expanded: bool,
    thin: bool,
    entered: bool,
    dwell_since: Option<u64>,
    outside_since: Option<u64>,
    sticky_until: u64,
    last_active: u64,
    input_lock: bool,
    dragging: bool,
    interactive: bool,
}

impl HoverMachine {
    pub fn new(timing: Timing) -> Self {
        Self { timing, ..Default::default() }
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    pub fn mode(&self) -> Mode {
        if self.expanded {
            Mode::Open
        } else if self.thin {
            Mode::Thin
        } else {
            Mode::Compact
        }
    }

    /// Whether the window should currently receive mouse input (the opposite
    /// of click-through).
    pub fn interactive(&self) -> bool {
        self.interactive
    }

    /// Text input has keyboard focus: do not close on mouse leave.
    pub fn set_input_lock(&mut self, locked: bool) {
        self.input_lock = locked;
    }

    pub fn input_lock(&self) -> bool {
        self.input_lock
    }

    /// A drag-out of a screenshot is in progress.
    pub fn set_dragging(&mut self, dragging: bool) {
        self.dragging = dragging;
    }

    /// Something worth showing happened (new clip or screenshot): the strip
    /// wakes up to the capsule on the next tick unless it is forced thin.
    pub fn poke(&mut self, now_ms: u64) {
        self.last_active = now_ms;
    }

    /// Opens the island without hovering (hotkey, double Ctrl+C). It stays open
    /// for `sticky_ms` even if the cursor never comes; once the cursor has
    /// visited the island the normal leave rule applies.
    pub fn open(&mut self, now_ms: u64, sticky_ms: u64) -> Option<Transition> {
        self.sticky_until = now_ms + sticky_ms;
        self.entered = false;
        self.outside_since = None;
        self.last_active = now_ms;
        self.thin = false;
        if self.expanded {
            return None;
        }
        self.expanded = true;
        Some(Transition::Expand)
    }

    pub fn close(&mut self, now_ms: u64) -> Option<Transition> {
        self.input_lock = false;
        self.dragging = false;
        self.dwell_since = None;
        self.outside_since = None;
        self.last_active = now_ms;
        if !self.expanded {
            return None;
        }
        self.expanded = false;
        Some(Transition::Collapse)
    }

    fn expand_now(&mut self, s: &Sample) -> Option<Transition> {
        self.dwell_since = None;
        self.expanded = true;
        self.thin = false;
        self.entered = true;
        self.outside_since = None;
        self.last_active = s.now_ms;
        self.interactive = s.expanded.contains(s.cursor.0, s.cursor.1);
        Some(Transition::Expand)
    }

    fn dwell(&mut self, now_ms: u64, delay_ms: u64) -> bool {
        let since = *self.dwell_since.get_or_insert(now_ms);
        now_ms.saturating_sub(since) >= delay_ms
    }

    pub fn tick(&mut self, s: &Sample) -> Option<Transition> {
        let (cx, cy) = s.cursor;
        if self.expanded {
            return self.tick_open(s);
        }
        let should_thin =
            s.force_thin || s.idle_thin_ms.is_some_and(|idle| s.now_ms.saturating_sub(self.last_active) >= idle);

        if self.thin {
            // The strip never catches clicks: whatever is under it (a tab, a
            // title bar) keeps working; opening is decided by polling alone.
            self.interactive = false;
            if s.strip.contains(cx, cy) && !s.left_button_down {
                if self.dwell(s.now_ms, self.timing.strip_open_delay_ms) {
                    return self.expand_now(s);
                }
                return None;
            }
            self.dwell_since = None;
            if !should_thin {
                self.thin = false;
                return Some(Transition::Wake);
            }
            return None;
        }

        let over = s.compact.contains(cx, cy);
        if !s.left_button_down || !self.interactive {
            self.interactive = over;
        }
        if over {
            self.last_active = s.now_ms;
            if s.left_button_down {
                self.dwell_since = None;
                return None;
            }
            if self.dwell(s.now_ms, self.timing.open_delay_ms) {
                return self.expand_now(s);
            }
            return None;
        }
        self.dwell_since = None;
        if should_thin && !s.left_button_down {
            self.thin = true;
            self.interactive = false;
            return Some(Transition::Shrink);
        }
        None
    }

    fn tick_open(&mut self, s: &Sample) -> Option<Transition> {
        let (cx, cy) = s.cursor;
        self.last_active = s.now_ms;
        let over_visible = s.expanded.contains(cx, cy);
        // Keep catching the mouse while a button is held: a click or drag that
        // started on the island must not fall through halfway.
        if !(s.left_button_down && self.interactive) {
            self.interactive = over_visible;
        }
        if s.expanded.inflate(s.leave_margin).contains(cx, cy) {
            self.entered = true;
            self.outside_since = None;
            return None;
        }
        let sticky = !self.entered && s.now_ms < self.sticky_until;
        if self.input_lock || self.dragging || s.left_button_down || sticky {
            self.outside_since = None;
            return None;
        }
        let since = *self.outside_since.get_or_insert(s.now_ms);
        if s.now_ms.saturating_sub(since) >= self.timing.close_delay_ms {
            self.expanded = false;
            self.outside_since = None;
            self.interactive = s.compact.contains(cx, cy);
            return Some(Transition::Collapse);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPACT: Rect = Rect { x: 100.0, y: 0.0, w: 180.0, h: 32.0 };
    const EXPANDED: Rect = Rect { x: 0.0, y: 0.0, w: 380.0, h: 200.0 };

    const STRIP: Rect = Rect { x: 110.0, y: 0.0, w: 160.0, h: 6.0 };

    fn sample(now_ms: u64, x: f64, y: f64) -> Sample {
        Sample {
            now_ms,
            cursor: (x, y),
            compact: COMPACT,
            strip: STRIP,
            expanded: EXPANDED,
            leave_margin: 10.0,
            left_button_down: false,
            idle_thin_ms: None,
            force_thin: false,
        }
    }

    fn slow() -> Timing {
        Timing { open_delay_ms: 120, ..Timing::default() }
    }

    #[test]
    fn rect_helpers() {
        let outer = Rect { x: 10.0, y: 5.0, w: 540.0, h: 280.0 };
        assert_eq!(Rect::top_center(&outer, 180.0, 32.0), Rect { x: 190.0, y: 5.0, w: 180.0, h: 32.0 });
        assert!(COMPACT.inflate(5.0).contains(96.0, -4.0));
        assert!(!COMPACT.contains(280.0, 10.0));
    }

    #[test]
    fn opens_instantly_by_default() {
        let mut m = HoverMachine::new(Timing::default());
        assert_eq!(m.tick(&sample(0, 150.0, 10.0)), Some(Transition::Expand));
        assert_eq!(m.mode(), Mode::Open);
    }

    #[test]
    fn opens_after_dwell_only() {
        let mut m = HoverMachine::new(slow());
        assert_eq!(m.tick(&sample(0, 150.0, 10.0)), None);
        assert!(m.interactive());
        assert_eq!(m.tick(&sample(60, 150.0, 10.0)), None);
        // Leaving resets the dwell timer.
        assert_eq!(m.tick(&sample(80, 50.0, 100.0)), None);
        assert!(!m.interactive());
        assert_eq!(m.tick(&sample(150, 150.0, 10.0)), None);
        assert_eq!(m.tick(&sample(270, 150.0, 10.0)), Some(Transition::Expand));
        assert!(m.is_expanded());
    }

    #[test]
    fn closes_after_leave_delay() {
        let mut m = HoverMachine::new(Timing::default());
        m.tick(&sample(0, 150.0, 10.0));
        assert!(m.is_expanded());
        // Inside the margin: still open.
        assert_eq!(m.tick(&sample(300, 385.0, 50.0)), None);
        assert!(!m.interactive());
        assert_eq!(m.tick(&sample(400, 600.0, 500.0)), None);
        assert_eq!(m.tick(&sample(700, 600.0, 500.0)), None);
        assert_eq!(m.tick(&sample(760, 600.0, 500.0)), Some(Transition::Collapse));
        assert!(!m.is_expanded());
    }

    #[test]
    fn locks_hold_island_open() {
        let mut m = HoverMachine::new(Timing::default());
        m.open(0, 0);
        m.set_input_lock(true);
        assert_eq!(m.tick(&sample(10_000, 900.0, 900.0)), None);
        m.set_input_lock(false);
        m.set_dragging(true);
        assert_eq!(m.tick(&sample(20_000, 900.0, 900.0)), None);
        m.set_dragging(false);
        let mut held = sample(30_000, 900.0, 900.0);
        held.left_button_down = true;
        assert_eq!(m.tick(&held), None);
        assert_eq!(m.tick(&sample(30_100, 900.0, 900.0)), None);
        assert_eq!(m.tick(&sample(30_500, 900.0, 900.0)), Some(Transition::Collapse));
    }

    #[test]
    fn button_held_keeps_window_interactive() {
        let mut m = HoverMachine::new(Timing::default());
        m.open(0, 0);
        m.tick(&sample(10, 50.0, 50.0));
        assert!(m.interactive());
        let mut drag = sample(20, 900.0, 900.0);
        drag.left_button_down = true;
        m.tick(&drag);
        assert!(m.interactive());
        m.tick(&sample(30, 900.0, 900.0));
        assert!(!m.interactive());
    }

    #[test]
    fn programmatic_open_is_sticky_until_visited() {
        let mut m = HoverMachine::new(Timing::default());
        assert_eq!(m.open(0, 5000), Some(Transition::Expand));
        assert_eq!(m.tick(&sample(1000, 900.0, 900.0)), None);
        assert_eq!(m.tick(&sample(4000, 900.0, 900.0)), None);
        // Cursor visits, then leaves: normal close.
        m.tick(&sample(4100, 50.0, 50.0));
        m.tick(&sample(4200, 900.0, 900.0));
        assert_eq!(m.tick(&sample(4600, 900.0, 900.0)), Some(Transition::Collapse));

        // Never visited: closes once the sticky time is over.
        m.open(10_000, 2000);
        assert_eq!(m.tick(&sample(11_000, 900.0, 900.0)), None);
        assert_eq!(m.tick(&sample(12_100, 900.0, 900.0)), None);
        assert_eq!(m.tick(&sample(12_500, 900.0, 900.0)), Some(Transition::Collapse));
    }

    #[test]
    fn close_resets_locks() {
        let mut m = HoverMachine::new(Timing::default());
        m.open(0, 0);
        m.set_input_lock(true);
        assert_eq!(m.close(5), Some(Transition::Collapse));
        assert!(!m.input_lock());
        assert_eq!(m.close(6), None);
    }

    #[test]
    fn pressing_on_capsule_does_not_open() {
        let mut m = HoverMachine::new(Timing::default());
        let mut s = sample(0, 150.0, 10.0);
        s.left_button_down = true;
        m.tick(&s);
        s.now_ms = 500;
        assert_eq!(m.tick(&s), None);
    }

    #[test]
    fn idle_capsule_shrinks_to_strip_and_wakes_on_poke() {
        let mut m = HoverMachine::new(Timing::default());
        let idle = |now, x, y| Sample { idle_thin_ms: Some(3000), ..sample(now, x, y) };
        assert_eq!(m.tick(&idle(1000, 900.0, 900.0)), None);
        assert_eq!(m.tick(&idle(3000, 900.0, 900.0)), Some(Transition::Shrink));
        assert_eq!(m.mode(), Mode::Thin);
        assert!(!m.interactive());
        m.poke(4000);
        assert_eq!(m.tick(&idle(4010, 900.0, 900.0)), Some(Transition::Wake));
        assert_eq!(m.mode(), Mode::Compact);
        assert_eq!(m.tick(&idle(7010, 900.0, 900.0)), Some(Transition::Shrink));
    }

    #[test]
    fn strip_opens_after_short_rest_and_never_catches_clicks() {
        let mut m = HoverMachine::new(Timing::default());
        let forced = |now, x, y| Sample { force_thin: true, ..sample(now, x, y) };
        assert_eq!(m.tick(&forced(0, 900.0, 900.0)), Some(Transition::Shrink));
        // Over the capsule area but below the strip: nothing happens.
        assert_eq!(m.tick(&forced(10, 150.0, 20.0)), None);
        assert_eq!(m.tick(&forced(20, 150.0, 2.0)), None);
        assert!(!m.interactive());
        assert_eq!(m.tick(&forced(100, 150.0, 2.0)), None);
        assert_eq!(m.tick(&forced(180, 150.0, 2.0)), Some(Transition::Expand));
        // Poking does not wake a forced strip.
        m.close(200);
        assert_eq!(m.tick(&forced(210, 900.0, 900.0)), Some(Transition::Shrink));
        m.poke(220);
        assert_eq!(m.tick(&forced(230, 900.0, 900.0)), None);
        assert_eq!(m.tick(&sample(240, 900.0, 900.0)), Some(Transition::Wake));
    }

    #[test]
    fn programmatic_open_from_strip() {
        let mut m = HoverMachine::new(Timing::default());
        m.tick(&Sample { force_thin: true, ..sample(0, 900.0, 900.0) });
        assert_eq!(m.open(10, 1000), Some(Transition::Expand));
        assert_eq!(m.mode(), Mode::Open);
    }
}
