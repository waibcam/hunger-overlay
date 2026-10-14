//! Reference images of the game's 3 maps (the map texture, in grayscale), embedded in the executable.
//!
//! Storage: 512×512 at 4 bits per pixel (2 pixels per byte), i.e. 128 KB per map. They are decoded only once,
//! the first time they are needed. The 128×128 thumbnail (used to identify the map) is derived from them
//! instead of being stored.

use std::sync::OnceLock;

/// Side of the full reference images, in pixels.
pub const N: usize = 512;
/// Side of the identification thumbnails, in pixels.
pub const THUMB: usize = 128;
/// Map ids, in the same order as the embedded references (index = map index everywhere in the app).
pub const NAMES: [&str; 3] = ["Map01", "Map02", "Map03"];

/// Embedded 4-bit packed references (high nibble = left pixel, low nibble = right pixel).
static PACKED: [&[u8]; 3] = [
    include_bytes!("../assets/ref512_Map01.bin"),
    include_bytes!("../assets/ref512_Map02.bin"),
    include_bytes!("../assets/ref512_Map03.bin"),
];

/// Decoded 512×512 references (lazily initialised).
static FULL: OnceLock<Vec<Vec<u8>>> = OnceLock::new();
/// Derived 128×128 thumbnails (lazily initialised).
static THUMBS: OnceLock<Vec<Vec<u8>>> = OnceLock::new();

/// 512×512 reference (u8) of map `i`.
pub fn full(i: usize) -> &'static [u8] {
    let all = FULL.get_or_init(|| {
        // Unpack each byte into 2 pixels; ×17 maps the 4-bit range 0..15 onto 0..255.
        PACKED
            .iter()
            .map(|p| p.iter().flat_map(|&b| [(b >> 4) * 17, (b & 15) * 17]).collect())
            .collect()
    });
    &all[i]
}

/// 128×128 thumbnail of map `i` (average of 4×4 blocks of the 512 reference).
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
