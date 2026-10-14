//! Visual re-registration: finds the map's actual on-screen position (and, if needed, its zoom level) by
//! comparing the screen capture with the reference texture. Safety net for the input-based tracking (lost
//! mouse event, focus regained, return from menus, pan limit not re-applied when zooming out…).
//!
//! The texture is very fine-grained (hatching, trees): the correlation collapses after just a few pixels of
//! offset. So we compare sample points from the screen against the reference, interpolated (bilinear) at a
//! sub-pixel position, from coarse to fine: 16 px blocks (step 16), then 8 px (step 4), then 4 px (step 1).

use crate::capture::Gray;

/// Side of the full reference texture, in pixels.
const RN: usize = crate::refs::N;

/// Blocks darker than this threshold = outside the map (map-screen background, masked windows).
const DARK: f32 = 55.0;

/// One scale of the pyramid.
struct Level {
    /// Block size in screen pixels (4, 8, 16 or 32).
    f: usize,
    /// Sample points: screen (x, y) of the block centre, and its value.
    samples: Vec<(f32, f32, f32)>,
}

/// Capture reduced to several scales (4, 8, 16 and 32 px blocks), computed in a single pass.
pub struct Pyramid {
    /// 4 px blocks (final refinement).
    l4: Level,
    /// 8 px blocks.
    l8: Level,
    /// 16 px blocks (local search).
    l16: Level,
    /// 32 px blocks (exhaustive global search).
    l32: Level,
}

/// Builds the sample pyramid of a capture. `masked(x, y)` returns true for pixels to ignore
/// (overlapping UI); blocks that are too dark are dropped as well.
pub fn pyramid(g: &Gray, masked: &dyn Fn(usize, usize) -> bool) -> Pyramid {
    // Level 4: average of 4 pixels per 4×4 block (step of 2); the other levels are derived from it.
    let (w4, h4) = (g.w / 4, g.h / 4);
    let mut g4 = vec![0f32; w4 * h4];
    for j in 0..h4 {
        let (y0, y1) = (j * 4, j * 4 + 2);
        for i in 0..w4 {
            let x = i * 4;
            let s = g.at(x, y0) as u32 + g.at(x + 2, y0) as u32 + g.at(x, y1) as u32 + g.at(x + 2, y1) as u32;
            g4[j * w4 + i] = s as f32 / 4.0;
        }
    }
    // Level of block size 4·k: average k×k level-4 cells, keep the bright unmasked ones, then thin them
    // evenly down to about `max_samples` points.
    let mk = |k: usize, max_samples: usize| -> Level {
        let f = 4 * k;
        let (w, h) = (w4 / k, h4 / k);
        let mut cand = Vec::with_capacity(w * h);
        for j in 0..h {
            for i in 0..w {
                let mut s = 0.0;
                for b in 0..k {
                    let row = (j * k + b) * w4 + i * k;
                    for a in 0..k {
                        s += g4[row + a];
                    }
                }
                let m = s / (k * k) as f32;
                let (cx, cy) = (i * f + f / 2, j * f + f / 2);
                if m > DARK && !masked(cx, cy) {
                    cand.push((cx as f32, cy as f32, m));
                }
            }
        }
        let stride = (cand.len() / max_samples).max(1);
        Level { f, samples: cand.into_iter().step_by(stride).collect() }
    };
    // Few samples at the coarse levels (wide searches), more for the refinement.
    Pyramid { l4: mk(1, 3000), l8: mk(2, 1500), l16: mk(4, 600), l32: mk(8, 300) }
}

/// Reference of map `map` resampled to m×m (area average when shrinking, bilinear otherwise).
fn ref_at(map: usize, m: usize) -> Vec<f32> {
    let r = crate::refs::full(map);
    let k = RN as f64 / m as f64;
    let mut out = vec![0.0; m * m];
    let p = |x: usize, y: usize| r[y.min(RN - 1) * RN + x.min(RN - 1)] as f32;
    if k > 1.5 {
        // Shrinking: average up to 8×8 evenly spaced source samples per output pixel.
        let n = (k.ceil() as usize).min(8);
        for j in 0..m {
            for i in 0..m {
                let mut s = 0.0;
                for b in 0..n {
                    for a in 0..n {
                        let x = ((i as f64 + (a as f64 + 0.5) / n as f64) * k) as usize;
                        let y = ((j as f64 + (b as f64 + 0.5) / n as f64) * k) as usize;
                        s += p(x, y);
                    }
                }
                out[j * m + i] = s / (n * n) as f32;
            }
        }
    } else {
        // Same size or enlarging: bilinear interpolation at each output pixel centre.
        let rf: Vec<f32> = r.iter().map(|&b| b as f32).collect();
        for j in 0..m {
            for i in 0..m {
                out[j * m + i] = bilinear(&rf, RN, ((i as f64 + 0.5) * k - 0.5) as f32, ((j as f64 + 0.5) * k - 0.5) as f32);
            }
        }
    }
    out
}

/// Bilinear sample of the m×m image `img` at sub-pixel position (u, v), clamped to the image edges.
#[inline]
fn bilinear(img: &[f32], m: usize, u: f32, v: f32) -> f32 {
    let (x0, y0) = (u.floor().max(0.0) as usize, v.floor().max(0.0) as usize);
    let (x0, y0) = (x0.min(m - 1), y0.min(m - 1));
    let (x1, y1) = ((x0 + 1).min(m - 1), (y0 + 1).min(m - 1));
    let (ax, ay) = ((u - x0 as f32).clamp(0.0, 1.0), (v - y0 as f32).clamp(0.0, 1.0));
    let a = img[y0 * m + x0] * (1.0 - ax) + img[y0 * m + x1] * ax;
    let b = img[y1 * m + x0] * (1.0 - ax) + img[y1 * m + x1] * ax;
    a * (1.0 - ay) + b * ay
}

/// NCC between the screen sample points and the reference (`side` screen px wide) placed with its top-left
/// corner at (tx, ty). `rf` is the reference resampled to m×m. Returns -1 when too few samples fall on the
/// reference (fewer than 200, or under 30% of the level's samples) or when either side is uniform.
fn score(l: &Level, rf: &[f32], m: usize, side: f64, tx: f64, ty: f64) -> f64 {
    let k = (m as f64 / side) as f32;
    let (tx, ty) = (tx as f32, ty as f32);
    let lim = (m - 1) as f32;
    let (mut n, mut sa, mut sb, mut sab, mut saa, mut sbb) = (0f64, 0f64, 0f64, 0f64, 0f64, 0f64);
    for &(x, y, a) in &l.samples {
        // Screen point -> reference coordinates; skip points that fall outside the map.
        let u = (x - tx) * k - 0.5;
        let v = (y - ty) * k - 0.5;
        if u < 0.0 || v < 0.0 || u > lim || v > lim {
            continue;
        }
        let b = bilinear(rf, m, u, v) as f64;
        let a = a as f64;
        n += 1.0;
        sa += a;
        sb += b;
        sab += a * b;
        saa += a * a;
        sbb += b * b;
    }
    if n < 200.0 || n < 0.3 * l.samples.len() as f64 {
        return -1.0;
    }
    let cov = sab - sa * sb / n;
    let va = saa - sa * sa / n;
    let vb = sbb - sb * sb / n;
    if va <= 1e-6 || vb <= 1e-6 {
        -1.0
    } else {
        cov / (va * vb).sqrt()
    }
}

/// Best position around (tx, ty) within ±rad with a step of `step` (screen px), as (score, x, y).
fn search(l: &Level, rf: &[f32], m: usize, side: f64, tx: f64, ty: f64, rad: f64, step: f64) -> (f64, f64, f64) {
    let mut best = (-2.0, tx, ty);
    let k = (rad / step).round() as i32;
    for dy in -k..=k {
        for dx in -k..=k {
            let (x, y) = (tx + dx as f64 * step, ty + dy as f64 * step);
            let s = score(l, rf, m, side, x, y);
            if s > best.0 {
                best = (s, x, y);
            }
        }
    }
    best
}

/// Result of a re-registration search.
pub struct Found {
    /// Top-left corner of the whole map, in client coordinates (x).
    pub x0: f64,
    /// Top-left corner of the whole map, in client coordinates (y).
    pub y0: f64,
    /// Zoom level found (0 = default view; each level adds half of the default side).
    pub n: i32,
    /// Correlation score at the position found.
    pub score: f64,
    /// Score at the predicted position (used to decide whether a correction is needed).
    pub score_pred: f64,
}

/// Local search around the prediction.
/// `pred` = predicted top-left corner (client) at level `n_pred`; `side0` = map side in the default view;
/// `zooms` = candidate levels; `radius` = search radius (screen px).
pub fn locate(p: &Pyramid, map: usize, side0: f64, pred: (f64, f64), n_pred: i32, zooms: &[i32], radius: f64) -> Option<Found> {
    let (l16, l8, l4) = (&p.l16, &p.l8, &p.l4);
    if l4.samples.len() < 400 {
        return None;
    }
    // On-screen map side at zoom level n.
    let side_of = |n: i32| (1.0 + 0.5 * n as f64) * side0;
    // Reference resampled so that one reference pixel matches one block of level `l`.
    let refs = |l: &Level, n: i32| {
        let m = ((side_of(n) / l.f as f64).round() as usize).max(8);
        (ref_at(map, m), m)
    };
    // Score of the current prediction, at the finest level.
    let (r4p, m4p) = refs(l4, n_pred);
    let score_pred = score(l4, &r4p, m4p, side_of(n_pred), pred.0, pred.1);
    // Zooming happens around the (panned) centre of the map: at another level, the predicted corner changes.
    let (cx, cy) = (pred.0 + side_of(n_pred) / 2.0, pred.1 + side_of(n_pred) / 2.0);
    let mut best: Option<(f64, i32, f64, f64)> = None;
    // 1) Each candidate zoom level: coarse 16 px search within `radius`; keep the best level.
    for &n in zooms {
        let (rf, m) = refs(l16, n);
        let (px, py) = (cx - side_of(n) / 2.0, cy - side_of(n) / 2.0);
        let (s, x, y) = search(l16, &rf, m, side_of(n), px, py, radius, 16.0);
        if best.map(|b| s > b.0).unwrap_or(true) {
            best = Some((s, n, x, y));
        }
    }
    let (_, n, x, y) = best?;
    let side = side_of(n);
    // 2) Refine at that level: 8 px blocks (±24, step 4), then 4 px blocks (±6, step 1).
    let (rf, m) = refs(l8, n);
    let (_, x, y) = search(l8, &rf, m, side, x, y, 24.0, 4.0);
    let (rf, m) = refs(l4, n);
    let (score, x0, y0) = search(l4, &rf, m, side, x, y, 6.0, 1.0);
    Some(Found { x0, y0, n, score, score_pred })
}

/// "From scratch" search: every zoom level, every possible position, used when the state is lost
/// (app started with the map already zoomed in, return from menus…). `center` = centre of the map screen
/// (default view).
pub fn locate_global(p: &Pyramid, map: usize, side0: f64, center: (f64, f64)) -> Option<Found> {
    if p.l4.samples.len() < 400 || p.l32.samples.len() < 60 {
        return None;
    }
    // On-screen map side at zoom level n.
    let side_of = |n: i32| (1.0 + 0.5 * n as f64) * side0;
    // 1) 32 px level: full sweep over zoom levels 0..=10, keeping positions scoring above 0.3; the 4 best
    //    candidates are refined below.
    let mut cands: Vec<(f64, i32, f64, f64)> = vec![];
    for n in 0..=10 {
        let side = side_of(n);
        let m = ((side / 32.0).round() as usize).max(8);
        let rf = ref_at(map, m);
        // Possible offset: up to half a side (+ margin: the pan limit is not re-applied when zooming out).
        let reach = side / 2.0 + 0.3 * side0;
        let (px, py) = (center.0 - side / 2.0, center.1 - side / 2.0);
        let k = (reach / 32.0).ceil() as i32;
        for dy in -k..=k {
            for dx in -k..=k {
                let (x, y) = (px + dx as f64 * 32.0, py + dy as f64 * 32.0);
                let s = score(&p.l32, &rf, m, side, x, y);
                if s > 0.3 {
                    cands.push((s, n, x, y));
                }
            }
        }
    }
    cands.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    // 2) Refine the best candidates (16 → 8 → 4 px); keep the best final score.
    let mut best: Option<Found> = None;
    for &(_, n, x, y) in cands.iter().take(4) {
        let side = side_of(n);
        let mut cur = (x, y);
        for (l, rad, step) in [(&p.l16, 32.0, 8.0), (&p.l8, 12.0, 4.0), (&p.l4, 6.0, 1.0)] {
            let m = ((side / l.f as f64).round() as usize).max(8);
            let rf = ref_at(map, m);
            let (s, x2, y2) = search(l, &rf, m, side, cur.0, cur.1, rad, step);
            cur = (x2, y2);
            if l.f == 4 && best.as_ref().map(|b| s > b.score).unwrap_or(true) {
                best = Some(Found { x0: x2, y0: y2, n, score: s, score_pred: -1.0 });
            }
        }
    }
    best
}

/// Kind of search requested from the worker thread.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// Around the predicted position.
    Local,
    /// Around the prediction, then a full search if nothing good is found (score below 0.6).
    LocalThenGlobal,
    /// Full search only (state lost).
    Global,
}

/// A re-registration request sent to the worker thread.
pub struct Job {
    /// Request id, echoed in the result so stale answers can be recognised.
    pub id: u64,
    /// Grayscale capture of the game window's client area.
    pub g: Gray,
    /// Areas to ignore (the game's help panel, our own panel) as (left, top, right, bottom), client coordinates.
    pub masks: Vec<(f64, f64, f64, f64)>,
    /// Index of the map believed to be shown (in refs::NAMES).
    pub map: usize,
    /// Map side in the default view, in client px.
    pub side0: f64,
    /// Centre of the map in the default view.
    pub center: (f64, f64),
    /// Predicted top-left corner of the map (client coordinates).
    pub pred: (f64, f64),
    /// Predicted zoom level.
    pub n_pred: i32,
    /// Zoom levels to try in the local search.
    pub zooms: Vec<i32>,
    /// Local search radius, in screen px.
    pub radius: f64,
    /// Which search to run.
    pub kind: Kind,
}

/// Result of a `Job`.
pub struct Res {
    /// Id of the job this answers.
    pub id: u64,
    /// Recognised map (a full search also tries the other maps).
    pub map: usize,
    /// Position found, if any.
    pub found: Option<Found>,
    /// True if a full (global) search was run.
    pub global_used: bool,
    /// Computation time in milliseconds.
    pub ms: u128,
}

/// Shared slot where the worker drops its latest result.
pub type ResSlot = std::sync::Arc<std::sync::Mutex<Option<Res>>>;

/// Starts the re-registration worker thread (the display is never blocked by a search).
/// `notify` is called after each result is stored, e.g. to wake the UI thread.
pub fn spawn_worker(notify: Box<dyn Fn() + Send>) -> (std::sync::mpsc::Sender<Job>, ResSlot) {
    let (tx, rx) = std::sync::mpsc::channel::<Job>();
    let slot: ResSlot = Default::default();
    let out = slot.clone();
    std::thread::spawn(move || {
        while let Ok(mut job) = rx.recv() {
            // Only the most recent request is processed.
            while let Ok(newer) = rx.try_recv() {
                job = newer;
            }
            let t0 = std::time::Instant::now();
            let masks = job.masks.clone();
            // A pixel is ignored if it lies inside any mask rectangle.
            let masked = move |x: usize, y: usize| {
                let (fx, fy) = (x as f64, y as f64);
                masks.iter().any(|&(l, t, r, b)| fx >= l && fx <= r && fy >= t && fy <= b)
            };
            let p = pyramid(&job.g, &masked);
            let mut found = None;
            let mut global_used = false;
            let mut map = job.map;
            if job.kind != Kind::Global {
                found = locate(&p, job.map, job.side0, job.pred, job.n_pred, &job.zooms, job.radius);
            }
            // Below 0.6 the local result is not trusted.
            let weak = found.as_ref().map(|f| f.score < 0.6).unwrap_or(true);
            if job.kind == Kind::Global {
                // Map lost: try the last known one first, then the others (the player may have changed raid).
                // Stops as soon as a score of 0.6 or more is reached.
                for mi in std::iter::once(job.map).chain((0..crate::refs::NAMES.len()).filter(|&i| i != job.map)) {
                    if let Some(gf) = locate_global(&p, mi, job.side0, job.center) {
                        global_used = true;
                        if found.as_ref().map(|f: &Found| gf.score > f.score).unwrap_or(true) {
                            found = Some(gf);
                            map = mi;
                        }
                        if found.as_ref().map(|f| f.score >= 0.6).unwrap_or(false) {
                            break;
                        }
                    }
                }
            } else if job.kind == Kind::LocalThenGlobal && weak {
                // Weak local result: run a full search on the same map, keeping the prediction's score.
                if let Some(gf) = locate_global(&p, job.map, job.side0, job.center) {
                    global_used = true;
                    let sp = found.as_ref().map(|f| f.score_pred).unwrap_or(-1.0);
                    if found.as_ref().map(|f| gf.score > f.score).unwrap_or(true) {
                        found = Some(Found { score_pred: sp, ..gf });
                    }
                }
            }
            *out.lock().unwrap() = Some(Res { id: job.id, map, found, global_used, ms: t0.elapsed().as_millis() });
            notify();
        }
    });
    (tx, slot)
}

#[cfg(test)]
mod tests {
    //! Offline validation on real captures whose position is known (SIFT registration).
    //! `HUNGER_RAWCAPS=<folder> cargo test --release -- --nocapture` (cases.txt + *.gray).
    use super::*;

    /// For each capture: identification (default view only), full search, and recovery from a wrong
    /// prediction. Skipped when HUNGER_RAWCAPS is not set.
    #[test]
    fn real_captures() {
        let Ok(dir) = std::env::var("HUNGER_RAWCAPS") else { return };
        let cases = std::fs::read_to_string(format!("{dir}/cases.txt")).unwrap();
        let (side0, center) = (1366.0, (596.92 + 683.0, 36.92 + 683.0));
        for line in cases.lines() {
            // Line format: name MapNN zoom_level true_x0 true_y0 width height
            let v: Vec<&str> = line.split_whitespace().collect();
            let (name, map, n): (&str, usize, i32) = (v[0], v[1][3..].parse::<usize>().unwrap() - 1, v[2].parse().unwrap());
            let (tx, ty): (f64, f64) = (v[3].parse().unwrap(), v[4].parse().unwrap());
            let (w, h): (usize, usize) = (v[5].parse().unwrap(), v[6].parse().unwrap());
            let g = Gray { w, h, px: std::fs::read(format!("{dir}/{name}.gray")).unwrap() };
            let masked = |_: usize, _: usize| false;
            if n == 0 {
                // Default view: identify the map with the thumbnail derived from the 4-bit reference.
                let sq = crate::detect::find_square(&g).expect("square");
                let id = crate::detect::identify(&g, &sq).expect("identification");
                println!("{name:7} identification: {} score {:.3} margin {:.3}", crate::refs::NAMES[id.0], id.1, id.2);
            }
            let t0 = std::time::Instant::now();
            let p = pyramid(&g, &masked);
            let tp = t0.elapsed().as_millis();
            // 1) Full search.
            let t1 = std::time::Instant::now();
            let gf = locate_global(&p, map, side0, center);
            let tg = t1.elapsed().as_millis();
            // 2) Re-registration after movement, with a zoom wrong by 2 steps and a position offset by 150 px.
            let side_of = |k: i32| (1.0 + 0.5 * k as f64) * side0;
            let wrong = (n - 2).max(0);
            let (cx, cy) = (tx + side_of(n) / 2.0 + 150.0, ty + side_of(n) / 2.0 - 120.0);
            let pred = (cx - side_of(wrong) / 2.0, cy - side_of(wrong) / 2.0);
            let zooms: Vec<i32> = (wrong - 2..=wrong + 2).filter(|k| (0..=10).contains(k)).collect();
            let t2 = std::time::Instant::now();
            let mf = locate(&p, map, side0, pred, wrong, &zooms, 400.0);
            let tm = t2.elapsed().as_millis();
            let fmt = |f: &Option<Found>| match f {
                Some(f) => format!("n={} err=({:+.1},{:+.1}) score={:.3}", f.n, f.x0 - tx, f.y0 - ty, f.score),
                None => "FAILED".into(),
            };
            println!("{name:7} true n={n:2} | pyramid {tp:3} ms | full {tg:4} ms: {} | after movement (wrong n {wrong}) {tm:3} ms: {}", fmt(&gf), fmt(&mf));
        }
    }
}
