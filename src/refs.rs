//! Images de référence des 3 cartes du jeu (texture de la carte, en niveaux de gris), embarquées dans l'exe.
//!
//! Stockage : 512×512 sur 4 bits (2 pixels par octet), soit 128 Ko par carte. Elles sont décodées une seule fois,
//! au premier besoin. La vignette 128×128 (identification de la carte) en est dérivée, au lieu d'être stockée.

use std::sync::OnceLock;

pub const N: usize = 512;
pub const THUMB: usize = 128;
pub const NAMES: [&str; 3] = ["Map01", "Map02", "Map03"];

static PACKED: [&[u8]; 3] = [
    include_bytes!("../assets/ref512_Map01.bin"),
    include_bytes!("../assets/ref512_Map02.bin"),
    include_bytes!("../assets/ref512_Map03.bin"),
];

static FULL: OnceLock<Vec<Vec<u8>>> = OnceLock::new();
static THUMBS: OnceLock<Vec<Vec<u8>>> = OnceLock::new();

/// Référence 512×512 (u8) de la carte `i`.
pub fn full(i: usize) -> &'static [u8] {
    let all = FULL.get_or_init(|| {
        PACKED
            .iter()
            .map(|p| p.iter().flat_map(|&b| [(b >> 4) * 17, (b & 15) * 17]).collect())
            .collect()
    });
    &all[i]
}

/// Vignette 128×128 (moyenne de blocs 4×4 de la référence 512).
pub fn thumb(i: usize) -> &'static [u8] {
    let all = THUMBS.get_or_init(|| {
        (0..NAMES.len())
            .map(|m| {
                let f = full(m);
                let k = N / THUMB;
                let mut out = vec![0u8; THUMB * THUMB];
                for y in 0..THUMB {
                    for x in 0..THUMB {
                        let mut s = 0u32;
                        for b in 0..k {
                            for a in 0..k {
                                s += f[(y * k + b) * N + x * k + a] as u32;
                            }
                        }
                        out[y * THUMB + x] = (s / (k * k) as u32) as u8;
                    }
                }
                out
            })
            .collect()
    });
    &all[i]
}
