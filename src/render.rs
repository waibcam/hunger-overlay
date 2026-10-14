//! Rendu Direct2D/DirectWrite dans une DIB 32 bits prémultipliée, poussée sur la fenêtre superposée
//! (UpdateLayeredWindow, alpha par pixel). Aucun accès au rendu du jeu.

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
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL, DWRITE_TEXT_METRICS, DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_WORD_WRAPPING_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject, AC_SRC_ALPHA,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

pub type Rgba = (f32, f32, f32, f32);

#[derive(Clone, Copy)]
pub enum Font {
    Title,
    Bold,
    Text,
    Small,
}

pub struct Renderer {
    factory: ID2D1Factory,
    dw: IDWriteFactory,
    rt: ID2D1DCRenderTarget,
    brush: ID2D1SolidColorBrush,
    dash: ID2D1StrokeStyle,
    fonts: Vec<IDWriteTextFormat>,
    font_scale: f32,
    /// Tailles de base (px à l'échelle 1) : titre, gras, texte, petit.
    base: [f32; 4],
    mem: HDC,
    dib: HBITMAP,
    old: HGDIOBJ,
    pub w: i32,
    pub h: i32,
}

fn col(c: Rgba) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.0, g: c.1, b: c.2, a: c.3 }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

impl Renderer {
    pub fn new() -> Result<Renderer> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let rt = factory.CreateDCRenderTarget(&props)?;
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
            let mem = CreateCompatibleDC(HDC::default());
            let mut r = Renderer {
                factory,
                dw,
                rt,
                brush,
                dash,
                fonts: vec![],
                font_scale: 0.0,
                base: [18.0, 18.0, 16.5, 14.5],
                mem,
                dib: HBITMAP::default(),
                old: HGDIOBJ::default(),
                w: 0,
                h: 0,
            };
            r.set_font_scale(1.0)?;
            Ok(r)
        }
    }

    /// Recrée les polices pour un facteur d'échelle (hauteur de l'écran / 1440).
    pub fn set_font_scale(&mut self, s: f32) -> Result<()> {
        if (s - self.font_scale).abs() < 0.01 {
            return Ok(());
        }
        self.font_scale = s;
        let mk = |weight: DWRITE_FONT_WEIGHT, size: f32, wrap: bool| -> Result<IDWriteTextFormat> {
            unsafe {
                let f = self.dw.CreateTextFormat(
                    w!("Segoe UI"),
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
                    f.SetTrimming(&trim, None)?;
                }
                Ok(f)
            }
        };
        let b = self.base;
        self.fonts = vec![
            mk(DWRITE_FONT_WEIGHT_BOLD, b[0], false)?,
            mk(DWRITE_FONT_WEIGHT_SEMI_BOLD, b[1], true)?,
            mk(DWRITE_FONT_WEIGHT_NORMAL, b[2], true)?,
            mk(DWRITE_FONT_WEIGHT_NORMAL, b[3], true)?,
        ];
        Ok(())
    }

    /// Change les tailles de base des polices (interface de type application : texte 14 px).
    pub fn set_base_sizes(&mut self, b: [f32; 4]) {
        self.base = b;
        self.font_scale = 0.0;
    }

    /// Termine l'image et la copie dans un contexte de fenêtre classique (WM_PAINT).
    pub fn present_dc(&mut self, hdc: HDC) {
        unsafe {
            let _ = self.rt.EndDraw(None, None);
            let _ = windows::Win32::Graphics::Gdi::BitBlt(hdc, 0, 0, self.w, self.h, self.mem, 0, 0, windows::Win32::Graphics::Gdi::SRCCOPY);
        }
    }

    fn font(&self, f: Font) -> &IDWriteTextFormat {
        &self.fonts[f as usize]
    }

    fn ensure_size(&mut self, w: i32, h: i32) -> bool {
        if w == self.w && h == self.h && !self.dib.is_invalid() {
            return true;
        }
        unsafe {
            if !self.dib.is_invalid() {
                SelectObject(self.mem, self.old);
                let _ = DeleteObject(self.dib);
                self.dib = HBITMAP::default();
            }
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
                    self.w = w;
                    self.h = h;
                    true
                }
                Err(_) => false,
            }
        }
    }

    /// Commence une image de taille w×h, entièrement transparente.
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

    /// Termine l'image et la pousse sur la fenêtre à la position écran (x, y).
    pub fn present(&mut self, hwnd: HWND, x: i32, y: i32) {
        unsafe {
            let _ = self.rt.EndDraw(None, None);
            let screen = GetDC(HWND::default());
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

    fn set(&self, c: Rgba) {
        unsafe { self.brush.SetColor(&col(c)) }
    }

    pub fn fill_circle(&self, x: f32, y: f32, r: f32, c: Rgba) {
        self.set(c);
        unsafe {
            self.rt.FillEllipse(&D2D1_ELLIPSE { point: D2D_POINT_2F { x, y }, radiusX: r, radiusY: r }, &self.brush);
        }
    }

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

    pub fn line(&self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, c: Rgba) {
        self.set(c);
        unsafe {
            self.rt.DrawLine(D2D_POINT_2F { x: x0, y: y0 }, D2D_POINT_2F { x: x1, y: y1 }, &self.brush, width, None);
        }
    }

    pub fn diamond(&self, x: f32, y: f32, r: f32, width: f32, c: Rgba) {
        let p = [(x, y - r), (x + r, y), (x, y + r), (x - r, y), (x, y - r)];
        for i in 0..4 {
            self.line(p[i].0, p[i].1, p[i + 1].0, p[i + 1].1, width, c);
        }
    }

    pub fn fill_rrect(&self, l: f32, t: f32, r: f32, b: f32, radius: f32, c: Rgba) {
        self.set(c);
        unsafe {
            self.rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: l, top: t, right: r, bottom: b }, radiusX: radius, radiusY: radius },
                &self.brush,
            );
        }
    }

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

    /// Mesure un texte (largeur, hauteur) dans une largeur maximale.
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

    /// Dessine un texte dans une boîte ; renvoie la hauteur occupée.
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

}

impl Drop for Renderer {
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
