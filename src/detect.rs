//! Détection visuelle de la carte du jeu (mesures : docs/overlay/MAP_UI_MEASUREMENTS.md).
//!
//! - À l'ouverture, la carte est TOUJOURS en vue par défaut : un carré de parchemin clair, centré, sur fond sombre.
//! - Ce carré est la texture ENTIÈRE de la carte → on l'identifie en comparant une vignette 128×128 aux 3 références.
//! - Le panneau d'aide en bas à gauche (« Pan Around / Adjust Zoom / CLOSE ») reste affiché tant que la carte est
//!   ouverte : sa signature sert à détecter la fermeture.

use crate::capture::Gray;

pub const REF_N: usize = crate::refs::THUMB;

const BRIGHT: u8 = 150;

#[derive(Debug, Clone, Copy)]
pub struct Square {
    pub x0: f64,
    pub y0: f64,
    pub side: f64,
}

fn longest_run(flags: &[bool]) -> Option<(usize, usize)> {
    let (mut best, mut start) = (None::<(usize, usize)>, None);
    for (i, &f) in flags.iter().chain(std::iter::once(&false)).enumerate() {
        match (f, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if best.map(|(a, b)| b - a < i - 1 - s).unwrap_or(true) {
                    best = Some((s, i - 1));
                }
                start = None;
            }
            _ => {}
        }
    }
    best
}

/// Cherche le carré de parchemin (vue par défaut). Renvoie None si la carte n'est pas affichée.
pub fn find_square(g: &Gray) -> Option<Square> {
    const S: usize = 4;
    let (cw, ch) = (g.w / S, g.h / S);
    if cw < 32 || ch < 32 {
        return None;
    }
    let row_flags: Vec<bool> = (0..ch)
        .map(|r| (0..cw).filter(|&c| g.at(c * S, r * S) > BRIGHT).count() as f64 / cw as f64 > 0.3)
        .collect();
    let (r0, r1) = longest_run(&row_flags)?;
    let col_flags: Vec<bool> = (0..cw)
        .map(|c| (r0..=r1).filter(|&r| g.at(c * S, r * S) > BRIGHT).count() as f64 / (r1 - r0 + 1) as f64 > 0.3)
        .collect();
    let (c0, c1) = longest_run(&col_flags)?;
    let (cx0, cx1, cy0, cy1) = (c0 * S, c1 * S, r0 * S, r1 * S);
    // Affinage à pleine résolution : bord = première/dernière ligne majoritairement claire.
    let mid_rows: Vec<usize> = (cy0 + (cy1 - cy0) / 4..cy0 + 3 * (cy1 - cy0) / 4).step_by(2).collect();
    let mid_cols: Vec<usize> = (cx0 + (cx1 - cx0) / 4..cx0 + 3 * (cx1 - cx0) / 4).step_by(2).collect();
    let col_frac = |x: usize| mid_rows.iter().filter(|&&y| g.at(x, y) > BRIGHT).count() as f64 / mid_rows.len().max(1) as f64;
    let row_frac = |y: usize| mid_cols.iter().filter(|&&x| g.at(x, y) > BRIGHT).count() as f64 / mid_cols.len().max(1) as f64;
    let span = |a: usize, b: usize, lim: usize| (a.saturating_sub(8), (b + 8).min(lim - 1));
    let (lx0, lx1) = span(cx0, cx0, g.w);
    let x0 = (lx0..=lx1).find(|&x| col_frac(x) > 0.5)?;
    let (rx0, rx1) = span(cx1, cx1 + S, g.w);
    let x1 = (rx0..=rx1).rev().find(|&x| col_frac(x) > 0.5)?;
    let (ty0, ty1) = span(cy0, cy0, g.h);
    let y0 = (ty0..=ty1).find(|&y| row_frac(y) > 0.5)?;
    let (by0, by1) = span(cy1, cy1 + S, g.h);
    let y1 = (by0..=by1).rev().find(|&y| row_frac(y) > 0.5)?;
    let (w, h) = ((x1 - x0 + 1) as f64, (y1 - y0 + 1) as f64);
    if h < 0.5 * g.h as f64 || (w - h).abs() > (0.01 * w).max(4.0) {
        return None;
    }
    Some(Square { x0: x0 as f64, y0: y0 as f64, side: (w + h) / 2.0 })
}

/// Vignette 128×128 (moyenne par blocs) du carré détecté.
fn thumb(g: &Gray, sq: &Square) -> Vec<f64> {
    let mut out = vec![0.0; REF_N * REF_N];
    let cell = sq.side / REF_N as f64;
    for j in 0..REF_N {
        for i in 0..REF_N {
            let (xa, ya) = (sq.x0 + i as f64 * cell, sq.y0 + j as f64 * cell);
            let (mut s, mut n) = (0.0, 0.0);
            let steps = (cell.ceil() as usize).clamp(1, 6);
            for b in 0..steps {
                for a in 0..steps {
                    let x = (xa + (a as f64 + 0.5) * cell / steps as f64) as usize;
                    let y = (ya + (b as f64 + 0.5) * cell / steps as f64) as usize;
                    if x < g.w && y < g.h {
                        s += g.at(x, y) as f64;
                        n += 1.0;
                    }
                }
            }
            out[j * REF_N + i] = if n > 0.0 { s / n } else { 0.0 };
        }
    }
    out
}

fn ncc(a: &[f64], b: &[u8]) -> f64 {
    let n = a.len() as f64;
    let ma = a.iter().sum::<f64>() / n;
    let mb = b.iter().map(|&v| v as f64).sum::<f64>() / n;
    let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
    for (x, &y) in a.iter().zip(b) {
        let (da, db) = (x - ma, y as f64 - mb);
        sab += da * db;
        saa += da * da;
        sbb += db * db;
    }
    if saa <= 0.0 || sbb <= 0.0 {
        0.0
    } else {
        sab / (saa * sbb).sqrt()
    }
}

/// Identifie la carte : (index dans refs::NAMES, score, écart avec la 2e). None si trop incertain.
pub fn identify(g: &Gray, sq: &Square) -> Option<(usize, f64, f64)> {
    let t = thumb(g, sq);
    let mut scores: Vec<(usize, f64)> = (0..crate::refs::NAMES.len()).map(|i| (i, ncc(&t, crate::refs::thumb(i)))).collect();
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let (best, s1) = scores[0];
    let s2 = scores[1].1;
    if s1 > 0.5 && s1 - s2 > 0.1 {
        Some((best, s1, s1 - s2))
    } else {
        None
    }
}

/// Zone du panneau d'aide (coordonnées client), proportionnelle à la hauteur et ancrée en bas à gauche.
pub fn panel_rect(h: f64) -> (f64, f64, f64, f64) {
    (0.034 * h, h - 0.202 * h, 0.269 * h, h - 0.035 * h)
}

/// Corrélation normalisée entre deux images de même taille (sous-échantillonnées d'un facteur 2).
pub fn gray_ncc(a: &Gray, b: &Gray) -> f64 {
    if a.w != b.w || a.h != b.h || a.w < 4 || a.h < 4 {
        return 0.0;
    }
    let (mut n, mut sa, mut sb, mut sab, mut saa, mut sbb) = (0f64, 0f64, 0f64, 0f64, 0f64, 0f64);
    for y in (0..a.h).step_by(2) {
        for x in (0..a.w).step_by(2) {
            let (u, v) = (a.at(x, y) as f64, b.at(x, y) as f64);
            n += 1.0;
            sa += u;
            sb += v;
            sab += u * v;
            saa += u * u;
            sbb += v * v;
        }
    }
    let cov = sab - sa * sb / n;
    let va = saa - sa * sa / n;
    let vb = sbb - sb * sb / n;
    if va <= 1e-6 || vb <= 1e-6 {
        0.0
    } else {
        cov / (va * vb).sqrt()
    }
}

/// Signature du panneau d'aide : l'image de sa zone, comparée par corrélation (robuste au survol du
/// bouton CLOSE, mais distingue le panneau d'autres écrans sombres comme le Compendium).
#[derive(Clone)]
pub struct PanelSig {
    x: i32,
    y: i32,
    img: Gray,
}

impl PanelSig {
    pub fn from(g: Gray, x: i32, y: i32) -> Option<PanelSig> {
        let n = g.px.len() as f64;
        let m = g.px.iter().map(|&v| v as f64).sum::<f64>() / n;
        let sd = (g.px.iter().map(|&v| (v as f64 - m).powi(2)).sum::<f64>() / n).sqrt();
        if sd < 15.0 {
            return None; // zone uniforme : pas un panneau
        }
        Some(PanelSig { x, y, img: g })
    }

    pub fn rect(&self) -> (i32, i32, i32, i32) {
        (self.x, self.y, self.img.w as i32, self.img.h as i32)
    }

    pub fn score(&self, g: &Gray) -> f64 {
        gray_ncc(&self.img, g)
    }

    /// Sérialisation simple (pour retrouver la carte au lancement suivant).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(16 + self.img.px.len());
        for n in [self.x, self.y, self.img.w as i32, self.img.h as i32] {
            v.extend_from_slice(&n.to_le_bytes());
        }
        v.extend_from_slice(&self.img.px);
        v
    }

    pub fn from_bytes(b: &[u8]) -> Option<PanelSig> {
        let rd = |i: usize| Some(i32::from_le_bytes(b.get(i * 4..i * 4 + 4)?.try_into().ok()?));
        let (x, y, w, h) = (rd(0)?, rd(1)?, rd(2)? as usize, rd(3)? as usize);
        let px = b.get(16..16 + w * h)?.to_vec();
        Some(PanelSig { x, y, img: Gray { w, h, px } })
    }
}

/// Sonde très légère : une ligne horizontale et une colonne au centre de l'écran. Vraie si on voit un carré
/// clair centré sur fond sombre (vue par défaut de la carte). Coût : 2 petites captures.
pub fn probe(x: i32, y: i32, w: i32, h: i32) -> bool {
    let (Some(row), Some(col)) = (crate::capture::grab_gray(x, y + h / 2, w, 1), crate::capture::grab_gray(x + w / 2, y, 1, h)) else {
        return false;
    };
    let (wf, hf) = (w as f64, h as f64);
    let half = 0.474 * hf; // demi-côté attendu du carré (mesuré : 683/1440)
    let frac = |v: &[u8], a: f64, b: f64, f: &dyn Fn(u8) -> bool| -> Option<f64> {
        let (a, b) = (a.max(0.0) as usize, (b.min(v.len() as f64)) as usize);
        if b <= a + 4 {
            return None;
        }
        Some(v[a..b].iter().filter(|&&p| f(p)).count() as f64 / (b - a) as f64)
    };
    let bright = |p: u8| p > BRIGHT;
    let dark = |p: u8| p < 70;
    let cx = wf / 2.0;
    // Bande centrale claire (horizontale et verticale).
    if frac(&row.px, cx - 0.3 * hf, cx + 0.3 * hf, &bright).unwrap_or(0.0) < 0.5 {
        return false;
    }
    if frac(&col.px, 0.2 * hf, 0.8 * hf, &bright).unwrap_or(0.0) < 0.5 {
        return false;
    }
    // Marges latérales sombres, si l'écran est assez large pour en avoir.
    let margin = cx - half - 0.01 * wf;
    if margin > 0.03 * wf {
        for (a, b) in [(0.01 * wf, margin), (wf - margin, 0.99 * wf)] {
            if frac(&row.px, a, b, &dark).unwrap_or(1.0) < 0.85 {
                return false;
            }
        }
    }
    true
}
