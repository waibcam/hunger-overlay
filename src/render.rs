//! Direct2D/DirectWrite rendering into a premultiplied 32-bit DIB, pushed onto the layered window
//! (UpdateLayeredWindow, per-pixel alpha). No access whatsoever to the game's own rendering.

use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush, ID2D1StrokeStyle, D2D1_CAP_STYLE_ROUND,
    D2D1_DASH_STYLE_DASH, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_LINE_JOIN_ROUND, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1_DRAW_TEXT_OPTIONS_NONE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout, IDWriteTextLayout1, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL, DWRITE_TEXT_METRICS, DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_WORD_WRAPPING_WRAP, DWRITE_TEXT_RANGE,
};
use windows::Foundation::Numerics::Matrix3x2;
use windows::core::Interface;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject, AC_SRC_ALPHA,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

/// A colour as (red, green, blue, alpha), each component in 0.0..=1.0.
pub type Rgba = (f32, f32, f32, f32);

/// The text styles available to callers (index into `Renderer::fonts`, same order).
/// The first four follow the caller's base sizes (`set_base_sizes`); the others have fixed sizes
/// (px at 1440p, see docs/DESIGN.md §3.3). Single-line styles end with "…" when they do not fit.
#[derive(Clone, Copy)]
pub enum Font {
    /// Bold, single line (truncated with trimming if too long).
    Title,
    /// Semi-bold, wrapping.
    Bold,
    /// Regular body text, wrapping.
    Text,
    /// Smaller regular text, wrapping.
    Small,
    /// Palatino Linotype Bold 14.5, single line: panel title (game-like serif caps).
    Display,
    /// Palatino Linotype Bold 12.5, single line: section titles, the "!" of quest tokens.
    Section,
    /// Segoe UI Semibold 11, single line: sub-headers, tooltip eyebrow, pills' counts.
    Caps,
    /// Segoe Fluent Icons (Segoe MDL2 Assets on Windows 10) 13, single line: UI icons.
    Icon,
    /// Segoe UI Bold 11.5, single line: badge characters inside markers.
    Badge,
    /// Segoe UI Symbol 12.5, single line: the skull and other symbols.
    Symbol,
    /// Segoe UI 13.5, single line: chips, layer pills, tooltip lines.
    Chip,
    /// Segoe UI Semibold 15, single line: owner titles (quest, recipe, search).
    Name,
}

/// Off-screen 2D renderer: draws into a memory DIB that is then pushed to a window.
pub struct Renderer {
    /// Direct2D factory (kept alive for the lifetime of the render target).
    factory: ID2D1Factory,
    /// DirectWrite factory, used to create text formats and layouts.
    dw: IDWriteFactory,
    /// Software render target bound to the memory DC below.
    rt: ID2D1DCRenderTarget,
    /// Single reusable brush; its colour is changed before each draw call.
    brush: ID2D1SolidColorBrush,
    /// Dashed stroke style (used for dashed circles).
    dash: ID2D1StrokeStyle,
    /// Solid stroke style with round caps (glyph strokes).
    round: ID2D1StrokeStyle,
    /// Text formats, indexed by `Font`.
    fonts: Vec<IDWriteTextFormat>,
    /// Scale factor the current `fonts` were created for (0.0 = must be rebuilt).
    font_scale: f32,
    /// Base sizes (px at scale 1): title, bold, text, small.
    base: [f32; 4],
    /// Memory device context holding the DIB.
    mem: HDC,
    /// The 32-bit top-down DIB we render into (invalid when released).
    dib: HBITMAP,
    /// Bitmap that was selected in `mem` before our DIB (restored before deleting the DIB).
    old: HGDIOBJ,
    /// Pixels of the DIB (premultiplied BGRA, top-down), valid while `dib` is.
    bits: *mut std::ffi::c_void,
    /// Current image width in pixels.
    pub w: i32,
    /// Current image height in pixels.
    pub h: i32,
}

/// Converts an `Rgba` tuple into a Direct2D colour.
fn col(c: Rgba) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.0, g: c.1, b: c.2, a: c.3 }
}

/// Converts a Rust string into UTF-16 (no terminating NUL), as DirectWrite expects.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

impl Renderer {
    /// Creates the factories, the software DC render target, the brush, the dashed stroke style,
    /// the memory DC and the default fonts. The DIB itself is created lazily by `begin`.
    pub fn new() -> Result<Renderer> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            // BGRA with premultiplied alpha: the format UpdateLayeredWindow expects.
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let rt = factory.CreateDCRenderTarget(&props)?;
            // Grayscale antialiasing: ClearType does not work on a transparent background.
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let brush = rt.CreateSolidColorBrush(&col((1.0, 1.0, 1.0, 1.0)), None)?;
            let dash = factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_DASH,
                    dashOffset: 0.0,
                },
                None,
            )?;
            let round = factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    dashStyle: windows::Win32::Graphics::Direct2D::D2D1_DASH_STYLE_SOLID,
                    dashOffset: 0.0,
                },
                None,
            )?;
            let mem = CreateCompatibleDC(HDC::default());
            let mut r = Renderer {
                factory,
                dw,
                rt,
                brush,
                dash,
                round,
                fonts: vec![],
                font_scale: 0.0,
                base: [18.0, 18.0, 16.5, 14.5],
                mem,
                dib: HBITMAP::default(),
                old: HGDIOBJ::default(),
                bits: std::ptr::null_mut(),
                w: 0,
                h: 0,
            };
            r.set_font_scale(1.0)?;
            Ok(r)
        }
    }

    /// Recreates the fonts for a scale factor (screen height / 1440). No-op if the scale is unchanged.
    pub fn set_font_scale(&mut self, s: f32) -> Result<()> {
        if (s - self.font_scale).abs() < 0.01 {
            return Ok(());
        }
        self.font_scale = s;
        // Icon font: Segoe Fluent Icons (Windows 11), else Segoe MDL2 Assets (Windows 10). Both ship with Windows.
        let icon_family = unsafe {
            let mut coll = None;
            let mut found = windows::Win32::Foundation::BOOL(0);
            let mut idx = 0u32;
            let _ = self.dw.GetSystemFontCollection(&mut coll, false);
            match coll {
                Some(c) if c.FindFamilyName(w!("Segoe Fluent Icons"), &mut idx, &mut found).is_ok() && found.as_bool() => w!("Segoe Fluent Icons"),
                _ => w!("Segoe MDL2 Assets"),
            }
        };
        let segoe = w!("Segoe UI");
        // Builds one text format; non-wrapping formats are trimmed per character, with a "…" sign.
        let mk = |family: PCWSTR, weight: DWRITE_FONT_WEIGHT, size: f32, wrap: bool| -> Result<IDWriteTextFormat> {
            unsafe {
                let f = self.dw.CreateTextFormat(
                    family,
                    None,
                    weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size * s,
                    w!("fr-fr"),
                )?;
                f.SetWordWrapping(if wrap { DWRITE_WORD_WRAPPING_WRAP } else { DWRITE_WORD_WRAPPING_NO_WRAP })?;
                if !wrap {
                    let trim = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
                    let sign = self.dw.CreateEllipsisTrimmingSign(&f).ok();
                    f.SetTrimming(&trim, sign.as_ref())?;
                }
                Ok(f)
            }
        };
        let b = self.base;
        let serif = w!("Palatino Linotype");
        // Same order as the `Font` enum.
        self.fonts = vec![
            mk(segoe, DWRITE_FONT_WEIGHT_BOLD, b[0], false)?,
            mk(segoe, DWRITE_FONT_WEIGHT_SEMI_BOLD, b[1], true)?,
            mk(segoe, DWRITE_FONT_WEIGHT_NORMAL, b[2], true)?,
            mk(segoe, DWRITE_FONT_WEIGHT_NORMAL, b[3], true)?,
            mk(serif, DWRITE_FONT_WEIGHT_BOLD, 14.5, false)?,
            mk(serif, DWRITE_FONT_WEIGHT_BOLD, 12.5, false)?,
            mk(segoe, DWRITE_FONT_WEIGHT_SEMI_BOLD, 11.0, false)?,
            mk(icon_family, DWRITE_FONT_WEIGHT_NORMAL, 13.0, false)?,
            mk(segoe, DWRITE_FONT_WEIGHT_BOLD, 11.5, false)?,
            mk(w!("Segoe UI Symbol"), DWRITE_FONT_WEIGHT_NORMAL, 12.5, false)?,
            mk(segoe, DWRITE_FONT_WEIGHT_NORMAL, 13.5, false)?,
            mk(segoe, DWRITE_FONT_WEIGHT_SEMI_BOLD, 15.0, false)?,
        ];
        Ok(())
    }

    /// Changes the base font sizes (application-style UI: 14 px text). The fonts are rebuilt on the
    /// next `set_font_scale` call.
    pub fn set_base_sizes(&mut self, b: [f32; 4]) {
        self.base = b;
        self.font_scale = 0.0;
    }

    /// Finishes the image and copies it into a regular window device context (WM_PAINT).
    pub fn present_dc(&mut self, hdc: HDC) {
        unsafe {
            let _ = self.rt.EndDraw(None, None);
            let _ = windows::Win32::Graphics::Gdi::BitBlt(hdc, 0, 0, self.w, self.h, self.mem, 0, 0, windows::Win32::Graphics::Gdi::SRCCOPY);
        }
    }

    /// Returns the text format for a `Font` style.
    fn font(&self, f: Font) -> &IDWriteTextFormat {
        &self.fonts[f as usize]
    }

    /// Frees the render image (several MB at full screen) when nothing is displayed.
    pub fn release(&mut self) {
        if self.dib.is_invalid() {
            return;
        }
        unsafe {
            SelectObject(self.mem, self.old);
            let _ = DeleteObject(self.dib);
        }
        self.dib = HBITMAP::default();
        self.w = 0;
        self.h = 0;
    }

    /// Makes sure a DIB of exactly w×h exists (re-creating it if the size changed).
    /// Returns false if the DIB could not be created.
    fn ensure_size(&mut self, w: i32, h: i32) -> bool {
        if w == self.w && h == self.h && !self.dib.is_invalid() {
            return true;
        }
        unsafe {
            // Drop the previous DIB, if any.
            if !self.dib.is_invalid() {
                SelectObject(self.mem, self.old);
                let _ = DeleteObject(self.dib);
                self.dib = HBITMAP::default();
            }
            // 32 bpp, negative height = top-down rows (row 0 is the top of the image).
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
            let mut bits = std::ptr::null_mut();
            match CreateDIBSection(self.mem, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(dib) => {
                    self.old = SelectObject(self.mem, dib);
                    self.dib = dib;
                    self.bits = bits;
                    self.w = w;
                    self.h = h;
                    true
                }
                Err(_) => false,
            }
        }
    }

    /// Starts a w×h image, fully transparent. Returns false if nothing can be drawn.
    pub fn begin(&mut self, w: i32, h: i32) -> bool {
        if w <= 0 || h <= 0 || !self.ensure_size(w, h) {
            return false;
        }
        unsafe {
            if self.rt.BindDC(self.mem, &RECT { left: 0, top: 0, right: w, bottom: h }).is_err() {
                return false;
            }
            self.rt.BeginDraw();
            self.rt.Clear(Some(&col((0.0, 0.0, 0.0, 0.0))));
        }
        true
    }

    /// Finishes the image and pushes it onto the layered window at screen position (x, y).
    pub fn present(&mut self, hwnd: HWND, x: i32, y: i32) {
        unsafe {
            let _ = self.rt.EndDraw(None, None);
            let screen = GetDC(HWND::default());
            // Use the DIB's own per-pixel (premultiplied) alpha, no extra global transparency.
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                hwnd,
                screen,
                Some(&POINT { x, y }),
                Some(&SIZE { cx: self.w, cy: self.h }),
                self.mem,
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            ReleaseDC(HWND::default(), screen);
        }
    }

    /// Copy of the last finished image (premultiplied BGRA, top-down), for the offline preview test.
    #[cfg(test)]
    pub fn bgra(&self) -> Vec<u8> {
        if self.bits.is_null() {
            return vec![];
        }
        unsafe { std::slice::from_raw_parts(self.bits as *const u8, (self.w * self.h * 4) as usize).to_vec() }
    }

    /// Sets the shared brush colour.
    fn set(&self, c: Rgba) {
        unsafe { self.brush.SetColor(&col(c)) }
    }

    /// Fills a circle of radius `r` centred on (x, y).
    pub fn fill_circle(&self, x: f32, y: f32, r: f32, c: Rgba) {
        self.set(c);
        unsafe {
            self.rt.FillEllipse(&D2D1_ELLIPSE { point: D2D_POINT_2F { x, y }, radiusX: r, radiusY: r }, &self.brush);
        }
    }

    /// Draws a circle outline of radius `r` centred on (x, y), solid or dashed.
    pub fn stroke_circle(&self, x: f32, y: f32, r: f32, width: f32, c: Rgba, dashed: bool) {
        self.set(c);
        unsafe {
            let e = D2D1_ELLIPSE { point: D2D_POINT_2F { x, y }, radiusX: r, radiusY: r };
            if dashed {
                self.rt.DrawEllipse(&e, &self.brush, width, &self.dash);
            } else {
                self.rt.DrawEllipse(&e, &self.brush, width, None);
            }
        }
    }

    /// Draws a straight line from (x0, y0) to (x1, y1).
    pub fn line(&self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, c: Rgba) {
        self.set(c);
        unsafe {
            self.rt.DrawLine(D2D_POINT_2F { x: x0, y: y0 }, D2D_POINT_2F { x: x1, y: y1 }, &self.brush, width, None);
        }
    }

    /// Fills a rounded rectangle (left, top, right, bottom, corner radius).
    pub fn fill_rrect(&self, l: f32, t: f32, r: f32, b: f32, radius: f32, c: Rgba) {
        self.set(c);
        unsafe {
            self.rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: l, top: t, right: r, bottom: b }, radiusX: radius, radiusY: radius },
                &self.brush,
            );
        }
    }

    /// Draws a rounded rectangle outline (left, top, right, bottom, corner radius, line width).
    pub fn stroke_rrect(&self, l: f32, t: f32, r: f32, b: f32, radius: f32, width: f32, c: Rgba) {
        self.set(c);
        unsafe {
            self.rt.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: l, top: t, right: r, bottom: b }, radiusX: radius, radiusY: radius },
                &self.brush,
                width,
                None,
            );
        }
    }

    /// Measures a text (width, height) within a maximum width.
    pub fn measure(&self, s: &str, f: Font, max_w: f32) -> (f32, f32) {
        unsafe {
            let ws = wide(s);
            if let Ok(layout) = self.dw.CreateTextLayout(&ws, self.font(f), max_w, 10_000.0) {
                let mut m = DWRITE_TEXT_METRICS::default();
                if layout.GetMetrics(&mut m).is_ok() {
                    return (m.widthIncludingTrailingWhitespace.min(max_w), m.height);
                }
            }
            (0.0, 0.0)
        }
    }

    /// Draws a text in a box; returns the height it occupies.
    pub fn text(&self, s: &str, f: Font, x: f32, y: f32, max_w: f32, c: Rgba) -> f32 {
        let (_, h) = self.measure(s, f, max_w);
        self.set(c);
        unsafe {
            let ws = wide(s);
            let rect = D2D_RECT_F { left: x, top: y, right: x + max_w, bottom: y + h + 2.0 };
            self.rt.DrawText(&ws, self.font(f), &rect, &self.brush, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
        }
        h
    }

    /// Text layout with letter spacing (`tracking` px after each character), for game-like caps titles.
    fn spaced_layout(&self, s: &str, f: Font, max_w: f32, tracking: f32) -> Option<IDWriteTextLayout> {
        unsafe {
            let ws = wide(s);
            let layout = self.dw.CreateTextLayout(&ws, self.font(f), max_w.max(1.0), 10_000.0).ok()?;
            if tracking != 0.0 {
                if let Ok(l1) = layout.cast::<IDWriteTextLayout1>() {
                    let _ = l1.SetCharacterSpacing(0.0, tracking, 0.0, DWRITE_TEXT_RANGE { startPosition: 0, length: ws.len() as u32 });
                }
            }
            Some(layout)
        }
    }

    /// Measures a letter-spaced text (width, height).
    pub fn measure_spaced(&self, s: &str, f: Font, max_w: f32, tracking: f32) -> (f32, f32) {
        let Some(l) = self.spaced_layout(s, f, max_w, tracking) else { return (0.0, 0.0) };
        let mut m = DWRITE_TEXT_METRICS::default();
        unsafe {
            if l.GetMetrics(&mut m).is_ok() {
                return (m.widthIncludingTrailingWhitespace.min(max_w), m.height);
            }
        }
        (0.0, 0.0)
    }

    /// Draws a letter-spaced text (single line, clipped to `max_w`); returns its width.
    #[allow(clippy::too_many_arguments)]
    pub fn text_spaced(&self, s: &str, f: Font, x: f32, y: f32, max_w: f32, tracking: f32, c: Rgba) -> f32 {
        let Some(l) = self.spaced_layout(s, f, max_w, tracking) else { return 0.0 };
        self.set(c);
        let mut m = DWRITE_TEXT_METRICS::default();
        unsafe {
            let _ = l.GetMetrics(&mut m);
            self.rt.DrawTextLayout(D2D_POINT_2F { x, y }, &l, &self.brush, D2D1_DRAW_TEXT_OPTIONS_CLIP);
        }
        m.widthIncludingTrailingWhitespace.min(max_w)
    }

    /// Draws a single-line text centred on (cx, cy) (badges, glyphs, icons).
    pub fn text_center(&self, s: &str, f: Font, cx: f32, cy: f32, c: Rgba) {
        let (w, h) = self.measure(s, f, 1000.0);
        self.set(c);
        unsafe {
            let ws = wide(s);
            let rect = D2D_RECT_F { left: cx - w / 2.0, top: cy - h / 2.0, right: cx + w / 2.0 + 2.0, bottom: cy + h / 2.0 + 2.0 };
            self.rt.DrawText(&ws, self.font(f), &rect, &self.brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        }
    }

    /// Runs `f` with the drawing rotated by 45° around (x, y) (used for diamonds).
    fn rotated45(&self, x: f32, y: f32, f: impl FnOnce()) {
        let (c, s) = (std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
        // Rotation about (x, y): translate to the origin, rotate, translate back.
        let m = Matrix3x2 { M11: c, M12: s, M21: -s, M22: c, M31: x - c * x + s * y, M32: y - s * x - c * y };
        unsafe {
            self.rt.SetTransform(&m);
        }
        f();
        unsafe {
            self.rt.SetTransform(&Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: 0.0, M32: 0.0 });
        }
    }

    /// Fills a diamond of half-diagonal `r` centred on (x, y) (a square rotated by 45°; `round` = corner radius).
    pub fn fill_diamond(&self, x: f32, y: f32, r: f32, round: f32, c: Rgba) {
        let h = r * std::f32::consts::FRAC_1_SQRT_2;
        self.rotated45(x, y, || self.fill_rrect(x - h, y - h, x + h, y + h, round, c));
    }

    /// Draws a diamond outline of half-diagonal `r` centred on (x, y).
    pub fn stroke_diamond(&self, x: f32, y: f32, r: f32, width: f32, c: Rgba) {
        let h = r * std::f32::consts::FRAC_1_SQRT_2;
        self.rotated45(x, y, || self.stroke_rrect(x - h, y - h, x + h, y + h, 0.5, width, c));
    }

    /// Restricts drawing to a rectangle until `pop_clip` (scrolling panel body).
    pub fn push_clip(&self, l: f32, t: f32, r: f32, b: f32) {
        unsafe {
            self.rt.PushAxisAlignedClip(&D2D_RECT_F { left: l, top: t, right: r, bottom: b }, windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED);
        }
    }

    /// Ends the clip started by `push_clip`.
    pub fn pop_clip(&self) {
        unsafe {
            self.rt.PopAxisAlignedClip();
        }
    }

    /// Draws a straight line with round caps (glyph strokes: the trap cross, chevrons, key teeth).
    pub fn line_round(&self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, c: Rgba) {
        self.set(c);
        unsafe {
            self.rt.DrawLine(D2D_POINT_2F { x: x0, y: y0 }, D2D_POINT_2F { x: x1, y: y1 }, &self.brush, width, &self.round);
        }
    }
}

impl Drop for Renderer {
    /// Releases the GDI objects (DIB and memory DC); COM objects release themselves.
    fn drop(&mut self) {
        unsafe {
            if !self.dib.is_invalid() {
                SelectObject(self.mem, self.old);
                let _ = DeleteObject(self.dib);
            }
            let _ = DeleteDC(self.mem);
        }
        let _ = &self.factory;
        let _ = PCWSTR::null();
    }
}
