//! User-interface texts (French or English depending on the Windows language).

use windows::Win32::Globalization::GetUserDefaultUILanguage;

/// Every user-facing UI string, one field per text (prefixes: `st_` = status line, `tip_` = marker tooltip,
/// `sec_` = settings section, `menu_` = tray menu, `src_` = loot source, `panel_`/`legend_` = in-game panel).
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
    pub legend_filtered: &'static str,
    pub tip_container: &'static str,
    pub tip_chance: &'static str,
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
    pub panel_done: &'static str,
    pub panel_layers: &'static str,
    pub harvest_layer: &'static str,
    pub quests_layer: &'static str,
    pub tip_track_hint: &'static str,
    pub panel_opacity: &'static str,
    pub panel_search: &'static str,
    pub search_title: &'static str,
    pub search_help: &'static str,
    pub search_loading: &'static str,
    pub search_none: &'static str,
    pub src_container: &'static str,
    pub src_harvest: &'static str,
    pub src_placed: &'static str,
    pub src_points: &'static str,
    pub tip_placed: &'static str,
    pub quest_tracked: &'static str,
    pub quest_track: &'static str,
    pub track_unsupported: &'static str,
    pub track_readonly: &'static str,
    pub track_failed: &'static str,
    pub tip_harvest: &'static str,
    pub tip_level: &'static str,
    pub tip_you: &'static str,
    pub sec_account: &'static str,
    pub sec_map: &'static str,
    pub sec_display: &'static str,
    pub sec_app: &'static str,
    pub key_title: &'static str,
    pub key_help: &'static str,
    pub key_hide: &'static str,
    pub map_key_title: &'static str,
    pub map_key_help: &'static str,
    pub theme: &'static str,
    pub theme_auto: &'static str,
    pub theme_light: &'static str,
    pub theme_dark: &'static str,
    pub panel_chance: &'static str,
    pub panel_setup: &'static str,
    pub panel_hidden: &'static str,
    pub already_running: &'static str,
    pub update_available: &'static str,
    pub auto_update: &'static str,
    pub updated_to: &'static str,
    /// Search field placeholder.
    pub p_find: &'static str,
    /// Markers of an owner on this map ("{n}").
    pub p_spots: &'static str,
    /// Start of the one-line list of tracked quests with nothing on this map.
    pub p_other_map: &'static str,
    /// Sub-header of the untracked quests of the Quests layer.
    pub p_avail: &'static str,
    /// Right-aligned hint of that sub-header.
    pub p_avail_hint: &'static str,
    /// Button that starts tracking a quest.
    pub p_track: &'static str,
    /// Expands a list ("{n}").
    pub p_more: &'static str,
    /// Collapses a list.
    pub p_less: &'static str,
    /// Every marker of an owner is below the chance filter ("{n}" %).
    pub p_filter_warn: &'static str,
    /// Lowers the chance filter ("{n}" %).
    pub p_reveal: &'static str,
    /// Removes every item search.
    pub p_clear_all: &'static str,
    /// Number of layers switched on ("{n}").
    pub p_layers_on: &'static str,
    /// Layer group.
    pub g_survival: &'static str,
    /// Layer group.
    pub g_loot: &'static str,
    /// Layer group.
    pub g_world: &'static str,
    /// Tag of something new.
    pub p_new: &'static str,
    /// Toast after tracking a quest (followed by its title).
    pub toast_tracking: &'static str,
    /// Toast link that reverts the last tracking.
    pub toast_undo: &'static str,
    /// Toast after Undo (followed by the quest title).
    pub toast_undone: &'static str,
    /// Nothing tracked.
    pub p_empty: &'static str,
    /// Explanation under it.
    pub p_empty_hint: &'static str,
    /// Button that turns the Quests layer on.
    pub p_empty_btn: &'static str,
    /// The map position is being recomputed.
    pub p_relocating: &'static str,
    /// Collapsed panel summary ("{q}" quests, "{l}" layers).
    pub p_summary: &'static str,
    /// Hint that the panel can be scrolled.
    pub p_scroll: &'static str,
    /// Panel section.
    pub p_sec_quests: &'static str,
    /// Panel section.
    pub p_sec_crafting: &'static str,
    /// Panel section.
    pub p_sec_search: &'static str,
    /// Panel section.
    pub p_sec_layers: &'static str,
    /// Tooltip eyebrow: a container.
    pub k_container: &'static str,
    /// Tooltip eyebrow: an objective spot.
    pub k_objective: &'static str,
    /// Tooltip eyebrow: a harvest node.
    pub k_harvest: &'static str,
    /// Tooltip eyebrow: a placed object.
    pub k_placed: &'static str,
    /// Tooltip eyebrow: a done objective.
    pub k_done: &'static str,
    /// Tooltip eyebrow: an untracked quest.
    pub k_quest_avail: &'static str,
    /// Tooltip eyebrow of a cluster ("{n}").
    pub k_spots: &'static str,
    /// Tooltip footer of a cluster.
    pub t_zoom: &'static str,
    /// More entries than listed ("{n}").
    pub t_more: &'static str,
    /// More markers under the cursor ("{n}").
    pub t_more_here: &'static str,
    /// Tooltip key.
    pub t_giver: &'static str,
    /// Tooltip key.
    pub t_profession: &'static str,
    /// Tooltip key.
    pub t_level: &'static str,
    /// Placed object.
    pub t_always: &'static str,
    /// Tooltip key: objectives of a quest on this map.
    pub t_on_map: &'static str,
    /// Chance bar label.
    pub t_chance: &'static str,
    /// Tooltip key.
    pub t_objective: &'static str,
    /// After the "Ctrl" keycap.
    pub t_track_footer: &'static str,
    /// Tooltip key: height of a spot above the ground.
    pub t_height: &'static str,
    /// Spot more than 8 m below the ground.
    pub h_deep: &'static str,
    /// Spot 2 to 8 m below the ground.
    pub h_below: &'static str,
    /// Spot within 2 m of the ground.
    pub h_ground: &'static str,
    /// Spot 2 to 8 m above the ground.
    pub h_upper: &'static str,
    /// Spot more than 8 m above the ground.
    pub h_high: &'static str,
}

/// French UI texts.
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
    menu_update: "Installer la nouvelle version",
    menu_quit: "Quitter",
    legend_title: "QUÊTES SUIVIES",
    legend_none: "Aucune quête suivie. Suivez des quêtes sur hunger-companion.com/tracker (connecté).",
    legend_elsewhere: "sur une autre carte",
    legend_filtered: "points sous le seuil de chance (baissez « Chance ≥ »)",
    tip_container: "Conteneur",
    tip_chance: "de chance",
    st_ok: "à jour",
    st_no_key: "Aucune clé d'API : générez-en une sur hunger-companion.com (Compte → Sécurité).",
    st_invalid_key: "La clé d'API ne fonctionne plus (invalide ou révoquée) : veuillez la vérifier.",
    st_offline: "hors ligne (données en cache)",
    st_rate: "trop de requêtes, nouvelle tentative plus tard",
    st_testing: "Test en cours…",
    st_valid: "Clé valide — compte :",
    language: "Langue :",
    language_auto: "Automatique (langue du compte sur le site)",
    min_chance: "Points de ramassage : chance minimum",
    panel_done: "Terminés",
    panel_layers: "CALQUES",
    harvest_layer: "Récolte · vos niveaux",
    quests_layer: "Quêtes",
    tip_track_hint: "Ctrl + clic : suivre cette quête",
    panel_opacity: "Opacité du panneau",
    panel_search: "🔍 Chercher un objet…",
    search_title: "Chercher un objet",
    search_help: "Tapez un nom, puis ↑ ↓ et Entrée (ou un clic).",
    search_loading: "Chargement de la liste des objets…",
    search_none: "Aucun objet trouvé.",
    src_container: "conteneurs",
    src_harvest: "récolte",
    src_placed: "posé",
    src_points: "pts",
    tip_placed: "Loot posé",
    quest_tracked: "suivie (choisir pour arrêter)",
    quest_track: "quête à suivre",
    track_unsupported: "Le site ne permet pas encore d'activer une quête depuis l'appli.",
    track_readonly: "Cette clé d'API est en lecture seule. Cliquez ici, puis « Autoriser la modification du suivi » sur le site.",
    track_failed: "Impossible de modifier le suivi de la quête (réseau).",
    tip_harvest: "Récolte",
    tip_level: "niveau requis",
    tip_you: "vous",
    sec_account: "Compte",
    sec_map: "Carte",
    sec_display: "Affichage",
    sec_app: "Application",
    key_title: "Clé d'API",
    key_help: "À générer sur hunger-companion.com → Compte → Sécurité",
    key_hide: "Masquer",
    map_key_title: "Touche de la carte",
    map_key_help: "Facultatif : la carte est reconnue à l'écran, la touche accélère seulement la détection.",
    theme: "Thème",
    theme_auto: "Auto",
    theme_light: "Clair",
    theme_dark: "Sombre",
    panel_chance: "Chance ≥",
    panel_setup: "Configurer la clé…",
    panel_hidden: "masquée",
    st_bad_format: "✖ Ce n'est pas une clé : elle commence par « hco_ » et fait 36 caractères.",
    already_running: "Hunger Overlay est déjà lancé (icône près de l'horloge).",
    update_available: "Une nouvelle version de Hunger Overlay est disponible.",
    auto_update: "Mises à jour automatiques",
    updated_to: "Hunger Overlay a été mis à jour en version",
    p_find: "Chercher un objet ou une quête…",
    p_spots: "{n} points",
    p_other_map: "Autre carte :",
    p_avail: "DISPONIBLES SUR CETTE CARTE",
    p_avail_hint: "calque Quêtes",
    p_track: "+ Suivre",
    p_more: "Afficher {n} de plus",
    p_less: "Réduire",
    p_filter_warn: "Seulement sous votre filtre de {n} % ici",
    p_reveal: "Afficher ≥ {n} %",
    p_clear_all: "Tout effacer",
    p_layers_on: "{n} actifs",
    g_survival: "SURVIE",
    g_loot: "BUTIN & ACCÈS",
    g_world: "MONDE",
    p_new: "NOUVEAU",
    toast_tracking: "Quête suivie :",
    toast_undo: "Annuler",
    toast_undone: "Suivi annulé :",
    p_empty: "Aucune quête suivie pour l'instant.",
    p_empty_hint: "Suivez des quêtes sur hunger-companion.com, ou activez le calque Quêtes pour les découvrir sur cette carte.",
    p_empty_btn: "Afficher les quêtes de cette carte",
    p_relocating: "Recalage de la carte… repères masqués",
    p_summary: "{q} quêtes · {l} calques",
    p_scroll: "Suite plus bas · molette",
    p_sec_quests: "QUÊTES",
    p_sec_crafting: "ARTISANAT",
    p_sec_search: "RECHERCHE",
    p_sec_layers: "CALQUES DE LA CARTE",
    k_container: "CONTENEUR",
    k_objective: "OBJECTIF",
    k_harvest: "RÉCOLTE",
    k_placed: "POSÉ",
    k_done: "FAIT",
    k_quest_avail: "QUÊTE · NON SUIVIE",
    k_spots: "{n} POINTS",
    t_zoom: "Zoomez pour les séparer",
    t_more: "+{n} autres",
    t_more_here: "+{n} autres ici",
    t_giver: "Donneur",
    t_profession: "Métier",
    t_level: "Niveau",
    t_always: "Toujours à cet endroit",
    t_on_map: "Sur cette carte",
    t_chance: "Chance",
    t_objective: "Objectif",
    t_track_footer: "+ clic : suivre cette quête",
    t_height: "Hauteur",
    h_deep: "profondément sous terre",
    h_below: "sous terre, cave",
    h_ground: "au sol",
    h_upper: "à l'étage",
    h_high: "en hauteur (tour, toit)",
};

/// English UI texts (used for every language other than French).
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
    menu_update: "Install the new version",
    menu_quit: "Quit",
    legend_title: "TRACKED QUESTS",
    legend_none: "No tracked quest. Track quests on hunger-companion.com/tracker (signed in).",
    legend_elsewhere: "on another map",
    legend_filtered: "points below the chance threshold (lower \"Chance ≥\")",
    tip_container: "Container",
    tip_chance: "chance",
    st_ok: "up to date",
    st_no_key: "No API key: generate one on hunger-companion.com (Account → Security).",
    st_invalid_key: "The API key no longer works (invalid or revoked): please check it.",
    st_offline: "offline (cached data)",
    st_rate: "too many requests, will retry later",
    st_testing: "Testing…",
    st_valid: "Valid key — account:",
    language: "Language:",
    language_auto: "Automatic (your site account language)",
    min_chance: "Loot points: minimum chance",
    panel_done: "Completed",
    panel_layers: "LAYERS",
    harvest_layer: "Harvest · your levels",
    quests_layer: "Quests",
    tip_track_hint: "Ctrl + click: track this quest",
    panel_opacity: "Panel opacity",
    panel_search: "🔍 Find an item…",
    search_title: "Find an item",
    search_help: "Type a name, then ↑ ↓ and Enter (or click).",
    search_loading: "Loading the item list…",
    search_none: "No item found.",
    src_container: "containers",
    src_harvest: "harvest",
    src_placed: "placed",
    src_points: "pts",
    tip_placed: "Placed loot",
    quest_tracked: "tracked (pick to stop)",
    quest_track: "quest to track",
    track_unsupported: "The site does not allow tracking a quest from the app yet.",
    track_readonly: "This API key is read-only. Click here, then \"Allow tracking changes\" on the site.",
    track_failed: "Could not change the quest tracking (network).",
    tip_harvest: "Harvest",
    tip_level: "required level",
    tip_you: "you",
    sec_account: "Account",
    sec_map: "Map",
    sec_display: "Display",
    sec_app: "Application",
    key_title: "API key",
    key_help: "Generate it on hunger-companion.com → Account → Security",
    key_hide: "Hide",
    map_key_title: "Map key",
    map_key_help: "Optional: the map is recognised on screen, the key only speeds up detection.",
    theme: "Theme",
    theme_auto: "Auto",
    theme_light: "Light",
    theme_dark: "Dark",
    panel_chance: "Chance ≥",
    panel_setup: "Set up the key…",
    panel_hidden: "hidden",
    st_bad_format: "✖ Not a key: it starts with \"hco_\" and is 36 characters long.",
    already_running: "Hunger Overlay is already running (icon near the clock).",
    update_available: "A new version of Hunger Overlay is available.",
    auto_update: "Automatic updates",
    updated_to: "Hunger Overlay has been updated to version",
    p_find: "Find an item or quest…",
    p_spots: "{n} spots",
    p_other_map: "Other map:",
    p_avail: "AVAILABLE ON THIS MAP",
    p_avail_hint: "Quests layer",
    p_track: "+ Track",
    p_more: "Show {n} more",
    p_less: "Show less",
    p_filter_warn: "Only below your {n} % filter here",
    p_reveal: "Show ≥ {n} %",
    p_clear_all: "Clear all",
    p_layers_on: "{n} on",
    g_survival: "SURVIVAL",
    g_loot: "LOOT & ACCESS",
    g_world: "WORLD",
    p_new: "NEW",
    toast_tracking: "Now tracking",
    toast_undo: "Undo",
    toast_undone: "No longer tracking",
    p_empty: "No quest tracked yet.",
    p_empty_hint: "Track quests on hunger-companion.com, or turn on the Quests layer to discover them on this map.",
    p_empty_btn: "Show quests on this map",
    p_relocating: "Re-locating the map… markers hidden",
    p_summary: "{q} quests · {l} layers",
    p_scroll: "More below · scroll",
    p_sec_quests: "QUESTS",
    p_sec_crafting: "CRAFTING",
    p_sec_search: "SEARCH",
    p_sec_layers: "MAP LAYERS",
    k_container: "CONTAINER",
    k_objective: "OBJECTIVE",
    k_harvest: "HARVEST",
    k_placed: "PLACED",
    k_done: "DONE",
    k_quest_avail: "QUEST · NOT TRACKED",
    k_spots: "{n} SPOTS",
    t_zoom: "Zoom in to separate",
    t_more: "+{n} more",
    t_more_here: "+{n} more here",
    t_giver: "Giver",
    t_profession: "Profession",
    t_level: "Level",
    t_always: "Always at this spot",
    t_on_map: "On this map",
    t_chance: "Chance",
    t_objective: "Objective",
    t_track_footer: "+ click to track this quest",
    t_height: "Height",
    h_deep: "deep underground",
    h_below: "underground, cellar",
    h_ground: "ground level",
    h_upper: "upstairs",
    h_high: "high up (tower, roof)",
};

/// Languages offered by the site (code, display name).
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

/// UI texts: French if the effective language is French, English otherwise.
pub fn strings_for(lang: &str) -> &'static Strings {
    if lang.starts_with("fr") {
        &FR
    } else {
        &EN
    }
}

/// Effective language: the one chosen in the settings, otherwise ("Auto") the Windows display language when the
/// app offers it, otherwise English. The website account's language is not used.
pub fn effective(forced: &str) -> String {
    if !forced.is_empty() {
        return forced.to_lowercase();
    }
    let w = pack_locale("");
    if LANGS.iter().any(|(c, _)| *c == w) {
        w
    } else {
        "en".into()
    }
}

/// Label of the "Auto" choice, with the language it currently resolves to, e.g. "Auto (FR)".
pub fn auto_label() -> String {
    format!("Auto ({})", effective("").to_uppercase())
}

/// Display name of a language code ("" = the "Auto" label).
pub fn lang_name(code: &str) -> String {
    if code.is_empty() {
        return auto_label();
    }
    LANGS.iter().find(|l| l.0 == code).map(|l| l.1.to_string()).unwrap_or_else(|| code.to_uppercase())
}

/// UI texts for a language setting ("" = Auto).
pub fn t_for(forced: &str) -> &'static Strings {
    strings_for(&effective(forced))
}

/// Pack locale: the forced setting, otherwise the Windows UI language (2-letter ISO code).
pub fn pack_locale(forced: &str) -> String {
    if !forced.is_empty() {
        return forced.to_lowercase();
    }
    // Low 10 bits of the LANGID = primary language identifier.
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
