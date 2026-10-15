//! Settings window, fully custom-drawn (Direct2D): sections, toggle switches, sliders, selectors.
//! Light / dark theme (automatic = Windows' own), standard font (Segoe UI 14 px at 100 %).
//! Only the key field is a Windows control (EDIT), coloured according to the theme.

use crate::i18n::Strings;
use crate::render::{Font, Renderer, Rgba};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, EndPaint, InvalidateRect, SetBkColor, SetTextColor, HBRUSH, HDC,
    HFONT, HGDIOBJ, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::Controls::EM_SETPASSWORDCHAR;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyNameTextW, MapVirtualKeyW, ReleaseCapture, SetCapture, TrackMouseEvent, MAPVK_VK_TO_VSC, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowTextLengthW, GetWindowTextW,
    LoadCursorW, RegisterClassW, SendMessageW, SetCursor, SetForegroundWindow, SetWindowPos, ShowWindow, CW_USEDEFAULT,
    ES_AUTOHSCROLL, ES_PASSWORD, HMENU, IDC_ARROW, IDC_HAND, SWP_NOMOVE, SWP_NOZORDER, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_CLOSE, WM_COMMAND, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_SETCURSOR, WM_SETFONT,
    WM_SETTINGCHANGE, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};

// Actions passed on to the app (everything else is handled here).
/// Control id of the key EDIT field.
pub const ID_KEY_EDIT: i32 = 201;
/// Show / hide the key.
pub const ID_KEY_SHOW: i32 = 202;
/// Start capturing a new map key.
pub const ID_MAPKEY: i32 = 203;
/// Test the key against the website.
pub const ID_TEST: i32 = 207;
/// Save and close.
pub const ID_SAVE: i32 = 208;
/// Close without saving.
pub const ID_CANCEL: i32 = 209;
/// The minimum chance changed.
pub const ID_CHANCE: i32 = 211;
/// Apply without closing.
pub const ID_APPLY: i32 = 212;

/// Initial values the app passes when opening the settings window.
pub struct Init<'a> {
    /// Personal website key (shown masked).
    pub key: &'a str,
    /// Virtual-key code of the key that opens the in-game map.
    pub vk: u32,
    /// List objectives that are already done.
    pub show_done: bool,
    /// Overlay visible in screenshots and OBS.
    pub capture: bool,
    /// Start with Windows.
    pub autostart: bool,
    /// Chosen language code ("" = automatic).
    pub locale: &'a str,
    /// Minimum loot chance (percent, 0..=50).
    pub min_chance: u32,
    /// Automatic updates.
    pub auto_update: bool,
    /// "auto", "light" or "dark" ("" is treated as "auto").
    pub theme: &'a str,
    /// Panel background opacity (percent, clamped to 20..=100).
    pub panel_opacity: u32,
    /// Label of the "Auto (EN)" language entry, computed by the app.
    pub auto_lang: String,
}

/// Values edited in the window, read back by the app on Apply / Save.
#[derive(Clone)]
pub struct Values {
    /// Personal website key.
    pub key: String,
    /// Virtual-key code of the map key.
    pub vk: u32,
    /// List objectives that are already done.
    pub show_done: bool,
    /// Overlay visible in screenshots and OBS.
    pub capture_visible: bool,
    /// Start with Windows.
    pub autostart: bool,
    /// Language code ("" = automatic).
    pub locale: String,
    /// Minimum loot chance (percent, 0..=50).
    pub min_chance: u32,
    /// Automatic updates.
    pub auto_update: bool,
    /// "auto", "light" or "dark".
    pub theme: String,
    /// Panel background opacity (percent, 20..=100).
    pub panel_opacity: u32,
}

/// Interactive widgets of the window (what a clickable zone refers to).
#[derive(Clone, Copy, PartialEq, Debug)]
enum W {
    /// "Show / Hide" key button.
    KeyShow,
    /// "Test key" button.
    Test,
    /// Map key button (click, then press a key).
    MapKey,
    /// Minimum chance slider.
    Slider,
    /// Panel opacity slider.
    Opacity,
    /// "Show done objectives" switch.
    ShowDone,
    /// Language field: opens / closes the list.
    LangOpen,
    /// Entry of the language list (0 = Auto, then `LANGS` in order).
    LangItem(u8),
    /// Theme segment: 0 = auto, 1 = light, 2 = dark.
    Theme(u8),
    /// "Visible in captures" switch.
    Capture,
    /// "Start with Windows" switch.
    Autostart,
    /// "Automatic updates" switch.
    AutoUpdate,
    /// Cancel button.
    Cancel,
    /// Apply button.
    Apply,
    /// Save button.
    Save,
}

/// Clickable zone: rectangle (left, top, right, bottom) and the widget it belongs to.
struct Hit {
    r: (f32, f32, f32, f32),
    w: W,
}

/// Colour palette of the custom-drawn windows.
pub struct Theme {
    /// Window background.
    pub bg: Rgba,
    /// Section card background.
    pub card: Rgba,
    /// Borders and separators.
    pub border: Rgba,
    /// Main text.
    pub text: Rgba,
    /// Secondary text (help, hints).
    pub sub: Rgba,
    /// Accent colour (primary button, active switches, sliders).
    pub accent: Rgba,
    /// Control background (buttons, fields).
    pub ctrl: Rgba,
    /// Hovered control background.
    pub hover: Rgba,
    /// Text / knob drawn on top of the accent colour.
    pub on_accent: Rgba,
    /// Success message.
    pub ok: Rgba,
    /// Error message.
    pub err: Rgba,
}

/// Dark theme palette.
pub const DARK: Theme = Theme {
    bg: (0.125, 0.125, 0.125, 1.0),
    card: (0.17, 0.17, 0.17, 1.0),
    border: (0.26, 0.26, 0.26, 1.0),
    text: (0.95, 0.95, 0.95, 1.0),
    sub: (0.65, 0.65, 0.65, 1.0),
    accent: (0.83, 0.64, 0.33, 1.0),
    ctrl: (0.22, 0.22, 0.22, 1.0),
    hover: (0.28, 0.28, 0.28, 1.0),
    on_accent: (0.1, 0.08, 0.05, 1.0),
    ok: (0.45, 0.82, 0.5, 1.0),
    err: (1.0, 0.55, 0.5, 1.0),
};

/// Light theme palette.
pub const LIGHT: Theme = Theme {
    bg: (0.953, 0.953, 0.953, 1.0),
    card: (1.0, 1.0, 1.0, 1.0),
    border: (0.88, 0.88, 0.88, 1.0),
    text: (0.1, 0.1, 0.1, 1.0),
    sub: (0.38, 0.38, 0.38, 1.0),
    accent: (0.6, 0.42, 0.12, 1.0),
    ctrl: (0.97, 0.97, 0.97, 1.0),
    hover: (0.92, 0.92, 0.92, 1.0),
    on_accent: (1.0, 1.0, 1.0, 1.0),
    ok: (0.1, 0.55, 0.2, 1.0),
    err: (0.75, 0.15, 0.1, 1.0),
};

/// Effective theme for the setting ("auto" = Windows' own).
pub fn theme_for(setting: &str) -> &'static Theme {
    let dark = match setting {
        "dark" => true,
        "light" => false,
        _ => windows_dark(),
    };
    if dark {
        &DARK
    } else {
        &LIGHT
    }
}

/// Is Windows using the dark theme (for apps)?
///
/// Read-only lookup of the per-user `AppsUseLightTheme` value; if it cannot be read, light is assumed.
pub fn windows_dark() -> bool {
    let mut v: u32 = 1;
    let mut n = 4u32;
    unsafe {
        let _ = RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as *mut _),
            Some(&mut n),
        );
    }
    v == 0
}

/// State of the open settings window.
pub struct Settings {
    /// The top-level window.
    pub hwnd: HWND,
    /// Native EDIT control holding the key (password-style).
    edit: HWND,
    /// GDI font given to the EDIT control.
    font: HFONT,
    /// Background brush for the EDIT control (theme colour).
    edit_brush: HBRUSH,
    /// Renderer used to paint the window.
    r: Renderer,
    /// UI strings in the current language.
    s: &'static Strings,
    /// DPI scale factor (system dpi / 96).
    k: f32,
    /// Values being edited.
    v: Values,
    /// Map key currently chosen (updated by the app after a key capture).
    pub vk: u32,
    /// Key shown as dots.
    masked: bool,
    /// Waiting for the user to press the new map key.
    capturing: bool,
    /// Status line under the key: (text, kind) with kind 1 = success, -1 = error, 0 = neutral.
    status: (String, i8),
    /// Label of the automatic language entry.
    auto_lang: String,
    /// Clickable zones from the last layout.
    hits: Vec<Hit>,
    /// Widget under the mouse, if any.
    hover: Option<W>,
    /// Slider currently being dragged (chance or opacity).
    dragging: Option<W>,
    /// Horizontal extent (x0, x1) of the chance slider track.
    slider: (f32, f32),
    /// Horizontal extent (x0, x1) of the opacity slider track.
    slider2: (f32, f32),
    /// Painted frame of the key field (left, top, right, bottom); the EDIT control is placed inside.
    edit_rect: (f32, f32, f32, f32),
    /// Total client height computed by the layout.
    height: f32,
    /// The language list is open.
    lang_open: bool,
}

/// Converts a Rust string into a NUL-terminated UTF-16 string for Win32.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// Converts an `Rgba` colour into a GDI COLORREF (0x00BBGGRR, alpha ignored).
fn rgb(c: Rgba) -> COLORREF {
    COLORREF(((c.2 * 255.0) as u32) << 16 | ((c.1 * 255.0) as u32) << 8 | (c.0 * 255.0) as u32)
}

/// Human-readable name of a virtual key in the user's keyboard layout (e.g. "M", "Tab"),
/// or its hex code if Windows has no name for it.
pub fn key_name(vk: u32) -> String {
    unsafe {
        let mut sc = MapVirtualKeyW(vk, MAPVK_VK_TO_VSC);
        // Page Up .. Delete and arrows are "extended" keys: set the flag so they are not named as keypad keys.
        if matches!(vk, 0x21..=0x2E) {
            sc |= 0x100;
        }
        let mut buf = [0u16; 64];
        let n = GetKeyNameTextW((sc as i32) << 16, &mut buf);
        if n > 0 {
            String::from_utf16_lossy(&buf[..n as usize])
        } else {
            format!("0x{vk:02X}")
        }
    }
}

/// Window procedure of the settings window. State lives in the app (reached via `crate::with_settings`).
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // Mouse position (client coordinates, signed) from lParam.
    let pos = || (((lp.0 & 0xffff) as i16) as f32, (((lp.0 >> 16) & 0xffff) as i16) as f32);
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            crate::with_settings(|st| st.paint(hdc));
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        // We paint the whole background ourselves: skip erasing to avoid flicker.
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEMOVE => {
            let (x, y) = pos();
            // Ask for WM_MOUSELEAVE so the hover highlight can be cleared.
            let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
            let _ = TrackMouseEvent(&mut tme);
            crate::with_settings(|st| st.mouse_move(x, y));
            LRESULT(0)
        }
        0x02A3 => {
            // WM_MOUSELEAVE
            crate::with_settings(|st| st.mouse_move(-1.0, -1.0));
            LRESULT(0)
        }
        WM_SETCURSOR if (lp.0 & 0xffff) as u32 == 1 => {
            // In the client area (HTCLIENT): hand cursor over a widget, arrow elsewhere.
            let over = crate::with_settings(|st| st.hover.is_some()).unwrap_or(false);
            let _ = SetCursor(LoadCursorW(None, if over { IDC_HAND } else { IDC_ARROW }).unwrap_or_default());
            LRESULT(1)
        }
        // Clicks go through the app, which may need to act on the returned action.
        WM_LBUTTONDOWN => {
            let (x, y) = pos();
            crate::on_settings_click(x, y, true);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = pos();
            crate::on_settings_click(x, y, false);
            LRESULT(0)
        }
        // Colours of the text field (WM_CTLCOLOREDIT).
        0x0133 => {
            let hdc = HDC(wp.0 as _);
            crate::with_settings(|st| {
                let t = st.theme();
                SetTextColor(hdc, rgb(t.text));
                SetBkColor(hdc, rgb(t.ctrl));
                LRESULT(st.edit_brush.0 as isize)
            })
            .unwrap_or(LRESULT(0))
        }
        WM_COMMAND => LRESULT(0),
        // Windows settings changed (possibly the light / dark theme): re-apply the theme.
        WM_SETTINGCHANGE => {
            crate::with_settings(|st| st.retheme());
            LRESULT(0)
        }
        // Closing the window = Cancel.
        WM_CLOSE => {
            crate::on_settings_command(ID_CANCEL);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

impl Settings {
    /// Creates and shows the settings window with the given initial values.
    pub fn open(s: &'static Strings, init: Init) -> Option<Settings> {
        unsafe {
            let inst = GetModuleHandleW(None).ok()?;
            let class = w!("HungerOverlaySettings2");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: inst.into(),
                lpszClassName: class,
                hIcon: crate::tray::app_icon(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            };
            // Registering again on later openings simply fails harmlessly.
            RegisterClassW(&wc);
            let k = GetDpiForSystem() as f32 / 96.0;
            let title = wide(s.settings_title);
            let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
            // Created small; resized below once the layout height is known.
            // Topmost, like the overlay: otherwise the markers drawn over the game would cover it.
            let hwnd = CreateWindowExW(windows::Win32::UI::WindowsAndMessaging::WS_EX_TOPMOST, class, PCWSTR(title.as_ptr()), style, CW_USEDEFAULT, CW_USEDEFAULT, 100, 100, None, None, inst, None).ok()?;
            // Key field: native password-style EDIT control (positioned later by `place_edit`).
            let font =CreateFontW(-((14.0 * k) as i32), 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 5, 0, w!("Segoe UI"));
            let key_w = wide(init.key);
            let edit = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("EDIT"),
                PCWSTR(key_w.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE((ES_PASSWORD | ES_AUTOHSCROLL) as u32),
                0,
                0,
                10,
                10,
                hwnd,
                HMENU(ID_KEY_EDIT as isize as _),
                inst,
                None,
            )
            .ok()?;
            SendMessageW(edit, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
            // Renderer with application-style font sizes.
            let mut r = Renderer::new().ok()?;
            r.set_base_sizes([20.0, 14.0, 14.0, 12.0]);
            let _ = r.set_font_scale(k);
            let v = Values {
                key: init.key.to_string(),
                vk: init.vk,
                show_done: init.show_done,
                capture_visible: init.capture,
                autostart: init.autostart,
                locale: init.locale.to_string(),
                min_chance: init.min_chance,
                auto_update: init.auto_update,
                theme: if init.theme.is_empty() { "auto".into() } else { init.theme.to_string() },
                panel_opacity: init.panel_opacity.clamp(20, 100),
            };
            let mut st = Settings {
                hwnd,
                edit,
                font,
                edit_brush: HBRUSH::default(),
                r,
                s,
                k,
                v,
                vk: init.vk,
                masked: true,
                capturing: false,
                status: (String::new(), 0),
                auto_lang: init.auto_lang,
                hits: vec![],
                hover: None,
                dragging: None,
                slider: (0.0, 1.0),
                slider2: (0.0, 1.0),
                edit_rect: (0.0, 0.0, 0.0, 0.0),
                height: 600.0,
                lang_open: false,
            };
            st.retheme();
            // Measuring pass (no drawing) to get the total height.
            st.layout(false);
            // Window size from the layout.
            let mut rc = RECT { left: 0, top: 0, right: (560.0 * k) as i32, bottom: st.height as i32 };
            let _ = AdjustWindowRectEx(&mut rc, style, false, windows::Win32::UI::WindowsAndMessaging::WS_EX_TOPMOST);
            let _ = SetWindowPos(hwnd, None, 0, 0, rc.right - rc.left, rc.bottom - rc.top, SWP_NOMOVE | SWP_NOZORDER);
            st.place_edit();
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            Some(st)
        }
    }

    /// True if the theme currently selected in the window resolves to dark.
    fn is_dark(&self) -> bool {
        match self.v.theme.as_str() {
            "dark" => true,
            "light" => false,
            _ => windows_dark(),
        }
    }

    /// Palette for the theme currently selected in the window.
    fn theme(&self) -> &'static Theme {
        if self.is_dark() {
            &DARK
        } else {
            &LIGHT
        }
    }

    /// Applies the theme: title bar (dark or not), text-field brush, repaint.
    fn retheme(&mut self) {
        unsafe {
            let dark: i32 = self.is_dark() as i32;
            // DWMWA_USE_IMMERSIVE_DARK_MODE = 20
            let _ = DwmSetWindowAttribute(self.hwnd, DWMWINDOWATTRIBUTE(20), &dark as *const i32 as *const _, 4);
            if !self.edit_brush.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(self.edit_brush.0));
            }
            self.edit_brush = CreateSolidBrush(rgb(self.theme().ctrl));
            let _ = InvalidateRect(self.hwnd, None, false);
            let _ = InvalidateRect(self.edit, None, true);
        }
    }

    /// Moves the native EDIT control inside the painted key-field frame, vertically centred.
    fn place_edit(&self) {
        let (l, t, r, b) = self.edit_rect;
        let k = self.k;
        let h = (20.0 * k).round();
        unsafe {
            let _ = SetWindowPos(self.edit, None, (l + 10.0 * k) as i32, (t + (b - t - h) / 2.0) as i32, (r - l - 20.0 * k) as i32, h as i32, SWP_NOZORDER);
        }
    }

    /// Requests a repaint of the whole window.
    fn redraw(&self) {
        unsafe {
            let _ = InvalidateRect(self.hwnd, None, false);
        }
    }

    /// Paints the window (layout with drawing enabled) and copies it to the window DC.
    fn paint(&mut self, hdc: HDC) {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        if self.r.begin(rc.right, rc.bottom) {
            self.layout(true);
            self.r.present_dc(hdc);
        }
    }

    /// Layout (and drawing if `draw`). Computes the clickable zones and the total height.
    ///
    /// Sections from top to bottom: header, Account (key), Map (map key, minimum chance, done
    /// objectives), Display (language, theme, panel opacity, capture visibility), Application
    /// (autostart, updates), then the help text and the Cancel / Apply / Save buttons.
    fn layout(&mut self, draw: bool) {
        let k = self.k;
        let t = self.theme();
        let s = self.s;
        let w = 560.0 * k;
        let pad = 20.0 * k;
        let r = &self.r;
        let mut hits: Vec<Hit> = vec![];
        let hover = self.hover;
        if draw {
            r.fill_rrect(0.0, 0.0, w + 4.0, self.height + 4.0, 0.0, t.bg);
        }
        let mut y = 18.0 * k;
        // Header: title (last part of the window title) and app version on the right.
        if draw {
            r.text(s.settings_title.split(" — ").last().unwrap_or(s.settings_title), Font::Title, pad, y, w - 2.0 * pad, t.text);
            let ver = format!("v{}", env!("CARGO_PKG_VERSION"));
            let (vw, _) = r.measure(&ver, Font::Small, 200.0);
            r.text(&ver, Font::Small, w - pad - vw, y + 6.0 * k, vw + 2.0, t.sub);
        }
        y += 40.0 * k;

        // Small drawing building blocks.
        // Push button sized to its label; `primary` = filled with the accent colour. Returns its width.
        let btn =|hits: &mut Vec<Hit>, x: f32, y: f32, label: &str, id: W, primary: bool| -> f32 {
            let (tw, th) = r.measure(label, Font::Text, 400.0);
            let (bw, bh) = (tw + 28.0 * k, 32.0 * k);
            if draw {
                let hv = hover == Some(id);
                let (bg, fg) = if primary { (t.accent, t.on_accent) } else { (if hv { t.hover } else { t.ctrl }, t.text) };
                let bg = if primary && hv { (bg.0 * 0.92, bg.1 * 0.92, bg.2 * 0.92, 1.0) } else { bg };
                r.fill_rrect(x, y, x + bw, y + bh, 4.0 * k, bg);
                if !primary {
                    r.stroke_rrect(x + 0.5, y + 0.5, x + bw - 0.5, y + bh - 0.5, 4.0 * k, 1.0, t.border);
                }
                r.text(label, Font::Text, x + 14.0 * k, y + (bh - th) / 2.0, tw + 2.0, fg);
            }
            hits.push(Hit { r: (x, y, x + bw, y + bh), w: id });
            bw
        };
        // On / off switch (pill with a knob); the clickable zone is slightly larger than the drawing.
        let toggle = |hits: &mut Vec<Hit>, x: f32, y: f32, on: bool, id: W| {
            let (tw, th) = (40.0 * k, 20.0 * k);
            if draw {
                let hv = hover == Some(id);
                if on {
                    r.fill_rrect(x, y, x + tw, y + th, th / 2.0, t.accent);
                    r.fill_circle(x + tw - th / 2.0, y + th / 2.0, 6.0 * k, t.on_accent);
                } else {
                    r.fill_rrect(x, y, x + tw, y + th, th / 2.0, if hv { t.hover } else { t.ctrl });
                    r.stroke_rrect(x + 0.5, y + 0.5, x + tw - 0.5, y + th - 0.5, th / 2.0, 1.0, t.sub);
                    r.fill_circle(x + th / 2.0, y + th / 2.0, 5.0 * k, t.sub);
                }
            }
            hits.push(Hit { r: (x - 4.0, y - 4.0, x + tw + 4.0, y + th + 4.0), w: id });
        };
        // Section heading; advances `y`.
        let section = |y: &mut f32, title: &str| {
            if draw {
                r.text(title, Font::Bold, pad, *y, w - 2.0 * pad, t.text);
            }
            *y += 26.0 * k;
        };
        // Rounded card behind a section's rows, from y0 to y1.
        let card = |y0: f32, y1: f32| {
            if draw {
                r.fill_rrect(pad, y0, w - pad, y1, 6.0 * k, t.card);
                r.stroke_rrect(pad + 0.5, y0 + 0.5, w - pad - 0.5, y1 - 0.5, 6.0 * k, 1.0, t.border);
            }
        };
        // Row label (and optional help line below), leaving `right_w` free for the control on the right.
        let row_label = |y: f32, label: &str, help: Option<&str>, right_w: f32| {
            let lw = w - 2.0 * pad - 40.0 * k - right_w;
            let (_, lh) = r.measure(label, Font::Text, lw);
            if draw {
                r.text(label, Font::Text, pad + 16.0 * k, y, lw, t.text);
                if let Some(hp) = help {
                    r.text(hp, Font::Small, pad + 16.0 * k, y + lh + 2.0 * k, lw, t.sub);
                }
            }
        };
        let inner_r = w - pad - 16.0 * k; // right edge of the content inside a card
        let row_gap = 14.0 * k;

        // ---- Account ----
        // Card height is fixed in advance: padding, key title row, field row, gap, status line, padding.
        section(&mut y, s.sec_account);
        let c0 = y;
        let acc_h =16.0 * k + 26.0 * k + 32.0 * k + 8.0 * k + 18.0 * k + 12.0 * k;
        card(c0, c0 + acc_h);
        y += 16.0 * k;
        // "Key" title with its help text on the same line.
        if draw {
            let (kw, _) = r.measure(s.key_title, Font::Text, 300.0 * k);
            r.text(s.key_title, Font::Text, pad + 16.0 * k, y, kw + 2.0, t.text);
            r.text(s.key_help, Font::Small, pad + 16.0 * k + kw + 10.0 * k, y + 2.0 * k, w - 2.0 * pad - kw - 42.0 * k, t.sub);
        }
        y += 26.0 * k;
        // Key field frame (the EDIT control goes inside), then the "Show/Hide" and "Test" buttons.
        let tw2 = r.measure(s.test_key, Font::Text, 400.0).0 + 28.0 * k;
        let show_label = if self.masked { s.key_show } else { s.key_hide };
        let sw2 = r.measure(show_label, Font::Text, 400.0).0 + 28.0 * k;
        let ex1 = inner_r - tw2 - sw2 - 16.0 * k;
        self.edit_rect = (pad + 16.0 * k, y, ex1, y + 32.0 * k);
        if draw {
            r.fill_rrect(pad + 16.0 * k, y, ex1, y + 32.0 * k, 4.0 * k, t.ctrl);
            r.stroke_rrect(pad + 16.0 * k + 0.5, y + 0.5, ex1 - 0.5, y + 32.0 * k - 0.5, 4.0 * k, 1.0, t.border);
            r.line(pad + 20.0 * k, y + 32.0 * k - 1.0, ex1 - 4.0 * k, y + 32.0 * k - 1.0, 2.0 * k, t.accent);
        }
        btn(&mut hits, ex1 + 8.0 * k, y, show_label, W::KeyShow, false);
        btn(&mut hits, inner_r - tw2, y, s.test_key, W::Test, false);
        y += 32.0 * k + 8.0 * k;
        // Status line (result of the key test), coloured by kind.
        if draw && !self.status.0.is_empty() {
            let c = match self.status.1 {
                1 => t.ok,
                -1 => t.err,
                _ => t.sub,
            };
            r.text(&self.status.0, Font::Small, pad + 16.0 * k, y, w - 2.0 * pad - 32.0 * k, c);
        }
        y = c0 + acc_h + 18.0 * k;

        // ---- Map ----
        // Rows: map key (label + help, button), minimum chance (label, value, slider), show done (switch).
        section(&mut y, s.sec_map);
        let c1 = y;
        let map_h = 16.0 * k + 54.0 * k + row_gap + 50.0 * k + row_gap + 22.0 * k + 16.0 * k;
        card(c1, c1 + map_h);
        y += 16.0 * k;
        let key_txt = if self.capturing { s.press_key.to_string() } else { key_name(self.vk) };
        let kw = r.measure(&key_txt, Font::Text, 400.0).0 + 28.0 * k;
        row_label(y, s.map_key_title, Some(s.map_key_help), kw);
        btn(&mut hits, inner_r - kw, y + 6.0 * k, &key_txt, W::MapKey, self.capturing);
        y += 54.0 * k + row_gap;
        // Minimum chance slider: 0 to 50 %.
        let val = format!("{} %", self.v.min_chance);
        let vw = r.measure(&val, Font::Bold, 200.0).0;
        row_label(y, s.min_chance, None, vw);
        if draw {
            r.text(&val, Font::Bold, inner_r - vw, y, vw + 2.0, t.accent);
        }
        let (sx0, sx1, sy) = (pad + 26.0 * k, inner_r - 10.0 * k, y + 34.0 * k);
        self.slider = (sx0, sx1);
        if draw {
            let f = self.v.min_chance as f32 / 50.0;
            let kx = sx0 + (sx1 - sx0) * f;
            r.line(sx0, sy, sx1, sy, 4.0 * k, t.border);
            r.line(sx0, sy, kx, sy, 4.0 * k, t.accent);
            r.fill_circle(kx, sy, if hover == Some(W::Slider) || self.dragging == Some(W::Slider) { 9.0 * k } else { 8.0 * k }, t.accent);
            r.fill_circle(kx, sy, 3.5 * k, t.card);
        }
        hits.push(Hit { r: (sx0 - 10.0 * k, sy - 12.0 * k, sx1 + 10.0 * k, sy + 12.0 * k), w: W::Slider });
        y += 50.0 * k + row_gap;
        row_label(y, s.show_done, None, 40.0 * k);
        toggle(&mut hits, inner_r - 40.0 * k, y, self.v.show_done, W::ShowDone);
        y = c1 + map_h + 18.0 * k;

        // ---- Display ----
        // Rows: language (‹ value ›), theme (3 segments), panel opacity (slider), visible in captures (switch).
        section(&mut y, s.sec_display);
        let c2 = y;
        let disp_h = 16.0 * k + 32.0 * k + row_gap + 32.0 * k + row_gap + 50.0 * k + row_gap + 22.0 * k + 16.0 * k;
        card(c2, c2 + disp_h);
        y += 16.0 * k;
        // Language selector: empty locale = automatic, otherwise the language's display name.
        let lang_txt = if self.v.locale.is_empty() {
            self.auto_lang.clone()
        } else {
            crate::i18n::LANGS.iter().find(|l| l.0 == self.v.locale).map(|l| l.1.to_string()).unwrap_or(self.v.locale.clone())
        };
        // Drop-down field: current language and a chevron; the list itself is drawn last, over the window.
        let total = 230.0 * k;
        row_label(y + 6.0 * k, s.language.trim_end_matches([' ', ':']), None, total);
        let lx = inner_r - total;
        if draw {
            let hv = hover == Some(W::LangOpen) || self.lang_open;
            r.fill_rrect(lx, y, inner_r, y + 32.0 * k, 4.0 * k, if hv { t.hover } else { t.ctrl });
            r.stroke_rrect(lx + 0.5, y + 0.5, inner_r - 0.5, y + 32.0 * k - 0.5, 4.0 * k, 1.0, if self.lang_open { t.accent } else { t.border });
            r.text(&lang_txt, Font::Text, lx + 12.0 * k, y + 7.0 * k, total - 44.0 * k, t.text);
            // Chevron (down when closed, up when open).
            let (cx, cy, d) = (inner_r - 18.0 * k, y + 16.0 * k, 4.0 * k);
            let dy = if self.lang_open { -d / 2.0 } else { d / 2.0 };
            r.line_round(cx - d, cy - dy, cx, cy + dy, 1.6 * k, t.sub);
            r.line_round(cx, cy + dy, cx + d, cy - dy, 1.6 * k, t.sub);
        }
        hits.push(Hit { r: (lx, y, inner_r, y + 32.0 * k), w: W::LangOpen });
        let lang_field = (lx, y + 32.0 * k, inner_r);
        y += 32.0 * k + row_gap;
        // Theme: segmented control Auto / Light / Dark.
        let opts = [(s.theme_auto, "auto"), (s.theme_light, "light"), (s.theme_dark, "dark")];
        let segw = 90.0 * k;
        row_label(y + 6.0 * k, s.theme, None, segw * 3.0);
        let mut sx = inner_r - segw * 3.0;
        for (i, (label, code)) in opts.iter().enumerate() {
            let sel = self.v.theme == *code;
            if draw {
                let hv = hover == Some(W::Theme(i as u8));
                let bg = if sel { t.accent } else if hv { t.hover } else { t.ctrl };
                r.fill_rrect(sx, y, sx + segw, y + 32.0 * k, 4.0 * k, bg);
                r.stroke_rrect(sx + 0.5, y + 0.5, sx + segw - 0.5, y + 32.0 * k - 0.5, 4.0 * k, 1.0, t.border);
                let tw = r.measure(label, Font::Text, segw).0;
                r.text(label, Font::Text, sx + (segw - tw) / 2.0, y + 7.0 * k, tw + 2.0, if sel { t.on_accent } else { t.text });
            }
            hits.push(Hit { r: (sx, y, sx + segw, y + 32.0 * k), w: W::Theme(i as u8) });
            sx += segw;
        }
        y += 32.0 * k + row_gap;
        // Background opacity of the panel on the map (20 to 100 %).
        let val = format!("{} %", self.v.panel_opacity);
        let vw = r.measure(&val, Font::Bold, 200.0).0;
        row_label(y, s.panel_opacity, None, vw);
        if draw {
            r.text(&val, Font::Bold, inner_r - vw, y, vw + 2.0, t.accent);
        }
        let (ox0, ox1, oy) = (pad + 26.0 * k, inner_r - 10.0 * k, y + 34.0 * k);
        self.slider2 = (ox0, ox1);
        if draw {
            let f = (self.v.panel_opacity as f32 - 20.0) / 80.0;
            let kx = ox0 + (ox1 - ox0) * f;
            r.line(ox0, oy, ox1, oy, 4.0 * k, t.border);
            r.line(ox0, oy, kx, oy, 4.0 * k, t.accent);
            r.fill_circle(kx, oy, if hover == Some(W::Opacity) || self.dragging == Some(W::Opacity) { 9.0 * k } else { 8.0 * k }, t.accent);
            r.fill_circle(kx, oy, 3.5 * k, t.card);
        }
        hits.push(Hit { r: (ox0 - 10.0 * k, oy - 12.0 * k, ox1 + 10.0 * k, oy + 12.0 * k), w: W::Opacity });
        y += 50.0 * k + row_gap;
        row_label(y, s.capture_visible, None, 40.0 * k);
        toggle(&mut hits, inner_r - 40.0 * k, y, self.v.capture_visible, W::Capture);
        y = c2 + disp_h + 18.0 * k;

        // ---- Application ----
        // Rows: start with Windows, automatic updates (two switches).
        section(&mut y, s.sec_app);
        let c3 = y;
        let app_h = 16.0 * k + 22.0 * k + row_gap + 22.0 * k + 16.0 * k;
        card(c3, c3 + app_h);
        y += 16.0 * k;
        row_label(y, s.autostart, None, 40.0 * k);
        toggle(&mut hits, inner_r - 40.0 * k, y, self.v.autostart, W::Autostart);
        y += 22.0 * k + row_gap;
        row_label(y, s.auto_update, None, 40.0 * k);
        toggle(&mut hits, inner_r - 40.0 * k, y, self.v.auto_update, W::AutoUpdate);
        y = c3 + app_h + 14.0 * k;

        // Help + buttons (separator line, then Save / Apply / Cancel right-aligned, from right to left).
        let (_, hh) = r.measure(s.help, Font::Small, w - 2.0 * pad);
        if draw {
            r.text(s.help, Font::Small, pad, y, w - 2.0 * pad, t.sub);
        }
        y += hh + 16.0 * k;
        if draw {
            r.line(0.0, y, w, y, 1.0, t.border);
        }
        y += 14.0 * k;
        let save_w = r.measure(s.save, Font::Text, 400.0).0 + 28.0 * k;
        let apply_w = r.measure(s.apply, Font::Text, 400.0).0 + 28.0 * k;
        let cancel_w = r.measure(s.cancel, Font::Text, 400.0).0 + 28.0 * k;
        let mut bx = w - pad - save_w;
        btn(&mut hits, bx, y, s.save, W::Save, true);
        bx -= apply_w + 8.0 * k;
        btn(&mut hits, bx, y, s.apply, W::Apply, false);
        bx -= cancel_w + 8.0 * k;
        btn(&mut hits, bx, y, s.cancel, W::Cancel, false);
        y += 32.0 * k + 16.0 * k;
        // Language list, over everything else (opens downwards, or upwards if it would not fit).
        if self.lang_open {
            let entries: Vec<String> = std::iter::once(self.auto_lang.clone()).chain(crate::i18n::LANGS.iter().map(|l| l.1.to_string())).collect();
            let ih = 30.0 * k;
            let lh = ih * entries.len() as f32 + 8.0 * k;
            let (lx, below, rx) = lang_field;
            let mut ly = below + 4.0 * k;
            if ly + lh > y {
                ly = below - 32.0 * k - 4.0 * k - lh;
            }
            if draw {
                r.fill_rrect(lx + 2.0 * k, ly + 3.0 * k, rx + 2.0 * k, ly + lh + 3.0 * k, 6.0 * k, (0.0, 0.0, 0.0, 0.25));
                r.fill_rrect(lx, ly, rx, ly + lh, 6.0 * k, t.card);
                r.stroke_rrect(lx + 0.5, ly + 0.5, rx - 0.5, ly + lh - 0.5, 6.0 * k, 1.0, t.border);
            }
            let codes: Vec<&str> = std::iter::once("").chain(crate::i18n::LANGS.iter().map(|l| l.0)).collect();
            let mut iy = ly + 4.0 * k;
            for (i, name) in entries.iter().enumerate() {
                let id = W::LangItem(i as u8);
                let sel = codes[i] == self.v.locale;
                if draw {
                    if hover == Some(id) || sel {
                        r.fill_rrect(lx + 4.0 * k, iy, rx - 4.0 * k, iy + ih, 4.0 * k, if hover == Some(id) { t.hover } else { t.ctrl });
                    }
                    if sel {
                        r.fill_rrect(lx + 6.0 * k, iy + 7.0 * k, lx + 9.0 * k, iy + ih - 7.0 * k, 1.5 * k, t.accent);
                    }
                    r.text(name, Font::Text, lx + 16.0 * k, iy + 6.0 * k, rx - lx - 24.0 * k, t.text);
                }
                hits.push(Hit { r: (lx + 4.0 * k, iy, rx - 4.0 * k, iy + ih), w: id });
                iy += ih;
            }
        }
        self.height = y;
        self.hits = hits;
    }

    /// Widget at (x, y), if any. While the language list is open, only the list and its field respond.
    fn hit(&self, x: f32, y: f32) -> Option<W> {
        self.hits
            .iter()
            .filter(|h| !self.lang_open || matches!(h.w, W::LangOpen | W::LangItem(_)))
            .find(|h| x >= h.r.0 && x <= h.r.2 && y >= h.r.1 && y <= h.r.3)
            .map(|h| h.w)
    }

    /// Mouse moved to (x, y) (negative = left the window): drags the active slider, or updates hover.
    fn mouse_move(&mut self, x: f32, y: f32) {
        match self.dragging {
            Some(W::Slider) => return self.set_slider(x),
            Some(W::Opacity) => return self.set_opacity(x),
            _ => {}
        }
        let h = self.hit(x, y);
        if h != self.hover {
            self.hover = h;
            self.redraw();
        }
    }

    /// Sets the panel opacity (20..=100 %) from a mouse x position on the opacity slider.
    fn set_opacity(&mut self, x: f32) {
        let (a, b) = self.slider2;
        let f = ((x - a) / (b - a)).clamp(0.0, 1.0);
        let v = 20 + (f * 80.0).round() as u32;
        if v != self.v.panel_opacity {
            self.v.panel_opacity = v;
            self.redraw();
        }
    }

    /// Sets the minimum chance (0..=50 %) from a mouse x position on the chance slider.
    fn set_slider(&mut self, x: f32) {
        let (a, b) = self.slider;
        let f = ((x - a) / (b - a)).clamp(0.0, 1.0);
        let v = (f * 50.0).round() as u32;
        if v != self.v.min_chance {
            self.v.min_chance = v;
            self.redraw();
        }
    }

    /// Click in the window. Returns an action for the app to handle (test, apply, save…).
    pub fn click(&mut self, x: f32, y: f32, down: bool) -> Option<i32> {
        // Button released: end any slider drag.
        if !down {
            if self.dragging.take().is_some() {
                unsafe {
                    let _ = ReleaseCapture();
                }
                self.redraw();
            }
            return None;
        }
        let Some(w) = self.hit(x, y) else {
            // A click outside the open language list closes it.
            if self.lang_open {
                self.lang_open = false;
                self.redraw();
            }
            return None;
        };
        let mut out = None;
        match w {
            // Start dragging a slider (mouse captured until release) and jump to the click position.
            W::Slider | W::Opacity => {
                self.dragging = Some(w);
                unsafe {
                    SetCapture(self.hwnd);
                }
                if w == W::Slider {
                    self.set_slider(x);
                } else {
                    self.set_opacity(x);
                }
            }
            W::KeyShow => self.toggle_mask(),
            W::Test => out = Some(ID_TEST),
            W::MapKey => out = Some(ID_MAPKEY),
            W::ShowDone => self.v.show_done = !self.v.show_done,
            W::Capture => self.v.capture_visible = !self.v.capture_visible,
            W::Autostart => self.v.autostart = !self.v.autostart,
            W::AutoUpdate => self.v.auto_update = !self.v.auto_update,
            W::LangOpen => self.lang_open = !self.lang_open,
            // Entry 0 = Auto (""), then the languages in `LANGS` order.
            W::LangItem(i) => {
                self.v.locale = if i == 0 { String::new() } else { crate::i18n::LANGS[i as usize - 1].0.to_string() };
                self.lang_open = false;
            }
            W::Theme(i) => {
                self.v.theme = ["auto", "light", "dark"][i as usize].to_string();
                self.retheme();
            }
            W::Cancel => out = Some(ID_CANCEL),
            W::Apply => out = Some(ID_APPLY),
            W::Save => out = Some(ID_SAVE),
        }
        self.redraw();
        out
    }

    /// Shows or hides the key in clear text (0x25CF = "●" mask character), then re-lays out
    /// because the button label width changes.
    pub fn toggle_mask(&mut self) {
        self.masked = !self.masked;
        unsafe {
            SendMessageW(self.edit, EM_SETPASSWORDCHAR, WPARAM(if self.masked { 0x25CF } else { 0 }), LPARAM(0));
            let _ = InvalidateRect(self.edit, None, true);
        }
        self.layout(false);
        self.place_edit();
        self.redraw();
    }

    /// Label of the key button: "Press a key…" during capture, otherwise the key's name.
    pub fn set_mapkey_text(&mut self, t: &str) {
        self.capturing = t == self.s.press_key;
        self.redraw();
    }

    /// Sets the status line under the key; a leading ✔ shows it as success, ✖ as error.
    pub fn set_status(&mut self, t: &str) {
        let kind = if t.starts_with('✔') { 1 } else if t.starts_with('✖') { -1 } else { 0 };
        self.status = (t.to_string(), kind);
        self.redraw();
    }

    /// Kept for the app's `ID_CHANCE` handler; nothing to do here (the slider redraws itself).
    pub fn chance_moved(&self) {}

    /// Current text of the key field, trimmed.
    pub fn key_text(&self) -> String {
        unsafe {
            let n = GetWindowTextLengthW(self.edit);
            let mut buf = vec![0u16; n as usize + 1];
            let got = GetWindowTextW(self.edit, &mut buf);
            String::from_utf16_lossy(&buf[..got as usize]).trim().to_string()
        }
    }

    /// All edited values, with the key read from the field and the captured map key.
    pub fn values(&self) -> Values {
        let mut v = self.v.clone();
        v.key = self.key_text();
        v.vk = self.vk;
        v
    }

    /// Destroys the window and frees its GDI font and brush.
    pub fn close(self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            let _ = DeleteObject(HGDIOBJ(self.font.0));
            let _ = DeleteObject(HGDIOBJ(self.edit_brush.0));
        }
    }
}
