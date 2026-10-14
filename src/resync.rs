//! Recalage visuel : retrouve la position (et au besoin le niveau de zoom) réels de la carte à l'écran, en
//! comparant la capture à la texture de référence. Filet de sécurité du suivi par les entrées (événement souris
//! perdu, retour de focus, retour des menus, limite de déplacement non réappliquée au dézoom…).
//!
//! La texture est très fine (hachures, arbres) : la corrélation chute dès quelques pixels de décalage. On
//! compare donc des points échantillons de l'écran à la référence interpolée (bilinéaire) à une position
//! sous-pixel, du grossier au fin : blocs de 16 px (pas 16), puis 8 px (pas 4), puis 4 px (pas 1).

use crate::capture::Gray;

const RN: usize = 512;
pub static REF512: [&[u8]; 3] = [
    include_bytes!("../assets/ref512_Map01.bin"),
    include_bytes!("../assets/ref512_Map02.bin"),
    include_bytes!("../assets/ref512_Map03.bin"),
];

/// Blocs plus sombres que ce seuil = hors carte (fond de l'écran carte, fenêtres masquées).
const DARK: f32 = 55.0;

struct Level {
    f: usize,
    /// Points échantillons : (x, y) écran du centre du bloc, valeur.
    samples: Vec<(f32, f32, f32)>,
}

/// Capture réduite à plusieurs échelles (blocs de 4, 8, 16 et 32 px), calculée en une seule passe.
pub struct Pyramid {
    l4: Level,
    l8: Level,
    l16: Level,
    l32: Level,
}

pub fn pyramid(g: &Gray, masked: &dyn Fn(usize, usize) -> bool) -> Pyramid {
    // Niveau 4 : moyenne de 4 pixels par bloc 4×4 (pas de 2), le reste en est dérivé.
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
    // Peu d'échantillons aux niveaux grossiers (recherches larges), davantage pour l'affinage.
    Pyramid { l4: mk(1, 3000), l8: mk(2, 1500), l16: mk(4, 600), l32: mk(8, 300) }
}

/// Référence rééchantillonnée à m×m (moyenne de zone si réduction, bilinéaire sinon).
fn ref_at(map: usize, m: usize) -> Vec<f32> {
    let r = REF512[map];
    let k = RN as f64 / m as f64;
    let mut out = vec![0.0; m * m];
    let p = |x: usize, y: usize| r[y.min(RN - 1) * RN + x.min(RN - 1)] as f32;
    if k > 1.5 {
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
        let rf: Vec<f32> = r.iter().map(|&b| b as f32).collect();
        for j in 0..m {
            for i in 0..m {
                out[j * m + i] = bilinear(&rf, RN, ((i as f64 + 0.5) * k - 0.5) as f32, ((j as f64 + 0.5) * k - 0.5) as f32);
            }
        }
    }
    out
}

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

/// NCC entre les points de l'écran et la référence (côté `side` px écran) posée en (tx, ty).
fn score(l: &Level, rf: &[f32], m: usize, side: f64, tx: f64, ty: f64) -> f64 {
    let k = (m as f64 / side) as f32;
    let (tx, ty) = (tx as f32, ty as f32);
    let lim = (m - 1) as f32;
    let (mut n, mut sa, mut sb, mut sab, mut saa, mut sbb) = (0f64, 0f64, 0f64, 0f64, 0f64, 0f64);
    for &(x, y, a) in &l.samples {
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

/// Meilleure position autour de (tx, ty) dans ±rad avec un pas `step` (px écran).
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

pub struct Found {
    /// Coin haut-gauche de la carte entière, en coordonnées client.
    pub x0: f64,
    pub y0: f64,
    pub n: i32,
    pub score: f64,
    /// Score à la position prédite (pour décider s'il faut corriger).
    pub score_pred: f64,
}

/// `pred` = coin haut-gauche prédit (client) au niveau `n_pred` ; `side0` = côté en vue par défaut ;
/// `zooms` = niveaux candidats ; `radius` = rayon de recherche (px écran).
pub fn locate(p: &Pyramid, map: usize, side0: f64, pred: (f64, f64), n_pred: i32, zooms: &[i32], radius: f64) -> Option<Found> {
    let (l16, l8, l4) = (&p.l16, &p.l8, &p.l4);
    if l4.samples.len() < 400 {
        return None;
    }
    let side_of = |n: i32| (1.0 + 0.5 * n as f64) * side0;
    let refs = |l: &Level, n: i32| {
        let m = ((side_of(n) / l.f as f64).round() as usize).max(8);
        (ref_at(map, m), m)
    };
    let (r4p, m4p) = refs(l4, n_pred);
    let score_pred = score(l4, &r4p, m4p, side_of(n_pred), pred.0, pred.1);
    // Le zoom se fait autour du centre (déplacé) de la carte : à un autre niveau, le coin prédit change.
    let (cx, cy) = (pred.0 + side_of(n_pred) / 2.0, pred.1 + side_of(n_pred) / 2.0);
    let mut best: Option<(f64, i32, f64, f64)> = None;
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
    let (rf, m) = refs(l8, n);
    let (_, x, y) = search(l8, &rf, m, side, x, y, 24.0, 4.0);
    let (rf, m) = refs(l4, n);
    let (score, x0, y0) = search(l4, &rf, m, side, x, y, 6.0, 1.0);
    Some(Found { x0, y0, n, score, score_pred })
}

/// Recherche « de zéro » : tous les niveaux de zoom, toutes les positions possibles, quand l'état est perdu
/// (appli lancée carte déjà zoomée, retour des menus…). `center` = centre de l'écran carte (vue par défaut).
pub fn locate_global(p: &Pyramid, map: usize, side0: f64, center: (f64, f64)) -> Option<Found> {
    if p.l4.samples.len() < 400 || p.l32.samples.len() < 60 {
        return None;
    }
    let side_of = |n: i32| (1.0 + 0.5 * n as f64) * side0;
    // 1) Niveau 32 px : balayage complet, on garde les 4 meilleurs candidats.
    let mut cands: Vec<(f64, i32, f64, f64)> = vec![];
    for n in 0..=10 {
        let side = side_of(n);
        let m = ((side / 32.0).round() as usize).max(8);
        let rf = ref_at(map, m);
        // Décalage possible : jusqu'à un demi-côté (+ marge : la limite n'est pas réappliquée au dézoom).
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
    // 2) Affinage des meilleurs candidats (16 → 8 → 4 px) ; on garde le meilleur score final.
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

/// Type de recherche demandée au fil de calcul.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// Autour de la position prédite.
    Local,
    /// Autour de la prédiction, puis recherche complète si rien n'est trouvé.
    LocalThenGlobal,
    /// Recherche complète uniquement (état perdu).
    Global,
}

pub struct Job {
    pub id: u64,
    pub g: Gray,
    /// Zones à ignorer (panneau d'aide du jeu, notre panneau), coordonnées client.
    pub masks: Vec<(f64, f64, f64, f64)>,
    pub map: usize,
    pub side0: f64,
    /// Centre de la carte en vue par défaut.
    pub center: (f64, f64),
    pub pred: (f64, f64),
    pub n_pred: i32,
    pub zooms: Vec<i32>,
    pub radius: f64,
    pub kind: Kind,
}

pub struct Res {
    pub id: u64,
    /// Carte reconnue (une recherche complète essaie aussi les autres cartes).
    pub map: usize,
    pub found: Option<Found>,
    pub global_used: bool,
    pub ms: u128,
}

pub type ResSlot = std::sync::Arc<std::sync::Mutex<Option<Res>>>;

/// Lance le fil de calcul des recalages (l'affichage n'est jamais bloqué par une recherche).
pub fn spawn_worker(notify: Box<dyn Fn() + Send>) -> (std::sync::mpsc::Sender<Job>, ResSlot) {
    let (tx, rx) = std::sync::mpsc::channel::<Job>();
    let slot: ResSlot = Default::default();
    let out = slot.clone();
    std::thread::spawn(move || {
        while let Ok(mut job) = rx.recv() {
            // On ne traite que la demande la plus récente.
            while let Ok(newer) = rx.try_recv() {
                job = newer;
            }
            let t0 = std::time::Instant::now();
            let masks = job.masks.clone();
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
            let weak = found.as_ref().map(|f| f.score < 0.6).unwrap_or(true);
            if job.kind == Kind::Global {
                // Carte perdue : la dernière connue d'abord, puis les autres (on a pu changer de raid).
                for mi in std::iter::once(job.map).chain((0..REF512.len()).filter(|&i| i != job.map)) {
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
    //! Validation hors ligne sur des captures réelles dont la position est connue (recalage SIFT).
    //! `HUNGER_RAWCAPS=<dossier> cargo test --release -- --nocapture` (cases.txt + *.gray).
    use super::*;

    #[test]
    fn real_captures() {
        let Ok(dir) = std::env::var("HUNGER_RAWCAPS") else { return };
        let cases = std::fs::read_to_string(format!("{dir}/cases.txt")).unwrap();
        let (side0, center) = (1366.0, (596.92 + 683.0, 36.92 + 683.0));
        for line in cases.lines() {
            let v: Vec<&str> = line.split_whitespace().collect();
            let (name, map, n): (&str, usize, i32) = (v[0], v[1][3..].parse::<usize>().unwrap() - 1, v[2].parse().unwrap());
            let (tx, ty): (f64, f64) = (v[3].parse().unwrap(), v[4].parse().unwrap());
            let (w, h): (usize, usize) = (v[5].parse().unwrap(), v[6].parse().unwrap());
            let g = Gray { w, h, px: std::fs::read(format!("{dir}/{name}.gray")).unwrap() };
            let masked = |_: usize, _: usize| false;
            let t0 = std::time::Instant::now();
            let p = pyramid(&g, &masked);
            let tp = t0.elapsed().as_millis();
            // 1) Recherche complète.
            let t1 = std::time::Instant::now();
            let gf = locate_global(&p, map, side0, center);
            let tg = t1.elapsed().as_millis();
            // 2) Recalage après mouvement avec un zoom faux de 2 crans et une position décalée de 150 px.
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
                None => "ÉCHEC".into(),
            };
            println!("{name:7} vrai n={n:2} | pyramide {tp:3} ms | complète {tg:4} ms : {} | après mouvement (n faux {wrong}) {tm:3} ms : {}", fmt(&gf), fmt(&mf));
        }
    }
}
