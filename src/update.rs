//! Automatic update: downloads the new version announced by `app.json` (a direct link to the exe of the GitHub
//! release), verifies its SHA-256 fingerprint, replaces the running exe and restarts.
//! Nothing is replaced if the fingerprint does not match.
//!
//! What goes over the network here: one plain HTTPS GET to the exe URL taken from `app.json` (which itself is
//! fetched from hunger-companion.com, see `net.rs`). No API key, no user data and no custom headers are sent
//! with the download. The expected SHA-256 also comes from `app.json`; the downloaded bytes are hashed locally
//! with Windows CNG and the file is written to disk only if the hash matches exactly.

use std::io::Read;
use std::path::{Path, PathBuf};
use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};

/// Hard cap on the download size (64 MB): anything beyond is not read.
const MAX_SIZE: u64 = 64 * 1024 * 1024;

/// SHA-256 fingerprint as lowercase hexadecimal (Windows CNG, no extra dependency).
pub fn sha256_hex(data: &[u8]) -> Option<String> {
    let mut out = [0u8; 32];
    let st = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) };
    if st.is_err() {
        return None;
    }
    Some(out.iter().map(|b| format!("{b:02x}")).collect())
}

/// Staging folder for downloaded updates: %APPDATA%\HungerOverlay\update (created if missing).
fn update_dir() -> PathBuf {
    let d = crate::config::data_dir().join("update");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Downloads and verifies the exe. Returns the path of the file ready to install.
/// Checks, in order: HTTP success, size ≤ 64 MB, looks like a Windows executable (≥ 100 KB, starts with "MZ"),
/// and SHA-256 equal to `sha256`. The file is saved only after all checks pass; it is not run here.
pub fn download(agent: &ureq::Agent, url: &str, sha256: &str, version: &str) -> Result<PathBuf, String> {
    // Plain GET, no Authorization header: the API key is never sent to the download host.
    let resp = agent.get(url).call().map_err(|e| format!("download: {}", match e {
        ureq::Error::Status(c, _) => format!("HTTP {c}"),
        ureq::Error::Transport(t) => format!("{:?}", t.kind()),
    }))?;
    let mut data = Vec::new();
    resp.into_reader().take(MAX_SIZE).read_to_end(&mut data).map_err(|e| format!("read: {e}"))?;
    if data.len() < 100_000 || &data[..2] != b"MZ" {
        return Err(format!("unexpected file ({} bytes)", data.len()));
    }
    // Integrity check: the hash of the downloaded bytes must match the one published in app.json.
    let got = sha256_hex(&data).ok_or("SHA-256 computation failed")?;
    if !got.eq_ignore_ascii_case(sha256.trim()) {
        return Err(format!("SHA-256 mismatch (expected {sha256}, got {got})"));
    }
    let path = update_dir().join(format!("HungerOverlay-{version}.exe"));
    std::fs::write(&path, &data).map_err(|e| format!("write: {e}"))?;
    Ok(path)
}

/// Replaces the running exe with `new_exe` (already verified by `download`) and launches the new version.
/// The caller must exit afterwards. The previous exe is kept as `.exe.old` and deleted at the next start.
pub fn install_and_restart(new_exe: &Path) -> Result<(), String> {
    let cur = std::env::current_exe().map_err(|e| e.to_string())?;
    let old = cur.with_extension("exe.old");
    let _ = std::fs::remove_file(&old);
    // Windows allows renaming a running exe (but not overwriting it).
    std::fs::rename(&cur, &old).map_err(|e| format!("renaming the exe: {e}"))?;
    if let Err(e) = std::fs::copy(new_exe, &cur) {
        let _ = std::fs::rename(&old, &cur); // roll back
        return Err(format!("copying the new version: {e}"));
    }
    // Free the single-instance lock so the new process can start, then launch it with `--updated`.
    crate::release_single_instance();
    std::process::Command::new(&cur).arg("--updated").spawn().map_err(|e| format!("restart: {e}"))?;
    Ok(())
}

/// At startup: deletes the previous version and the downloads that are no longer needed.
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
