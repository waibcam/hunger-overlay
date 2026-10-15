//! Communication with the site, on a dedicated thread (never on the UI thread).
//!
//! Rate (contract v1): pack once per launch (ETag), tracker when the map is opened with
//! at least 30 s between calls and at most 100 requests per hour; no background polling.
//!
//! Everything below goes over HTTPS to `base` only (https://hunger-companion.com by default, see `config.rs`):
//! - `GET  /api/overlay/v1/pack[.<lang>].json`, `items.json`, `items_index[.<lang>].json`, `items/<ID_…>.json`,
//!   `app.json`: static public data files, sent WITHOUT the API key.
//! - `GET  /api/overlay/v1/me/tracker`: the user's tracked quests, with `Authorization: Bearer <key>`.
//! - `POST /api/overlay/v1/me/track`: only when the user picks a quest to (un)track; body
//!   `{"quest_id": "...", "tracked": true|false}`, same Authorization header.
//!
//! The API key travels only in the Authorization header: it is never put in a URL, never written to the log
//! (errors are logged through `err_kind`, which drops request details) and never sent to any other host.
//! Calls that use the key (tracker, track, key test) share the limits above: at most `MAX_PER_HOUR` per rolling
//! hour, at least `MIN_INTERVAL` apart for automatic refreshes, and a 429/503 `Retry-After` is honoured.
//! Responses are cached in %APPDATA%\HungerOverlay (pack.json, tracker.json, items.json, etags.json).

use crate::model::{ItemFile, ItemsPack, Pack, SearchIndex, Tracker};
use crate::logf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Minimum delay between two automatic tracker requests.
pub const MIN_INTERVAL: Duration = Duration::from_secs(30);
/// Maximum number of authenticated requests (tracker, track, key test) per rolling hour.
pub const MAX_PER_HOUR: usize = 100;

/// Connection state shown to the user.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// Last request succeeded.
    Ok,
    /// No API key configured: nothing authenticated is sent.
    NoKey,
    /// The site rejected the key (400/401): no further tracker request until the key changes.
    InvalidKey,
    /// Network error or unusable answer: cached data is shown.
    Offline,
    /// The site answered 429 (or the local limits were hit): waiting before retrying.
    RateLimited,
}

/// Requests sent from the UI thread to the network thread.
pub enum Req {
    /// Refresh the tracker (map opened; `force` = manual request, skips the 30 s interval but not the hourly cap).
    Refresh { force: bool },
    /// New configuration (key / URL / language).
    /// `locale` = forced language (empty = automatic: site account, otherwise Windows).
    Configure { key: Option<String>, base_url: String, locale: String },
    /// Test a key from the settings window, without keeping it.
    Validate(String),
    /// Item search index (in the wanted language).
    FetchIndex,
    /// Markers of a searched item.
    FetchItem(String),
    /// Track / untrack a quest (write: POST /me/track).
    Track { quest_id: String, tracked: bool },
    /// Check whether a new version exists; `install` = download it (and verify it) so it can be installed.
    CheckUpdate { install: bool },
}

/// State shared between the network thread (writer) and the UI thread (reader), behind a mutex.
#[derive(Default)]
pub struct Shared {
    /// Static quest data (markers, names) for the current language.
    pub pack: Option<Pack>,
    /// The user's tracked quests, as returned by /me/tracker.
    pub tracker: Option<Tracker>,
    /// Where to find recipe ingredients (loaded only if a crafting list exists).
    pub items: Option<ItemsPack>,
    /// Item search: index and files of the searched items.
    pub search_index: Option<SearchIndex>,
    /// Loaded files of searched items, by item key (ID_…).
    pub search_items: std::collections::HashMap<String, ItemFile>,
    /// Message to show the user after a refused write ("unsupported" | "readonly" | "failed").
    pub notice: Option<&'static str>,
    /// Current connection state (None = nothing attempted yet).
    pub status: Option<Status>,
    /// Incremented on every data change (the UI redraws).
    pub version: u64,
    /// Result of the last key test: Ok(account name) or Err(status).
    pub validate: Option<Result<String, Status>>,
    /// New version available: (version, page url).
    pub update: Option<(String, String)>,
    /// New version downloaded and verified, ready to install: (version, file).
    pub update_ready: Option<(String, std::path::PathBuf)>,
}

/// Handle to the shared state.
pub type SharedRef = Arc<Mutex<Shared>>;

/// HTTP cache validators persisted in etags.json, so unchanged files are answered with a light 304.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Etags {
    /// ETag of the cached pack.
    pack: Option<String>,
    /// URL the cached pack came from (the ETag is only reused for that same URL/language).
    pack_url: Option<String>,
    /// ETag of the cached tracker.
    tracker: Option<String>,
    /// ETag of the cached items.json.
    #[serde(default)]
    items: Option<String>,
    /// Unix time (s) of the last update check.
    last_update_check: u64,
}

/// Network thread state. Lives only on that thread.
struct Worker {
    shared: SharedRef,
    /// Callback that wakes the UI thread after a change.
    notify: Box<dyn Fn() + Send>,
    /// HTTPS client (15 s timeout, native Windows TLS).
    agent: ureq::Agent,
    /// Decrypted API key, kept in memory only (sent solely in the Authorization header).
    key: Option<String>,
    /// Site base URL, without trailing slash.
    base: String,
    /// Forced language (empty = automatic).
    forced: String,
    /// Language of the pack currently cached.
    locale: String,
    etags: Etags,
    /// Times of authenticated requests in the last hour (for `MAX_PER_HOUR`).
    history: Vec<Instant>,
    /// Time of the last automatic tracker request (for `MIN_INTERVAL`).
    last: Option<Instant>,
    /// Set after a 429/503: no authenticated request before this instant.
    blocked_until: Option<Instant>,
    /// The site rejected the key: stop calling until the key or URL changes.
    invalid_key: bool,
    /// Recipes whose ingredients are currently kept in memory.
    items_for: Vec<String>,
}

/// User-Agent sent with every request: app name + version + info page, nothing about the user.
pub fn user_agent() -> String {
    format!("HungerOverlay/{} (Windows NT 10.0; +https://hunger-companion.com/overlay)", env!("CARGO_PKG_VERSION"))
}

/// Starts the network thread (loads the disk cache, then serves `Req`s one at a time) and returns its request channel.
pub fn spawn(shared: SharedRef, notify: Box<dyn Fn() + Send>) -> Sender<Req> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let tls = native_tls::TlsConnector::new().expect("TLS");
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(15))
            .user_agent(&user_agent())
            .tls_connector(Arc::new(tls))
            .build();
        let mut w = Worker {
            shared,
            notify,
            agent,
            key: None,
            base: String::new(),
            forced: String::new(),
            locale: String::new(),
            etags: Default::default(),
            history: vec![],
            last: None,
            blocked_until: None,
            invalid_key: false,
            items_for: vec![],
        };
        w.load_cache();
        w.run(rx);
    });
    tx
}

/// Current Unix time in seconds.
fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Worker {
    /// Path of a cache file in %APPDATA%\HungerOverlay.
    fn cache(name: &str) -> std::path::PathBuf {
        crate::config::data_dir().join(name)
    }

    /// Loads the last pack/tracker/items from disk so markers show immediately, even offline.
    /// An ETag is dropped when its cached file is missing or unreadable, forcing a full download next time.
    fn load_cache(&mut self) {
        let pack: Option<Pack> = std::fs::read_to_string(Self::cache("pack.json")).ok().and_then(|s| serde_json::from_str(&s).ok());
        let tracker: Option<Tracker> = std::fs::read_to_string(Self::cache("tracker.json")).ok().and_then(|s| serde_json::from_str(&s).ok());
        self.etags = std::fs::read_to_string(Self::cache("etags.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        if pack.is_none() {
            self.etags.pack = None;
        }
        if tracker.is_none() {
            self.etags.tracker = None;
        }
        let mut items: Option<ItemsPack> = std::fs::read_to_string(Self::cache("items.json")).ok().and_then(|s| serde_json::from_str(&s).ok());
        if items.is_none() {
            self.etags.items = None;
        }
        if let Some(it) = items.as_mut() {
            keep_needed_items(it, tracker.as_ref());
        }
        self.shared.lock().unwrap().items = items;
        logf!("cache: pack={} tracker={}",pack.as_ref().map(|p| p.pack_version.as_str()).unwrap_or("-"), tracker.is_some());
        self.locale = pack.as_ref().map(|p| p.locale.clone()).unwrap_or_default();
        let mut s = self.shared.lock().unwrap();
        s.pack = pack;
        s.tracker = tracker;
        s.version += 1;
    }

    /// Language wanted now: the one chosen in the settings, otherwise Windows' (English if not offered).
    fn wanted_locale(&self) -> String {
        crate::i18n::effective(&self.forced)
    }

    /// Persists the cache validators to etags.json.
    fn save_etags(&self) {
        let _ = std::fs::write(Self::cache("etags.json"), serde_json::to_string(&self.etags).unwrap_or_default());
    }

    /// Updates the status and wakes the UI, only if it actually changed.
    fn set_status(&self, st: Status) {
        let mut s = self.shared.lock().unwrap();
        if s.status.as_ref() != Some(&st) {
            s.status = Some(st);
            s.version += 1;
            drop(s);
            (self.notify)();
        }
    }

    /// Main loop: blocks until the UI sends a request (nothing happens on its own), handles it, repeats.
    /// Ends when the UI drops the channel.
    fn run(&mut self, rx: Receiver<Req>) {
        while let Ok(req) = rx.recv() {
            match req {
                Req::Configure { key, base_url, locale } => {
                    // A different key or site: forget the "invalid key" flag and the tracker ETag.
                    if key != self.key || base_url != self.base {
                        self.invalid_key = false;
                        self.etags.tracker = None;
                    }
                    self.key = key;
                    self.base = base_url.trim_end_matches('/').to_string();
                    self.forced = locale;
                    if self.key.is_none() {
                        self.set_status(Status::NoKey);
                    }
                }
                Req::Refresh { force } => self.refresh(force),
                Req::Validate(key) => {
                    let r = self.validate(&key);
                    self.shared.lock().unwrap().validate = Some(r);
                    (self.notify)();
                }
                Req::CheckUpdate { install } => self.check_update(install),
                Req::FetchIndex => self.fetch_index(),
                Req::FetchItem(key) => self.fetch_item(&key),
                Req::Track { quest_id, tracked } => self.track(&quest_id, tracked),
            }
        }
    }

    /// Client-side rate limiter for authenticated requests. Refuses if: a server-requested pause
    /// (`Retry-After`) is still running, `MAX_PER_HOUR` requests were already made in the last hour, or
    /// (unless `force`) the previous automatic refresh was less than `MIN_INTERVAL` ago.
    fn allowed(&mut self, force: bool) -> bool {
        let now = Instant::now();
        if let Some(t) = self.blocked_until {
            if now < t {
                return false;
            }
        }
        self.history.retain(|t| now.duration_since(*t) < Duration::from_secs(3600));
        if self.history.len() >= MAX_PER_HOUR {
            return false;
        }
        if !force {
            if let Some(l) = self.last {
                if now.duration_since(l) < MIN_INTERVAL {
                    return false;
                }
            }
        }
        true
    }

    /// Pause requested by the server (`Retry-After`, seconds), default 300 s, clamped to 30 s – 1 h.
    fn retry_after(resp: &ureq::Response) -> Duration {
        Duration::from_secs(resp.header("Retry-After").and_then(|v| v.trim().parse().ok()).unwrap_or(300).clamp(30, 3600))
    }

    /// Fetches the user's tracker: `GET {base}/api/overlay/v1/me/tracker` with `Authorization: Bearer <key>`
    /// (plus `If-None-Match` when cached). Subject to `allowed`. Then downloads the pack/items if needed (`finish`).
    fn refresh(&mut self, force: bool) {
        let Some(key) = self.key.clone() else {
            self.set_status(Status::NoKey);
            return;
        };
        if self.invalid_key || !self.allowed(force) {
            return;
        }
        // Count this request for the rate limits before sending it.
        let now = Instant::now();
        self.history.push(now);
        self.last = Some(now);
        // The key goes only in the Authorization header, never in the URL.
        let url = format!("{}/api/overlay/v1/me/tracker", self.base);
        let mut req = self.agent.get(&url).set("Authorization", &format!("Bearer {key}")).set("Accept", "application/json");
        if let Some(e) = &self.etags.tracker {
            req = req.set("If-None-Match", e);
        }
        match req.call() {
            Ok(resp) if resp.status() == 304 => {
                // Tracker unchanged: still fetch the pack if it is missing, outdated or in the wrong language.
                logf!("tracker: 304");
                let s = self.shared.lock().unwrap();
                let need_pack = match (&s.pack, &s.tracker) {
                    (Some(p), Some(t)) => p.pack_version != t.pack_version,
                    _ => true,
                };
                drop(s);
                let need_pack = need_pack || self.wanted_locale() != self.locale;
                return self.finish(need_pack);
            }
            Ok(resp) => {
                // ETag sent back AS IS (Cloudflare rewrites it as W/"…"; the site knows how to compare it).
                let etag = resp.header("ETag").map(|e| e.trim().to_string());
                let body = resp.into_string().unwrap_or_default();
                match serde_json::from_str::<Tracker>(&body) {
                    Ok(t) => {
                        logf!("tracker: 200, {} tracked quest(s), pack {}", t.tracked.len(), t.pack_version);
                        let _ = std::fs::write(Self::cache("tracker.json"), &body);
                        self.etags.tracker = etag.or_else(|| t.rev.as_ref().map(|r| format!("\"{r}\"")));
                        let mut s = self.shared.lock().unwrap();
                        let need_pack = s.pack.as_ref().map(|p| p.pack_version != t.pack_version).unwrap_or(true);
                        s.tracker = Some(t);
                        s.version += 1;
                        drop(s);
                        let need_pack = need_pack || self.wanted_locale() != self.locale;
                        return self.finish(need_pack);
                    }
                    Err(e) => {
                        logf!("tracker: invalid JSON ({e})");
                        self.set_status(Status::Offline);
                        return;
                    }
                }
            }
            Err(ureq::Error::Status(code, resp)) => {
                logf!("tracker: HTTP {code}");
                match code {
                    // Key rejected: stop calling until the user changes it.
                    401 | 400 => {
                        self.invalid_key = true;
                        self.set_status(Status::InvalidKey);
                    }
                    // Server asks us to slow down: pause all authenticated requests for Retry-After.
                    429 | 503 => {
                        self.blocked_until = Some(Instant::now() + Self::retry_after(&resp));
                        self.set_status(if code == 429 { Status::RateLimited } else { Status::Offline });
                    }
                    _ => self.set_status(Status::Offline),
                }
                (self.notify)();
                return;
            }
            Err(e) => {
                logf!("tracker: network ({})", err_kind(&e));
                self.set_status(Status::Offline);
                return;
            }
        }
    }

    /// After a successful tracker call: downloads the pack if `need_pack`, the ingredient list if the crafting
    /// list needs it, saves the ETags and reports `Status::Ok` (or `Offline` if the pack could not be fetched).
    fn finish(&mut self, need_pack: bool) {
        if need_pack && !self.fetch_pack() {
            self.save_etags();
            self.set_status(Status::Offline);
            (self.notify)();
            return;
        }
        // Crafting list: we need to know where to find the ingredients (items.json, same version as the pack).
        let need_items = {
            let s = self.shared.lock().unwrap();
            match &s.tracker {
                Some(t) if !t.crafting.is_empty() => s.items.as_ref().map(|i| i.pack_version != t.pack_version).unwrap_or(true),
                _ => false,
            }
        };
        if need_items {
            self.fetch_items();
        } else {
            // Crafting list changed: reload the ingredients from the disk cache (no network).
            let mut s = self.shared.lock().unwrap();
            let wanted: Vec<String> = s.tracker.as_ref().map(|t| t.crafting.iter().map(|c| c.recipe_id.clone()).collect()).unwrap_or_default();
            if wanted != self.items_for && s.items.is_some() {
                self.items_for = wanted;
                if let Some(mut it) = std::fs::read_to_string(Self::cache("items.json")).ok().and_then(|b| serde_json::from_str::<ItemsPack>(&b).ok()) {
                    keep_needed_items(&mut it, s.tracker.as_ref());
                    s.items = Some(it);
                    s.version += 1;
                }
            }
        }
        self.save_etags();
        self.set_status(Status::Ok);
        (self.notify)();
    }

    /// Unauthenticated GET of a public JSON file (no API key sent), parsed into `T`.
    fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, String> {
        let resp = self.agent.get(url).set("Accept", "application/json").call().map_err(|e| err_kind(&e))?;
        let body = resp.into_string().map_err(|e| e.to_string())?;
        serde_json::from_str(&body).map_err(|e| e.to_string())
    }

    /// Downloads the public item search index (`items_index.<lang>.json`, falling back to the English
    /// `items_index.json`). No API key is sent.
    fn fetch_index(&mut self) {
        let want = self.wanted_locale();
        let mut urls = vec![];
        if want != "en" {
            urls.push(format!("{}/api/overlay/v1/items_index.{want}.json", self.base));
        }
        urls.push(format!("{}/api/overlay/v1/items_index.json", self.base));
        for url in urls {
            match self.get_json::<SearchIndex>(&url) {
                Ok(ix) => {
                    logf!("search index: {} items ({})", ix.items.len(), ix.locale);
                    let mut s = self.shared.lock().unwrap();
                    s.search_index = Some(ix);
                    s.version += 1;
                    drop(s);
                    (self.notify)();
                    return;
                }
                Err(e) => logf!("search index: {e} ({url})"),
            }
        }
    }

    /// Downloads the public marker file of one searched item (`items/<ID_…>.json`). No API key is sent.
    /// `key` here is the game's item key, not the API key.
    fn fetch_item(&mut self, key: &str) {
        // Game key only (ID_…): nothing else goes into the URL.
        if !key.starts_with("ID_") || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return;
        }
        let tmpl = self.shared.lock().unwrap().search_index.as_ref().map(|i| i.item_url.clone()).filter(|u| u.contains("{item_key}"));
        // The URL template from the index is used only if it points to the same site; otherwise the default path.
        let url = match tmpl {
            Some(t) if t.starts_with(&self.base) => t.replace("{item_key}", key),
            _ => format!("{}/api/overlay/v1/items/{key}.json", self.base),
        };
        match self.get_json::<ItemFile>(&url) {
            Ok(f) => {
                logf!("item {key}: {} points", f.markers.len());
                let mut s = self.shared.lock().unwrap();
                s.search_items.insert(key.to_string(), f);
                s.version += 1;
                drop(s);
                (self.notify)();
            }
            Err(e) => logf!("item {key}: {e}"),
        }
    }

    /// Track / untrack a quest. The site returns the new tracker state (same JSON as GET).
    /// The only write the app ever makes: `POST {base}/api/overlay/v1/me/track` with
    /// `Authorization: Bearer <key>` and body `{"quest_id": ..., "tracked": ...}`, sent only on an explicit user
    /// action. Counts toward `MAX_PER_HOUR` (the 30 s interval does not apply to user actions).
    fn track(&mut self, quest_id: &str, tracked: bool) {
        let Some(key) = self.key.clone() else { return };
        if !self.allowed(true) {
            return;
        }
        self.history.push(Instant::now());
        let url = format!("{}/api/overlay/v1/me/track", self.base);
        let body = serde_json::json!({ "quest_id": quest_id, "tracked": tracked }).to_string();
        let notice = match self.agent.post(&url).set("Authorization", &format!("Bearer {key}")).set("Content-Type", "application/json").send_string(&body) {
            Ok(resp) => {
                let etag = resp.header("ETag").map(|e| e.trim().to_string());
                let text = resp.into_string().unwrap_or_default();
                match serde_json::from_str::<Tracker>(&text) {
                    Ok(t) => {
                        logf!("tracking: {quest_id} → {tracked}");
                        let _ = std::fs::write(Self::cache("tracker.json"), &text);
                        self.etags.tracker = etag.or_else(|| t.rev.as_ref().map(|r| format!("\"{r}\"")));
                        self.save_etags();
                        let mut s = self.shared.lock().unwrap();
                        s.tracker = Some(t);
                        s.version += 1;
                        None
                    }
                    Err(_) => {
                        // Response without the tracker: simply re-read the state.
                        self.etags.tracker = None;
                        self.refresh(true);
                        None
                    }
                }
            }
            // 404/405: the site has no write endpoint yet; 403: the key was issued read-only.
            Err(ureq::Error::Status(404, _)) | Err(ureq::Error::Status(405, _)) => Some("unsupported"),
            Err(ureq::Error::Status(403, _)) => Some("readonly"),
            Err(e) => {
                logf!("tracking: {}", err_kind(&e));
                Some("failed")
            }
        };
        if let Some(n) = notice {
            logf!("tracking refused: {n}");
            self.shared.lock().unwrap().notice = Some(n);
        }
        (self.notify)();
    }

    /// Downloads the public `items.json` (where each crafting ingredient can be found), with ETag.
    /// No API key is sent. Only the ingredients of the user's crafting list are kept in memory.
    fn fetch_items(&mut self) {
        let url = format!("{}/api/overlay/v1/items.json", self.base);
        let mut req = self.agent.get(&url).set("Accept", "application/json");
        if let Some(e) = &self.etags.items {
            req = req.set("If-None-Match", e);
        }
        match req.call() {
            Ok(resp) if resp.status() == 304 => logf!("items: 304"),
            Ok(resp) => {
                let etag = resp.header("ETag").map(|e| e.trim().to_string());
                let body = resp.into_string().unwrap_or_default();
                match serde_json::from_str::<ItemsPack>(&body) {
                    Ok(mut it) => {
                        logf!("items: 200 ({} items, {} KB)", it.items.len(), body.len() / 1024);
                        let _ = std::fs::write(Self::cache("items.json"), &body);
                        self.etags.items = etag;
                        let mut s = self.shared.lock().unwrap();
                        keep_needed_items(&mut it, s.tracker.as_ref());
                        s.items = Some(it);
                        s.version += 1;
                    }
                    Err(e) => logf!("items: invalid JSON ({e})"),
                }
            }
            Err(e) => logf!("items: {}", err_kind(&e)),
        }
    }

    /// Downloads the pack (localized version if it exists, otherwise English). Returns false on network failure.
    /// Public static file: no API key is sent.
    fn fetch_pack(&mut self) -> bool {
        let want = self.wanted_locale();
        let mut urls = vec![];
        if want != "en" {
            urls.push(format!("{}/api/overlay/v1/pack.{}.json", self.base, want));
        }
        urls.push(format!("{}/api/overlay/v1/pack.json", self.base));
        for url in urls {
            let mut req = self.agent.get(&url).set("Accept", "application/json");
            // Reuse the ETag only if the cached pack came from this exact URL (same language).
            if self.etags.pack_url.as_deref() == Some(url.as_str()) {
                if let Some(e) = &self.etags.pack {
                    req = req.set("If-None-Match", e);
                }
            }
            match req.call() {
                Ok(resp) if resp.status() == 304 => {
                    logf!("pack: 304 ({url})");
                    self.locale = want.clone();
                    return true;
                }
                Ok(resp) => {
                    let etag = resp.header("ETag").map(|e| e.trim().to_string());
                    let body = resp.into_string().unwrap_or_default();
                    match serde_json::from_str::<Pack>(&body) {
                        Ok(p) => {
                            logf!("pack: 200 {} ({} quests, {} KB)", p.pack_version, p.quests.len(), body.len() / 1024);
                            let _ = std::fs::write(Self::cache("pack.json"), &body);
                            self.etags.pack = etag;
                            self.etags.pack_url = Some(url);
                            self.locale = if p.locale.is_empty() { want.clone() } else { p.locale.clone() };
                            let mut s = self.shared.lock().unwrap();
                            s.pack = Some(p);
                            s.version += 1;
                            return true;
                        }
                        Err(e) => {
                            logf!("pack: invalid JSON ({e})");
                            return false;
                        }
                    }
                }
                Err(ureq::Error::Status(404, _)) => {
                    logf!("pack: 404 ({url}), falling back");
                    continue;
                }
                Err(e) => {
                    logf!("pack: error ({})", err_kind(&e));
                    return false;
                }
            }
        }
        false
    }

    /// Tests a key typed in the settings (not stored): one `GET {base}/api/overlay/v1/me/tracker` with
    /// `Authorization: Bearer <key>`. Counts toward `MAX_PER_HOUR`. Returns the account name on success.
    fn validate(&mut self, key: &str) -> Result<String, Status> {
        if !self.allowed(true) {
            return Err(Status::RateLimited);
        }
        self.history.push(Instant::now());
        let url = format!("{}/api/overlay/v1/me/tracker", self.base);
        match self.agent.get(&url).set("Authorization", &format!("Bearer {}", key.trim())).call() {
            Ok(resp) => {
                let t: Option<Tracker> = resp.into_string().ok().and_then(|b| serde_json::from_str(&b).ok());
                Ok(t.and_then(|t| t.user).unwrap_or_default())
            }
            Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(400, _)) => Err(Status::InvalidKey),
            Err(ureq::Error::Status(429, _)) => Err(Status::RateLimited),
            Err(_) => Err(Status::Offline),
        }
    }

    /// Reads `app.json` (static file, no key). If a newer version exists: notification, and if
    /// `install`, download + SHA-256 verification (the UI installs it as soon as the map is not displayed).
    /// `app.json` provides `latest`, `notes_url`, `url` (the exe) and `sha256`; the exe is downloaded only from
    /// that `url`, and only if it ends in ".exe" and a 64-character hash is given (see `update::download`).
    fn check_update(&mut self, install: bool) {
        self.etags.last_update_check = now_secs();
        self.save_etags();
        let url = format!("{}/api/overlay/v1/app.json", self.base);
        let Ok(resp) = self.agent.get(&url).call() else { return };
        let Some(v) = resp.into_string().ok().and_then(|b| serde_json::from_str::<serde_json::Value>(&b).ok()) else { return };
        let latest = v["latest"].as_str().unwrap_or("").to_string();
        if !version_gt(&latest, env!("CARGO_PKG_VERSION")) {
            return;
        }
        let page = v["notes_url"].as_str().unwrap_or("https://hunger-companion.com/overlay").to_string();
        logf!("update available: {latest}");
        self.shared.lock().unwrap().update = Some((latest.clone(), page));
        (self.notify)();
        let (Some(exe), Some(sha)) = (v["url"].as_str(), v["sha256"].as_str()) else { return };
        if !install || !exe.ends_with(".exe") || sha.len() != 64 {
            return;
        }
        // Already downloaded and verified this version: nothing to do.
        if self.shared.lock().unwrap().update_ready.as_ref().map(|u| u.0 == latest).unwrap_or(false) {
            return;
        }
        match crate::update::download(&self.agent, exe, sha, &latest) {
            Ok(path) => {
                logf!("update {latest} downloaded and verified (SHA-256 OK)");
                self.shared.lock().unwrap().update_ready = Some((latest, path));
                (self.notify)();
            }
            Err(e) => logf!("update {latest}: {e}"),
        }
    }
}

/// `items.json` covers every ingredient of every recipe: only those of the player's crafting list are kept in
/// memory (the full file stays in the disk cache for when the list changes).
fn keep_needed_items(it: &mut ItemsPack, tracker: Option<&Tracker>) {
    let needed: std::collections::HashSet<String> = tracker
        .map(|t| {
            t.crafting
                .iter()
                .flat_map(|c| c.ingredients.iter())
                .map(|i| match &i.item_id {
                    serde_json::Value::String(s) => s.clone(),
                    v => v.to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    it.items.retain(|k, _| needed.contains(k));
    it.items.shrink_to_fit();
}

/// Error description WITHOUT the request details (which may contain the Authorization header): only the HTTP
/// status code or the transport error kind. This is what keeps the API key out of the log.
fn err_kind(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Status(c, _) => format!("HTTP {c}"),
        ureq::Error::Transport(t) => format!("{:?}", t.kind()),
    }
}

/// Expected key format: "hco_" + 32 alphanumeric characters (local check only, nothing is sent).
pub fn key_format_ok(k: &str) -> bool {
    k.len() == 36 && k.starts_with("hco_") && k[4..].bytes().all(|b| b.is_ascii_alphanumeric())
}

/// True if dotted version `a` is newer than `b` (numeric comparison, part by part: "1.10.0" > "1.9.3").
fn version_gt(a: &str, b: &str) -> bool {
    // Numeric parts only ("0.2.0-beta" → [0, 2, 0]); missing parts count as 0 ("0.2" = "0.2.0").
    let p = |s: &str| {
        let mut v: Vec<u32> = s.trim().trim_start_matches('v').split('.').map(|x| x.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0)).collect();
        while v.len() < 3 {
            v.push(0);
        }
        v
    };
    p(a) > p(b)
}

#[cfg(test)]
mod tests {
    /// A newer local build (e.g. a test version installed by hand) is never "updated" back to the published one.
    #[test]
    fn version_order() {
        use super::version_gt;
        assert!(version_gt("0.1.1", "0.1.0"));
        assert!(version_gt("0.1.10", "0.1.9"));
        assert!(version_gt("v0.2", "0.1.9"));
        assert!(!version_gt("0.1.0", "0.1.0"));
        assert!(!version_gt("0.1.0", "0.2.0"));
        assert!(!version_gt("0.2", "0.2.0"));
        assert!(!version_gt("", "0.1.0"));
    }
}
