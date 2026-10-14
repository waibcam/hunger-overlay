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

fn level(g: &Gray, f: usize, max_samples: usize, masked: &dyn Fn(usize, usize) -> bool) -> Level {
    let (w, h) = (g.w / f, g.h / f);
    let st = if f >= 8 { 2 } else { 1 };
    let cnt = ((f / st) * (f / st)) as f32;
    let mut cand = vec![];
    for j in 0..h {
        for i in 0..w {
            let (x0, y0) = (i * f, j * f);
            let mut s = 0u32;
            for y in (y0..y0 + f).step_by(st) {
                for x in (x0..x0 + f).step_by(st) {
                    s += g.at(x, y) as u32;
                }
            }
            let m = s as f32 / cnt;
            let (cx, cy) = (x0 + f / 2, y0 + f / 2);
            if m > DARK && !masked(cx, cy) {
                cand.push((cx as f32, cy as f32, m));
            }
        }
    }
    let stride = (cand.len() / max_samples).max(1);
    let samples = cand.into_iter().step_by(stride).collect();
    Level { f, samples }
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
pub fn locate(g: &Gray, map: usize, side0: f64, pred: (f64, f64), n_pred: i32, zooms: &[i32], radius: f64, masked: &dyn Fn(usize, usize) -> bool) -> Option<Found> {
    // Peu d'échantillons au niveau grossier (recherche large), davantage pour l'affinage.
    let l16 = level(g, 16, 600, masked);
    let l8 = level(g, 8, 1500, masked);
    let l4 = level(g, 4, 3000, masked);
    if l4.samples.len() < 400 {
        return None;
    }
    let side_of = |n: i32| (1.0 + 0.5 * n as f64) * side0;
    let refs = |l: &Level, n: i32| {
        let m = ((side_of(n) / l.f as f64).round() as usize).max(8);
        (ref_at(map, m), m)
    };
    let (r4p, m4p) = refs(&l4, n_pred);
    let score_pred = score(&l4, &r4p, m4p, side_of(n_pred), pred.0, pred.1);
    // Le zoom se fait autour du centre (déplacé) de la carte : à un autre niveau, le coin prédit change.
    let (cx, cy) = (pred.0 + side_of(n_pred) / 2.0, pred.1 + side_of(n_pred) / 2.0);
    let mut best: Option<(f64, i32, f64, f64)> = None;
    for &n in zooms {
        let (rf, m) = refs(&l16, n);
        let (px, py) = (cx - side_of(n) / 2.0, cy - side_of(n) / 2.0);
        let (s, x, y) = search(&l16, &rf, m, side_of(n), px, py, radius, 16.0);
        if best.map(|b| s > b.0).unwrap_or(true) {
            best = Some((s, n, x, y));
        }
    }
    let (_, n, x, y) = best?;
    let side = side_of(n);
    let (rf, m) = refs(&l8, n);
    let (_, x, y) = search(&l8, &rf, m, side, x, y, 24.0, 4.0);
    let (rf, m) = refs(&l4, n);
    let (score, x0, y0) = search(&l4, &rf, m, side, x, y, 6.0, 1.0);
    Some(Found { x0, y0, n, score, score_pred })
}
