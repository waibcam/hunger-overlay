//! Types du contrat v1 avec le site (pack.json et /me/tracker). Tolérants : tout champ inconnu est ignoré,
//! tout champ facultatif a une valeur par défaut, pour qu'une évolution du site ne casse pas l'appli.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackMap {
    pub map_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_tex")]
    pub texture_px: f64,
    pub affine_world_to_px: Affine,
}
fn default_tex() -> f64 {
    4096.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub map: String,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub z: Option<f64>,
    #[serde(default)]
    pub px: Option<f64>,
    #[serde(default)]
    pub py: Option<f64>,
    #[serde(default)]
    pub label: Option<String>,
    /// Chance (0..1) que le conteneur contienne l'objet, si le site la fournit.
    #[serde(default)]
    pub chance: Option<f64>,
}

impl Marker {
    /// Chance du point : champ `chance`, sinon lue dans le libellé (« 1/4.7 » ou « 34% »).
    pub fn chance(&self) -> Option<f64> {
        if let Some(c) = self.chance {
            return Some(if c > 1.0 { c / 100.0 } else { c });
        }
        let l = self.label.as_deref()?;
        if let Some(i) = l.find("1/") {
            let num: String = l[i + 2..].chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',').collect();
            if let Ok(v) = num.replace(',', ".").parse::<f64>() {
                if v > 0.0 {
                    return Some(1.0 / v);
                }
            }
        }
        if let Some(p) = l.find('%') {
            let head: String = l[..p].trim_end().chars().rev().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',').collect::<Vec<_>>().into_iter().rev().collect();
            if let Ok(v) = head.replace(',', ".").parse::<f64>() {
                return Some(v / 100.0);
            }
        }
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackObjective {
    pub objective_key: String,
    #[serde(default)]
    pub seq: i64,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub hint: Option<String>,
    #[serde(default)]
    pub markers: Vec<Marker>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackQuest {
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub giver: Option<String>,
    #[serde(default)]
    pub objectives: Vec<PackObjective>,
    #[serde(default)]
    pub places: Vec<Marker>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pack {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub locale: String,
    pub pack_version: String,
    #[serde(default)]
    pub maps: Vec<PackMap>,
    #[serde(default)]
    pub quests: HashMap<String, PackQuest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedObjective {
    pub objective_key: String,
    #[serde(default)]
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedQuest {
    pub quest_id: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub objectives: Vec<TrackedObjective>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tracker {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub pack_version: String,
    #[serde(default)]
    pub rev: Option<String>,
    /// Langue du compte sur le site (si le site l'envoie).
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub tracked: Vec<TrackedQuest>,
}

/// Couleur "#rrggbb" → (r,g,b) en 0..1. Repli : gris clair.
pub fn parse_color(s: &str) -> (f32, f32, f32) {
    let h = s.trim().trim_start_matches('#');
    if h.len() == 6 {
        if let Ok(v) = u32::from_str_radix(h, 16) {
            return (((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0);
        }
    }
    (0.85, 0.85, 0.85)
}

/// Un point prêt à dessiner sur la carte courante, en coordonnées texture normalisées (0..1).
#[derive(Debug, Clone)]
pub struct DrawPoint {
    pub u: f64,
    pub v: f64,
    pub color: (f32, f32, f32),
    pub quest: usize,
    pub objective: Option<usize>,
    pub done: bool,
    pub optional: bool,
    pub place: bool,
    pub label: String,
}

/// Une ligne de la légende (quête suivie et ses objectifs).
#[derive(Debug, Clone)]
pub struct LegendQuest {
    pub quest_id: String,
    pub title: String,
    pub color: (f32, f32, f32),
    pub objectives: Vec<LegendObjective>,
    pub on_this_map: usize,
}

#[derive(Debug, Clone)]
pub struct LegendObjective {
    pub label: String,
    pub hint: Option<String>,
    pub done: bool,
    pub markers_here: usize,
}

/// Construit les points et la légende pour une carte donnée à partir du pack et du tracker.
pub fn build_view(pack: &Pack, tracker: &Tracker, map_id: &str, show_done: bool, hidden: &[String], min_chance: f64) -> (Vec<DrawPoint>, Vec<LegendQuest>) {
    let mut pts = Vec::new();
    let mut legend = Vec::new();
    let Some(pm) = pack.maps.iter().find(|m| m.map_id == map_id) else {
        return (pts, legend);
    };
    let to_uv = |mk: &Marker| -> Option<(f64, f64)> {
        let a = &pm.affine_world_to_px;
        let (px, py) = match (mk.x, mk.y) {
            (Some(x), Some(y)) => (a.a * x + a.b * y + a.c, a.d * x + a.e * y + a.f),
            _ => (mk.px?, mk.py?),
        };
        Some((px / pm.texture_px, py / pm.texture_px))
    };
    for tq in &tracker.tracked {
        if tq.completed || hidden.iter().any(|h| h == &tq.quest_id) {
            continue;
        }
        let Some(pq) = pack.quests.get(&tq.quest_id) else { continue };
        let color = parse_color(&tq.color);
        let qi = legend.len();
        let mut lq = LegendQuest {
            quest_id: tq.quest_id.clone(),
            title: if tq.title.is_empty() { pq.title.clone() } else { tq.title.clone() },
            color,
            objectives: vec![],
            on_this_map: 0,
        };
        for po in &pq.objectives {
            let done = tq.objectives.iter().any(|o| o.objective_key == po.objective_key && o.state == "done");
            let oi = lq.objectives.len();
            let mut here = 0;
            if !done || show_done {
                // Filtre « chance minimum » : seulement pour les points dont la chance est connue (loot).
                for mk in po.markers.iter().filter(|m| m.map == map_id && m.chance().map(|c| c + 1e-9 >= min_chance).unwrap_or(true)) {
                    if let Some((u, v)) = to_uv(mk) {
                        here += 1;
                        pts.push(DrawPoint {
                            u,
                            v,
                            color,
                            quest: qi,
                            objective: Some(oi),
                            done,
                            optional: po.optional,
                            place: false,
                            label: mk.label.clone().unwrap_or_else(|| po.label.clone()),
                        });
                    }
                }
            }
            lq.on_this_map += here;
            lq.objectives.push(LegendObjective { label: po.label.clone(), hint: po.hint.clone(), done, markers_here: here });
        }
        for mk in pq.places.iter().filter(|m| m.map == map_id) {
            if let Some((u, v)) = to_uv(mk) {
                pts.push(DrawPoint {
                    u,
                    v,
                    color,
                    quest: qi,
                    objective: None,
                    done: false,
                    optional: false,
                    place: true,
                    label: mk.label.clone().unwrap_or_default(),
                });
            }
        }
        legend.push(lq);
    }
    (pts, legend)
}
