//! Textes de l'interface (français ou anglais selon la langue de Windows).

use windows::Win32::Globalization::GetUserDefaultUILanguage;

pub struct Strings {
    pub settings_title: &'static str,
    pub key_label: &'static str,
    pub key_show: &'static str,
    pub map_key_label: &'static str,
    pub press_key: &'static str,
    pub show_done: &'static str,
    pub capture_visible: &'static str,
    pub autostart: &'static str,
    pub test_key: &'static str,
    pub save: &'static str,
    pub apply: &'static str,
    pub st_applied: &'static str,
    pub cancel: &'static str,
    pub help: &'static str,
    pub menu_settings: &'static str,
    pub menu_refresh: &'static str,
    pub menu_update: &'static str,
    pub menu_quit: &'static str,
    pub legend_title: &'static str,
    pub legend_none: &'static str,
    pub legend_elsewhere: &'static str,
    pub st_ok: &'static str,
    pub st_no_key: &'static str,
    pub st_invalid_key: &'static str,
    pub st_offline: &'static str,
    pub st_rate: &'static str,
    pub st_testing: &'static str,
    pub st_valid: &'static str,
    pub st_bad_format: &'static str,
    pub language: &'static str,
    pub language_auto: &'static str,
    pub min_chance: &'static str,
    pub already_running: &'static str,
    pub update_available: &'static str,
}

pub static FR: Strings = Strings {
    settings_title: "Hunger Overlay — Paramètres",
    key_label: "Clé d'API (à générer sur hunger-companion.com → Compte → Sécurité) :",
    key_show: "Afficher",
    map_key_label: "Touche de la carte en jeu :",
    press_key: "Appuyez sur une touche…",
    show_done: "Afficher les objectifs terminés (grisés)",
    capture_visible: "Visible dans les captures d'écran et OBS",
    autostart: "Lancer avec Windows",
    test_key: "Tester la clé",
    save: "Enregistrer",
    apply: "Appliquer",
    st_applied: "✔ Paramètres appliqués.",
    cancel: "Annuler",
    help: "Le jeu doit être en mode fenêtré plein écran (sans bordure). L'appli ne lit que l'écran : aucune lecture ni modification du jeu.",
    menu_settings: "Paramètres…",
    menu_refresh: "Rafraîchir le tracker",
    menu_update: "Télécharger la nouvelle version",
    menu_quit: "Quitter",
    legend_title: "QUÊTES SUIVIES",
    legend_none: "Aucune quête suivie. Suivez des quêtes sur hunger-companion.com/tracker (connecté).",
    legend_elsewhere: "sur une autre carte",
    st_ok: "à jour",
    st_no_key: "aucune clé d'API — clic droit sur l'icône → Paramètres",
    st_invalid_key: "clé invalide ou révoquée",
    st_offline: "hors ligne (données en cache)",
    st_rate: "trop de requêtes, nouvelle tentative plus tard",
    st_testing: "Test en cours…",
    st_valid: "Clé valide — compte :",
    language: "Langue :",
    language_auto: "Automatique (langue du compte sur le site)",
    min_chance: "Points de ramassage : chance minimum",
    st_bad_format: "✖ Ce n'est pas une clé : elle commence par « hco_ » et fait 36 caractères.",
    already_running: "Hunger Overlay est déjà lancé (icône près de l'horloge).",
    update_available: "Une nouvelle version de Hunger Overlay est disponible.",
};

pub static EN: Strings = Strings {
    settings_title: "Hunger Overlay — Settings",
    key_label: "API key (generate it on hunger-companion.com → Account → Security):",
    key_show: "Show",
    map_key_label: "In-game map key:",
    press_key: "Press a key…",
    show_done: "Show completed objectives (greyed out)",
    capture_visible: "Visible in screenshots and OBS",
    autostart: "Start with Windows",
    test_key: "Test key",
    save: "Save",
    apply: "Apply",
    st_applied: "✔ Settings applied.",
    cancel: "Cancel",
    help: "The game must run in borderless (windowed fullscreen) mode. The app only reads the screen: it never reads or modifies the game.",
    menu_settings: "Settings…",
    menu_refresh: "Refresh tracker",
    menu_update: "Download the new version",
    menu_quit: "Quit",
    legend_title: "TRACKED QUESTS",
    legend_none: "No tracked quest. Track quests on hunger-companion.com/tracker (signed in).",
    legend_elsewhere: "on another map",
    st_ok: "up to date",
    st_no_key: "no API key — right-click the tray icon → Settings",
    st_invalid_key: "invalid or revoked key",
    st_offline: "offline (cached data)",
    st_rate: "too many requests, will retry later",
    st_testing: "Testing…",
    st_valid: "Valid key — account:",
    language: "Language:",
    language_auto: "Automatic (your site account language)",
    min_chance: "Loot points: minimum chance",
    st_bad_format: "✖ Not a key: it starts with \"hco_\" and is 36 characters long.",
    already_running: "Hunger Overlay is already running (icon near the clock).",
    update_available: "A new version of Hunger Overlay is available.",
};

/// Langues du site (code, nom affiché).
pub static LANGS: [(&str, &str); 10] = [
    ("en", "English"),
    ("fr", "Français"),
    ("de", "Deutsch"),
    ("es", "Español"),
    ("it", "Italiano"),
    ("pt", "Português"),
    ("ru", "Русский"),
    ("ja", "日本語"),
    ("ko", "한국어"),
    ("zh", "中文"),
];

/// Textes de l'interface : français si la langue effective est le français, anglais sinon.
pub fn strings_for(lang: &str) -> &'static Strings {
    if lang.starts_with("fr") {
        &FR
    } else {
        &EN
    }
}

/// Langue effective : forcée, sinon celle du compte du site, sinon celle de Windows (anglais par défaut).
pub fn effective(forced: &str, account: Option<&str>) -> String {
    if !forced.is_empty() {
        return forced.to_lowercase();
    }
    if let Some(a) = account {
        let a = a.to_lowercase();
        let a = a.split(['-', '_']).next().unwrap_or("").to_string();
        if LANGS.iter().any(|(c, _)| *c == a) {
            return a;
        }
    }
    pack_locale("")
}

pub fn t() -> &'static Strings {
    strings_for(&pack_locale(""))
}

/// Locale du pack : réglage forcé, sinon langue de Windows (code ISO à 2 lettres).
pub fn pack_locale(forced: &str) -> String {
    if !forced.is_empty() {
        return forced.to_lowercase();
    }
    let primary = unsafe { GetUserDefaultUILanguage() } & 0x3ff;
    match primary {
        0x0c => "fr",
        0x07 => "de",
        0x0a => "es",
        0x10 => "it",
        0x16 => "pt",
        0x15 => "pl",
        0x19 => "ru",
        0x11 => "ja",
        0x12 => "ko",
        0x04 => "zh",
        _ => "en",
    }
    .into()
}
