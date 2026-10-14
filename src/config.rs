//! Réglages (%APPDATA%\HungerOverlay\config.json). La clé d'API est chiffrée avec DPAPI (liée à la session Windows).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
use windows::Win32::System::Registry::{RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ};

pub const DEFAULT_BASE_URL: &str = "https://hunger-companion.com";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Clé chiffrée DPAPI, en hexadécimal.
    pub api_key_enc: Option<String>,
    /// Code de touche virtuelle de la carte (M = 0x4D par défaut).
    pub map_vk: u32,
    pub show_done: bool,
    /// Rendre l'overlay visible dans les captures (OBS, captures d'écran).
    pub capture_visible: bool,
    pub autostart: bool,
    /// Facteur de taille des marqueurs (1.0 = normal).
    pub marker_scale: f32,
    /// URL de base du site (modifiable à la main pour le développement).
    pub base_url: String,
    /// Langue forcée ("en", "fr", "de"…). Vide = automatique : langue du compte sur le site, sinon de Windows.
    pub locale: String,
    /// Chance minimum (en %) pour afficher un point de ramassage (0 = tout ce que le site envoie).
    pub min_chance: u32,
    /// Quêtes masquées localement (quest_id).
    pub hidden_quests: Vec<String>,
    /// Panneau « Quêtes suivies » réduit à sa barre de titre.
    pub panel_collapsed: bool,
    /// Le jeu zoome-t-il d'un cran par ÉVÉNEMENT de molette (molettes à défilement fin) plutôt que par
    /// tranche de 120 ? Appris automatiquement par les recalages (None = pas encore su).
    pub wheel_per_event: Option<bool>,
    /// Mises à jour : installées automatiquement (sinon simple notification).
    pub auto_update: bool,
    /// Thème de la fenêtre de paramètres : "auto" (celui de Windows), "light", "dark".
    pub theme: String,
    /// Calques cochés dans le panneau (id).
    pub layers_on: Vec<String>,
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
            // 10 % = même règle que le carnet du site (le pack envoie les points ≥ 1 %).
            min_chance: 10,
            hidden_quests: vec![],
            panel_collapsed: false,
            wheel_per_event: None,
            auto_update: true,
            theme: "auto".into(),
            layers_on: vec![],
        }
    }
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let d = base.join("HungerOverlay");
    let _ = std::fs::create_dir_all(&d);
    d
}

impl Config {
    pub fn load() -> Config {
        std::fs::read_to_string(data_dir().join("config.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let tmp = data_dir().join("config.json.tmp");
        if std::fs::write(&tmp, serde_json::to_string_pretty(self).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, data_dir().join("config.json"));
        }
    }

    pub fn api_key(&self) -> Option<String> {
        let enc = hex_decode(self.api_key_enc.as_deref()?)?;
        let plain = dpapi(&enc, false)?;
        String::from_utf8(plain).ok().filter(|k| !k.is_empty())
    }

    pub fn set_api_key(&mut self, key: &str) {
        let key = key.trim();
        self.api_key_enc = if key.is_empty() { None } else { dpapi(key.as_bytes(), true).map(|b| hex_encode(&b)) };
    }
}

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
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(out.pbData as _));
        Some(v)
    }
}

fn hex_encode(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

/// Démarrage avec Windows (clé Run de l'utilisateur, aucun droit admin).
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
