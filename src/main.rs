//! Hunger Overlay : affiche les objectifs du quest tracker Hunger Companion par-dessus la carte du jeu.
//!
//! Principe : aucune lecture ni modification du jeu. L'appli surveille la touche carte, confirme l'ouverture
//! par quelques pixels de l'écran, identifie la carte, puis dessine les marqueurs dans une fenêtre transparente
//! non cliquable posée au-dessus du jeu. Le zoom et le déplacement suivent la molette et la souris
//! (modèle mesuré : docs/overlay/MAP_UI_MEASUREMENTS.md).

#![windows_subsystem = "windows"]

mod capture;
mod config;
mod detect;
mod i18n;
mod log;
mod model;
mod net;
mod panel;
mod render;
mod resync;
mod settings;
mod tray;
mod update;

use config::Config;
use detect::{PanelSig, Square};
use model::{DrawPoint, LegendQuest};
use net::{Req, SharedRef, Status};
use render::{Font, Renderer, Rgba};
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

const WM_TRAY: u32 = WM_APP + 1;
const WM_NET: u32 = WM_APP + 2;
const TIMER_TICK: usize = 1;
const GAME_EXE: &str = "projectrlh-win64-shipping.exe";

/// Loi de zoom mesurée : 11 niveaux, facteur 1 + 0,5·n, animation exponentielle (k ≈ 5/s).
const ZOOM_MAX_N: i32 = 10;
const ZOOM_K: f64 = 5.0;
/// Sonde visuelle (carte fermée) : 4 fois par seconde, 8 fois juste après la touche carte.
const PROBE_EVERY: Duration = Duration::from_millis(250);
const PROBE_FAST: Duration = Duration::from_millis(120);
const PANEL_EVERY: Duration = Duration::from_millis(150);
/// La signature du panneau d'aide est prise après le fondu d'ouverture (≈ 190 ms).
const PANEL_SIG_DELAY: Duration = Duration::from_millis(260);
/// Carte « suspendue » (panneau caché par un menu, Échap…) : on la retrouve telle quelle pendant ce délai.
const SUSPEND_MAX: Duration = Duration::from_secs(180);

struct OpenMap {
    map_idx: usize,
    map_id: &'static str,
    sq: Square,
    n: i32,
    zoom: f64,
    off: (f64, f64),
    panel: Option<PanelSig>,
    /// Instant où prendre la signature du panneau (après le fondu) ; on attend 2 captures identiques.
    panel_due: Option<Instant>,
    panel_prev: Option<capture::Gray>,
    panel_tries: u32,
    panel_miss: u32,
    next_panel: Instant,
    /// Vérification de fermeture en cours (touche carte / Échap) : overlay masqué jusqu'au verdict.
    closing_check: Option<Instant>,
    last: Instant,
    drag: Option<(i32, i32)>,
    wheel_acc: i32,
    /// Niveau de zoom selon les deux façons de compter la molette (par 120 / par événement).
    n_a: i32,
    n_b: i32,
    /// Un mouvement (molette, glisser) a eu lieu depuis le dernier recalage.
    motion: bool,
    /// Recalage large en cours (retour de focus / des menus) : overlay masqué jusqu'au résultat.
    wide_pending: bool,
    /// Dernière action souris (molette / glisser) : le recalage périodique attend que la carte soit immobile.
    last_input: Instant,
    /// Recalage visuel programmé ; `true` = recherche large (retour de focus), overlay masqué en attendant.
    resync_at: Option<(Instant, bool)>,
    next_idle_resync: Instant,
    points: Vec<DrawPoint>,
    legend: Vec<LegendQuest>,
    layers: Vec<model::LegendLayer>,
    data_version: u64,
}

impl OpenMap {
    fn target_zoom(&self) -> f64 {
        1.0 + 0.5 * self.n as f64
    }
    fn center(&self) -> (f64, f64) {
        (self.sq.x0 + self.sq.side / 2.0, self.sq.y0 + self.sq.side / 2.0)
    }
    /// Texture normalisée (u, v) → coordonnées client.
    fn project(&self, u: f64, v: f64) -> (f64, f64) {
        let (cx, cy) = self.center();
        let s = self.zoom * self.sq.side;
        (cx + self.off.0 + s * (u - 0.5), cy + self.off.1 + s * (v - 0.5))
    }
}

/// La détection ne dépend d'AUCUNE touche : la carte est reconnue à l'écran (carré de parchemin en vue par
/// défaut + panneau d'aide), sa fermeture par la disparition du panneau. La touche carte n'est qu'un accélérateur.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Resync {
    /// Retour de focus / des menus : large, et recherche complète en dernier recours.
    Wide,
    /// Juste après un mouvement : niveaux voisins (±2) et rayon moyen.
    Motion,
    /// Contrôle périodique au repos.
    Idle,
}

enum MapState {
    Closed { next_probe: Instant, fast_until: Instant },
    Open(Box<OpenMap>),
    /// Panneau disparu (menu Échap, fermeture…) : on garde l'état pour reprendre si la carte réapparaît.
    Suspended { m: Box<OpenMap>, since: Instant, next_probe: Instant },
}

struct App {
    main: HWND,
    overlay: HWND,
    /// Panneau cliquable (légende + réglages rapides).
    panel: HWND,
    prenderer: Renderer,
    panel_hits: Vec<panel::Hit>,
    panel_hover: Option<usize>,
    panel_shown: bool,
    cfg: Config,
    s: &'static i18n::Strings,
    shared: SharedRef,
    net: Sender<Req>,
    renderer: Renderer,
    fg_cache: (isize, bool),
    client: (i32, i32, i32, i32),
    state: MapState,
    shown: bool,
    dirty: bool,
    keys_down: HashSet<u16>,
    hover: Option<usize>,
    settings: Option<settings::Settings>,
    capture_key: bool,
    taskbar_created: u32,
    last_status: Option<Status>,
    update_hint_shown: bool,
    was_fg: bool,
    legend_rect: Option<(f32, f32, f32, f32)>,
    /// Détection en attente de confirmation (animations d'ouverture : on exige 2 détections identiques).
    candidate: Option<(Square, usize)>,
    /// Votes (par 120, par événement) des recalages sur la façon dont le jeu compte la molette.
    wheel_votes: (u32, u32),
    /// Dernière carte vue (index, carré par défaut, signature du panneau) : permet de la retrouver « de zéro ».
    remember: Option<(usize, Square, PanelSig)>,
    next_global: Instant,
    /// Fil de calcul des recalages.
    rs_tx: Sender<resync::Job>,
    rs_out: resync::ResSlot,
    rs_job: Option<JobInfo>,
    rs_next_id: u64,
    /// Installation demandée depuis le menu (en mode « me prévenir »).
    install_requested: bool,
    next_update_check: Instant,
}

/// Mémorise sur disque la dernière carte vue (pour cette taille d'écran) : carte, carré par défaut, panneau d'aide.
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

fn load_remember(client: (i32, i32, i32, i32)) -> Option<(usize, Square, PanelSig)> {
    let b = std::fs::read(config::data_dir().join("map_memory.bin")).ok()?;
    if b.get(0..4)? != b"HOR1" {
        return None;
    }
    let i32_at = |o: usize| Some(i32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?));
    let f64_at = |o: usize| Some(f64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?));
    if (i32_at(4)?, i32_at(8)?) != (client.2, client.3) {
        return None; // autre résolution : la mémoire ne s'applique pas
    }
    let idx = i32_at(12)? as usize;
    let sq = Square { x0: f64_at(16)?, y0: f64_at(24)?, side: f64_at(32)? };
    let sig = PanelSig::from_bytes(b.get(40..)?)?;
    (idx < detect::REFS.len()).then_some((idx, sq, sig))
}

/// Ce qu'on savait au moment de la capture envoyée au fil de calcul.
struct JobInfo {
    id: u64,
    mode: Resync,
    time: Instant,
    map_idx: usize,
    n: i32,
    n_a: i32,
    n_b: i32,
    pred: (f64, f64),
    /// Recherche complète d'une carte « perdue » (état fermé) : (carte, carré par défaut, panneau).
    lost: Option<(usize, Square, PanelSig)>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

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

fn client_rect(hwnd: HWND) -> Option<(i32, i32, i32, i32)> {
    unsafe {
        let mut r = RECT::default();
        GetClientRect(hwnd, &mut r).ok()?;
        let mut p = POINT { x: 0, y: 0 };
        let _ = windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut p);
        Some((p.x, p.y, r.right - r.left, r.bottom - r.top))
    }
}

fn cursor() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

impl App {
    /// Fenêtre du jeu si elle est au premier plan.
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

    fn sc(&self) -> f32 {
        (self.client.3 as f32 / 1440.0).max(0.5)
    }

    fn configure_net(&self) {
        let _ = self.net.send(Req::Configure {
            key: self.cfg.api_key(),
            base_url: self.cfg.base_url.clone(),
            locale: self.cfg.locale.clone(),
        });
    }

    fn apply_capture_affinity(&self) {
        let a = if self.cfg.capture_visible { WDA_NONE } else { WDA_EXCLUDEFROMCAPTURE };
        unsafe {
            let _ = SetWindowDisplayAffinity(self.overlay, a);
            let _ = SetWindowDisplayAffinity(self.panel, a);
        }
    }

    /// Curseur au-dessus du panneau ? (coordonnées écran)
    fn over_panel(&self, sx: i32, sy: i32) -> bool {
        match self.legend_rect {
            Some((l, t, r, b)) => {
                let (x, y) = ((sx - self.client.0) as f32, (sy - self.client.1) as f32);
                self.panel_shown && x >= l && x <= r && y >= t && y <= b
            }
            None => false,
        }
    }

    fn lang_label(&self) -> String {
        if self.cfg.locale.is_empty() {
            let account = self.shared.lock().unwrap().tracker.as_ref().and_then(|t| t.locale.clone());
            format!("Auto ({})", i18n::effective("", account.as_deref()).to_uppercase())
        } else {
            self.cfg.locale.to_uppercase()
        }
    }

    fn panel_mouse(&mut self, x: f32, y: f32) {
        let h = self.panel_hits.iter().position(|h| h.contains(x, y));
        if h != self.panel_hover {
            self.panel_hover = h;
            self.dirty = true;
        }
    }

    /// Clic dans le panneau : réglages rapides appliqués et enregistrés immédiatement.
    fn panel_click(&mut self, x: f32, y: f32) {
        let Some(hit) = self.panel_hits.iter().find(|h| h.contains(x, y)).cloned() else { return };
        match hit.action {
            panel::Action::Settings => {
                self.open_settings();
                return;
            }
            panel::Action::Collapse => self.cfg.panel_collapsed = !self.cfg.panel_collapsed,
            panel::Action::ToggleDone => self.cfg.show_done = !self.cfg.show_done,
            panel::Action::ChanceDown => self.cfg.min_chance = self.cfg.min_chance.saturating_sub(5) / 5 * 5,
            panel::Action::ChanceUp => self.cfg.min_chance = ((self.cfg.min_chance / 5 + 1) * 5).min(50),
            panel::Action::Language => {
                let order: Vec<&str> = std::iter::once("").chain(i18n::LANGS.iter().map(|l| l.0)).collect();
                let i = order.iter().position(|c| *c == self.cfg.locale).unwrap_or(0);
                self.cfg.locale = order[(i + 1) % order.len()].to_string();
                self.configure_net();
                let _ = self.net.send(Req::Refresh { force: true });
            }
            panel::Action::ToggleLayer(id) => {
                if let Some(p) = self.cfg.layers_on.iter().position(|q| *q == id) {
                    self.cfg.layers_on.remove(p);
                } else {
                    self.cfg.layers_on.push(id);
                }
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

    // ---------- Entrées ----------

    fn on_key(&mut self, vk: u16, down: bool) {
        if !down {
            self.keys_down.remove(&vk);
            return;
        }
        if !self.keys_down.insert(vk) {
            return; // répétition automatique
        }
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
                // Accélérateur : on sonde l'écran plus souvent pendant un instant.
                MapState::Closed { next_probe, fast_until } => {
                    *next_probe = now + Duration::from_millis(200);
                    *fast_until = now + Duration::from_millis(1200);
                }
                MapState::Suspended { next_probe, .. } => *next_probe = now + Duration::from_millis(200),
                // Fermeture probable : on masque tout de suite et on vérifie à l'écran.
                MapState::Open(m) => {
                    m.closing_check = Some(now + Duration::from_millis(250));
                    m.drag = None;
                }
            }
            self.dirty = true;
        }
    }

    fn on_mouse(&mut self, flags: u16, data: u16) {
        const LEFT_DOWN: u16 = 0x0001;
        const LEFT_UP: u16 = 0x0002;
        const WHEEL: u16 = 0x0400;
        // Le relâchement termine TOUJOURS un glisser, même si le jeu a perdu le focus entre-temps
        // (sinon chaque mouvement de souris hors du jeu décalait les marqueurs).
        if flags & LEFT_UP != 0 {
            if let MapState::Open(m) = &mut self.state {
                if m.drag.take().is_some() {
                    m.last_input = Instant::now();
                }
            }
        }
        if self.game_fg().is_none() {
            return;
        }
        // Clic ou molette sur NOTRE panneau : le jeu ne les reçoit pas, la carte ne bouge donc pas.
        let (sx, sy) = cursor();
        if self.over_panel(sx, sy) {
            return;
        }
        let (gx, gy, gw, gh) = self.client;
        let per_event = self.cfg.wheel_per_event == Some(true);
        let MapState::Open(m) = &mut self.state else { return };
        if m.closing_check.is_some() {
            return;
        }
        if flags & WHEEL != 0 {
            m.last_input = Instant::now();
            m.motion = true;
            let d = data as i16 as i32;
            // Modèle A : un cran par tranche de 120 (molette classique).
            m.wheel_acc += d;
            while m.wheel_acc >= 120 {
                m.wheel_acc -= 120;
                m.n_a = (m.n_a + 1).min(ZOOM_MAX_N);
            }
            while m.wheel_acc <= -120 {
                m.wheel_acc += 120;
                m.n_a = (m.n_a - 1).max(0);
            }
            // Modèle B : un cran par événement (molettes à défilement fin).
            m.n_b = (m.n_b + d.signum()).clamp(0, ZOOM_MAX_N);
            m.n = if per_event { m.n_b } else { m.n_a };
        }
        if flags & LEFT_DOWN != 0 {
            let (cx, cy) = cursor();
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

    // ---------- Boucle (≈ 64 Hz) ----------

    fn tick(&mut self) {
        let now = Instant::now();
        let game = self.game_fg();
        let regained = game.is_some() && !self.was_fg;
        self.was_fg = game.is_some();
        if let MapState::Open(m) = &mut self.state {
            if game.is_none() {
                m.drag = None;
            } else if regained {
                // Retour dans le jeu : la carte a pu bouger sans nous (clic de reprise de focus…) → recalage large.
                m.resync_at = Some((now + Duration::from_millis(350), true));
            }
        }
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
                    logf!("carte considérée fermée");
                    MapState::Closed { next_probe: now, fast_until: now }
                } else if game.is_some() && now >= next_probe {
                    let fresh = self.probe_confirmed();
                    if let Ok(fresh) = fresh {
                        // Nouvelle ouverture (vue par défaut).
                        self.opened(fresh)
                    } else if matches!(fresh, Err(true)) {
                        MapState::Suspended { m, since, next_probe: now + Duration::from_millis(100) }
                    } else if self.panel_present(&m) {
                        logf!("carte retrouvée (reprise de l'état, recalage)");
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
                    logf!("panneau de carte disparu : overlay masqué");
                    self.dirty = true;
                    MapState::Suspended { m, since: now, next_probe: now + PROBE_EVERY }
                }
            }
        };
        let visible = game.is_some()
            && matches!(&self.state, MapState::Open(m) if m.closing_check.is_none() && !m.wide_pending && !m.resync_at.map(|r| r.1).unwrap_or(false));
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
            }
            if !visible {
                self.show_panel(false);
            }
            self.dirty = true;
        }
        if visible && self.dirty {
            self.dirty = false;
            self.draw();
        }
    }

    /// Sonde légère (2 lignes de pixels) puis, si elle passe, détection complète et identification.
    fn probe_and_detect(&self) -> Option<OpenMap> {
        let (x, y, w, h) = self.client;
        if !detect::probe(x, y, w, h) {
            return None;
        }
        self.detect_open()
    }

    /// Ok = carte confirmée ; Err(true) = 1re détection, à confirmer au prochain passage ; Err(false) = rien.
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

    fn opened(&mut self, m: OpenMap) -> MapState {
        logf!("carte ouverte : {} (carré {:.0},{:.0} côté {:.1})", m.map_id, m.sq.x0, m.sq.y0, m.sq.side);
        let _ = self.net.send(Req::Refresh { force: false });
        self.dirty = true;
        MapState::Open(Box::new(m))
    }

    fn panel_present(&self, m: &OpenMap) -> bool {
        match &m.panel {
            Some(sig) => {
                let (x, y, w, h) = sig.rect();
                capture::grab_gray(self.client.0 + x, self.client.1 + y, w, h).map(|g| sig.score(&g) > 0.85).unwrap_or(false)
            }
            None => false,
        }
    }

    /// Une image de la carte ouverte. Renvoie false si le panneau d'aide a disparu (carte cachée ou fermée).
    fn tick_open(&mut self, m: &mut OpenMap, fg: bool, now: Instant) -> bool {
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
        let lmb_down = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(0x01) } < 0;
        if m.drag.is_some() && (!lmb_down || !fg) {
            m.drag = None;
            m.last_input = now;
        }
        if let Some((lx, ly)) = m.drag {
            m.last_input = now;
            let (cx, cy) = cursor();
            if (cx, cy) != (lx, ly) {
                let lim = m.zoom * m.sq.side / 2.0;
                m.off.0 = (m.off.0 + (cx - lx) as f64).clamp(-lim, lim);
                m.off.1 = (m.off.1 + (cy - ly) as f64).clamp(-lim, lim);
                m.drag = Some((cx, cy));
                self.dirty = true;
            }
        }
        if !fg {
            return true;
        }
        // Signature du panneau d'aide, une fois le fondu terminé ; on affine aussi le carré si la vue n'a pas bougé.
        if let Some(t) = m.panel_due {
            if now < t {
                return true;
            }
            let (x, y, w, h) = self.client;
            let (px0, py0, px1, py1) = detect::panel_rect(h as f64);
            let Some(cur) = capture::grab_gray(x + px0 as i32, y + py0 as i32, (px1 - px0) as i32, (py1 - py0) as i32) else {
                return true;
            };
            // Écran stable (fondu terminé) quand deux captures successives sont identiques.
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
                logf!("panneau d'aide non reconnu");
                return false;
            };
            self.remember = Some((m.map_idx, m.sq, sig.clone()));
            save_remember(self.client, m.map_idx, &m.sq, sig);
            m.next_panel = now + PANEL_EVERY;
        }
        // Recalage visuel : programmé (retour de focus, reprise) ou périodique quand la carte est immobile.
        // - juste après un mouvement (dès que la carte est immobile et l'animation de zoom presque finie) ;
        // - périodiquement au repos ; - sur demande (retour de focus / des menus : recherche large).
        let quiet = m.drag.is_none() && now.duration_since(m.last_input) > Duration::from_millis(220) && (m.zoom - tz).abs() < 0.006 * tz;
        // 1) Résultat d'un recalage lancé plus tôt ?
        if let Some((info, res)) = self.take_resync() {
            if info.lost.is_none() {
                match self.apply_resync(m, &info, res, now) {
                    Some(false) => return false, // carte non retrouvée après un retour
                    Some(true) => self.dirty = true,
                    None => {}
                }
            }
        }
        if m.wide_pending && self.rs_job.is_none() {
            m.wide_pending = false; // demande abandonnée (délai dépassé) : on réaffiche
        }
        // 2) Lancer un recalage si nécessaire (un seul à la fois).
        let due = match m.resync_at {
            Some((t, _)) => now >= t && m.drag.is_none(),
            None => quiet && (m.motion || now >= m.next_idle_resync),
        };
        if due && m.closing_check.is_none() && self.rs_job.is_none() {
            let mode = if m.resync_at.map(|r| r.1).unwrap_or(false) {
                Resync::Wide
            } else if m.motion {
                Resync::Motion
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

    fn detect_open(&self) -> Option<OpenMap> {
        let (x, y, w, h) = self.client;
        let g = capture::grab_gray(x, y, w, h)?;
        let sq = detect::find_square(&g)?;
        let (idx, score, margin) = detect::identify(&g, &sq)?;
        logf!("identification : {} score {:.3} marge {:.3}", detect::REFS[idx].0, score, margin);
        Some(Self::new_open(idx, sq, Instant::now()))
    }

    fn new_open(idx: usize, sq: Square, now: Instant) -> OpenMap {
        OpenMap {
            map_idx: idx,
            map_id: detect::REFS[idx].0,
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
            last_input: now,
            // Premier recalage dès l'ouverture (corrige une détection du carré à quelques pixels près).
            resync_at: Some((now + Duration::from_millis(320), false)),
            next_idle_resync: now + Duration::from_millis(1200),
            points: vec![],
            legend: vec![],
            layers: vec![],
            data_version: u64::MAX,
        }
    }

    /// Carte fermée pour nous, mais panneau d'aide visible : la carte est ouverte et déjà zoomée/déplacée
    /// (appli relancée, état perdu…). On la retrouve par une recherche complète (au plus toutes les 1,5 s).
    fn relocate_lost(&mut self, now: Instant) -> Option<OpenMap> {
        // Résultat d'une recherche complète lancée plus tôt ?
        if let Some((info, res)) = self.take_resync() {
            let (Some((_, sq, sig)), Some(f)) = (info.lost, res.found) else { return None };
            let idx = res.map;
            logf!("recherche complète : {} score {:.3}, n {}, {} ms", detect::REFS[idx].0, f.score, f.n, res.ms);
            if f.score < 0.6 {
                return None;
            }
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
        // Panneau d'aide visible mais carte non reconnue en vue par défaut : elle est ouverte, zoomée ou déplacée.
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

    /// Apprend comment le jeu compte la molette, d'après le niveau réellement trouvé par un recalage.
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
            logf!("molette : le jeu compte {}", if want == Some(true) { "un cran par événement" } else { "un cran par tranche de 120" });
        }
    }

    /// Reconstruit les points de la carte ouverte quand les données changent.
    fn sync_data(&mut self) {
        // Langue de l'interface : forcée, sinon celle du compte du site, sinon Windows (anglais par défaut).
        let account = self.shared.lock().unwrap().tracker.as_ref().and_then(|t| t.locale.clone());
        let s = i18n::strings_for(&i18n::effective(&self.cfg.locale, account.as_deref()));
        if !std::ptr::eq(s, self.s) {
            self.s = s;
            self.dirty = true;
        }
        let sh = self.shared.lock().unwrap();
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
        let MapState::Open(m) = &mut self.state else { return };
        if m.data_version == sh.version {
            return;
        }
        m.data_version = sh.version;
        if let (Some(p), Some(t)) = (&sh.pack, &sh.tracker) {
            let opts = model::ViewOpts {
                map_id: m.map_id,
                show_done: self.cfg.show_done,
                hidden: &self.cfg.hidden_quests,
                min_chance: self.cfg.min_chance as f64 / 100.0,
                layers_on: &self.cfg.layers_on,
                items: sh.items.as_ref(),
            };
            let v = model::build_view(p, t, &opts);
            m.points = v.points;
            m.legend = v.legend;
            m.layers = v.layers;
        } else {
            m.points.clear();
            m.legend.clear();
            m.layers.clear();
        }
        self.dirty = true;
    }

    /// Recale la position/zoom de la carte sur la capture d'écran. Renvoie Some(true) si une correction a été appliquée.
    /// Zones à ignorer pour le recalage : panneau d'aide du jeu et notre panneau.
    fn masks(&self) -> Vec<(f64, f64, f64, f64)> {
        let mut v = vec![detect::panel_rect(self.client.3 as f64)];
        if let Some((l, t, r, b)) = self.legend_rect {
            v.push((l as f64, t as f64, r as f64, b as f64));
        }
        v
    }

    /// Capture l'écran et envoie une recherche au fil de calcul.
    fn start_resync(&mut self, m: &OpenMap, mode: Resync, now: Instant) -> bool {
        let (x, y, w, h) = self.client;
        let Some(g) = capture::grab_gray(x, y, w, h) else { return false };
        let side = (1.0 + 0.5 * m.n as f64) * m.sq.side;
        let (cx, cy) = m.center();
        let pred = (cx + m.off.0 - side / 2.0, cy + m.off.1 - side / 2.0);
        let (span, radius, kind) = match mode {
            Resync::Wide => (2, 640.0, resync::Kind::LocalThenGlobal),
            Resync::Motion => (2, 400.0, resync::Kind::Local),
            Resync::Idle => (1, 160.0, resync::Kind::Local),
        };
        let mut zooms: Vec<i32> = (m.n - span..=m.n + span).chain([m.n_a, m.n_b]).filter(|n| (0..=ZOOM_MAX_N).contains(n)).collect();
        zooms.sort();
        zooms.dedup();
        let id = self.rs_next_id;
        self.rs_next_id += 1;
        let job = resync::Job { id, g, masks: self.masks(), map: m.map_idx, side0: m.sq.side, center: (cx, cy), pred, n_pred: m.n, zooms, radius, kind };
        if self.rs_tx.send(job).is_err() {
            return false;
        }
        self.rs_job = Some(JobInfo { id, mode, time: now, map_idx: m.map_idx, n: m.n, n_a: m.n_a, n_b: m.n_b, pred, lost: None });
        true
    }

    /// Résultat du fil de calcul correspondant à la demande en cours (ou abandon au bout de 5 s).
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

    /// Applique un résultat de recalage. Some(true) = corrigé, Some(false) = carte perdue, None = rien à faire.
    fn apply_resync(&mut self, m: &mut OpenMap, info: &JobInfo, res: resync::Res, now: Instant) -> Option<bool> {
        let wide = info.mode == Resync::Wide;
        if wide {
            m.wide_pending = false;
        }
        if info.map_idx != m.map_idx {
            return None;
        }
        // La carte a bougé depuis la capture : résultat périmé (un nouveau recalage suivra le mouvement).
        if m.last_input > info.time {
            if wide {
                m.resync_at = Some((now, true));
            }
            return None;
        }
        let Some(f) = res.found else {
            return if wide { Some(false) } else { None };
        };
        let side_p = (1.0 + 0.5 * info.n as f64) * m.sq.side;
        let side_f = (1.0 + 0.5 * f.n as f64) * m.sq.side;
        let shift = ((f.x0 + side_f / 2.0 - (info.pred.0 + side_p / 2.0)).powi(2) + (f.y0 + side_f / 2.0 - (info.pred.1 + side_p / 2.0)).powi(2)).sqrt();
        let better = f.score > f.score_pred + 0.02 || f.score_pred < 0.4;
        let apply = f.score >= 0.6 && (f.n != info.n || shift > 2.0) && better;
        logf!(
            "recalage {:?}{} : score {:.3} (prévu {:.3}) n {}→{} écart {:.1} px, {} ms{}",
            info.mode,
            if res.global_used { " + recherche complète" } else { "" },
            f.score,
            f.score_pred,
            info.n,
            f.n,
            shift,
            res.ms,
            if apply { " → CORRIGÉ" } else { "" }
        );
        if f.score >= 0.6 {
            self.learn_wheel(f.n, info.n_a, info.n_b);
        }
        if wide && f.score.max(f.score_pred) < 0.5 {
            logf!("carte non retrouvée à l'écran après reprise");
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

    fn marker_r(&self) -> f64 {
        (9.0 * self.sc() * self.cfg.marker_scale).max(5.0) as f64
    }

    fn compute_hover(&self) -> Option<usize> {
        let MapState::Open(m) = &self.state else { return None };
        if m.drag.is_some() {
            return None;
        }
        let (cx, cy) = cursor();
        if self.over_panel(cx, cy) {
            return None;
        }
        let (lx, ly) = ((cx - self.client.0) as f64, (cy - self.client.1) as f64);
        let lim = self.marker_r() + 6.0;
        let mut best = None;
        let mut bd = lim * lim;
        for (i, p) in m.points.iter().enumerate() {
            let (x, y) = m.project(p.u, p.v);
            let d = (x - lx).powi(2) + (y - ly).powi(2);
            if d < bd {
                bd = d;
                best = Some(i);
            }
        }
        best
    }

    // ---------- Dessin ----------

    fn show_panel(&mut self, on: bool) {
        if on != self.panel_shown {
            self.panel_shown = on;
            if !on {
                self.panel_hover = None;
            }
            unsafe {
                let _ = ShowWindow(self.panel, if on { SW_SHOWNOACTIVATE } else { SW_HIDE });
            }
        }
    }

    fn draw(&mut self) {
        let lang_label = self.lang_label();
        let (gx, gy, gw, gh) = self.client;
        let sc = self.sc();
        let _ = self.renderer.set_font_scale(sc);
        if !self.renderer.begin(gw, gh) {
            return;
        }
        let MapState::Open(m) = &self.state else {
            self.renderer.present(self.overlay, gx, gy);
            return;
        };
        let r = &self.renderer;
        let rad = self.marker_r() as f32;
        let (mx0, my0) = m.project(0.0, 0.0);
        let (mx1, my1) = m.project(1.0, 1.0);
        for (i, p) in m.points.iter().enumerate() {
            let (x, y) = m.project(p.u, p.v);
            let (x, y) = (x as f32, y as f32);
            if x < -20.0 || y < -20.0 || x > gw as f32 + 20.0 || y > gh as f32 + 20.0 {
                continue;
            }
            if (x as f64) < mx0 || (x as f64) > mx1 || (y as f64) < my0 || (y as f64) > my1 {
                continue;
            }
            let hov = self.hover == Some(i);
            let rr = if hov { rad * 1.35 } else { rad };
            let c: Rgba = (p.color.0, p.color.1, p.color.2, if p.locked { 0.45 } else { 1.0 });
            if p.layer.is_some() {
                // Point de calque : plus petit et plus discret que les objectifs.
                let lr = rr * 0.62;
                if p.diamond {
                    r.diamond(x, y, lr * 1.35, 3.5 * sc, (0.0, 0.0, 0.0, 0.7));
                    r.diamond(x, y, lr * 1.35, 1.8 * sc, c);
                } else {
                    r.fill_circle(x, y, lr + 1.5 * sc, (0.0, 0.0, 0.0, 0.6));
                    r.fill_circle(x, y, lr, c);
                }
            } else if p.harvest.is_some() {
                // Nœud de récolte : carré arrondi (grisé si le niveau requis dépasse celui du joueur).
                let hr = rr * 0.95;
                r.fill_rrect(x - hr - 2.0 * sc, y - hr - 2.0 * sc, x + hr + 2.0 * sc, y + hr + 2.0 * sc, 3.0 * sc, (0.0, 0.0, 0.0, 0.55));
                r.fill_rrect(x - hr, y - hr, x + hr, y + hr, 2.5 * sc, c);
                r.stroke_rrect(x - hr, y - hr, x + hr, y + hr, 2.5 * sc, 1.4 * sc, (1.0, 1.0, 1.0, if p.locked { 0.5 } else { 0.95 }));
            } else if p.place {
                r.diamond(x, y, rr * 1.3, 4.0 * sc, (0.0, 0.0, 0.0, 0.75));
                r.diamond(x, y, rr * 1.3, 2.0 * sc, c);
            } else if p.done {
                r.fill_circle(x, y, rr * 0.8, (0.35, 0.35, 0.35, 0.65));
                r.stroke_circle(x, y, rr * 0.8, 1.5 * sc, (0.9, 0.9, 0.9, 0.7), false);
            } else {
                r.fill_circle(x, y, rr + 2.5 * sc, (0.0, 0.0, 0.0, 0.55));
                r.fill_circle(x, y, rr, c);
                r.stroke_circle(x, y, rr, 1.6 * sc, (1.0, 1.0, 1.0, 0.95), false);
                r.fill_circle(x, y, rr * 0.28, (1.0, 1.0, 1.0, 0.95));
                if p.optional {
                    r.stroke_circle(x, y, rr + 5.0 * sc, 1.5 * sc, c, true);
                }
            }
        }
        // Légende à droite du carré par défaut.
        let right = (m.sq.x0 + m.sq.side) as f32 + 24.0 * sc;
        let lw = (gw as f32 - right - 16.0 * sc).min(if self.cfg.panel_collapsed { 270.0 } else { 430.0 } * sc);
        let mut legend_rect = None;
        if lw > 180.0 * sc {
            let top = 60.0 * sc;
            let view = panel::View {
                s: self.s,
                legend: &m.legend,
                layers: &m.layers,
                status: self.last_status.as_ref(),
                show_done: self.cfg.show_done,
                min_chance: self.cfg.min_chance,
                lang_label,
                collapsed: self.cfg.panel_collapsed,
            };
            let _ = self.prenderer.set_font_scale(sc);
            let (h, _) = panel::layout(&self.prenderer, &view, lw.floor(), gh as f32 - 120.0 * sc, sc, None, false);
            let (pw, ph) = (lw.floor() as i32, h.ceil() as i32);
            if self.prenderer.begin(pw, ph) {
                let (_, hits) = panel::layout(&self.prenderer, &view, pw as f32, ph as f32, sc, self.panel_hover, true);
                self.prenderer.present(self.panel, gx + right as i32, gy + top as i32);
                self.panel_hits = hits;
            }
            legend_rect = Some((right, top, right + pw as f32, top + ph as f32));
        }
        if let Some(i) = self.hover {
            if let Some(p) = m.points.get(i) {
                let (x, y) = m.project(p.u, p.v);
                let lq = m.legend.get(p.quest);
                let title = match p.layer {
                    Some(li) => m.layers.get(li).map(|l| l.title.as_str()).unwrap_or(""),
                    None => lq.map(|q| q.title.as_str()).unwrap_or(""),
                };
                // Infobulle = ce point seulement : l'objectif, puis le conteneur et SA chance.
                // (Le résumé « Meilleur : … · Ennemis : … » concerne tout l'objectif : il reste dans le panneau.)
                let mut lines = vec![];
                let obj = p.objective.and_then(|oi| lq.and_then(|q| q.objectives.get(oi)));
                if let Some(o) = obj {
                    lines.push(o.label.clone());
                }
                if let Some(h) = &p.harvest {
                    let node = h.node.clone().unwrap_or_default();
                    let mut l = format!("{} : {node}", self.s.tip_harvest);
                    if let Some(pr) = &h.profession {
                        l += &format!(" · {pr}");
                    }
                    if let Some(c) = p.chance {
                        l += &format!(" · {:.0} % {}", c * 100.0, self.s.tip_chance);
                    }
                    lines.push(l);
                    if let Some(lv) = h.level {
                        let you = h.player.map(|pl| format!(" ({} : {pl})", self.s.tip_you)).unwrap_or_default();
                        lines.push(format!("{} : {lv}{you}", self.s.tip_level));
                    }
                } else { match p.chance {
                    Some(c) => {
                        let container = p.container.as_deref().or_else(|| p.label.rfind('(').and_then(|i| p.label[i + 1..].strip_suffix(')')).map(str::trim)).filter(|t| !t.is_empty());
                        let pct = c * 100.0;
                        let pct = if pct >= 10.0 { format!("{pct:.0}") } else { format!("{pct:.1}") };
                        match container {
                            Some(k) => lines.push(format!("{} : {k} · {pct} % {}", self.s.tip_container, self.s.tip_chance)),
                            None => lines.push(format!("{} · {pct} % {}", p.label, self.s.tip_chance)),
                        }
                    }
                    None => {
                        if !p.label.is_empty() && obj.map(|o| o.label != p.label && !p.label.starts_with(&o.label)).unwrap_or(true) {
                            lines.push(p.label.clone());
                        }
                    }
                } }
                draw_tooltip(r, title, p.color, &lines, x as f32, y as f32, gw as f32, gh as f32, sc);
            }
        }
        self.renderer.present(self.overlay, gx, gy);
        self.legend_rect = legend_rect;
        self.show_panel(legend_rect.is_some());
    }

    // ---------- Paramètres ----------

    fn open_settings(&mut self) {
        if let Some(s) = self.settings.as_mut() {
            unsafe {
                let _ = SetForegroundWindow(s.hwnd);
            }
            return;
        }
        self.shared.lock().unwrap().validate = None;
        let key = self.cfg.api_key().unwrap_or_default();
        let auto_lang = {
            let account = self.shared.lock().unwrap().tracker.as_ref().and_then(|t| t.locale.clone());
            format!("Auto ({})", i18n::effective("", account.as_deref()).to_uppercase())
        };
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
                auto_lang: auto_lang.clone(),
            },
        );
    }

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
    /// Installe une mise à jour téléchargée, jamais pendant que la carte est affichée ; vérifie toutes les 6 h.
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
        logf!("installation de la version {version}");
        tray::remove(self.main);
        match update::install_and_restart(&path) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                logf!("mise à jour impossible : {e}");
                tray::add(self.main, WM_TRAY, "Hunger Overlay");
                tray::balloon(self.main, "Hunger Overlay", self.s.update_available);
            }
        }
    }

    /// Applique les valeurs de la fenêtre de paramètres (sans la fermer). Renvoie false si la clé est invalide.
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
        if v.autostart != self.cfg.autostart {
            config::set_autostart(v.autostart);
        }
        self.cfg.autostart = v.autostart;
        self.cfg.save();
        self.apply_capture_affinity();
        if net_changed {
            // Nouvelle clé ou langue : on recharge (le plafond horaire reste appliqué par le thread réseau).
            self.configure_net();
            let _ = self.net.send(Req::Refresh { force: true });
        }
        if let MapState::Open(m) = &mut self.state {
            m.data_version = u64::MAX; // reconstruit les points (filtre de chance, terminés…)
        }
        self.dirty = true;
        logf!("paramètres appliqués (touche 0x{:02X}, chance min {} %)", self.cfg.map_vk, self.cfg.min_chance);
        true
    }
}

fn status_text(s: &i18n::Strings, st: Option<&Status>) -> &'static str {
    match st {
        Some(Status::Ok) | None => s.st_ok,
        Some(Status::NoKey) => s.st_no_key,
        Some(Status::InvalidKey) => s.st_invalid_key,
        Some(Status::Offline) => s.st_offline,
        Some(Status::RateLimited) => s.st_rate,
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_tooltip(r: &Renderer, title: &str, color: (f32, f32, f32), lines: &[String], x: f32, y: f32, gw: f32, gh: f32, sc: f32) {
    let pad = 10.0 * sc;
    let maxw = 380.0 * sc;
    let (tw, th) = r.measure(title, Font::Bold, maxw);
    let mut w = tw;
    let mut h = th;
    let mut hs = vec![];
    for l in lines {
        let (lw, lh) = r.measure(l, Font::Text, maxw);
        w = w.max(lw);
        h += lh + 2.0 * sc;
        hs.push(lh);
    }
    let (bw, bh) = (w + 2.0 * pad, h + 2.0 * pad);
    let mut bx = x + 16.0 * sc;
    let mut by = y - bh / 2.0;
    if bx + bw > gw - 8.0 {
        bx = x - 16.0 * sc - bw;
    }
    by = by.clamp(8.0, gh - bh - 8.0);
    r.fill_rrect(bx, by, bx + bw, by + bh, 6.0 * sc, (0.06, 0.05, 0.04, 0.92));
    r.stroke_rrect(bx, by, bx + bw, by + bh, 6.0 * sc, 1.5 * sc, (color.0, color.1, color.2, 0.9));
    let mut cy = by + pad;
    r.text(title, Font::Bold, bx + pad, cy, maxw, (color.0.max(0.6), color.1.max(0.6), color.2.max(0.6), 1.0));
    cy += th + 2.0 * sc;
    for (l, lh) in lines.iter().zip(hs) {
        r.text(l, Font::Text, bx + pad, cy, maxw, (0.93, 0.91, 0.86, 1.0));
        cy += lh + 2.0 * sc;
    }
}

/// Accès à la fenêtre de paramètres ouverte (depuis sa procédure de fenêtre).
pub fn with_settings<R>(f: impl FnOnce(&mut settings::Settings) -> R) -> Option<R> {
    with_app(|a| a.settings.as_mut().map(f)).flatten()
}

/// Clic dans la fenêtre de paramètres : géré par la fenêtre, puis par l'appli pour les actions globales.
pub fn on_settings_click(x: f32, y: f32, down: bool) {
    with_app(|a| {
        let act = a.settings.as_mut().and_then(|s| s.click(x, y, down));
        if let Some(id) = act {
            a.settings_command(id);
        }
    });
}

/// Appelé par la fenêtre de paramètres.
pub fn on_settings_command(id: i32) {
    with_app(|a| a.settings_command(id));
}

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
            if flags != 0 {
                with_app(|a| a.on_mouse(flags, data));
            }
        }
    }
}

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_INPUT => {
            handle_raw_input(lp);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_TIMER if wp.0 == TIMER_TICK => {
            with_app(|a| a.tick());
            LRESULT(0)
        }
        WM_NET => {
            with_app(|a| {
                a.dirty = true;
                a.sync_data();
            });
            LRESULT(0)
        }
        WM_TRAY => {
            let ev = (lp.0 & 0xffff) as u32;
            if ev == WM_RBUTTONUP || ev == WM_LBUTTONUP || ev == WM_CONTEXTMENU {
                let upd = with_app(|a| a.shared.lock().unwrap().update.is_some()).unwrap_or(false);
                let s = i18n::t();
                tray::menu(hwnd, s, upd);
            } else if ev == WM_LBUTTONDBLCLK {
                with_app(|a| a.open_settings());
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            match wp.0 & 0xffff {
                tray::ID_SETTINGS => {
                    with_app(|a| a.open_settings());
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
        WM_DESTROY => {
            tray::remove(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => {
            let tc = with_app(|a| a.taskbar_created).unwrap_or(0);
            if tc != 0 && msg == tc {
                tray::add(hwnd, WM_TRAY, "Hunger Overlay");
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
    }
}

unsafe extern "system" fn panel_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let pos = || (((lp.0 & 0xffff) as i16) as f32, (((lp.0 >> 16) & 0xffff) as i16) as f32);
    match msg {
        WM_MOUSEACTIVATE => LRESULT(3), // MA_NOACTIVATE : le jeu garde le focus
        WM_MOUSEMOVE => {
            let (x, y) = pos();
            let mut tme = windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT>() as u32,
                dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::TrackMouseEvent(&mut tme);
            with_app(|a| a.panel_mouse(x, y));
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
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MOUSEWHEEL => LRESULT(0),
        WM_LBUTTONUP => {
            let (x, y) = pos();
            with_app(|a| a.panel_click(x, y));
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(-1), // HTTRANSPARENT : les clics passent au jeu
        WM_MOUSEACTIVATE => LRESULT(3), // MA_NOACTIVATE
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

static SINGLE_INSTANCE: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Libère le verrou « une seule instance » (avant de lancer la version mise à jour).
pub fn release_single_instance() {
    let h = SINGLE_INSTANCE.swap(0, std::sync::atomic::Ordering::SeqCst);
    if h != 0 {
        unsafe {
            let _ = windows::Win32::System::Threading::ReleaseMutex(windows::Win32::Foundation::HANDLE(h as _));
            let _ = CloseHandle(windows::Win32::Foundation::HANDLE(h as _));
        }
    }
}

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let s = i18n::t();
        let updated = std::env::args().any(|a| a == "--updated");
        // Après une mise à jour, l'ancienne instance peut mettre un instant à se fermer : on attend un peu.
        let mut tries = if updated { 50 } else { 1 };
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
        logf!("démarrage v{}{}", env!("CARGO_PKG_VERSION"), if updated { " (après mise à jour)" } else { "" });
        update::cleanup();
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);

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
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(panel_proc), hInstance: inst.into(), lpszClassName: panel_class, ..Default::default() });
        // Pas de WS_EX_TRANSPARENT : ce panneau reçoit les clics (sans s'activer).
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

        let devices = [
            RAWINPUTDEVICE { usUsagePage: 1, usUsage: 6, dwFlags: RIDEV_INPUTSINK, hwndTarget: main },
            RAWINPUTDEVICE { usUsagePage: 1, usUsage: 2, dwFlags: RIDEV_INPUTSINK, hwndTarget: main },
        ];
        if RegisterRawInputDevices(&devices, std::mem::size_of::<RAWINPUTDEVICE>() as u32).is_err() {
            logf!("RegisterRawInputDevices a échoué");
        }

        let shared: SharedRef = Arc::new(Mutex::new(Default::default()));
        let main_id = main.0 as isize;
        let net = net::spawn(
            shared.clone(),
            Box::new(move || {
                let _ = PostMessageW(HWND(main_id as _), WM_NET, WPARAM(0), LPARAM(0));
            }),
        );
        let cfg = Config::load();
        let (rs_tx, rs_out) = resync::spawn_worker(Box::new(move || {
            let _ = PostMessageW(HWND(main_id as _), WM_NET, WPARAM(0), LPARAM(0));
        }));
        let renderer = Renderer::new().expect("Direct2D");
        let prenderer = Renderer::new().expect("Direct2D");
        let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
        let app = App {
            main,
            overlay,
            panel,
            prenderer,
            panel_hits: vec![],
            panel_hover: None,
            panel_shown: false,
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
            next_update_check: Instant::now() + Duration::from_secs(6 * 3600),
        };
        app.apply_capture_affinity();
        app.configure_net();
        let _ = app.net.send(Req::CheckUpdate { install: app.cfg.auto_update });
        let no_key = app.cfg.api_key().is_none();
        APP.with(|a| *a.borrow_mut() = Some(app));
        tray::add(main, WM_TRAY, "Hunger Overlay");
        if updated {
            let t = format!("{} {}.", i18n::t().updated_to, env!("CARGO_PKG_VERSION"));
            tray::balloon(main, "Hunger Overlay", &t);
        }
        if no_key {
            with_app(|a| a.open_settings());
        } else {
            with_app(|a| {
                let _ = a.net.send(Req::Refresh { force: true });
            });
        }
        SetTimer(main, TIMER_TICK, 15, None);

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
        logf!("arrêt");
    }
}
