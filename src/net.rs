//! Communication avec le site, dans un thread dédié (jamais sur le thread de l'interface).
//!
//! Cadence (contrat v1) : pack une fois par lancement (ETag), tracker à l'ouverture de la carte avec
//! au moins 30 s d'intervalle et au plus 100 requêtes par heure ; aucun polling de fond.

use crate::model::{Pack, Tracker};
use crate::logf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const MIN_INTERVAL: Duration = Duration::from_secs(30);
pub const MAX_PER_HOUR: usize = 100;

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Idle,
    Ok,
    NoKey,
    InvalidKey,
    Offline,
    RateLimited,
}

pub enum Req {
    /// Rafraîchir le tracker (ouverture de carte ; `force` = demande manuelle, ignore l'intervalle de 30 s mais pas le plafond horaire).
    Refresh { force: bool },
    /// Nouvelle configuration (clé / URL / langue).
    Configure { key: Option<String>, base_url: String, locale: String },
    /// Tester une clé depuis les paramètres, sans la conserver.
    Validate(String),
    /// Vérifier s'il existe une nouvelle version (une fois par jour).
    CheckUpdate,
}

#[derive(Default)]
pub struct Shared {
    pub pack: Option<Pack>,
    pub tracker: Option<Tracker>,
    pub status: Option<Status>,
    /// Incrémenté à chaque changement de données (l'interface redessine).
    pub version: u64,
    /// Résultat du dernier test de clé : Ok(nom du compte) ou Err(message).
    pub validate: Option<Result<String, Status>>,
    /// Nouvelle version disponible : (version, url de la page).
    pub update: Option<(String, String)>,
}

pub type SharedRef = Arc<Mutex<Shared>>;

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Etags {
    pack: Option<String>,
    pack_url: Option<String>,
    tracker: Option<String>,
    last_update_check: u64,
}

struct Worker {
    shared: SharedRef,
    notify: Box<dyn Fn() + Send>,
    agent: ureq::Agent,
    key: Option<String>,
    base: String,
    locale: String,
    etags: Etags,
    history: Vec<Instant>,
    last: Option<Instant>,
    blocked_until: Option<Instant>,
    invalid_key: bool,
}

pub fn user_agent() -> String {
    format!("HungerOverlay/{} (Windows NT 10.0; +https://hunger-companion.com/overlay)", env!("CARGO_PKG_VERSION"))
}

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
            locale: "en".into(),
            etags: Default::default(),
            history: vec![],
            last: None,
            blocked_until: None,
            invalid_key: false,
        };
        w.load_cache();
        w.run(rx);
    });
    tx
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Worker {
    fn cache(name: &str) -> std::path::PathBuf {
        crate::config::data_dir().join(name)
    }

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
        logf!("cache : pack={} tracker={}", pack.as_ref().map(|p| p.pack_version.as_str()).unwrap_or("-"), tracker.is_some());
        let mut s = self.shared.lock().unwrap();
        s.pack = pack;
        s.tracker = tracker;
        s.version += 1;
    }

    fn save_etags(&self) {
        let _ = std::fs::write(Self::cache("etags.json"), serde_json::to_string(&self.etags).unwrap_or_default());
    }

    fn set_status(&self, st: Status) {
        let mut s = self.shared.lock().unwrap();
        if s.status.as_ref() != Some(&st) {
            s.status = Some(st);
            s.version += 1;
            drop(s);
            (self.notify)();
        }
    }

    fn run(&mut self, rx: Receiver<Req>) {
        while let Ok(req) = rx.recv() {
            match req {
                Req::Configure { key, base_url, locale } => {
                    if key != self.key || base_url != self.base {
                        self.invalid_key = false;
                        self.etags.tracker = None;
                    }
                    if locale != self.locale {
                        self.etags.pack = None;
                    }
                    self.key = key;
                    self.base = base_url.trim_end_matches('/').to_string();
                    self.locale = locale;
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
                Req::CheckUpdate => self.check_update(),
            }
        }
    }

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

    fn retry_after(resp: &ureq::Response) -> Duration {
        Duration::from_secs(resp.header("Retry-After").and_then(|v| v.trim().parse().ok()).unwrap_or(300).clamp(30, 3600))
    }

    fn refresh(&mut self, force: bool) {
        let Some(key) = self.key.clone() else {
            self.set_status(Status::NoKey);
            return;
        };
        if self.invalid_key || !self.allowed(force) {
            return;
        }
        let now = Instant::now();
        self.history.push(now);
        self.last = Some(now);
        let url = format!("{}/api/overlay/v1/me/tracker", self.base);
        let mut req = self.agent.get(&url).set("Authorization", &format!("Bearer {key}")).set("Accept", "application/json");
        if let Some(e) = &self.etags.tracker {
            req = req.set("If-None-Match", e);
        }
        let need_pack;
        match req.call() {
            Ok(resp) if resp.status() == 304 => {
                logf!("tracker : 304");
                let s = self.shared.lock().unwrap();
                need_pack = match (&s.pack, &s.tracker) {
                    (Some(p), Some(t)) => p.pack_version != t.pack_version,
                    _ => true,
                };
            }
            Ok(resp) => {
                // ETag renvoyé TEL QUEL (Cloudflare le réécrit en W/"…" ; le site sait le comparer).
                let etag = resp.header("ETag").map(|e| e.trim().to_string());
                let body = resp.into_string().unwrap_or_default();
                match serde_json::from_str::<Tracker>(&body) {
                    Ok(t) => {
                        logf!("tracker : 200, {} quête(s) suivie(s), pack {}", t.tracked.len(), t.pack_version);
                        let _ = std::fs::write(Self::cache("tracker.json"), &body);
                        self.etags.tracker = etag.or_else(|| t.rev.as_ref().map(|r| format!("\"{r}\"")));
                        let mut s = self.shared.lock().unwrap();
                        need_pack = s.pack.as_ref().map(|p| p.pack_version != t.pack_version).unwrap_or(true);
                        s.tracker = Some(t);
                        s.version += 1;
                    }
                    Err(e) => {
                        logf!("tracker : JSON invalide ({e})");
                        self.set_status(Status::Offline);
                        return;
                    }
                }
            }
            Err(ureq::Error::Status(code, resp)) => {
                logf!("tracker : HTTP {code}");
                match code {
                    401 | 400 => {
                        self.invalid_key = true;
                        self.set_status(Status::InvalidKey);
                    }
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
                logf!("tracker : réseau ({e})");
                self.set_status(Status::Offline);
                return;
            }
        }
        if need_pack && !self.fetch_pack() {
            self.save_etags();
            self.set_status(Status::Offline);
            (self.notify)();
            return;
        }
        self.save_etags();
        self.set_status(Status::Ok);
        (self.notify)();
    }

    /// Télécharge le pack (version localisée si elle existe, sinon anglais). Renvoie false en cas d'échec réseau.
    fn fetch_pack(&mut self) -> bool {
        let mut urls = vec![];
        if self.locale != "en" {
            urls.push(format!("{}/api/overlay/v1/pack.{}.json", self.base, self.locale));
        }
        urls.push(format!("{}/api/overlay/v1/pack.json", self.base));
        for url in urls {
            let mut req = self.agent.get(&url).set("Accept", "application/json");
            if self.etags.pack_url.as_deref() == Some(url.as_str()) {
                if let Some(e) = &self.etags.pack {
                    req = req.set("If-None-Match", e);
                }
            }
            match req.call() {
                Ok(resp) if resp.status() == 304 => {
                    logf!("pack : 304 ({url})");
                    return true;
                }
                Ok(resp) => {
                    let etag = resp.header("ETag").map(|e| e.trim().to_string());
                    let body = resp.into_string().unwrap_or_default();
                    match serde_json::from_str::<Pack>(&body) {
                        Ok(p) => {
                            logf!("pack : 200 {} ({} quêtes, {} Ko)", p.pack_version, p.quests.len(), body.len() / 1024);
                            let _ = std::fs::write(Self::cache("pack.json"), &body);
                            self.etags.pack = etag;
                            self.etags.pack_url = Some(url);
                            let mut s = self.shared.lock().unwrap();
                            s.pack = Some(p);
                            s.version += 1;
                            return true;
                        }
                        Err(e) => {
                            logf!("pack : JSON invalide ({e})");
                            return false;
                        }
                    }
                }
                Err(ureq::Error::Status(404, _)) => {
                    logf!("pack : 404 ({url}), repli");
                    continue;
                }
                Err(e) => {
                    logf!("pack : erreur ({e})");
                    return false;
                }
            }
        }
        false
    }

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

    fn check_update(&mut self) {
        if now_secs().saturating_sub(self.etags.last_update_check) < 86_400 {
            return;
        }
        self.etags.last_update_check = now_secs();
        self.save_etags();
        let url = format!("{}/api/overlay/v1/app.json", self.base);
        let Ok(resp) = self.agent.get(&url).call() else { return };
        let Some(v) = resp.into_string().ok().and_then(|b| serde_json::from_str::<serde_json::Value>(&b).ok()) else { return };
        let latest = v["latest"].as_str().unwrap_or("").to_string();
        if version_gt(&latest, env!("CARGO_PKG_VERSION")) {
            let page = v["notes_url"].as_str().unwrap_or("https://hunger-companion.com/overlay").to_string();
            logf!("mise à jour disponible : {latest}");
            self.shared.lock().unwrap().update = Some((latest, page));
            (self.notify)();
        }
    }
}

fn version_gt(a: &str, b: &str) -> bool {
    let p = |s: &str| s.split('.').map(|x| x.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
    p(a) > p(b)
}
