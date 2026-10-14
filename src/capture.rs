//! Lecture de pixels de l'écran (GDI BitBlt). Sans CAPTUREBLT, nos propres fenêtres superposées ne sont pas capturées.

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};

/// Image en niveaux de gris (luminance 0..255).
pub struct Gray {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Gray {
    #[inline]
    pub fn at(&self, x: usize, y: usize) -> u8 {
        self.px[y * self.w + x]
    }
}

/// Capture le rectangle (x, y, w, h) en coordonnées ÉCRAN et le convertit en niveaux de gris.
pub fn grab_gray(x: i32, y: i32, w: i32, h: i32) -> Option<Gray> {
    if w <= 0 || h <= 0 {
        return None;
    }
    unsafe {
        let screen = GetDC(HWND::default());
        if screen.is_invalid() {
            return None;
        }
        let mem = CreateCompatibleDC(screen);
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(screen, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let old = SelectObject(mem, dib);
        let ok = BitBlt(mem, 0, 0, w, h, screen, x, y, SRCCOPY).is_ok();
        let mut out = None;
        if ok && !bits.is_null() {
            let n = (w * h) as usize;
            let src = std::slice::from_raw_parts(bits as *const u8, n * 4);
            let mut px = vec![0u8; n];
            for (i, p) in px.iter_mut().enumerate() {
                let b = src[i * 4] as u32;
                let g = src[i * 4 + 1] as u32;
                let r = src[i * 4 + 2] as u32;
                *p = ((r * 77 + g * 150 + b * 29) >> 8) as u8;
            }
            out = Some(Gray { w: w as usize, h: h as usize, px });
        }
        SelectObject(mem, old);
        let _ = DeleteObject(dib);
        let _ = DeleteDC(mem);
        ReleaseDC(HWND::default(), screen);
        out
    }
}
