//! Icône dans la zone de notification + menu contextuel.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, LoadIconW, PostMessageW, SetForegroundWindow, TrackPopupMenu,
    HICON, IDI_APPLICATION, MF_SEPARATOR, MF_STRING, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON, WM_NULL,
};

pub const ID_SETTINGS: usize = 101;
pub const ID_REFRESH: usize = 102;
pub const ID_UPDATE: usize = 103;
pub const ID_QUIT: usize = 104;
pub const ID_SEARCH: usize = 105;

fn copy(dst: &mut [u16], s: &str) {
    let v: Vec<u16> = s.encode_utf16().take(dst.len() - 1).collect();
    dst[..v.len()].copy_from_slice(&v);
    dst[v.len()] = 0;
}

pub fn app_icon() -> HICON {
    unsafe {
        let inst = GetModuleHandleW(None).unwrap_or_default();
        LoadIconW(inst, PCWSTR(1 as _)).or_else(|_| LoadIconW(None, IDI_APPLICATION)).unwrap_or_default()
    }
}

fn base(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: 1, ..Default::default() }
}

pub fn add(hwnd: HWND, msg: u32, tip: &str) {
    let mut nid = base(hwnd);
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = msg;
    nid.hIcon = app_icon();
    copy(&mut nid.szTip, tip);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    }
}

pub fn set_tip(hwnd: HWND, tip: &str) {
    let mut nid = base(hwnd);
    nid.uFlags = NIF_TIP;
    copy(&mut nid.szTip, tip);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

pub fn balloon(hwnd: HWND, title: &str, text: &str) {
    let mut nid = base(hwnd);
    nid.uFlags = NIF_INFO;
    nid.dwInfoFlags = NIIF_INFO;
    copy(&mut nid.szInfoTitle, title);
    copy(&mut nid.szInfo, text);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

pub fn remove(hwnd: HWND) {
    let nid = base(hwnd);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

/// Affiche le menu ; les choix arrivent en WM_COMMAND sur `hwnd`.
pub fn menu(hwnd: HWND, s: &crate::i18n::Strings, update: bool) {
    unsafe {
        let Ok(m) = CreatePopupMenu() else { return };
        let item = |id: usize, t: &str| {
            let w: Vec<u16> = t.encode_utf16().chain([0]).collect();
            let _ = AppendMenuW(m, MF_STRING, id, PCWSTR(w.as_ptr()));
        };
        item(ID_SETTINGS, s.menu_settings);
        item(ID_SEARCH, s.search_title);
        item(ID_REFRESH, s.menu_refresh);
        if update {
            item(ID_UPDATE, s.menu_update);
        }
        let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
        item(ID_QUIT, s.menu_quit);
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(m, TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, None);
        let _ = PostMessageW(hwnd, WM_NULL, windows::Win32::Foundation::WPARAM(0), windows::Win32::Foundation::LPARAM(0));
        let _ = DestroyMenu(m);
    }
}
