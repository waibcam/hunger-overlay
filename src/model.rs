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
    /// Type de conteneur (nom anglais), si le site le fournit.
    #[serde(default)]
    pub container: Option<String>,
    /// "objective" | "container" | "harvest".
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub node: Option<String>,
    #[serde(default)]
    pub profession: Option<String>,
    #[serde(default)]
    pub required_level: Option<u32>,
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
    #[serde(default)]
    pub layers: Vec<Layer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub shape: String,
    #[serde(default)]
    pub default_on: bool,
    #[serde(default)]
    pub markers: Vec<Marker>,
}

/// `items.json` : où trouver chaque ingrédient de recette (conteneurs + récolte).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemsPack {
    #[serde(default)]
    pub pack_version: String,
    #[serde(default)]
    pub items: HashMap<String, ItemEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemEntry {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub markers: Vec<Marker>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Craft {
    pub recipe_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub color: String,
    #[serde(default = "one")]
    pub qty: u32,
    #[serde(default)]
    pub ingredients: Vec<Ingredient>,
}
fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ingredient {
    pub item_id: serde_json::Value,
    #[serde(default)]
    pub item_key: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub need: u32,
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
    /// Niveaux de récolte déclarés sur le site (`/gathering`), par métier.
    #[serde(default)]
    pub gathering_levels: HashMap<String, u32>,
    /// Liste de fabrication (comptes admins, en essai).
    #[serde(default)]
    pub crafting: Vec<Craft>,
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
    /// Chance (0..1) que CE conteneur contienne l'objet, si connue.
    pub chance: Option<f64>,
    pub container: Option<String>,
    /// Nœud de récolte (et niveau requis / niveau du joueur).
    pub harvest: Option<Harvest>,
    /// Récolte au-dessus du niveau déclaré du joueur : dessinée grisée.
    pub locked: bool,
    /// Index du calque (point de calque, pas d'objectif).
    pub layer: Option<usize>,
    pub diamond: bool,
}

#[derive(Debug, Clone)]
pub struct Harvest {
    pub node: Option<String>,
    pub profession: Option<String>,
    pub level: Option<u32>,
    pub player: Option<u32>,
}

/// Une ligne « calque » du panneau.
#[derive(Debug, Clone)]
pub struct LegendLayer {
    pub id: String,
    pub title: String,
    pub color: (f32, f32, f32),
    pub diamond: bool,
    pub on: bool,
    /// Points sur la carte courante.
    pub here: usize,
}

/// Une ligne de la légende (quête suivie et ses objectifs).
#[derive(Debug, Clone)]
pub struct LegendQuest {
    pub quest_id: String,
    pub title: String,
    pub color: (f32, f32, f32),
    pub objectives: Vec<LegendObjective>,
    pub on_this_map: usize,
    /// Points de cette carte masqués par le filtre « chance minimum ».
    pub filtered: usize,
    /// Masquée localement (clic dans le panneau) : affichée grisée, sans points.
    pub hidden: bool,
}

#[derive(Debug, Clone)]
pub struct LegendObjective {
    pub label: String,
    pub hint: Option<String>,
    pub done: bool,
    pub markers_here: usize,
}

/// Construit les points et la légende pour une carte donnée à partir du pack et du tracker.
pub struct ViewOpts<'a> {
    pub map_id: &'a str,
    pub show_done: bool,
    pub hidden: &'a [String],
    pub min_chance: f64,
    pub layers_on: &'a [String],
    pub items: Option<&'a ItemsPack>,
}

pub struct ViewOut {
    pub points: Vec<DrawPoint>,
    pub legend: Vec<LegendQuest>,
    pub layers: Vec<LegendLayer>,
}

/// Construit les points et la légende d'une carte : quêtes suivies, liste de fabrication, calques activés.
pub fn build_view(pack: &Pack, tracker: &Tracker, o: &ViewOpts) -> ViewOut {
    let mut out = ViewOut { points: vec![], legend: vec![], layers: vec![] };
    let map_id = o.map_id;
    let Some(pm) = pack.maps.iter().find(|m| m.map_id == map_id) else {
        return out;
    };
    let to_uv = |mk: &Marker| -> Option<(f64, f64)> {
        let a = &pm.affine_world_to_px;
        let (px, py) = match (mk.x, mk.y) {
            (Some(x), Some(y)) => (a.a * x + a.b * y + a.c, a.d * x + a.e * y + a.f),
            _ => (mk.px?, mk.py?),
        };
        Some((px / pm.texture_px, py / pm.texture_px))
    };
    let keep = |m: &Marker| m.map == map_id && m.chance().map(|c| c + 1e-9 >= o.min_chance).unwrap_or(true);
    // Point d'un objectif (ou d'un ingrédient), avec les infos de récolte et le niveau du joueur.
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
        }
    };

    // 1. Quêtes suivies.
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
            // Titre du pack (dans la langue choisie) ; celui du tracker n'est qu'un repli (il est en anglais).
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
        for mk in pq.places.iter().filter(|m| m.map == map_id) {
            if let Some((u, v)) = to_uv(mk) {
                let mut p = point(mk, u, v, color, qi, None, false, false, "");
                p.place = true;
                out.points.push(p);
            }
        }
        out.legend.push(lq);
    }

    // 2. Liste de fabrication : chaque recette comme une quête, ses ingrédients comme objectifs.
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

    // 3. Calques (extractions, lieux, récolte, clés, pièges…), cochés dans le panneau.
    for (li, ly) in pack.layers.iter().enumerate() {
        let on = o.layers_on.iter().any(|l| *l == ly.id);
        let color = parse_color(&ly.color);
        let mut here = 0;
        for mk in ly.markers.iter().filter(|m| m.map == map_id) {
            let Some((u, v)) = to_uv(mk) else { continue };
            if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                continue; // hors de la texture (bord de carte)
            }
            here += 1;
            if on {
                let mut p = point(mk, u, v, color, usize::MAX, None, false, false, "");
                p.layer = Some(li);
                p.diamond = ly.shape == "diamond";
                out.points.push(p);
            }
        }
        out.layers.push(LegendLayer { id: ly.id.clone(), title: ly.title.clone(), color, diamond: ly.shape == "diamond", on, here });
    }
    out
}

#[cfg(test)]
mod tests {
    //! Contrôle sur les données en cache de l'appli (%APPDATA%\HungerOverlay) : `cargo test --release view_cache -- --nocapture`.
    use super::*;

    #[test]
    fn view_cache() {
        let d = crate::config::data_dir();
        let (Ok(p), Ok(t)) = (std::fs::read_to_string(d.join("pack.json")), std::fs::read_to_string(d.join("tracker.json"))) else { return };
        let pack: Pack = serde_json::from_str(&p).unwrap();
        let tracker: Tracker = serde_json::from_str(&t).unwrap();
        let items: Option<ItemsPack> = std::fs::read_to_string(d.join("items.json")).ok().and_then(|s| serde_json::from_str(&s).ok());
        let all: Vec<String> = pack.layers.iter().map(|l| l.id.clone()).collect();
        for map in ["Map01", "Map02", "Map03"] {
            let o = ViewOpts { map_id: map, show_done: false, hidden: &[], min_chance: 0.10, layers_on: &all, items: items.as_ref() };
            let v = build_view(&pack, &tracker, &o);
            let harvest = v.points.iter().filter(|p| p.harvest.is_some()).count();
            let locked = v.points.iter().filter(|p| p.locked).count();
            let layer_pts = v.points.iter().filter(|p| p.layer.is_some()).count();
            println!("{map}: {} points ({} calques, {} récolte dont {} au-dessus du niveau)", v.points.len(), layer_pts, harvest, locked);
            for q in &v.legend {
                println!("   {} — {} pt ici, {} filtrés", q.title, q.on_this_map, q.filtered);
            }
        }
    }
}
