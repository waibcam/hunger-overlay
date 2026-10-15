//! Clickable panel placed next to the map (docs/DESIGN.md §4): tracked quests, crafting list, item searches,
//! untracked quests of the Quests layer, map layers and quick settings. It is a small separate window that
//! receives clicks WITHOUT taking focus away from the game (WS_EX_NOACTIVATE); the rest of the overlay lets
//! every click pass through to the game.
//!
//! Look: the game's own UI style (near-black box with a thin double gold frame, serif caps titles), Segoe UI
//! for body text, Segoe Fluent Icons for the icons. All sizes are px at 1440p, multiplied by `sc`.

use crate::i18n::Strings;
use crate::markers;
use crate::model::{AllyRow, LegendLayer, LegendQuest, OwnerKind};
use crate::net::Status;
use crate::render::{Font, Renderer, Rgba};

/// What a click on a panel zone does (handled by the panel window's owner).
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Header: drag to move the panel, double-click to put it back in its default place.
    Move,
    /// Open the settings window.
    Settings,
    /// Collapse / expand the panel to its header.
    Collapse,
    /// Show / hide objectives that are already done.
    ToggleDone,
    /// Lower the minimum loot chance filter by one step.
    ChanceDown,
    /// Raise the minimum loot chance filter by one step.
    ChanceUp,
    /// Open / close the language list.
    Language,
    /// Language picked in the list ("" = Auto).
    SetLang(String),
    /// Hide / show one legend row's markers (by quest id, `craft:…` or `find:…`).
    ToggleQuest(String),
    /// Turn one map layer on / off (by layer id).
    ToggleLayer(String),
    /// Turn a whole layer group on or off (layer ids of the group).
    ToggleGroup(Vec<String>),
    /// Open the search window.
    Search,
    /// Remove one item search from the map (by item key).
    RemoveSearch(String),
    /// Remove every item search.
    ClearSearches,
    /// Collapse / expand one panel section ("quests", "crafting", "search", "layers").
    ToggleSection(String),
    /// Lower the chance filter to this value (%), to reveal markers that are all below it.
    RevealFilter(u32),
    /// Start tracking an untracked quest on the website account (by quest id).
    Track(String),
    /// Row of an untracked quest (hover = spotlight its markers on the map; click does nothing).
    AvailRow(String),
    /// Show more / fewer untracked quests.
    MoreAvail,
    /// List the objectives of a tracked quest that has nothing on this map (by quest id).
    ExpandOther(String),
    /// Revert the last tracking change (toast link).
    Undo,
    /// Mark a tracked quest done (or finished, for a repeatable).
    Done(String),
    /// Open the quest journal.
    Journal,
    /// Open the quest journal on the "+ Ally" tab.
    ManageAllies,
    /// Hide / show every marker of a raid ally (ally key).
    ToggleAlly(String),
    /// Hide / show one quest of a raid ally (ally key, quest id).
    ToggleAllyQuest(String, String),
    /// Remove a quest from a local ally (ally key, quest id).
    RemoveAllyQuest(String, String),
    /// Hide / show every quest marker on the map (mine and the allies').
    ToggleQuestMarkers,
}

/// Clickable zone (coordinates local to the panel).
#[derive(Debug, Clone)]
pub struct Hit {
    /// Left edge.
    pub l: f32,
    /// Top edge.
    pub t: f32,
    /// Right edge.
    pub r: f32,
    /// Bottom edge.
    pub b: f32,
    /// What clicking this zone does.
    pub action: Action,
}

impl Hit {
    /// True if the point (x, y) lies inside this zone (edges included).
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.l && x <= self.r && y >= self.t && y <= self.b
    }
}

/// Values offered by the chance stepper (%): finer steps at the low end, where they matter.
pub const CHANCE_STEPS: [u32; 10] = [0, 1, 2, 5, 10, 15, 20, 25, 33, 50];

/// Next chance step below `v`.
pub fn chance_down(v: u32) -> u32 {
    CHANCE_STEPS.iter().rev().copied().find(|&s| s < v).unwrap_or(0)
}

/// Next chance step above `v`.
pub fn chance_up(v: u32) -> u32 {
    CHANCE_STEPS.iter().copied().find(|&s| s > v).unwrap_or(50)
}

/// Highest chance step that still shows a marker whose best chance is `best` (0..1).
pub fn reveal_step(best: f64) -> u32 {
    let pct = best * 100.0 + 1e-6;
    CHANCE_STEPS.iter().rev().copied().find(|&s| s as f64 <= pct).unwrap_or(0)
}

/// One row of "Up next" in the panel: a quest the player can start now.
#[derive(Debug, Clone)]
pub struct UpRow {
    /// Quest id.
    pub quest_id: String,
    /// Title.
    pub title: String,
    /// Quest giver.
    pub giver: String,
    /// Step in the giver's story ("2/10").
    pub step: Option<(usize, usize)>,
    /// Has objective spots on this map.
    pub here: bool,
}

/// Transient message under the header, after a change made on the website account.
#[derive(Debug, Clone)]
pub struct Toast {
    /// Message.
    pub text: String,
    /// Success (green) or problem (orange).
    pub ok: bool,
    /// Offer "Undo".
    pub undo: bool,
    /// Next quest to propose on a second line (quest id, title), with a "+ Track" button.
    pub next: Option<(String, String)>,
}

/// Everything the panel displays, gathered by the caller for one layout/draw pass.
pub struct View<'a> {
    /// UI strings in the current language.
    pub s: &'static Strings,
    /// Legend rows: tracked quests, crafting recipes and item searches, for the current map.
    pub legend: &'a [LegendQuest],
    /// Map layers.
    pub layers: &'a [LegendLayer],
    /// Connection / key status, if known.
    pub status: Option<&'a Status>,
    /// Whether objectives already done are listed.
    pub show_done: bool,
    /// Minimum loot chance (percent) below which loot markers are hidden.
    pub min_chance: u32,
    /// Label of the current language (shown on the language chip).
    pub lang_label: String,
    /// Panel reduced to its header.
    pub collapsed: bool,
    /// Background opacity (0..1).
    pub opacity: f32,
    /// Sections collapsed by the user.
    pub sections_collapsed: &'a [String],
    /// Message under the header, if any.
    pub toast: Option<&'a Toast>,
    /// "Available on this map" fully listed.
    pub avail_open: bool,
    /// Tracked quests with nothing on this map that the user expanded.
    pub other_open: &'a [String],
    /// The language list is open.
    pub lang_open: bool,
    /// Language setting ("" = Auto), to tick the current entry of the list.
    pub locale: &'a str,
    /// Every quest marker is hidden on the map (eye of the QUESTS header).
    pub quest_markers_hidden: bool,
    /// Quest tracked a few seconds ago (gets a "NEW" tag).
    pub recent: Option<&'a str>,
    /// The map position is being recomputed (markers hidden).
    pub uncertain: bool,
    /// "Up next": quests the player can start now (empty when the website does not publish prerequisites).
    pub up_next: &'a [UpRow],
    /// Number of repeatable quests the player can start.
    pub repeat_avail: usize,
    /// The website accepts "done" (contract v1.2): show the ✓ buttons.
    pub can_done: bool,
    /// Raid allies.
    pub allies: &'a [AllyRow],
    /// Overflow level (0 = everything; 1…4 = more and more compact, see `layout`).
    pub compact: u8,
    /// Scroll offset of the body (px).
    pub scroll: f32,
}

/// Result of a layout pass.
pub struct Out {
    /// Height of the panel (≤ the maximum height).
    pub h: f32,
    /// Height everything would need without the maximum height.
    pub content_h: f32,
    /// How far the body can be scrolled.
    pub max_scroll: f32,
    /// Clickable zones, in drawing order.
    pub hits: Vec<Hit>,
}

// Design tokens (§3.1).
pub(crate) const INK950: (f32, f32, f32) = (0.067, 0.055, 0.039);
pub(crate) const INK900: Rgba = (0.043, 0.035, 0.024, 0.75);
pub(crate) const INK800: Rgba = (0.141, 0.114, 0.086, 0.95);
pub(crate) const INK700: Rgba = (0.188, 0.153, 0.114, 1.0);
pub(crate) const LINE_GOLD: (f32, f32, f32) = (0.784, 0.655, 0.396);
pub(crate) const GOLD: Rgba = (0.831, 0.686, 0.416, 1.0);
pub(crate) const GOLD_HI: Rgba = (0.922, 0.796, 0.545, 1.0);
pub(crate) const ACCENT: Rgba = (0.851, 0.647, 0.290, 1.0);
pub(crate) const DARK_INK: Rgba = (0.090, 0.071, 0.047, 1.0);
pub(crate) const T100: Rgba = (0.945, 0.918, 0.859, 1.0);
pub(crate) const T200: Rgba = (0.851, 0.820, 0.761, 1.0);
pub(crate) const T300: Rgba = (0.702, 0.663, 0.584, 1.0);
pub(crate) const T500: Rgba = (0.522, 0.486, 0.427, 1.0);
pub(crate) const WARN: (f32, f32, f32) = (0.941, 0.635, 0.294);
pub(crate) const WARN_TEXT: Rgba = (0.953, 0.765, 0.541, 1.0);
pub(crate) const OK: (f32, f32, f32) = (0.525, 0.761, 0.494);
pub(crate) const HOVER_ROW: Rgba = (1.0, 1.0, 1.0, 0.07);

// Icons (Segoe Fluent Icons / MDL2 Assets code points).
pub(crate) const IC_SETTINGS: &str = "\u{E713}";
pub(crate) const IC_SEARCH: &str = "\u{E721}";
pub(crate) const IC_DOWN: &str = "\u{E70D}";
pub(crate) const IC_RIGHT: &str = "\u{E76C}";
pub(crate) const IC_CLOSE: &str = "\u{E711}";
pub(crate) const IC_GLOBE: &str = "\u{E774}";
pub(crate) const IC_WARN: &str = "\u{E7BA}";
pub(crate) const IC_CHECK: &str = "\u{E73E}";
pub(crate) const IC_HIDDEN: &str = "\u{ED1A}";
pub(crate) const IC_INFO: &str = "\u{E946}";
pub(crate) const IC_BOOK: &str = "\u{E82D}";
pub(crate) const IC_EYE: &str = "\u{E7B3}";
pub(crate) const IC_ADD_FRIEND: &str = "\u{E8FA}";
pub(crate) const IC_CLOUD: &str = "\u{E753}";
pub(crate) const IC_PC: &str = "\u{E7F8}";

/// Layer groups of the MAP LAYERS section (§4.3). Layers not listed go to the last group.
const GROUPS: [&[&str]; 3] = [
    &["extractions", "player_spawns", "minibosses", "traps", "sound_traps"],
    &["placed_loot", "harvest", "keys", "locks"],
    &["quests", "places"],
];

/// `c` with its alpha multiplied by `a`.
fn fade(c: Rgba, a: f32) -> Rgba {
    (c.0, c.1, c.2, c.3 * a)
}

/// Layout / drawing state of one pass.
struct Ctx<'a> {
    r: &'a Renderer,
    sc: f32,
    draw: bool,
    hover: Option<usize>,
    hits: Vec<Hit>,
    /// Panel width.
    w: f32,
    /// Side padding.
    pad: f32,
}

impl Ctx<'_> {
    /// True if the zone about to be pushed is the hovered one.
    fn hov(&self) -> bool {
        self.hover == Some(self.hits.len())
    }

    /// Adds a clickable zone.
    fn push(&mut self, l: f32, t: f32, r: f32, b: f32, action: Action) {
        self.hits.push(Hit { l, t, r, b, action });
    }

    /// Height of one line of `f`.
    fn fh(&self, f: Font) -> f32 {
        self.r.measure("Ag", f, 1000.0).1
    }

    /// Single-line text vertically centred in a row [y, y + h].
    fn text_mid(&self, s: &str, f: Font, x: f32, y: f32, h: f32, max_w: f32, c: Rgba) {
        if self.draw && max_w > 0.0 {
            self.r.text(s, f, x, y + (h - self.fh(f)) / 2.0, max_w, c);
        }
    }

    /// Thin gold separator line.
    fn hairline(&self, y: f32) {
        if self.draw {
            self.r.line(self.pad, y, self.w - self.pad, y, 1.0, (LINE_GOLD.0, LINE_GOLD.1, LINE_GOLD.2, 0.18));
        }
    }

    /// Square icon button (28×28).
    fn icon_btn(&mut self, x: f32, y: f32, icon: &str, action: Action) {
        let s = 28.0 * self.sc;
        if self.draw {
            if self.hov() {
                self.r.fill_rrect(x, y, x + s, y + s, 6.0 * self.sc, INK700);
            }
            self.r.text_center(icon, Font::Icon, x + s / 2.0, y + s / 2.0, if self.hov() { GOLD_HI } else { T300 });
        }
        self.push(x, y, x + s, y + s, action);
    }
}

/// Chip look.
#[derive(Clone, Copy, PartialEq)]
enum Chip {
    /// Dark chip (quick settings).
    Normal,
    /// Filled with the accent colour (main action).
    Primary,
    /// Gold outline (secondary action: "+ Track", "Show ≥ 1 %").
    Ghost,
}

/// Draws a chip (optional icon + text) at (x, y) with height `h`; returns its width.
#[allow(clippy::too_many_arguments)]
fn chip(c: &mut Ctx, x: f32, y: f32, h: f32, icon: Option<&str>, text: &str, action: Option<Action>, style: Chip) -> f32 {
    let sc = c.sc;
    let (tw, _) = c.r.measure(text, Font::Chip, 600.0 * sc);
    let iw = if icon.is_some() { 20.0 * sc } else { 0.0 };
    let w = tw + iw + 20.0 * sc;
    if c.draw {
        let hov = action.is_some() && c.hov();
        let rad = 6.0 * sc;
        let (fill, edge, ink) = match style {
            Chip::Normal => (if hov { INK700 } else { INK800 }, if hov { fade(GOLD, 0.5) } else { (1.0, 1.0, 1.0, 0.08) }, T200),
            Chip::Primary => (if hov { GOLD_HI } else { ACCENT }, (0.0, 0.0, 0.0, 0.0), DARK_INK),
            Chip::Ghost => (if hov { fade(GOLD, 0.22) } else { (0.0, 0.0, 0.0, 0.0) }, fade(GOLD, 0.55), GOLD_HI),
        };
        c.r.fill_rrect(x, y, x + w, y + h, rad, fill);
        if edge.3 > 0.0 {
            c.r.stroke_rrect(x + 0.5, y + 0.5, x + w - 0.5, y + h - 0.5, rad, 1.0, edge);
        }
        if let Some(ic) = icon {
            c.r.text_center(ic, Font::Icon, x + 10.0 * sc + iw / 2.0 - 2.0 * sc, y + h / 2.0, ink);
        }
        c.text_mid(text, Font::Chip, x + 10.0 * sc + iw, y, h, tw + 2.0, ink);
    }
    if let Some(a) = action {
        c.push(x, y, x + w, y + h, a);
    }
    w
}

/// Warning / info callout (tinted box with an icon); returns the new y.
fn callout(c: &mut Ctx, x: f32, y: f32, icon: &str, title: &str, body: Option<&str>, tint: (f32, f32, f32)) -> f32 {
    let sc = c.sc;
    let w = c.w - c.pad - x;
    let tx = x + 30.0 * sc;
    let tw = w - 30.0 * sc - 10.0 * sc;
    let (_, th) = c.r.measure(title, Font::Text, tw);
    let bh = body.map(|b| c.r.measure(b, Font::Small, tw).1 + 3.0 * sc).unwrap_or(0.0);
    let h = th + bh + 14.0 * sc;
    if c.draw {
        c.r.fill_rrect(x, y, x + w, y + h, 6.0 * sc, (tint.0, tint.1, tint.2, 0.10));
        c.r.stroke_rrect(x + 0.5, y + 0.5, x + w - 0.5, y + h - 0.5, 6.0 * sc, 1.0, (tint.0, tint.1, tint.2, 0.38));
        c.r.text_center(icon, Font::Icon, x + 15.0 * sc, y + 7.0 * sc + th / 2.0, (tint.0, tint.1, tint.2, 1.0));
        let ink = if tint == WARN { WARN_TEXT } else { T200 };
        c.r.text(title, Font::Text, tx, y + 7.0 * sc, tw, ink);
        if let Some(b) = body {
            c.r.text(b, Font::Small, tx, y + 7.0 * sc + th + 3.0 * sc, tw, T300);
        }
    }
    y + h
}

/// Section header (chevron, serif caps title, count pill, optional right-hand text or link); returns the new y.
#[allow(clippy::too_many_arguments)]
fn section(c: &mut Ctx, y: f32, id: &str, title: &str, count: Option<usize>, right: Option<(String, Option<Action>)>, collapsed: bool) -> f32 {
    section_eye(c, y, id, title, count, right, collapsed, None)
}

/// Section header with an optional eye button at the right end: (markers hidden, action).
#[allow(clippy::too_many_arguments)]
fn section_eye(c: &mut Ctx, y: f32, id: &str, title: &str, count: Option<usize>, right: Option<(String, Option<Action>)>, collapsed: bool, eye: Option<(bool, Action)>) -> f32 {
    let sc = c.sc;
    let h = 30.0 * sc;
    let y = y + 6.0 * sc;
    // Buttons and the right-hand link first (they take precedence over the header zone when clicked).
    let mut right_x = c.w - c.pad;
    if let Some((hidden, a)) = eye {
        let bs = 24.0 * sc;
        let bx = right_x - bs;
        let by = y + (h - bs) / 2.0;
        if c.draw {
            let hov = c.hov();
            if hov || hidden {
                c.r.fill_rrect(bx, by, bx + bs, by + bs, 5.0 * sc, if hidden { fade(GOLD, 0.22) } else { INK700 });
            }
            c.r.text_center(if hidden { IC_HIDDEN } else { IC_EYE }, Font::Icon, bx + bs / 2.0, by + bs / 2.0, if hidden || hov { GOLD_HI } else { T300 });
        }
        c.push(bx, by, bx + bs, by + bs, a);
        right_x = bx - 8.0 * sc;
    }
    if let Some((t, a)) = &right {
        let (tw, _) = c.r.measure(t, Font::Small, 300.0 * sc);
        right_x -= tw;
        if c.draw {
            let hov = a.is_some() && c.hov();
            c.text_mid(t, Font::Small, right_x, y, h, tw + 2.0, if a.is_some() { if hov { GOLD_HI } else { GOLD } } else { T500 });
        }
        if let Some(a) = a {
            c.push(right_x - 4.0 * sc, y, c.w - c.pad, y + h, a.clone());
        }
        right_x -= 8.0 * sc;
    }
    if c.draw {
        let hov = c.hov();
        if hov {
            c.r.fill_rrect(c.pad - 6.0 * sc, y, right_x, y + h, 6.0 * sc, HOVER_ROW);
        }
        c.r.text_center(if collapsed { IC_RIGHT } else { IC_DOWN }, Font::Icon, c.pad + 6.0 * sc, y + h / 2.0, T500);
        let (tw, th) = c.r.measure_spaced(title, Font::Section, right_x - c.pad - 60.0 * sc, 1.8 * sc);
        c.r.text_spaced(title, Font::Section, c.pad + 20.0 * sc, y + (h - th) / 2.0, tw + 4.0, 1.8 * sc, GOLD);
        if let Some(n) = count {
            let t = n.to_string();
            let (nw, _) = c.r.measure(&t, Font::Caps, 100.0);
            let (pl, ph) = (c.pad + 26.0 * sc + tw, 17.0 * sc);
            let pw = (nw + 10.0 * sc).max(ph);
            let pt = y + (h - ph) / 2.0;
            c.r.fill_rrect(pl, pt, pl + pw, pt + ph, ph / 2.0, INK700);
            c.r.text_center(&t, Font::Caps, pl + pw / 2.0, pt + ph / 2.0 - 0.5, T100);
        }
    }
    c.push(c.pad - 6.0 * sc, y, right_x, y + h, Action::ToggleSection(id.to_string()));
    y + h + 2.0 * sc
}

/// Badge disc of an owner (owner colour, white ring, badge character).
fn badge(c: &Ctx, x: f32, y: f32, q: &LegendQuest, alpha: f32) {
    let sc = c.sc;
    let col = markers::guard(q.color);
    c.r.fill_circle(x, y, 9.5 * sc, (col.0, col.1, col.2, alpha));
    c.r.stroke_circle(x, y, 9.5 * sc - 0.7 * sc, 1.4 * sc, (1.0, 1.0, 1.0, 0.95 * alpha), false);
    if q.badge.is_empty() {
        c.r.fill_circle(x, y, 2.6 * sc, (1.0, 1.0, 1.0, alpha));
    } else {
        let lum = 255.0 * (0.299 * col.0 + 0.587 * col.1 + 0.114 * col.2);
        let ink = if lum > 120.0 { DARK_INK } else { (1.0, 1.0, 1.0, 1.0) };
        c.r.text_center(&q.badge, Font::Badge, x, y - 0.5, fade(ink, alpha));
    }
}

/// One owner (quest, recipe or search) with its objectives; returns the new y.
fn owner(c: &mut Ctx, v: &View, y: f32, q: &LegendQuest) -> f32 {
    let s = v.s;
    let sc = c.sc;
    let pad = c.pad;
    let h = 30.0 * sc;
    let mut y = y;
    // Right-hand side: ✕ (searches), then the count / hidden icon.
    let mut right = c.w - pad;
    if q.kind == OwnerKind::Search {
        let bs = 22.0 * sc;
        let bx = right - bs;
        let by = y + (h - bs) / 2.0;
        if c.draw {
            if c.hov() {
                c.r.fill_rrect(bx, by, bx + bs, by + bs, 5.0 * sc, INK700);
            }
            c.r.text_center(IC_CLOSE, Font::Icon, bx + bs / 2.0, by + bs / 2.0, if c.hov() { GOLD_HI } else { T300 });
        }
        let key = q.quest_id.strip_prefix("find:").unwrap_or(&q.quest_id).to_string();
        c.push(bx, by, bx + bs, by + bs, Action::RemoveSearch(key));
        right = bx - 8.0 * sc;
    }
    // ✓ = this quest is done (it never cascades: a tracked quest's earlier quests are already done).
    if q.kind == OwnerKind::Quest && v.can_done && !q.hidden {
        let bs = 22.0 * sc;
        let bx = right - bs;
        let by = y + (h - bs) / 2.0;
        if c.draw {
            let hov = c.hov();
            c.r.fill_rrect(bx, by, bx + bs, by + bs, 5.0 * sc, if hov { INK700 } else { (0.0, 0.0, 0.0, 0.0) });
            c.r.stroke_rrect(bx + 0.5, by + 0.5, bx + bs - 0.5, by + bs - 0.5, 5.0 * sc, 1.0, if hov { GOLD } else { (1.0, 1.0, 1.0, 0.10) });
            c.r.text_center(IC_CHECK, Font::Icon, bx + bs / 2.0, by + bs / 2.0, if hov { GOLD_HI } else { T300 });
        }
        c.push(bx, by, bx + bs, by + bs, Action::Done(q.quest_id.clone()));
        right = bx - 8.0 * sc;
    }
    let done_n = q.objectives.iter().filter(|o| o.done).count();
    let hide_done = v.compact >= 2 && v.show_done;
    let count_t = if q.hidden {
        String::new()
    } else {
        let mut t = s.spots(q.on_this_map as usize);
        if hide_done && done_n > 0 {
            t = format!("+{done_n} {} · {t}", s.k_done.to_lowercase());
        }
        t
    };
    let (cw, _) = c.r.measure(&count_t, Font::Small, 300.0 * sc);
    let count_w = if q.hidden { 18.0 * sc } else { cw };
    let new_tag = v.recent == Some(q.quest_id.as_str());
    if c.draw {
        if c.hov() {
            c.r.fill_rrect(pad - 6.0 * sc, y, right + 4.0 * sc, y + h, 6.0 * sc, HOVER_ROW);
        }
        let a = if q.hidden { 0.4 } else { 1.0 };
        badge(c, pad + 10.0 * sc, y + h / 2.0, q, a);
        let mut tw_max = right - count_w - 10.0 * sc - (pad + 28.0 * sc);
        let tag_w = if new_tag { c.r.measure(s.p_new, Font::Caps, 200.0).0 + 10.0 * sc } else { 0.0 };
        tw_max -= tag_w;
        let (tw, _) = c.r.measure(&q.title, Font::Name, tw_max.max(10.0));
        c.text_mid(&q.title, Font::Name, pad + 28.0 * sc, y, h, tw_max.max(10.0), if q.hidden { fade(T500, 0.4) } else { T100 });
        if new_tag {
            let (tx, th) = (pad + 34.0 * sc + tw, 16.0 * sc);
            let ty = y + (h - th) / 2.0;
            c.r.fill_rrect(tx, ty, tx + tag_w - 4.0 * sc, ty + th, 4.0 * sc, ACCENT);
            c.r.text_center(s.p_new, Font::Caps, tx + (tag_w - 4.0 * sc) / 2.0, ty + th / 2.0 - 0.5, DARK_INK);
        }
        if q.hidden {
            c.r.text_center(IC_HIDDEN, Font::Icon, right - count_w / 2.0, y + h / 2.0, fade(T500, 0.8));
        } else {
            c.text_mid(&count_t, Font::Small, right - cw, y, h, cw + 2.0, T500);
        }
    }
    c.push(pad - 6.0 * sc, y, right + 4.0 * sc, y + h, Action::ToggleQuest(q.quest_id.clone()));
    y += h;
    if q.hidden {
        return y + 2.0 * sc;
    }
    // Every marker below the chance filter: say so, and offer to lower the filter.
    if q.on_this_map == 0 && q.filtered > 0 {
        let x = pad + 28.0 * sc;
        let step = reveal_step(q.best_filtered);
        let msg = s.p_filter_warn.replace("{n}", &v.min_chance.to_string());
        let btn = s.p_reveal.replace("{n}", &step.to_string());
        let (bw, _) = c.r.measure(&btn, Font::Chip, 300.0 * sc);
        let bw = bw + 20.0 * sc;
        let w = c.w - pad - x;
        let tw = w - 30.0 * sc - bw - 16.0 * sc;
        let (_, th) = c.r.measure(&msg, Font::Small, tw);
        let bh = 26.0 * sc;
        let hh = th.max(bh) + 10.0 * sc;
        if c.draw {
            c.r.fill_rrect(x, y + 2.0 * sc, x + w, y + 2.0 * sc + hh, 6.0 * sc, (WARN.0, WARN.1, WARN.2, 0.10));
            c.r.stroke_rrect(x + 0.5, y + 2.5 * sc, x + w - 0.5, y + 1.5 * sc + hh, 6.0 * sc, 1.0, (WARN.0, WARN.1, WARN.2, 0.38));
            c.r.text_center(IC_WARN, Font::Icon, x + 15.0 * sc, y + 2.0 * sc + hh / 2.0, (WARN.0, WARN.1, WARN.2, 1.0));
            c.r.text(&msg, Font::Small, x + 30.0 * sc, y + 2.0 * sc + (hh - th) / 2.0, tw, WARN_TEXT);
        }
        chip(c, x + w - bw - 6.0 * sc, y + 2.0 * sc + (hh - bh) / 2.0, bh, None, &btn, Some(Action::RevealFilter(step)), Chip::Ghost);
        y += hh + 6.0 * sc;
    }
    // Search rows have a single pseudo-objective (the item itself): not listed.
    if q.kind == OwnerKind::Search {
        return y + 2.0 * sc;
    }
    let tx = pad + 46.0 * sc;
    let tw = c.w - pad - tx;
    let mut listed = 0;
    let cap = if v.compact >= 3 { 2 } else { usize::MAX };
    let shown: Vec<_> = q.objectives.iter().filter(|o| !o.done || (v.show_done && !hide_done)).collect();
    for o in &shown {
        if listed >= cap {
            break;
        }
        listed += 1;
        let (_, th) = c.r.measure(&o.label, Font::Text, tw);
        let rh = (th + 4.0 * sc).max(22.0 * sc);
        if c.draw {
            let (gx, gy) = (pad + 34.0 * sc, y + 11.0 * sc);
            if o.done {
                c.r.text_center(IC_CHECK, Font::Icon, gx, gy, T500);
            } else if o.markers_here > 0 {
                let col = markers::guard(q.color);
                c.r.fill_circle(gx, gy, 4.0 * sc, (col.0, col.1, col.2, 1.0));
            } else {
                c.r.stroke_circle(gx, gy, 3.6 * sc, 1.2 * sc, T500, false);
            }
            c.r.text(&o.label, Font::Text, tx, y + (22.0 * sc - c.fh(Font::Text)) / 2.0, tw, if o.done { T500 } else { T200 });
        }
        y += rh;
        // Website hint, only for an open objective with no marker on this map.
        if !o.done && o.markers_here == 0 {
            if let Some(hint) = &o.hint {
                let (_, hh) = c.r.measure(hint, Font::Small, tw);
                if c.draw {
                    c.r.text(hint, Font::Small, tx, y - 1.0 * sc, tw, T500);
                }
                y += hh + 2.0 * sc;
            }
        }
    }
    if shown.len() > listed {
        let t = s.t_more.replace("{n}", &(shown.len() - listed).to_string());
        if c.draw {
            c.r.stroke_circle(pad + 34.0 * sc, y + 11.0 * sc, 3.6 * sc, 1.2 * sc, T500, false);
            c.text_mid(&t, Font::Small, tx, y, 22.0 * sc, tw, T500);
        }
        y += 22.0 * sc;
    }
    y + 4.0 * sc
}

/// One raid ally (hollow disc with the initial, name, SITE / LOCAL tag, spots) and their quests; returns the new y.
fn ally_rows(c: &mut Ctx, v: &View, y: f32, a: &AllyRow) -> f32 {
    let s = v.s;
    let (sc, pad) = (c.sc, c.pad);
    let mut y = y;
    let h = 30.0 * sc;
    let alpha = if a.hidden { 0.4 } else { 1.0 };
    let count = s.spots(a.spots as usize);
    let (cw, _) = c.r.measure(&count, Font::Small, 200.0 * sc);
    if c.draw {
        if c.hov() {
            c.r.fill_rrect(pad - 6.0 * sc, y, c.w - pad + 4.0 * sc, y + h, 6.0 * sc, HOVER_ROW);
        }
        markers::draw(c.r, markers::Kind::Ally, a.color, &a.initial, pad + 10.0 * sc, y + h / 2.0, sc, sc, alpha);
        let (nw, _) = c.r.measure(&a.name, Font::Name, 200.0 * sc);
        c.text_mid(&a.name, Font::Name, pad + 28.0 * sc, y, h, nw + 2.0, fade(T100, alpha));
        // Source: icon (icon font) then the tag.
        c.r.text_center(if a.site { IC_CLOUD } else { IC_PC }, Font::Icon, pad + 44.0 * sc + nw, y + h / 2.0, fade(T500, alpha));
        let tag = if a.site { s.tag_site } else { s.tag_local };
        c.text_mid(tag, Font::Caps, pad + 56.0 * sc + nw, y, h, 120.0 * sc, fade(T500, alpha));
        if a.hidden {
            c.r.text_center(IC_HIDDEN, Font::Icon, c.w - pad - 9.0 * sc, y + h / 2.0, fade(T500, 0.8));
        } else {
            c.text_mid(&count, Font::Small, c.w - pad - cw, y, h, cw + 2.0, T500);
        }
    }
    c.push(pad - 6.0 * sc, y, c.w - pad + 4.0 * sc, y + h, Action::ToggleAlly(a.key.clone()));
    y += h;
    if a.hidden {
        return y + 2.0 * sc;
    }
    // Their quests (folded into one line at overflow level 3).
    if v.compact >= 3 && !a.quests.is_empty() {
        let t = s.p_ally_quests.replace("{n}", &a.quests.len().to_string()).replace("{list}", &a.quests.iter().map(|q| q.1.clone()).collect::<Vec<_>>().join(", "));
        c.text_mid(&t, Font::Small, pad + 28.0 * sc, y, 22.0 * sc, c.w - 2.0 * pad - 28.0 * sc, T500);
        return y + 24.0 * sc;
    }
    for (qid, title, spots, also, q_hidden) in &a.quests {
        let rh = 24.0 * sc;
        let mut right = c.w - pad;
        if !a.site {
            let bs = 20.0 * sc;
            let bx = right - bs;
            let by = y + (rh - bs) / 2.0;
            if c.draw {
                if c.hov() {
                    c.r.fill_rrect(bx, by, bx + bs, by + bs, 5.0 * sc, INK700);
                }
                c.r.text_center(IC_CLOSE, Font::Icon, bx + bs / 2.0, by + bs / 2.0, if c.hov() { GOLD_HI } else { T500 });
            }
            c.push(bx, by, bx + bs, by + bs, Action::RemoveAllyQuest(a.key.clone(), qid.clone()));
            right = bx - 6.0 * sc;
        }
        let qa = if *q_hidden { 0.4 } else { 1.0 };
        let cnt = s.spots(*spots as usize);
        let (cw, _) = c.r.measure(&cnt, Font::Small, 200.0 * sc);
        if c.draw {
            if c.hov() {
                c.r.fill_rrect(pad + 20.0 * sc, y, right + 2.0 * sc, y + rh, 5.0 * sc, HOVER_ROW);
            }
            c.r.stroke_circle(pad + 34.0 * sc, y + rh / 2.0, 4.2 * sc, 1.8 * sc, (a.color.0, a.color.1, a.color.2, qa), false);
            let (tw, _) = c.r.measure(title, Font::Chip, right - cw - pad - 70.0 * sc);
            c.text_mid(title, Font::Chip, pad + 46.0 * sc, y, rh, right - cw - pad - 70.0 * sc, fade(T200, qa));
            if let Some(b) = also {
                let ax = pad + 52.0 * sc + tw;
                let (aw, _) = c.r.measure(s.p_also, Font::Small, 100.0 * sc);
                c.text_mid(s.p_also, Font::Small, ax, y, rh, aw + 2.0, T500);
                if !b.is_empty() {
                    c.r.fill_circle(ax + aw + 10.0 * sc, y + rh / 2.0, 7.0 * sc, (0.8, 0.8, 0.8, 0.75));
                    c.r.text_center(b, Font::Caps, ax + aw + 10.0 * sc, y + rh / 2.0 - 0.5, DARK_INK);
                }
            }
            if *q_hidden {
                c.r.text_center(IC_HIDDEN, Font::Icon, right - 9.0 * sc, y + rh / 2.0, fade(T500, 0.8));
            } else {
                c.text_mid(&cnt, Font::Small, right - cw, y, rh, cw + 2.0, T500);
            }
        }
        c.push(pad + 20.0 * sc, y, right + 2.0 * sc, y + rh, Action::ToggleAllyQuest(a.key.clone(), qid.clone()));
        y += rh;
    }
    y + 4.0 * sc
}

/// Lays out the panel; draws it if `draw` is true.
///
/// The same function is used twice: once with `draw = false` to measure, then with `draw = true` to paint.
/// `hover` is the index of the hovered zone in the returned list, so zones are pushed in the same order in
/// both passes. When the content does not fit in `max_h`, the caller retries with a higher `v.compact`:
/// 1 = untracked quests folded, 2 = done objectives hidden, 3 = 2 objectives per owner, 4 = crafting and
/// search folded; past that the body scrolls (`v.scroll`) under a fixed header.
pub fn layout(r: &Renderer, v: &View, w: f32, max_h: f32, sc: f32, hover: Option<usize>, draw: bool) -> Out {
    let s = v.s;
    let pad = 14.0 * sc;
    let mut c = Ctx { r, sc, draw, hover, hits: vec![], w, pad };
    let iw = w - 2.0 * pad;

    // Frame: body, 1 px dark edge, inner gold hairline 3 px inside (the game's double-line box).
    if draw {
        let (b, rad) = (max_h.max(1.0), 7.0 * sc);
        r.fill_rrect(0.0, 0.0, w, b, rad, (INK950.0, INK950.1, INK950.2, 0.94 * v.opacity.max(0.2)));
        r.stroke_rrect(0.5, 0.5, w - 0.5, b - 0.5, rad, 1.0, (0.0, 0.0, 0.0, 0.7));
        let i = 3.0 * sc;
        r.stroke_rrect(i, i, w - i, b - i, 5.0 * sc, 1.0, (LINE_GOLD.0, LINE_GOLD.1, LINE_GOLD.2, 0.32));
    }

    // Key / network problem.
    let key_problem = match v.status {
        Some(Status::NoKey) => Some(s.st_no_key),
        Some(Status::InvalidKey) => Some(s.st_invalid_key),
        _ => None,
    };
    let net_problem = match v.status {
        Some(Status::Offline) => Some(s.st_offline),
        Some(Status::RateLimited) => Some(s.st_rate),
        _ => None,
    };

    // 1. Header (46): grip, title, sync dot, collapse and settings buttons. The rest of it is the drag handle.
    let hh = 46.0 * sc;
    let bs = 28.0 * sc;
    let by = (hh - bs) / 2.0 + 2.0 * sc;
    let gx = w - pad + 4.0 * sc - bs;
    let cx = gx - bs - 2.0 * sc;
    // Quest journal (book icon), collapse, settings.
    let jx = cx - bs - 2.0 * sc;
    c.icon_btn(jx, by, IC_BOOK, Action::Journal);
    c.icon_btn(cx, by, if v.collapsed { IC_RIGHT } else { IC_DOWN }, Action::Collapse);
    c.icon_btn(gx, by, IC_SETTINGS, Action::Settings);
    if draw {
        for (dx, dy) in [(0.0, -5.0), (0.0, 0.0), (0.0, 5.0), (4.5, -5.0), (4.5, 0.0), (4.5, 5.0)] {
            r.fill_circle(pad + 1.5 * sc + dx * sc, hh / 2.0 + 2.0 * sc + dy * sc, 1.5 * sc, T500);
        }
        let title = if v.collapsed { "COMPANION" } else { "HUNGER COMPANION" };
        let tx = pad + 16.0 * sc;
        let (tw, th) = r.measure_spaced(title, Font::Display, jx - tx, 2.0 * sc);
        r.text_spaced(title, Font::Display, tx, hh / 2.0 + 2.0 * sc - th / 2.0, tw + 4.0, 2.0 * sc, GOLD);
        let dot = if key_problem.is_some() || net_problem.is_some() { WARN } else if v.status.is_none() { (T500.0, T500.1, T500.2) } else { OK };
        let dx = tx + tw + 8.0 * sc;
        r.fill_circle(dx, hh / 2.0 + 2.0 * sc, 3.5 * sc, (dot.0, dot.1, dot.2, 1.0));
        // Collapsed: one-line summary (or the problem).
        if v.collapsed {
            let quests = v.legend.iter().filter(|q| q.kind == OwnerKind::Quest).count();
            let layers = v.layers.iter().filter(|l| l.on).count();
            let summary = key_problem.or(net_problem).map(|p| p.to_string()).unwrap_or_else(|| {
                let t = s.p_summary.replace("{q}", &quests.to_string()).replace("{a}", &v.allies.len().to_string()).replace("{l}", &layers.to_string());
                // No allies: drop that part ("3 quests · 0 allies · 1 layer" → "3 quests · 1 layer").
                if v.allies.is_empty() {
                    t.split(" · ").filter(|p| !p.starts_with('0')).collect::<Vec<_>>().join(" · ")
                } else {
                    t
                }
            });
            let sx = dx + 10.0 * sc;
            c.text_mid(&summary, Font::Chip, sx, 2.0 * sc, hh, jx - sx - 6.0 * sc, if key_problem.is_some() { WARN_TEXT } else { T300 });
        }
    }
    c.push(0.0, 0.0, jx - 2.0 * sc, hh, Action::Move);
    if v.collapsed {
        let h = hh + 4.0 * sc;
        return Out { h, content_h: h, max_scroll: 0.0, hits: c.hits };
    }
    let mut y = hh + 2.0 * sc;
    c.hairline(y);
    y += 10.0 * sc;

    // 2. Key problem: explanation and "Set up…"; nothing else can work without a key.
    if let Some(msg) = key_problem {
        y = callout(&mut c, pad, y, IC_WARN, msg, None, WARN);
        y += 10.0 * sc;
        chip(&mut c, pad, y, 30.0 * sc, None, s.panel_setup, Some(Action::Settings), Chip::Primary);
        y += 30.0 * sc + pad;
        return Out { h: y.min(max_h), content_h: y, max_scroll: 0.0, hits: c.hits };
    }
    if let Some(msg) = net_problem {
        y = callout(&mut c, pad, y, IC_WARN, msg, None, WARN) + 8.0 * sc;
    }
    if v.uncertain {
        y = callout(&mut c, pad, y, IC_INFO, s.p_relocating, None, (T300.0, T300.1, T300.2)) + 8.0 * sc;
    }

    // 3. Toast: result of the last tracking change, with "Undo".
    if let Some(t) = v.toast {
        let th = 32.0 * sc;
        let th2 = if t.next.is_some() { 26.0 * sc } else { 0.0 };
        let tint = if t.ok { OK } else { WARN };
        let undo_w = if t.undo { r.measure(s.toast_undo, Font::Chip, 200.0).0 + 16.0 * sc } else { 0.0 };
        if draw {
            r.fill_rrect(pad, y, w - pad, y + th + th2, 6.0 * sc, (tint.0, tint.1, tint.2, 0.13));
            r.stroke_rrect(pad + 0.5, y + 0.5, w - pad - 0.5, y + th + th2 - 0.5, 6.0 * sc, 1.0, (tint.0, tint.1, tint.2, 0.45));
            r.text_center(if t.ok { IC_CHECK } else { IC_WARN }, Font::Icon, pad + 15.0 * sc, y + th / 2.0, (tint.0, tint.1, tint.2, 1.0));
            c.text_mid(&t.text, Font::Chip, pad + 30.0 * sc, y, th, iw - 36.0 * sc - undo_w, T100);
        }
        if t.undo {
            let ux = w - pad - undo_w;
            if draw {
                let ink = if c.hov() { (1.0, 0.9, 0.7, 1.0) } else { GOLD_HI };
                let (tw, _) = r.measure(s.toast_undo, Font::Chip, 200.0);
                c.text_mid(s.toast_undo, Font::Chip, ux + 6.0 * sc, y, th, tw + 2.0, ink);
                let ly = y + th / 2.0 + c.fh(Font::Chip) / 2.0 - 2.0 * sc;
                r.line(ux + 6.0 * sc, ly, ux + 6.0 * sc + tw, ly, 1.0, ink);
            }
            c.push(ux, y, w - pad, y + th, Action::Undo);
        }
        // Second line: the next quest of the same character, with "+ Track".
        if let Some((qid, title)) = &t.next {
            let ny = y + th - 6.0 * sc;
            let (bw, _) = r.measure(s.p_track, Font::Chip, 200.0 * sc);
            let bw = bw + 20.0 * sc;
            let bh = 24.0 * sc;
            let bx = w - pad - bw - 6.0 * sc;
            if draw {
                let lab = format!("{} {title}", s.toast_next);
                c.text_mid(&lab, Font::Chip, pad + 30.0 * sc, ny, th2, bx - pad - 40.0 * sc, T200);
            }
            chip(&mut c, bx, ny + (th2 - bh) / 2.0, bh, None, s.p_track, Some(Action::Track(qid.clone())), Chip::Ghost);
        }
        y += th + th2 + 8.0 * sc;
    }

    // 4. Search field (opens the search window).
    let fh = 34.0 * sc;
    if draw {
        let hov = c.hov();
        r.fill_rrect(pad, y, w - pad, y + fh, 6.0 * sc, INK900);
        r.stroke_rrect(pad + 0.5, y + 0.5, w - pad - 0.5, y + fh - 0.5, 6.0 * sc, 1.0, if hov { fade(GOLD, 0.6) } else { (1.0, 1.0, 1.0, 0.10) });
        r.text_center(IC_SEARCH, Font::Icon, pad + 17.0 * sc, y + fh / 2.0, T500);
        c.text_mid(s.p_find, Font::Chip, pad + 34.0 * sc, y, fh, iw - 40.0 * sc, if hov { T300 } else { T500 });
    }
    c.push(pad, y, w - pad, y + fh, Action::Search);
    y += fh + 8.0 * sc;

    // 5. Quick settings: Show done, chance stepper, language (wraps first on narrow panels).
    let qh = 30.0 * sc;
    let mut x = pad;
    let done_w = {
        let t = s.panel_done;
        let (tw, _) = r.measure(t, Font::Chip, 300.0 * sc);
        let cw = tw + 40.0 * sc;
        if draw {
            let hov = c.hov();
            r.fill_rrect(x, y, x + cw, y + qh, 6.0 * sc, if hov { INK700 } else { INK800 });
            r.stroke_rrect(x + 0.5, y + 0.5, x + cw - 0.5, y + qh - 0.5, 6.0 * sc, 1.0, if hov { fade(GOLD, 0.5) } else { (1.0, 1.0, 1.0, 0.08) });
            let (bx, bsz) = (x + 10.0 * sc, 13.0 * sc);
            let bt = y + (qh - bsz) / 2.0;
            if v.show_done {
                r.fill_rrect(bx, bt, bx + bsz, bt + bsz, 3.0 * sc, ACCENT);
                r.line_round(bx + 3.0 * sc, bt + 6.8 * sc, bx + 5.6 * sc, bt + 9.4 * sc, 1.6 * sc, DARK_INK);
                r.line_round(bx + 5.6 * sc, bt + 9.4 * sc, bx + 10.2 * sc, bt + 3.8 * sc, 1.6 * sc, DARK_INK);
            } else {
                r.stroke_rrect(bx + 0.5, bt + 0.5, bx + bsz - 0.5, bt + bsz - 0.5, 3.0 * sc, 1.2 * sc, T300);
            }
            c.text_mid(t, Font::Chip, bx + bsz + 7.0 * sc, y, qh, tw + 2.0, T200);
        }
        c.push(x, y, x + cw, y + qh, Action::ToggleDone);
        cw
    };
    x += done_w + 6.0 * sc;
    // Chance stepper: [−] Chance ≥ 10 % [+], one segmented chip.
    let ct = format!("{} {} %", s.panel_chance, v.min_chance);
    let (ctw, _) = r.measure(&ct, Font::Chip, 300.0 * sc);
    let seg = 26.0 * sc;
    let sw = seg * 2.0 + ctw + 16.0 * sc;
    if draw {
        r.fill_rrect(x, y, x + sw, y + qh, 6.0 * sc, INK800);
        r.stroke_rrect(x + 0.5, y + 0.5, x + sw - 0.5, y + qh - 0.5, 6.0 * sc, 1.0, (1.0, 1.0, 1.0, 0.08));
        r.line(x + seg, y + 5.0 * sc, x + seg, y + qh - 5.0 * sc, 1.0, (1.0, 1.0, 1.0, 0.10));
        r.line(x + sw - seg, y + 5.0 * sc, x + sw - seg, y + qh - 5.0 * sc, 1.0, (1.0, 1.0, 1.0, 0.10));
        c.text_mid(&ct, Font::Chip, x + seg + 8.0 * sc, y, qh, ctw + 2.0, T200);
    }
    for (bx, t, a) in [(x, "−", Action::ChanceDown), (x + sw - seg, "+", Action::ChanceUp)] {
        if draw {
            if c.hov() {
                r.fill_rrect(bx + 2.0 * sc, y + 2.0 * sc, bx + seg - 2.0 * sc, y + qh - 2.0 * sc, 5.0 * sc, INK700);
            }
            r.text_center(t, Font::Chip, bx + seg / 2.0, y + qh / 2.0 - 1.0 * sc, if c.hov() { GOLD_HI } else { T100 });
        }
        c.push(bx, y, bx + seg, y + qh, a);
    }
    x += sw + 6.0 * sc;
    // Language: a chip that opens a list (drawn at the end, over the panel).
    let lang = format!("{}  \u{2304}", v.lang_label);
    let (lw, _) = r.measure(&lang, Font::Chip, 300.0 * sc);
    let lw = lw + 40.0 * sc;
    let lang_x = if x + lw > w - pad {
        y += qh + 6.0 * sc;
        pad
    } else {
        w - pad - lw
    };
    chip(&mut c, lang_x, y, qh, Some(IC_GLOBE), &lang, Some(Action::Language), if v.lang_open { Chip::Ghost } else { Chip::Normal });
    let lang_chip = (lang_x, y + qh);
    y += qh + 10.0 * sc;
    c.hairline(y);
    y += 2.0 * sc;

    // ---- Scrollable body ----
    let body_top = y;
    let body_bottom = max_h - 6.0 * sc;
    let first_body_hit = c.hits.len();
    if draw {
        r.push_clip(0.0, body_top, w, body_bottom);
    }
    let mut y = body_top - v.scroll;
    let collapsed = |id: &str| v.sections_collapsed.iter().any(|x| x == id);

    // 6. QUESTS.
    let quests: Vec<&LegendQuest> = v.legend.iter().filter(|q| q.kind == OwnerKind::Quest).collect();
    let open_q = !collapsed("quests");
    y = section_eye(&mut c, y, "quests", s.p_sec_quests, (!quests.is_empty()).then_some(quests.len()), None, !open_q, Some((v.quest_markers_hidden, Action::ToggleQuestMarkers)));
    if v.quest_markers_hidden {
        let rh = 22.0 * sc;
        if draw {
            r.text_center(IC_HIDDEN, Font::Icon, pad + 6.0 * sc, y + rh / 2.0, T500);
            c.text_mid(s.p_markers_hidden, Font::Small, pad + 20.0 * sc, y, rh, iw - 20.0 * sc, T500);
        }
        y += rh;
    }
    if open_q {
        // Quests with markers here (or hidden, or filtered out) are listed; the others share one line.
        let other = |q: &LegendQuest| !q.hidden && q.on_this_map == 0 && q.filtered == 0 && !v.other_open.contains(&q.quest_id);
        for q in quests.iter().filter(|q| !other(q)) {
            y = owner(&mut c, v, y, q);
        }
        let others: Vec<&&LegendQuest> = quests.iter().filter(|q| other(q)).collect();
        if !others.is_empty() {
            let rh = 26.0 * sc;
            let (lw, _) = r.measure(s.p_other_map, Font::Small, 300.0 * sc);
            c.text_mid(s.p_other_map, Font::Small, pad + 4.0 * sc, y, rh, lw + 2.0, T500);
            let mut x = pad + 4.0 * sc + lw + 8.0 * sc;
            for q in others {
                let (tw, _) = r.measure(&q.title, Font::Small, 300.0 * sc);
                let iw2 = 24.0 * sc + tw;
                if x + iw2 > w - pad && x > pad + lw + 20.0 * sc {
                    x = pad + 4.0 * sc + lw + 8.0 * sc;
                    y += rh;
                }
                if draw {
                    if c.hov() {
                        r.fill_rrect(x - 4.0 * sc, y + 2.0 * sc, x + iw2 + 2.0 * sc, y + rh - 2.0 * sc, 5.0 * sc, HOVER_ROW);
                    }
                    let k = 0.78;
                    let col = markers::guard(q.color);
                    r.fill_circle(x + 8.0 * sc, y + rh / 2.0, 9.5 * sc * k, (col.0, col.1, col.2, 0.7));
                    if !q.badge.is_empty() {
                        r.text_center(&q.badge, Font::Caps, x + 8.0 * sc, y + rh / 2.0 - 0.5, DARK_INK);
                    }
                    c.text_mid(&q.title, Font::Small, x + 20.0 * sc, y, rh, tw + 2.0, T300);
                }
                c.push(x - 4.0 * sc, y, x + iw2, y + rh, Action::ExpandOther(q.quest_id.clone()));
                x += iw2 + 10.0 * sc;
            }
            y += rh + 2.0 * sc;
        }
        // Nothing tracked at all.
        if v.legend.is_empty() {
            let (_, h1) = r.measure(s.p_empty, Font::Bold, iw);
            if draw {
                r.text(s.p_empty, Font::Bold, pad, y + 2.0 * sc, iw, T200);
            }
            y += h1 + 4.0 * sc;
            let (_, h2) = r.measure(s.p_empty_hint, Font::Small, iw);
            if draw {
                r.text(s.p_empty_hint, Font::Small, pad, y, iw, T500);
            }
            y += h2 + 8.0 * sc;
            chip(&mut c, pad, y, 30.0 * sc, Some(IC_BOOK), s.p_empty_btn, Some(Action::Journal), Chip::Primary);
            y += 30.0 * sc + 6.0 * sc;
        }
        // "Up next": quests the player can start now (the website's prerequisites). Without them, the untracked
        // quests on this map (Quests layer ON), as before.
        let rows: Vec<(String, String, String, bool)> = if !v.up_next.is_empty() {
            v.up_next
                .iter()
                .map(|u| (u.quest_id.clone(), u.title.clone(), match u.step { Some((n, t)) => format!("{} · {n}/{t}", u.giver), None => u.giver.clone() }, u.here))
                .collect()
        } else {
            vec![]
        };
        let up = !v.up_next.is_empty();
        if !rows.is_empty() {
            let sh = 22.0 * sc;
            y += 6.0 * sc;
            let t = format!("{}  {}", if up { s.p_up_next } else { s.p_avail }, rows.len());
            let hint = if up { s.p_up_next_hint } else { s.p_avail_hint };
            let (hw, _) = r.measure(hint, Font::Small, 300.0 * sc);
            if draw {
                let (tw, th) = r.measure_spaced(&t, Font::Caps, iw - hw - 10.0 * sc, 1.2 * sc);
                r.text_spaced(&t, Font::Caps, pad, y + (sh - th) / 2.0, tw + 4.0, 1.2 * sc, T500);
                c.text_mid(hint, Font::Small, w - pad - hw, y, sh, hw + 2.0, T500);
            }
            y += sh;
            let limit = if v.avail_open { usize::MAX } else if v.compact >= 1 { 0 } else { 3 };
            for (qid, title, sub, here) in rows.iter().take(limit) {
                let rh = 30.0 * sc;
                let btn = s.p_track;
                let (bw, _) = r.measure(btn, Font::Chip, 200.0 * sc);
                let bw = bw + 20.0 * sc;
                let bh = 24.0 * sc;
                let bx = w - pad - bw;
                // The button first: it wins over the row when clicked.
                chip(&mut c, bx, y + (rh - bh) / 2.0, bh, None, btn, Some(Action::Track(qid.clone())), Chip::Ghost);
                if draw {
                    if c.hov() {
                        r.fill_rrect(pad - 6.0 * sc, y, bx - 4.0 * sc, y + rh, 6.0 * sc, HOVER_ROW);
                    }
                    // "!" token when it has spots on this map, a hollow ring otherwise.
                    if *here {
                        markers::draw(r, markers::Kind::QuestAvail, markers::QUEST_AVAIL, "", pad + 9.0 * sc, y + rh / 2.0, 0.8 * sc, sc, 1.0);
                    } else {
                        let qa = markers::QUEST_AVAIL;
                        r.stroke_circle(pad + 9.0 * sc, y + rh / 2.0, 5.0 * sc, 1.4 * sc, (qa.0, qa.1, qa.2, 0.8), false);
                    }
                    let (gw, _) = r.measure(sub, Font::Small, 160.0 * sc);
                    let tmax = bx - 8.0 * sc - (pad + 24.0 * sc) - gw - 8.0 * sc;
                    let (tw, _) = r.measure(title, Font::Chip, tmax.max(20.0));
                    c.text_mid(title, Font::Chip, pad + 24.0 * sc, y, rh, tmax.max(20.0), T200);
                    c.text_mid(sub, Font::Small, pad + 32.0 * sc + tw, y, rh, gw + 2.0, T500);
                }
                c.push(pad - 6.0 * sc, y, bx - 4.0 * sc, y + rh, Action::AvailRow(qid.clone()));
                y += rh;
            }
            let hidden = rows.len().saturating_sub(limit);
            if hidden > 0 || (v.avail_open && rows.len() > 3) {
                let rh = 24.0 * sc;
                let t = if v.avail_open { format!("{} \u{2303}", s.p_less) } else { format!("{} \u{2304}", s.p_more.replace("{n}", &hidden.to_string())) };
                let (tw, _) = r.measure(&t, Font::Small, 300.0 * sc);
                if draw {
                    c.text_mid(&t, Font::Small, pad + 24.0 * sc, y, rh, tw + 2.0, if c.hov() { GOLD_HI } else { GOLD });
                }
                c.push(pad + 20.0 * sc, y, pad + 28.0 * sc + tw, y + rh, Action::MoreAvail);
                y += rh;
            }
        }
        // Repeatable quests available: a link to the journal.
        if v.repeat_avail > 0 {
            let rh = 24.0 * sc;
            let t = s.p_repeat_link.replace("{n}", &v.repeat_avail.to_string());
            let (tw, _) = r.measure(&t, Font::Small, 300.0 * sc);
            if draw {
                c.text_mid(&t, Font::Small, pad + 24.0 * sc, y, rh, tw + 2.0, if c.hov() { GOLD_HI } else { GOLD });
            }
            c.push(pad + 20.0 * sc, y, pad + 28.0 * sc + tw, y + rh, Action::Journal);
            y += rh;
        }
        y += 4.0 * sc;
    }

    // 6 bis. ALLIES (raid mates, at most 2).
    {
        c.hairline(y + 2.0 * sc);
        let folded = collapsed("allies") || (v.compact >= 4 && !v.allies.is_empty());
        let right = if v.allies.is_empty() { s.p_add.to_string() } else { s.p_manage.to_string() };
        let right = if folded && !v.allies.is_empty() { (v.allies.iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", "), None) } else { (right, Some(Action::ManageAllies)) };
        y = section(&mut c, y, "allies", s.p_sec_allies, (!v.allies.is_empty()).then_some(v.allies.len()), Some(right), folded);
        if !folded {
            if v.allies.is_empty() {
                let (_, h) = r.measure(s.p_allies_empty, Font::Small, iw - 30.0 * sc);
                if draw {
                    r.stroke_circle(pad + 9.0 * sc, y + 10.0 * sc, 8.0 * sc, 1.2 * sc, fade(T500, 0.7), false);
                    r.text_center("+", Font::Chip, pad + 9.0 * sc, y + 9.0 * sc, T500);
                    r.text(s.p_allies_empty, Font::Small, pad + 26.0 * sc, y + 2.0 * sc, iw - 30.0 * sc, T500);
                }
                y += h.max(20.0 * sc) + 8.0 * sc;
            }
            for a in v.allies {
                y = ally_rows(&mut c, v, y, a);
            }
            y += 4.0 * sc;
        }
    }

    // 7. CRAFTING and 8. SEARCH (only when they have rows).
    for (id, title, kind) in [("crafting", s.p_sec_crafting, OwnerKind::Craft), ("search", s.p_sec_search, OwnerKind::Search)] {
        let rows: Vec<&LegendQuest> = v.legend.iter().filter(|q| q.kind == kind).collect();
        if rows.is_empty() {
            continue;
        }
        c.hairline(y + 2.0 * sc);
        let folded = collapsed(id) || v.compact >= 4;
        let right = if kind == OwnerKind::Search && rows.len() >= 2 && !folded {
            Some((s.p_clear_all.to_string(), Some(Action::ClearSearches)))
        } else if folded {
            Some((rows[0].title.clone(), None))
        } else {
            None
        };
        y = section(&mut c, y, id, title, Some(rows.len()), right, folded);
        if !folded {
            for q in rows {
                y = owner(&mut c, v, y, q);
            }
            y += 4.0 * sc;
        }
    }

    // 9. MAP LAYERS: pills in a two-column grid, by group.
    if !v.layers.is_empty() {
        c.hairline(y + 2.0 * sc);
        let on = v.layers.iter().filter(|l| l.on).count();
        let folded = collapsed("layers");
        y = section(&mut c, y, "layers", s.p_sec_layers, None, Some((s.p_layers_on.replace("{n}", &on.to_string()), None)), folded);
        if !folded {
            let names = [s.g_survival, s.g_loot, s.g_world];
            for (gi, ids) in GROUPS.iter().enumerate() {
                let members: Vec<&LegendLayer> = v
                    .layers
                    .iter()
                    .filter(|l| if gi == GROUPS.len() - 1 { ids.contains(&l.id.as_str()) || !GROUPS.iter().any(|g| g.contains(&l.id.as_str())) } else { ids.contains(&l.id.as_str()) })
                    .collect();
                if members.is_empty() {
                    continue;
                }
                // Group label: toggles the whole group.
                let gh = 24.0 * sc;
                let (gw, gth) = r.measure_spaced(names[gi], Font::Caps, iw, 1.2 * sc);
                if draw {
                    r.text_spaced(names[gi], Font::Caps, pad, y + 2.0 * sc + (gh - gth) / 2.0, gw + 4.0, 1.2 * sc, if c.hov() { GOLD_HI } else { T500 });
                }
                c.push(pad - 4.0 * sc, y + 2.0 * sc, pad + gw + 6.0 * sc, y + 2.0 * sc + gh, Action::ToggleGroup(members.iter().map(|l| l.id.clone()).collect()));
                y += gh + 4.0 * sc;
                let pw = (iw - 6.0 * sc) / 2.0;
                let ph = 30.0 * sc;
                for (k, ly) in members.iter().enumerate() {
                    let px = pad + (k % 2) as f32 * (pw + 6.0 * sc);
                    if draw {
                        let hov = c.hov();
                        let (fill, edge) = if ly.on { (INK700, fade(GOLD, if hov { 0.85 } else { 0.55 })) } else { ((0.0, 0.0, 0.0, 0.18), if hov { fade(GOLD, 0.5) } else { (1.0, 1.0, 1.0, 0.08) }) };
                        r.fill_rrect(px, y, px + pw, y + ph, 6.0 * sc, fill);
                        r.stroke_rrect(px + 0.5, y + 0.5, px + pw - 0.5, y + ph - 0.5, 6.0 * sc, 1.0, edge);
                        let kind = markers::kind_for_layer(&ly.id, ly.diamond);
                        let col = markers::layer_color(&ly.id, ly.color);
                        markers::draw(r, kind, col, "", px + 15.0 * sc, y + ph / 2.0, 0.8 * sc, sc, if ly.on { 1.0 } else { 0.42 });
                        let count = ly.here.to_string();
                        let (nw, _) = r.measure(&count, Font::Caps, 100.0);
                        let new_tag = false;
                        let tag_w = if new_tag { r.measure(s.p_new, Font::Caps, 200.0).0 + 10.0 * sc } else { 0.0 };
                        let name_x = px + 30.0 * sc;
                        let name_max = pw - 30.0 * sc - nw - 14.0 * sc - tag_w;
                        let (tw, _) = r.measure(&ly.title, Font::Chip, name_max.max(10.0));
                        c.text_mid(&ly.title, Font::Chip, name_x, y, ph, name_max.max(10.0), if ly.on { T100 } else { T500 });
                        if new_tag {
                            let (tx, th) = (name_x + tw + 5.0 * sc, 15.0 * sc);
                            let ty = y + (ph - th) / 2.0;
                            r.fill_rrect(tx, ty, tx + tag_w - 4.0 * sc, ty + th, 4.0 * sc, ACCENT);
                            r.text_center(s.p_new, Font::Caps, tx + (tag_w - 4.0 * sc) / 2.0, ty + th / 2.0 - 0.5, DARK_INK);
                        }
                        c.text_mid(&count, Font::Caps, px + pw - nw - 9.0 * sc, y, ph, nw + 2.0, T500);
                    }
                    c.push(px, y, px + pw, y + ph, Action::ToggleLayer(ly.id.clone()));
                    if k % 2 == 1 || k == members.len() - 1 {
                        y += ph + 6.0 * sc;
                    }
                }
            }
        }
    }

    // End of the body: overflow handling.
    let content_end = y + v.scroll + pad - 4.0 * sc;
    let content_h = content_end;
    let max_scroll = (content_h - max_h).max(0.0);
    if draw {
        r.pop_clip();
        // More below: fade to the background and a "scroll" hint.
        if max_scroll > 0.5 && v.scroll < max_scroll - 0.5 {
            let fade_h = 30.0 * sc;
            for k in 0..6 {
                let t0 = body_bottom - fade_h + fade_h * k as f32 / 6.0;
                r.fill_rrect(4.0 * sc, t0, w - 4.0 * sc, body_bottom, 0.0, (INK950.0, INK950.1, INK950.2, 0.16));
            }
            let t = format!("{} \u{2304}", s.p_scroll);
            let (tw, th) = r.measure(&t, Font::Caps, w);
            r.text(&t, Font::Caps, (w - tw) / 2.0, body_bottom - th - 2.0 * sc, tw + 2.0, T300);
        }
    }
    // Body zones scrolled out of view cannot be clicked.
    for h in c.hits.iter_mut().skip(first_body_hit) {
        h.t = h.t.max(body_top);
        h.b = h.b.min(body_bottom);
        if h.b <= h.t {
            h.t = -1.0;
            h.b = -1.0;
        }
    }
    // Language list, over everything: "Auto (XX)" then every language. While it is open, only the list and the
    // chip can be clicked (a click anywhere else closes it).
    let mut h_out = content_h.min(max_h);
    if v.lang_open {
        for h in c.hits.iter_mut().filter(|h| h.action != Action::Language) {
            h.t = -1.0;
            h.b = -1.0;
        }
        let entries: Vec<(String, String)> = std::iter::once((String::new(), crate::i18n::auto_label()))
            .chain(crate::i18n::LANGS.iter().map(|l| (l.0.to_string(), l.1.to_string())))
            .collect();
        let ih = 28.0 * sc;
        let lw = entries.iter().map(|e| r.measure(&e.1, Font::Chip, 400.0 * sc).0).fold(0.0, f32::max) + 52.0 * sc;
        let lx = (lang_chip.0 + (w - pad - lang_chip.0) - lw).clamp(pad, w - pad - lw);
        let ly = lang_chip.1 + 4.0 * sc;
        let lh = ih * entries.len() as f32 + 8.0 * sc;
        if draw {
            r.fill_rrect(lx + 2.0 * sc, ly + 3.0 * sc, lx + lw + 2.0 * sc, ly + lh + 3.0 * sc, 7.0 * sc, (0.0, 0.0, 0.0, 0.4));
            r.fill_rrect(lx, ly, lx + lw, ly + lh, 7.0 * sc, (INK950.0, INK950.1, INK950.2, 0.98));
            r.stroke_rrect(lx + 0.5, ly + 0.5, lx + lw - 0.5, ly + lh - 0.5, 7.0 * sc, 1.0, (LINE_GOLD.0, LINE_GOLD.1, LINE_GOLD.2, 0.45));
        }
        let mut iy = ly + 4.0 * sc;
        for (code, name) in entries {
            let sel = code == v.locale;
            if draw {
                if c.hov() {
                    r.fill_rrect(lx + 4.0 * sc, iy, lx + lw - 4.0 * sc, iy + ih, 5.0 * sc, INK700);
                }
                if sel {
                    r.text_center(IC_CHECK, Font::Icon, lx + 18.0 * sc, iy + ih / 2.0, ACCENT);
                }
                c.text_mid(&name, Font::Chip, lx + 34.0 * sc, iy, ih, lw - 40.0 * sc, if sel { GOLD_HI } else if c.hov() { T100 } else { T200 });
            }
            c.push(lx + 4.0 * sc, iy, lx + lw - 4.0 * sc, iy + ih, Action::SetLang(code));
            iy += ih;
        }
        h_out = h_out.max(ly + lh + 8.0 * sc);
    }
    Out { h: h_out, content_h, max_scroll, hits: c.hits }
}
