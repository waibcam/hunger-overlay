//! Mise à jour automatique : téléchargement de la nouvelle version annoncée par `app.json` (lien direct vers l'exe
//! de la release GitHub), vérification de l'empreinte SHA-256, remplacement de l'exe en cours et redémarrage.
//! Rien n'est remplacé si l'empreinte ne correspond pas.

use std::io::Read;
use std::path::{Path, PathBuf};
use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};

const MAX_SIZE: u64 = 64 * 1024 * 1024;

/// Empreinte SHA-256 en hexadécimal minuscule (CNG Windows, sans dépendance).
pub fn sha256_hex(data: &[u8]) -> Option<String> {
    let mut out = [0u8; 32];
    let st = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) };
    if st.is_err() {
        return None;
    }
    Some(out.iter().map(|b| format!("{b:02x}")).collect())
}

fn update_dir() -> PathBuf {
    let d = crate::config::data_dir().join("update");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Télécharge et vérifie l'exe. Renvoie le chemin du fichier prêt à installer.
pub fn download(agent: &ureq::Agent, url: &str, sha256: &str, version: &str) -> Result<PathBuf, String> {
    let resp = agent.get(url).call().map_err(|e| format!("téléchargement : {}", match e {
        ureq::Error::Status(c, _) => format!("HTTP {c}"),
        ureq::Error::Transport(t) => format!("{:?}", t.kind()),
    }))?;
    let mut data = Vec::new();
    resp.into_reader().take(MAX_SIZE).read_to_end(&mut data).map_err(|e| format!("lecture : {e}"))?;
    if data.len() < 100_000 || &data[..2] != b"MZ" {
        return Err(format!("fichier inattendu ({} octets)", data.len()));
    }
    let got = sha256_hex(&data).ok_or("calcul SHA-256 impossible")?;
    if !got.eq_ignore_ascii_case(sha256.trim()) {
        return Err(format!("empreinte SHA-256 différente (attendu {sha256}, obtenu {got})"));
    }
    let path = update_dir().join(format!("HungerOverlay-{version}.exe"));
    std::fs::write(&path, &data).map_err(|e| format!("écriture : {e}"))?;
    Ok(path)
}

/// Remplace l'exe en cours par `new_exe` et lance la nouvelle version. L'appelant doit quitter ensuite.
pub fn install_and_restart(new_exe: &Path) -> Result<(), String> {
    let cur = std::env::current_exe().map_err(|e| e.to_string())?;
    let old = cur.with_extension("exe.old");
    let _ = std::fs::remove_file(&old);
    // Windows autorise à renommer un exe en cours d'exécution (pas à l'écraser).
    std::fs::rename(&cur, &old).map_err(|e| format!("renommage de l'exe : {e}"))?;
    if let Err(e) = std::fs::copy(new_exe, &cur) {
        let _ = std::fs::rename(&old, &cur); // retour arrière
        return Err(format!("copie de la nouvelle version : {e}"));
    }
    crate::release_single_instance();
    std::process::Command::new(&cur).arg("--updated").spawn().map_err(|e| format!("redémarrage : {e}"))?;
    Ok(())
}

/// Au démarrage : supprime l'ancienne version et les téléchargements devenus inutiles.
pub fn cleanup() {
    if let Ok(cur) = std::env::current_exe() {
        let _ = std::fs::remove_file(cur.with_extension("exe.old"));
    }
    if let Ok(rd) = std::fs::read_dir(update_dir()) {
        for e in rd.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
}
