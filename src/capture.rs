//! Reads screen pixels (GDI BitBlt). This is the app's only input from the game: it looks at what is on screen,
//! exactly like a screenshot, and never reads the game's memory or files.
//! Without the CAPTUREBLT flag, our own layered (overlay) windows are not included in the capture.

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};

/// Grayscale image (luminance 0..255).
#[derive(Clone)]
pub struct Gray {
    /// Width in pixels.
    pub w: usize,
    /// Height in pixels.
    pub h: usize,
    /// Row-major pixels, `w * h` bytes.
    pub px: Vec<u8>,
}

impl Gray {
    /// Luminance of the pixel at (x, y). Panics if out of bounds.
    #[inline]
    pub fn at(&self, x: usize, y: usize) -> u8 {
        self.px[y * self.w + x]
    }
}

/// Captures the rectangle (x, y, w, h) in SCREEN coordinates and converts it to grayscale.
/// Returns None if the size is empty or a GDI call fails.
pub fn grab_gray(x: i32, y: i32, w: i32, h: i32) -> Option<Gray> {
    if w <= 0 || h <= 0 {
        return None;
    }
    unsafe {
        let screen = GetDC(HWND::default());
        if screen.is_invalid() {
            return None;
        }
        // Off-screen 32-bit DIB to copy into; a negative height makes it top-down (row 0 = top).
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
        // Copy the screen rectangle into the DIB.
        let old = SelectObject(mem, dib);
        let ok = BitBlt(mem, 0, 0, w, h, screen, x, y, SRCCOPY).is_ok();
        let mut out = None;
        if ok && !bits.is_null() {
            let n = (w * h) as usize;
            let src = std::slice::from_raw_parts(bits as *const u8, n * 4);
            let mut px = vec![0u8; n];
            // BGRA -> luminance with integer weights ~ 0.30 R + 0.59 G + 0.11 B (sum 256).
            for (i, p) in px.iter_mut().enumerate() {
                let b = src[i * 4] as u32;
                let g = src[i * 4 + 1] as u32;
                let r = src[i * 4 + 2] as u32;
                *p = ((r * 77 + g * 150 + b * 29) >> 8) as u8;
            }
            out = Some(Gray { w: w as usize, h: h as usize, px });
        }
        // Release every GDI object we created.
        SelectObject(mem, old);
        let _ = DeleteObject(dib);
        let _ = DeleteDC(mem);
        ReleaseDC(HWND::default(), screen);
        out
    }
}
