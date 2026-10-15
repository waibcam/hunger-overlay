//! Per-frame marker layout (docs/DESIGN.md §5.3–5.4): turns the visible points of the open map into the list
//! of markers to draw, with their kind, colour, badge and screen position, after decluttering:
//! - a layer marker hidden under a tracked marker is skipped (the tracked one wins);
//! - dense layers turn into small dots ("dust") at overview zoom;
//! - nearby points of the same owner are grouped into one marker with a count (clusters);
//! - tracked markers of different owners that would overlap are spread around their common centre ("petals").
//! Pure computation on screen coordinates: nothing here touches the game or the network.

use crate::markers::{self, Kind, Rgb};
use crate::model::{DrawPoint, LegendLayer, LegendQuest};

/// Who a marker belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Owner {
    /// A legend row (tracked quest, crafting recipe or searched item), by index.
    Legend(usize),
    /// A map layer, by index.
    Layer(usize),
}

/// What the panel is spotlighting (hovered row or pill): its markers stay bright, the others are dimmed.
#[derive(Debug, Clone, PartialEq)]
pub enum Spot {
    /// A legend row, by index.
    Owner(usize),
    /// A layer, by index.
    Layer(usize),
    /// One untracked quest of the Quests layer, by quest id.
    Quest(String),
}

/// One marker to draw.
#[derive(Debug, Clone)]
pub struct Item {
    /// Screen position (local to the game window) where the marker is drawn.
    pub x: f32,
    pub y: f32,
    /// True position when the marker was moved aside as a petal (a leader line joins them).
    pub anchor: Option<(f32, f32)>,
    /// Marker type.
    pub kind: Kind,
    /// Fill / ring colour.
    pub color: Rgb,
    /// Owner badge ("1"…"9", "A"…), empty for layers.
    pub badge: String,
    /// Owner (legend row or layer).
    pub owner: Owner,
    /// Indexes of the points it stands for (more than one = a cluster); the first one is the representative.
    pub members: Vec<usize>,
}

impl Item {
    /// Drawn radius at size factor `k` (clusters are 15 % larger).
    pub fn radius(&self, k: f32) -> f32 {
        markers::radius(self.kind) * k * if self.members.len() > 1 { 1.15 } else { 1.0 }
    }

    /// True if the spotlight is on this marker.
    pub fn lit(&self, spot: &Spot, points: &[DrawPoint]) -> bool {
        match (spot, self.owner) {
            (Spot::Owner(a), Owner::Legend(b)) => *a == b,
            (Spot::Layer(a), Owner::Layer(b)) => *a == b,
            (Spot::Quest(q), Owner::Layer(_)) => self.members.iter().any(|&i| points[i].quest_ref.as_ref().map(|r| &r.0 == q).unwrap_or(false)),
            _ => false,
        }
    }
}

/// Inputs of `build`.
pub struct Input<'a> {
    /// Visible points: (index in `points`, screen x, screen y).
    pub visible: &'a [(usize, f32, f32)],
    /// All points of the map.
    pub points: &'a [DrawPoint],
    /// Legend rows (owner colours and badges).
    pub legend: &'a [LegendQuest],
    /// Layers (ids).
    pub layers: &'a [LegendLayer],
    /// Zoom metric Z (1 = the game's default, fully zoomed-out view).
    pub zoom: f32,
    /// UI scale (screen height / 1440).
    pub sc: f32,
    /// Spotlighted owner, if any: its markers are never clustered.
    pub spot: Option<&'a Spot>,
}

/// Kind, owner, colour and badge of one point.
fn classify(p: &DrawPoint, legend: &[LegendQuest], layers: &[LegendLayer], dust: &[bool]) -> Option<(Kind, Owner, Rgb, String)> {
    if let Some(li) = p.layer {
        let ly = layers.get(li)?;
        let mut kind = markers::kind_for_layer(&ly.id, ly.diamond);
        if dust.get(li).copied().unwrap_or(false) {
            kind = Kind::Dust;
        }
        let color = match p.harvest.as_ref().and_then(|h| h.profession.as_deref()).and_then(markers::profession_color) {
            Some(c) => c,
            None => markers::layer_color(&ly.id, p.color),
        };
        return Some((kind, Owner::Layer(li), color, String::new()));
    }
    let lq = legend.get(p.quest)?;
    let kind = if p.done {
        Kind::Done
    } else if p.harvest.is_some() {
        if p.locked {
            Kind::TrackedNodeLocked
        } else {
            Kind::TrackedNode
        }
    } else if p.placed {
        Kind::TrackedPlaced
    } else if p.optional {
        Kind::TrackedOptional
    } else {
        Kind::Tracked
    };
    Some((kind, Owner::Legend(p.quest), markers::guard(p.color), lq.badge.clone()))
}

/// Builds the markers to draw, sorted bottom to top.
pub fn build(i: &Input) -> Vec<Item> {
    let sc = i.sc;
    // Dense layers at overview zoom: sound traps below Z = 2, and any layer with more than 120 visible points.
    let mut per_layer = vec![0usize; i.layers.len()];
    for &(pi, _, _) in i.visible {
        if let Some(li) = i.points[pi].layer {
            if li < per_layer.len() {
                per_layer[li] += 1;
            }
        }
    }
    let dust: Vec<bool> = i
        .layers
        .iter()
        .enumerate()
        .map(|(li, l)| i.zoom < 2.0 && (l.id == "sound_traps" || per_layer[li] > 120))
        .collect();

    // 1. Classify every visible point.
    let mut raw: Vec<(usize, f32, f32, Kind, Owner, Rgb, String)> = Vec::with_capacity(i.visible.len());
    for &(pi, x, y) in i.visible {
        if let Some((k, o, c, b)) = classify(&i.points[pi], i.legend, i.layers, &dust) {
            raw.push((pi, x, y, k, o, c, b));
        }
    }

    // 2. A layer marker within 8 px of a tracked marker is skipped (the tracked marker wins).
    let tracked: Vec<(f32, f32)> = raw.iter().filter(|r| markers::is_owner(r.3) && r.3 != Kind::Done).map(|r| (r.1, r.2)).collect();
    let near = 8.0 * sc;
    raw.retain(|r| {
        if !matches!(r.4, Owner::Layer(_)) || r.3 == Kind::Place {
            return true;
        }
        !tracked.iter().any(|&(x, y)| (x - r.1).powi(2) + (y - r.2).powi(2) < near * near)
    });

    // 3. Clusters: per owner and kind, greedy on the running centroid. The radius depends on the zoom.
    let cr = sc * if i.zoom < 1.5 {
        24.0
    } else if i.zoom < 3.0 {
        16.0
    } else {
        8.0
    };
    let mut items: Vec<Item> = vec![];
    // (index in `items`, running sum x, sum y) of the open clusters, per (owner, kind).
    let mut open: std::collections::HashMap<(Owner, u8), Vec<(usize, f32, f32)>> = std::collections::HashMap::new();
    for (pi, x, y, kind, owner, color, badge) in raw {
        let spot_lit = match (i.spot, owner) {
            (Some(Spot::Owner(a)), Owner::Legend(b)) => *a == b,
            (Some(Spot::Layer(a)), Owner::Layer(b)) => *a == b,
            (Some(Spot::Quest(q)), Owner::Layer(_)) => i.points[pi].quest_ref.as_ref().map(|r| &r.0 == q).unwrap_or(false),
            _ => false,
        };
        let clusterable = !spot_lit && !matches!(kind, Kind::Dust | Kind::Place);
        if clusterable {
            let list = open.entry((owner, markers::z(kind) * 32 + kind as u8)).or_default();
            if let Some(c) = list.iter_mut().find(|c| {
                let n = items[c.0].members.len() as f32;
                let (cx, cy) = (c.1 / n, c.2 / n);
                (cx - x).powi(2) + (cy - y).powi(2) < cr * cr
            }) {
                c.1 += x;
                c.2 += y;
                let it = &mut items[c.0];
                it.members.push(pi);
                let n = it.members.len() as f32;
                it.x = c.1 / n;
                it.y = c.2 / n;
                continue;
            }
            list.push((items.len(), x, y));
        }
        items.push(Item { x, y, anchor: None, kind, color, badge, owner, members: vec![pi] });
    }

    // 4. Petals: tracked markers of DIFFERENT owners closer than 22 px are spread on a small circle around
    //    their common centre, so no owner hides another one.
    let close = 22.0 * sc;
    let idx: Vec<usize> = (0..items.len()).filter(|&k| markers::is_owner(items[k].kind) && items[k].kind != Kind::Done).collect();
    let mut group: Vec<usize> = (0..items.len()).collect();
    fn root(g: &mut [usize], mut a: usize) -> usize {
        while g[a] != a {
            g[a] = g[g[a]];
            a = g[a];
        }
        a
    }
    for (n, &a) in idx.iter().enumerate() {
        for &b in &idx[n + 1..] {
            if (items[a].x - items[b].x).powi(2) + (items[a].y - items[b].y).powi(2) < close * close {
                let (ra, rb) = (root(&mut group, a), root(&mut group, b));
                group[ra] = rb;
            }
        }
    }
    let mut groups: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for &a in &idx {
        let r = root(&mut group, a);
        groups.entry(r).or_default().push(a);
    }
    for (_, g) in groups {
        let owners: std::collections::HashSet<Owner> = g.iter().map(|&a| items[a].owner).collect();
        if g.len() < 2 || owners.len() < 2 {
            continue;
        }
        let n = g.len() as f32;
        let cx = g.iter().map(|&a| items[a].x).sum::<f32>() / n;
        let cy = g.iter().map(|&a| items[a].y).sum::<f32>() / n;
        let rr = if g.len() == 2 { 14.0 } else { 18.0 } * sc;
        for (j, &a) in g.iter().enumerate() {
            // 2 owners: left / right; 3 or more: evenly spaced, starting at the top.
            let ang = if g.len() == 2 {
                std::f32::consts::PI * j as f32
            } else {
                -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * j as f32 / n
            };
            items[a].anchor = Some((cx, cy));
            items[a].x = cx + rr * ang.cos();
            items[a].y = cy + rr * ang.sin();
        }
    }

    // 5. Bottom to top.
    items.sort_by_key(|it| markers::z(it.kind));
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DrawPoint, LegendLayer};

    fn pt(layer: Option<usize>) -> DrawPoint {
        DrawPoint {
            u: 0.0,
            v: 0.0,
            color: (1.0, 0.0, 0.0),
            quest: 0,
            objective: Some(0),
            done: false,
            optional: false,
            place: false,
            label: String::new(),
            chance: None,
            container: None,
            harvest: None,
            locked: false,
            layer,
            diamond: false,
            placed: false,
            quest_ref: None,
            extra: vec![],
        }
    }

    fn lq(badge: &str) -> LegendQuest {
        LegendQuest {
            quest_id: badge.into(),
            title: badge.into(),
            color: (1.0, 0.0, 0.0),
            objectives: vec![],
            on_this_map: 0,
            filtered: 0,
            hidden: false,
            badge: badge.into(),
            kind: crate::model::OwnerKind::Quest,
            best_filtered: 0.0,
        }
    }

    /// Two points of one owner 10 px apart form one cluster; two owners at the same spot become petals;
    /// a layer point under a tracked marker is skipped.
    #[test]
    fn declutter() {
        let mut p1 = pt(None);
        p1.quest = 1;
        let points = vec![pt(None), pt(None), p1, pt(Some(0))];
        let legend = vec![lq("1"), lq("2")];
        let layers = vec![LegendLayer { id: "traps".into(), title: "Traps".into(), color: (1.0, 0.5, 0.0), diamond: false, on: true, here: 1 }];
        let visible = vec![(0, 100.0, 100.0), (1, 110.0, 100.0), (2, 106.0, 101.0), (3, 104.0, 100.0)];
        let items = build(&Input { visible: &visible, points: &points, legend: &legend, layers: &layers, zoom: 1.0, sc: 1.0, spot: None });
        assert_eq!(items.len(), 2, "{items:?}");
        assert!(items.iter().all(|it| it.anchor.is_some()));
        assert!(items.iter().any(|it| it.members.len() == 2));
    }
}
