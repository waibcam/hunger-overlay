//! Fenêtre de paramètres (Win32 natif, sans dépendance).

use crate::i18n::Strings;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateFontW, DeleteObject, GetSysColorBrush, COLOR_3DFACE, HFONT, HGDIOBJ};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::EM_SETPASSWORDCHAR;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyNameTextW, MapVirtualKeyW, MAPVK_VK_TO_VSC};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowTextLengthW, GetWindowTextW, RegisterClassW, SendMessageW,
    SetForegroundWindow, SetWindowTextW, ShowWindow, BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_PUSHBUTTON,
    CW_USEDEFAULT, ES_AUTOHSCROLL, ES_PASSWORD, HMENU, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_CLOSE, WM_COMMAND, WM_SETFONT, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_MINIMIZEBOX,
    WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};

pub const ID_KEY_EDIT: i32 = 201;
pub const ID_KEY_SHOW: i32 = 202;
pub const ID_MAPKEY: i32 = 203;
pub const ID_SHOW_DONE: i32 = 204;
pub const ID_CAPTURE: i32 = 205;
pub const ID_AUTOSTART: i32 = 206;
pub const ID_TEST: i32 = 207;
pub const ID_SAVE: i32 = 208;
pub const ID_CANCEL: i32 = 209;
pub const ID_LANG: i32 = 210;
pub const ID_CHANCE: i32 = 211;
pub const ID_APPLY: i32 = 212;

const TBM_GETPOS: u32 = 0x0400;
const TBM_SETPOS: u32 = 0x0405;
const TBM_SETRANGE: u32 = 0x0406;
const TBM_SETTICFREQ: u32 = 0x0414;
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_SETCURSEL: u32 = 0x014E;

pub struct Settings {
    pub hwnd: HWND,
    edit: HWND,
    mapkey: HWND,
    show_done: HWND,
    capture: HWND,
    autostart: HWND,
    status: HWND,
    lang: HWND,
    chance: HWND,
    chance_label: HWND,
    font: HFONT,
    masked: bool,
    pub vk: u32,
    chance_text: &'static str,
}

/// Valeurs initiales de la fenêtre.
pub struct Init<'a> {
    pub key: &'a str,
    pub vk: u32,
    pub show_done: bool,
    pub capture: bool,
    pub autostart: bool,
    pub locale: &'a str,
    pub min_chance: u32,
}

pub struct Values {
    pub key: String,
    pub vk: u32,
    pub show_done: bool,
    pub capture_visible: bool,
    pub autostart: bool,
    pub locale: String,
    pub min_chance: u32,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

pub fn key_name(vk: u32) -> String {
    unsafe {
        let mut sc = MapVirtualKeyW(vk, MAPVK_VK_TO_VSC);
        // Touches étendues (flèches, Inser, Suppr…) : bit 24.
        if matches!(vk, 0x21..=0x2E) {
            sc |= 0x100;
        }
        let mut buf = [0u16; 64];
        let n = GetKeyNameTextW((sc as i32) << 16, &mut buf);
        if n > 0 {
            String::from_utf16_lossy(&buf[..n as usize])
        } else {
            format!("0x{vk:02X}")
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            crate::on_settings_command((wp.0 & 0xffff) as i32);
            LRESULT(0)
        }
        WM_CLOSE => {
            crate::on_settings_command(ID_CANCEL);
            LRESULT(0)
        }
        // Curseur déplacé (WM_HSCROLL).
        0x0114 => {
            crate::on_settings_command(ID_CHANCE);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

impl Settings {
    pub fn open(s: &'static Strings, init: Init) -> Option<Settings> {
        let Init { key, vk, show_done, capture, autostart, locale, min_chance } = init;
        unsafe {
            let icc = windows::Win32::UI::Controls::INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<windows::Win32::UI::Controls::INITCOMMONCONTROLSEX>() as u32,
                dwICC: windows::Win32::UI::Controls::ICC_BAR_CLASSES,
            };
            let _ = windows::Win32::UI::Controls::InitCommonControlsEx(&icc);
            let inst = GetModuleHandleW(None).ok()?;
            let class = w!("HungerOverlaySettings");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: inst.into(),
                lpszClassName: class,
                hbrBackground: GetSysColorBrush(COLOR_3DFACE),
                hIcon: crate::tray::app_icon(),
                ..Default::default()
            };
            RegisterClassW(&wc);
            let k = GetDpiForSystem() as f32 / 96.0;
            let px = |v: f32| (v * k) as i32;
            let (ww, wh) = (px(520.0), px(520.0));
            let title = wide(s.settings_title);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                PCWSTR(title.as_ptr()),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                ww,
                wh,
                None,
                None,
                inst,
                None,
            )
            .ok()?;
            let font = CreateFontW(-px(15.0), 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 5, 0, w!("Segoe UI"));
            let mk = |cls: PCWSTR, text: &str, style: u32, ex: u32, x: f32, y: f32, w: f32, h: f32, id: i32| -> HWND {
                let t = wide(text);
                let c = CreateWindowExW(
                    WINDOW_EX_STYLE(ex),
                    cls,
                    PCWSTR(t.as_ptr()),
                    WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style),
                    px(x),
                    px(y),
                    px(w),
                    px(h),
                    hwnd,
                    HMENU(id as isize as _),
                    inst,
                    None,
                )
                .unwrap_or_default();
                SendMessageW(c, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
                c
            };
            let st = w!("STATIC");
            let bt = w!("BUTTON");
            mk(st, s.key_label, 0, 0, 16.0, 14.0, 480.0, 22.0, 0);
            let edit = mk(w!("EDIT"), key, (ES_PASSWORD | ES_AUTOHSCROLL) as u32 | WS_TABSTOP.0, WS_EX_CLIENTEDGE.0, 16.0, 40.0, 380.0, 26.0, ID_KEY_EDIT);
            mk(bt, s.key_show, BS_PUSHBUTTON as u32 | WS_TABSTOP.0, 0, 404.0, 40.0, 92.0, 26.0, ID_KEY_SHOW);
            mk(st, s.map_key_label, 0, 0, 16.0, 86.0, 220.0, 22.0, 0);
            let mapkey = mk(bt, &key_name(vk), BS_PUSHBUTTON as u32 | WS_TABSTOP.0, 0, 240.0, 82.0, 256.0, 28.0, ID_MAPKEY);
            let show_done_h = mk(bt, s.show_done, BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0, 0, 16.0, 124.0, 480.0, 24.0, ID_SHOW_DONE);
            let capture_h = mk(bt, s.capture_visible, BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0, 0, 16.0, 152.0, 480.0, 24.0, ID_CAPTURE);
            let autostart_h = mk(bt, s.autostart, BS_AUTOCHECKBOX as u32 | WS_TABSTOP.0, 0, 16.0, 180.0, 480.0, 24.0, ID_AUTOSTART);
            mk(st, s.language, 0, 0, 16.0, 220.0, 220.0, 22.0, 0);
            // CBS_DROPDOWNLIST (3) | WS_VSCROLL
            let lang = mk(w!("COMBOBOX"), "", 3 | 0x0020_0000 | WS_TABSTOP.0, 0, 240.0, 216.0, 256.0, 300.0, ID_LANG);
            let add = |t: &str| {
                let w = wide(t);
                SendMessageW(lang, CB_ADDSTRING, WPARAM(0), LPARAM(w.as_ptr() as isize));
            };
            add(s.language_auto);
            for (_, name) in crate::i18n::LANGS.iter() {
                add(name);
            }
            let sel = crate::i18n::LANGS.iter().position(|(c, _)| *c == locale).map(|i| i + 1).unwrap_or(0);
            SendMessageW(lang, CB_SETCURSEL, WPARAM(sel), LPARAM(0));
            let chance_label = mk(st, &format!("{} : {} %", s.min_chance, min_chance), 0, 0, 16.0, 258.0, 480.0, 22.0, 0);
            // TBS_AUTOTICKS (1) | horizontal
            let chance = mk(w!("msctls_trackbar32"), "", 1 | WS_TABSTOP.0, 0, 16.0, 282.0, 480.0, 32.0, ID_CHANCE);
            SendMessageW(chance, TBM_SETRANGE, WPARAM(1), LPARAM((50 << 16) as isize));
            SendMessageW(chance, TBM_SETTICFREQ, WPARAM(5), LPARAM(0));
            SendMessageW(chance, TBM_SETPOS, WPARAM(1), LPARAM(min_chance.min(50) as isize));
            mk(st, s.help, 0, 0, 16.0, 328.0, 480.0, 60.0, 0);
            let status = mk(st, "", 0, 0, 16.0, 396.0, 480.0, 24.0, 0);
            mk(bt, s.test_key, BS_PUSHBUTTON as u32 | WS_TABSTOP.0, 0, 16.0, 430.0, 124.0, 30.0, ID_TEST);
            mk(bt, s.cancel, BS_PUSHBUTTON as u32 | WS_TABSTOP.0, 0, 176.0, 430.0, 102.0, 30.0, ID_CANCEL);
            mk(bt, s.apply, BS_PUSHBUTTON as u32 | WS_TABSTOP.0, 0, 285.0, 430.0, 102.0, 30.0, ID_APPLY);
            mk(bt, s.save, BS_PUSHBUTTON as u32 | WS_TABSTOP.0, 0, 394.0, 430.0, 102.0, 30.0, ID_SAVE);
            let set = |h: HWND, on: bool| {
                SendMessageW(h, BM_SETCHECK, WPARAM(on as usize), LPARAM(0));
            };
            set(show_done_h, show_done);
            set(capture_h, capture);
            set(autostart_h, autostart);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            Some(Settings {
                hwnd,
                edit,
                mapkey,
                show_done: show_done_h,
                capture: capture_h,
                autostart: autostart_h,
                status,
                lang,
                chance,
                chance_label,
                font,
                masked: true,
                vk,
                chance_text: s.min_chance,
            })
        }
    }

    pub fn toggle_mask(&mut self) {
        self.masked = !self.masked;
        unsafe {
            SendMessageW(self.edit, EM_SETPASSWORDCHAR, WPARAM(if self.masked { 0x25CF } else { 0 }), LPARAM(0));
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(self.edit, None, true);
        }
    }

    fn chance_value(&self) -> u32 {
        unsafe { SendMessageW(self.chance, TBM_GETPOS, WPARAM(0), LPARAM(0)).0.clamp(0, 50) as u32 }
    }

    /// Met à jour le libellé du curseur pendant qu'on le déplace.
    pub fn chance_moved(&self) {
        let w = wide(&format!("{} : {} %", self.chance_text, self.chance_value()));
        unsafe {
            let _ = SetWindowTextW(self.chance_label, PCWSTR(w.as_ptr()));
        }
    }

    pub fn set_mapkey_text(&self, t: &str) {
        let w = wide(t);
        unsafe {
            let _ = SetWindowTextW(self.mapkey, PCWSTR(w.as_ptr()));
        }
    }

    pub fn set_status(&self, t: &str) {
        let w = wide(t);
        unsafe {
            let _ = SetWindowTextW(self.status, PCWSTR(w.as_ptr()));
        }
    }

    pub fn key_text(&self) -> String {
        unsafe {
            let n = GetWindowTextLengthW(self.edit);
            let mut buf = vec![0u16; n as usize + 1];
            let got = GetWindowTextW(self.edit, &mut buf);
            String::from_utf16_lossy(&buf[..got as usize]).trim().to_string()
        }
    }

    pub fn values(&self) -> Values {
        let checked = |h: HWND| unsafe { SendMessageW(h, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1 };
        Values {
            key: self.key_text(),
            vk: self.vk,
            show_done: checked(self.show_done),
            capture_visible: checked(self.capture),
            autostart: checked(self.autostart),
            locale: {
                let i = unsafe { SendMessageW(self.lang, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 };
                if i >= 1 { crate::i18n::LANGS.get(i as usize - 1).map(|l| l.0.to_string()).unwrap_or_default() } else { String::new() }
            },
            min_chance: self.chance_value(),
        }
    }

    pub fn close(self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            let _ = DeleteObject(HGDIOBJ(self.font.0));
        }
    }
}
