//! Marker catalogue (docs/DESIGN.md §5): what each kind of map point looks like, drawn with the
//! primitives of `render.rs`. Shape tells "what it is" (circle = container / objective spot, square =
//! harvest node, diamond = place or placed object, dark token with a glyph = information layer) and
//! colour tells "whose it is" (a quest, recipe or search colour, or the layer's colour).
//! Every marker sits on a dark halo so it stays readable on the beige, ink-hatched map.

use crate::render::{Font, Renderer, Rgba};

/// RGB colour in 0..1.
pub type Rgb = (f32, f32, f32);

/// Dark underlay drawn under every marker.
pub const HALO: Rgb = (0.043, 0.035, 0.027);
/// Disc of the information tokens.
const TOKEN_FILL: Rgb = (0.086, 0.071, 0.051);
/// Ivory (spawn crosshair, done ring, leader lines).
pub const IVORY: Rgb = (0.953, 0.925, 0.867);
/// Hover / focus ring.
pub const GOLD_HI: Rgb = (0.922, 0.796, 0.545);
/// Gold of the place diamonds (same as the panel's titles).
pub const GOLD: Rgb = (0.831, 0.686, 0.416);
/// "!" token of the Quests layer.
pub const QUEST_AVAIL: Rgb = (0.941, 0.835, 0.549);

/// One marker type of the catalogue (§5.2). Owner markers (tracked quests, recipes, searches) carry their
/// owner's colour and badge; layer markers have fixed colours.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// Tracked objective / ingredient / searched item at a container or spot: owner-coloured disc, white ring, badge.
    Tracked,
    /// Same, for an optional objective: extra dashed ring.
    TrackedOptional,
    /// Tracked / searched harvest node: rounded square.
    TrackedNode,
    /// Same, above the player's declared level: greyed out.
    TrackedNodeLocked,
    /// Tracked / searched placed object: filled diamond.
    TrackedPlaced,
    /// Objective already done (only with "Show done").
    Done,
    /// Quests layer: an objective of a quest the player does not track yet ("!" token).
    QuestAvail,
    /// Named place (POI).
    Place,
    /// Player spawn: dark token with an ivory crosshair whose ticks stick out (a target).
    Spawn,
    /// Extraction: blue token with an up arrow.
    Extraction,
    /// Key spot: token with a key glyph.
    Key,
    /// Locked door / container: token with a padlock.
    Lock,
    /// Miniboss: token with a skull.
    Boss,
    /// Trap: token with a cross.
    Trap,
    /// Sound trap, zoomed in: dot + dashed ring.
    Sound,
    /// Any dense layer at overview zoom: a small dot.
    Dust,
    /// Placed loot (layer): small gold diamond.
    Loot,
    /// Harvest node (layer): small square in the profession's colour.
    Node,
    /// Raid ally's objective at a container or spot: hollow disc in the ally colour, white hairline ring, initial.
    Ally,
    /// Raid ally's harvest node: the same as a rounded square.
    AllyNode,
    /// Raid ally's placed object: the same as a diamond.
    AllyPlaced,
    /// Layer the app does not know (added later by the website): small disc or diamond in the layer colour.
    Generic,
    /// Same, drawn as a diamond.
    GenericDiamond,
}

/// Visual kind of the points of a layer, by layer id.
pub fn kind_for_layer(id: &str, diamond: bool) -> Kind {
    match id {
        "quests" => Kind::QuestAvail,
        "places" => Kind::Place,
        "player_spawns" => Kind::Spawn,
        "extractions" => Kind::Extraction,
        "keys" => Kind::Key,
        "locks" => Kind::Lock,
        "minibosses" => Kind::Boss,
        "traps" => Kind::Trap,
        "sound_traps" => Kind::Sound,
        "placed_loot" => Kind::Loot,
        "harvest" => Kind::Node,
        _ if diamond => Kind::GenericDiamond,
        _ => Kind::Generic,
    }
}

/// Fixed colour of a layer (the design palette); `fallback` = the website's colour for unknown layers.
pub fn layer_color(id: &str, fallback: Rgb) -> Rgb {
    match id {
        "quests" => QUEST_AVAIL,
        "places" => GOLD,
        "player_spawns" => IVORY,
        "extractions" => (0.243, 0.620, 0.859),
        "keys" => (0.890, 0.702, 0.255),
        "locks" => (0.663, 0.737, 0.776),
        "minibosses" => (0.878, 0.271, 0.227),
        "traps" => (0.941, 0.478, 0.180),
        "sound_traps" => (0.643, 0.420, 0.878),
        "placed_loot" => (0.949, 0.761, 0.188),
        "harvest" => (0.384, 0.761, 0.416),
        _ => fallback,
    }
}

/// Colour of a gathering profession (Conservator is rose so it does not clash with the purple sound traps).
pub fn profession_color(p: &str) -> Option<Rgb> {
    match p.to_ascii_lowercase().as_str() {
        "naturalist" => Some((0.384, 0.761, 0.416)),
        "scavenger" => Some((0.820, 0.635, 0.227)),
        "conservator" => Some((0.851, 0.451, 0.624)),
        _ => None,
    }
}

/// Base radius (px at 1440p, marker scale 1) of a kind: used for hit testing, focus ring and label placement.
pub fn radius(k: Kind) -> f32 {
    match k {
        Kind::Tracked | Kind::TrackedOptional => 10.0,
        Kind::TrackedNode | Kind::TrackedNodeLocked => 9.0,
        Kind::TrackedPlaced => 12.0,
        Kind::Done => 6.5,
        Kind::QuestAvail | Kind::Place | Kind::Key | Kind::Lock => 8.5,
        Kind::Spawn => 11.0,
        Kind::Extraction => 11.0,
        Kind::Boss => 9.0,
        Kind::Trap => 8.0,
        Kind::Sound => 7.5,
        Kind::Dust => 3.2,
        Kind::Loot => 7.0,
        Kind::Node => 5.5,
        Kind::Generic | Kind::GenericDiamond => 6.0,
        Kind::Ally => 9.0,
        Kind::AllyNode => 8.0,
        Kind::AllyPlaced => 10.5,
    }
}

/// Drawing order, bottom to top (§5.2 "Z" column).
pub fn z(k: Kind) -> u8 {
    match k {
        Kind::Dust => 1,
        Kind::Sound => 2,
        Kind::Node => 3,
        Kind::Loot => 4,
        Kind::Generic | Kind::GenericDiamond => 5,
        Kind::Key | Kind::Lock | Kind::Boss | Kind::Trap => 6,
        Kind::Place => 7,
        Kind::QuestAvail => 8,
        Kind::Done | Kind::Spawn | Kind::Extraction => 9,
        // Allies under every marker of mine.
        Kind::Ally | Kind::AllyNode | Kind::AllyPlaced => 10,
        Kind::TrackedNodeLocked => 11,
        Kind::TrackedNode | Kind::TrackedPlaced => 12,
        Kind::Tracked | Kind::TrackedOptional => 14,
    }
}

/// True for the markers of a tracked owner (quest, recipe, search, or a raid ally's quest).
pub fn is_owner(k: Kind) -> bool {
    matches!(k, Kind::Tracked | Kind::TrackedOptional | Kind::TrackedNode | Kind::TrackedNodeLocked | Kind::TrackedPlaced | Kind::Done) || is_ally(k)
}

/// True for a raid ally's marker.
pub fn is_ally(k: Kind) -> bool {
    matches!(k, Kind::Ally | Kind::AllyNode | Kind::AllyPlaced)
}

/// Owner colour guard: a website colour too dark to show on the halo is lightened by 25 %.
pub fn guard(c: Rgb) -> Rgb {
    let lum = 255.0 * (0.299 * c.0 + 0.587 * c.1 + 0.114 * c.2);
    if lum < 70.0 {
        (c.0 + (1.0 - c.0) * 0.25, c.1 + (1.0 - c.1) * 0.25, c.2 + (1.0 - c.2) * 0.25)
    } else {
        c
    }
}

/// Badge text colour: near-black on light fills, white otherwise.
fn badge_ink(c: Rgb) -> Rgba {
    let lum = 255.0 * (0.299 * c.0 + 0.587 * c.1 + 0.114 * c.2);
    if lum > 120.0 {
        (0.090, 0.071, 0.047, 1.0)
    } else {
        (1.0, 1.0, 1.0, 1.0)
    }
}

/// `c` with alpha `a`.
fn rgba(c: Rgb, a: f32) -> Rgba {
    (c.0, c.1, c.2, a)
}

/// Draws one marker centred on (x, y).
/// `k` = size factor (UI scale × marker size setting × 1.25 when hovered), `sc` = UI scale (strokes, fonts),
/// `alpha` multiplies every colour (spotlight dimming, "off" layer swatches), `badge` = owner badge ("" = none).
#[allow(clippy::too_many_arguments)]
pub fn draw(r: &Renderer, kind: Kind, color: Rgb, badge: &str, x: f32, y: f32, k: f32, sc: f32, alpha: f32) {
    let a = alpha;
    let halo = |extra: f32| rgba(HALO, extra * a);
    let rad = radius(kind) * k;
    match kind {
        Kind::Tracked | Kind::TrackedOptional => {
            r.fill_circle(x, y, rad + 2.5 * k, halo(0.6));
            r.fill_circle(x, y, rad, rgba(color, a));
            r.stroke_circle(x, y, rad - 1.0 * k, 2.0 * k, (1.0, 1.0, 1.0, 0.95 * a), false);
            owner_center(r, color, badge, x, y, rad, a);
            if kind == Kind::TrackedOptional {
                r.stroke_circle(x, y, rad + 5.0 * k, 1.5 * k, rgba(color, a), true);
            }
        }
        Kind::TrackedNode | Kind::TrackedNodeLocked => {
            let locked = kind == Kind::TrackedNodeLocked;
            let h = 2.5 * k;
            r.fill_rrect(x - rad - h, y - rad - h, x + rad + h, y + rad + h, 4.0 * k, halo(0.6));
            let fill = if locked { rgba((0.467, 0.443, 0.416), 0.9 * a) } else { rgba(color, a) };
            r.fill_rrect(x - rad, y - rad, x + rad, y + rad, 3.0 * k, fill);
            let i = 1.0 * k;
            r.stroke_rrect(x - rad + i, y - rad + i, x + rad - i, y + rad - i, 2.5 * k, 2.0 * k, (1.0, 1.0, 1.0, if locked { 0.5 } else { 0.95 } * a));
            owner_center(r, if locked { (0.467, 0.443, 0.416) } else { color }, badge, x, y, rad, a);
        }
        Kind::TrackedPlaced => {
            r.fill_diamond(x, y, rad + 3.5 * k, 2.0 * k, halo(0.6));
            r.fill_diamond(x, y, rad, 1.5 * k, rgba(color, a));
            r.stroke_diamond(x, y, rad - 1.4 * k, 2.0 * k, (1.0, 1.0, 1.0, 0.95 * a));
            owner_center(r, color, badge, x, y, rad * 0.8, a);
        }
        Kind::Done => {
            r.fill_circle(x, y, rad + 1.5 * k, halo(0.5));
            r.fill_circle(x, y, rad, (0.435, 0.416, 0.384, 0.85 * a));
            r.stroke_circle(x, y, rad, 1.2 * k, rgba(IVORY, 0.6 * a), false);
            let c = rgba(IVORY, 0.9 * a);
            r.line_round(x - 2.8 * k, y + 0.2 * k, x - 0.8 * k, y + 2.3 * k, 1.6 * k, c);
            r.line_round(x - 0.8 * k, y + 2.3 * k, x + 3.0 * k, y - 2.2 * k, 1.6 * k, c);
        }
        Kind::QuestAvail => {
            token(r, x, y, rad, k, color, 1.8, a);
            r.text_center("!", Font::Section, x, y - 0.5 * sc, rgba(color, a));
        }
        Kind::Place => {
            r.fill_diamond(x, y, rad + 2.4 * k, 1.0 * k, halo(0.65));
            r.fill_diamond(x, y, rad, 1.0 * k, (0.102, 0.082, 0.063, 0.92 * a));
            r.stroke_diamond(x, y, rad - 1.1 * k, 2.2 * k, rgba(color, a));
            r.fill_diamond(x, y, 2.2 * k, 0.3 * k, rgba(color, a));
        }
        Kind::Spawn => {
            // Four ticks sticking out of the ring, drawn over a dark outline, then a dark disc with
            // an ivory ring and centre: readable on the light parchment and on dark ink alike.
            let ticks = |w: f32, c: Rgba| {
                for (dx, dy) in [(0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)] {
                    let (a0, a1) = (rad - 1.0 * k, rad + 5.5 * k);
                    r.line_round(x + dx * a0, y + dy * a0, x + dx * a1, y + dy * a1, w, c);
                }
            };
            ticks(6.2 * k, halo(0.85));
            r.fill_circle(x, y, rad + 1.8 * k, halo(0.6));
            r.fill_circle(x, y, rad, rgba(TOKEN_FILL, 0.92 * a));
            r.stroke_circle(x, y, rad - 1.5 * k, 3.0 * k, rgba(color, a), false);
            ticks(2.8 * k, rgba(color, a));
            r.fill_circle(x, y, 3.4 * k, rgba(color, a));
        }
        Kind::Extraction => {
            token(r, x, y, rad, k, color, 2.6, a);
            let c = (0.918, 0.957, 0.984, a);
            let w = 2.3 * k;
            r.line_round(x - 4.0 * k, y + 0.2 * k, x, y - 4.2 * k, w, c);
            r.line_round(x, y - 4.2 * k, x + 4.0 * k, y + 0.2 * k, w, c);
            r.line_round(x, y - 3.6 * k, x, y + 5.0 * k, w, c);
        }
        Kind::Key => {
            token(r, x, y, rad, k, color, 1.8, a);
            let c = (0.949, 0.812, 0.408, a);
            r.stroke_circle(x - 2.6 * k, y, 2.1 * k, 1.5 * k, c, false);
            r.line_round(x - 0.5 * k, y, x + 4.6 * k, y, 1.5 * k, c);
            r.line_round(x + 2.4 * k, y, x + 2.4 * k, y + 2.2 * k, 1.4 * k, c);
            r.line_round(x + 4.3 * k, y, x + 4.3 * k, y + 2.2 * k, 1.4 * k, c);
        }
        Kind::Lock => {
            token(r, x, y, rad, k, color, 1.8, a);
            let c = (0.835, 0.882, 0.906, a);
            r.stroke_circle(x, y - 1.6 * k, 2.5 * k, 1.4 * k, c, false);
            r.fill_rrect(x - 3.9 * k, y - 0.9 * k, x + 3.9 * k, y + 4.0 * k, 1.0 * k, c);
            r.fill_circle(x, y + 1.5 * k, 0.9 * k, rgba(TOKEN_FILL, a));
        }
        Kind::Boss => {
            token(r, x, y, rad, k, color, 2.2, a);
            r.text_center("☠", Font::Symbol, x, y, (0.973, 0.863, 0.831, a));
        }
        Kind::Trap => {
            token(r, x, y, rad, k, color, 1.8, a);
            let c = (1.0, 0.612, 0.361, a);
            let d = 3.3 * k;
            r.line_round(x - d, y - d, x + d, y + d, 2.1 * k, c);
            r.line_round(x - d, y + d, x + d, y - d, 2.1 * k, c);
        }
        Kind::Sound => {
            let d = 3.2 * k;
            r.fill_circle(x, y, d + 1.4 * k, halo(0.5));
            r.fill_circle(x, y, d, rgba(color, a));
            r.stroke_circle(x, y, rad, 2.6 * k, halo(0.35), false);
            r.stroke_circle(x, y, rad, 1.3 * k, rgba(color, 0.9 * a), true);
        }
        Kind::Dust => {
            r.fill_circle(x, y, rad, rgba(color, 0.8 * a));
        }
        Kind::Loot => {
            r.fill_diamond(x, y, rad + 1.6 * k, 0.8 * k, halo(0.75));
            r.fill_diamond(x, y, rad, 0.8 * k, rgba(color, a));
            r.fill_diamond(x, y, rad * 0.35, 0.2 * k, (1.0, 0.965, 0.847, 0.85 * a));
        }
        Kind::Node => {
            let h = 1.6 * k;
            r.fill_rrect(x - rad - h, y - rad - h, x + rad + h, y + rad + h, 2.4 * k, halo(0.75));
            r.fill_rrect(x - rad, y - rad, x + rad, y + rad, 1.8 * k, rgba(color, a));
        }
        Kind::Ally => {
            r.fill_circle(x, y, rad + 2.6 * k, halo(0.6));
            r.fill_circle(x, y, rad, rgba(TOKEN_FILL, 0.94 * a));
            r.stroke_circle(x, y, rad - 1.1 * k, 2.2 * k, rgba(color, a), false);
            r.stroke_circle(x, y, rad + 0.3 * k, 1.1 * k, (1.0, 1.0, 1.0, 0.85 * a), false);
            r.text_center(badge, Font::Badge, x, y - 0.5, rgba(color, a));
        }
        Kind::AllyNode => {
            let h = 2.6 * k;
            r.fill_rrect(x - rad - h, y - rad - h, x + rad + h, y + rad + h, 4.0 * k, halo(0.6));
            r.fill_rrect(x - rad, y - rad, x + rad, y + rad, 3.0 * k, rgba(TOKEN_FILL, 0.94 * a));
            let i = 1.1 * k;
            r.stroke_rrect(x - rad + i, y - rad + i, x + rad - i, y + rad - i, 2.5 * k, 2.2 * k, rgba(color, a));
            r.stroke_rrect(x - rad - 0.3 * k, y - rad - 0.3 * k, x + rad + 0.3 * k, y + rad + 0.3 * k, 3.2 * k, 1.1 * k, (1.0, 1.0, 1.0, 0.85 * a));
            r.text_center(badge, Font::Badge, x, y - 0.5, rgba(color, a));
        }
        Kind::AllyPlaced => {
            r.fill_diamond(x, y, rad + 3.2 * k, 2.0 * k, halo(0.6));
            r.fill_diamond(x, y, rad, 1.5 * k, rgba(TOKEN_FILL, 0.94 * a));
            r.stroke_diamond(x, y, rad - 1.5 * k, 2.2 * k, rgba(color, a));
            r.stroke_diamond(x, y, rad + 0.4 * k, 1.1 * k, (1.0, 1.0, 1.0, 0.85 * a));
            r.text_center(badge, Font::Badge, x, y - 0.5, rgba(color, a));
        }
        Kind::Generic => {
            r.fill_circle(x, y, rad + 1.6 * k, halo(0.7));
            r.fill_circle(x, y, rad, rgba(color, a));
        }
        Kind::GenericDiamond => {
            r.fill_diamond(x, y, rad + 1.6 * k, 0.8 * k, halo(0.7));
            r.fill_diamond(x, y, rad, 0.8 * k, rgba(color, a));
        }
    }
}

/// Dark information token: halo, dark disc, coloured ring of width `ring` (px at scale 1).
#[allow(clippy::too_many_arguments)]
fn token(r: &Renderer, x: f32, y: f32, rad: f32, k: f32, color: Rgb, ring: f32, a: f32) {
    r.fill_circle(x, y, rad + 1.5 * k, rgba(HALO, 0.45 * a));
    r.fill_circle(x, y, rad, rgba(TOKEN_FILL, 0.92 * a));
    r.stroke_circle(x, y, rad - ring * k / 2.0, ring * k, rgba(color, a), false);
}

/// Centre of an owner marker: its badge character, or a white dot when it has none.
fn owner_center(r: &Renderer, fill: Rgb, badge: &str, x: f32, y: f32, rad: f32, a: f32) {
    if badge.is_empty() {
        r.fill_circle(x, y, rad * 0.28, (1.0, 1.0, 1.0, 0.95 * a));
    } else {
        let ink = badge_ink(fill);
        r.text_center(badge, Font::Badge, x, y - 0.5, (ink.0, ink.1, ink.2, a));
    }
}

/// Ally pip on one of my markers (same spot, same quest): a small hollow disc with the ally's initial, at the
/// top right of my marker (second ally at the top left). `owner_r` = radius of my marker.
#[allow(clippy::too_many_arguments)]
pub fn pip(r: &Renderer, x: f32, y: f32, owner_r: f32, slot: usize, color: Rgb, initial: &str, k: f32, alpha: f32) {
    let ang = if slot == 0 { -std::f32::consts::FRAC_PI_4 } else { -3.0 * std::f32::consts::FRAC_PI_4 };
    let d = owner_r + 1.5 * k;
    let (px, py) = (x + d * ang.cos(), y + d * ang.sin());
    let pr = 6.2 * k;
    r.fill_circle(px, py, pr + 1.4 * k, rgba(HALO, 0.7 * alpha));
    r.fill_circle(px, py, pr, rgba(TOKEN_FILL, 0.95 * alpha));
    r.stroke_circle(px, py, pr - 0.8 * k, 1.6 * k, rgba(color, alpha), false);
    r.text_center(initial, Font::Caps, px, py - 0.5, rgba(color, alpha));
}

/// Hover focus ring (§5.2): dark halo under a light gold ring, around a marker of radius `rad`.
pub fn focus_ring(r: &Renderer, x: f32, y: f32, rad: f32, k: f32) {
    r.stroke_circle(x, y, rad + 4.5 * k, 4.4 * k, rgba(HALO, 0.55), false);
    r.stroke_circle(x, y, rad + 4.5 * k, 2.2 * k, rgba(GOLD_HI, 1.0), false);
}

/// Count pill of a cluster, at the top right of its marker.
pub fn count_pill(r: &Renderer, n: usize, color: Rgb, x: f32, y: f32, sc: f32, alpha: f32) {
    let t = n.to_string();
    let (tw, _) = r.measure(&t, Font::Caps, 200.0);
    let (h, l, t0) = (15.0 * sc, x + 6.0 * sc, y - 20.0 * sc);
    let w = tw + 9.0 * sc;
    r.fill_rrect(l, t0, l + w, t0 + h, h / 2.0, (0.067, 0.055, 0.039, 0.95 * alpha));
    r.stroke_rrect(l, t0, l + w, t0 + h, h / 2.0, 1.2 * sc, rgba(color, alpha));
    r.text_center(&t, Font::Caps, l + w / 2.0, t0 + h / 2.0 - 0.5, (0.945, 0.918, 0.859, alpha));
}

/// Expanding ring used to pulse a spotlighted marker; `t` = progress 0..1.
pub fn pulse(r: &Renderer, x: f32, y: f32, rad: f32, k: f32, color: Rgb, t: f32) {
    let t = t.clamp(0.0, 1.0);
    let rr = rad + (4.0 + 6.0 * t) * k;
    r.stroke_circle(x, y, rr, 2.0 * k, rgba(color, 0.9 * (1.0 - t)), false);
}
