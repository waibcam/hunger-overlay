//! Types of the v1 contract with the website (pack.json and /me/tracker). Lenient: any unknown field is ignored
//! and every optional field has a default value, so that a change on the website does not break the app.
//! Also builds the list of points to draw on the current map (`build_view`).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 2D affine transform: px = a·x + b·y + c, py = d·x + e·y + f.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Affine {
    /// x coefficient for px.
    pub a: f64,
    /// y coefficient for px.
    pub b: f64,
    /// Offset for px.
    pub c: f64,
    /// x coefficient for py.
    pub d: f64,
    /// y coefficient for py.
    pub e: f64,
    /// Offset for py.
    pub f: f64,
}

/// One map described by the pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackMap {
    /// Map id ("Map01", "Map02", "Map03").
    pub map_id: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Side of the map texture in pixels (default 4096).
    #[serde(default = "default_tex")]
    pub texture_px: f64,
    /// Transform from game world coordinates to texture pixels.
    pub affine_world_to_px: Affine,
}
/// Default texture side when the pack does not give one.
fn default_tex() -> f64 {
    4096.0
}

/// A point of interest on a map. Position is given either in world coordinates (x, y) or directly in
/// texture pixels (px, py).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    /// Map id the point belongs to.
    pub map: String,
    /// World x coordinate.
    #[serde(default)]
    pub x: Option<f64>,
    /// World y coordinate.
    #[serde(default)]
    pub y: Option<f64>,
    /// World z coordinate (height; not used for drawing).
    #[serde(default)]
    pub z: Option<f64>,
    /// Texture x in pixels (used when x/y are absent).
    #[serde(default)]
    pub px: Option<f64>,
    /// Texture y in pixels (used when x/y are absent).
    #[serde(default)]
    pub py: Option<f64>,
    /// Text shown for the point.
    #[serde(default)]
    pub label: Option<String>,
    /// Chance (0..1) that the container holds the item, if the website provides it.
    #[serde(default)]
    pub chance: Option<f64>,
    /// Container type (English name), if the website provides it.
    #[serde(default)]
    pub container: Option<String>,
    /// "objective" | "container" | "harvest".
    #[serde(default)]
    pub source: Option<String>,
    /// Harvest node name.
    #[serde(default)]
    pub node: Option<String>,
    /// Gathering profession needed for a harvest node.
    #[serde(default)]
    pub profession: Option<String>,
    /// Profession level needed for a harvest node.
    #[serde(default)]
    pub required_level: Option<u32>,
}

impl Marker {
    /// Chance of the point: the `chance` field, otherwise read from the label ("1/4.7" or "34%").
    /// Values above 1 are treated as percentages.
    pub fn chance(&self) -> Option<f64> {
        if let Some(c) = self.chance {
            return Some(if c > 1.0 { c / 100.0 } else { c });
        }
        let l = self.label.as_deref()?;
        // "1/N" form: chance = 1/N (a comma is accepted as decimal separator).
        if let Some(i) = l.find("1/") {
            let num: String = l[i + 2..].chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',').collect();
            if let Ok(v) = num.replace(',', ".").parse::<f64>() {
                if v > 0.0 {
                    return Some(1.0 / v);
                }
            }
        }
        // "N%" form: read the number just before the percent sign.
        if let Some(p) = l.find('%') {
            let head: String = l[..p].trim_end().chars().rev().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',').collect::<Vec<_>>().into_iter().rev().collect();
            if let Ok(v) = head.replace(',', ".").parse::<f64>() {
                return Some(v / 100.0);
            }
        }
        None
    }
}

/// One step of a quest, as described by the pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackObjective {
    /// Stable key, matched against the tracker's objective states.
    pub objective_key: String,
    /// Order within the quest.
    #[serde(default)]
    pub seq: i64,
    /// Objective type, if given.
    #[serde(default)]
    pub kind: Option<String>,
    /// Text of the objective.
    #[serde(default)]
    pub label: String,
    /// True for optional objectives.
    #[serde(default)]
    pub optional: bool,
    /// Extra help text.
    #[serde(default)]
    pub hint: Option<String>,
    /// Where the objective can be done.
    #[serde(default)]
    pub markers: Vec<Marker>,
}

/// One quest, as described by the pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackQuest {
    /// URL-friendly name.
    #[serde(default)]
    pub slug: String,
    /// Title in the pack's language.
    #[serde(default)]
    pub title: String,
    /// NPC giving the quest.
    #[serde(default)]
    pub giver: Option<String>,
    /// Quest steps.
    #[serde(default)]
    pub objectives: Vec<PackObjective>,
    /// Places related to the quest (not tied to a specific objective).
    #[serde(default)]
    pub places: Vec<Marker>,
}

/// The data pack downloaded from the website (`pack.json`): maps, quests and optional layers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pack {
    /// Contract version.
    #[serde(default)]
    pub schema: u32,
    /// Language of the texts.
    #[serde(default)]
    pub locale: String,
    /// Data version, used to detect updates.
    pub pack_version: String,
    /// Maps and their coordinate transforms.
    #[serde(default)]
    pub maps: Vec<PackMap>,
    /// Quests by quest id.
    #[serde(default)]
    pub quests: HashMap<String, PackQuest>,
    /// Toggleable layers of points.
    #[serde(default)]
    pub layers: Vec<Layer>,
}

/// A toggleable set of points (spawns, extractions, places, harvest, keys, traps…).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    /// Layer id (`harvest_*` ids are merged into one "Harvest" layer).
    pub id: String,
    /// Display title.
    #[serde(default)]
    pub title: String,
    /// "#rrggbb" colour.
    #[serde(default)]
    pub color: String,
    /// Marker shape ("diamond" or default circle).
    #[serde(default)]
    pub shape: String,
    /// Whether the layer is enabled by default.
    #[serde(default)]
    pub default_on: bool,
    /// Points of the layer.
    #[serde(default)]
    pub markers: Vec<Marker>,
}

/// Item search index (`items_index[.locale].json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchIndex {
    /// Data version.
    #[serde(default)]
    pub pack_version: String,
    /// Language of the item names.
    #[serde(default)]
    pub locale: String,
    /// URL template of an item file (contains `{item_key}`).
    #[serde(default)]
    pub item_url: String,
    /// Searchable items.
    #[serde(default)]
    pub items: Vec<IndexItem>,
}

/// One entry of the item search index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexItem {
    /// Item key (also the name of its item file).
    pub item_key: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Rarity name, if any.
    #[serde(default)]
    pub rarity: Option<String>,
    /// Number of known points for the item.
    #[serde(default)]
    pub n: u32,
    /// Kinds of sources where the item is found.
    #[serde(default)]
    pub sources: Vec<String>,
}

/// File of one item (`items/<item_key>.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemFile {
    /// Data version.
    #[serde(default)]
    pub pack_version: String,
    /// Item key.
    pub item_key: String,
    /// Where the item can be found.
    #[serde(default)]
    pub markers: Vec<Marker>,
}

/// `items.json`: where to find each recipe ingredient (containers + harvest).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemsPack {
    /// Data version.
    #[serde(default)]
    pub pack_version: String,
    /// Entries by item id.
    #[serde(default)]
    pub items: HashMap<String, ItemEntry>,
}

/// Where one ingredient can be found.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemEntry {
    /// Item key.
    #[serde(default)]
    pub key: String,
    /// Locations of the ingredient.
    #[serde(default)]
    pub markers: Vec<Marker>,
}

/// One recipe in the user's crafting list (from the tracker).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Craft {
    /// Recipe id.
    pub recipe_id: String,
    /// Recipe title.
    #[serde(default)]
    pub title: String,
    /// "#rrggbb" colour.
    #[serde(default)]
    pub color: String,
    /// Number of times to craft it (default 1).
    #[serde(default = "one")]
    pub qty: u32,
    /// Ingredients needed.
    #[serde(default)]
    pub ingredients: Vec<Ingredient>,
}
/// Default crafting quantity.
fn one() -> u32 {
    1
}

/// One ingredient of a recipe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ingredient {
    /// Item id (string or number), the key into `ItemsPack::items`.
    pub item_id: serde_json::Value,
    /// Item key.
    #[serde(default)]
    pub item_key: String,
    /// Display name.
    #[serde(default)]
    pub label: String,
    /// Quantity needed.
    #[serde(default)]
    pub need: u32,
}

/// Progress of one objective of a tracked quest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedObjective {
    /// Matches `PackObjective::objective_key`.
    pub objective_key: String,
    /// State ("done" when completed).
    #[serde(default)]
    pub state: String,
}

/// A quest the user tracks on the website.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedQuest {
    /// Quest id (key into `Pack::quests`).
    pub quest_id: String,
    /// URL-friendly name.
    #[serde(default)]
    pub slug: String,
    /// Title (English; fallback only).
    #[serde(default)]
    pub title: String,
    /// "#rrggbb" colour chosen on the website.
    #[serde(default)]
    pub color: String,
    /// True once the whole quest is completed (then it is not drawn).
    #[serde(default)]
    pub completed: bool,
    /// Objective states.
    #[serde(default)]
    pub objectives: Vec<TrackedObjective>,
}

/// The user's tracker (`/me/tracker`): tracked quests, crafting list and declared gathering levels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tracker {
    /// Contract version.
    #[serde(default)]
    pub schema: u32,
    /// Account name.
    #[serde(default)]
    pub user: Option<String>,
    /// Data version the tracker refers to.
    #[serde(default)]
    pub pack_version: String,
    /// Revision tag, used for conditional requests.
    #[serde(default)]
    pub rev: Option<String>,
    /// Account language on the website (if the website sends it).
    #[serde(default)]
    pub locale: Option<String>,
    /// Gathering levels declared on the website (`/gathering`), per profession.
    #[serde(default)]
    pub gathering_levels: HashMap<String, u32>,
    /// Crafting list (admin accounts, in trial).
    #[serde(default)]
    pub crafting: Vec<Craft>,
    /// Tracked quests.
    #[serde(default)]
    pub tracked: Vec<TrackedQuest>,
}

/// Colour "#rrggbb" → (r,g,b) in 0..1. Fallback: light grey.
pub fn parse_color(s: &str) -> (f32, f32, f32) {
    let h = s.trim().trim_start_matches('#');
    if h.len() == 6 {
        if let Ok(v) = u32::from_str_radix(h, 16) {
            return (((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0);
        }
    }
    (0.85, 0.85, 0.85)
}

/// A point ready to draw on the current map, in normalised texture coordinates (0..1).
#[derive(Debug, Clone)]
pub struct DrawPoint {
    /// Horizontal position, 0 = left edge of the map texture, 1 = right edge.
    pub u: f64,
    /// Vertical position, 0 = top edge, 1 = bottom edge.
    pub v: f64,
    /// RGB colour in 0..1.
    pub color: (f32, f32, f32),
    /// Index of the legend row it belongs to (`usize::MAX` for layer points).
    pub quest: usize,
    /// Index of the objective within that row, if any.
    pub objective: Option<usize>,
    /// Objective already completed.
    pub done: bool,
    /// Optional objective.
    pub optional: bool,
    /// Quest place rather than an objective location.
    pub place: bool,
    /// Text shown for the point.
    pub label: String,
    /// Chance (0..1) that THIS container holds the item, if known.
    pub chance: Option<f64>,
    /// Container type, if known.
    pub container: Option<String>,
    /// Harvest node (and required level / player's level).
    pub harvest: Option<Harvest>,
    /// Harvest above the player's declared level: drawn greyed out.
    pub locked: bool,
    /// Layer index (layer point, not an objective).
    pub layer: Option<usize>,
    /// Draw as a diamond instead of a circle.
    pub diamond: bool,
    /// Placed loot (weapon stuck in the ground, placed object, key on the floor).
    pub placed: bool,
    /// Quest-layer point: the (untracked) quest it belongs to, so the player can start tracking it (Ctrl+click).
    pub quest_ref: Option<(String, String)>,
}

/// Harvest details of a point.
#[derive(Debug, Clone)]
pub struct Harvest {
    /// Node name.
    pub node: Option<String>,
    /// Gathering profession.
    pub profession: Option<String>,
    /// Required profession level.
    pub level: Option<u32>,
    /// Player's level in that profession, as declared on the website.
    pub player: Option<u32>,
}

/// One "layer" row of the panel.
#[derive(Debug, Clone)]
pub struct LegendLayer {
    /// Layer id.
    pub id: String,
    /// Display title.
    pub title: String,
    /// RGB colour in 0..1.
    pub color: (f32, f32, f32),
    /// Drawn as diamonds.
    pub diamond: bool,
    /// Currently enabled.
    pub on: bool,
    /// Points on the current map.
    pub here: usize,
}

/// One row of the legend (a tracked quest and its objectives).
#[derive(Debug, Clone)]
pub struct LegendQuest {
    /// Quest id (`craft:<recipe>` for crafting rows, `find:<item>` for searched items).
    pub quest_id: String,
    /// Display title.
    pub title: String,
    /// RGB colour in 0..1.
    pub color: (f32, f32, f32),
    /// Objective rows.
    pub objectives: Vec<LegendObjective>,
    /// Number of points drawn on this map.
    pub on_this_map: usize,
    /// Points on this map hidden by the "minimum chance" filter.
    pub filtered: usize,
    /// Hidden locally (click in the panel): shown greyed out, without points.
    pub hidden: bool,
}

/// One objective row of the legend.
#[derive(Debug, Clone)]
pub struct LegendObjective {
    /// Objective text.
    pub label: String,
    /// Extra help text.
    pub hint: Option<String>,
    /// Already completed.
    pub done: bool,
    /// Number of points drawn on this map for this objective.
    pub markers_here: usize,
}

/// Options for building the points and the legend of a given map from the pack and the tracker.
pub struct ViewOpts<'a> {
    /// Map currently shown.
    pub map_id: &'a str,
    /// Also draw completed objectives.
    pub show_done: bool,
    /// Legend rows hidden by the user (quest ids).
    pub hidden: &'a [String],
    /// Points with a known chance below this value are hidden.
    pub min_chance: f64,
    /// Ids of the enabled layers.
    pub layers_on: &'a [String],
    /// Ingredient locations for the crafting list, if loaded.
    pub items: Option<&'a ItemsPack>,
    /// Searched items: (item_key, name, markers if already loaded).
    pub searched: Vec<(String, String, Option<&'a [Marker]>)>,
    /// Title of the merged "Harvest" layer (translated).
    pub harvest_title: &'a str,
    /// Translated title of the "Quests" layer (objective locations of every quest the player does not track).
    pub quests_title: &'a str,
}

/// Colours of searched items (distinct from the website's quest palette).
pub const SEARCH_COLORS: [&str; 6] = ["#ffd54f", "#4dd0e1", "#f48fb1", "#aed581", "#ffab91", "#b39ddb"];

/// Output of `build_view`.
pub struct ViewOut {
    /// Points to draw.
    pub points: Vec<DrawPoint>,
    /// Legend rows (quests, crafting recipes, searched items).
    pub legend: Vec<LegendQuest>,
    /// Layer rows.
    pub layers: Vec<LegendLayer>,
}

/// Builds the points and the legend of a map: tracked quests, crafting list, searched items, enabled layers.
pub fn build_view(pack: &Pack, tracker: &Tracker, o: &ViewOpts) -> ViewOut {
    let mut out = ViewOut { points: vec![], legend: vec![], layers: vec![] };
    let map_id = o.map_id;
    let Some(pm) = pack.maps.iter().find(|m| m.map_id == map_id) else {
        return out;
    };
    // Marker -> normalised texture coordinates: world (x, y) through the map's affine, else raw (px, py).
    let to_uv = |mk: &Marker| -> Option<(f64, f64)> {
        let a = &pm.affine_world_to_px;
        let (px, py) = match (mk.x, mk.y) {
            (Some(x), Some(y)) => (a.a * x + a.b * y + a.c, a.d * x + a.e * y + a.f),
            _ => (mk.px?, mk.py?),
        };
        Some((px / pm.texture_px, py / pm.texture_px))
    };
    // On this map and not below the minimum chance (points without a known chance are always kept).
    let keep = |m: &Marker| m.map == map_id && m.chance().map(|c| c + 1e-9 >= o.min_chance).unwrap_or(true);
    // Point of an objective (or an ingredient), with the harvest info and the player's level.
    #[allow(clippy::too_many_arguments)]
    let point = |mk: &Marker, u: f64, v: f64, color: (f32, f32, f32), quest: usize, objective: Option<usize>, done: bool, optional: bool, fallback: &str| -> DrawPoint {
        let harvest = mk.source.as_deref() == Some("harvest");
        let player = mk.profession.as_ref().and_then(|p| tracker.gathering_levels.get(p)).copied();
        let locked = harvest && matches!((mk.required_level, player), (Some(r), Some(p)) if r > p);
        DrawPoint {
            u,
            v,
            color,
            quest,
            objective,
            done,
            optional,
            place: false,
            label: mk.label.clone().unwrap_or_else(|| fallback.to_string()),
            chance: mk.chance(),
            container: mk.container.clone(),
            harvest: harvest.then(|| Harvest { node: mk.node.clone(), profession: mk.profession.clone(), level: mk.required_level, player }),
            locked,
            layer: None,
            diamond: false,
            placed: mk.source.as_deref() == Some("placed"),
            quest_ref: None,
        }
    };

    // 1. Tracked quests (completed ones and quests missing from the pack are skipped).
    for tq in &tracker.tracked {
        if tq.completed {
            continue;
        }
        let Some(pq) = pack.quests.get(&tq.quest_id) else { continue };
        let color = parse_color(&tq.color);
        let qi = out.legend.len();
        let is_hidden = o.hidden.iter().any(|h| h == &tq.quest_id);
        let mut lq = LegendQuest {
            quest_id: tq.quest_id.clone(),
            // Title from the pack (in the chosen language); the tracker's one is only a fallback (it is in English).
            title: if pq.title.is_empty() { tq.title.clone() } else { pq.title.clone() },
            color,
            objectives: vec![],
            on_this_map: 0,
            filtered: 0,
            hidden: is_hidden,
        };
        if is_hidden {
            out.legend.push(lq);
            continue;
        }
        // Objectives: count what the chance filter hides, add the kept points.
        for po in &pq.objectives {
            let done = tq.objectives.iter().any(|ob| ob.objective_key == po.objective_key && ob.state == "done");
            let oi = lq.objectives.len();
            let mut here = 0;
            if !done || o.show_done {
                let total_here = po.markers.iter().filter(|m| m.map == map_id).count();
                let kept: Vec<&Marker> = po.markers.iter().filter(|m| keep(m)).collect();
                lq.filtered += total_here - kept.len();
                for mk in kept {
                    if let Some((u, v)) = to_uv(mk) {
                        here += 1;
                        out.points.push(point(mk, u, v, color, qi, Some(oi), done, po.optional, &po.label));
                    }
                }
            }
            lq.on_this_map += here;
            lq.objectives.push(LegendObjective { label: po.label.clone(), hint: po.hint.clone(), done, markers_here: here });
        }
        // Quest places (never filtered by chance).
        for mk in pq.places.iter().filter(|m| m.map == map_id) {
            if let Some((u, v)) = to_uv(mk) {
                let mut p = point(mk, u, v, color, qi, None, false, false, "");
                p.place = true;
                out.points.push(p);
            }
        }
        out.legend.push(lq);
    }

    // 2. Crafting list: each recipe as a quest, its ingredients as objectives.
    for cr in &tracker.crafting {
        let id = format!("craft:{}", cr.recipe_id);
        let color = parse_color(&cr.color);
        let qi = out.legend.len();
        let is_hidden = o.hidden.iter().any(|h| *h == id);
        let mut lq = LegendQuest {
            quest_id: id,
            title: if cr.qty > 1 { format!("🛠 {} ×{}", cr.title, cr.qty) } else { format!("🛠 {}", cr.title) },
            color,
            objectives: vec![],
            on_this_map: 0,
            filtered: 0,
            hidden: is_hidden,
        };
        if is_hidden {
            out.legend.push(lq);
            continue;
        }
        for ing in &cr.ingredients {
            let oi = lq.objectives.len();
            let label = format!("{} ×{}", ing.label, ing.need);
            let mut here = 0;
            // The item id may be a JSON string or number; both are used as a string key.
            let key = match &ing.item_id {
                serde_json::Value::String(s) => s.clone(),
                v => v.to_string(),
            };
            if let Some(entry) = o.items.and_then(|it| it.items.get(&key)) {
                let total_here = entry.markers.iter().filter(|m| m.map == map_id).count();
                let kept: Vec<&Marker> = entry.markers.iter().filter(|m| keep(m)).collect();
                lq.filtered += total_here - kept.len();
                for mk in kept {
                    if let Some((u, v)) = to_uv(mk) {
                        here += 1;
                        out.points.push(point(mk, u, v, color, qi, Some(oi), false, false, &ing.label));
                    }
                }
            }
            lq.on_this_map += here;
            lq.objectives.push(LegendObjective { label, hint: None, done: false, markers_here: here });
        }
        out.legend.push(lq);
    }

    // 3. Searched items ("I'm looking for this item"), one legend row each, coloured from SEARCH_COLORS.
    for (i, (key, name, markers)) in o.searched.iter().enumerate() {
        let color = parse_color(SEARCH_COLORS[i % SEARCH_COLORS.len()]);
        let qi = out.legend.len();
        let mut lq = LegendQuest {
            quest_id: format!("find:{key}"),
            title: format!("🔍 {name}"),
            color,
            objectives: vec![],
            on_this_map: 0,
            filtered: 0,
            hidden: false,
        };
        let mut here = 0;
        if let Some(ms) = markers {
            let total_here = ms.iter().filter(|m| m.map == map_id).count();
            let kept: Vec<&Marker> = ms.iter().filter(|m| keep(m)).collect();
            lq.filtered += total_here - kept.len();
            for mk in kept {
                if let Some((u, v)) = to_uv(mk) {
                    here += 1;
                    out.points.push(point(mk, u, v, color, qi, Some(0), false, false, name));
                }
            }
        }
        lq.on_this_map = here;
        lq.objectives.push(LegendObjective { label: name.clone(), hint: None, done: false, markers_here: here });
        out.legend.push(lq);
    }

    // 4. Layers (spawns, extractions, places, harvest, keys, traps…), ticked in the panel.
    //    The per-profession harvest layers (harvest_*) are merged into a single "Harvest" layer, limited to the
    //    nodes the player can harvest according to the levels declared on the website (everything is shown if
    //    nothing is declared).
    let harvest_on = o.layers_on.iter().any(|l| l == "harvest");
    let mut harvest_slot: Option<usize> = None;
    for ly in pack.layers.iter() {
        let is_harvest = ly.id.starts_with("harvest_");
        let on = if is_harvest { harvest_on } else { o.layers_on.iter().any(|l| *l == ly.id) };
        let color = parse_color(&ly.color);
        if is_harvest && harvest_slot.is_none() {
            harvest_slot = Some(out.layers.len());
            out.layers.push(LegendLayer { id: "harvest".into(), title: o.harvest_title.into(), color: (0.42, 0.75, 0.35), diamond: false, on, here: 0 });
        }
        let li = if is_harvest { harvest_slot.unwrap() } else { out.layers.len() };
        let mut here = 0;
        for mk in ly.markers.iter().filter(|m| m.map == map_id) {
            let Some((u, v)) = to_uv(mk) else { continue };
            if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                continue; // outside the texture (map edge)
            }
            if is_harvest {
                let player = mk.profession.as_ref().and_then(|p| tracker.gathering_levels.get(p)).copied();
                if matches!((mk.required_level, player), (Some(r), Some(p)) if r > p) {
                    continue; // level too low
                }
            }
            here += 1;
            if on {
                let mut p = point(mk, u, v, color, usize::MAX, None, false, false, "");
                p.layer = Some(li);
                p.diamond = ly.shape == "diamond";
                out.points.push(p);
            }
        }
        if is_harvest {
            out.layers[li].here += here;
        } else {
            out.layers.push(LegendLayer { id: ly.id.clone(), title: ly.title.clone(), color, diamond: ly.shape == "diamond", on, here });
        }
    }
    // 5. "Quests" layer: the precise objective locations of every quest the player does NOT track yet, so quests
    //    can be discovered on the map (and tracked with Ctrl+click). Loot-source points are not included (too many).
    let quests_on = o.layers_on.iter().any(|l| l == "quests");
    let tracked: std::collections::HashSet<&str> = tracker.tracked.iter().map(|t| t.quest_id.as_str()).collect();
    let li = out.layers.iter().position(|l| l.id == "places").map(|p| p + 1).unwrap_or(out.layers.len());
    let qcolor = (0.96, 0.84, 0.45);
    let mut here = 0;
    let mut qpoints = vec![];
    for (qid, pq) in &pack.quests {
        if tracked.contains(qid.as_str()) {
            continue;
        }
        for po in &pq.objectives {
            for mk in po.markers.iter().filter(|m| m.map == map_id && m.source.as_deref() == Some("objective")) {
                let Some((u, v)) = to_uv(mk) else { continue };
                here += 1;
                if quests_on {
                    let mut p = point(mk, u, v, qcolor, usize::MAX, None, false, false, &po.label);
                    p.label = po.label.clone();
                    p.quest_ref = Some((qid.clone(), pq.title.clone()));
                    qpoints.push(p);
                }
            }
        }
    }
    // Layer indexes after the insertion point move by one.
    for p in out.points.iter_mut() {
        if let Some(l) = p.layer.as_mut() {
            if *l >= li {
                *l += 1;
            }
        }
    }
    for p in qpoints.iter_mut() {
        p.layer = Some(li);
    }
    out.points.extend(qpoints);
    out.layers.insert(li, LegendLayer { id: "quests".into(), title: o.quests_title.into(), color: qcolor, diamond: false, on: quests_on, here });
    out
}

#[cfg(test)]
mod tests {
    //! Check against the app's cached data (%APPDATA%\HungerOverlay): `cargo test --release view_cache -- --nocapture`.
    use super::*;

    /// Prints per-map point counts built from the cached pack/tracker. Skipped if no cache exists.
    #[test]
    fn view_cache() {
        let d = crate::config::data_dir();
        let (Ok(p), Ok(t)) = (std::fs::read_to_string(d.join("pack.json")), std::fs::read_to_string(d.join("tracker.json"))) else { return };
        let pack: Pack = serde_json::from_str(&p).unwrap();
        let tracker: Tracker = serde_json::from_str(&t).unwrap();
        let items: Option<ItemsPack> = std::fs::read_to_string(d.join("items.json")).ok().and_then(|s| serde_json::from_str(&s).ok());
        let all: Vec<String> = pack.layers.iter().map(|l| l.id.clone()).collect();
        for map in ["Map01", "Map02", "Map03"] {
            let o = ViewOpts { map_id: map, show_done: false, hidden: &[], min_chance: 0.10, layers_on: &all, items: items.as_ref(), searched: vec![], harvest_title: "Harvest", quests_title: "Quests" };
            let v = build_view(&pack, &tracker, &o);
            let harvest = v.points.iter().filter(|p| p.harvest.is_some()).count();
            let locked = v.points.iter().filter(|p| p.locked).count();
            let layer_pts = v.points.iter().filter(|p| p.layer.is_some()).count();
            println!("{map}: {} points ({} layer, {} harvest of which {} above level)", v.points.len(), layer_pts, harvest, locked);
            for q in &v.legend {
                println!("   {} — {} pt here, {} filtered", q.title, q.on_this_map, q.filtered);
            }
        }
    }
}
