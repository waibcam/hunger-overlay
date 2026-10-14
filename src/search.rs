//! "Find an item" window: a text field + results (from the website's index), using the settings theme.
//! Picking an item adds it to the map (its containers, harvest nodes and placed loot).
//! Quests from the already loaded pack are listed too, so they can be tracked from here.

use crate::i18n::Strings;
use crate::render::{Font, Renderer};
use crate::settings::{theme_for, Theme};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, EndPaint, InvalidateRect, SetBkColor, SetTextColor, HBRUSH, HDC,
    HFONT, HGDIOBJ, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowTextLengthW, GetWindowTextW,
    LoadCursorW, RegisterClassW, SendMessageW, SetCursor, SetForegroundWindow, SetWindowPos, ShowWindow, ES_AUTOHSCROLL, HMENU,
    IDC_ARROW, IDC_HAND, SWP_NOZORDER, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_SETCURSOR, WM_SETFONT, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_TOPMOST,
    WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};

/// Maximum number of results listed (the window height is sized for this many rows).
const MAX_RESULTS: usize = 12;

/// One searchable entry: an item from the website's index, or a quest from the pack.
#[derive(Clone)]
pub struct Entry {
    /// `ID_…` for an item, `quest:<quest_id>` for a quest.
    pub key: String,
    /// Display name (in the current language).
    pub name: String,
    /// Item rarity ("common", "rare", ...), used for the colour dot.
    pub rarity: Option<String>,
    /// Number of map points for the item (also used to rank results).
    pub n: u32,
    /// Kinds of sources: "container", "harvest", "placed".
    pub sources: Vec<String>,
    /// Quest: (giver, already tracked).
    pub quest: Option<(String, bool)>,
}

/// State of the open search window.
pub struct Search {
    /// The top-level window.
    pub hwnd: HWND,
    /// Native EDIT control used for typing.
    edit: HWND,
    /// GDI font given to the EDIT control.
    font: HFONT,
    /// Background brush for the EDIT control (theme colour).
    edit_brush: HBRUSH,
    /// Renderer used to paint the window.
    r: Renderer,
    /// UI strings in the current language.
    s: &'static Strings,
    /// DPI scale factor (dpi / 96).
    k: f32,
    /// Light or dark colour theme.
    theme: &'static Theme,
    /// All searchable entries (None while the index is still downloading).
    entries: Option<Vec<Entry>>,
    /// Has the item index arrived? (quests, on the other hand, come from the already loaded pack)
    items_loaded: bool,
    /// Indices into `entries` of the current results, best first.
    results: Vec<usize>,
    /// Index into `results` of the selected row (keyboard / hover).
    sel: usize,
    /// Index of the row under the mouse, if any.
    hover: Option<usize>,
    /// Painted rows: (top, bottom, entry index), used for hit-testing.
    rows: Vec<(f32, f32, usize)>,
    /// Client width in pixels.
    w: f32,
    /// Client height in pixels.
    h: f32,
}

/// Converts a Rust string into a NUL-terminated UTF-16 string for Win32.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// Converts an `Rgba` colour into a GDI COLORREF (0x00BBGGRR, alpha ignored).
fn rgb(c: crate::render::Rgba) -> COLORREF {
    COLORREF(((c.2 * 255.0) as u32) << 16 | ((c.1 * 255.0) as u32) << 8 | (c.0 * 255.0) as u32)
}

/// Lowercase without accents, for a forgiving search ("achillee" finds "Achillée").
pub fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'ö' | 'õ' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ý' | 'ÿ' => 'y',
            'œ' => 'o',
            'ß' => 's',
            c => c,
        })
        .collect()
}

/// Colour of the dot shown next to an item, by rarity (grey for common / unknown).
fn rarity_color(r: Option<&str>) -> crate::render::Rgba {
    match r.unwrap_or("").to_lowercase().as_str() {
        "uncommon" => (0.45, 0.78, 0.35, 1.0),
        "rare" => (0.35, 0.6, 0.95, 1.0),
        "epic" => (0.7, 0.45, 0.95, 1.0),
        "legendary" => (0.98, 0.62, 0.2, 1.0),
        _ => (0.62, 0.62, 0.62, 1.0),
    }
}

/// Subclass procedure of the EDIT control: intercepts navigation keys (↑ ↓ Enter Esc) before the
/// control handles them; everything else goes to the default EDIT behaviour.
unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    if msg == WM_KEYDOWN {
        let handled = crate::with_search(|s| s.key(wp.0 as u32)).flatten();
        match handled {
            Some(Some(key)) => {
                crate::on_search_pick(key);
                return LRESULT(0);
            }
            Some(None) => return LRESULT(0),
            None => {}
        }
    }
    // No "beep" on Enter / Esc (0x0102 = WM_CHAR).
    if msg == 0x0102 && (wp.0 == 13 || wp.0 == 27) {
        return LRESULT(0);
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}

/// Window procedure of the search window. State lives in the app (reached via `crate::with_search`).
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // Mouse position (client coordinates, signed) from lParam.
    let pos = || (((lp.0 & 0xffff) as i16) as f32, (((lp.0 >> 16) & 0xffff) as i16) as f32);
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            crate::with_search(|s| s.paint(hdc));
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        // We paint the whole background ourselves: skip erasing to avoid flicker.
        WM_ERASEBKGND => LRESULT(1),
        WM_COMMAND => {
            if (wp.0 >> 16) & 0xffff == 0x0300 {
                // EN_CHANGE: the text was edited
                crate::with_search(|s| s.refilter());
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = pos();
            // Ask for WM_MOUSELEAVE so the hover highlight can be cleared.
            let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
            let _ = TrackMouseEvent(&mut tme);
            crate::with_search(|s| s.mouse(x, y));
            LRESULT(0)
        }
        0x02A3 => {
            // WM_MOUSELEAVE
            crate::with_search(|s| s.mouse(-1.0, -1.0));
            LRESULT(0)
        }
        WM_SETCURSOR if (lp.0 & 0xffff) as u32 == 1 => {
            // In the client area (HTCLIENT): hand cursor over a result row, arrow elsewhere.
            let over = crate::with_search(|s| s.hover.is_some()).unwrap_or(false);
            let _ = SetCursor(LoadCursorW(None, if over { IDC_HAND } else { IDC_ARROW }).unwrap_or_default());
            LRESULT(1)
        }
        WM_LBUTTONUP => {
            let (x, y) = pos();
            if let Some(Some(key)) = crate::with_search(|s| s.click(x, y)) {
                crate::on_search_pick(key);
            }
            LRESULT(0)
        }
        0x0133 => {
            // WM_CTLCOLOREDIT: theme colours for the EDIT control.
            let hdc = HDC(wp.0 as _);
            crate::with_search(|s| {
                SetTextColor(hdc, rgb(s.theme.text));
                SetBkColor(hdc, rgb(s.theme.ctrl));
                LRESULT(s.edit_brush.0 as isize)
            })
            .unwrap_or(LRESULT(0))
        }
        WM_CLOSE => {
            // Let the app drop its `Search` (which destroys the window).
            crate::on_search_close();
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

impl Search {
    /// Creates and shows the search window. `area` = the game's screen rectangle (x, y, width, height),
    /// used to centre the window on it. `entries` is None while the item index is still downloading.
    pub fn open(s: &'static Strings, theme: &str, entries: Option<Vec<Entry>>, items_loaded: bool, area: (i32, i32, i32, i32)) -> Option<Search> {
        unsafe {
            let inst = GetModuleHandleW(None).ok()?;
            let class = w!("HungerOverlaySearch");
            // Registering again on later openings simply fails harmlessly.
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: inst.into(),
                lpszClassName: class,
                hIcon: crate::tray::app_icon(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            });
            // Centred on the game; without the game, on the primary screen. The scale is that of the screen it opens on.
            let area = if area.2 > 0 && area.3 > 0 {
                area
            } else {
                (0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
            };
            let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
            let title = wide(s.search_title);
            let (cx, cy) = (area.0 + area.2 / 2, area.1 + area.3 / 3);
            // Create small first, then resize once the DPI of the target screen is known.
            let hwnd = CreateWindowExW(WS_EX_TOPMOST, class, PCWSTR(title.as_ptr()), style, cx, cy, 100, 100, None, None, inst, None).ok()?;
            let dpi = GetDpiForWindow(hwnd).max(96);
            let k = dpi as f32 / 96.0;
            // Client size: header + field (96 px) + one 40 px row per result + bottom margin.
            let (w, h) = (480.0 * k, (96.0 + 40.0 * MAX_RESULTS as f32 + 20.0) * k);
            let mut rc = RECT { left: 0, top: 0, right: w as i32, bottom: h as i32 };
            let _ = AdjustWindowRectExForDpi(&mut rc, style, false, WS_EX_TOPMOST, dpi);
            let (ww, wh) = (rc.right - rc.left, rc.bottom - rc.top);
            let _ = SetWindowPos(hwnd, None, cx - ww / 2, cy - wh / 3, ww, wh, SWP_NOZORDER);
            // Native EDIT control (Segoe UI 15 px, ClearType) for text input.
            let font = CreateFontW(-((15.0 * k) as i32), 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 5, 0, w!("Segoe UI"));
            let edit = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("EDIT"),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                (26.0 * k) as i32,
                (60.0 * k - 10.0 * k) as i32,
                (w - 52.0 * k) as i32,
                (22.0 * k) as i32,
                hwnd,
                HMENU(1 as _),
                inst,
                None,
            )
            .ok()?;
            SendMessageW(edit, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
            let _ = SetWindowSubclass(edit, Some(edit_proc), 1, 0);
            // Renderer with application-style font sizes.
            let mut r = Renderer::new().ok()?;
            r.set_base_sizes([18.0, 14.0, 14.0, 12.0]);
            let _ = r.set_font_scale(k);
            let th = theme_for(theme);
            // Dark title bar when the dark theme is used (20 = DWMWA_USE_IMMERSIVE_DARK_MODE).
            let dark: i32 = std::ptr::eq(th, &crate::settings::DARK) as i32;
            let _ = DwmSetWindowAttribute(hwnd, DWMWINDOWATTRIBUTE(20), &dark as *const i32 as *const _, 4);
            let mut st = Search {
                hwnd,
                edit,
                font,
                edit_brush: CreateSolidBrush(rgb(th.ctrl)),
                r,
                s,
                k,
                theme: th,
                entries,
                items_loaded,
                results: vec![],
                sel: 0,
                hover: None,
                rows: vec![],
                w,
                h,
            };
            st.refilter();
            // Place the EDIT control inside the painted field frame, then show and focus.
            let _ = SetWindowPos(edit, None, (26.0 * k) as i32, (40.0 * k) as i32, (w - 52.0 * k) as i32, (22.0 * k) as i32, SWP_NOZORDER);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(edit);
            Some(st)
        }
    }

    /// The index has just arrived (it was being downloaded).
    pub fn set_entries(&mut self, e: Vec<Entry>) {
        self.entries = Some(e);
        self.items_loaded = true;
        self.refilter();
    }

    /// True once the item index has been received.
    pub fn has_entries(&self) -> bool {
        self.items_loaded
    }

    /// Current text of the EDIT control, trimmed.
    fn query(&self) -> String {
        unsafe {
            let n = GetWindowTextLengthW(self.edit);
            let mut buf = vec![0u16; n as usize + 1];
            let got = GetWindowTextW(self.edit, &mut buf);
            String::from_utf16_lossy(&buf[..got as usize]).trim().to_string()
        }
    }

    /// Recomputes the results for the current query: name prefix matches first, then name / key
    /// substring matches; within each group, entries with more map points come first.
    fn refilter(&mut self) {
        let q = fold(&self.query());
        self.results.clear();
        if let Some(es) = &self.entries {
            if !q.is_empty() {
                let mut found: Vec<(u8, u32, usize)> = es
                    .iter()
                    .enumerate()
                    .filter_map(|(i, e)| {
                        let n = fold(&e.name);
                        // Quests come before items at equal relevance (there are few of them).
                        let quest_bonus = if e.quest.is_some() { 10_000 } else { 0 };
                        if n.starts_with(&q) {
                            Some((0, e.n + quest_bonus, i))
                        } else if n.contains(&q) || fold(&e.key).contains(&q) {
                            Some((1, e.n + quest_bonus, i))
                        } else {
                            None
                        }
                    })
                    .collect();
                found.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
                self.results = found.into_iter().take(MAX_RESULTS).map(|f| f.2).collect();
            }
        }
        self.sel = 0;
        unsafe {
            let _ = InvalidateRect(self.hwnd, None, false);
        }
    }

    /// Field keys: ↑ ↓ to choose, Enter to confirm, Esc to close.
    /// Returns Some(Some(key)) = item chosen, Some(None) = key handled, None = normal key.
    fn key(&mut self, vk: u32) -> Option<Option<String>> {
        // Virtual-key codes: 0x28 = Down, 0x26 = Up, 0x0D = Enter, 0x1B = Esc.
        match vk {
            0x28 => {
                if self.sel + 1 < self.results.len() {
                    self.sel += 1;
                }
            }
            0x26 => self.sel = self.sel.saturating_sub(1),
            0x0D => {
                let i = *self.results.get(self.sel)?;
                return Some(self.entries.as_ref().map(|e| e[i].key.clone()));
            }
            0x1B => {
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(self.hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                return Some(None);
            }
            _ => return None,
        }
        unsafe {
            let _ = InvalidateRect(self.hwnd, None, false);
        }
        Some(None)
    }

    /// Mouse moved to (x, y) (negative = left the window): updates hover and selection, repaints if changed.
    fn mouse(&mut self, x: f32, y: f32) {
        let h = self.rows.iter().position(|&(t, b, _)| x >= 0.0 && y >= t && y <= b);
        if h != self.hover {
            self.hover = h;
            if let Some(i) = h {
                self.sel = i;
            }
            unsafe {
                let _ = InvalidateRect(self.hwnd, None, false);
            }
        }
    }

    /// Click at (x, y): returns the key of the clicked result, if any.
    fn click(&mut self, x: f32, y: f32) -> Option<String> {
        let (_, _, i) = *self.rows.iter().find(|&&(t, b, _)| x >= 0.0 && y >= t && y <= b)?;
        self.entries.as_ref().map(|e| e[i].key.clone())
    }

    /// Paints the whole client area and records the result rows for hit-testing.
    fn paint(&mut self, hdc: HDC) {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        if !self.r.begin(rc.right, rc.bottom) {
            return;
        }
        let (k, t, s, r) = (self.k, self.theme, self.s, &self.r);
        let (w, pad) = (self.w, 16.0 * k);
        // Background and help line.
        r.fill_rrect(0.0, 0.0, w + 4.0, self.h + 4.0, 0.0, t.bg);
        r.text(s.search_help, Font::Small, pad, 12.0 * k, w - 2.0 * pad, t.sub);
        // Frame of the text field (the native control sits inside it).
        r.fill_rrect(pad, 32.0 * k, w - pad, 70.0 * k, 5.0 * k, t.ctrl);
        r.stroke_rrect(pad + 0.5, 32.0 * k + 0.5, w - pad - 0.5, 70.0 * k - 0.5, 5.0 * k, 1.0, t.border);
        r.line(pad + 4.0 * k, 70.0 * k - 1.0, w - pad - 4.0 * k, 70.0 * k - 1.0, 2.0 * k, t.accent);
        // Results list (or a "loading" / "no result" message).
        let mut y = 82.0 * k;
        let mut rows = vec![];
        match &self.entries {
            None => {
                r.text(s.search_loading, Font::Text, pad, y, w - 2.0 * pad, t.sub);
            }
            Some(es) => {
                if self.results.is_empty() && !self.query().is_empty() {
                    let msg = if self.items_loaded { s.search_none } else { s.search_loading };
                    r.text(msg, Font::Text, pad, y, w - 2.0 * pad, t.sub);
                }
                for (ri, &i) in self.results.iter().enumerate() {
                    let e = &es[i];
                    let rh = 38.0 * k;
                    if ri == self.sel {
                        r.fill_rrect(pad - 6.0 * k, y, w - pad + 6.0 * k, y + rh, 5.0 * k, t.hover);
                    }
                    if let Some((giver, tracked)) = &e.quest {
                        // Quest: 📜, giver, and whether it is already tracked.
                        let right = format!("{giver} · {}", if *tracked { s.quest_tracked } else { s.quest_track });
                        let (rw, _) = r.measure(&right, Font::Small, 280.0 * k);
                        r.text("📜", Font::Text, pad, y + 9.0 * k, 18.0 * k, t.text);
                        r.text(&e.name, Font::Text, pad + 20.0 * k, y + 9.0 * k, w - 2.0 * pad - rw - 32.0 * k, if *tracked { t.accent } else { t.text });
                        r.text(&right, Font::Small, w - pad - rw, y + 11.0 * k, rw + 2.0, t.sub);
                        rows.push((y, y + rh, i));
                        y += rh + 2.0 * k;
                        continue;
                    }
                    // Item: rarity dot, name, and on the right its source kinds + number of map points.
                    r.fill_circle(pad + 6.0 * k, y + rh / 2.0, 5.0 * k, rarity_color(e.rarity.as_deref()));
                    let src: Vec<&str> = e
                        .sources
                        .iter()
                        .map(|x| match x.as_str() {
                            "container" => s.src_container,
                            "harvest" => s.src_harvest,
                            "placed" => s.src_placed,
                            other => other,
                        })
                        .collect();
                    let right = format!("{} · {} {}", src.join(", "), e.n, s.src_points);
                    let (rw, _) = r.measure(&right, Font::Small, 260.0 * k);
                    r.text(&e.name, Font::Text, pad + 20.0 * k, y + 9.0 * k, w - 2.0 * pad - rw - 32.0 * k, t.text);
                    r.text(&right, Font::Small, w - pad - rw, y + 11.0 * k, rw + 2.0, t.sub);
                    rows.push((y, y + rh, i));
                    y += rh + 2.0 * k;
                }
            }
        }
        self.rows = rows;
        self.r.present_dc(hdc);
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
