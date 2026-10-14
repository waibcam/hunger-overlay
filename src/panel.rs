//! Clickable panel placed over the map (in place of the legend): tracked quests, quick settings and access
//! to the settings window. It is a small separate window that receives clicks WITHOUT taking focus away
//! from the game (WS_EX_NOACTIVATE); the rest of the overlay lets every click pass through to the game.

use crate::i18n::Strings;
use crate::model::{LegendLayer, LegendQuest};
use crate::net::Status;
use crate::render::{Font, Renderer, Rgba};

/// What a click on a panel zone does (handled by the panel window's owner).
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Handle: drag to move the panel, double-click to put it back in its default place.
    Move,
    /// Open the settings window.
    Settings,
    /// Collapse / expand the panel to its title bar.
    Collapse,
    /// Show / hide objectives that are already done.
    ToggleDone,
    /// Lower the minimum loot chance filter.
    ChanceDown,
    /// Raise the minimum loot chance filter.
    ChanceUp,
    /// Switch the display language.
    Language,
    /// Hide / show one tracked quest (by quest id).
    ToggleQuest(String),
    /// Turn one map layer on / off (by layer id).
    ToggleLayer(String),
    /// Open the item search window.
    Search,
    /// Remove one item search from the map (by search key).
    RemoveSearch(String),
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

/// Everything the panel displays, gathered by the caller for one layout/draw pass.
pub struct View<'a> {
    /// UI strings in the current language.
    pub s: &'static Strings,
    /// Tracked quests (and item searches) with their objectives, for the current map.
    pub legend: &'a [LegendQuest],
    /// Optional map layers offered by the pack.
    pub layers: &'a [LegendLayer],
    /// Connection / key status, if known.
    pub status: Option<&'a Status>,
    /// Whether objectives already done are listed.
    pub show_done: bool,
    /// Minimum loot chance (percent) below which loot markers are hidden.
    pub min_chance: u32,
    /// Label of the current language (shown on the language button).
    pub lang_label: String,
    /// Panel reduced to its title bar.
    pub collapsed: bool,
    /// Background opacity (0..1).
    pub opacity: f32,
}

/// Main text colour.
const TEXT: Rgba = (0.97, 0.95, 0.9, 1.0);
/// Dimmed text (secondary info, hidden quests).
const DIM: Rgba = (0.7, 0.68, 0.62, 1.0);
/// Accent colour for titles and icons.
const GOLD: Rgba = (0.93, 0.88, 0.76, 1.0);
/// Warning colour (key / network problems, filtered markers).
const WARN: Rgba = (1.0, 0.72, 0.45, 1.0);

/// Lays out the panel; draws it if `draw` is true. Returns (height, clickable zones).
///
/// The same function is used twice: once with `draw = false` to measure the height, then with
/// `draw = true` to paint. `hover` is the index of the hovered zone in the returned list, so zones
/// must be pushed in the same order in both passes. `sc` is the UI scale factor.
pub fn layout(r: &Renderer, v: &View, w: f32, max_h: f32, sc: f32, hover: Option<usize>, draw: bool) -> (f32, Vec<Hit>) {
    let s = v.s;
    let pad = 14.0 * sc;
    let iw = w - 2.0 * pad;
    let mut hits: Vec<Hit> = vec![];
    let mut y = pad;
    // True if the zone about to be pushed (index = current length) is the hovered one.
    let hov = |hits: &Vec<Hit>| hover.map(|h| h == hits.len()).unwrap_or(false);
    // Background and border.
    if draw {
        r.fill_rrect(0.5, 0.5, w - 0.5, max_h.max(1.0) - 0.5, 8.0 * sc, (0.07, 0.06, 0.05, v.opacity));
        r.stroke_rrect(0.5, 0.5, w - 0.5, max_h.max(1.0) - 0.5, 8.0 * sc, 1.0 * sc, (0.85, 0.75, 0.55, 0.35));
    }

    // Small button (chip): returns its width.
    let chip = |hits: &mut Vec<Hit>, x: f32, y: f32, text: &str, action: Action, accent: bool| -> f32 {
        let (tw, th) = r.measure(text, Font::Small, 400.0 * sc);
        let (cw, ch) = (tw + 16.0 * sc, th + 8.0 * sc);
        if draw {
            let h = hover.map(|h| h == hits.len()).unwrap_or(false);
            let bg = if h { (0.32, 0.27, 0.2, 0.95) } else if accent { (0.45, 0.3, 0.12, 0.95) } else { (0.18, 0.16, 0.13, 0.95) };
            r.fill_rrect(x, y, x + cw, y + ch, 5.0 * sc, bg);
            r.stroke_rrect(x, y, x + cw, y + ch, 5.0 * sc, 1.0 * sc, (0.85, 0.75, 0.55, if h { 0.8 } else { 0.4 }));
            r.text(text, Font::Small, x + 8.0 * sc, y + 4.0 * sc, tw + 2.0, TEXT);
        }
        hits.push(Hit { l: x, t: y, r: x + cw, b: y + ch, action });
        cw
    };

    // 1. Title + "collapse" and "settings" buttons.
    let (_, th) = r.measure(s.legend_title, Font::Title, iw);
    let (gw, gh) = r.measure("⚙", Font::Bold, 100.0);
    let gs = gw.max(gh) + 8.0 * sc; // side of the square icon buttons
    // Square icon button on the title row.
    let icon_btn =|hits: &mut Vec<Hit>, x: f32, glyph: &str, action: Action| {
        if draw {
            let (bw, bh) = r.measure(glyph, Font::Bold, 100.0);
            if hover.map(|h| h == hits.len()).unwrap_or(false) {
                r.fill_rrect(x, y, x + gs, y + gs, 5.0 * sc, (0.32, 0.27, 0.2, 0.95));
            }
            r.stroke_rrect(x, y, x + gs, y + gs, 5.0 * sc, 1.0 * sc, (0.85, 0.75, 0.55, 0.5));
            r.text(glyph, Font::Bold, x + (gs - bw) / 2.0, y + (gs - bh) / 2.0, bw + 2.0, GOLD);
        }
        hits.push(Hit { l: x, t: y, r: x + gs, b: y + gs, action });
    };
    // Buttons right-aligned, from right to left: settings, collapse, move.
    let gx = w - pad - gs;
    let cx = gx - gs - 6.0 * sc;
    let mx = cx - gs - 6.0 * sc;
    if draw {
        r.text(s.legend_title, Font::Title, pad, y + (gs - th) / 2.0, mx - pad - 4.0 * sc, GOLD);
    }
    icon_btn(&mut hits, mx, "✥", Action::Move);
    icon_btn(&mut hits, cx, if v.collapsed { "▸" } else { "▾" }, Action::Collapse);
    icon_btn(&mut hits, gx, "⚙", Action::Settings);
    y += gs + 8.0 * sc;

    // 2. Key / network problem. The bool says whether to offer a "set up" button (key problems only).
    let warn = match v.status {
        Some(Status::NoKey) => Some((s.st_no_key, true)),
        Some(Status::InvalidKey) => Some((s.st_invalid_key, true)),
        Some(Status::Offline) => Some((s.st_offline, false)),
        Some(Status::RateLimited) => Some((s.st_rate, false)),
        _ => None,
    };
    if let Some((msg, setup)) = warn {
        let t = format!("⚠ {msg}");
        let (_, h) = r.measure(&t, Font::Text, iw);
        if draw {
            r.text(&t, Font::Text, pad, y, iw, WARN);
        }
        y += h + 6.0 * sc;
        if setup {
            let (_, ch) = r.measure(s.panel_setup, Font::Small, 400.0);
            chip(&mut hits, pad, y, s.panel_setup, Action::Settings, true);
            y += ch + 8.0 * sc + 8.0 * sc;
        }
    }

    // Collapsed panel: only the title bar (and the key problem, if any).
    if v.collapsed {
        return ((y + pad - 12.0 * sc).min(max_h), hits);
    }

    // 3. Quick settings (on one or two rows depending on the width).
    let done_t = format!("{} {}", if v.show_done { "☑" } else { "☐" }, s.panel_done);
    let chance_t = format!("{} {} %", s.panel_chance, v.min_chance);
    let lang_t = format!("🌐 {}", v.lang_label);
    let items: Vec<(String, Action)> = vec![
        (done_t, Action::ToggleDone),
        ("−".into(), Action::ChanceDown),
        (chance_t, Action::ChanceDown),
        ("+".into(), Action::ChanceUp),
        (lang_t, Action::Language),
    ];
    let (_, ch) = r.measure("Ag", Font::Small, 400.0);
    let row_h = ch + 8.0 * sc;
    let mut x = pad;
    for (i, (t, a)) in items.iter().enumerate() {
        let (tw, _) = r.measure(t, Font::Small, 400.0 * sc);
        let cw = tw + 16.0 * sc;
        // Wrap to a new row when the item does not fit.
        if x + cw > w - pad && x > pad {
            x = pad;
            y += row_h + 6.0 * sc;
        }
        if i == 2 {
            // Chance label: plain text between the − and + buttons.
            if draw {
                r.text(t, Font::Small, x + 4.0 * sc, y + 4.0 * sc, tw + 2.0, TEXT);
            }
            x += tw + 8.0 * sc;
            continue;
        }
        x += chip(&mut hits, x, y, t, a.clone(), false) + 6.0 * sc;
    }
    // Item search: on its own row, followed by a separator line.
    y += row_h + 6.0 * sc;
    chip(&mut hits, pad, y, s.panel_search, Action::Search, false);
    y += row_h + 10.0 * sc;
    if draw {
        r.line(pad, y - 4.0 * sc, w - pad, y - 4.0 * sc, 1.0, (0.85, 0.75, 0.55, 0.25));
    }
    y += 4.0 * sc;

    // 4. Tracked quests (click on the title = hide / show again).
    // "No tracked quest" hint, unless a key problem already explains why the list is empty.
    if v.legend.is_empty() && !matches!(warn, Some((_, true))) {
        let (_, h) = r.measure(s.legend_none, Font::Small, iw);
        if draw {
            r.text(s.legend_none, Font::Small, pad, y, iw, DIM);
        }
        y += h + 4.0 * sc;
    }
    let gap = 3.0 * sc;
    for q in v.legend {
        // Measuring pass: stop once the panel is full.
        if y > max_h - pad - 10.0 * sc && !draw {
            break;
        }
        // Item searches appear as pseudo-quests whose id starts with "find:"; clicking removes them.
        let find_key = q.quest_id.strip_prefix("find:");
        let title = if q.hidden {
            format!("{}  ({})", q.title, s.panel_hidden)
        } else if find_key.is_some() {
            format!("{}  ✕", q.title)
        } else {
            q.title.clone()
        };
        let indent = 20.0 * sc;
        let (_, h) = r.measure(&title, Font::Bold, iw - indent);
        // Quest title row: colour dot + title (dimmed when hidden).
        if draw {
            if hov(&hits) {
                r.fill_rrect(pad - 6.0 * sc, y - 2.0 * sc, w - pad + 6.0 * sc, y + h + 2.0 * sc, 4.0 * sc, (1.0, 1.0, 1.0, 0.08));
            }
            let a = if q.hidden { 0.35 } else { 1.0 };
            r.fill_circle(pad + 6.0 * sc, y + h / 2.0, 6.0 * sc, (q.color.0, q.color.1, q.color.2, a));
            r.stroke_circle(pad + 6.0 * sc, y + h / 2.0, 6.0 * sc, 1.2 * sc, (1.0, 1.0, 1.0, 0.9 * a), false);
            r.text(&title, Font::Bold, pad + indent, y, iw - indent, if q.hidden { DIM } else { TEXT });
        }
        let action = match find_key {
            Some(k) => Action::RemoveSearch(k.to_string()),
            None => Action::ToggleQuest(q.quest_id.clone()),
        };
        hits.push(Hit { l: pad - 6.0 * sc, t: y - 2.0 * sc, r: w - pad + 6.0 * sc, b: y + h + 2.0 * sc, action });
        y += h + gap;
        if q.hidden {
            continue;
        }
        // No marker on this map: explain whether they are filtered out or on another map.
        if q.on_this_map == 0 {
            let msg = if q.filtered > 0 { s.legend_filtered } else { s.legend_elsewhere };
            let (_, h) = r.measure(msg, Font::Small, iw - indent);
            if draw {
                r.text(msg, Font::Small, pad + indent, y, iw - indent, if q.filtered > 0 { WARN } else { DIM });
            }
            y += h + gap;
        }
        // Objectives: ✓ done, ● has markers on this map, ○ no marker here.
        for o in &q.objectives {
            if o.done && !v.show_done {
                continue;
            }
            let (mark, c) = if o.done { ("✓", (0.55, 0.55, 0.55, 1.0)) } else if o.markers_here > 0 { ("●", (0.92, 0.9, 0.85, 1.0)) } else { ("○", (0.75, 0.73, 0.68, 1.0)) };
            let t = format!("{mark}  {}", o.label);
            let (_, h) = r.measure(&t, Font::Text, iw - indent);
            if draw {
                r.text(&t, Font::Text, pad + indent, y, iw - indent, c);
            }
            y += h + gap;
            // Open objective without a marker here: show the website's hint, if any.
            if !o.done && o.markers_here == 0 {
                if let Some(hint) = &o.hint {
                    let (_, h) = r.measure(hint, Font::Small, iw - 2.0 * indent);
                    if draw {
                        r.text(hint, Font::Small, pad + 2.0 * indent, y, iw - 2.0 * indent, DIM);
                    }
                    y += h + gap;
                }
            }
        }
        y += 4.0 * sc;
    }

    // 5. Layers (checkboxes), if the pack has any: separator, heading, then one row per layer
    // with its marker shape and the number of markers on this map.
    if !v.layers.is_empty() {
        y += 2.0 * sc;
        if draw {
            r.line(pad, y, w - pad, y, 1.0, (0.85, 0.75, 0.55, 0.25));
        }
        y += 8.0 * sc;
        let (_, lh) = r.measure(s.panel_layers, Font::Small, iw);
        if draw {
            r.text(s.panel_layers, Font::Small, pad, y, iw, GOLD);
        }
        y += lh + 4.0 * sc;
        for ly in v.layers {
            let t = format!("{} {}  ({})", if ly.on { "☑" } else { "☐" }, ly.title, ly.here);
            let (_, h) = r.measure(&t, Font::Text, iw - 20.0 * sc);
            if draw {
                if hov(&hits) {
                    r.fill_rrect(pad - 6.0 * sc, y - 2.0 * sc, w - pad + 6.0 * sc, y + h + 2.0 * sc, 4.0 * sc, (1.0, 1.0, 1.0, 0.08));
                }
                let (cx, cy) = (pad + 6.0 * sc, y + h / 2.0);
                let c = (ly.color.0, ly.color.1, ly.color.2, if ly.on { 1.0 } else { 0.45 });
                if ly.diamond {
                    r.diamond(cx, cy, 6.0 * sc, 2.0 * sc, c);
                } else {
                    r.fill_circle(cx, cy, 5.0 * sc, c);
                }
                r.text(&t, Font::Text, pad + 20.0 * sc, y, iw - 20.0 * sc, if ly.on { TEXT } else { DIM });
            }
            hits.push(Hit { l: pad - 6.0 * sc, t: y - 2.0 * sc, r: w - pad + 6.0 * sc, b: y + h + 2.0 * sc, action: Action::ToggleLayer(ly.id.clone()) });
            y += h + gap;
        }
    }
    ((y + pad - 4.0 * sc).min(max_h), hits)
}
