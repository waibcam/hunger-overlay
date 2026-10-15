//! Quest journal window (docs/DESIGN_QUESTS.md §3): the player's progression (and the raid allies') per character,
//! "Up next" suggestions, "I am here" with a preview of every cascaded change, exact undo with its history, and the
//! choice of raid allies. A topmost window, drawn with the panel's tokens because it opens over the game.
//!
//! The window only displays data handed over by the app (`Data`) and returns commands (`Cmd`); the app writes the
//! changes (to the website for the player, to allies.json for a local ally) and records them for undo.

use crate::i18n::Strings;
use crate::markers;
use crate::model::HubEntry;
use crate::panel::{ACCENT, DARK_INK, GOLD, GOLD_HI, HOVER_ROW, INK700, INK800, INK900, INK950, LINE_GOLD, T100, T200, T300, T500, WARN, WARN_TEXT, OK};
use crate::panel::{IC_ADD_FRIEND, IC_CHECK, IC_CLOUD, IC_DOWN, IC_HIDDEN, IC_PC, IC_RIGHT, IC_SEARCH, IC_WARN};
use crate::quests::{Change, Graph, Next, Progress, State, Write};
use crate::render::{Font, Renderer, Rgba};
use std::collections::{HashMap, HashSet};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::Graphics::Gdi::{BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, EndPaint, InvalidateRect, SetBkColor, SetTextColor, HBRUSH, HDC, HFONT, HGDIOBJ, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetSystemMetrics, GetWindowTextLengthW, GetWindowTextW, LoadCursorW, PostMessageW,
    RegisterClassW, SendMessageW, SetCursor, SetForegroundWindow, SetWindowPos, SetWindowTextW, ShowWindow, ES_AUTOHSCROLL, HMENU, IDC_ARROW, IDC_HAND,
    SM_CXSCREEN, SM_CYSCREEN, SWP_NOZORDER, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_SETCURSOR, WM_SETFONT, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_TOPMOST, WS_OVERLAPPED,
    WS_SYSMENU, WS_TABSTOP,
};

/// Key of the "+ Ally" tab.
pub const ADD_TAB: &str = "+ally";

/// Whose progression a tab shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// The player (the website account).
    Me,
    /// A website ally (read-only).
    Site,
    /// An ally kept on this PC (editable).
    Local,
}

/// One tab: the player or a raid ally.
#[derive(Debug, Clone)]
pub struct Subject {
    /// "me", "u_…"/"m_…" (website) or "l_…" (local).
    pub key: String,
    /// Display name.
    pub name: String,
    /// Initial(s).
    pub initial: String,
    /// Colour (gold for the player, the fixed ally colour otherwise).
    pub color: (f32, f32, f32),
    /// Who it is.
    pub kind: Kind,
    /// Hidden on the map.
    pub hidden: bool,
}

/// One entry of the undo history: (record id, label, age in seconds, quests changed elsewhere since).
pub type HistoryEntry = (u64, String, u64, Vec<String>);

/// Everything the journal displays, handed over by the app.
#[derive(Clone, Default)]
pub struct Data {
    /// The player first, then the allies.
    pub subjects: Vec<Subject>,
    /// Quest prerequisites.
    pub graph: Graph,
    /// Progression per subject key.
    pub progress: HashMap<String, Progress>,
    /// Quest titles (current language).
    pub titles: HashMap<String, String>,
    /// The player's tracked quests: badge and colour.
    pub badges: HashMap<String, (String, (f32, f32, f32))>,
    /// Undo history per subject key, newest first.
    pub history: HashMap<String, Vec<HistoryEntry>>,
    /// The website publishes prerequisites and full progression (contract v1.2).
    pub v12: bool,
    /// Synced with the website (no network / key problem).
    pub synced: bool,
    /// Hub list for the "+ Ally" tab (None = loading).
    pub hub: Option<Result<Vec<HubEntry>, &'static str>>,
    /// Number of markers on the open map per quest id (0 / absent = none).
    pub spots: HashMap<String, usize>,
}

/// What the journal asks the app to do.
#[derive(Debug, Clone)]
pub enum Cmd {
    /// Commit changes to a subject's progression (one undoable record).
    Apply { subject: String, label: String, changes: Vec<Change> },
    /// Undo one record.
    Undo(u64),
    /// Undo this record and every newer one of the same subject.
    UndoTo(String, u64),
    /// Add a website ally (user id).
    AddSiteAlly(String),
    /// Add a local ally (name).
    AddLocalAlly(String),
    /// Remove an ally from the raid.
    RemoveAlly(String),
    /// Hide / show an ally on the map.
    HideAlly(String),
    /// Fetch the hub list.
    FetchHub,
    /// The selected tab changed (remembered by the app).
    Tab(String),
}

/// Clickable zones of the journal.
#[derive(Debug, Clone, PartialEq)]
enum Hit {
    Tab(String),
    ToggleRepeat,
    ToggleCard(String),
    /// Opens the confirm strip of a quest (stepper dot, locked row, waiting row).
    Strip(String),
    CloseStrip,
    /// Commits changes directly (single-quest actions).
    Do(String, Vec<Change>),
    ToggleHistory,
    Undo(u64),
    UndoTo(u64),
    AddSite(String),
    AddLocal,
    Remove(String),
    Hide(String),
    Retry,
}

/// The open journal window.
pub struct Journal {
    /// Top-level window.
    pub hwnd: HWND,
    /// Filter field.
    filter: HWND,
    /// Name field of the "+ Ally" tab.
    name: HWND,
    /// Font of both fields.
    font: HFONT,
    /// Background brush of both fields.
    brush: HBRUSH,
    /// Renderer.
    r: Renderer,
    /// UI strings.
    s: &'static Strings,
    /// DPI scale.
    k: f32,
    /// Client size.
    w: f32,
    h: f32,
    /// Displayed data.
    d: Data,
    /// Selected tab (subject key or `ADD_TAB`).
    pub tab: String,
    /// Expanded character cards (giver names).
    open: HashSet<String>,
    /// Quest whose confirm strip is open.
    strip: Option<String>,
    /// Repeatable quests listed.
    repeat: bool,
    /// History list open.
    hist_open: bool,
    /// Scroll offset of the body.
    scroll: f32,
    /// Maximum scroll.
    max_scroll: f32,
    /// Clickable zones of the last paint.
    hits: Vec<(f32, f32, f32, f32, Hit)>,
    /// Index of the hovered zone.
    hover: Option<usize>,
    /// Current filter text (folded).
    query: String,
    /// Quest to scroll to once painted.
    focus: Option<String>,
    /// Mouse position (client coordinates; negative = outside the window).
    mouse: (f32, f32),
}

/// NUL-terminated UTF-16.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// `Rgba` → COLORREF.
fn rgb(c: Rgba) -> COLORREF {
    COLORREF(((c.2 * 255.0) as u32) << 16 | ((c.1 * 255.0) as u32) << 8 | (c.0 * 255.0) as u32)
}

/// `c` with its alpha multiplied by `a`.
fn fade(c: Rgba, a: f32) -> Rgba {
    (c.0, c.1, c.2, c.3 * a)
}

/// Field keys: Enter in the name field adds the ally, Esc closes the window.
unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, id: usize, _data: usize) -> LRESULT {
    if msg == WM_KEYDOWN && (wp.0 == 0x1B || (wp.0 == 0x0D && id == 2)) {
        if wp.0 == 0x1B {
            if let Some(Some(h)) = crate::with_journal(|j| Some(j.hwnd)) {
                let _ = PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        } else if let Some(Some(c)) = crate::with_journal(|j| j.act(Hit::AddLocal)) {
            crate::on_journal_cmd(c);
        }
        return LRESULT(0);
    }
    if msg == 0x0102 && (wp.0 == 13 || wp.0 == 27) {
        return LRESULT(0);
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}

/// Window procedure. State lives in the app (reached through `crate::with_journal`).
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let pos = || (((lp.0 & 0xffff) as i16) as f32, (((lp.0 >> 16) & 0xffff) as i16) as f32);
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            crate::with_journal(|j| j.paint(hdc));
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_COMMAND => {
            // EN_CHANGE of the filter field.
            if (wp.0 >> 16) & 0xffff == 0x0300 && wp.0 & 0xffff == 1 {
                crate::with_journal(|j| j.refilter());
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = pos();
            let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
            let _ = TrackMouseEvent(&mut tme);
            crate::with_journal(|j| j.mouse(x, y));
            LRESULT(0)
        }
        0x02A3 => {
            crate::with_journal(|j| j.mouse(-1.0, -1.0));
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp.0 >> 16) & 0xffff) as u16 as i16;
            crate::with_journal(|j| j.wheel(delta));
            LRESULT(0)
        }
        WM_SETCURSOR if (lp.0 & 0xffff) as u32 == 1 => {
            let over = crate::with_journal(|j| j.hover.is_some()).unwrap_or(false);
            let _ = SetCursor(LoadCursorW(None, if over { IDC_HAND } else { IDC_ARROW }).unwrap_or_default());
            LRESULT(1)
        }
        WM_LBUTTONUP => {
            let (x, y) = pos();
            if let Some(Some(c)) = crate::with_journal(|j| j.click(x, y)) {
                crate::on_journal_cmd(c);
            }
            LRESULT(0)
        }
        0x0133 => {
            // WM_CTLCOLOREDIT: dark fields.
            let hdc = HDC(wp.0 as _);
            crate::with_journal(|j| {
                SetTextColor(hdc, rgb(T100));
                SetBkColor(hdc, rgb((0.055, 0.045, 0.032, 1.0)));
                LRESULT(j.brush.0 as isize)
            })
            .unwrap_or(LRESULT(0))
        }
        WM_CLOSE => {
            crate::on_journal_close();
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Paint context: renderer, scale, hits being built.
struct P<'a> {
    r: &'a Renderer,
    k: f32,
    hits: Vec<(f32, f32, f32, f32, Hit)>,
    /// Mouse position (client coordinates; negative = outside).
    mouse: (f32, f32),
    /// Visible body band (zones outside cannot be clicked).
    clip: (f32, f32),
}

impl P<'_> {
    /// True if the mouse is over the rectangle (and inside the visible band). Hover is decided by position, never
    /// by the order of the zones, so buttons appearing on hover cannot shift it.
    fn over(&self, l: f32, t: f32, r: f32, b: f32) -> bool {
        let (x, y) = self.mouse;
        x >= l && x <= r && y >= t && y <= b && y >= self.clip.0 && y <= self.clip.1
    }

    /// Adds a clickable zone (clipped to the visible band).
    fn push(&mut self, l: f32, t: f32, r: f32, b: f32, h: Hit) {
        let (t2, b2) = (t.max(self.clip.0), b.min(self.clip.1));
        if b2 > t2 {
            self.hits.push((l, t2, r, b2, h));
        } else {
            self.hits.push((-1.0, -1.0, -1.0, -1.0, h));
        }
    }

    /// Single-line text vertically centred in [y, y + h].
    fn mid(&self, s: &str, f: Font, x: f32, y: f32, h: f32, max_w: f32, c: Rgba) {
        let fh = self.r.measure("Ag", f, 1000.0).1;
        self.r.text(s, f, x, y + (h - fh) / 2.0, max_w.max(1.0), c);
    }

    /// Button (text, optional icon): `style` 0 = normal, 1 = ghost (gold), 2 = primary. Returns its width.
    #[allow(clippy::too_many_arguments)]
    fn btn(&mut self, x: f32, y: f32, h: f32, icon: Option<&str>, text: &str, style: u8, hit: Hit) -> f32 {
        let k = self.k;
        let (tw, _) = self.r.measure(text, Font::Chip, 400.0 * k);
        let iw = if icon.is_some() { 18.0 * k } else { 0.0 };
        let w = tw + iw + 20.0 * k;
        let hov = self.over(x, y, x + w, y + h);
        let (fill, edge, ink) = match style {
            2 => (if hov { GOLD_HI } else { ACCENT }, (0.0, 0.0, 0.0, 0.0), DARK_INK),
            1 => (if hov { fade(GOLD, 0.22) } else { (0.0, 0.0, 0.0, 0.0) }, fade(GOLD, 0.55), GOLD_HI),
            _ => (if hov { INK700 } else { INK800 }, if hov { fade(GOLD, 0.5) } else { (1.0, 1.0, 1.0, 0.10) }, T200),
        };
        self.r.fill_rrect(x, y, x + w, y + h, 5.0 * k, fill);
        if edge.3 > 0.0 {
            self.r.stroke_rrect(x + 0.5, y + 0.5, x + w - 0.5, y + h - 0.5, 5.0 * k, 1.0, edge);
        }
        if let Some(ic) = icon {
            self.r.text_center(ic, Font::Icon, x + 10.0 * k + iw / 2.0 - 2.0 * k, y + h / 2.0, ink);
        }
        self.mid(text, Font::Chip, x + 10.0 * k + iw, y, h, tw + 2.0, ink);
        self.push(x, y, x + w, y + h, hit);
        w
    }

    /// Width a button would take.
    fn btn_w(&self, icon: bool, text: &str) -> f32 {
        self.r.measure(text, Font::Chip, 400.0 * self.k).0 + if icon { 18.0 * self.k } else { 0.0 } + 20.0 * self.k
    }
}

/// State glyph of a quest (§2.1): done = gold disc with ✓, tracked = its badge, available = hollow ring, locked = dot.
fn glyph(r: &Renderer, st: State, badge: Option<&(String, (f32, f32, f32))>, color: (f32, f32, f32), x: f32, y: f32, k: f32, alpha: f32) {
    match st {
        State::Done => {
            r.fill_circle(x, y, 7.0 * k, fade(GOLD, alpha));
            r.line_round(x - 3.0 * k, y + 0.2 * k, x - 0.8 * k, y + 2.4 * k, 1.7 * k, fade(DARK_INK, alpha));
            r.line_round(x - 0.8 * k, y + 2.4 * k, x + 3.2 * k, y - 2.4 * k, 1.7 * k, fade(DARK_INK, alpha));
        }
        State::Tracked => {
            let (b, c) = badge.cloned().unwrap_or((String::new(), color));
            let c = markers::guard(c);
            r.fill_circle(x, y, 7.5 * k, (c.0, c.1, c.2, alpha));
            r.stroke_circle(x, y, 7.0 * k, 1.2 * k, (1.0, 1.0, 1.0, 0.95 * alpha), false);
            if !b.is_empty() {
                let lum = 255.0 * (0.299 * c.0 + 0.587 * c.1 + 0.114 * c.2);
                r.text_center(&b, Font::Caps, x, y - 0.5, if lum > 120.0 { DARK_INK } else { (1.0, 1.0, 1.0, alpha) });
            }
        }
        State::Available => {
            let q = markers::QUEST_AVAIL;
            r.stroke_circle(x, y, 6.2 * k, 1.7 * k, (q.0, q.1, q.2, alpha), false);
        }
        State::Locked => r.fill_circle(x, y, 2.6 * k, fade(T500, 0.7 * alpha)),
    }
}

impl Journal {
    /// Creates the window centred on `area` (the game's screen rectangle) and shows it.
    pub fn open(s: &'static Strings, d: Data, tab: String, area: (i32, i32, i32, i32)) -> Option<Journal> {
        unsafe {
            let inst = GetModuleHandleW(None).ok()?;
            let class = w!("HungerOverlayJournal");
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: inst.into(),
                lpszClassName: class,
                hIcon: crate::tray::app_icon(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            });
            let area = if area.2 > 0 && area.3 > 0 { area } else { (0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
            let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
            let title = wide(s.j_title);
            let (cx, cy) = (area.0 + area.2 / 2, area.1 + area.3 / 2);
            let hwnd = CreateWindowExW(WS_EX_TOPMOST, class, PCWSTR(title.as_ptr()), style, cx, cy, 100, 100, None, None, inst, None).ok()?;
            let dpi = GetDpiForWindow(hwnd).max(96);
            let k = dpi as f32 / 96.0;
            let w = 800.0 * k;
            let h = (1150.0 * k).min(area.3 as f32 - 80.0 * k).max(500.0 * k);
            let mut rc = RECT { left: 0, top: 0, right: w as i32, bottom: h as i32 };
            let _ = AdjustWindowRectExForDpi(&mut rc, style, false, WS_EX_TOPMOST, dpi);
            let (ww, wh) = (rc.right - rc.left, rc.bottom - rc.top);
            let _ = SetWindowPos(hwnd, None, cx - ww / 2, (cy - wh / 2).max(area.1), ww, wh, SWP_NOZORDER);
            let font = CreateFontW(-((14.0 * k) as i32), 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 5, 0, w!("Segoe UI"));
            let mk_edit = |id: isize| {
                let e = CreateWindowExW(WINDOW_EX_STYLE(0), w!("EDIT"), PCWSTR::null(), WS_CHILD | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32), 0, 0, 10, 10, hwnd, HMENU(id as _), inst, None).ok();
                if let Some(e) = e {
                    SendMessageW(e, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
                    let _ = SetWindowSubclass(e, Some(edit_proc), id as usize, 0);
                    // Limit the ally name to 24 characters (EM_SETLIMITTEXT).
                    if id == 2 {
                        SendMessageW(e, 0x00C5, WPARAM(24), LPARAM(0));
                    }
                }
                e
            };
            let filter = mk_edit(1)?;
            let name = mk_edit(2)?;
            // Grey hint texts inside the fields (EM_SETCUEBANNER).
            for (e, t) in [(filter, s.j_filter), (name, s.al_name)] {
                let wt = wide(t);
                SendMessageW(e, 0x1501, WPARAM(1), LPARAM(wt.as_ptr() as isize));
            }
            let mut r = Renderer::new().ok()?;
            r.set_base_sizes([15.5, 15.5, 14.0, 12.5]);
            let _ = r.set_font_scale(k);
            // Dark title bar (20 = DWMWA_USE_IMMERSIVE_DARK_MODE).
            let dark: i32 = 1;
            let _ = DwmSetWindowAttribute(hwnd, DWMWINDOWATTRIBUTE(20), &dark as *const i32 as *const _, 4);
            let mut j = Journal {
                hwnd,
                filter,
                name,
                font,
                brush: CreateSolidBrush(rgb((0.055, 0.045, 0.032, 1.0))),
                r,
                s,
                k,
                w,
                h,
                d,
                tab: String::new(),
                open: HashSet::new(),
                strip: None,
                repeat: false,
                hist_open: false,
                scroll: 0.0,
                max_scroll: 0.0,
                hits: vec![],
                hover: None,
                query: String::new(),
                focus: None,
                mouse: (-1.0, -1.0),
            };
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            j.goto(&tab);
            Some(j)
        }
    }

    /// New data (after a change, a refresh, the hub list…): repaint.
    pub fn set_data(&mut self, d: Data) {
        self.d = d;
        if !self.d.subjects.iter().any(|s| s.key == self.tab) && self.tab != ADD_TAB {
            self.tab = crate::progress::ME.into();
        }
        self.redraw();
    }

    /// Selects a tab, or opens the player's tab at a quest (card expanded, confirm strip open when it cascades).
    pub fn goto(&mut self, at: &str) {
        if at == ADD_TAB || self.d.subjects.iter().any(|s| s.key == at) {
            self.tab = at.to_string();
        } else {
            self.tab = crate::progress::ME.into();
            if !at.is_empty() {
                let g = self.d.graph.giver(at).to_string();
                self.open.insert(g);
                self.strip = Some(at.to_string());
                self.focus = Some(at.to_string());
            }
        }
        self.place_fields();
        self.redraw();
    }

    /// Requests a repaint.
    fn redraw(&self) {
        unsafe {
            let _ = InvalidateRect(self.hwnd, None, false);
        }
    }

    /// Shows the filter field on subject tabs and the name field on the "+ Ally" tab, at their painted frames.
    fn place_fields(&self) {
        let k = self.k;
        unsafe {
            let add = self.tab == ADD_TAB;
            let _ = ShowWindow(self.filter, if add { SW_HIDE } else { SW_SHOW });
            let _ = ShowWindow(self.name, if add { SW_SHOW } else { SW_HIDE });
            let _ = SetWindowPos(self.filter, None, (52.0 * k) as i32, (70.0 * k) as i32, (self.w - 260.0 * k) as i32, (20.0 * k) as i32, SWP_NOZORDER);
            let _ = SetFocus(if add { self.name } else { self.filter });
        }
    }

    /// Text of a field.
    fn text_of(h: HWND) -> String {
        unsafe {
            let n = GetWindowTextLengthW(h);
            let mut buf = vec![0u16; n as usize + 1];
            let got = GetWindowTextW(h, &mut buf);
            String::from_utf16_lossy(&buf[..got as usize]).trim().to_string()
        }
    }

    /// The filter text changed.
    fn refilter(&mut self) {
        self.query = crate::search::fold(&Self::text_of(self.filter));
        self.scroll = 0.0;
        self.redraw();
    }

    /// Mouse moved: repaint when the zone under it (the top-most one) changes, or always inside the body rows.
    fn mouse(&mut self, x: f32, y: f32) {
        let old = self.mouse;
        self.mouse = (x, y);
        let h = self.hits.iter().rposition(|z| x >= z.0 && x <= z.2 && y >= z.1 && y <= z.3);
        // Row highlights are not zones of their own: repaint when the mouse moves to another 4 px band too.
        if h != self.hover || (old.1 / 4.0).floor() != (y / 4.0).floor() {
            self.hover = h;
            self.redraw();
        }
    }

    /// Wheel: scrolls the body.
    fn wheel(&mut self, delta: i16) {
        self.scroll = (self.scroll - delta as f32 / 120.0 * 84.0 * self.k).clamp(0.0, self.max_scroll);
        self.redraw();
    }

    /// Click.
    fn click(&mut self, x: f32, y: f32) -> Option<Cmd> {
        // The top-most zone wins (buttons are pushed after the row they sit on, popovers last).
        let hit = self.hits.iter().rev().find(|z| x >= z.0 && x <= z.2 && y >= z.1 && y <= z.3).map(|z| z.4.clone())?;
        self.act(hit)
    }

    /// Runs a clicked zone: internal UI changes here, everything else returned to the app.
    fn act(&mut self, hit: Hit) -> Option<Cmd> {
        self.redraw();
        match hit {
            Hit::Tab(t) => {
                self.tab = t.clone();
                self.strip = None;
                self.scroll = 0.0;
                self.place_fields();
                if t == ADD_TAB {
                    return Some(Cmd::FetchHub);
                }
                Some(Cmd::Tab(t))
            }
            Hit::ToggleRepeat => {
                self.repeat = !self.repeat;
                None
            }
            Hit::ToggleCard(g) => {
                if !self.open.remove(&g) {
                    self.open.insert(g);
                }
                None
            }
            Hit::Strip(q) => {
                self.strip = if self.strip.as_deref() == Some(q.as_str()) { None } else { Some(q) };
                None
            }
            Hit::CloseStrip => {
                self.strip = None;
                None
            }
            Hit::Do(label, changes) => {
                self.strip = None;
                Some(Cmd::Apply { subject: self.tab.clone(), label, changes })
            }
            Hit::ToggleHistory => {
                self.hist_open = !self.hist_open;
                None
            }
            Hit::Undo(id) => Some(Cmd::Undo(id)),
            Hit::UndoTo(id) => {
                self.hist_open = false;
                Some(Cmd::UndoTo(self.tab.clone(), id))
            }
            Hit::AddSite(id) => Some(Cmd::AddSiteAlly(id)),
            Hit::AddLocal => {
                let n = Self::text_of(self.name);
                if n.is_empty() {
                    return None;
                }
                unsafe {
                    let _ = SetWindowTextW(self.name, PCWSTR(wide("").as_ptr()));
                }
                Some(Cmd::AddLocalAlly(n))
            }
            Hit::Remove(k) => Some(Cmd::RemoveAlly(k)),
            Hit::Hide(k) => Some(Cmd::HideAlly(k)),
            Hit::Retry => Some(Cmd::FetchHub),
        }
    }

    /// Title of a quest.
    fn title(&self, q: &str) -> String {
        self.d.titles.get(q).cloned().unwrap_or_else(|| q.to_string())
    }

    /// Names of the quests of a change list (up to 4, then "+N").
    fn names(&self, qs: &[String]) -> String {
        let mut v: Vec<String> = qs.iter().take(4).map(|q| self.title(q)).collect();
        if qs.len() > 4 {
            v.push(format!("+{}", qs.len() - 4));
        }
        v.join(", ")
    }

    /// Paints the window.
    fn paint(&mut self, hdc: HDC) {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        if !self.r.begin(rc.right, rc.bottom) {
            return;
        }
        self.h = rc.bottom as f32;
        let k = self.k;
        let (w, h) = (self.w, self.h);
        let pad = 18.0 * k;
        let s = self.s;
        let mut p = P { r: &self.r, k, hits: vec![], mouse: self.mouse, clip: (0.0, h) };
        p.r.fill_rrect(0.0, 0.0, w + 4.0, h + 4.0, 0.0, (INK950.0, INK950.1, INK950.2, 1.0));

        // 1. Tabs: me, the allies, "+ Ally" (while fewer than 2), and the sync state on the right.
        let mut x = pad;
        let ty = 12.0 * k;
        let th = 36.0 * k;
        let n_allies = self.d.subjects.iter().filter(|s| s.kind != Kind::Me).count();
        for sub in self.d.subjects.clone() {
            let sel = self.tab == sub.key;
            let label = if sub.kind == Kind::Me { format!("{}  {}", sub.name, s.j_you) } else { sub.name.clone() };
            let (tw, _) = p.r.measure(&label, Font::Chip, 300.0 * k);
            let tab_w = tw + 52.0 * k;
            let hov = p.over(x, ty, x + tab_w, ty + th);
            p.r.fill_rrect(x, ty, x + tab_w, ty + th, 6.0 * k, if sel || hov { INK700 } else { INK800 });
            p.r.stroke_rrect(x + 0.5, ty + 0.5, x + tab_w - 0.5, ty + th - 0.5, 6.0 * k, 1.0, if sel { fade(GOLD, 0.7) } else { (1.0, 1.0, 1.0, 0.08) });
            if sub.kind == Kind::Me {
                p.r.fill_circle(x + 18.0 * k, ty + th / 2.0, 9.0 * k, GOLD);
                p.r.text_center(&sub.initial, Font::Badge, x + 18.0 * k, ty + th / 2.0 - 0.5, DARK_INK);
            } else {
                markers::draw(p.r, markers::Kind::Ally, sub.color, &sub.initial, x + 18.0 * k, ty + th / 2.0, 0.95 * k, k, 1.0);
            }
            p.mid(&label, Font::Chip, x + 34.0 * k, ty, th, tw + 2.0, if sel { T100 } else { T300 });
            p.push(x, ty, x + tab_w, ty + th, Hit::Tab(sub.key.clone()));
            x += tab_w + 8.0 * k;
        }
        // Raid management, always available ("+ Ally" while there is room, "Manage" once the raid is full).
        {
            let sel = self.tab == ADD_TAB;
            let label = if n_allies < 2 { s.j_add_ally } else { s.p_manage };
            let bw = p.btn_w(true, label);
            if sel {
                p.r.fill_rrect(x - 2.0 * k, ty - 2.0 * k, x + bw + 2.0 * k, ty + th + 2.0 * k, 7.0 * k, fade(GOLD, 0.15));
            }
            p.btn(x, ty, th, Some(IC_ADD_FRIEND), label, 1, Hit::Tab(ADD_TAB.into()));
        }
        let dot = if self.d.synced { OK } else { WARN };
        let (sw, _) = p.r.measure(s.j_synced, Font::Small, 200.0 * k);
        p.r.fill_circle(w - pad - sw - 12.0 * k, ty + th / 2.0, 3.5 * k, (dot.0, dot.1, dot.2, 1.0));
        p.mid(s.j_synced, Font::Small, w - pad - sw, ty, th, sw + 2.0, T500);

        let top = ty + th + 12.0 * k;
        let footer_h = 52.0 * k;
        let mut hits_before = p.hits.len();
        if self.tab == ADD_TAB {
            self.paint_add(&mut p, top, pad);
        } else {
            // 2. Filter field (the EDIT control sits in it) and the "Repeatables" chip.
            let fh = 34.0 * k;
            p.r.fill_rrect(pad, top, w - 200.0 * k, top + fh, 6.0 * k, INK900);
            p.r.stroke_rrect(pad + 0.5, top + 0.5, w - 200.0 * k - 0.5, top + fh - 0.5, 6.0 * k, 1.0, (1.0, 1.0, 1.0, 0.10));
            p.r.text_center(IC_SEARCH, Font::Icon, pad + 17.0 * k, top + fh / 2.0, T500);
            let rx = w - 186.0 * k;
            let rw = p.btn_w(false, s.j_repeatables) + 22.0 * k;
            let hov = p.over(rx, top, rx + rw, top + fh);
            p.r.fill_rrect(rx, top, rx + rw, top + fh, 6.0 * k, if hov { INK700 } else { INK800 });
            p.r.stroke_rrect(rx + 0.5, top + 0.5, rx + rw - 0.5, top + fh - 0.5, 6.0 * k, 1.0, (1.0, 1.0, 1.0, 0.08));
            let (bx, bs) = (rx + 10.0 * k, 13.0 * k);
            let bt = top + (fh - bs) / 2.0;
            if self.repeat {
                p.r.fill_rrect(bx, bt, bx + bs, bt + bs, 3.0 * k, ACCENT);
                p.r.line_round(bx + 3.0 * k, bt + 6.8 * k, bx + 5.6 * k, bt + 9.4 * k, 1.6 * k, DARK_INK);
                p.r.line_round(bx + 5.6 * k, bt + 9.4 * k, bx + 10.2 * k, bt + 3.8 * k, 1.6 * k, DARK_INK);
            } else {
                p.r.stroke_rrect(bx + 0.5, bt + 0.5, bx + bs - 0.5, bt + bs - 0.5, 3.0 * k, 1.2 * k, T300);
            }
            p.mid(s.j_repeatables, Font::Chip, bx + bs + 8.0 * k, top, fh, rw, T200);
            p.push(rx, top, rx + rw, top + fh, Hit::ToggleRepeat);
            let body_top = top + fh + 10.0 * k;
            p.r.line(pad, body_top - 1.0, w - pad, body_top - 1.0, 1.0, (LINE_GOLD.0, LINE_GOLD.1, LINE_GOLD.2, 0.18));
            // 3. Body (scrolls between the fields and the footer).
            let body_bottom = h - footer_h;
            p.clip = (body_top, body_bottom);
            hits_before = p.hits.len();
            p.r.push_clip(0.0, body_top, w, body_bottom);
            let (end, focus_scroll) = self.paint_body(&mut p, body_top + 8.0 * k - self.scroll, pad);
            p.r.pop_clip();
            self.max_scroll = (end + self.scroll - body_bottom + 16.0 * k).max(0.0);
            if let Some(t) = focus_scroll {
                self.focus = None;
                self.scroll = t.min(self.max_scroll);
                self.redraw();
            }
            if self.scroll > self.max_scroll {
                self.scroll = self.max_scroll;
            }
            p.clip = (0.0, h);
            // 4. Footer: last change with Undo, and the history list.
            self.paint_footer(&mut p, body_bottom, pad);
        }
        let _ = hits_before;
        self.hits = p.hits;
        if self.hover.map(|i| i >= self.hits.len()).unwrap_or(false) {
            self.hover = None;
        }
        self.r.present_dc(hdc);
    }

    /// Body of a subject tab: Up next, then one card per character. Returns the bottom y.
    fn paint_body(&self, p: &mut P, y0: f32, pad: f32) -> (f32, Option<f32>) {
        let mut focus_scroll = None;
        let (k, w, s) = (p.k, self.w, self.s);
        let mut y = y0;
        let sub = self.d.subjects.iter().find(|x| x.key == self.tab).cloned();
        let Some(sub) = sub else { return (y, None) };
        let editable = sub.kind != Kind::Site && (sub.kind == Kind::Local || self.d.v12);
        let prog = self.d.progress.get(&sub.key).cloned().unwrap_or_default();
        let g = self.d.graph.clone();
        let q = self.query.clone();
        let matches = |id: &str| q.is_empty() || crate::search::fold(&self.title(id)).contains(&q);

        // Ally header: note, Hide on map, Remove from raid.
        if sub.kind != Kind::Me {
            let note = if sub.kind == Kind::Site { s.j_ally_site_note.replace("{name}", &sub.name) } else { s.j_ally_local_note.to_string() };
            p.mid(&note, Font::Small, pad, y, 28.0 * k, w - 2.0 * pad - 300.0 * k, T500);
            let rw = p.btn_w(false, s.a_remove_ally);
            let hw = p.btn_w(true, s.a_hide_map);
            let bx = w - pad - rw;
            p.btn(bx, y, 26.0 * k, None, s.a_remove_ally, 0, Hit::Remove(sub.key.clone()));
            p.btn(bx - 8.0 * k - hw, y, 26.0 * k, Some(IC_HIDDEN), s.a_hide_map, if sub.hidden { 1 } else { 0 }, Hit::Hide(sub.key.clone()));
            y += 38.0 * k;
        }
        if !g.has_chains() {
            p.mid(s.toast_needs_site, Font::Chip, pad, y, 30.0 * k, w - 2.0 * pad, WARN_TEXT);
            return (y + 40.0 * k, None);
        }

        // UP NEXT (hidden while filtering).
        if q.is_empty() {
            let (tw, th) = p.r.measure_spaced(s.j_up_next, Font::Section, 300.0 * k, 1.8 * k);
            p.r.text_spaced(s.j_up_next, Font::Section, pad, y + (26.0 * k - th) / 2.0, tw + 4.0, 1.8 * k, GOLD);
            p.mid(s.j_up_next_hint, Font::Small, pad + tw + 16.0 * k, y, 26.0 * k, 400.0 * k, T500);
            y += 30.0 * k;
            for (giver, next) in g.suggestions(&prog) {
                let rh = 36.0 * k;
                let (qid, st) = match &next {
                    Next::Tracked(q) => (q.clone(), State::Tracked),
                    Next::Available(q) => (q.clone(), State::Available),
                    Next::Waiting(q, _) => (q.clone(), State::Locked),
                };
                if p.over(pad - 6.0 * k, y, w - pad + 6.0 * k, y + rh) {
                    p.r.fill_rrect(pad - 6.0 * k, y, w - pad + 6.0 * k, y + rh, 6.0 * k, HOVER_ROW);
                }
                p.push(pad - 6.0 * k, y, pad + 300.0 * k, y + rh, Hit::Strip(qid.clone()));
                p.mid(&giver, Font::Chip, pad, y, rh, 100.0 * k, T300);
                glyph(p.r, st, self.d.badges.get(&qid).filter(|_| sub.kind == Kind::Me), sub.color, pad + 116.0 * k, y + rh / 2.0, k, 1.0);
                let title = self.title(&qid);
                let font = if st == State::Tracked { Font::Name } else { Font::Chip };
                let (tw, _) = p.r.measure(&title, font, 300.0 * k);
                p.mid(&title, font, pad + 132.0 * k, y, rh, 300.0 * k, if st == State::Locked { T300 } else { T100 });
                let step_w = match g.step(&qid) {
                    Some((n, t)) => {
                        let st = format!("{n}/{t}");
                        p.mid(&st, Font::Small, pad + 140.0 * k + tw.min(300.0 * k), y, rh, 60.0 * k, T500);
                        p.r.measure(&st, Font::Small, 100.0 * k).0 + 8.0 * k
                    }
                    None => 0.0,
                };
                let after_title = pad + 140.0 * k + tw.min(300.0 * k) + step_w;
                // Actions, right-aligned.
                let mut bx = w - pad;
                let bh = 26.0 * k;
                let by = y + (rh - bh) / 2.0;
                let done_label = s.toast_done.replace("{quest}", &title);
                match &next {
                    Next::Tracked(_) => {
                        if editable {
                            bx -= p.btn_w(true, s.a_done);
                            p.btn(bx, by, bh, Some(IC_CHECK), s.a_done, 0, Hit::Do(done_label, g.plan_done(&prog, &qid)));
                        }
                        let (iw, _) = p.r.measure(s.j_in_progress, Font::Small, 200.0 * k);
                        p.mid(s.j_in_progress, Font::Small, bx - iw - 12.0 * k, y, rh, iw + 2.0, T500);
                    }
                    Next::Available(_) if editable => {
                        bx -= p.btn_w(true, s.a_already_done);
                        p.btn(bx, by, bh, Some(IC_CHECK), s.a_already_done, 0, Hit::Do(done_label, g.plan_done(&prog, &qid)));
                        bx -= p.btn_w(false, &format!("+ {}", s.a_track)) + 8.0 * k;
                        p.btn(bx, by, bh, None, &format!("+ {}", s.a_track), 1, Hit::Do(format!("{} {title}", s.toast_tracking), g.plan_track(&prog, &qid)));
                    }
                    Next::Waiting(_, b) => {
                        if editable {
                            bx -= p.btn_w(true, s.a_already_done);
                            p.btn(bx, by, bh, Some(IC_CHECK), s.a_already_done, 0, Hit::Strip(qid.clone()));
                        }
                        let t = s.j_waiting.replace("{giver}", g.giver(b)).replace("{quest}", &self.title(b));
                        p.mid(&t, Font::Small, after_title, y, rh, (bx - after_title - 8.0 * k).max(10.0), T500);
                    }
                    _ => {}
                }
                y += rh;
                if self.strip.as_deref() == Some(qid.as_str()) {
                    y = self.paint_strip(p, y, pad, &sub, &prog, &qid, editable);
                }
            }
            // Repeatables available.
            let reps: Vec<String> = g.givers().iter().flat_map(|gv| g.repeatables_of(gv)).filter(|r| g.state(&prog, r) == State::Available).collect();
            if !reps.is_empty() {
                let t = format!("{} · {}", s.j_repeat_avail.replace("{n}", &reps.len().to_string()), if self.repeat { "▴" } else { "▾" });
                let (tw, _) = p.r.measure(&t, Font::Small, 400.0 * k);
                let hv = p.over(pad + 128.0 * k, y, pad + 136.0 * k + tw, y + 26.0 * k);
                p.mid(&t, Font::Small, pad + 132.0 * k, y, 26.0 * k, tw + 2.0, if hv { GOLD_HI } else { GOLD });
                p.push(pad + 128.0 * k, y, pad + 136.0 * k + tw, y + 26.0 * k, Hit::ToggleRepeat);
                y += 30.0 * k;
            }
            p.r.line(pad, y + 4.0 * k, w - pad, y + 4.0 * k, 1.0, (LINE_GOLD.0, LINE_GOLD.1, LINE_GOLD.2, 0.18));
            y += 14.0 * k;
        }

        // CHARACTERS: header with the stepper, expanded rows.
        let (tw, th) = p.r.measure_spaced(s.j_characters, Font::Section, 300.0 * k, 1.8 * k);
        p.r.text_spaced(s.j_characters, Font::Section, pad, y + (26.0 * k - th) / 2.0, tw + 4.0, 1.8 * k, GOLD);
        let (hw, _) = p.r.measure(s.j_click_step, Font::Small, 300.0 * k);
        p.mid(s.j_click_step, Font::Small, w - pad - hw, y, 26.0 * k, hw + 2.0, T500);
        y += 32.0 * k;
        for giver in g.givers().to_vec() {
            let story = g.story(&giver);
            let reps = g.repeatables_of(&giver);
            let shown: Vec<String> = story.iter().chain(if self.repeat || !q.is_empty() { reps.iter() } else { [].iter() }).filter(|x| matches(x)).cloned().collect();
            if !q.is_empty() && shown.is_empty() {
                continue;
            }
            let open = self.open.contains(&giver) || !q.is_empty();
            // Header (40): chevron + name, stepper, "n / N done".
            let hh = 40.0 * k;
            if p.over(pad - 6.0 * k, y, pad + 170.0 * k, y + hh) {
                p.r.fill_rrect(pad - 6.0 * k, y, pad + 170.0 * k, y + hh, 6.0 * k, HOVER_ROW);
            }
            p.push(pad - 6.0 * k, y, pad + 170.0 * k, y + hh, Hit::ToggleCard(giver.clone()));
            p.r.text_center(if open { IC_DOWN } else { IC_RIGHT }, Font::Icon, pad + 6.0 * k, y + hh / 2.0, T500);
            let up = giver.to_uppercase();
            let (nw, nh) = p.r.measure_spaced(&up, Font::Section, 150.0 * k, 1.5 * k);
            p.r.text_spaced(&up, Font::Section, pad + 20.0 * k, y + (hh - nh) / 2.0, nw + 4.0, 1.5 * k, T100);
            let done_n = story.iter().filter(|x| prog.done.contains(*x)).count();
            let cnt = s.j_n_done.replace("{n}", &done_n.to_string()).replace("{t}", &story.len().to_string());
            let (cw, _) = p.r.measure(&cnt, Font::Small, 200.0 * k);
            p.mid(&cnt, Font::Small, w - pad - cw, y, hh, cw + 2.0, T500);
            // Stepper: every story quest of the character, connected; a click opens "I am here".
            let sx0 = pad + 190.0 * k;
            let pitch = ((w - pad - cw - 30.0 * k - sx0) / story.len().max(1) as f32).min(26.0 * k);
            let cy = y + hh / 2.0;
            for (i, qid) in story.iter().enumerate() {
                let x = sx0 + i as f32 * pitch;
                if i > 0 {
                    let both = prog.done.contains(qid) && prog.done.contains(&story[i - 1]);
                    p.r.line(x - pitch + 7.0 * k, cy, x - 7.0 * k, cy, if both { 1.4 * k } else { 1.0 * k }, if both { fade(GOLD, 0.6) } else { fade(T500, 0.35) });
                }
                let st = g.state(&prog, qid);
                let sel = self.strip.as_deref() == Some(qid.as_str());
                if p.over(x - pitch / 2.0, y + 6.0 * k, x + pitch / 2.0, y + hh - 6.0 * k) || sel {
                    p.r.stroke_circle(x, cy, 10.5 * k, 2.0 * k, GOLD_HI, false);
                }
                glyph(p.r, st, self.d.badges.get(qid).filter(|_| sub.kind == Kind::Me), sub.color, x, cy, k * 0.9, 1.0);
                p.push(x - pitch / 2.0, y + 6.0 * k, x + pitch / 2.0, y + hh - 6.0 * k, Hit::Strip(qid.clone()));
            }
            y += hh;
            if self.strip.as_ref().map(|x| story.contains(x) && !open).unwrap_or(false) {
                let qid = self.strip.clone().unwrap();
                y = self.paint_strip(p, y, pad, &sub, &prog, &qid, editable);
            }
            if open {
                if story.is_empty() && reps.is_empty() {
                    p.mid(s.j_no_order, Font::Small, pad + 30.0 * k, y, 26.0 * k, 300.0 * k, T500);
                    y += 28.0 * k;
                }
                let lines = g.lines_of(&giver);
                let mut prev_line: Option<usize> = None;
                for qid in &shown {
                    // Line separator ("Line 2 starts after …") and side steps.
                    let li = lines.iter().position(|l| l.contains(qid));
                    if li.is_some() && li != prev_line && li.unwrap_or(0) > 0 {
                        let first = &lines[li.unwrap()][0];
                        if let Some(b) = g.requires(first).iter().find(|r| !lines[li.unwrap()].contains(r)) {
                            let t = s.j_line_starts.replace("{n}", &(li.unwrap() + 1).to_string()).replace("{giver}", g.giver(b)).replace("{quest}", &self.title(b));
                            p.r.text_center("\u{E72E}", Font::Icon, pad + 38.0 * k, y + 13.0 * k, T500);
                            p.mid(&t, Font::Small, pad + 52.0 * k, y, 26.0 * k, w - 2.0 * pad - 60.0 * k, T500);
                            y += 26.0 * k;
                        }
                    }
                    prev_line = li;
                    y = self.paint_row(p, y, pad, &sub, &prog, qid, editable, li.is_none() && !g.is_repeatable(qid));
                    if self.strip.as_deref() == Some(qid.as_str()) {
                        y = self.paint_strip(p, y, pad, &sub, &prog, qid, editable);
                    }
                    if self.focus.as_deref() == Some(qid.as_str()) {
                        // Bring the focused quest into view (once, see `paint`).
                        focus_scroll = Some((y - y0 - 200.0 * k).max(0.0));
                    }
                }
                if !self.repeat && q.is_empty() && !reps.is_empty() {
                    let count = |pre: &str| reps.iter().filter(|r| r.rsplit('.').next().map(|x| x.starts_with(pre)).unwrap_or(false)).count();
                    let t = format!("{} · {}", s.j_repeat_sum.replace("{d}", &count("DQ").to_string()).replace("{w}", &count("WQ").to_string()).replace("{m}", &count("MQ").to_string()), "▾");
                    let (tw, _) = p.r.measure(&t, Font::Small, 500.0 * k);
                    let hv = p.over(pad + 48.0 * k, y, pad + 56.0 * k + tw, y + 26.0 * k);
                    p.mid(&t, Font::Small, pad + 52.0 * k, y, 26.0 * k, tw + 2.0, if hv { GOLD_HI } else { T500 });
                    p.push(pad + 48.0 * k, y, pad + 56.0 * k + tw, y + 26.0 * k, Hit::ToggleRepeat);
                    y += 28.0 * k;
                }
                y += 6.0 * k;
            }
            p.r.line(pad, y + 1.0, w - pad, y + 1.0, 1.0, (LINE_GOLD.0, LINE_GOLD.1, LINE_GOLD.2, 0.10));
            y += 4.0 * k;
        }
        (y, focus_scroll)
    }

    /// One quest row of a card (28): glyph, title, note, actions. Returns the new y.
    #[allow(clippy::too_many_arguments)]
    fn paint_row(&self, p: &mut P, y: f32, pad: f32, sub: &Subject, prog: &Progress, qid: &str, editable: bool, side: bool) -> f32 {
        let (k, w, s) = (p.k, self.w, self.s);
        let g = &self.d.graph;
        let rh = 28.0 * k;
        let st = g.state(prog, qid);
        let rep = g.is_repeatable(qid);
        let title = self.title(qid);
        let hov_row = p.over(pad + 24.0 * k, y, w - pad + 4.0 * k, y + rh);
        if hov_row {
            p.r.fill_rrect(pad + 24.0 * k, y, w - pad + 4.0 * k, y + rh, 5.0 * k, HOVER_ROW);
        }
        glyph(p.r, st, self.d.badges.get(qid).filter(|_| sub.kind == Kind::Me), sub.color, pad + 38.0 * k, y + rh / 2.0, k * 0.85, 1.0);
        let ink = match st {
            State::Done => T300,
            State::Locked => T500,
            _ => T100,
        };
        let (tw, _) = p.r.measure(&title, Font::Chip, 360.0 * k);
        p.mid(&title, Font::Chip, pad + 52.0 * k, y, rh, 360.0 * k, ink);
        // Note: tracked / after … / side step / inconsistent.
        let note = if g.inconsistent(prog, qid) {
            Some((s.j_inconsistent.to_string(), WARN_TEXT))
        } else if st == State::Tracked {
            Some((s.s_tracked.to_lowercase(), T500))
        } else if st == State::Locked {
            let req = g.requires(qid);
            req.first().map(|b| {
                if side {
                    (s.j_side_step.replace("{giver}", g.giver(b)).replace("{quest}", &self.title(b)), T500)
                } else {
                    (s.j_after.replace("{quest}", &self.title(b)), T500)
                }
            })
        } else {
            None
        };
        // Actions (shown on hover, always for the tracked row), right-aligned.
        let mut bx = w - pad;
        let bh = 24.0 * k;
        let by = y + (rh - bh) / 2.0;
        let show = editable && (hov_row || st == State::Tracked || g.inconsistent(prog, qid));
        let mut acts: Vec<(Option<&str>, String, u8, Hit)> = vec![];
        if editable {
            let done_label = s.toast_done.replace("{quest}", &title);
            let one_or_strip = |label: String, plan: Vec<Change>| if g.cascades(&plan, qid) { Hit::Strip(qid.to_string()) } else { Hit::Do(label, plan) };
            match st {
                State::Done => acts.push((None, s.a_reopen.into(), 0, one_or_strip(s.toast_reopened.replace("{quest}", &title), g.plan_reopen(prog, qid)))),
                State::Tracked => {
                    if rep {
                        acts.push((Some(IC_CHECK), s.a_finished.into(), 0, Hit::Do(s.toast_finished.replace("{quest}", &title), g.plan_untrack(prog, qid))));
                    } else {
                        acts.push((Some(IC_CHECK), s.a_done.into(), 0, one_or_strip(done_label, g.plan_done(prog, qid))));
                        acts.push((None, s.a_untrack.into(), 0, Hit::Do(format!("{} {title}", s.toast_undone), g.plan_untrack(prog, qid))));
                    }
                }
                State::Available => {
                    if !rep {
                        acts.push((Some(IC_CHECK), s.a_done.into(), 0, Hit::Do(done_label, g.plan_done(prog, qid))));
                    }
                    acts.push((None, format!("+ {}", s.a_track), 1, Hit::Do(format!("{} {title}", s.toast_tracking), g.plan_track(prog, qid))));
                }
                State::Locked => {
                    if !rep {
                        acts.push((Some(IC_CHECK), s.a_done_up_to.into(), 0, Hit::Strip(qid.to_string())));
                    }
                    acts.push((None, format!("+ {}", s.a_track_here), 1, Hit::Strip(qid.to_string())));
                }
            }
            if g.inconsistent(prog, qid) {
                acts.push((Some(IC_WARN), s.a_fix.into(), 1, Hit::Strip(qid.to_string())));
            }
        }
        // Row zone first (the whole band), then the buttons on top of it.
        p.push(pad + 24.0 * k, y, w - pad + 4.0 * k, y + rh, Hit::Strip(qid.to_string()));
        if show {
            for (ic, label, style, hit) in acts {
                bx -= p.btn_w(ic.is_some(), &label);
                p.btn(bx, by, bh, ic, &label, style, hit);
                bx -= 6.0 * k;
            }
        }
        if let Some((n, c)) = note {
            p.mid(&n, Font::Small, pad + 60.0 * k + tw, y, rh, (bx - pad - 70.0 * k - tw).max(10.0), c);
        }
        if let Some(n) = self.d.spots.get(qid).filter(|n| **n > 0) {
            if !show {
                let t = s.p_spots.replace("{n}", &n.to_string());
                let (cw, _) = p.r.measure(&t, Font::Small, 200.0 * k);
                p.mid(&t, Font::Small, w - pad - cw, y, rh, cw + 2.0, T500);
            }
        }
        y + rh
    }

    /// Confirm strip of a quest (§3.2): what "I am here" / "Done up to here" / "Reopen" will change. Returns the new y.
    #[allow(clippy::too_many_arguments)]
    fn paint_strip(&self, p: &mut P, y: f32, pad: f32, sub: &Subject, prog: &Progress, qid: &str, editable: bool) -> f32 {
        let (k, w, s) = (p.k, self.w, self.s);
        let g = &self.d.graph;
        let st = g.state(prog, qid);
        let title = self.title(qid);
        let x0 = pad + 10.0 * k;
        let x1 = w - pad - 10.0 * k;
        let y0 = y + 4.0 * k;
        // Plans and the sentence.
        let (sentence, buttons): (String, Vec<(Option<&str>, String, u8, Hit)>) = if st == State::Done {
            let plan = g.plan_reopen(prog, qid);
            let others: Vec<String> = plan.iter().skip(1).map(|c| c.0.clone()).collect();
            let sent = if others.is_empty() { String::new() } else { format!("{} {}", s.j_cascade_reopen.replace("{n}", &others.len().to_string()), self.names(&others)) };
            (sent, vec![(None, s.a_reopen.into(), 2, Hit::Do(s.toast_reopened.replace("{quest}", &title), plan))])
        } else {
            let here = g.plan_here(prog, qid);
            let up: Vec<String> = here.iter().filter(|c| c.1 == Write::Done).map(|c| c.0.clone()).collect();
            let reset: Vec<String> = here.iter().filter(|c| c.1 == Write::None).map(|c| c.0.clone()).collect();
            let mut sent = if up.is_empty() { String::new() } else { format!("{} {}", s.j_cascade_done.replace("{n}", &up.len().to_string()), self.names(&up)) };
            if !reset.is_empty() {
                sent += &format!("  {} {}", s.j_cascade_reopen.replace("{n}", &reset.len().to_string()), self.names(&reset));
            }
            let mut b = vec![(None, format!("+ {}", s.a_track_here), 2, Hit::Do(format!("{} {title}", s.toast_tracking), here))];
            if !g.is_repeatable(qid) {
                let done = g.plan_done(prog, qid);
                let n = done.len().saturating_sub(1);
                let label = if n > 0 { s.toast_done_n.replace("{quest}", &title).replace("{n}", &n.to_string()) } else { s.toast_done.replace("{quest}", &title) };
                b.push((Some(IC_CHECK), s.a_done_up_to.into(), 0, Hit::Do(label, done)));
            }
            (sent, b)
        };
        let tw = x1 - x0 - 24.0 * k;
        let (_, sh) = if sentence.is_empty() { (0.0, 0.0) } else { p.r.measure(&sentence, Font::Small, tw) };
        let h = 30.0 * k + sh + if editable { 40.0 * k } else { 8.0 * k };
        p.r.fill_rrect(x0, y0, x1, y0 + h, 6.0 * k, fade(GOLD, 0.07));
        p.r.stroke_rrect(x0 + 0.5, y0 + 0.5, x1 - 0.5, y0 + h - 0.5, 6.0 * k, 1.0, fade(GOLD, 0.45));
        let (nw, _) = p.r.measure(&title, Font::Name, tw);
        p.mid(&title, Font::Name, x0 + 12.0 * k, y0 + 4.0 * k, 24.0 * k, tw, T100);
        if let Some((n, t)) = g.step(qid) {
            let st = s.j_step.replace("{giver}", g.giver(qid)).replace("{n}", &n.to_string()).replace("{t}", &t.to_string());
            p.mid(&st, Font::Small, x0 + 20.0 * k + nw, y0 + 4.0 * k, 24.0 * k, tw - nw, T500);
        }
        if !sentence.is_empty() {
            p.r.text(&sentence, Font::Small, x0 + 12.0 * k, y0 + 30.0 * k, tw, T200);
        }
        if editable {
            let by = y0 + 30.0 * k + sh + 8.0 * k;
            let mut bx = x0 + 12.0 * k;
            for (ic, label, style, hit) in buttons {
                bx += p.btn(bx, by, 26.0 * k, ic, &label, style, hit) + 8.0 * k;
            }
            p.btn(bx, by, 26.0 * k, None, s.cancel, 0, Hit::CloseStrip);
            let (uw, _) = p.r.measure(s.j_undo_note, Font::Small, 300.0 * k);
            p.mid(s.j_undo_note, Font::Small, x1 - uw - 12.0 * k, by, 26.0 * k, uw + 2.0, T500);
        }
        let _ = sub;
        y0 + h + 6.0 * k
    }

    /// Footer: last change with Undo, History list (popover above the footer).
    fn paint_footer(&self, p: &mut P, top: f32, pad: f32) {
        let (k, w, s) = (p.k, self.w, self.s);
        p.r.fill_rrect(0.0, top, w + 4.0, self.h + 4.0, 0.0, (INK950.0, INK950.1, INK950.2, 1.0));
        p.r.line(pad, top, w - pad, top, 1.0, (LINE_GOLD.0, LINE_GOLD.1, LINE_GOLD.2, 0.25));
        let hist = self.d.history.get(&self.tab).cloned().unwrap_or_default();
        let fh = 52.0 * k;
        if let Some((id, label, age, conflicts)) = hist.first().cloned() {
            let t = format!("{} {label} · {}", s.j_last_change, ago(age));
            let mut bx = w - pad;
            let hw = p.btn_w(false, &format!("{} ⌄", s.j_history));
            bx -= hw;
            p.btn(bx, top + 13.0 * k, 26.0 * k, None, &format!("{} ⌄", s.j_history), 0, Hit::ToggleHistory);
            let label_u = if conflicts.is_empty() { s.toast_undo } else { s.j_undo_anyway };
            bx -= p.btn_w(false, label_u) + 8.0 * k;
            p.btn(bx, top + 13.0 * k, 26.0 * k, None, label_u, 1, Hit::Undo(id));
            p.r.text_center("\u{E81C}", Font::Icon, pad + 8.0 * k, top + fh / 2.0, T500);
            let line = if conflicts.is_empty() { t } else { s.j_conflict.replace("{list}", &self.names(&conflicts)) };
            p.mid(&line, Font::Chip, pad + 24.0 * k, top, fh, bx - pad - 32.0 * k, if conflicts.is_empty() { T300 } else { WARN_TEXT });
            // History popover.
            if self.hist_open {
                let ih = 30.0 * k;
                let lh = ih * hist.len() as f32 + 8.0 * k;
                let (lx, ly) = (w - pad - 520.0 * k, top - lh - 6.0 * k);
                p.r.fill_rrect(lx + 2.0 * k, ly + 3.0 * k, w - pad + 2.0 * k, ly + lh + 3.0 * k, 7.0 * k, (0.0, 0.0, 0.0, 0.4));
                p.r.fill_rrect(lx, ly, w - pad, ly + lh, 7.0 * k, (INK950.0, INK950.1, INK950.2, 0.99));
                p.r.stroke_rrect(lx + 0.5, ly + 0.5, w - pad - 0.5, ly + lh - 0.5, 7.0 * k, 1.0, fade(GOLD, 0.45));
                let mut iy = ly + 4.0 * k;
                for (hid, hl, hage, _) in hist {
                    let uw = p.btn_w(false, s.j_undo_to);
                    p.btn(w - pad - uw - 8.0 * k, iy + 3.0 * k, 24.0 * k, None, s.j_undo_to, 0, Hit::UndoTo(hid));
                    p.mid(&format!("{hl} · {}", ago(hage)), Font::Small, lx + 14.0 * k, iy, ih, 520.0 * k - uw - 40.0 * k, T200);
                    iy += ih;
                }
            }
        } else {
            p.r.text_center("\u{E81C}", Font::Icon, pad + 8.0 * k, top + fh / 2.0, T500);
        }
    }

    /// "+ Ally" tab: hub members (website) and a local ally by name.
    fn paint_add(&self, p: &mut P, top: f32, pad: f32) {
        let (k, w, s) = (p.k, self.w, self.s);
        let n = self.d.subjects.iter().filter(|x| x.kind != Kind::Me).count();
        let full = n >= 2;
        let mut y = top;
        let (tw, th) = p.r.measure_spaced(s.al_title, Font::Section, 300.0 * k, 1.8 * k);
        p.r.text_spaced(s.al_title, Font::Section, pad, y + (26.0 * k - th) / 2.0, tw + 4.0, 1.8 * k, GOLD);
        let cnt = s.al_count.replace("{n}", &n.to_string());
        let (cw, _) = p.r.measure(&cnt, Font::Small, 200.0 * k);
        p.mid(&cnt, Font::Small, w - pad - cw, y, 26.0 * k, cw + 2.0, T500);
        y += 28.0 * k;
        y += p.r.text(s.al_help, Font::Small, pad, y, w - 2.0 * pad, T300) + 14.0 * k;
        // 0. Current raid allies, each with "Remove from raid".
        for a in self.d.subjects.iter().filter(|x| x.kind != Kind::Me) {
            let rh = 36.0 * k;
            markers::draw(p.r, markers::Kind::Ally, a.color, &a.initial, pad + 12.0 * k, y + rh / 2.0, k, k, 1.0);
            let (nw, _) = p.r.measure(&a.name, Font::Name, 300.0 * k);
            p.mid(&a.name, Font::Name, pad + 30.0 * k, y, rh, 300.0 * k, T100);
            let tag = if a.kind == Kind::Site { s.tag_site } else { s.tag_local };
            p.r.text_center(if a.kind == Kind::Site { IC_CLOUD } else { IC_PC }, Font::Icon, pad + 42.0 * k + nw, y + rh / 2.0, T500);
            p.mid(tag, Font::Caps, pad + 54.0 * k + nw, y, rh, 100.0 * k, T500);
            let bh = 26.0 * k;
            let rw = p.btn_w(false, s.a_remove_ally);
            p.btn(w - pad - rw, y + (rh - bh) / 2.0, bh, None, s.a_remove_ally, 0, Hit::Remove(a.key.clone()));
            let hw = p.btn_w(true, s.a_hide_map);
            p.btn(w - pad - rw - 8.0 * k - hw, y + (rh - bh) / 2.0, bh, Some(IC_HIDDEN), s.a_hide_map, if a.hidden { 1 } else { 0 }, Hit::Hide(a.key.clone()));
            y += rh;
        }
        if n > 0 {
            y += 12.0 * k;
        }
        // 1. From the hub.
        p.r.text_center(IC_CLOUD, Font::Icon, pad + 8.0 * k, y + 13.0 * k, T300);
        p.mid(s.al_hub, Font::Name, pad + 24.0 * k, y, 26.0 * k, w - 2.0 * pad, T100);
        y += 32.0 * k;
        match &self.d.hub {
            None => {
                p.mid(s.search_loading, Font::Small, pad + 24.0 * k, y, 26.0 * k, 400.0 * k, T500);
                y += 30.0 * k;
            }
            Some(Err(e)) => {
                let msg = if *e == "unsupported" { s.toast_needs_site } else { s.toast_write_failed };
                p.mid(msg, Font::Small, pad + 24.0 * k, y, 26.0 * k, w - 200.0 * k, WARN_TEXT);
                p.btn(w - pad - 120.0 * k, y, 26.0 * k, None, "↻", 0, Hit::Retry);
                y += 32.0 * k;
            }
            Some(Ok(list)) => {
                if list.is_empty() {
                    p.mid(s.p_allies_empty, Font::Small, pad + 24.0 * k, y, 26.0 * k, w - 2.0 * pad - 30.0 * k, T500);
                    y += 30.0 * k;
                }
                for e in list {
                    let rh = 36.0 * k;
                    // The current raid decides (the hub list may predate the last change).
                    let added = self.d.subjects.iter().any(|x| x.key == e.id);
                    p.mid(&e.name, Font::Name, pad + 24.0 * k, y, rh, 300.0 * k, T100);
                    let rel = match &e.hub {
                        Some(h) => format!("hub · {h}"),
                        None => e.relation.clone(),
                    };
                    let rel = if e.has_account == Some(false) { format!("{rel} · {}", s.al_no_account) } else { rel };
                    p.mid(&rel, Font::Small, pad + 260.0 * k, y, rh, 260.0 * k, T500);
                    let bh = 26.0 * k;
                    if added {
                        let bw = p.btn_w(true, s.al_added);
                        p.btn(w - pad - bw, y + (rh - bh) / 2.0, bh, Some(IC_CHECK), s.al_added, 0, Hit::Remove(e.id.clone()));
                    } else if full {
                        let (fw, _) = p.r.measure(s.al_full, Font::Small, 200.0 * k);
                        p.mid(s.al_full, Font::Small, w - pad - fw, y, rh, fw + 2.0, T500);
                    } else {
                        let bw = p.btn_w(false, &format!("+ {}", s.al_add));
                        p.btn(w - pad - bw, y + (rh - bh) / 2.0, bh, None, &format!("+ {}", s.al_add), 1, Hit::AddSite(e.id.clone()));
                    }
                    y += rh;
                }
            }
        }
        y += 16.0 * k;
        // 2. Someone without the app or an account (name field + Add).
        p.r.text_center(IC_PC, Font::Icon, pad + 8.0 * k, y + 13.0 * k, T300);
        p.mid(s.al_manual, Font::Name, pad + 24.0 * k, y, 26.0 * k, w - 2.0 * pad, T100);
        y += 32.0 * k;
        let fx1 = w - pad - 140.0 * k;
        p.r.fill_rrect(pad + 24.0 * k, y, fx1, y + 34.0 * k, 6.0 * k, INK900);
        p.r.stroke_rrect(pad + 24.5 * k, y + 0.5, fx1 - 0.5, y + 34.0 * k - 0.5, 6.0 * k, 1.0, (1.0, 1.0, 1.0, 0.12));
        unsafe {
            let _ = SetWindowPos(self.name, None, (pad + 36.0 * k) as i32, (y + 7.0 * k) as i32, (fx1 - pad - 48.0 * k) as i32, (20.0 * k) as i32, SWP_NOZORDER);
        }
        if full {
            let (fw, _) = p.r.measure(s.al_full, Font::Small, 200.0 * k);
            p.mid(s.al_full, Font::Small, w - pad - fw, y, 34.0 * k, fw + 2.0, T500);
        } else {
            p.btn(fx1 + 12.0 * k, y + 4.0 * k, 26.0 * k, None, &format!("+ {}", s.al_add), 2, Hit::AddLocal);
        }
        y += 42.0 * k;
        p.r.text(s.al_manual_help, Font::Small, pad + 24.0 * k, y, w - 2.0 * pad - 30.0 * k, T500);
    }

    /// Destroys the window.
    pub fn close(self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            let _ = DeleteObject(HGDIOBJ(self.font.0));
            let _ = DeleteObject(HGDIOBJ(self.brush.0));
        }
    }
}

/// "3 min", "2 h", "1 d" ago.
fn ago(secs: u64) -> String {
    if secs < 60 {
        "< 1 min".into()
    } else if secs < 3600 {
        format!("{} min", secs / 60)
    } else if secs < 86400 {
        format!("{} h", secs / 3600)
    } else {
        format!("{} d", secs / 86400)
    }
}
