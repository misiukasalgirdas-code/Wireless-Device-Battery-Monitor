//! The "Request support for a device" window (see report.rs for what goes into the request).
//!
//! Runs on the UI thread of the program; the device scan runs on a worker thread. *Send* opens the
//! filled-in "new issue" form on GitHub in the browser, and the user submits it there.
use std::cell::RefCell;
use std::ffi::c_void;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Graphics::Gdi::{
    CreateFontIndirectW, CreateSolidBrush, DeleteObject, FillRect, GetMonitorInfoW, GetSysColor, GetSysColorBrush, InvalidateRect,
    MonitorFromPoint, SetBkColor, SetTextColor, COLOR_BTNFACE, COLOR_WINDOW, COLOR_WINDOWTEXT, HBRUSH, HDC, HFONT, HGDIOBJ,
    MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::UI::Controls::SetWindowTheme;
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::i18n::{tr, trf};
use crate::report::{self, Candidate, Request, TYPES};

const CLASS: PCWSTR = w!("BatteryHubRequest");
const WM_CANDIDATES: u32 = WM_APP + 20;

const IDC_DEVICE: i32 = 101;
const IDC_MODEL: i32 = 102;
const IDC_TYPE: i32 = 103;
const IDC_NOTES: i32 = 104;
const IDC_PREVIEW: i32 = 105;
const IDC_COPY: i32 = 106;
const IDC_SEND: i32 = 107;
const IDC_STATUS: i32 = 108;
const IDC_NAME: i32 = 109;
const IDC_CLOSE: i32 = 2; // IDCANCEL: Esc closes the window

// Control styles and messages (plain numbers: the windows crate types them inconsistently).
const ES_MULTILINE: u32 = 0x4;
const ES_AUTOVSCROLL: u32 = 0x40;
const ES_AUTOHSCROLL: u32 = 0x80;
const ES_READONLY: u32 = 0x800;
const ES_WANTRETURN: u32 = 0x1000;
const CBS_DROPDOWNLIST: u32 = 0x3;
const BS_DEFPUSHBUTTON: u32 = 0x1;
const CB_ADDSTRING: u32 = 0x143;
const CB_GETCURSEL: u32 = 0x147;
const CB_RESETCONTENT: u32 = 0x14B;
const CB_SETCURSEL: u32 = 0x14E;
const EM_LIMITTEXT: u32 = 0xC5;
const CBN_SELCHANGE: u32 = 1;
const EN_CHANGE: u32 = 0x300;
const CF_UNICODETEXT: u32 = 13;

const MODEL_MAX: usize = 80;
const NOTES_MAX: usize = 500;
const NAME_MAX: usize = 40;

// Dark theme colours (Windows 11 dark dialogs).
const DARK_BG: u32 = 0x00202020;
const DARK_CTL: u32 = 0x002B2B2B;
const DARK_TEXT: u32 = 0x00F0F0F0;

#[derive(Default)]
struct State {
    wnd: HWND,
    font: HFONT,
    dpi: u32,
    cands: Vec<Candidate>,
    scanned: bool,
    /// Follows the Windows app theme (Settings > Personalisation > Colours).
    dark: bool,
    bg: HBRUSH,
    ctl: HBRUSH,
}

thread_local! {
    static STANDALONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn wnd() -> HWND {
    STATE.with(|s| s.borrow().wnd)
}

/// Opens the window, or brings it to the front if it is already open.
pub fn show() {
    unsafe {
        let open = wnd();
        if !open.is_invalid() {
            let _ = ShowWindow(open, SW_RESTORE);
            let _ = SetForegroundWindow(open);
            return;
        }
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        thread_local!(static REGISTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
        if !REGISTERED.with(|r| r.replace(true)) {
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc_),
                hInstance: hinst.into(),
                hIcon: LoadIconW(Some(hinst.into()), PCWSTR(std::ptr::without_provenance(1)) /* MAKEINTRESOURCE(1) */).unwrap_or_default(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: GetSysColorBrush(COLOR_WINDOW),
                lpszClassName: CLASS,
                ..Default::default()
            };
            RegisterClassExW(&wc);
        }
        // Client area 460 x 590 at the DPI of the monitor with the cursor, centred on it.
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let mut rc = RECT { left: 0, top: 0, right: scale(460, dx), bottom: scale(590, dx) };
        let _ = AdjustWindowRectExForDpi(&mut rc, style, false, WINDOW_EX_STYLE(0), dx);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
        let work = mi.rcWork;
        let x = work.left + (work.right - work.left - w) / 2;
        let y = work.top + (work.bottom - work.top - h) / 2;
        if let Ok(h) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS,
            &HSTRING::from(tr("BatteryHub: request support for a device")),
            style,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(hinst.into()),
            None,
        ) {
            let _ = ShowWindow(h, SW_SHOW);
            let _ = SetForegroundWindow(h);
        }
    }
}

/// Shows the window on its own (`--report`) and returns when it is closed.
pub fn run_standalone() {
    STANDALONE.with(|q| q.set(true));
    show();
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if pre_translate(&msg) {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Lets Tab / Enter / Esc work in the window: call from the message loop before TranslateMessage.
pub fn pre_translate(msg: &MSG) -> bool {
    let h = wnd();
    !h.is_invalid() && unsafe { IsDialogMessageW(h, msg) }.as_bool()
}

fn scale(v: i32, dpi: u32) -> i32 {
    v * dpi as i32 / 96
}

fn item(id: i32) -> HWND {
    unsafe { GetDlgItem(Some(wnd()), id).unwrap_or_default() }
}

fn send(h: HWND, msg: u32, wp: usize, lp: isize) -> isize {
    unsafe { SendMessageW(h, msg, Some(WPARAM(wp)), Some(LPARAM(lp))).0 }
}

fn set_text(id: i32, text: &str) {
    unsafe {
        let _ = SetWindowTextW(item(id), &HSTRING::from(text));
    }
}

fn text(id: i32) -> String {
    unsafe {
        let h = item(id);
        let n = GetWindowTextLengthW(h).max(0) as usize;
        let mut buf = vec![0u16; n + 1];
        let got = GetWindowTextW(h, &mut buf).max(0) as usize;
        String::from_utf16_lossy(&buf[..got]).trim().to_string()
    }
}

fn enable(id: i32, on: bool) {
    unsafe {
        let _ = EnableWindow(item(id), on);
    }
}

#[allow(clippy::too_many_arguments)]
fn add(class: PCWSTR, text: &str, style: u32, id: i32, x: i32, y: i32, w: i32, h: i32, ex: WINDOW_EX_STYLE) -> HWND {
    STATE.with(|s| {
        let s = s.borrow();
        unsafe {
            let c = CreateWindowExW(
                ex,
                class,
                &HSTRING::from(text),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style),
                scale(x, s.dpi),
                scale(y, s.dpi),
                scale(w, s.dpi),
                scale(h, s.dpi),
                Some(s.wnd),
                Some(HMENU(id as isize as *mut c_void)),
                GetModuleHandleW(None).ok().map(|m| m.into()),
                None,
            )
            .unwrap_or_default();
            SendMessageW(c, WM_SETFONT, Some(WPARAM(s.font.0 as usize)), Some(LPARAM(1)));
            c
        }
    })
}

fn make_font(dpi: u32) -> HFONT {
    unsafe {
        let mut ncm = NONCLIENTMETRICSW { cbSize: size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
        let _ = windows::Win32::UI::HiDpi::SystemParametersInfoForDpi(
            SPI_GETNONCLIENTMETRICS.0,
            ncm.cbSize,
            Some(&mut ncm as *mut _ as *mut c_void),
            0,
            dpi,
        );
        CreateFontIndirectW(&ncm.lfMessageFont)
    }
}

fn create_controls() {
    const L: i32 = 16;
    const W: i32 = 428;
    let none = WINDOW_EX_STYLE(0);
    let edge = WS_EX_CLIENTEDGE;
    let tab = WS_TABSTOP.0;
    add(
        w!("STATIC"),
        tr("Is one of your devices not shown? Pick it below. Send opens a new issue on the project's GitHub page, \
            filled in with what is shown here; you check it and submit it there (a GitHub account is needed)."),
        0,
        -1,
        L,
        12,
        W,
        48,
        none,
    );
    add(w!("STATIC"), tr("Device"), 0, -1, L, 66, W, 18, none);
    let dev = add(w!("COMBOBOX"), "", CBS_DROPDOWNLIST | WS_VSCROLL.0 | tab, IDC_DEVICE, L, 84, W, 320, none);
    send(dev, CB_ADDSTRING, 0, HSTRING::from(tr("Scanning devices...")).as_ptr() as isize);
    send(dev, CB_SETCURSEL, 0, 0);
    enable(IDC_DEVICE, false);
    add(w!("STATIC"), tr("Device name (as it's sold)"), 0, -1, L, 116, W, 18, none);
    let model = add(w!("EDIT"), "", ES_AUTOHSCROLL | tab, IDC_MODEL, L, 134, W, 24, edge);
    send(model, EM_LIMITTEXT, MODEL_MAX, 0);
    add(w!("STATIC"), tr("Type"), 0, -1, L, 166, W, 18, none);
    let ty = add(w!("COMBOBOX"), "", CBS_DROPDOWNLIST | tab, IDC_TYPE, L, 184, 200, 200, none);
    // Shown in the interface language; the issue itself gets the English name (same index).
    for t in TYPES {
        send(ty, CB_ADDSTRING, 0, HSTRING::from(tr(t)).as_ptr() as isize);
    }
    send(ty, CB_SETCURSEL, TYPES.len() - 1, 0);
    add(w!("STATIC"), tr("Your name (optional, shown in the GitHub issue)"), 0, -1, L, 216, W, 18, none);
    let name = add(w!("EDIT"), "", ES_AUTOHSCROLL | tab, IDC_NAME, L, 234, W, 24, edge);
    send(name, EM_LIMITTEXT, NAME_MAX, 0);
    add(w!("STATIC"), tr("Anything else we should know? (optional)"), 0, -1, L, 266, W, 18, none);
    let notes = add(
        w!("EDIT"),
        "",
        ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN | WS_VSCROLL.0 | tab,
        IDC_NOTES,
        L,
        284,
        W,
        54,
        edge,
    );
    send(notes, EM_LIMITTEXT, NOTES_MAX, 0);
    add(w!("STATIC"), tr("This is exactly what the GitHub issue will contain:"), 0, -1, L, 346, W, 18, none);
    add(w!("EDIT"), "", ES_MULTILINE | ES_READONLY | WS_VSCROLL.0 | tab, IDC_PREVIEW, L, 364, W, 132, edge);
    add(w!("STATIC"), tr("Scanning your devices..."), 0, IDC_STATUS, L, 502, W, 44, none);
    add(w!("BUTTON"), tr("Copy full report"), tab, IDC_COPY, L, 550, 180, 28, none);
    add(w!("BUTTON"), tr("Send"), BS_DEFPUSHBUTTON | tab, IDC_SEND, L + W - 186, 550, 90, 28, none);
    add(w!("BUTTON"), tr("Close"), tab, IDC_CLOSE, L + W - 90, 550, 90, 28, none);
    enable(IDC_SEND, false);
}

fn selected() -> Option<Candidate> {
    let i = send(item(IDC_DEVICE), CB_GETCURSEL, 0, 0);
    STATE.with(|s| {
        let s = s.borrow();
        (s.scanned && i >= 0).then(|| s.cands.get(i as usize).cloned()).flatten()
    })
}

fn device_chosen() -> bool {
    STATE.with(|s| s.borrow().scanned) && send(item(IDC_DEVICE), CB_GETCURSEL, 0, 0) >= 0
}

fn current() -> (Request, Option<Candidate>) {
    let t = send(item(IDC_TYPE), CB_GETCURSEL, 0, 0);
    let kind = TYPES.get(t.max(0) as usize).copied().unwrap_or("other");
    let r = Request {
        model: text(IDC_MODEL).chars().take(MODEL_MAX).collect(),
        name: text(IDC_NAME).chars().take(NAME_MAX).collect(),
        kind: kind.into(),
        notes: text(IDC_NOTES).replace("\r\n", "\n").chars().take(NOTES_MAX).collect(),
    };
    (r, selected())
}

fn issue() -> (String, String) {
    let (r, c) = current();
    let title = report::issue_title(&r, c.as_ref());
    let body = report::issue_body(&r, c.as_ref(), &report::windows_build(), &crate::install::version_label());
    (title, body)
}

fn update_preview() {
    let (r, _) = current();
    let (title, body) = issue();
    let shown = if r.model.is_empty() { tr("(type the device name)").to_string() } else { title };
    set_text(IDC_PREVIEW, &format!("Title: {shown}\n\n{body}").replace('\n', "\r\n"));
    enable(IDC_SEND, device_chosen() && r.model.chars().count() >= 2);
}

fn on_device_changed() {
    let c = selected();
    set_text(IDC_MODEL, c.as_ref().map(|c| c.name.as_str()).unwrap_or(""));
    let t = c.as_ref().map(|c| c.type_index()).unwrap_or(TYPES.len() - 1);
    send(item(IDC_TYPE), CB_SETCURSEL, t, 0);
    update_preview();
}

fn fill_devices(list: Vec<Candidate>) {
    let cb = item(IDC_DEVICE);
    send(cb, CB_RESETCONTENT, 0, 0);
    for c in &list {
        send(cb, CB_ADDSTRING, 0, HSTRING::from(c.label()).as_ptr() as isize);
    }
    send(cb, CB_ADDSTRING, 0, HSTRING::from(tr("My device is not in this list")).as_ptr() as isize);
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.cands = list;
        s.scanned = true;
    });
    enable(IDC_DEVICE, true);
    set_text(
        IDC_STATUS,
        tr("Pick the device that isn't shown. Devices with a vendor-specific interface or audio are listed first."),
    );
    update_preview();
}

fn copy_to_clipboard(hwnd: HWND, text: &str) -> bool {
    let wide: Vec<u16> = text.replace('\n', "\r\n").encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        if OpenClipboard(Some(hwnd)).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let mut ok = false;
        if let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            let p = GlobalLock(mem) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
                let _ = GlobalUnlock(mem);
                ok = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(mem.0))).is_ok();
            }
            if !ok {
                let _ = GlobalFree(Some(mem));
            }
        }
        let _ = CloseClipboard();
        ok
    }
}

/// Light or dark, following the Windows app theme: title bar, background, text, edit boxes, lists,
/// buttons and scroll bars. Called at start and whenever the theme changes.
fn apply_theme(h: HWND) {
    let dark = !crate::ui::apps_are_light();
    unsafe {
        let v: i32 = dark as i32;
        let _ = DwmSetWindowAttribute(h, DWMWINDOWATTRIBUTE(20), &v as *const i32 as *const c_void, 4);
        crate::ui::set_menu_theme(h, dark); // lets the dark control themes apply to this window
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.dark = dark;
            for b in [s.bg, s.ctl] {
                if !b.is_invalid() {
                    let _ = DeleteObject(HGDIOBJ(b.0));
                }
            }
            s.bg = CreateSolidBrush(COLORREF(if dark { DARK_BG } else { GetSysColor(COLOR_WINDOW) }));
            s.ctl = CreateSolidBrush(COLORREF(if dark { DARK_CTL } else { GetSysColor(COLOR_WINDOW) }));
        });
        // Every control: dark or normal visual style.
        let mut c = GetWindow(h, GW_CHILD).unwrap_or_default();
        while !c.is_invalid() {
            let mut class = [0u16; 32];
            let n = GetClassNameW(c, &mut class) as usize;
            let class = String::from_utf16_lossy(&class[..n]).to_ascii_lowercase();
            let theme = match (dark, class.as_str()) {
                (true, "combobox") | (true, "edit") => w!("DarkMode_CFD"),
                (true, _) => w!("DarkMode_Explorer"),
                (false, _) => w!("Explorer"),
            };
            if class != "static" {
                let _ = SetWindowTheme(c, theme, PCWSTR::null());
            }
            let _ = InvalidateRect(Some(c), None, true);
            c = GetWindow(c, GW_HWNDNEXT).unwrap_or_default();
        }
        let _ = InvalidateRect(Some(h), None, true);
    }
}

/// Text and background of a control (WM_CTLCOLOR*): `ctl` for edit boxes and lists.
fn control_colors(wp: WPARAM, field: bool) -> LRESULT {
    STATE.with(|s| {
        let s = s.borrow();
        let dc = HDC(wp.0 as *mut c_void);
        unsafe {
            let (text, bg, brush) = if s.dark {
                (DARK_TEXT, if field { DARK_CTL } else { DARK_BG }, if field { s.ctl } else { s.bg })
            } else {
                (GetSysColor(COLOR_WINDOWTEXT), GetSysColor(COLOR_WINDOW), if field { s.ctl } else { s.bg })
            };
            SetTextColor(dc, COLORREF(text));
            SetBkColor(dc, COLORREF(bg));
            LRESULT(brush.0 as isize)
        }
    })
}

extern "system" fn proc_(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_CREATE => {
                let dpi = GetDpiForWindow(h);
                STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    s.wnd = h;
                    s.dpi = if dpi == 0 { 96 } else { dpi };
                    s.font = make_font(s.dpi);
                });
                create_controls();
                apply_theme(h);
                let hv = h.0 as usize;
                std::thread::spawn(move || {
                    let ptr = Box::into_raw(Box::new(report::scan()));
                    if PostMessageW(Some(HWND(hv as *mut c_void)), WM_CANDIDATES, WPARAM(0), LPARAM(ptr as isize)).is_err() {
                        drop(Box::from_raw(ptr));
                    }
                });
                LRESULT(0)
            }
            WM_CANDIDATES => {
                let list = Box::from_raw(lp.0 as *mut Vec<Candidate>);
                fill_devices(*list);
                LRESULT(0)
            }
            WM_SETTINGCHANGE => {
                let area = if lp.0 != 0 { PCWSTR(lp.0 as *const u16).to_string().unwrap_or_default() } else { String::new() };
                if area == "ImmersiveColorSet" {
                    apply_theme(h);
                }
                DefWindowProcW(h, msg, wp, lp)
            }
            WM_COMMAND => {
                let id = (wp.0 & 0xFFFF) as i32;
                let code = ((wp.0 >> 16) & 0xFFFF) as u32;
                match id {
                    IDC_CLOSE => {
                        let _ = DestroyWindow(h);
                    }
                    IDC_DEVICE if code == CBN_SELCHANGE => on_device_changed(),
                    IDC_TYPE if code == CBN_SELCHANGE => update_preview(),
                    IDC_MODEL | IDC_NAME | IDC_NOTES if code == EN_CHANGE => update_preview(),
                    IDC_SEND | 1 /* IDOK: Enter */ => {
                        if IsWindowEnabled(item(IDC_SEND)).as_bool() {
                            let (title, body) = issue();
                            let url = report::issue_url(&title, &body);
                            let r = ShellExecuteW(None, w!("open"), &HSTRING::from(url), None, None, SW_SHOWNORMAL);
                            if r.0 as isize > 32 {
                                set_text(
                                    IDC_STATUS,
                                    tr("GitHub opened in your browser with the request filled in. Check it and click Create to \
                                        submit it. Thank you!"),
                                );
                            } else {
                                let ok = copy_to_clipboard(h, &format!("{title}\n\n{body}"));
                                set_text(
                                    IDC_STATUS,
                                    &if ok {
                                        trf(
                                            "Couldn't open the browser. The request is copied: paste it into a new issue at \
                                             github.com/{0}/issues.",
                                            &[&report::REPO],
                                        )
                                    } else {
                                        trf("Couldn't open the browser. Please open a new issue at github.com/{0}/issues.", &[&report::REPO])
                                    },
                                );
                            }
                        }
                    }
                    IDC_COPY => {
                        let (title, body) = issue();
                        let sel = selected();
                        let all = STATE.with(|s| report::full_report(&s.borrow().cands, sel.as_ref(), &title, &body));
                        let done = copy_to_clipboard(h, &all);
                        set_text(
                            IDC_STATUS,
                            if done {
                                tr("The full report (with every device BatteryHub sees) is copied. Paste it into the GitHub \
                                    issue if we ask for more details.")
                            } else {
                                tr("Couldn't copy to the clipboard. Please try again.")
                            },
                        );
                    }
                    _ => return DefWindowProcW(h, msg, wp, lp),
                }
                LRESULT(0)
            }
            WM_CTLCOLORSTATIC => {
                // The read-only preview is an edit box: same background as the other fields.
                let field = HWND(lp.0 as *mut c_void) == item(IDC_PREVIEW);
                control_colors(wp, field)
            }
            WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => control_colors(wp, true),
            WM_CTLCOLORBTN => control_colors(wp, false),
            WM_ERASEBKGND => {
                let mut rc = RECT::default();
                let _ = GetClientRect(h, &mut rc);
                let bg = STATE.with(|s| s.borrow().bg);
                FillRect(HDC(wp.0 as *mut c_void), &rc, if bg.is_invalid() { GetSysColorBrush(COLOR_BTNFACE) } else { bg });
                LRESULT(1)
            }
            WM_DESTROY => {
                let old = STATE.with(|s| std::mem::take(&mut *s.borrow_mut()));
                for o in [HGDIOBJ(old.font.0), HGDIOBJ(old.bg.0), HGDIOBJ(old.ctl.0)] {
                    if !o.is_invalid() {
                        let _ = DeleteObject(o);
                    }
                }
                // The window and the scan used some memory: hand it back.
                let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
                if STANDALONE.with(|q| q.get()) {
                    PostQuitMessage(0);
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(h, msg, wp, lp),
        }
    }
}

