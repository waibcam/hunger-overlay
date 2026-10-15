//! Settings (%APPDATA%\HungerOverlay\config.json). The API key is encrypted with DPAPI (tied to the Windows user
//! session), so the file never contains the key in clear text and cannot be decrypted by another Windows account.
//! The only other system change is the optional "start with Windows" entry in the user's Run registry key.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
use windows::Win32::System::Registry::{RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ};

/// The only site the app talks to by default.
pub const DEFAULT_BASE_URL: &str = "https://hunger-companion.com";

/// User settings persisted as JSON. Missing fields fall back to `Default` (`#[serde(default)]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// DPAPI-encrypted API key, as hexadecimal (never stored in clear text).
    pub api_key_enc: Option<String>,
    /// Virtual-key code of the in-game map key (M = 0x4D by default).
    pub map_vk: u32,
    /// Also show completed objectives (greyed out).
    pub show_done: bool,
    /// Make the overlay visible in captures (OBS, screenshots).
    pub capture_visible: bool,
    /// Start the app with Windows. The Run registry entry is the reference: this value is re-read from it at
    /// every start (`autostart_enabled`), so the setting shown always matches what Windows will do.
    pub autostart: bool,
    /// Marker size factor (1.0 = normal).
    pub marker_scale: f32,
    /// Base URL of the site (can be edited by hand for development).
    pub base_url: String,
    /// Forced language ("en", "fr", "de"…). Empty = automatic: the site account's language, otherwise Windows'.
    pub locale: String,
    /// Minimum chance (in %) for a loot point to be shown (0 = everything the site sends).
    pub min_chance: u32,
    /// Quests hidden locally (quest_id).
    pub hidden_quests: Vec<String>,
    /// "Tracked quests" panel collapsed to its title bar.
    pub panel_collapsed: bool,
    /// Does the game zoom one step per wheel EVENT (fine-scrolling wheels) rather than per 120-unit
    /// notch? Learned automatically during recalibrations (None = not known yet).
    pub wheel_per_event: Option<bool>,
    /// Updates: installed automatically (otherwise just a notification).
    pub auto_update: bool,
    /// Theme of the settings window: "auto" (follows Windows), "light", "dark".
    pub theme: String,
    /// Layers ticked in the panel (id).
    pub layers_on: Vec<String>,
    /// Layers already seen: a new layer flagged `default_on` by the site is ticked ONCE, after that the user's
    /// choice prevails.
    pub layers_known: Vec<String>,
    /// Searched items shown on the map (item_key).
    pub search_items: Vec<String>,
    /// Panel position (fractions of the game window); None = to the right of the map.
    pub panel_pos: Option<(f32, f32)>,
    /// Panel background opacity, in % (20 to 100).
    pub panel_opacity: u32,
    /// Panel sections collapsed by the user ("quests", "crafting", "search", "layers").
    pub sections_collapsed: Vec<String>,
    /// The Quests layer has been switched on at least once (its "NEW" tag is then no longer shown).
    pub quests_layer_seen: bool,
    /// Badge slots of the tracked quests: slot i = badge i + 1, "" = free. A quest keeps its number while it is
    /// tracked, and a newly tracked quest takes the first free number (the website's list order can change).
    pub quest_slots: Vec<String>,
    /// Raid allies in adding order ("u_…" website, "l_…" local): the order gives their slot and colour.
    pub ally_order: Vec<String>,
    /// Allies hidden on the map.
    pub hidden_allies: Vec<String>,
    /// Ally quests hidden on the map ("<ally key>|<quest id>").
    pub hidden_ally_quests: Vec<String>,
    /// Every quest marker (mine and the allies') hidden on the map, to see only the layers.
    pub quest_markers_hidden: bool,
    /// Version that ran last (to say "updated to X" once after an update).
    pub last_version: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            api_key_enc: None,
            map_vk: 0x4D,
            show_done: false,
            capture_visible: false,
            autostart: false,
            marker_scale: 1.0,
            base_url: DEFAULT_BASE_URL.into(),
            locale: String::new(),
            // 10 % = same rule as the site's notebook (the pack sends points ≥ 1 %).
            min_chance: 10,
            hidden_quests: vec![],
            panel_collapsed: false,
            wheel_per_event: None,
            auto_update: true,
            theme: "auto".into(),
            layers_on: vec![],
            layers_known: vec![],
            search_items: vec![],
            panel_pos: None,
            panel_opacity: 85,
            sections_collapsed: vec![],
            quests_layer_seen: false,
            quest_slots: vec![],
            ally_order: vec![],
            hidden_allies: vec![],
            hidden_ally_quests: vec![],
            quest_markers_hidden: false,
            last_version: String::new(),
        }
    }
}

/// The app's data folder, %APPDATA%\HungerOverlay (created if missing): settings, cache, log, update staging.
pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let d = base.join("HungerOverlay");
    let _ = std::fs::create_dir_all(&d);
    d
}

impl Config {
    /// Updates `quest_slots` for the quests tracked now; returns true if it changed (to be saved).
    pub fn update_quest_slots(&mut self, tracked: &[&str]) -> bool {
        let before = self.quest_slots.clone();
        for s in self.quest_slots.iter_mut() {
            if !tracked.contains(&s.as_str()) {
                s.clear();
            }
        }
        for id in tracked {
            if !self.quest_slots.iter().any(|s| s == id) {
                match self.quest_slots.iter().position(|s| s.is_empty()) {
                    Some(i) => self.quest_slots[i] = id.to_string(),
                    None => self.quest_slots.push(id.to_string()),
                }
            }
        }
        while self.quest_slots.last().map(|s| s.is_empty()).unwrap_or(false) {
            self.quest_slots.pop();
        }
        self.quest_slots != before
    }

    /// Reads config.json; any missing or unreadable file yields the defaults.
    pub fn load() -> Config {
        std::fs::read_to_string(data_dir().join("config.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Writes config.json atomically (write to a .tmp file, then rename over the old one).
    /// Never during tests: they must not touch the user's real settings.
    pub fn save(&self) {
        if cfg!(test) {
            return;
        }
        let tmp = data_dir().join("config.json.tmp");
        if std::fs::write(&tmp, serde_json::to_string_pretty(self).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, data_dir().join("config.json"));
        }
    }

    /// Decrypts the stored API key with DPAPI (None if absent, empty or not decryptable by this Windows user).
    pub fn api_key(&self) -> Option<String> {
        let enc = hex_decode(self.api_key_enc.as_deref()?)?;
        let plain = dpapi(&enc, false)?;
        String::from_utf8(plain).ok().filter(|k| !k.is_empty())
    }

    /// Encrypts `key` with DPAPI and stores it as hex; an empty key removes it.
    pub fn set_api_key(&mut self, key: &str) {
        let key = key.trim();
        self.api_key_enc = if key.is_empty() { None } else { dpapi(key.as_bytes(), true).map(|b| hex_encode(&b)) };
    }
}

/// Windows DPAPI wrapper: encrypts (`protect` = true) or decrypts `data` for the current Windows user,
/// without any UI prompt. Returns None on failure.
fn dpapi(data: &[u8], protect: bool) -> Option<Vec<u8>> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        let ok = if protect {
            CryptProtectData(&input, w!("HungerOverlay"), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        } else {
            CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        };
        if ok.is_err() || out.pbData.is_null() {
            return None;
        }
        // Copy the result, then free the buffer that DPAPI allocated.
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(out.pbData as _));
        Some(v)
    }
}

/// Bytes → lowercase hex string.
fn hex_encode(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Hex string → bytes (None if malformed).
fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

/// True if the "HungerOverlay" value exists in the user's Run key (the app starts with Windows).
pub fn autostart_enabled() -> bool {
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"), w!("HungerOverlay"), RRF_RT_REG_SZ, None, None, None).is_ok()
    }
}

/// Start with Windows (the user's own Run key, no admin rights): adds or removes the "HungerOverlay" value
/// pointing to the current exe.
pub fn set_autostart(on: bool) {
    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .is_err()
        {
            return;
        }
        if on {
            if let Ok(exe) = std::env::current_exe() {
                let v: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain([0]).collect();
                let bytes = std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 2);
                let _ = RegSetValueExW(key, w!("HungerOverlay"), 0, REG_SZ, Some(bytes));
            }
        } else {
            let _ = RegDeleteValueW(key, w!("HungerOverlay"));
        }
        let _ = RegCloseKey(key);
    }
}
