//! Hunger Overlay: shows the Hunger Companion quest-tracker objectives on top of the in-game map.
//!
//! Principle: the game is never read from or modified (no memory access, no file access, no injection,
//! no simulated input). The app watches for the map key, confirms that the map is open by looking at a few
//! screen pixels (BitBlt screen capture), identifies which map it is, then draws the markers in a transparent,
//! click-through window placed on top of the game. Zoom and panning follow the mouse wheel and mouse drags,
//! observed passively through Raw Input (events are never consumed or blocked)
//! (measured model: docs/overlay/MAP_UI_MEASUREMENTS.md).
//!
//! Network: the app only talks to the Hunger Companion website, over HTTPS (see `net.rs` for the exact list of
//! requests). The only user data sent is the user's API key (in the Authorization header) and, when the user
//! picks a quest in the search window, that quest's id to (un)track it. No screen captures, input or game data
//! ever leave the computer.

#![windows_subsystem = "windows"]

mod capture;
mod config;
mod detect;
mod i18n;
mod journal;
mod log;
mod markers;
mod model;
mod net;
mod panel;
mod progress;
mod quests;
mod render;
mod refs;
mod resync;
mod scene;
mod search;
mod settings;
mod tray;
mod update;

use config::Config;
use detect::{PanelSig, Square};
use model::{DrawPoint, LegendQuest};
use net::{Req, SharedRef, Status};
use render::{Font, Renderer, Rgba};
use scene::Spot;
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    CreateMutexW, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RIDEV_INPUTSINK,
    RID_INPUT, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Window message posted by the system-tray icon.
const WM_TRAY: u32 = WM_APP + 1;
/// Window message posted by the network thread when new data / a new status is available.
const WM_NET: u32 = WM_APP + 2;
/// Main periodic timer (map detection, zoom animation, redraw).
const TIMER_TICK: usize = 1;
/// Fires once, 20 s after startup: give back the memory used by the initial load (JSON, decoding…).
const TIMER_TRIM: usize = 2;
/// Executable name of the game; used only to recognise the game window (process name check, nothing else).
const GAME_EXE: &str = "projectrlh-win64-shipping.exe";

/// Measured zoom law: 11 levels, factor 1 + 0.5·n, exponential animation (k ≈ 5/s).
const ZOOM_MAX_N: i32 = 10;
/// Speed of the exponential zoom animation (per second).
const ZOOM_K: f64 = 5.0;
/// Visual probe (map closed): 4 times per second, 8 times per second right after the map key.
const PROBE_EVERY: Duration = Duration::from_millis(250);
/// Faster probe interval used right after the map key was pressed.
const PROBE_FAST: Duration = Duration::from_millis(120);
/// How often the help panel is checked while the map is open (to detect that it was closed).
const PANEL_EVERY: Duration = Duration::from_millis(150);
/// The help-panel signature is taken after the opening fade-in (≈ 190 ms).
const PANEL_SIG_DELAY: Duration = Duration::from_millis(260);
/// "Suspended" map (panel hidden by a menu, Esc…): it is restored as-is if it reappears within this delay.
const SUSPEND_MAX: Duration = Duration::from_secs(180);

/// Everything known about the in-game map while it is open: which map, where it is on screen,
/// the current zoom/pan (predicted from mouse input and corrected from screen pixels) and the markers to draw.
struct OpenMap {
    /// Index of the identified map in `refs::NAMES`.
    map_idx: usize,
    /// Identifier of the identified map (as used by the website data).
    map_id: &'static str,
    /// On-screen square of the map texture at the default view (zoom 1, no pan).
    sq: Square,
    /// Target zoom level (0..=ZOOM_MAX_N).
    n: i32,
    /// Current (animated) zoom factor.
    zoom: f64,
    /// Current pan offset in client pixels.
    off: (f64, f64),
    /// Signature of the map's help panel, used to detect when the map is closed.
    panel: Option<PanelSig>,
    /// When to take the panel signature (after the fade-in); we wait for 2 identical captures.
    panel_due: Option<Instant>,
    /// Previous panel capture, compared with the next one to make sure the fade-in is over.
    panel_prev: Option<capture::Gray>,
    /// Number of attempts made to take the panel signature.
    panel_tries: u32,
    /// Consecutive panel checks that did not match (map possibly closed).
    panel_miss: u32,
    /// When to check the panel next.
    next_panel: Instant,
    /// Closing check in progress (map key / Esc): overlay hidden until the verdict.
    closing_check: Option<Instant>,
    /// Time of the last animation step.
    last: Instant,
    /// Mouse position at the last drag event, while the map is being dragged.
    drag: Option<(i32, i32)>,
    /// Accumulated wheel delta not yet converted into whole notches.
    wheel_acc: i32,
    /// Zoom level according to the two ways of counting the wheel (per 120 units / per event).
    n_a: i32,
    n_b: i32,
    /// A movement (wheel, drag) happened since the last visual resync.
    motion: bool,
    /// Wide resync in progress (back from focus loss / menus): overlay hidden until the result.
    wide_pending: bool,
    /// Uncertain position (a resync failed): points are hidden and a full search is started.
    uncertain: bool,
    /// Last zoom level confirmed by the image (the game can ignore wheel notches that are too close together:
    /// the real level is then between this one and the computed one).
    n_sync: i32,
    /// Last mouse action (wheel / drag): the periodic resync waits until the map is still.
    last_input: Instant,
    /// Scheduled visual resync; `true` = wide search (back from focus loss), overlay hidden meanwhile.
    resync_at: Option<(Instant, bool)>,
    /// When the next periodic resync (map at rest) may run.
    next_idle_resync: Instant,
    /// Markers to draw on this map (from the website's quest-tracker data).
    points: Vec<DrawPoint>,
    /// Quests shown in the legend panel.
    legend: Vec<LegendQuest>,
    /// Extra layers shown in the legend panel.
    layers: Vec<model::LegendLayer>,
    /// Untracked quests with objectives on this map ("Available on this map").
    available: Vec<model::AvailQuest>,
    /// Raid allies (panel rows).
    allies: Vec<model::AllyRow>,
    /// "Up next": quests the player can start now (when the website publishes prerequisites).
    up_next: Vec<panel::UpRow>,
    /// Number of repeatable quests the player can start.
    repeat_avail: usize,
    /// Version of the shared data the points were built from (rebuilt when it changes).
    data_version: u64,
}

impl OpenMap {
    /// Zoom factor the animation is heading to (1 + 0.5·n).
    fn target_zoom(&self) -> f64 {
        1.0 + 0.5 * self.n as f64
    }
    /// Centre of the map square in client coordinates.
    fn center(&self) -> (f64, f64) {
        (self.sq.x0 + self.sq.side / 2.0, self.sq.y0 + self.sq.side / 2.0)
    }
    /// Normalised texture coordinates (u, v) → client coordinates.
    fn project(&self, u: f64, v: f64) -> (f64, f64) {
        let (cx, cy) = self.center();
        let s = self.zoom * self.sq.side;
        (cx + self.off.0 + s * (u - 0.5), cy + self.off.1 + s * (v - 0.5))
    }
}

/// Kind of visual resync (re-locating the map on screen from captured pixels).
///
/// Detection does NOT depend on ANY key: the map is recognised on screen (parchment square in the default
/// view + help panel), and its closing by the disappearance of the panel. The map key is only an accelerator.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Resync {
    /// Back from focus loss / menus: wide search, with a full search as a last resort.
    Wide,
    /// Right after a movement: neighbouring levels (±2) and a medium radius.
    Motion,
    /// Periodic check while at rest.
    Idle,
    /// Full search (all levels, all positions) after a failed resync.
    Global,
}

/// State of the in-game map as seen by the overlay.
enum MapState {
    /// Map not on screen: probe the screen periodically (faster until `fast_until`, e.g. after the map key).
    Closed { next_probe: Instant, fast_until: Instant },
    /// Map open and identified: markers are drawn.
    Open(Box<OpenMap>),
    /// Panel gone (Esc menu, closing…): the state is kept so it can resume if the map reappears.
    Suspended { m: Box<OpenMap>, since: Instant, next_probe: Instant },
}

/// Whole application state (lives on the UI thread, see `APP`).
struct App {
    /// Hidden main window: receives tray, timer, Raw Input and network messages.
    main: HWND,
    /// Transparent, click-through, topmost window where the markers are drawn.
    overlay: HWND,
    /// Clickable panel (legend + quick settings).
    panel: HWND,
    /// Renderer of the clickable panel.
    prenderer: Renderer,
    /// Clickable areas of the panel (buttons, toggles…).
    panel_hits: Vec<panel::Hit>,
    /// Index of the panel area under the mouse, if any.
    panel_hover: Option<usize>,
    /// Whether the panel window is currently visible.
    panel_shown: bool,
    /// Panel being moved: grabbed point (coordinates local to the panel).
    panel_grab: Option<(f32, f32)>,
    /// User settings (saved locally).
    cfg: Config,
    /// UI strings in the selected language.
    s: &'static i18n::Strings,
    /// Data shared with the network thread (quest-tracker data downloaded from the website, status).
    shared: SharedRef,
    /// Channel used to send requests to the network thread.
    net: Sender<Req>,
    /// Renderer of the overlay window.
    renderer: Renderer,
    /// Cache of the last foreground-window check: (window handle, is it the game?).
    fg_cache: (isize, bool),
    /// Game client area in screen coordinates: (x, y, width, height).
    client: (i32, i32, i32, i32),
    /// Current map state (closed / open / suspended).
    state: MapState,
    /// Whether the overlay window is currently visible.
    shown: bool,
    /// The overlay needs to be redrawn.
    dirty: bool,
    /// Keys currently held down (from Raw Input, observed only), to ignore auto-repeat.
    keys_down: HashSet<u16>,
    /// Index of the marker under the mouse (tooltip), if any.
    hover: Option<usize>,
    /// Settings window, when open.
    settings: Option<settings::Settings>,
    /// The settings window is waiting for the user to press the new map key.
    capture_key: bool,
    /// "TaskbarCreated" message id (to re-add the tray icon after Explorer restarts).
    taskbar_created: u32,
    /// Last network status shown to the user (to notify only on changes).
    last_status: Option<Status>,
    /// The "update available" hint has already been shown.
    update_hint_shown: bool,
    /// The game was in the foreground at the previous tick.
    was_fg: bool,
    /// On-screen rectangle of the legend, if drawn.
    legend_rect: Option<(f32, f32, f32, f32)>,
    /// Detection waiting for confirmation (opening animations: 2 identical detections are required).
    candidate: Option<(Square, usize)>,
    /// Votes (per 120, per event) cast by resyncs on how the game counts wheel notches.
    wheel_votes: (u32, u32),
    /// Last map seen (index, default square, panel signature): lets it be found again "from scratch".
    remember: Option<(usize, Square, PanelSig)>,
    /// Earliest time for the next full search of a "lost" map while closed.
    next_global: Instant,
    /// Resync worker thread (image matching runs off the UI thread).
    rs_tx: Sender<resync::Job>,
    /// Slot where the worker thread puts its result.
    rs_out: resync::ResSlot,
    /// The job currently running on the worker thread, if any.
    rs_job: Option<JobInfo>,
    /// Id given to the next resync job.
    rs_next_id: u64,
    /// Install requested from the menu (in "notify me" mode).
    install_requested: bool,
    /// When to check for an app update next.
    next_update_check: Instant,
    /// Page to open if the user clicks the last notification.
    balloon_url: Option<String>,
    /// "Search for an item" window.
    search: Option<search::Search>,
    /// Items / index already requested from the network (so they are not requested again in a loop).
    items_requested: HashSet<String>,
    index_requested: bool,
    /// Markers drawn in the last frame (after decluttering): used for hover, tooltips and Ctrl+click.
    scene: Vec<scene::Item>,
    /// Panel spotlight (hovered row or pill): its markers stay bright, the others are dimmed.
    spot: Option<Spot>,
    /// Spotlight about to change (it follows the hover after a short delay, so moving across rows does not flicker).
    spot_pending: Option<(Option<Spot>, Instant)>,
    /// When the current spotlight started (pulse animation).
    spot_start: Instant,
    /// Scroll offset of the panel body.
    panel_scroll: f32,
    /// Message under the panel header and when it appeared.
    toast: Option<(panel::Toast, Instant)>,
    /// Last change made from the app (history record id), for the toast's "Undo".
    last_record: Option<u64>,
    /// Prerequisite graph of the quests (rebuilt when the pack changes) and the pack version it was built from.
    graph: quests::Graph,
    graph_pack: String,
    /// Undo history of the quest changes (history.json).
    history: progress::History,
    /// Allies without the app or an account, kept on this PC (allies.json).
    locals: progress::LocalAllies,
    /// Quest journal window, when open.
    journal: Option<journal::Journal>,
    /// Last tab of the journal.
    journal_tab: String,
    /// Quest tracked a few seconds ago (gets a "NEW" tag in the panel).
    recent_track: Option<(String, Instant)>,
    /// "Available on this map" fully listed.
    avail_open: bool,
    /// Tracked quests with nothing on this map that the user expanded in the panel.
    other_open: Vec<String>,
    /// The panel's language list is open.
    lang_open: bool,
}

/// Saves the last map seen (for this screen size) to disk: map, default square, help panel.
/// This is a small local file in the app's own data folder (`map_memory.bin`); nothing is sent anywhere.
fn save_remember(client: (i32, i32, i32, i32), idx: usize, sq: &Square, sig: &PanelSig) {
    let mut v = Vec::new();
    v.extend_from_slice(b"HOR1");
    for n in [client.2, client.3, idx as i32] {
        v.extend_from_slice(&n.to_le_bytes());
    }
    for f in [sq.x0, sq.y0, sq.side] {
        v.extend_from_slice(&f.to_le_bytes());
    }
    v.extend_from_slice(&sig.to_bytes());
    let _ = std::fs::write(config::data_dir().join("map_memory.bin"), v);
}

/// Loads the last map seen from `map_memory.bin`, if it was saved for the same client size.
fn load_remember(client: (i32, i32, i32, i32)) -> Option<(usize, Square, PanelSig)> {
    let b = std::fs::read(config::data_dir().join("map_memory.bin")).ok()?;
    if b.get(0..4)? != b"HOR1" {
        return None;
    }
    let i32_at = |o: usize| Some(i32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?));
    let f64_at = |o: usize| Some(f64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?));
    if (i32_at(4)?, i32_at(8)?) != (client.2, client.3) {
        return None; // different resolution: the saved memory does not apply
    }
    let idx = i32_at(12)? as usize;
    let sq = Square { x0: f64_at(16)?, y0: f64_at(24)?, side: f64_at(32)? };
    let sig = PanelSig::from_bytes(b.get(40..)?)?;
    (idx < crate::refs::NAMES.len()).then_some((idx, sq, sig))
}

/// What was known at the moment the capture was sent to the worker thread.
struct JobInfo {
    /// Job id (to match the result with the request).
    id: u64,
    /// Kind of resync requested.
    mode: Resync,
    /// When the screen was captured.
    time: Instant,
    /// Map being tracked.
    map_idx: usize,
    /// Zoom levels at capture time (main estimate and the two wheel-counting variants).
    n: i32,
    n_a: i32,
    n_b: i32,
    /// Predicted pan offset at capture time.
    pred: (f64, f64),
    /// Full search for a "lost" map (closed state): (map, default square, panel).
    lost: Option<(usize, Square, PanelSig)>,
}

thread_local! {
    /// The single application state, owned by the UI thread.
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Runs `f` on the application state, if it exists and is not already borrowed (re-entrant window messages).
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

/// Converts a Rust string to a null-terminated UTF-16 string for Win32 APIs.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// Is this window the game? Only compares the owning process's executable name with `GAME_EXE`
/// (limited "query information" access: the game process is never read or modified).
fn is_game_window(hwnd: HWND) -> bool {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return false;
        }
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let mut buf = [0u16; 512];
        let mut n = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut n).is_ok();
        let _ = CloseHandle(h);
        ok && String::from_utf16_lossy(&buf[..n as usize]).to_lowercase().ends_with(GAME_EXE)
    }
}

/// Client area of a window in screen coordinates: (x, y, width, height).
fn client_rect(hwnd: HWND) -> Option<(i32, i32, i32, i32)> {
    unsafe {
        let mut r = RECT::default();
        GetClientRect(hwnd, &mut r).ok()?;
        let mut p = POINT { x: 0, y: 0 };
        let _ = windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut p);
        Some((p.x, p.y, r.right - r.left, r.bottom - r.top))
    }
}

/// Gives unused memory pages back to Windows (the "working set" drops to the bare minimum).
fn trim_memory() {
    unsafe {
        let _ = windows::Win32::System::Threading::SetProcessWorkingSetSize(windows::Win32::System::Threading::GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

/// Is one of the app's own windows (search, settings) in the foreground?
fn ours_fg() -> bool {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        pid == windows::Win32::System::Threading::GetCurrentProcessId()
    }
}

/// Current mouse-cursor position in screen coordinates.
fn cursor() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

impl App {
    /// The game window, if it is in the foreground (result cached per foreground window).
    fn game_fg(&mut self) -> Option<HWND> {
        let fg = unsafe { GetForegroundWindow() };
        if fg.0 as isize != self.fg_cache.0 {
            self.fg_cache = (fg.0 as isize, !fg.is_invalid() && is_game_window(fg));
        }
        if self.fg_cache.1 {
            Some(fg)
        } else {
            None
        }
    }

    /// UI scale factor relative to a 1440-pixel-high game window (minimum 0.5).
    fn sc(&self) -> f32 {
        (self.client.3 as f32 / 1440.0).max(0.5)
    }

    /// Sends the current settings to the network thread: API key, website base URL and language.
    /// The API key is only ever sent to that website, over HTTPS (Authorization header).
    fn configure_net(&self) {
        let _ = self.net.send(Req::Configure {
            key: self.cfg.api_key(),
            base_url: self.cfg.base_url.clone(),
            locale: self.cfg.locale.clone(),
        });
    }

    /// Shows or hides the overlay and panel in screenshots / recordings / streams, per the user's setting
    /// (`SetWindowDisplayAffinity`).
    fn apply_capture_affinity(&self) {
        let a = if self.cfg.capture_visible { WDA_NONE } else { WDA_EXCLUDEFROMCAPTURE };
        unsafe {
            let _ = SetWindowDisplayAffinity(self.overlay, a);
            let _ = SetWindowDisplayAffinity(self.panel, a);
        }
    }

    /// Is the cursor over the panel? (screen coordinates)
    fn over_panel(&self, sx: i32, sy: i32) -> bool {
        match self.legend_rect {
            Some((l, t, r, b)) => {
                let (x, y) = ((sx - self.client.0) as f32, (sy - self.client.1) as f32);
                self.panel_shown && x >= l && x <= r && y >= t && y <= b
            }
            None => false,
        }
    }

    /// Label of the current language for the panel ("Auto (XX)" when it follows Windows).
    fn lang_label(&self) -> String {
        i18n::lang_name(&self.cfg.locale)
    }

    /// Mouse button pressed on the panel: starts moving it if it is the grip.
    fn panel_down(&mut self, x: f32, y: f32) -> bool {
        let on_grip = self.panel_hits.iter().any(|h| h.contains(x, y) && h.action == panel::Action::Move);
        if on_grip {
            self.panel_grab = Some((x, y));
        }
        on_grip
    }

    /// Moves the panel (cursor in screen coordinates); the position is stored relative to the game window.
    fn panel_drag_to(&mut self, sx: i32, sy: i32) {
        let Some((gx, gy)) = self.panel_grab else { return };
        let (cx, cy, cw, ch) = self.client;
        if cw <= 0 || ch <= 0 {
            return;
        }
        let x = (sx - cx) as f32 - gx;
        let y = (sy - cy) as f32 - gy;
        self.cfg.panel_pos = Some(((x / cw as f32).clamp(0.0, 0.95), (y / ch as f32).clamp(0.0, 0.95)));
        self.dirty = true;
    }

    /// Mouse button released: ends a panel move and saves the new position.
    fn panel_up(&mut self) -> bool {
        if self.panel_grab.take().is_some() {
            self.cfg.save();
            return true;
        }
        false
    }

    /// Mouse moved over the panel: updates the hovered area.
    fn panel_mouse(&mut self, x: f32, y: f32) {
        let h = self.panel_hits.iter().position(|h| h.contains(x, y));
        if h != self.panel_hover {
            self.panel_hover = h;
            self.dirty = true;
        }
        // Spotlight what the hovered row / pill stands for (applied after a short delay by `tick`).
        let want = h.and_then(|i| self.panel_hits.get(i)).and_then(|h| self.spot_for(&h.action));
        let pending_same = self.spot_pending.as_ref().map(|p| p.0 == want).unwrap_or(false);
        if want != self.spot && !pending_same {
            self.spot_pending = Some((want, Instant::now()));
        } else if want == self.spot {
            self.spot_pending = None;
        }
    }

    /// What a panel zone spotlights on the map, if anything.
    fn spot_for(&self, a: &panel::Action) -> Option<Spot> {
        let MapState::Open(m) = &self.state else { return None };
        match a {
            panel::Action::ToggleQuest(id) | panel::Action::ExpandOther(id) => m.legend.iter().position(|q| q.quest_id == *id).map(Spot::Owner),
            panel::Action::RemoveSearch(k) => m.legend.iter().position(|q| q.quest_id == format!("find:{k}")).map(Spot::Owner),
            panel::Action::ToggleLayer(id) => m.layers.iter().position(|l| l.id == *id && l.on).map(Spot::Layer),
            panel::Action::Track(q) | panel::Action::AvailRow(q) => Some(Spot::Quest(q.clone())),
            panel::Action::Done(q) => m.legend.iter().position(|l| l.quest_id == *q).map(Spot::Owner),
            panel::Action::ToggleAlly(k) => m.allies.iter().position(|a| a.key == *k).map(Spot::Ally),
            panel::Action::ToggleAllyQuest(k, q) | panel::Action::RemoveAllyQuest(k, q) => m.allies.iter().position(|a| a.key == *k).map(|i| Spot::AllyQuest(i, q.clone())),
            _ => None,
        }
    }

    /// Mouse wheel over the panel: scrolls its body (3 rows per notch). The game does not receive it.
    fn panel_wheel(&mut self, delta: i16) {
        self.panel_scroll = (self.panel_scroll - delta as f32 / 120.0 * 90.0 * self.sc()).max(0.0);
        self.dirty = true;
    }

    /// Starts (or stops) tracking a quest on the website account and shows the result in the panel.
    /// This is the app's only write to the website (`POST /me/track`, see `net.rs`); it happens only on an
    /// explicit user action (Ctrl+click on a Quests-layer marker, "+ Track", "Undo", or the search window).
    fn track_quest(&mut self, qid: String, title: String, tracked: bool) {
        let p = self.progress_of(progress::ME);
        let changes = if tracked { self.graph.plan_track(&p, &qid) } else { self.graph.plan_untrack(&p, &qid) };
        let s = self.s;
        let label = format!("{} {title}", if tracked { s.toast_tracking } else { s.toast_undone });
        if self.run_change(progress::ME, label, changes).is_some() && tracked {
            self.recent_track = Some((qid, Instant::now()));
        }
    }

    /// Opens the quest journal (optionally at a quest, or on the "+ally" tab), or brings it to the front.
    fn open_journal(&mut self, at: Option<String>) {
        self.refresh_graph();
        let d = self.journal_data();
        let target = at.clone().unwrap_or_else(|| self.journal_tab.clone());
        if target == journal::ADD_TAB {
            self.shared.lock().unwrap().hub = None;
            let _ = self.net.send(Req::FetchHub);
        }
        if let Some(j) = self.journal.as_mut() {
            j.set_data(d);
            if at.is_some() {
                j.goto(&target);
            }
            unsafe {
                let _ = SetForegroundWindow(j.hwnd);
            }
            return;
        }
        self.journal = journal::Journal::open(self.s, d, target, self.client);
    }

    /// Hands fresh data to the open journal.
    fn refresh_journal(&mut self) {
        if self.journal.is_some() {
            let d = self.journal_data();
            if let Some(j) = self.journal.as_mut() {
                j.set_data(d);
            }
        }
    }

    /// Everything the journal shows: the player and the allies, their progression, titles, badges, undo history.
    fn journal_data(&self) -> journal::Data {
        let sh = self.shared.lock().unwrap();
        let mut d = journal::Data { graph: self.graph.clone(), v12: sh.tracker.as_ref().map(|t| t.completed.is_some()).unwrap_or(false), ..Default::default() };
        d.synced = matches!(sh.status, Some(Status::Ok));
        d.hub = sh.hub.clone();
        if let Some(p) = sh.pack.as_ref() {
            d.titles = p.quests.iter().map(|(k, q)| (k.clone(), q.title.clone())).collect();
        }
        let me_name = sh.tracker.as_ref().and_then(|t| t.user.clone()).unwrap_or_else(|| "Me".into());
        d.subjects.push(journal::Subject {
            key: progress::ME.into(),
            initial: me_name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default(),
            name: me_name,
            color: (0.831, 0.686, 0.416),
            kind: journal::Kind::Me,
            hidden: false,
        });
        if let Some(t) = sh.tracker.as_ref() {
            d.progress.insert(progress::ME.into(), progress::me_progress(t));
            // Badges and colours of my tracked quests (stable slots).
            for q in &t.tracked {
                if let Some(slot) = self.cfg.quest_slots.iter().position(|x| *x == q.quest_id) {
                    let b = if slot < 9 { (slot + 1).to_string() } else { String::new() };
                    d.badges.insert(q.quest_id.clone(), (b, model::parse_color(&q.color)));
                }
            }
            // Allies in adding order (at most 2).
            let names: Vec<(String, String, journal::Kind)> = self
                .cfg
                .ally_order
                .iter()
                .take(2)
                .filter_map(|k| {
                    t.allies.iter().find(|a| a.id == *k).map(|a| (k.clone(), a.name.clone(), journal::Kind::Site)).or_else(|| self.locals.allies.iter().find(|a| a.id == *k).map(|a| (k.clone(), a.name.clone(), journal::Kind::Local)))
                })
                .collect();
            for (i, (k, name, kind)) in names.iter().enumerate() {
                let other = names.get(1 - i.min(1)).filter(|_| names.len() == 2).map(|o| o.1.as_str());
                let p = match kind {
                    journal::Kind::Local => self.locals.allies.iter().find(|a| a.id == *k).map(|a| a.progress()).unwrap_or_default(),
                    _ => t
                        .allies
                        .iter()
                        .find(|a| a.id == *k)
                        .map(|a| {
                            let mut p = quests::Progress::default();
                            p.done.extend(a.completed.clone().unwrap_or_default());
                            p.tracked.extend(a.tracked.iter().filter(|q| !q.completed).map(|q| q.quest_id.clone()));
                            p
                        })
                        .unwrap_or_default(),
                };
                d.progress.insert(k.clone(), p);
                d.subjects.push(journal::Subject {
                    key: k.clone(),
                    initial: progress::initials(name, other),
                    name: name.clone(),
                    color: progress::ALLY_COLORS[i],
                    kind: kind.clone(),
                    hidden: self.cfg.hidden_allies.contains(k),
                });
            }
        }
        drop(sh);
        // Undo history per subject, newest first, with the quests changed elsewhere since.
        let now = progress::now_secs();
        for sub in &d.subjects {
            let cur = d.progress.get(&sub.key).cloned().unwrap_or_default();
            let v: Vec<journal::HistoryEntry> = self.history.of(&sub.key).into_iter().map(|r| (r.id, r.label.clone(), now.saturating_sub(r.at), r.conflicts(&cur))).collect();
            d.history.insert(sub.key.clone(), v);
        }
        // Markers on the open map, per quest.
        if let MapState::Open(m) = &self.state {
            for l in &m.legend {
                d.spots.insert(l.quest_id.clone(), l.on_this_map);
            }
            for a in &m.available {
                d.spots.entry(a.quest_id.clone()).or_insert(a.spots);
            }
        }
        d
    }

    /// A command from the journal.
    fn journal_cmd(&mut self, c: journal::Cmd) {
        match c {
            journal::Cmd::Apply { subject, label, changes } => {
                self.run_change(&subject, label, changes);
            }
            journal::Cmd::Undo(id) => self.undo_record(id),
            journal::Cmd::UndoTo(subject, id) => {
                // This record and every newer one of the subject, merged into one write (each quest goes back to its
                // state before the oldest of them).
                let recs: Vec<progress::Record> = {
                    let all = self.history.of(&subject);
                    let n = all.iter().position(|r| r.id == id).map(|i| i + 1).unwrap_or(0);
                    all.into_iter().take(n).cloned().collect()
                };
                if let Some(oldest) = recs.last().cloned() {
                    let mut merged: Vec<(String, quests::Write)> = vec![];
                    for r in &recs {
                        for o in &r.ops {
                            match merged.iter_mut().find(|m| m.0 == o.quest) {
                                Some(m) => m.1 = o.from,
                                None => merged.push((o.quest.clone(), o.from)),
                            }
                        }
                    }
                    let ops: Vec<progress::Op> = merged.iter().map(|(q, w)| progress::Op { quest: q.clone(), from: *w, to: *w, slot: None }).collect();
                    // Replace them by one synthetic record and undo it.
                    for r in &recs {
                        self.history.remove(r.id);
                    }
                    let rid = self.history.next_id();
                    self.history.push(progress::Record { id: rid, subject: subject.clone(), label: oldest.label.clone(), at: oldest.at, ops });
                    self.undo_record(rid);
                }
            }
            journal::Cmd::AddSiteAlly(id) => {
                let mut ids: Vec<String> = self.shared.lock().unwrap().tracker.as_ref().map(|t| t.allies.iter().map(|a| a.id.clone()).collect()).unwrap_or_default();
                if !ids.contains(&id) && ids.len() + self.locals.allies.len() < 2 {
                    ids.push(id.clone());
                    self.cfg.ally_order.retain(|k| *k != id);
                    self.cfg.ally_order.push(id);
                    self.cfg.save();
                    let _ = self.net.send(Req::SetAllies(ids));
                }
            }
            journal::Cmd::AddLocalAlly(name) => {
                let n_site = self.shared.lock().unwrap().tracker.as_ref().map(|t| t.allies.len()).unwrap_or(0);
                if n_site + self.locals.allies.len() < 2 {
                    let id = format!("l_{}", progress::now_secs());
                    self.locals.allies.push(progress::LocalAlly { id: id.clone(), name: name.chars().take(24).collect(), done: vec![], tracked: vec![] });
                    self.locals.save();
                    self.cfg.ally_order.push(id.clone());
                    self.cfg.save();
                    self.journal_tab = id.clone();
                    let d = self.journal_data();
                    if let Some(j) = self.journal.as_mut() {
                        j.set_data(d);
                        j.goto(&id);
                    }
                }
            }
            journal::Cmd::RemoveAlly(k) => {
                if self.locals.allies.iter().any(|a| a.id == k) {
                    self.locals.allies.retain(|a| a.id != k);
                    self.locals.save();
                } else {
                    let ids: Vec<String> = self.shared.lock().unwrap().tracker.as_ref().map(|t| t.allies.iter().map(|a| a.id.clone()).filter(|x| *x != k).collect()).unwrap_or_default();
                    let _ = self.net.send(Req::SetAllies(ids));
                }
                self.cfg.ally_order.retain(|x| *x != k);
                self.cfg.hidden_allies.retain(|x| *x != k);
                self.cfg.hidden_ally_quests.retain(|x| !x.starts_with(&format!("{k}|")));
                self.cfg.save();
            }
            journal::Cmd::HideAlly(k) => {
                if let Some(p) = self.cfg.hidden_allies.iter().position(|x| *x == k) {
                    self.cfg.hidden_allies.remove(p);
                } else {
                    self.cfg.hidden_allies.push(k);
                }
                self.cfg.save();
            }
            journal::Cmd::FetchHub => {
                self.shared.lock().unwrap().hub = None;
                let _ = self.net.send(Req::FetchHub);
            }
            journal::Cmd::Tab(t) => self.journal_tab = t,
        }
        if let MapState::Open(m) = &mut self.state {
            m.data_version = u64::MAX;
        }
        self.dirty = true;
        self.refresh_journal();
    }

    /// Closes the journal and gives the focus back to the game.
    fn close_journal(&mut self) {
        if let Some(j) = self.journal.take() {
            self.journal_tab = j.tab.clone();
            j.close();
        }
        unsafe {
            let mut game = HWND::default();
            let _ = EnumWindows(Some(find_game), LPARAM(&mut game as *mut HWND as isize));
            if !game.is_invalid() {
                let _ = SetForegroundWindow(game);
            }
        }
    }

    // ---------- Quest progression (docs/DESIGN_QUESTS.md §2) ----------

    /// Rebuilds the prerequisite graph when the pack changed.
    fn refresh_graph(&mut self) {
        let sh = self.shared.lock().unwrap();
        if let Some(p) = sh.pack.as_ref() {
            if p.pack_version != self.graph_pack {
                self.graph = quests::Graph::from_pack(p);
                self.graph_pack = p.pack_version.clone();
            }
        }
    }

    /// Progression of a subject: "me" (the website tracker) or a local ally.
    fn progress_of(&self, subject: &str) -> quests::Progress {
        if subject == progress::ME {
            return self.shared.lock().unwrap().tracker.as_ref().map(progress::me_progress).unwrap_or_default();
        }
        if let Some(a) = self.locals.allies.iter().find(|a| a.id == subject) {
            return a.progress();
        }
        // A website ally (read-only).
        let sh = self.shared.lock().unwrap();
        sh.tracker
            .as_ref()
            .and_then(|t| t.allies.iter().find(|a| a.id == subject))
            .map(|a| {
                let mut p = quests::Progress::default();
                p.done.extend(a.completed.clone().unwrap_or_default());
                for q in &a.tracked {
                    if !q.completed {
                        p.tracked.insert(q.quest_id.clone());
                    }
                }
                p
            })
            .unwrap_or_default()
    }

    /// True when the website supports the full progression (contract v1.2: `completed` in the tracker).
    fn site_v12(&self) -> bool {
        self.shared.lock().unwrap().tracker.as_ref().map(|t| t.completed.is_some()).unwrap_or(false)
    }

    /// Title of a quest in the current language (its id if unknown).
    fn quest_title(&self, q: &str) -> String {
        self.shared.lock().unwrap().pack.as_ref().and_then(|p| p.quests.get(q)).map(|x| x.title.clone()).unwrap_or_else(|| q.to_string())
    }

    /// Sends changes of the player's progression to the website: one atomic write (v1.2), or, on a website
    /// without it, single track / untrack calls. Returns false when the website cannot do it yet.
    fn send_changes(&mut self, changes: &[quests::Change]) -> bool {
        // Order of the batch: first what stops being tracked (frees its colour on the website), then "done", then
        // "tracked" (which takes the first free colour). So an undo gives a quest its colour back.
        let mut changes = changes.to_vec();
        changes.sort_by_key(|(_, w)| match w {
            quests::Write::None => 0,
            quests::Write::Done => 1,
            quests::Write::Tracked => 2,
        });
        let changes = &changes[..];
        if self.site_v12() {
            // The revision of the player's own progression (an ally changing must not cause a conflict).
            let base_rev = self.shared.lock().unwrap().tracker.as_ref().and_then(|t| t.progress_rev.clone().or_else(|| t.rev.clone()));
            let _ = self.net.send(Req::SetQuests { changes: changes.iter().map(|(q, w)| (q.clone(), w.as_str())).collect(), base_rev });
            return true;
        }
        if changes.iter().any(|(_, w)| *w == quests::Write::Done) {
            return false;
        }
        for (q, w) in changes {
            let _ = self.net.send(Req::Track { quest_id: q.clone(), tracked: *w == quests::Write::Tracked });
        }
        true
    }

    /// Commits one user action on a subject's progression: keeps its exact "before" states in the history (undo),
    /// then writes it (the website for "me", allies.json for a local ally) and shows a toast with Undo.
    /// Returns the history record id.
    fn run_change(&mut self, subject: &str, label: String, changes: Vec<quests::Change>) -> Option<u64> {
        if changes.is_empty() {
            return None;
        }
        let before = self.progress_of(subject);
        let ops: Vec<progress::Op> = changes
            .iter()
            .map(|(q, w)| {
                let from = quests::write_state(&before, q);
                let slot = (subject == progress::ME && from == quests::Write::Tracked).then(|| self.cfg.quest_slots.iter().position(|s| s == q)).flatten();
                progress::Op { quest: q.clone(), from, to: *w, slot }
            })
            .collect();
        logf!("change on {subject}: {} quest(s)", ops.len());
        // A finished quest whose follow-ups were tracked automatically: say which ones.
        let auto: Vec<String> = changes
            .iter()
            .filter(|(q, w)| *w == quests::Write::Tracked && changes.iter().any(|(d, dw)| *dw == quests::Write::Done && d != q && before.tracked.contains(d) && self.graph.requires(q).contains(d)))
            .map(|(q, _)| self.quest_title(q))
            .collect();
        let label = if auto.is_empty() { label } else { format!("{label} · {} {}", self.s.toast_next, auto.join(", ")) };
        if subject == progress::ME {
            if !self.send_changes(&changes) {
                self.toast = Some((panel::Toast { text: self.s.toast_needs_site.into(), ok: false, undo: false, next: None }, Instant::now()));
                self.dirty = true;
                return None;
            }
        } else if let Some(a) = self.locals.get_mut(subject) {
            a.apply(&changes);
            self.locals.save();
            if let MapState::Open(m) = &mut self.state {
                m.data_version = u64::MAX;
            }
        } else {
            return None; // website allies are read-only
        }
        let id = self.history.next_id();
        self.history.push(progress::Record { id, subject: subject.to_string(), label: label.clone(), at: progress::now_secs(), ops });
        self.last_record = Some(id);
        // Next quest of the same character after a "done" (the "propose the next one" loop).
        let after = quests::apply(&before, &changes);
        let next = changes
            .iter()
            .rev()
            .find(|(_, w)| *w == quests::Write::Done)
            .and_then(|(q, _)| match self.graph.up_next(&after, self.graph.giver(q)) {
                Some(quests::Next::Available(n)) if subject == progress::ME => Some((n.clone(), self.quest_title(&n))),
                _ => None,
            });
        self.toast = Some((panel::Toast { text: label, ok: true, undo: true, next }, Instant::now()));
        self.dirty = true;
        Some(id)
    }

    /// Undoes a recorded change: writes back the exact "before" states (badge numbers included).
    fn undo_record(&mut self, id: u64) {
        let Some(r) = self.history.records.iter().find(|r| r.id == id).cloned() else { return };
        let changes = r.undo_changes();
        if r.subject == progress::ME {
            // An untracked quest gets its old number (and so its colour on the map) back.
            for o in r.ops.iter().filter(|o| o.from == quests::Write::Tracked) {
                if let Some(slot) = o.slot {
                    if self.cfg.quest_slots.len() <= slot {
                        self.cfg.quest_slots.resize(slot + 1, String::new());
                    }
                    if self.cfg.quest_slots[slot].is_empty() {
                        self.cfg.quest_slots[slot] = o.quest.clone();
                    }
                }
            }
            self.cfg.save();
            if !self.send_changes(&changes) {
                self.toast = Some((panel::Toast { text: self.s.toast_needs_site.into(), ok: false, undo: false, next: None }, Instant::now()));
                self.dirty = true;
                return;
            }
        } else if let Some(a) = self.locals.get_mut(&r.subject) {
            a.apply(&changes);
            self.locals.save();
            if let MapState::Open(m) = &mut self.state {
                m.data_version = u64::MAX;
            }
        }
        self.history.remove(id);
        self.last_record = None;
        self.recent_track = None;
        let text = self.s.toast_undone_change.replace("{label}", &r.label);
        self.toast = Some((panel::Toast { text, ok: true, undo: false, next: None }, Instant::now()));
        self.dirty = true;
    }

    /// Click in the panel: quick settings are applied and saved immediately.
    fn panel_click(&mut self, x: f32, y: f32) {
        let Some(hit) = self.panel_hits.iter().find(|h| h.contains(x, y)).cloned() else {
            // A click outside the open language list closes it.
            if self.lang_open {
                self.lang_open = false;
                self.dirty = true;
            }
            return;
        };
        match hit.action {
            panel::Action::Move => return,
            panel::Action::Settings => {
                self.open_settings();
                return;
            }
            panel::Action::Collapse => self.cfg.panel_collapsed = !self.cfg.panel_collapsed,
            panel::Action::ToggleDone => self.cfg.show_done = !self.cfg.show_done,
            panel::Action::ChanceDown => self.cfg.min_chance = panel::chance_down(self.cfg.min_chance),
            panel::Action::ChanceUp => self.cfg.min_chance = panel::chance_up(self.cfg.min_chance),
            panel::Action::RevealFilter(v) => self.cfg.min_chance = v,
            // Language chip: opens / closes the language list.
            panel::Action::Language => {
                self.lang_open = !self.lang_open;
                self.dirty = true;
                return;
            }
            // A language picked in the list: one data reload, only if it really changed.
            panel::Action::SetLang(code) => {
                self.lang_open = false;
                self.dirty = true;
                if code == self.cfg.locale {
                    return;
                }
                self.cfg.locale = code;
                self.s = i18n::t_for(&self.cfg.locale);
                self.configure_net();
                let _ = self.net.send(Req::Refresh { force: true });
            }
            panel::Action::Search => {
                self.open_search();
                return;
            }
            panel::Action::RemoveSearch(key) => {
                self.cfg.search_items.retain(|k| *k != key);
            }
            panel::Action::ClearSearches => self.cfg.search_items.clear(),
            panel::Action::ToggleLayer(id) => {
                if let Some(p) = self.cfg.layers_on.iter().position(|q| *q == id) {
                    self.cfg.layers_on.remove(p);
                } else {
                    self.cfg.quests_layer_seen |= id == "quests";
                    self.cfg.layers_on.push(id);
                }
            }
            // Group label: everything off if any layer of the group is on, otherwise everything on.
            panel::Action::ToggleGroup(ids) => {
                if ids.iter().any(|id| self.cfg.layers_on.contains(id)) {
                    self.cfg.layers_on.retain(|l| !ids.contains(l));
                } else {
                    self.cfg.quests_layer_seen |= ids.iter().any(|i| i == "quests");
                    self.cfg.layers_on.extend(ids);
                }
            }
            panel::Action::ToggleSection(id) => {
                if let Some(p) = self.cfg.sections_collapsed.iter().position(|q| *q == id) {
                    self.cfg.sections_collapsed.remove(p);
                } else {
                    self.cfg.sections_collapsed.push(id);
                }
                self.cfg.save();
                self.dirty = true;
                return;
            }
            panel::Action::Track(qid) => {
                let p = self.progress_of(progress::ME);
                if self.graph.plan_track(&p, &qid).len() > 1 {
                    self.open_journal(Some(qid));
                    return;
                }
                let title = self.quest_title(&qid);
                self.track_quest(qid, title, true);
                return;
            }
            panel::Action::Undo => {
                if let Some(id) = self.last_record.take() {
                    self.undo_record(id);
                }
                return;
            }
            panel::Action::AvailRow(_) => return,
            panel::Action::ToggleQuestMarkers => self.cfg.quest_markers_hidden = !self.cfg.quest_markers_hidden,
            // ✓ on a tracked quest: done (it never cascades from the panel; if it would, the journal shows the preview).
            panel::Action::Done(qid) => {
                let p = self.progress_of(progress::ME);
                let plan = self.graph.plan_done(&p, &qid);
                if self.graph.cascades(&plan, &qid) {
                    self.open_journal(Some(qid));
                    return;
                }
                let title = self.quest_title(&qid);
                let label = if self.graph.is_repeatable(&qid) { self.s.toast_finished } else { self.s.toast_done }.replace("{quest}", &title);
                self.run_change(progress::ME, label, plan);
                return;
            }
            panel::Action::Journal => {
                self.open_journal(None);
                return;
            }
            panel::Action::ManageAllies => {
                self.open_journal(Some("+ally".into()));
                return;
            }
            panel::Action::ToggleAlly(k) => {
                if let Some(p) = self.cfg.hidden_allies.iter().position(|x| *x == k) {
                    self.cfg.hidden_allies.remove(p);
                } else {
                    self.cfg.hidden_allies.push(k);
                }
            }
            panel::Action::ToggleAllyQuest(k, q) => {
                let id = format!("{k}|{q}");
                if let Some(p) = self.cfg.hidden_ally_quests.iter().position(|x| *x == id) {
                    self.cfg.hidden_ally_quests.remove(p);
                } else {
                    self.cfg.hidden_ally_quests.push(id);
                }
            }
            panel::Action::RemoveAllyQuest(k, q) => {
                let label = self.s.toast_undone.to_string() + " " + &self.quest_title(&q);
                self.run_change(&k, label, vec![(q, quests::Write::None)]);
                return;
            }
            panel::Action::MoreAvail => {
                self.avail_open = !self.avail_open;
                self.dirty = true;
                return;
            }
            panel::Action::ExpandOther(id) => {
                self.other_open.push(id);
                self.dirty = true;
                return;
            }
            panel::Action::ToggleQuest(id) => {
                if let Some(p) = self.cfg.hidden_quests.iter().position(|q| *q == id) {
                    self.cfg.hidden_quests.remove(p);
                } else {
                    self.cfg.hidden_quests.push(id);
                }
            }
        }
        self.cfg.save();
        if let MapState::Open(m) = &mut self.state {
            m.data_version = u64::MAX;
        }
        self.dirty = true;
    }

    // ---------- Input ----------
    // Keyboard and mouse events arrive through Raw Input registered in passive mode (RIDEV_INPUTSINK):
    // the overlay only OBSERVES them; they are never consumed, blocked, modified or re-sent to the game.

    /// Keyboard event (observed only). Used to capture a new map key in the settings window, and to
    /// react faster when the map key / Esc is pressed while the game is in the foreground.
    fn on_key(&mut self, vk: u16, down: bool) {
        if !down {
            self.keys_down.remove(&vk);
            return;
        }
        if !self.keys_down.insert(vk) {
            return; // auto-repeat
        }
        // Settings window is waiting for the new map key (Esc cancels).
        if self.capture_key {
            if vk != 0x1B {
                self.cfg.map_vk = vk as u32;
                if let Some(st) = self.settings.as_mut() {
                    st.vk = vk as u32;
                }
            }
            self.capture_key = false;
            let name = settings::key_name(self.settings.as_ref().map(|s| s.vk).unwrap_or(self.cfg.map_vk));
            if let Some(st) = self.settings.as_mut() {
                st.set_mapkey_text(&name);
            }
            return;
        }
        if self.game_fg().is_none() {
            return;
        }
        let now = Instant::now();
        if vk as u32 == self.cfg.map_vk || vk == 0x1B {
            match &mut self.state {
                // Accelerator: probe the screen more often for a short while.
                MapState::Closed { next_probe, fast_until } => {
                    *next_probe = now + Duration::from_millis(200);
                    *fast_until = now + Duration::from_millis(1200);
                }
                MapState::Suspended { next_probe, .. } => *next_probe = now + Duration::from_millis(200),
                // Probably closing: hide right away and verify on screen.
                MapState::Open(m) => {
                    m.closing_check = Some(now + Duration::from_millis(250));
                    m.drag = None;
                }
            }
            self.dirty = true;
        }
    }

    /// Raw mouse event (observed only, never consumed). `pos` = cursor position AT THE TIME of the event
    /// (GetMessagePos), not at processing time: a very fast drag is therefore never lost.
    /// Used to predict the in-game map's zoom (wheel) and pan (left-button drag).
    fn on_mouse(&mut self, flags: u16, data: u16, pos: (i32, i32)) {
        const LEFT_DOWN: u16 = 0x0001;
        const LEFT_UP: u16 = 0x0002;
        const WHEEL: u16 = 0x0400;
        // Drag in progress: follow every movement up to and including the release.
        if let MapState::Open(m) = &mut self.state {
            if let Some((lx, ly)) = m.drag {
                if pos != (lx, ly) {
                    let lim = m.zoom * m.sq.side / 2.0;
                    m.off.0 = (m.off.0 + (pos.0 - lx) as f64).clamp(-lim, lim);
                    m.off.1 = (m.off.1 + (pos.1 - ly) as f64).clamp(-lim, lim);
                    m.drag = Some(pos);
                    m.last_input = Instant::now();
                    self.dirty = true;
                }
            }
            // The release ALWAYS ends a drag, even if the game lost focus in the meantime.
            if flags & LEFT_UP != 0 && m.drag.take().is_some() {
                m.last_input = Instant::now();
            }
        }
        if flags == 0 || self.game_fg().is_none() {
            return;
        }
        // Click or wheel on OUR panel: the game does not receive them, so the map does not move.
        let (sx, sy) = pos;
        if self.over_panel(sx, sy) {
            return;
        }
        let (gx, gy, gw, gh) = self.client;
        // Measured in game (2026-10-14): one zoom notch per wheel EVENT, whatever its size.
        // The "per 120" model is only used if resyncs have explicitly learned it.
        let per_event = self.cfg.wheel_per_event != Some(false);
        let MapState::Open(m) = &mut self.state else { return };
        if m.closing_check.is_some() {
            return;
        }
        if flags & WHEEL != 0 {
            m.last_input = Instant::now();
            m.motion = true;
            let d = data as i16 as i32;
            // Model A: one notch per 120 units (classic wheel).
            m.wheel_acc += d;
            while m.wheel_acc >= 120 {
                m.wheel_acc -= 120;
                m.n_a = (m.n_a + 1).min(ZOOM_MAX_N);
            }
            while m.wheel_acc <= -120 {
                m.wheel_acc += 120;
                m.n_a = (m.n_a - 1).max(0);
            }
            // Model B: one notch per event (smooth-scrolling wheels).
            m.n_b = (m.n_b + d.signum()).clamp(0, ZOOM_MAX_N);
            m.n = if per_event { m.n_b } else { m.n_a };
        }
        // Left button pressed on the map (not on the game's help panel): a drag (pan) starts.
        if flags & LEFT_DOWN != 0 {
            let (cx, cy) = pos;
            let (lx, ly) = ((cx - gx) as f64, (cy - gy) as f64);
            let (px0, py0, px1, py1) = detect::panel_rect(gh as f64);
            let on_panel = lx >= px0 && lx <= px1 && ly >= py0 && ly <= py1;
            let inside = lx >= 0.0 && ly >= 0.0 && lx < gw as f64 && ly < gh as f64;
            if inside && !on_panel {
                m.drag = Some((cx, cy));
                m.last_input = Instant::now();
                m.motion = true;
            }
        }
    }

    // ---------- Loop (≈ 64 Hz) ----------

    /// Main periodic step: tracks the game window, detects the map opening/closing from screen pixels,
    /// updates zoom/pan, shows/hides the overlay and redraws it when needed.
    fn tick(&mut self) {
        let now = Instant::now();
        let game = self.game_fg();
        let regained = game.is_some() && !self.was_fg;
        self.was_fg = game.is_some();
        if let MapState::Open(m) = &mut self.state {
            if game.is_none() {
                m.drag = None;
            } else if regained {
                // Back in the game: the map may have moved without us (focus-regain click…) → wide resync.
                m.resync_at = Some((now + Duration::from_millis(350), true));
            }
        }
        // Follow the game window's position and size.
        if let Some(g) = game {
            if let Some(rc) = client_rect(g) {
                if rc != self.client {
                    self.client = rc;
                    self.dirty = true;
                }
            }
        }
        self.sync_data();
        self.maybe_install_update(now);
        // Advance the map state machine (closed → open → suspended → …).
        let state = std::mem::replace(&mut self.state, MapState::Closed { next_probe: now, fast_until: now });
        self.state = match state {
            MapState::Closed { next_probe, fast_until } => {
                if game.is_some() && now >= next_probe {
                    let step = if now < fast_until { PROBE_FAST } else { PROBE_EVERY };
                    match self.probe_confirmed() {
                        Ok(m) => self.opened(m),
                        Err(true) => MapState::Closed { next_probe: now + Duration::from_millis(100), fast_until },
                        Err(false) => match self.relocate_lost(now) {
                            Some(m) => self.opened(m),
                            None => MapState::Closed { next_probe: now + step, fast_until },
                        },
                    }
                } else {
                    MapState::Closed { next_probe, fast_until }
                }
            }
            MapState::Suspended { mut m, since, next_probe } => {
                if now.duration_since(since) > SUSPEND_MAX {
                    logf!("map considered closed");
                    MapState::Closed { next_probe: now, fast_until: now }
                } else if game.is_some() && now >= next_probe {
                    let fresh = self.probe_confirmed();
                    if let Ok(fresh) = fresh {
                        // New opening (default view).
                        self.opened(fresh)
                    } else if matches!(fresh, Err(true)) {
                        MapState::Suspended { m, since, next_probe: now + Duration::from_millis(100) }
                    } else if self.panel_present(&m) {
                        logf!("map found again (state restored, resync)");
                        m.panel_miss = 0;
                        m.closing_check = None;
                        m.drag = None;
                        m.last = now;
                        m.resync_at = Some((now + Duration::from_millis(150), true));
                        self.dirty = true;
                        MapState::Open(m)
                    } else {
                        MapState::Suspended { m, since, next_probe: now + PROBE_EVERY }
                    }
                } else {
                    MapState::Suspended { m, since, next_probe }
                }
            }
            MapState::Open(mut m) => {
                if self.tick_open(&mut m, game.is_some(), now) {
                    MapState::Open(m)
                } else {
                    logf!("map panel gone: overlay hidden");
                    self.dirty = true;
                    MapState::Suspended { m, since: now, next_probe: now + PROBE_EVERY }
                }
            }
        };
        // The overlay is visible only while the game (or one of our windows) is in front and the map is open and located.
        let visible = (game.is_some() || ours_fg())
            && matches!(&self.state, MapState::Open(m) if m.closing_check.is_none() && !m.wide_pending && !m.resync_at.map(|r| r.1).unwrap_or(false));
        // Spotlight follows the panel hover after 150 ms (200 ms to clear); its pulse animates for 1.8 s.
        if let Some((want, since)) = &self.spot_pending {
            let delay = if want.is_some() { 150 } else { 200 };
            if now.duration_since(*since) >= Duration::from_millis(delay) {
                self.spot = want.clone();
                self.spot_pending = None;
                self.spot_start = now;
                self.dirty = true;
            }
        }
        if self.spot.is_some() && now.duration_since(self.spot_start) < Duration::from_millis(1900) {
            self.dirty = true;
        }
        if self.toast.as_ref().map(|t| now.duration_since(t.1) > Duration::from_secs(if t.0.undo { 8 } else if t.0.ok { 5 } else { 9 })).unwrap_or(false) {
            self.toast = None;
            self.dirty = true;
        }
        if self.recent_track.as_ref().map(|t| now.duration_since(t.1) > Duration::from_secs(10)).unwrap_or(false) {
            self.recent_track = None;
            self.dirty = true;
        }
        if visible {
            let hov = self.compute_hover();
            if hov != self.hover {
                self.hover = hov;
                self.dirty = true;
            }
        }
        if visible != self.shown {
            self.shown = visible;
            unsafe {
                let _ = ShowWindow(self.overlay, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
                if visible {
                    self.raise_dialogs();
                }
            }
            if !visible {
                self.show_panel(false);
                // Map hidden: give the render images' memory (2560×1440 ≈ 15 MB) back to Windows.
                self.renderer.release();
                self.prenderer.release();
                trim_memory();
            }
            self.dirty = true;
        }
        if visible && self.dirty {
            self.dirty = false;
            self.draw();
        }
    }

    /// Light probe (2 lines of screen pixels) then, if it passes, full detection and identification.
    fn probe_and_detect(&self) -> Option<OpenMap> {
        let (x, y, w, h) = self.client;
        if !detect::probe(x, y, w, h) {
            return None;
        }
        self.detect_open()
    }

    /// Ok = map confirmed; Err(true) = 1st detection, to be confirmed on the next pass; Err(false) = nothing.
    fn probe_confirmed(&mut self) -> Result<OpenMap, bool> {
        let Some(m) = self.probe_and_detect() else {
            self.candidate = None;
            return Err(false);
        };
        let same = self.candidate.as_ref().map(|(c, i)| {
            *i == m.map_idx && (c.x0 - m.sq.x0).abs() < 1.5 && (c.y0 - m.sq.y0).abs() < 1.5 && (c.side - m.sq.side).abs() < 1.5
        });
        if same == Some(true) {
            self.candidate = None;
            Ok(m)
        } else {
            self.candidate = Some((m.sq, m.map_idx));
            Err(true)
        }
    }

    /// A map was just detected as open: asks the network thread for fresh tracker data and switches to `Open`.
    fn opened(&mut self, m: OpenMap) -> MapState {
        logf!("map opened: {} (square {:.0},{:.0} side {:.1})", m.map_id, m.sq.x0, m.sq.y0, m.sq.side);
        let _ = self.net.send(Req::Refresh { force: false });
        self.dirty = true;
        MapState::Open(Box::new(m))
    }

    /// Is the map's help panel still on screen? (captures that small screen area and compares it with the signature)
    fn panel_present(&self, m: &OpenMap) -> bool {
        match &m.panel {
            Some(sig) => {
                let (x, y, w, h) = sig.rect();
                capture::grab_gray(self.client.0 + x, self.client.1 + y, w, h).map(|g| sig.score(&g) > 0.85).unwrap_or(false)
            }
            None => false,
        }
    }

    /// One frame of the open map. Returns false if the help panel disappeared (map hidden or closed).
    fn tick_open(&mut self, m: &mut OpenMap, fg: bool, now: Instant) -> bool {
        // Animate the zoom towards its target, like the game does.
        let dt = now.duration_since(m.last).as_secs_f64().min(0.1);
        m.last = now;
        let tz = m.target_zoom();
        if (m.zoom - tz).abs() > 1e-4 {
            m.zoom += (tz - m.zoom) * (1.0 - (-ZOOM_K * dt).exp());
            if (m.zoom - tz).abs() < 1e-4 {
                m.zoom = tz;
            }
            self.dirty = true;
        }
        // End a drag if the left button is no longer held (state read only) or the game lost focus.
        let lmb_down = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(0x01) } < 0;
        if m.drag.is_some() && (!lmb_down || !fg) {
            m.drag = None;
            m.last_input = now;
        }
        if m.drag.is_some() {
            m.last_input = now; // the movement itself is followed event by event (on_mouse)
        }
        if !fg {
            return true;
        }
        // Help-panel signature, once the fade-in is over; the square is also refined if the view has not moved.
        if let Some(t) = m.panel_due {
            if now < t {
                return true;
            }
            let (x, y, w, h) = self.client;
            let (px0, py0, px1, py1) = detect::panel_rect(h as f64);
            let Some(cur) = capture::grab_gray(x + px0 as i32, y + py0 as i32, (px1 - px0) as i32, (py1 - py0) as i32) else {
                return true;
            };
            // Screen stable (fade-in over) when two successive captures are identical.
            let stable = m.panel_prev.as_ref().map(|p| detect::gray_ncc(p, &cur) > 0.97).unwrap_or(false);
            m.panel_tries += 1;
            if !stable && m.panel_tries < 10 {
                m.panel_prev = Some(cur);
                m.panel_due = Some(now + Duration::from_millis(120));
                return true;
            }
            m.panel_due = None;
            m.panel_prev = None;
            if m.n == 0 && m.off == (0.0, 0.0) && m.drag.is_none() {
                if let Some(g) = capture::grab_gray(x, y, w, h) {
                    if let Some(sq) = detect::find_square(&g) {
                        m.sq = sq;
                    }
                }
            }
            m.panel = PanelSig::from(cur, px0 as i32, py0 as i32);
            let Some(sig) = &m.panel else {
                logf!("help panel not recognised");
                return false;
            };
            self.remember = Some((m.map_idx, m.sq, sig.clone()));
            save_remember(self.client, m.map_idx, &m.sq, sig);
            m.next_panel = now + PANEL_EVERY;
        }
        // Visual resync: scheduled (focus regained, resume) or periodic while the map is still.
        // - right after a movement (as soon as the map is still and the zoom animation almost finished);
        // - periodically at rest; - on request (focus regained / back from menus: wide search).
        let quiet = m.drag.is_none() && now.duration_since(m.last_input) > Duration::from_millis(220) && (m.zoom - tz).abs() < 0.006 * tz;
        // 1) Result of a resync started earlier?
        if let Some((info, res)) = self.take_resync() {
            if info.lost.is_none() {
                match self.apply_resync(m, &info, res, now) {
                    Some(false) => return false, // map not found again after coming back
                    Some(true) => self.dirty = true,
                    None => {}
                }
            }
        }
        if m.wide_pending && self.rs_job.is_none() {
            m.wide_pending = false; // request dropped (timed out): show again
        }
        // 2) Start a resync if needed (only one at a time).
        let due = match m.resync_at {
            Some((t, _)) => now >= t && m.drag.is_none(),
            None => quiet && (m.motion || now >= m.next_idle_resync),
        };
        if due && m.closing_check.is_none() && self.rs_job.is_none() {
            // Choose the kind of search from what triggered it.
            let mode = if m.resync_at.map(|r| r.1).unwrap_or(false) {
                Resync::Wide
            } else if m.motion {
                Resync::Motion
            } else if m.uncertain {
                Resync::Global
            } else {
                Resync::Idle
            };
            m.resync_at = None;
            m.motion = false;
            m.next_idle_resync = now + Duration::from_millis(1500);
            if self.start_resync(m, mode, now) && mode == Resync::Wide {
                m.wide_pending = true;
            }
        }
        // 3) Is the help panel still there? Two misses in a row (or one during a closing check) = map gone.
        if now >= m.next_panel || m.closing_check.map(|t| now >= t).unwrap_or(false) {
            m.next_panel = now + PANEL_EVERY;
            if self.panel_present(m) {
                m.panel_miss = 0;
                if m.closing_check.take().is_some() {
                    self.dirty = true;
                }
            } else {
                m.panel_miss += 1;
                if m.panel_miss >= 2 || m.closing_check.is_some() {
                    m.closing_check = None;
                    return false;
                }
            }
        }
        true
    }

    /// Full detection: captures the game's client area (grayscale screen pixels), finds the map square
    /// and identifies which map it is by comparing it with the bundled reference images.
    fn detect_open(&self) -> Option<OpenMap> {
        let (x, y, w, h) = self.client;
        let g = capture::grab_gray(x, y, w, h)?;
        let sq = detect::find_square(&g)?;
        let (idx, score, margin) = detect::identify(&g, &sq)?;
        logf!("identification: {} score {:.3} margin {:.3}", crate::refs::NAMES[idx], score, margin);
        Some(Self::new_open(idx, sq, Instant::now()))
    }

    /// Fresh state for a map just opened in its default view (zoom 1, no pan).
    fn new_open(idx: usize, sq: Square, now: Instant) -> OpenMap {
        OpenMap {
            map_idx: idx,
            map_id: crate::refs::NAMES[idx],
            sq,
            n: 0,
            zoom: 1.0,
            off: (0.0, 0.0),
            panel: None,
            panel_due: Some(now + PANEL_SIG_DELAY),
            panel_prev: None,
            panel_tries: 0,
            panel_miss: 0,
            next_panel: now + PANEL_EVERY,
            closing_check: None,
            last: now,
            drag: None,
            wheel_acc: 0,
            n_a: 0,
            n_b: 0,
            motion: false,
            wide_pending: false,
            uncertain: false,
            n_sync: 0,
            last_input: now,
            // First resync right after opening (corrects a square detection that is off by a few pixels).
            resync_at: Some((now + Duration::from_millis(320), false)),
            next_idle_resync: now + Duration::from_millis(1200),
            points: vec![],
            legend: vec![],
            layers: vec![],
            available: vec![],
            allies: vec![],
            up_next: vec![],
            repeat_avail: 0,
            data_version: u64::MAX,
        }
    }

    /// Map closed as far as we know, but help panel visible: the map is open and already zoomed/moved
    /// (app restarted, state lost…). It is found again by a full search (at most every 1.5 s).
    fn relocate_lost(&mut self, now: Instant) -> Option<OpenMap> {
        // Result of a full search started earlier?
        if let Some((info, res)) = self.take_resync() {
            let (Some((_, sq, sig)), Some(f)) = (info.lost, res.found) else { return None };
            let idx = res.map;
            logf!("full search: {} score {:.3}, n {}, {} ms", crate::refs::NAMES[idx], f.score, f.n, res.ms);
            if f.score < 0.6 {
                return None;
            }
            // Rebuild the open-map state from the zoom level and position that were found.
            let center = (sq.x0 + sq.side / 2.0, sq.y0 + sq.side / 2.0);
            let mut m = Self::new_open(idx, sq, now);
            let side = (1.0 + 0.5 * f.n as f64) * sq.side;
            m.n = f.n;
            m.n_a = f.n;
            m.n_b = f.n;
            m.zoom = 1.0 + 0.5 * f.n as f64;
            m.off = (f.x0 + side / 2.0 - center.0, f.y0 + side / 2.0 - center.1);
            m.panel = Some(sig);
            m.panel_due = None;
            m.resync_at = None;
            return Some(m);
        }
        if self.rs_job.is_some() || now < self.next_global {
            return None;
        }
        // Only possible if we know which map was last seen (and where its help panel is).
        if self.remember.is_none() {
            self.remember = load_remember(self.client);
        }
        let (idx, sq, sig) = self.remember.clone()?;
        let (x, y, w, h) = self.client;
        let (sx, sy, sw, sh) = sig.rect();
        let present = capture::grab_gray(x + sx, y + sy, sw, sh).map(|g| sig.score(&g) > 0.85).unwrap_or(false);
        if !present {
            return None;
        }
        // Help panel visible but map not recognised in its default view: it is open, zoomed or moved.
        // Capture the client area and hand it to the worker thread for a full search.
        self.next_global = now + Duration::from_millis(1500);
        let g = capture::grab_gray(x, y, w, h)?;
        let center = (sq.x0 + sq.side / 2.0, sq.y0 + sq.side / 2.0);
        let id = self.rs_next_id;
        self.rs_next_id += 1;
        let job = resync::Job { id, g, masks: self.masks(), map: idx, side0: sq.side, center, pred: (sq.x0, sq.y0), n_pred: 0, zooms: vec![], radius: 0.0, kind: resync::Kind::Global };
        if self.rs_tx.send(job).is_ok() {
            self.rs_job = Some(JobInfo { id, mode: Resync::Wide, time: now, map_idx: idx, n: 0, n_a: 0, n_b: 0, pred: (sq.x0, sq.y0), lost: Some((idx, sq, sig)) });
        }
        None
    }

    /// Learns how the game counts wheel notches, from the level actually found by a resync.
    fn learn_wheel(&mut self, found: i32, n_a: i32, n_b: i32) {
        if n_a == n_b {
            return;
        }
        if found == n_b {
            self.wheel_votes.1 += 1;
        } else if found == n_a {
            self.wheel_votes.0 += 1;
        } else {
            return;
        }
        let want = if self.wheel_votes.1 >= 2 && self.wheel_votes.1 > self.wheel_votes.0 {
            Some(true)
        } else if self.wheel_votes.0 >= 2 && self.wheel_votes.0 > self.wheel_votes.1 {
            Some(false)
        } else {
            None
        };
        if want.is_some() && want != self.cfg.wheel_per_event {
            self.cfg.wheel_per_event = want;
            self.cfg.save();
            logf!("wheel: the game counts {}", if want == Some(true) { "one notch per event" } else { "one notch per 120 units" });
        }
    }

    /// Rebuilds the open map's points when the data changes.
    /// Also picks up what the network thread has received (status, key validation, update notice, search
    /// data) and asks it for missing search data. No game data is involved: everything comes from the website.
    fn sync_data(&mut self) {
        self.refresh_graph();
        // Interface language: the one chosen in the settings, otherwise Windows' (English when not offered).
        let s = i18n::t_for(&self.cfg.locale);
        if !std::ptr::eq(s, self.s) {
            self.s = s;
            self.dirty = true;
        }
        // Network status → tray tooltip; API-key validation result → settings window.
        let mut sh = self.shared.lock().unwrap();
        let st = sh.status.clone();
        if st != self.last_status {
            self.last_status = st.clone();
            self.dirty = true;
            let tip = format!("Hunger Overlay — {}", status_text(self.s, st.as_ref()));
            tray::set_tip(self.main, &tip);
        }
        if let Some(r) = &sh.validate {
            if let Some(set) = self.settings.as_mut() {
                let t = match r {
                    Ok(user) => format!("✔ {} {}", self.s.st_valid, user),
                    Err(e) => format!("✖ {}", status_text(self.s, Some(e))),
                };
                set.set_status(&t);
            }
        }
        if sh.update.is_some() && !self.update_hint_shown {
            self.update_hint_shown = true;
            tray::balloon(self.main, "Hunger Overlay", self.s.update_available);
        }
        // Item search: index (names) and files of the searched items, requested from the website only once.
        let need_index = (!self.cfg.search_items.is_empty() || self.search.is_some()) && sh.search_index.is_none();
        if need_index && !self.index_requested {
            self.index_requested = true;
            let _ = self.net.send(Req::FetchIndex);
        }
        for k in &self.cfg.search_items {
            if !sh.search_items.contains_key(k) && self.items_requested.insert(k.clone()) {
                let _ = self.net.send(Req::FetchItem(k.clone()));
            }
        }
        if let (Some(st), Some(ix)) = (self.search.as_mut(), sh.search_index.as_ref()) {
            if !st.has_entries() {
                st.set_entries(search_entries(Some(ix), sh.pack.as_ref(), sh.tracker.as_ref(), &self.graph, self.s));
            }
        }
        // Write refused by the website (quest tracking): tell the user.
        if let Some(n) = sh.notice.take() {
            let msg = match n {
                "unsupported" => self.s.track_unsupported,
                "readonly" => self.s.track_readonly,
                _ => self.s.track_failed,
            };
            self.toast = Some((panel::Toast { text: msg.to_string(), ok: false, undo: false, next: None }, Instant::now()));
            self.recent_track = None;
            self.last_record = None;
            self.dirty = true;
            // Read-only key: the tray notification links to the website page where the key can be changed.
            if n == "readonly" {
                self.balloon_url = Some(format!("{}/account/security#overlay-keys", self.cfg.base_url.trim_end_matches('/')));
                tray::balloon(self.main, "Hunger Overlay", msg);
            }
        }
        // New layers published by the website: remember them and switch on those that are on by default.
        if let Some(p) = sh.pack.as_ref() {
            let mut changed = false;
            for ly in &p.layers {
                let id = if ly.id.starts_with("harvest_") { "harvest".to_string() } else { ly.id.clone() };
                if !self.cfg.layers_known.contains(&id) {
                    self.cfg.layers_known.push(id.clone());
                    // Website default, plus Extractions (always useful) on first sight.
                    if (ly.default_on || id == "extractions") && !self.cfg.layers_on.contains(&id) {
                        self.cfg.layers_on.push(id);
                    }
                    changed = true;
                }
            }
            if changed {
                self.cfg.save();
            }
        }
        // Stable quest numbers: each tracked quest keeps its badge slot.
        if let Some(t) = sh.tracker.as_ref() {
            let ids: Vec<&str> = t.tracked.iter().filter(|q| !q.completed).map(|q| q.quest_id.as_str()).collect();
            if self.cfg.update_quest_slots(&ids) {
                self.cfg.save();
                if let MapState::Open(m) = &mut self.state {
                    m.data_version = u64::MAX;
                }
            }
        }
        // Rebuild the markers / legend of the open map if the data or the display options changed.
        let MapState::Open(m) = &mut self.state else { return };
        if m.data_version == sh.version {
            return;
        }
        m.data_version = sh.version;
        if let (Some(p), Some(t)) = (&sh.pack, &sh.tracker) {
            let searched: Vec<(String, String, Option<&[model::Marker]>)> = self
                .cfg
                .search_items
                .iter()
                .map(|k| {
                    let name = sh.search_index.as_ref().and_then(|ix| ix.items.iter().find(|i| i.item_key == *k)).map(|i| i.name.clone()).unwrap_or_else(|| k.clone());
                    (k.clone(), name, sh.search_items.get(k).map(|f| f.markers.as_slice()))
                })
                .collect();
            let opts = model::ViewOpts {
                map_id: m.map_id,
                show_done: self.cfg.show_done,
                hidden: &self.cfg.hidden_quests,
                min_chance: self.cfg.min_chance as f64 / 100.0,
                layers_on: &self.cfg.layers_on,
                items: sh.items.as_ref(),
                searched,
                harvest_title: self.s.harvest_layer,
                slots: &self.cfg.quest_slots,
                allies: vec![],
                startable: None,
            };
            // The player's progression and what can be started now (only when the website publishes prerequisites).
            let me = progress::me_progress(t);
            let chains = self.graph.has_chains();
            let startable: std::collections::HashSet<String> = if chains {
                p.quests.keys().filter(|q| self.graph.state(&me, q) == quests::State::Available).cloned().collect()
            } else {
                Default::default()
            };
            // Raid allies: website ones (chosen on the website, delivered with the tracker) and local ones, in adding
            // order; at most 2.
            let mut keys: Vec<String> = t.allies.iter().map(|a| a.id.clone()).chain(self.locals.allies.iter().map(|a| a.id.clone())).collect();
            keys.dedup();
            self.cfg.ally_order.retain(|k| keys.contains(k));
            for k in &keys {
                if !self.cfg.ally_order.contains(k) {
                    self.cfg.ally_order.push(k.clone());
                }
            }
            let names: Vec<(String, String)> = self
                .cfg
                .ally_order
                .iter()
                .take(2)
                .map(|k| {
                    let n = t.allies.iter().find(|a| a.id == *k).map(|a| a.name.clone()).or_else(|| self.locals.allies.iter().find(|a| a.id == *k).map(|a| a.name.clone())).unwrap_or_default();
                    (k.clone(), n)
                })
                .collect();
            let allies: Vec<model::AllyIn> = names
                .iter()
                .enumerate()
                .map(|(i, (k, name))| {
                    let other = names.get(1 - i.min(1)).filter(|_| names.len() == 2).map(|o| o.1.as_str());
                    let tracked: Vec<(String, Vec<String>, bool)> = match t.allies.iter().find(|a| a.id == *k) {
                        Some(a) => a
                            .tracked
                            .iter()
                            .filter(|q| !q.completed)
                            .map(|q| (q.quest_id.clone(), q.objectives.iter().filter(|o| o.state == "done").map(|o| o.objective_key.clone()).collect(), self.cfg.hidden_ally_quests.contains(&format!("{k}|{}", q.quest_id))))
                            .collect(),
                        None => self.locals.allies.iter().find(|a| a.id == *k).map(|a| a.tracked.iter().map(|q| (q.clone(), vec![], self.cfg.hidden_ally_quests.contains(&format!("{k}|{q}")))).collect()).unwrap_or_default(),
                    };
                    model::AllyIn {
                        key: k.clone(),
                        name: name.clone(),
                        initial: progress::initials(name, other),
                        color: progress::ALLY_COLORS[i],
                        site: k.starts_with("u_") || t.allies.iter().any(|a| a.id == *k),
                        hidden: self.cfg.hidden_allies.contains(k),
                        tracked,
                    }
                })
                .collect();
            let opts = model::ViewOpts { allies, startable: chains.then_some(&startable), ..opts };
            let v = model::build_view(p, t, &opts);
            m.points = v.points;
            m.legend = v.legend;
            m.layers = v.layers;
            m.available = v.available;
            m.allies = v.allies;
            // Up next: the first startable quest of each character, then (Quests layer on) the other startable quests
            // with spots on this map.
            m.up_next.clear();
            m.repeat_avail = 0;
            if chains {
                for (g, n) in self.graph.suggestions(&me) {
                    if let quests::Next::Available(q) = n {
                        let here = m.available.iter().any(|a| a.quest_id == q);
                        m.up_next.push(panel::UpRow { title: p.quests.get(&q).map(|x| x.title.clone()).unwrap_or_default(), giver: g, step: self.graph.step(&q), here, quest_id: q });
                    }
                }
                if self.cfg.layers_on.iter().any(|l| l == "quests") {
                    for a in &m.available {
                        if !m.up_next.iter().any(|u| u.quest_id == a.quest_id) && !self.graph.is_repeatable(&a.quest_id) {
                            m.up_next.push(panel::UpRow { quest_id: a.quest_id.clone(), title: a.title.clone(), giver: a.giver.clone(), step: self.graph.step(&a.quest_id), here: true });
                        }
                    }
                }
                m.repeat_avail = startable.iter().filter(|q| self.graph.is_repeatable(q)).count();
            }
        } else {
            m.points.clear();
            m.legend.clear();
            m.layers.clear();
            m.available.clear();
            m.allies.clear();
            m.up_next.clear();
        }
        self.dirty = true;
    }

    /// Re-aligns the map position/zoom on the screen capture. Returns Some(true) if a correction was applied.
    /// Areas to ignore for the resync: the game's help panel and our panel.
    fn masks(&self) -> Vec<(f64, f64, f64, f64)> {
        let mut v = vec![detect::panel_rect(self.client.3 as f64)];
        if let Some((l, t, r, b)) = self.legend_rect {
            v.push((l as f64, t as f64, r as f64, b as f64));
        }
        v
    }

    /// Captures the screen (game client area, grayscale) and sends a search to the worker thread.
    fn start_resync(&mut self, m: &OpenMap, mode: Resync, now: Instant) -> bool {
        let (x, y, w, h) = self.client;
        let Some(g) = capture::grab_gray(x, y, w, h) else { return false };
        // Predicted position of the map square, and the search range (zoom levels, radius) for this mode.
        let side = (1.0 + 0.5 * m.n as f64) * m.sq.side;
        let (cx, cy) = m.center();
        let pred = (cx + m.off.0 - side / 2.0, cy + m.off.1 - side / 2.0);
        let (span, radius, kind) = match mode {
            Resync::Wide => (2, 640.0, resync::Kind::LocalThenGlobal),
            Resync::Motion => (2, 400.0, resync::Kind::Local),
            Resync::Idle => (1, 160.0, resync::Kind::Local),
            Resync::Global => (0, 0.0, resync::Kind::Global),
        };
        let (lo, hi) = (m.n.min(m.n_sync).min(m.n_a).min(m.n_b), m.n.max(m.n_sync).max(m.n_a).max(m.n_b));
        let mut zooms: Vec<i32> = (lo - span.min(1)..=hi + span.min(1)).chain(m.n - span..=m.n + span).filter(|n| (0..=ZOOM_MAX_N).contains(n)).collect();
        zooms.sort();
        zooms.dedup();
        // Hand the job to the worker thread; the result is picked up later by `take_resync`.
        let id = self.rs_next_id;
        self.rs_next_id += 1;
        let center = (m.sq.x0 + m.sq.side / 2.0, m.sq.y0 + m.sq.side / 2.0);
        let job = resync::Job { id, g, masks: self.masks(), map: m.map_idx, side0: m.sq.side, center, pred, n_pred: m.n, zooms, radius, kind };
        if self.rs_tx.send(job).is_err() {
            return false;
        }
        self.rs_job = Some(JobInfo { id, mode, time: now, map_idx: m.map_idx, n: m.n, n_a: m.n_a, n_b: m.n_b, pred, lost: None });
        true
    }

    /// Worker-thread result matching the current request (or the request is dropped after 5 s).
    fn take_resync(&mut self) -> Option<(JobInfo, resync::Res)> {
        let info = self.rs_job.as_ref()?;
        let mut slot = self.rs_out.lock().unwrap();
        let ready = slot.as_ref().map(|r| r.id == info.id).unwrap_or(false);
        if !ready {
            if info.time.elapsed() > Duration::from_secs(5) {
                drop(slot);
                self.rs_job = None;
            }
            return None;
        }
        let res = slot.take()?;
        drop(slot);
        Some((self.rs_job.take()?, res))
    }

    /// Applies a resync result. Some(true) = corrected, Some(false) = map lost, None = nothing to do.
    fn apply_resync(&mut self, m: &mut OpenMap, info: &JobInfo, res: resync::Res, now: Instant) -> Option<bool> {
        let wide = info.mode == Resync::Wide;
        if wide {
            m.wide_pending = false;
        }
        if info.map_idx != m.map_idx || res.map != m.map_idx {
            return None;
        }
        // The map moved since the capture: stale result (a new resync will follow the movement).
        if m.last_input > info.time {
            if wide {
                m.resync_at = Some((now, true));
            }
            return None;
        }
        // Failed resync (score too low): uncertain position → points hidden, full search at rest.
        let weak = res.found.as_ref().map(|f| f.score.max(f.score_pred) < 0.5).unwrap_or(true);
        if weak && !wide {
            if !m.uncertain {
                logf!("uncertain position: points hidden, full search");
            }
            m.uncertain = true;
            m.next_idle_resync = now + Duration::from_millis(if info.mode == Resync::Global { 2500 } else { 300 });
            self.dirty = true;
        }
        let Some(f) = res.found else {
            return if wide { Some(false) } else { None };
        };
        if f.score >= 0.6 && m.uncertain {
            m.uncertain = false;
            self.dirty = true;
        }
        if f.score >= 0.6 {
            m.n_sync = f.n;
        }
        // Compare the found position with the predicted one; correct only if clearly better and different.
        let side_p = (1.0 + 0.5 * info.n as f64) * m.sq.side;
        let side_f = (1.0 + 0.5 * f.n as f64) * m.sq.side;
        let shift = ((f.x0 + side_f / 2.0 - (info.pred.0 + side_p / 2.0)).powi(2) + (f.y0 + side_f / 2.0 - (info.pred.1 + side_p / 2.0)).powi(2)).sqrt();
        let better = f.score > f.score_pred + 0.02 || f.score_pred < 0.4;
        let apply = f.score >= 0.6 && (f.n != info.n || shift > 2.0) && better;
        logf!(
            "resync {:?}{}: score {:.3} (predicted {:.3}) n {}→{} offset {:.1} px, {} ms{}",
            info.mode,
            if res.global_used { " + full search" } else { "" },
            f.score,
            f.score_pred,
            info.n,
            f.n,
            shift,
            res.ms,
            if apply { " → CORRECTED" } else { "" }
        );
        if f.score >= 0.6 {
            self.learn_wheel(f.n, info.n_a, info.n_b);
        }
        if wide && f.score.max(f.score_pred) < 0.5 {
            logf!("map not found on screen again after resuming");
            return Some(false);
        }
        if apply {
            let (cx, cy) = m.center();
            m.n = f.n;
            m.zoom = 1.0 + 0.5 * f.n as f64;
            m.off = (f.x0 + side_f / 2.0 - cx, f.y0 + side_f / 2.0 - cy);
        }
        m.n_a = m.n;
        m.n_b = m.n;
        m.wheel_acc = 0;
        apply.then_some(true)
    }

    /// Marker size factor: UI scale × the user's marker-size setting.
    fn marker_k(&self) -> f32 {
        (self.sc() * self.cfg.marker_scale).max(0.45)
    }

    /// Zoom metric Z of the open map: 1 = the game's default (fully zoomed-out) view.
    fn zoom_z(&self, m: &OpenMap) -> f32 {
        (m.zoom * m.sq.side / (0.9486 * self.client.3.max(1) as f64)) as f32
    }

    /// Markers of the last frame under a point (client coordinates), closest first.
    fn items_at(&self, lx: f32, ly: f32) -> Vec<usize> {
        let (k, sc) = (self.marker_k(), self.sc());
        let mut v: Vec<(f32, usize)> = self
            .scene
            .iter()
            .enumerate()
            .filter_map(|(i, it)| {
                let lim = (it.radius(k) * 1.25).max(12.0 * sc) + 4.0 * sc;
                let d = (it.x - lx).powi(2) + (it.y - ly).powi(2);
                (d < lim * lim).then_some((d, i))
            })
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v.into_iter().map(|x| x.1).collect()
    }

    /// Marker under the mouse cursor (tooltip), identified by the first point it stands for.
    fn compute_hover(&self) -> Option<usize> {
        let MapState::Open(m) = &self.state else { return None };
        if m.drag.is_some() || m.uncertain {
            return None;
        }
        let (cx, cy) = cursor();
        if self.over_panel(cx, cy) {
            return None;
        }
        let (lx, ly) = ((cx - self.client.0) as f32, (cy - self.client.1) as f32);
        self.items_at(lx, ly).first().map(|&i| self.scene[i].members[0])
    }

    // ---------- Drawing ----------
    // Everything is drawn into our own windows (overlay + panel); nothing is drawn into the game.

    /// Puts the settings and search windows back above the overlay and the panel (all are "topmost"; the last one
    /// shown would otherwise cover them).
    fn raise_dialogs(&self) {
        let dialogs = [self.settings.as_ref().map(|s| s.hwnd), self.search.as_ref().map(|s| s.hwnd), self.journal.as_ref().map(|j| j.hwnd)];
        for h in dialogs.into_iter().flatten() {
            unsafe {
                let _ = SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
    }

    /// Shows or hides the clickable panel window (without taking focus from the game).
    fn show_panel(&mut self, on: bool) {
        if on != self.panel_shown {
            self.panel_shown = on;
            if !on {
                self.panel_hover = None;
                self.spot = None;
                self.spot_pending = None;
            }
            unsafe {
                let _ = ShowWindow(self.panel, if on { SW_SHOWNOACTIVATE } else { SW_HIDE });
                if on {
                    self.raise_dialogs();
                }
            }
        }
    }

    /// Redraws the overlay: the panel (its own window), then the markers projected onto the map, place labels and
    /// the tooltip of the hovered marker (docs/DESIGN.md §4–6).
    fn draw(&mut self) {
        let lang_label = self.lang_label();
        let (gx, gy, gw, gh) = self.client;
        let sc = self.sc();
        let k = self.marker_k();
        let now = Instant::now();
        let _ = self.renderer.set_font_scale(sc);
        if !self.renderer.begin(gw, gh) {
            return;
        }
        let can_done = self.site_v12();
        let MapState::Open(m) = &self.state else {
            self.renderer.present(self.overlay, gx, gy);
            return;
        };

        // 1. Panel: to the right of the map by default, or wherever the user moved it (position stored as
        //    fractions of the window, so it stays valid at another resolution).
        let max_w = if self.cfg.panel_collapsed { 360.0 } else { 430.0 } * sc;
        let (right, top, lw) = match self.cfg.panel_pos {
            Some((fx, fy)) => {
                let x = (fx * gw as f32).clamp(0.0, (gw as f32 - 200.0 * sc).max(0.0));
                (x, (fy * gh as f32).clamp(0.0, gh as f32 - 60.0 * sc), max_w.min(gw as f32 - x - 4.0))
            }
            None => {
                let x = (m.sq.x0 + m.sq.side) as f32 + 24.0 * sc;
                (x, 60.0 * sc, (gw as f32 - x - 16.0 * sc).min(max_w))
            }
        };
        let mut legend_rect = None;
        if lw > 180.0 * sc {
            let sq_bottom = (m.sq.y0 + m.sq.side) as f32;
            let max_h = (sq_bottom.min(gh as f32 - 20.0 * sc) - top).max(260.0 * sc).min(gh as f32 - top - 8.0 * sc);
            let mut view = panel::View {
                s: self.s,
                legend: &m.legend,
                layers: &m.layers,
                status: self.last_status.as_ref(),
                show_done: self.cfg.show_done,
                min_chance: self.cfg.min_chance,
                lang_label,
                collapsed: self.cfg.panel_collapsed,
                opacity: self.cfg.panel_opacity.clamp(20, 100) as f32 / 100.0,
                sections_collapsed: &self.cfg.sections_collapsed,
                toast: self.toast.as_ref().map(|t| &t.0),
                avail_open: self.avail_open,
                other_open: &self.other_open,
                lang_open: self.lang_open,
                locale: &self.cfg.locale,
                quest_markers_hidden: self.cfg.quest_markers_hidden,
                recent: self.recent_track.as_ref().map(|t| t.0.as_str()),
                up_next: &m.up_next,
                repeat_avail: m.repeat_avail,
                can_done: can_done,
                allies: &m.allies,
                uncertain: m.uncertain,
                compact: 0,
                scroll: 0.0,
            };
            let _ = self.prenderer.set_font_scale(sc);
            let pw = lw.floor();
            // Overflow: fold more and more (§4.5) until it fits, then scroll.
            let mut out = panel::layout(&self.prenderer, &view, pw, max_h, sc, None, false);
            while out.content_h > max_h && view.compact < 4 {
                view.compact += 1;
                out = panel::layout(&self.prenderer, &view, pw, max_h, sc, None, false);
            }
            self.panel_scroll = self.panel_scroll.clamp(0.0, out.max_scroll);
            view.scroll = self.panel_scroll;
            let ph = out.h.ceil();
            // The image is a little larger than the panel, for its drop shadow.
            let (iw, ih) = ((pw + 4.0 * sc).ceil() as i32, (ph + 6.0 * sc).ceil() as i32);
            if self.prenderer.begin(iw, ih) {
                self.prenderer.fill_rrect(3.0 * sc, 5.0 * sc, pw + 3.0 * sc, ph + 5.0 * sc, 7.0 * sc, (0.0, 0.0, 0.0, 0.25));
                let out = panel::layout(&self.prenderer, &view, pw, ph, sc, self.panel_hover, true);
                self.prenderer.present(self.panel, gx + right as i32, gy + top as i32);
                self.panel_hits = out.hits;
            }
            legend_rect = Some((right, top, right + pw, top + ph));
        }

        // 2. Markers: skipped while the position is uncertain, and clipped to the window and to the map texture.
        let r = &self.renderer;
        let (mx0, my0) = m.project(0.0, 0.0);
        let (mx1, my1) = m.project(1.0, 1.0);
        let visible: Vec<(usize, f32, f32)> = if m.uncertain {
            vec![]
        } else {
            m.points
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    let (x, y) = m.project(p.u, p.v);
                    let inside = x >= mx0 && x <= mx1 && y >= my0 && y <= my1 && x > -20.0 && y > -20.0 && x < gw as f64 + 20.0 && y < gh as f64 + 20.0;
                    // Not under the panel (it is translucent: markers would show through it).
                    let under_panel = legend_rect.map(|(l, t, r, b)| x as f32 > l - 4.0 && (x as f32) < r + 4.0 && y as f32 > t - 4.0 && (y as f32) < b + 4.0).unwrap_or(false);
                    // Eye of the QUESTS header: my quests and the allies' are hidden, layers / crafting / searches stay.
                    let quest_marker = p.layer.is_none() && (p.ally.is_some() || m.legend.get(p.quest).map(|l| l.kind == model::OwnerKind::Quest).unwrap_or(false));
                    let hidden = self.cfg.quest_markers_hidden && quest_marker;
                    (inside && !under_panel && !hidden).then_some((i, x as f32, y as f32))
                })
                .collect()
        };
        let z = self.zoom_z(m);
        let initials: Vec<String> = m.allies.iter().map(|a| a.initial.clone()).collect();
        let items = scene::build(&scene::Input { visible: &visible, points: &m.points, legend: &m.legend, layers: &m.layers, zoom: z, sc, spot: self.spot.as_ref(), ally_initials: &initials });
        let hovered = self.hover.and_then(|h| items.iter().position(|it| it.members[0] == h));
        let lit = |it: &scene::Item| self.spot.as_ref().map(|s| it.lit(s, &m.points)).unwrap_or(true);
        let dim = |it: &scene::Item| if lit(it) { 1.0 } else { 0.35 };
        // Petals: leader lines from the true position, and a small ivory dot there.
        for it in &items {
            if let Some((ax, ay)) = it.anchor {
                let a = dim(it);
                r.line_round(ax, ay, it.x, it.y, 2.6 * sc, (markers::HALO.0, markers::HALO.1, markers::HALO.2, 0.55 * a));
                r.line_round(ax, ay, it.x, it.y, 1.2 * sc, (markers::IVORY.0, markers::IVORY.1, markers::IVORY.2, a));
            }
        }
        for it in &items {
            if let Some((ax, ay)) = it.anchor {
                r.fill_circle(ax, ay, 3.2 * sc, (markers::HALO.0, markers::HALO.1, markers::HALO.2, 0.6));
                r.fill_circle(ax, ay, 2.0 * sc, (markers::IVORY.0, markers::IVORY.1, markers::IVORY.2, 1.0));
            }
        }
        for (n, it) in items.iter().enumerate() {
            if Some(n) == hovered {
                continue;
            }
            let kk = k * if it.members.len() > 1 { 1.15 } else { 1.0 };
            markers::draw(r, it.kind, it.color, &it.badge, it.x, it.y, kk, sc, dim(it));
            if it.members.len() > 1 {
                markers::count_pill(r, it.members.len(), it.color, it.x, it.y, sc, dim(it));
            }
            // Allies on the same spot for the same quest: pips on my marker.
            let mut pips: Vec<usize> = it.members.iter().flat_map(|&pi| m.points[pi].pips.iter().copied()).collect();
            pips.sort();
            pips.dedup();
            for ai in pips {
                if let Some(a) = m.allies.get(ai) {
                    markers::pip(r, it.x, it.y, it.radius(k), ai, a.color, &a.initial, k, dim(it));
                }
            }
        }
        // Spotlight pulse: once, or twice for the quests of the "Available on this map" list.
        if let Some(spot) = &self.spot {
            let t = now.duration_since(self.spot_start).as_secs_f32() / 0.9;
            let times = if matches!(spot, Spot::Quest(_)) { 2.0 } else { 1.0 };
            if t < times {
                for it in items.iter().filter(|it| lit(it)) {
                    markers::pulse(r, it.x, it.y, it.radius(k), k, it.color, t.fract());
                }
            }
        }

        // 3. Labels: names of places (and of tracked spots when zoomed in), on dark plaques, never overlapping
        //    the panel, another label, a token or the window edge; a label that does not fit is dropped.
        let mut placed: Vec<(f32, f32, f32, f32)> = vec![];
        let blockers: Vec<(f32, f32, f32)> = items.iter().filter(|it| markers::z(it.kind) >= 6).map(|it| (it.x, it.y, it.radius(k) + 3.0 * sc)).collect();
        let mut seen_names: HashSet<String> = HashSet::new();
        let mut cands: Vec<(u8, usize, String)> = vec![];
        for (n, it) in items.iter().enumerate() {
            let p = &m.points[it.members[0]];
            if it.kind == markers::Kind::Place && !p.label.is_empty() {
                cands.push((1, n, p.label.clone()));
            } else if z >= 3.0 && matches!(it.kind, markers::Kind::Tracked | markers::Kind::TrackedOptional) && it.members.len() == 1 && p.chance.is_none() && p.container.is_none() {
                let label = p.objective.and_then(|oi| m.legend.get(p.quest).and_then(|q| q.objectives.get(oi))).map(|o| o.label.clone()).unwrap_or_default();
                if !label.is_empty() {
                    cands.push((0, n, label));
                }
            }
        }
        cands.sort_by_key(|c| c.0);
        let lh = 20.0 * sc;
        for (_, n, label) in cands {
            if !seen_names.insert(label.clone()) {
                continue;
            }
            let it = &items[n];
            let (tw, th) = r.measure(&label, Font::Section, 320.0 * sc);
            let w = tw + 12.0 * sc;
            let rad = it.radius(k);
            let gap = rad + 6.0 * sc;
            let slots = [
                (it.x + gap, it.y - lh / 2.0),
                (it.x - gap - w, it.y - lh / 2.0),
                (it.x - w / 2.0, it.y - gap - lh),
                (it.x - w / 2.0, it.y + gap),
            ];
            let free = |l: f32, t: f32| {
                let (rr, b) = (l + w, t + lh);
                if l < 4.0 || t < 4.0 || rr > gw as f32 - 4.0 || b > gh as f32 - 4.0 {
                    return false;
                }
                if let Some((pl, pt, pr, pb)) = legend_rect {
                    if rr > pl && l < pr && b > pt && t < pb {
                        return false;
                    }
                }
                if placed.iter().any(|&(ql, qt, qr, qb)| rr + 4.0 * sc > ql && l - 4.0 * sc < qr && b + 3.0 * sc > qt && t - 3.0 * sc < qb) {
                    return false;
                }
                // Marker footprints (circle vs rectangle), the labelled marker itself excepted.
                !blockers.iter().any(|&(bx, by, br)| {
                    if (bx - it.x).abs() < 0.5 && (by - it.y).abs() < 0.5 {
                        return false;
                    }
                    let (cx, cy) = (bx.clamp(l, rr), by.clamp(t, b));
                    (cx - bx).powi(2) + (cy - by).powi(2) < br * br
                })
            };
            if let Some(&(l, t)) = slots.iter().find(|&&(l, t)| free(l, t)) {
                let a = dim(it);
                r.fill_rrect(l, t, l + w, t + lh, 4.0 * sc, (0.082, 0.067, 0.047, 0.82 * a));
                r.stroke_rrect(l + 0.5, t + 0.5, l + w - 0.5, t + lh - 0.5, 4.0 * sc, 1.0, (0.784, 0.655, 0.396, 0.30 * a));
                r.text(&label, Font::Section, l + 6.0 * sc, t + (lh - th) / 2.0, tw + 2.0, (0.945, 0.918, 0.859, a));
                placed.push((l, t, l + w, t + lh));
            }
        }

        // 4. Hovered marker on top (×1.25 + focus ring), then its tooltip.
        if let Some(n) = hovered {
            let it = &items[n];
            let kk = k * 1.25 * if it.members.len() > 1 { 1.15 } else { 1.0 };
            markers::draw(r, it.kind, it.color, &it.badge, it.x, it.y, kk, sc, 1.0);
            markers::focus_ring(r, it.x, it.y, markers::radius(it.kind) * kk, k);
            if it.members.len() > 1 {
                markers::count_pill(r, it.members.len(), it.color, it.x, it.y, sc, 1.0);
            }
            // Every marker under the cursor gets a section (at most 3).
            let (cx, cy) = cursor();
            let (lx, ly) = ((cx - gx) as f32, (cy - gy) as f32);
            let near: Vec<usize> = {
                let mut v: Vec<(f32, usize)> = items
                    .iter()
                    .enumerate()
                    .filter_map(|(i, o)| {
                        let lim = (o.radius(k) * 1.25).max(12.0 * sc) + 4.0 * sc;
                        let d = (o.x - lx).powi(2) + (o.y - ly).powi(2);
                        (i == n || d < lim * lim).then_some((if i == n { -1.0 } else { d }, i))
                    })
                    .collect();
                v.sort_by(|a, b| a.0.total_cmp(&b.0));
                v.into_iter().map(|x| x.1).collect()
            };
            let secs: Vec<TipSection> = near.iter().take(3).map(|&i| tip_for(self.s, m, &items[i])).collect();
            let more = near.len().saturating_sub(3);
            draw_tooltip(r, self.s, &secs, more, it.x, it.y, it.radius(k * 1.25), gw as f32, gh as f32, legend_rect, sc);
        }
        // Push the finished image to the transparent overlay window.
        self.renderer.present(self.overlay, gx, gy);
        self.scene = items;
        self.legend_rect = legend_rect;
        self.show_panel(legend_rect.is_some());
    }

    // ---------- Item search ----------

    /// Opens the "Search for an item" window (or brings it to the front); fetches the item index if needed.
    fn open_search(&mut self) {
        if let Some(s) = &self.search {
            unsafe {
                let _ = SetForegroundWindow(s.hwnd);
            }
            return;
        }
        let (entries, loaded) = {
            let sh = self.shared.lock().unwrap();
            (search_entries(sh.search_index.as_ref(), sh.pack.as_ref(), sh.tracker.as_ref(), &self.graph, self.s), sh.search_index.is_some())
        };
        if !loaded && !self.index_requested {
            self.index_requested = true;
            let _ = self.net.send(Req::FetchIndex);
        }
        self.search = search::Search::open(self.s, &self.cfg.theme, Some(entries), loaded, self.client);
    }

    /// An entry was picked in the search window: a quest is (un)tracked, an item is added to the map.
    fn search_pick(&mut self, key: String) {
        // Quest: track / untrack (written to the website account; the website returns the new tracker).
        if let Some(qid) = key.strip_prefix("quest:") {
            // Available: tracked at once (toast + Undo). Tracked: nothing to do. Locked or done: opens the journal on
            // it, where the cascade is previewed (the search never cascades on its own).
            let me = self.progress_of(progress::ME);
            let st = self.graph.state(&me, qid);
            self.close_search();
            match st {
                quests::State::Available => {
                    let title = self.quest_title(qid);
                    self.track_quest(qid.to_string(), title, true);
                }
                quests::State::Tracked => {}
                _ if self.graph.has_chains() => self.open_journal(Some(qid.to_string())),
                // Website without prerequisites: plain track / untrack, as before.
                _ => {
                    let title = self.quest_title(qid);
                    self.track_quest(qid.to_string(), title, !me.tracked.contains(qid));
                }
            }
            return;
        }
        // Item: remember it in the settings and download its (public) marker file.
        if !self.cfg.search_items.contains(&key) {
            self.cfg.search_items.push(key.clone());
            self.cfg.save();
        }
        if self.items_requested.insert(key.clone()) {
            let _ = self.net.send(Req::FetchItem(key));
        }
        self.close_search();
        if let MapState::Open(m) = &mut self.state {
            m.data_version = u64::MAX;
        }
        self.dirty = true;
    }

    /// Closes the search window and gives the focus back to the game.
    fn close_search(&mut self) {
        if let Some(s) = self.search.take() {
            s.close();
        }
        // Back to the game (we are in the foreground, so this is allowed).
        unsafe {
            let mut game = HWND::default();
            let _ = EnumWindows(Some(find_game), LPARAM(&mut game as *mut HWND as isize));
            if !game.is_invalid() {
                let _ = SetForegroundWindow(game);
            }
        }
    }

    // ---------- Settings ----------

    /// Opens the settings window (or brings it to the front), pre-filled with the current settings.
    fn open_settings(&mut self) {
        if let Some(s) = self.settings.as_mut() {
            unsafe {
                let _ = SetForegroundWindow(s.hwnd);
            }
            return;
        }
        self.shared.lock().unwrap().validate = None;
        let key = self.cfg.api_key().unwrap_or_default();
        let auto_lang = i18n::auto_label();
        self.settings = settings::Settings::open(
            self.s,
            settings::Init {
                key: &key,
                vk: self.cfg.map_vk,
                show_done: self.cfg.show_done,
                capture: self.cfg.capture_visible,
                autostart: self.cfg.autostart,
                locale: &self.cfg.locale,
                min_chance: self.cfg.min_chance,
                auto_update: self.cfg.auto_update,
                theme: &self.cfg.theme,
                panel_opacity: self.cfg.panel_opacity,
                auto_lang: auto_lang.clone(),
            },
        );
    }

    /// Handles a button / control of the settings window.
    fn settings_command(&mut self, id: i32) {
        match id {
            settings::ID_CHANCE => {
                if let Some(s) = self.settings.as_mut() {
                    s.chance_moved();
                }
            }
            settings::ID_KEY_SHOW => {
                if let Some(s) = self.settings.as_mut() {
                    s.toggle_mask();
                }
            }
            settings::ID_MAPKEY => {
                self.capture_key = true;
                if let Some(s) = self.settings.as_mut() {
                    s.set_mapkey_text(self.s.press_key);
                }
            }
            // "Test" button: checks the typed API key's format, then asks the website whether it is valid.
            settings::ID_TEST => {
                if let Some(s) = self.settings.as_mut() {
                    let k = s.key_text();
                    if !net::key_format_ok(&k) {
                        s.set_status(self.s.st_bad_format);
                        return;
                    }
                    s.set_status(self.s.st_testing);
                    self.shared.lock().unwrap().validate = None;
                    let _ = self.net.send(Req::Configure { key: self.cfg.api_key(), base_url: self.cfg.base_url.clone(), locale: self.cfg.locale.clone() });
                    let _ = self.net.send(Req::Validate(k));
                }
            }
            settings::ID_SAVE | settings::ID_APPLY => {
                if !self.apply_settings() {
                    return;
                }
                if id == settings::ID_SAVE {
                    if let Some(s) = self.settings.take() {
                        s.close();
                    }
                    self.capture_key = false;
                } else if let Some(s) = self.settings.as_mut() {
                    s.set_status(self.s.st_applied);
                }
            }
            settings::ID_CANCEL => {
                if let Some(s) = self.settings.take() {
                    s.close();
                }
                self.cfg.map_vk = Config::load().map_vk;
                self.capture_key = false;
            }
            _ => {}
        }
    }
}

impl App {
    /// Installs a downloaded update, never while the map is shown; checks every 6 h.
    fn maybe_install_update(&mut self, now: Instant) {
        if now >= self.next_update_check {
            self.next_update_check = now + Duration::from_secs(6 * 3600);
            let _ = self.net.send(Req::CheckUpdate { install: self.cfg.auto_update });
        }
        if matches!(self.state, MapState::Open(_)) || self.settings.is_some() {
            return;
        }
        if !(self.cfg.auto_update || self.install_requested) {
            return;
        }
        let ready = self.shared.lock().unwrap().update_ready.take();
        let Some((version, path)) = ready else { return };
        logf!("installing version {version}");
        // Put in place for the next start; this version keeps running (no relaunch: see update.rs).
        match update::install(&path) {
            Ok(()) => {
                self.shared.lock().unwrap().installed = Some(version.clone());
                tray::balloon(self.main, "Hunger Overlay", &self.s.update_installed.replace("{v}", &version));
            }
            Err(e) => {
                logf!("update failed: {e}");
                tray::balloon(self.main, "Hunger Overlay", self.s.update_available);
            }
        }
    }

    /// Applies the values of the settings window (without closing it). Returns false if the key is invalid.
    /// Settings are saved locally (see `config.rs`).
    fn apply_settings(&mut self) -> bool {
        let msg = self.s.st_bad_format;
        let Some(s) = self.settings.as_mut() else { return false };
        let v = s.values();
        if !v.key.is_empty() && !net::key_format_ok(&v.key) {
            s.set_status(msg);
            return false;
        }
        let net_changed = self.cfg.api_key().unwrap_or_default() != v.key || self.cfg.locale != v.locale;
        self.cfg.set_api_key(&v.key);
        self.cfg.map_vk = v.vk;
        self.cfg.show_done = v.show_done;
        self.cfg.capture_visible = v.capture_visible;
        self.cfg.locale = v.locale;
        self.cfg.min_chance = v.min_chance;
        if v.auto_update && !self.cfg.auto_update {
            let _ = self.net.send(Req::CheckUpdate { install: true });
        }
        self.cfg.auto_update = v.auto_update;
        self.cfg.theme = v.theme.clone();
        self.cfg.panel_opacity = v.panel_opacity;
        if v.autostart != config::autostart_enabled() {
            config::set_autostart(v.autostart);
        }
        self.cfg.autostart = v.autostart;
        self.cfg.save();
        self.apply_capture_affinity();
        if net_changed {
            // New key or language: reload (the hourly cap is still enforced by the network thread).
            self.configure_net();
            let _ = self.net.send(Req::Refresh { force: true });
        }
        if let MapState::Open(m) = &mut self.state {
            m.data_version = u64::MAX; // rebuilds the points (chance filter, completed…)
        }
        self.dirty = true;
        logf!("settings applied (key 0x{:02X}, min chance {} %)", self.cfg.map_vk, self.cfg.min_chance);
        true
    }
}

/// User-facing text for a network status.
fn status_text(s: &i18n::Strings, st: Option<&Status>) -> &'static str {
    match st {
        Some(Status::Ok) | None => s.st_ok,
        Some(Status::NoKey) => s.st_no_key,
        Some(Status::InvalidKey) => s.st_invalid_key,
        Some(Status::Offline) => s.st_offline,
        Some(Status::RateLimited) => s.st_rate,
    }
}

/// One section of a marker tooltip (docs/DESIGN.md §6).
struct TipSection {
    /// Colour of the accent bar (owner or layer colour).
    color: (f32, f32, f32),
    /// Small caps line: "<badge> · <owner> · <kind>".
    eyebrow: String,
    /// What the marker is (container, node, place, quest…).
    title: String,
    /// (key, value) lines; an empty key = a plain line.
    lines: Vec<(String, String)>,
    /// Chance (0..1), drawn as a bar.
    chance: Option<f64>,
    /// Footer: (keycap, hint), e.g. ("Ctrl", "+ click to track this quest"); empty keycap = plain hint.
    footer: Option<(String, String)>,
    /// Height line ("Height", "+5.5 m · upstairs"), added after the other lines.
    footer_height: Option<(String, String)>,
}

/// Formats a chance (0..1) as a percentage ("48 %", "2.5 %", "0.25 %").
fn pct(c: f64) -> String {
    let p = c * 100.0;
    if p >= 10.0 {
        format!("{p:.0} %")
    } else if p >= 1.0 {
        format!("{p:.1} %")
    } else {
        format!("{p:.2} %")
    }
}

/// Container name of a point: its `container` field, else the "(…)" part of its label.
fn container_of(p: &DrawPoint) -> Option<String> {
    p.container
        .clone()
        .or_else(|| p.label.rfind('(').and_then(|i| p.label[i + 1..].strip_suffix(')')).map(|t| t.trim().to_string()))
        .filter(|t| !t.is_empty())
}

/// Builds the tooltip section of one drawn marker.
fn tip_for(s: &i18n::Strings, m: &OpenMap, it: &scene::Item) -> TipSection {
    let p = &m.points[it.members[0]];
    let mut t = TipSection { color: it.color, eyebrow: String::new(), title: String::new(), lines: vec![], chance: None, footer: None, footer_height: None };
    let owner = match it.owner {
        scene::Owner::Legend(q) => m.legend.get(q).map(|lq| format!("{} · {}", lq.badge, lq.title.to_uppercase()).trim_start_matches(" · ").to_string()),
        scene::Owner::Ally(a) => m.allies.get(a).map(|al| format!("{} · {} · {}", al.initial, al.name.to_uppercase(), s.k_ally)),
        scene::Owner::Layer(_) => None,
    };
    let layer = p.layer.and_then(|li| m.layers.get(li));
    // Objective label(s) of a point: its own, then those merged into it (same spot).
    let objectives = |p: &DrawPoint| -> Vec<String> {
        let own = p.objective.and_then(|oi| m.legend.get(p.quest).and_then(|q| q.objectives.get(oi))).map(|o| o.label.clone()).unwrap_or_else(|| p.label.clone());
        std::iter::once(own).chain(p.extra.iter().cloned()).filter(|l| !l.is_empty()).collect()
    };
    let harvest_lines = |t: &mut TipSection, p: &DrawPoint| {
        if let Some(h) = &p.harvest {
            if let Some(pr) = h.profession_label.as_ref().or(h.profession.as_ref()) {
                t.lines.push((s.t_profession.into(), pr.clone()));
            }
            if let Some(lv) = h.level {
                let you = h.player.map(|pl| format!("  ({} {pl})", s.tip_you)).unwrap_or_default();
                t.lines.push((s.t_level.into(), format!("{lv}{you}")));
            }
        }
    };

    // Cluster: up to 4 members, best chance first.
    if it.members.len() > 1 {
        let n = it.members.len();
        let what = layer.map(|l| l.title.to_uppercase()).or(owner.clone()).unwrap_or_default();
        t.eyebrow = format!("{what} · {}", s.k_spots.replace("{n}", &n.to_string()));
        t.title = match it.kind {
            markers::Kind::QuestAvail => s.k_quest_avail.to_string(),
            _ => layer.map(|l| l.title.clone()).or_else(|| m.legend.get(p.quest).map(|q| q.title.clone())).unwrap_or_default(),
        };
        let mut ms: Vec<&DrawPoint> = it.members.iter().map(|&i| &m.points[i]).collect();
        ms.sort_by(|a, b| b.chance.unwrap_or(0.0).total_cmp(&a.chance.unwrap_or(0.0)));
        let mut seen = HashSet::new();
        let mut shown = 0;
        for q in &ms {
            let name = match (&q.quest_ref, &q.harvest) {
                (Some((_, qt)), _) => qt.clone(),
                (_, Some(h)) => h.node.clone().unwrap_or_else(|| q.label.clone()),
                _ => container_of(q).unwrap_or_else(|| q.label.clone()),
            };
            if !seen.insert(name.clone()) {
                continue;
            }
            if shown < 4 {
                t.lines.push((String::new(), match q.chance {
                    Some(c) => format!("{name}  ·  {}", pct(c)),
                    None => name,
                }));
            }
            shown += 1;
        }
        if shown > 4 {
            t.lines.push((String::new(), s.t_more.replace("{n}", &(shown - 4).to_string())));
        }
        t.footer = Some(match it.kind {
            markers::Kind::QuestAvail => ("Ctrl".into(), format!("{} · {}", s.t_track_footer, ms.first().and_then(|q| q.quest_ref.as_ref()).map(|r| r.1.clone()).unwrap_or_default())),
            _ => (String::new(), s.t_zoom.into()),
        });
        return t;
    }

    // Height above the ground (single markers), e.g. "Height  +5.5 m · upstairs".
    if let Some(h) = p.height {
        let what = if h < -8.0 {
            s.h_deep
        } else if h < -2.0 {
            s.h_below
        } else if h <= 2.0 {
            s.h_ground
        } else if h <= 8.0 {
            s.h_upper
        } else {
            s.h_high
        };
        // Approximate (ground grid of 7.5-10 m: up to ±10 m off near cliffs and banks), hence the "≈".
        let v = if h.abs() < 0.5 { what.to_string() } else { format!("≈ {h:+.1} m · {what}") };
        t.footer_height = Some((s.t_height.to_string(), v));
    }
    // My marker carrying allies' pips: "Also for: Marius (M)".
    let also: Vec<String> = it.members.iter().flat_map(|&i| m.points[i].pips.iter().copied()).filter_map(|a| m.allies.get(a).map(|al| format!("{} ({})", al.name, al.initial))).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    match (it.owner, it.kind) {
        // A raid ally's objective: quest, objective, container and chance.
        (scene::Owner::Ally(ai), kind) => {
            let al = m.allies.get(ai);
            let quest = p.ally.as_ref().and_then(|(_, q)| al.and_then(|a| a.quests.iter().find(|x| x.0 == *q)).map(|x| x.1.clone())).unwrap_or_default();
            let what = if p.harvest.is_some() {
                s.k_harvest
            } else if p.placed {
                s.k_placed
            } else if p.chance.is_some() || p.container.is_some() {
                s.k_container
            } else {
                s.k_objective
            };
            t.eyebrow = format!("{} · {what}", owner.unwrap_or_default());
            t.title = match (&p.harvest, kind) {
                (Some(h), _) => h.node.clone().unwrap_or_else(|| p.label.clone()),
                _ if p.chance.is_some() || p.container.is_some() => container_of(p).unwrap_or_else(|| p.label.clone()),
                _ => p.label.clone(),
            };
            t.lines.push((s.t_quest.into(), quest));
            if t.title != p.label {
                t.lines.push((String::new(), p.label.clone()));
            }
            if al.map(|a| !a.site).unwrap_or(false) {
                t.lines.push((s.t_added_by_you.into(), s.t_local_ally.into()));
            }
            t.chance = p.chance;
        }
        // Untracked quest (Quests layer).
        (_, markers::Kind::QuestAvail) => {
            t.eyebrow = s.k_quest_avail.into();
            if let Some((qid, title)) = &p.quest_ref {
                t.title = title.clone();
                let objs = objectives(p);
                for o in objs.iter().take(2) {
                    t.lines.push((String::new(), o.clone()));
                }
                if objs.len() > 2 {
                    t.lines.push((String::new(), s.t_more.replace("{n}", &(objs.len() - 2).to_string())));
                }
                if let Some(a) = m.available.iter().find(|a| a.quest_id == *qid) {
                    if !a.giver.is_empty() {
                        t.lines.push((s.t_giver.into(), a.giver.clone()));
                    }
                    t.lines.push((s.t_on_map.into(), a.spots.to_string()));
                }
            }
            t.footer = Some(("Ctrl".into(), s.t_track_footer.into()));
        }
        // Tracked quest / recipe / search.
        (scene::Owner::Legend(_), kind) => {
            let owner = owner.unwrap_or_default();
            let objs = objectives(p);
            if kind == markers::Kind::Done {
                t.eyebrow = format!("{owner} · {}", s.k_done);
                t.title = objs.first().cloned().unwrap_or_default();
            } else if let Some(h) = &p.harvest {
                t.eyebrow = format!("{owner} · {}", s.k_harvest);
                t.title = h.node.clone().unwrap_or_else(|| p.label.clone());
                harvest_lines(&mut t, p);
                t.chance = p.chance;
            } else if p.placed {
                t.eyebrow = format!("{owner} · {}", s.k_placed);
                t.title = objs.first().cloned().unwrap_or_else(|| p.label.clone());
                t.lines.push((String::new(), s.t_always.into()));
                t.chance = p.chance.filter(|c| *c < 0.999);
            } else if p.chance.is_some() || p.container.is_some() {
                t.eyebrow = format!("{owner} · {}", s.k_container);
                t.title = container_of(p).unwrap_or_else(|| p.label.clone());
                for o in &objs {
                    t.lines.push((String::new(), o.clone()));
                }
                t.chance = p.chance;
            } else {
                t.eyebrow = format!("{owner} · {}", s.k_objective);
                t.title = objs.first().cloned().unwrap_or_default();
                for o in objs.iter().skip(1) {
                    t.lines.push((String::new(), o.clone()));
                }
                if !p.label.is_empty() && !objs.contains(&p.label) {
                    t.lines.push((String::new(), p.label.clone()));
                }
            }
        }
        // Map layer.
        (scene::Owner::Layer(_), _) => {
            t.eyebrow = layer.map(|l| l.title.to_uppercase()).unwrap_or_default();
            if let Some(h) = &p.harvest {
                t.title = h.node.clone().unwrap_or_else(|| p.label.clone());
                harvest_lines(&mut t, p);
                t.chance = p.chance;
            } else {
                t.title = if p.label.is_empty() { layer.map(|l| l.title.clone()).unwrap_or_default() } else { p.label.clone() };
                if layer.map(|l| l.id == "placed_loot").unwrap_or(false) {
                    t.lines.push((String::new(), s.t_always.into()));
                    t.chance = p.chance.filter(|c| *c < 0.999);
                }
            }
        }
    }
    if !also.is_empty() {
        t.lines.push((s.t_also_for.into(), also.join(", ")));
    }
    if let Some(hl) = t.footer_height.take() {
        t.lines.push(hl);
    }
    t
}

/// Draws a marker tooltip next to the marker at (x, y) of radius `mr`: dark box with a thin gold edge, a coloured
/// accent bar per section, and stacked sections when several markers are under the cursor. It never covers the
/// marker, avoids the panel and stays inside the window.
#[allow(clippy::too_many_arguments)]
fn draw_tooltip(r: &Renderer, s: &i18n::Strings, secs: &[TipSection], more: usize, x: f32, y: f32, mr: f32, gw: f32, gh: f32, panel: Option<(f32, f32, f32, f32)>, sc: f32) {
    if secs.is_empty() {
        return;
    }
    const T100: Rgba = (0.945, 0.918, 0.859, 1.0);
    const T200: Rgba = (0.851, 0.820, 0.761, 1.0);
    const T300: Rgba = (0.702, 0.663, 0.584, 1.0);
    const GOLD_HI: Rgba = (0.922, 0.796, 0.545, 1.0);
    let (padx, pady) = (16.0 * sc, 10.0 * sc);
    let maxw = 320.0 * sc;
    let cw = maxw - 2.0 * padx;
    let gap = 3.0 * sc;
    let line_h = r.measure("Ag", Font::Chip, 1000.0).1;
    // Measure: width of the content (≤ maxw) and the height of each section.
    let mut w: f32 = 0.0;
    let mut heights = vec![];
    for t in secs {
        let mut h = 0.0;
        let (ew, eh) = r.measure_spaced(&t.eyebrow, Font::Caps, cw, 1.1 * sc);
        w = w.max(ew);
        h += eh + gap;
        let (tw, th) = r.measure(&t.title, Font::Bold, cw);
        w = w.max(tw);
        h += th + gap;
        for (k, v) in &t.lines {
            let lw = if k.is_empty() {
                let (lw, lh) = r.measure(v, Font::Text, cw);
                h += lh + gap;
                lw
            } else {
                h += line_h + gap;
                r.measure(k, Font::Chip, cw).0 + 8.0 * sc + r.measure(v, Font::Chip, cw).0
            };
            w = w.max(lw);
        }
        if t.chance.is_some() {
            h += line_h + 2.0 * gap;
            w = w.max(200.0 * sc);
        }
        if let Some((kc, hint)) = &t.footer {
            h += 8.0 * sc + line_h + 4.0 * sc;
            let kw = if kc.is_empty() { 0.0 } else { r.measure(kc, Font::Caps, 200.0).0 + 18.0 * sc };
            w = w.max(kw + r.measure(hint, Font::Chip, cw).0);
        }
        heights.push(h);
    }
    let more_t = (more > 0).then(|| s.t_more_here.replace("{n}", &more.to_string()));
    let sep = 10.0 * sc;
    let mut bh = heights.iter().sum::<f32>() + sep * (secs.len() - 1) as f32 + 2.0 * pady;
    if more_t.is_some() {
        bh += line_h + gap;
    }
    let bw = w.min(cw) + 2.0 * padx;
    // Placement: to the right of the marker, its top 24 px above the marker centre; flipped to the left if it
    // would cross the window edge or the panel.
    let mut bx = x + mr + 14.0 * sc;
    let hits_panel = |l: f32, t: f32| panel.map(|(pl, pt, pr, pb)| l + bw > pl && l < pr && t + bh > pt && t < pb).unwrap_or(false);
    let by = (y - 24.0 * sc).clamp(8.0, (gh - bh - 8.0).max(8.0));
    if bx + bw > gw - 8.0 || hits_panel(bx, by) {
        bx = x - mr - 14.0 * sc - bw;
    }
    r.fill_rrect(bx + 2.0 * sc, by + 3.0 * sc, bx + bw + 2.0 * sc, by + bh + 3.0 * sc, 8.0 * sc, (0.0, 0.0, 0.0, 0.35));
    r.fill_rrect(bx, by, bx + bw, by + bh, 8.0 * sc, (0.067, 0.055, 0.039, 0.95));
    r.stroke_rrect(bx + 0.5, by + 0.5, bx + bw - 0.5, by + bh - 0.5, 8.0 * sc, 1.0, (0.784, 0.655, 0.396, 0.38));
    let tx = bx + padx;
    let mut cy = by + pady;
    for (n, (t, h)) in secs.iter().zip(&heights).enumerate() {
        if n > 0 {
            r.line(tx, cy + sep / 2.0 - gap / 2.0, bx + bw - padx, cy + sep / 2.0 - gap / 2.0, 1.0, (0.784, 0.655, 0.396, 0.18));
            cy += sep;
        }
        // Accent bar, inside the left edge, along this section.
        r.fill_rrect(bx + 5.0 * sc, cy, bx + 8.0 * sc, cy + h - gap, 1.5 * sc, (t.color.0, t.color.1, t.color.2, 1.0));
        let (_, eh) = r.measure_spaced(&t.eyebrow, Font::Caps, cw, 1.1 * sc);
        r.text_spaced(&t.eyebrow, Font::Caps, tx, cy, cw, 1.1 * sc, T300);
        cy += eh + gap;
        cy += r.text(&t.title, Font::Bold, tx, cy, cw, T100) + gap;
        for (k, v) in &t.lines {
            if k.is_empty() {
                cy += r.text(v, Font::Text, tx, cy, cw, T200) + gap;
            } else {
                let kw = r.measure(k, Font::Chip, cw).0;
                r.text(k, Font::Chip, tx, cy, kw + 2.0, T300);
                r.text(v, Font::Chip, tx + kw + 8.0 * sc, cy, cw - kw - 8.0 * sc, T100);
                cy += line_h + gap;
            }
        }
        // Chance bar: label, 7 px track filled in the owner colour, value right-aligned.
        if let Some(c) = t.chance {
            cy += gap;
            let lw = r.measure(s.t_chance, Font::Chip, cw).0;
            r.text(s.t_chance, Font::Chip, tx, cy, lw + 2.0, T300);
            let val = pct(c);
            let vw = r.measure(&val, Font::Chip, cw).0;
            r.text(&val, Font::Chip, tx + (w.min(cw)) - vw, cy, vw + 2.0, T100);
            let (b0, b1) = (tx + lw + 10.0 * sc, tx + w.min(cw) - vw - 10.0 * sc);
            let bm = cy + line_h / 2.0;
            if b1 > b0 {
                r.fill_rrect(b0, bm - 3.5 * sc, b1, bm + 3.5 * sc, 3.5 * sc, (1.0, 1.0, 1.0, 0.10));
                let f = b0 + (b1 - b0) * (c as f32).clamp(0.02, 1.0);
                r.fill_rrect(b0, bm - 3.5 * sc, f, bm + 3.5 * sc, 3.5 * sc, (t.color.0, t.color.1, t.color.2, 1.0));
            }
            cy += line_h + gap;
        }
        // Footer: keycap + hint, under a hairline.
        if let Some((kc, hint)) = &t.footer {
            cy += 4.0 * sc;
            r.line(tx, cy, bx + bw - padx, cy, 1.0, (0.784, 0.655, 0.396, 0.18));
            cy += 4.0 * sc;
            let mut hx = tx;
            if !kc.is_empty() {
                let kw = r.measure(kc, Font::Caps, 200.0).0 + 12.0 * sc;
                r.fill_rrect(hx, cy + 1.0 * sc, hx + kw, cy + line_h - 1.0 * sc, 4.0 * sc, (0.188, 0.153, 0.114, 1.0));
                r.stroke_rrect(hx + 0.5, cy + 1.5 * sc, hx + kw - 0.5, cy + line_h - 1.5 * sc, 4.0 * sc, 1.0, (0.522, 0.486, 0.427, 1.0));
                r.text_center(kc, Font::Caps, hx + kw / 2.0, cy + line_h / 2.0, T100);
                hx += kw + 6.0 * sc;
            }
            r.text(hint, Font::Chip, hx, cy, tx + cw - hx, if kc.is_empty() { T300 } else { GOLD_HI });
            cy += line_h + gap;
        }
    }
    if let Some(mt) = more_t {
        r.text(&mt, Font::Chip, tx, cy, cw, T300);
    }
}

/// Search entries: the pack's quests (translated title, giver, already tracked?) then the index's items.
fn search_entries(ix: Option<&model::SearchIndex>, pack: Option<&model::Pack>, tracker: Option<&model::Tracker>, g: &quests::Graph, s: &i18n::Strings) -> Vec<search::Entry> {
    let mut out = vec![];
    if let Some(p) = pack {
        let me = tracker.map(progress::me_progress).unwrap_or_default();
        for (qid, q) in &p.quests {
            let st = g.state(&me, qid);
            // State line: done / tracked / available / locked (how many quests before it, or its gate if repeatable).
            let line = match st {
                quests::State::Done => s.s_done.to_string(),
                quests::State::Tracked => s.s_tracked.to_string(),
                quests::State::Available => s.s_available.split(" · ").next().unwrap_or("").to_string(),
                quests::State::Locked if g.is_repeatable(qid) => {
                    let gate = g.requires(qid).first().and_then(|b| p.quests.get(b)).map(|b| b.title.clone()).unwrap_or_default();
                    s.s_locked_rep.replace("{quest}", &gate)
                }
                quests::State::Locked => {
                    let n = g.ancestors(qid).iter().filter(|a| !me.done.contains(*a) && !g.is_repeatable(a)).count();
                    s.s_locked_n.replace("{n}", &n.to_string())
                }
            };
            out.push(search::Entry {
                key: format!("quest:{qid}"),
                name: q.title.clone(),
                rarity: None,
                n: 0,
                sources: vec![],
                quest: Some((q.giver.clone().unwrap_or_default(), line, st == quests::State::Tracked)),
            });
        }
    }
    if let Some(ix) = ix {
        out.extend(ix.items.iter().map(|i| search::Entry {
            key: i.item_key.clone(),
            name: i.name.clone(),
            rarity: i.rarity.clone(),
            n: i.n,
            sources: i.sources.clone(),
            quest: None,
        }));
    }
    out
}

/// `EnumWindows` callback: stores the first visible game window into the `HWND` pointed to by `lp`.
unsafe extern "system" fn find_game(h: HWND, lp: LPARAM) -> windows::Win32::Foundation::BOOL {
    if IsWindowVisible(h).as_bool() && is_game_window(h) {
        *(lp.0 as *mut HWND) = h;
        return false.into();
    }
    true.into()
}

/// Access to the open search window (from its window procedure).
pub fn with_journal<R>(f: impl FnOnce(&mut journal::Journal) -> R) -> Option<R> {
    with_app(|a| a.journal.as_mut().map(f)).flatten()
}

/// A command from the journal window.
pub fn on_journal_cmd(c: journal::Cmd) {
    with_app(|a| a.journal_cmd(c));
}

/// The journal window was closed.
pub fn on_journal_close() {
    with_app(|a| a.close_journal());
}

/// Runs `f` on the open search window, if any.
pub fn with_search<R>(f: impl FnOnce(&mut search::Search) -> R) -> Option<R> {
    with_app(|a| a.search.as_mut().map(f)).flatten()
}

/// Called by the search window when an entry is picked.
pub fn on_search_pick(key: String) {
    with_app(|a| a.search_pick(key));
}

/// Called by the search window when it is closed.
pub fn on_search_close() {
    with_app(|a| a.close_search());
}

/// Access to the open settings window (from its window procedure).
pub fn with_settings<R>(f: impl FnOnce(&mut settings::Settings) -> R) -> Option<R> {
    with_app(|a| a.settings.as_mut().map(f)).flatten()
}

/// Click in the settings window: handled by the window, then by the app for global actions.
pub fn on_settings_click(x: f32, y: f32, down: bool) {
    with_app(|a| {
        let act = a.settings.as_mut().and_then(|s| s.click(x, y, down));
        if let Some(id) = act {
            a.settings_command(id);
        }
    });
}

/// Called by the settings window.
pub fn on_settings_command(id: i32) {
    with_app(|a| a.settings_command(id));
}

/// Decodes one WM_INPUT (Raw Input) message: keyboard key up/down, or mouse buttons / wheel.
/// Raw Input is read-only here: the event still reaches the game unchanged (the message is passed on to
/// DefWindowProcW and nothing is blocked, consumed or injected).
fn handle_raw_input(lp: LPARAM) {
    unsafe {
        let mut size = 0u32;
        let hdr = std::mem::size_of::<RAWINPUTHEADER>() as u32;
        GetRawInputData(HRAWINPUT(lp.0 as _), RID_INPUT, None, &mut size, hdr);
        if size == 0 || size as usize > 1024 {
            return;
        }
        let mut buf = [0u64; 128];
        if GetRawInputData(HRAWINPUT(lp.0 as _), RID_INPUT, Some(buf.as_mut_ptr() as _), &mut size, hdr) == u32::MAX {
            return;
        }
        let ri = &*(buf.as_ptr() as *const RAWINPUT);
        if ri.header.dwType == RIM_TYPEKEYBOARD.0 {
            let kb = ri.data.keyboard;
            let down = kb.Flags & 1 == 0; // RI_KEY_BREAK = 1
            with_app(|a| a.on_key(kb.VKey, down));
        } else if ri.header.dwType == RIM_TYPEMOUSE.0 {
            let m = ri.data.mouse;
            let flags = m.Anonymous.Anonymous.usButtonFlags;
            let data = m.Anonymous.Anonymous.usButtonData;
            let mp = GetMessagePos();
            let pos = ((mp & 0xffff) as u16 as i16 as i32, ((mp >> 16) & 0xffff) as u16 as i16 as i32);
            with_app(|a| a.on_mouse(flags, data, pos));
        }
    }
}

/// Window procedure of the hidden main window: Raw Input, timers, network notifications, tray icon and menu.
unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // Passively observed keyboard / mouse input.
        WM_INPUT => {
            handle_raw_input(lp);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_TIMER if wp.0 == TIMER_TICK => {
            with_app(|a| a.tick());
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_TRIM => {
            let _ = KillTimer(hwnd, TIMER_TRIM);
            if !with_app(|a| a.shown).unwrap_or(false) {
                trim_memory();
            }
            LRESULT(0)
        }
        // The network thread received new data or a new status.
        WM_NET => {
            with_app(|a| {
                a.dirty = true;
                a.sync_data();
                a.refresh_journal();
            });
            LRESULT(0)
        }
        WM_TRAY => {
            let ev = (lp.0 & 0xffff) as u32;
            if ev == WM_RBUTTONUP || ev == WM_LBUTTONUP || ev == WM_CONTEXTMENU {
                let upd = with_app(|a| a.shared.lock().unwrap().update.is_some()).unwrap_or(false);
                let s = with_app(|a| a.s).unwrap_or_else(|| i18n::t_for(""));
                tray::menu(hwnd, s, upd);
            } else if ev == WM_LBUTTONDBLCLK {
                with_app(|a| a.open_settings());
            } else if ev == 0x0405 {
                // NIN_BALLOONUSERCLICK: open the page linked to the notification (e.g. the key settings page).
                // Only https:// links are opened, in the user's default browser.
                if let Some(url) = with_app(|a| a.balloon_url.take()).flatten() {
                    if url.starts_with("https://") {
                        let w = wide(&url);
                        windows::Win32::UI::Shell::ShellExecuteW(None, w!("open"), PCWSTR(w.as_ptr()), None, None, SW_SHOWNORMAL);
                    }
                }
            }
            LRESULT(0)
        }
        // Tray-menu commands.
        WM_COMMAND => {
            match wp.0 & 0xffff {
                tray::ID_SETTINGS => {
                    with_app(|a| a.open_settings());
                }
                tray::ID_SEARCH => {
                    with_app(|a| a.open_search());
                }
                tray::ID_JOURNAL => {
                    with_app(|a| a.open_journal(None));
                }
                tray::ID_REFRESH => {
                    with_app(|a| {
                        let _ = a.net.send(Req::Refresh { force: true });
                    });
                }
                tray::ID_UPDATE => {
                    with_app(|a| {
                        a.install_requested = true;
                        let _ = a.net.send(Req::CheckUpdate { install: true });
                    });
                }
                tray::ID_QUIT => {
                    let _ = DestroyWindow(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }
        // Quit: remove the tray icon and end the message loop.
        WM_DESTROY => {
            tray::remove(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => {
            // Explorer restarted ("TaskbarCreated"): add the tray icon again.
            let tc = with_app(|a| a.taskbar_created).unwrap_or(0);
            if tc != 0 && msg == tc {
                tray::add(hwnd, WM_TRAY, "Hunger Overlay");
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
    }
}

/// Window procedure of the clickable legend panel (our own window): hover, click, move by its grip.
/// Clicks on it are handled by the overlay only and never forwarded to the game.
unsafe extern "system" fn panel_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let pos = || (((lp.0 & 0xffff) as i16) as f32, (((lp.0 >> 16) & 0xffff) as i16) as f32);
    match msg {
        WM_MOUSEACTIVATE => LRESULT(3), // MA_NOACTIVATE: the game keeps the focus
        WM_MOUSEMOVE => {
            let (x, y) = pos();
            let mut tme = windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT>() as u32,
                dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::TrackMouseEvent(&mut tme);
            let (sx, sy) = cursor();
            with_app(|a| {
                if a.panel_grab.is_some() {
                    a.panel_drag_to(sx, sy);
                } else {
                    a.panel_mouse(x, y);
                }
            });
            LRESULT(0)
        }
        0x02A3 => {
            // WM_MOUSELEAVE
            with_app(|a| a.panel_mouse(-1.0, -1.0));
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let over = with_app(|a| a.panel_hover.is_some()).unwrap_or(false);
            let _ = SetCursor(LoadCursorW(None, if over { IDC_HAND } else { IDC_ARROW }).unwrap_or_default());
            LRESULT(1)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = pos();
            if with_app(|a| a.panel_down(x, y)).unwrap_or(false) {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            // Double-click on the grip: back to the default position.
            let (x, y) = pos();
            with_app(|a| {
                if a.panel_hits.iter().any(|h| h.contains(x, y) && h.action == panel::Action::Move) {
                    a.cfg.panel_pos = None;
                    a.cfg.save();
                    a.dirty = true;
                }
            });
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp.0 >> 16) & 0xffff) as u16 as i16;
            with_app(|a| a.panel_wheel(delta));
            LRESULT(0)
        }
        WM_RBUTTONDOWN | WM_RBUTTONUP => LRESULT(0),
        WM_LBUTTONUP => {
            let (x, y) = pos();
            let moved = with_app(|a| a.panel_up()).unwrap_or(false);
            if moved {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
            } else {
                with_app(|a| a.panel_click(x, y));
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Window procedure of the marker overlay: fully click-through and never activated.
unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(-1), // HTTRANSPARENT: clicks go through to the game
        WM_MOUSEACTIVATE => LRESULT(3), // MA_NOACTIVATE
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Handle of the "single instance" mutex (0 = none).
static SINGLE_INSTANCE: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Entry point: single-instance check, creation of the three windows (hidden main window, click-through
/// overlay, clickable panel), passive Raw Input registration, network and resync threads, tray icon,
/// then the Windows message loop.
fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // Texts in the language chosen in the settings (Auto = Windows').
        let s = i18n::t_for(&Config::load().locale);
        let mut tries = 1;
        loop {
            let m = CreateMutexW(None, true, w!("Local\\HungerOverlay.SingleInstance"));
            let exists = GetLastError() == ERROR_ALREADY_EXISTS;
            if let Ok(h) = m {
                if !exists {
                    SINGLE_INSTANCE.store(h.0 as isize, std::sync::atomic::Ordering::SeqCst);
                    break;
                }
                let _ = CloseHandle(h);
            }
            tries -= 1;
            if tries == 0 {
                let t = wide(s.already_running);
                MessageBoxW(None, PCWSTR(t.as_ptr()), w!("Hunger Overlay"), MB_ICONINFORMATION);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        log::init();
        logf!("starting v{}", env!("CARGO_PKG_VERSION"));
        update::cleanup();
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);

        // Window classes and windows. The main window is never shown; it only receives messages.
        let inst = GetModuleHandleW(None).unwrap();
        let main_class = w!("HungerOverlayMain");
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(main_proc), hInstance: inst.into(), lpszClassName: main_class, ..Default::default() });
        let ov_class = w!("HungerOverlayLayer");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(overlay_proc),
            hInstance: inst.into(),
            lpszClassName: ov_class,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        });
        let main = CreateWindowExW(WINDOW_EX_STYLE(0), main_class, w!("Hunger Overlay"), WS_OVERLAPPED, 0, 0, 0, 0, None, None, inst, None).unwrap();
        let panel_class = w!("HungerOverlayPanel");
        RegisterClassW(&WNDCLASSW { style: CS_DBLCLKS, lpfnWndProc: Some(panel_proc), hInstance: inst.into(), lpszClassName: panel_class, ..Default::default() });
        // No WS_EX_TRANSPARENT: this panel receives clicks (without being activated).
        let panel = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            panel_class,
            w!("Hunger Overlay"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            inst,
            None,
        )
        .unwrap();
        // Marker overlay: layered (per-pixel transparency), WS_EX_TRANSPARENT (click-through), topmost,
        // no taskbar entry, never activated.
        let overlay = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            ov_class,
            w!("Hunger Overlay"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            inst,
            None,
        )
        .unwrap();

        // Raw Input for the keyboard (usage 6) and the mouse (usage 2), with RIDEV_INPUTSINK so events are
        // also received while the game has the focus. This is passive: Windows delivers a copy of each
        // event to us; the game still receives it normally (no RIDEV_NOLEGACY, no hook, nothing blocked).
        let devices = [
            RAWINPUTDEVICE { usUsagePage: 1, usUsage: 6, dwFlags: RIDEV_INPUTSINK, hwndTarget: main },
            RAWINPUTDEVICE { usUsagePage: 1, usUsage: 2, dwFlags: RIDEV_INPUTSINK, hwndTarget: main },
        ];
        if RegisterRawInputDevices(&devices, std::mem::size_of::<RAWINPUTDEVICE>() as u32).is_err() {
            logf!("RegisterRawInputDevices failed");
        }

        // Network thread (HTTPS to the website only) and resync worker thread (image matching on captures,
        // all local); both wake the UI thread with WM_NET when they have something new.
        let shared: SharedRef = Arc::new(Mutex::new(Default::default()));
        let main_id = main.0 as isize;
        let net = net::spawn(
            shared.clone(),
            Box::new(move || {
                let _ = PostMessageW(HWND(main_id as _), WM_NET, WPARAM(0), LPARAM(0));
            }),
        );
        let mut cfg = Config::load();
        // First start of a new version (put in place by the automatic update): say so once.
        let updated = !cfg.last_version.is_empty() && cfg.last_version != env!("CARGO_PKG_VERSION");
        if cfg.last_version != env!("CARGO_PKG_VERSION") {
            cfg.last_version = env!("CARGO_PKG_VERSION").into();
            cfg.save();
        }
        // "Start with Windows": the Run registry entry is the reference. If it exists, it is rewritten to point to
        // this exe (in case the exe was moved), so it never launches an old copy.
        cfg.autostart = config::autostart_enabled();
        if cfg.autostart {
            config::set_autostart(true);
        }
        let (rs_tx, rs_out) = resync::spawn_worker(Box::new(move || {
            let _ = PostMessageW(HWND(main_id as _), WM_NET, WPARAM(0), LPARAM(0));
        }));
        let mut renderer = Renderer::new().expect("Direct2D");
        let mut prenderer = Renderer::new().expect("Direct2D");
        // Overlay text sizes (px at 1440p): bold title, owner titles / tooltip title, body, small (DESIGN.md §3.3).
        renderer.set_base_sizes([15.5, 15.5, 14.0, 12.5]);
        prenderer.set_base_sizes([15.5, 15.5, 14.0, 12.5]);
        let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
        let app = App {
            main,
            overlay,
            panel,
            prenderer,
            panel_hits: vec![],
            panel_hover: None,
            panel_shown: false,
            panel_grab: None,
            cfg,
            s,
            shared,
            net,
            renderer,
            fg_cache: (0, false),
            client: (0, 0, 0, 0),
            state: MapState::Closed { next_probe: Instant::now(), fast_until: Instant::now() },
            shown: false,
            dirty: true,
            keys_down: HashSet::new(),
            hover: None,
            settings: None,
            capture_key: false,
            taskbar_created,
            last_status: None,
            update_hint_shown: false,
            was_fg: false,
            legend_rect: None,
            candidate: None,
            wheel_votes: (0, 0),
            remember: None,
            next_global: Instant::now(),
            rs_tx,
            rs_out,
            rs_job: None,
            rs_next_id: 1,
            install_requested: false,
            balloon_url: None,
            search: None,
            items_requested: HashSet::new(),
            index_requested: false,
            scene: vec![],
            spot: None,
            spot_pending: None,
            spot_start: Instant::now(),
            panel_scroll: 0.0,
            toast: None,
            last_record: None,
            graph: Default::default(),
            graph_pack: String::new(),
            history: progress::History::load(),
            locals: progress::LocalAllies::load(),
            journal: None,
            journal_tab: progress::ME.into(),
            recent_track: None,
            avail_open: false,
            other_open: vec![],
            lang_open: false,
            next_update_check: Instant::now() + Duration::from_secs(6 * 3600),
        };
        // Apply the settings, check for an update, show the tray icon; open the settings if no API key is set yet.
        app.apply_capture_affinity();
        app.configure_net();
        let _ = app.net.send(Req::CheckUpdate { install: app.cfg.auto_update });
        let no_key = app.cfg.api_key().is_none();
        APP.with(|a| *a.borrow_mut() = Some(app));
        tray::add(main, WM_TRAY, "Hunger Overlay");
        if updated {
            let t = format!("{} {}.", s.updated_to, env!("CARGO_PKG_VERSION"));
            tray::balloon(main, "Hunger Overlay", &t);
        }
        // `--journal`: open the quest journal at startup (for a desktop shortcut).
        if std::env::args().any(|a| a == "--journal") {
            with_app(|a| a.open_journal(None));
        }
        if no_key {
            with_app(|a| a.open_settings());
        } else {
            with_app(|a| {
                let _ = a.net.send(Req::Refresh { force: true });
            });
        }
        SetTimer(main, TIMER_TICK, 15, None);
        SetTimer(main, TIMER_TRIM, 20_000, None);

        // Standard Windows message loop (the settings window is a dialog-like window: Tab/Enter handling).
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let dlg = with_app(|a| a.settings.as_ref().map(|s| s.hwnd)).flatten();
            if let Some(d) = dlg {
                if IsDialogMessageW(d, &msg).as_bool() {
                    continue;
                }
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        logf!("shutdown");
    }
}

#[cfg(test)]
mod preview {
    //! Offline preview of the overlay, drawn by the real drawing code over a map that is not on screen:
    //! `HUNGER_PREVIEW=<dir> cargo test --release preview -- --nocapture` writes, for two views of Map01 (default
    //! and zoomed ×2.5), the overlay and panel images as raw premultiplied BGRA (`overlay_<view>.bgra`,
    //! `panel_<view>.bgra`) plus `<view>.txt` (sizes and panel position), to be composited over screenshots.
    //! Data: the app's cache (%APPDATA%\HungerOverlay). Skipped when HUNGER_PREVIEW is not set.
    use super::*;

    #[test]
    fn preview() {
        let Some(dir) = std::env::var_os("HUNGER_PREVIEW").map(std::path::PathBuf::from) else { return };
        let d = config::data_dir();
        let pack: model::Pack = serde_json::from_str(&std::fs::read_to_string(d.join("pack.json")).unwrap()).unwrap();
        let tracker: model::Tracker = serde_json::from_str(&std::fs::read_to_string(d.join("tracker.json")).unwrap()).unwrap();
        let items: Option<model::ItemsPack> = std::fs::read_to_string(d.join("items.json")).ok().and_then(|s| serde_json::from_str(&s).ok());
        // (name, zoom level n, pan offset, layers on, quests-layer token to hover)
        let views: [(&str, i32, (f64, f64), &[&str]); 2] = [
            ("default", 0, (0.0, 0.0), &["extractions", "player_spawns", "quests"]),
            ("zoomed", 3, (201.0, 199.5), &["extractions", "player_spawns", "quests", "places", "minibosses", "traps", "sound_traps", "placed_loot", "keys", "locks", "harvest"]),
        ];
        for (name, n, off, layers) in views {
            let shared: SharedRef = Arc::new(Mutex::new(Default::default()));
            {
                let mut sh = shared.lock().unwrap();
                sh.pack = Some(pack.clone());
                sh.tracker = Some(tracker.clone());
                sh.items = items.clone();
                sh.status = Some(Status::Ok);
                sh.version = 1;
            }
            let (net, _rx) = std::sync::mpsc::channel();
            let (rs_tx, _rs_rx) = std::sync::mpsc::channel();
            let mut renderer = Renderer::new().unwrap();
            let mut prenderer = Renderer::new().unwrap();
            renderer.set_base_sizes([15.5, 15.5, 14.0, 12.5]);
            prenderer.set_base_sizes([15.5, 15.5, 14.0, 12.5]);
            let mut cfg = Config::default();
            cfg.layers_on = layers.iter().map(|s| s.to_string()).collect();
            cfg.layers_known = cfg.layers_on.clone();
            cfg.quests_layer_seen = false;
            // Interface language of the preview: HUNGER_PREVIEW_LANG (default: Auto).
            cfg.locale = std::env::var("HUNGER_PREVIEW_LANG").unwrap_or_default();
            let now = Instant::now();
            // Map01 default square measured on the designer's screenshot (2560×1440).
            let sq = Square { x0: 597.11, y0: 37.27, side: 4096.0 * 0.33342 };
            let mut m = App::new_open(0, sq, now);
            m.n = n;
            m.zoom = 1.0 + 0.5 * n as f64;
            m.off = off;
            let mut app = App {
                main: HWND::default(),
                overlay: HWND::default(),
                panel: HWND::default(),
                prenderer,
                panel_hits: vec![],
                panel_hover: None,
                panel_shown: false,
                panel_grab: None,
                cfg,
                s: i18n::strings_for("en"),
                shared,
                net,
                renderer,
                fg_cache: (0, false),
                client: (0, 0, 2560, 1440),
                state: MapState::Open(Box::new(m)),
                shown: true,
                dirty: true,
                keys_down: HashSet::new(),
                hover: None,
                settings: None,
                capture_key: false,
                taskbar_created: 0,
                last_status: Some(Status::Ok),
                update_hint_shown: true,
                was_fg: true,
                legend_rect: None,
                candidate: None,
                wheel_votes: (0, 0),
                remember: None,
                next_global: now,
                rs_tx,
                rs_out: Default::default(),
                rs_job: None,
                rs_next_id: 1,
                install_requested: false,
                next_update_check: now,
                balloon_url: None,
                search: None,
                items_requested: HashSet::new(),
                index_requested: true,
                scene: vec![],
                spot: None,
                spot_pending: None,
                spot_start: now,
                panel_scroll: 0.0,
                toast: None,
                last_record: None,
                graph: Default::default(),
                graph_pack: String::new(),
                history: Default::default(),
                // Read-only: the preview shows the local allies of this PC (tests never write files).
                locals: progress::LocalAllies::load(),
                journal: None,
                journal_tab: progress::ME.into(),
                recent_track: None,
                avail_open: false,
                other_open: vec![],
                lang_open: false,
            };
            app.sync_data();
            app.draw();
            // Hover a Quests-layer token (default view) or a tracked container (zoomed view), and show a toast.
            let target = app.scene.iter().find(|it| if name == "default" { it.kind == markers::Kind::QuestAvail && it.members.len() == 1 } else { markers::is_owner(it.kind) && it.members.len() == 1 });
            app.hover = target.map(|it| it.members[0]);

            app.draw();
            let (l, t, r, b) = app.legend_rect.unwrap();
            std::fs::write(dir.join(format!("overlay_{name}.bgra")), app.renderer.bgra()).unwrap();
            std::fs::write(dir.join(format!("panel_{name}.bgra")), app.prenderer.bgra()).unwrap();
            std::fs::write(
                dir.join(format!("{name}.txt")),
                format!("{} {} {} {} {} {} {}", app.renderer.w, app.renderer.h, l, t, app.prenderer.w, app.prenderer.h, (r - l) as i32 * 0 + (b - t) as i32),
            )
            .unwrap();
            println!("{name}: {} markers drawn, panel {}×{} at ({l}, {t})", app.scene.len(), app.prenderer.w, app.prenderer.h);
        }
    }
}
