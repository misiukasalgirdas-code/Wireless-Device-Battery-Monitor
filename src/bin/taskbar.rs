//! BatteryHub Taskbar. Runs in the background (the installer can add it to autostart)
//! and draws "🎧 25%  🖱 32%" straight onto the taskbar, left of the notification area, so no
//! hovering is needed. Levels reload every minute. Left click does nothing; right click opens the
//! menu (warning levels, refresh options, exit). When the taskbar cannot host the strip (vertical
//! taskbar, unknown Explorer layout) it falls back to one notification-area icon per device.
#![windows_subsystem = "windows"]

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::ffi::c_void;
use std::time::Instant;

use batteryhub::device::Missing;
use batteryhub::i18n::{self, tr, trf};
use batteryhub::settings;
use batteryhub::ui::{self, rgb, split};
use batteryhub::{DeviceKind, DeviceStatus};
use windows::core::w;
use windows::Win32::Foundation::{GetLastError, COLORREF, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, SetProcessWorkingSetSize};
use windows::Win32::UI::Controls::{
    TOOLTIPS_CLASSW, TTF_ABSOLUTE, TTF_TRACK, TTM_ADDTOOLW, TTM_GETBUBBLESIZE, TTM_SETMAXTIPWIDTH, TTM_TRACKACTIVATE, TTM_TRACKPOSITION,
    TTM_UPDATETIPTEXTW, TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW,
};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::*;

const WM_POLL_DONE: u32 = WM_APP + 2;
const WM_TRAY: u32 = WM_APP + 3;
const WM_QUICK_DONE: u32 = WM_APP + 4;
const WM_UPDATE_CHECKED: u32 = WM_APP + 5;
const WM_UPDATE_DOWNLOADED: u32 = WM_APP + 6;
const WM_SHOW_UPDATED: u32 = WM_APP + 7;
// Not exported by the windows crate's WindowsAndMessaging module.
const WM_MOUSELEAVE: u32 = 0x02A3;
const TIMER_POLL: usize = 1;
const TIMER_PLACE: usize = 2;
const TIMER_DEVCHANGE: usize = 3;
const TIMER_BTWATCH: usize = 4;
const TIMER_QUICK: usize = 5;
const TIMER_AUTO_UPDATE: usize = 6;
/// "Update finished" message shortly after the first start of an updated version.
const TIMER_UPDATED: usize = 7;
/// Automatic update check this long after start-up; when the network is not up yet (e.g. right
/// at sign-in) it is tried again every AUTO_UPDATE_RETRY_MS, at most AUTO_UPDATE_RETRIES times.
const AUTO_UPDATE_DELAY_MS: u32 = 2_000;
const AUTO_UPDATE_RETRY_MS: u32 = 15_000;
const AUTO_UPDATE_RETRIES: u32 = 8;
static AUTO_UPDATE_TRIES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// How often receivers whose device is off or asleep are checked again.
const QUICK_MS: u32 = 10_000;
const WM_DEVICECHANGE_MSG: u32 = 0x0219;
const DBT_DEVNODES_CHANGED: usize = 0x0007;
const DBT_DEVICEARRIVAL: usize = 0x8000;
const DBT_DEVICEREMOVECOMPLETE: usize = 0x8004;
/// Seconds the strip may be impossible to place before falling back to notification-area icons.
const FALLBACK_AFTER_TICKS: u32 = 3;
const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;
/// Battery refresh intervals offered in the menu, in minutes.
const INTERVALS: [(u32, &str); 5] = [
    (1, "1 minute"),
    (5, "5 minutes"),
    (10, "10 minutes"),
    (30, "30 minutes"),
    (60, "1 hour"),
];
const ID_INTERVAL_BASE: usize = 10;
const ID_ON_CHANGE: usize = 20;
const MIN_GRACE_SEC: u64 = 150;
const ID_EXIT: usize = 1;
const ID_REFRESH_DEVICES: usize = 2;
const ID_REFRESH_LEVELS: usize = 3;
const ID_CHECK_UPDATES: usize = 4;
const ID_REQUEST: usize = 5;
const ID_AUTOSTART: usize = 6;
const ID_AUTO_UPDATE: usize = 7;
const ID_BACKGROUND: usize = 8;
const ID_SILENT_UPDATE: usize = 9;
/// "Show widget on": all monitors, main monitor only, or ID_MONITOR_FIRST + i for one other monitor.
const ID_MONITOR_ALL: usize = 40;
const ID_MONITOR_MAIN: usize = 41;
const ID_MONITOR_FIRST: usize = 42;
const ID_MONITOR_GONE: usize = 73;
/// "Position on the taskbar": ID_POSITION_FIRST + i for POSITION_OFFSETS[i].
const ID_POSITION_FIRST: usize = 80;
const ID_POSITION_CUSTOM: usize = 95;
const POSITION_OFFSETS: [u32; 11] = [0, 20, 40, 60, 80, 100, 120, 140, 160, 180, 200];
/// "Height on the taskbar": ID_HEIGHT_FIRST + i for HEIGHT_OFFSETS[i] (px, + = down).
const ID_HEIGHT_FIRST: usize = 100;
const ID_HEIGHT_CUSTOM: usize = 115;
const HEIGHT_OFFSETS: [i32; 7] = [-15, -10, -5, 0, 5, 10, 15];
/// "Language": ID_LANG_FIRST + i for i18n::Lang::ALL[i].
const ID_LANG_FIRST: usize = 120;

pub const MAIN_CLASS: windows::core::PCWSTR = w!("BatteryHub.Taskbar.Main");
const STRIP_CLASS: windows::core::PCWSTR = w!("BatteryHub.Taskbar.Strip");
const SETTINGS_KEY: &str = batteryhub::install::TASKBAR.settings_key;

struct Tracked {
    status: DeviceStatus,
    seen: Instant,
}

#[derive(Default)]
struct App {
    main: HWND,
    strip: HWND,
    taskbar: HWND,
    devices: BTreeMap<String, Tracked>,
    order: Vec<String>,
    polling: bool,
    loaded: bool,
    /// A refresh started from the menu: show the ↻ indicator until it finishes.
    manual: bool,
    /// "Refresh devices": forget every known device (no grace period) on the next result.
    forget: bool,
    /// Whether the devices were enumerated at least once (the first read always enumerates).
    scanned: bool,
    /// The next poll was triggered by a device connecting / disconnecting: devices that are gone
    /// and have no sleep grace period disappear at once instead of after MIN_GRACE_SEC.
    devchange: bool,
    size: SIZE,
    last_place: (RECT, RECT, i32),
    /// Strip x ranges of the devices drawn in the last paint: (from, to, device index).
    hits: Vec<(i32, i32, usize)>,
    /// Hover tooltip (tracking tooltip owned by the main window) and what it shows right now.
    tip: HWND,
    tip_for: Option<usize>,
    tracking: bool,
    taskbar_created: u32,
    /// (taskbar client height, DPI, light theme) the strip was last rendered for; a change (e.g.
    /// after leaving full screen, a DPI or theme change) triggers a fresh render.
    painted_for: (i32, u32, bool),
    /// Notification-area icons instead of the strip (the taskbar cannot host it right now).
    fallback: bool,
    /// Consecutive seconds the strip could not be placed.
    fail_ticks: u32,
    /// Fallback icons: device key -> (icon id, icon handle).
    tray: BTreeMap<String, (u32, HICON)>,
    next_uid: u32,
    /// Last Bluetooth connection signature (connect / disconnect detection).
    bt_sig: u64,
    /// A device change arrived while a read was running: read again as soon as it finishes.
    pending_change: bool,
    /// Receivers / cables (vid, pid) whose device reported "off" in the last read.
    offline_hw: HashSet<(u16, u16)>,
    /// Light taskbar theme and the optional extra gap (TaskbarOffset), read once and again only
    /// when Windows reports a settings change - not from the registry every second.
    light: bool,
    offset: u32,
    /// Up / down shift of the widget in px (TaskbarOffsetY, + = down).
    offset_y: i32,
    /// Copies of the strip on the taskbars of other monitors ("Show widget on"): (taskbar, strip).
    mirrors: Vec<(HWND, HWND)>,
    /// Pixels of the last render, pushed to every mirror as well.
    pixels: Vec<u32>,
    /// Strip the tooltip belongs to (the main strip or a mirror).
    tip_on: HWND,
    /// "Show widget on": "all", "main" or the id of one other monitor (cached setting).
    choice: String,
    /// The widget is shown only on another monitor: the main taskbar's strip is hidden.
    primary_hidden: bool,
    /// Plate behind the widget for transparent taskbars (cached setting).
    background: bool,
    /// Version the setup just updated from (shown once in "Update finished").
    updated_from: Option<String>,
}

thread_local! {
    static APP: RefCell<App> = RefCell::new(App::default());
    /// Receivers being read by the running quick read.
    static QUICK_IDS: RefCell<Vec<(u16, u16)>> = const { RefCell::new(Vec::new()) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().map(|mut a| f(&mut a)))
}

// ---- drawing -----------------------------------------------------------------------------------

/// Premultiplied BGRA canvas.
struct Canvas {
    w: i32,
    h: i32,
    px: Vec<u32>,
}

impl Canvas {
    /// Fills the canvas with a rounded rectangle, `inset` from the top and bottom edge.
    fn round_rect(&mut self, inset: i32, radius: i32, c: COLORREF, a: u32) {
        let (w, h, r) = (self.w, self.h, radius.max(1));
        for y in inset..h - inset {
            for x in 0..w {
                let dx = if x < r { r - x } else if x >= w - r { x - (w - r - 1) } else { 0 };
                let dy = if y < inset + r { inset + r - y } else if y >= h - inset - r { y - (h - inset - r - 1) } else { 0 };
                if dx * dx + dy * dy <= r * r {
                    self.blend(x, y, c, a);
                }
            }
        }
    }

    fn blend(&mut self, x: i32, y: i32, c: COLORREF, a: u32) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h || a == 0 {
            return;
        }
        let (r, g, b) = split(c);
        let d = &mut self.px[(y * self.w + x) as usize];
        let inv = 255 - a;
        let ch = |src: u8, shift: u32| ((src as u32 * a + ((*d >> shift) & 0xFF) * inv) / 255) & 0xFF;
        let na = (a + ((*d >> 24) & 0xFF) * inv / 255).min(255);
        *d = na << 24 | ch(r, 16) << 16 | ch(g, 8) << 8 | ch(b, 0);
    }
}

/// Ink coverage of `text`, cropped to the drawn pixels. Same technique as the original widget:
/// ClearType text, white on black (black on white for a light taskbar), coverage = RGB average.
struct Mask {
    w: i32,
    h: i32,
    a: Vec<u8>,
}

fn mask(text: &str, font: HFONT, dark_on_light: bool) -> Mask {
    unsafe {
        let dc = CreateCompatibleDC(None);
        let old_f = SelectObject(dc, HGDIOBJ(font.0));
        let wtext: Vec<u16> = text.encode_utf16().collect();
        let mut sz = SIZE::default();
        let _ = GetTextExtentPoint32W(dc, &wtext, &mut sz);
        let (w, h) = (sz.cx + 8, sz.cy + 8);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let mut out = Mask { w: 0, h: 0, a: Vec::new() };
        if let Ok(bmp) = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            let old_b = SelectObject(dc, HGDIOBJ(bmp.0));
            let px = std::slice::from_raw_parts_mut(bits as *mut u32, (w * h) as usize);
            px.fill(if dark_on_light { 0xFFFF_FFFF } else { 0 });
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, if dark_on_light { rgb(0, 0, 0) } else { rgb(255, 255, 255) });
            let _ = TextOutW(dc, 4, 4, &wtext);
            let _ = GdiFlush();
            let cover = |v: u32| -> u8 {
                let c = ((v & 0xFF) + ((v >> 8) & 0xFF) + ((v >> 16) & 0xFF)) / 3;
                (if dark_on_light { 255 - c } else { c }) as u8
            };
            let (mut x0, mut y0, mut x1, mut y1) = (w, h, -1, -1);
            for y in 0..h {
                for x in 0..w {
                    if cover(px[(y * w + x) as usize]) != 0 {
                        x0 = x0.min(x);
                        x1 = x1.max(x);
                        y0 = y0.min(y);
                        y1 = y1.max(y);
                    }
                }
            }
            if x1 >= 0 {
                out.w = x1 - x0 + 1;
                out.h = y1 - y0 + 1;
                out.a = Vec::with_capacity((out.w * out.h) as usize);
                for y in 0..out.h {
                    for x in 0..out.w {
                        out.a.push(cover(px[((y0 + y) * w + x0 + x) as usize]));
                    }
                }
            }
            SelectObject(dc, old_b);
            let _ = DeleteObject(HGDIOBJ(bmp.0));
        }
        SelectObject(dc, old_f);
        let _ = DeleteDC(dc);
        out
    }
}

/// Creates a ClearType font, falling back when the preferred face is not installed (GDI would
/// otherwise silently substitute another face).
fn make_font(px: i32, weight: i32, face: &str, fallback: &str, fallback_px: i32) -> HFONT {
    for (f, size) in [(face, px), (fallback, if fallback_px > 0 { fallback_px } else { px })] {
        unsafe {
            let h = CreateFontW(
                -size,
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                0,
                &windows::core::HSTRING::from(f),
            );
            if f == fallback {
                return h;
            }
            let dc = CreateCompatibleDC(None);
            let o = SelectObject(dc, HGDIOBJ(h.0));
            let mut got = [0u16; 64];
            GetTextFaceW(dc, Some(&mut got));
            SelectObject(dc, o);
            let _ = DeleteDC(dc);
            if batteryhub::util::from_wide(&got).eq_ignore_ascii_case(f) {
                return h;
            }
            let _ = DeleteObject(HGDIOBJ(h.0));
        }
    }
    unreachable!()
}

struct Piece {
    /// Index of the device this piece belongs to (for hover hit-testing), None for others.
    dev: Option<usize>,
    m: Mask,
    color: COLORREF,
    alpha: u32,
    gap_after: i32,
}

impl App {
    fn scale(&self) -> f32 {
        let dpi = unsafe { GetDpiForWindow(self.taskbar) };
        if dpi == 0 {
            1.0
        } else {
            dpi as f32 / 96.0
        }
    }

    fn visible(&self) -> Vec<DeviceStatus> {
        self.order
            .iter()
            .filter_map(|k| self.devices.get(k))
            .map(|t| t.status.clone())
            .filter(|s| s.connected)
            .collect()
    }

    /// Renders the strip and hands it to the layered window.
    fn paint(&mut self) {
        if self.fallback {
            self.sync_tray();
            return;
        }
        if self.strip.0.is_null() {
            return;
        }
        self.painted_for = self.render_target();
        let s = self.scale();
        let px = |v: f32| (v * s).round() as i32;
        let light = self.light;
        // Same colours, fonts and spacing as the original C++ widget.
        let fg = if light { rgb(0, 0, 0) } else { rgb(255, 255, 255) };
        let red = ui::low_color(light);
        let icons = make_font(px(16.0), 400, "Segoe Fluent Icons", "Segoe MDL2 Assets", 0);
        let bolt = make_font(px(11.0), 400, "Segoe Fluent Icons", "Segoe MDL2 Assets", 0);
        let text = make_font(px(13.0), 400, "Segoe UI Variable Text", "Segoe UI", px(12.0));

        let mut pieces: Vec<Piece> = Vec::new();
        let devices = self.visible();
        if devices.is_empty() {
            // Battery-unknown glyph, so there is always something to right-click.
            pieces.push(Piece {
                dev: None,
                m: mask("\u{E996}", icons, light),
                color: fg,
                alpha: 110,
                gap_after: 0,
            });
        }
        for (i, d) in devices.iter().enumerate() {
            let alpha = if d.asleep { 120 } else { 255 };
            // Red at or below this device's own warning level (right-click menu, default 15%).
            let low = settings::is_low(d, settings::threshold(SETTINGS_KEY, &d.key));
            // Lightning bolt on power: green while charging, blue once it reached 100%.
            let bolt_color = ui::bolt_color(d, light);
            let mark = bolt_color.is_some();
            pieces.push(Piece {
                dev: Some(i),
                m: mask(&d.kind.glyph().to_string(), icons, light),
                color: fg,
                alpha,
                gap_after: px(5.0),
            });
            let t = if !d.level_text.is_empty() && d.percent < 0 {
                i18n::tr_any(&d.level_text).to_string()
            } else if d.percent >= 0 {
                format!("{}%", d.percent)
            } else {
                "?".into()
            };
            pieces.push(Piece {
                dev: Some(i),
                m: mask(&t, text, light),
                color: if low { red } else { fg },
                alpha,
                gap_after: if mark { px(2.0) } else { 0 },
            });
            if let Some(c) = bolt_color {
                pieces.push(Piece {
                    dev: Some(i),
                    m: mask("\u{E945}", bolt, light),
                    color: c,
                    alpha,
                    gap_after: 0,
                });
            }
            if i + 1 < devices.len() {
                pieces.last_mut().unwrap().gap_after = px(14.0);
            }
        }
        if self.manual && self.polling {
            // Refresh started from the menu: a small refresh glyph until it finishes.
            pieces.last_mut().unwrap().gap_after = px(10.0);
            pieces.push(Piece {
                dev: None,
                m: mask("\u{E72C}", bolt, light),
                color: fg,
                alpha: 170,
                gap_after: 0,
            });
        }
        for f in [icons, bolt, text] {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(f.0));
            }
        }

        let mut tb = RECT::default();
        unsafe {
            let _ = GetClientRect(self.taskbar, &mut tb);
        }
        let h = (tb.bottom - tb.top).clamp(px(24.0), px(64.0));
        let pad = px(8.0);
        let w = pad * 2 + pieces.iter().map(|p| p.m.w + p.gap_after).sum::<i32>();
        // Alpha 1 everywhere: fully transparent pixels of a layered window let clicks fall through.
        let mut cv = Canvas {
            w,
            h,
            px: vec![0x0100_0000; (w * h) as usize],
        };
        if self.background {
            // Rounded plate in the taskbar's own colour: keeps the text readable on a transparent
            // or restyled taskbar (e.g. TranslucentTB, Windhawk's Taskbar Styler).
            let plate = if light { rgb(243, 243, 243) } else { rgb(32, 32, 32) };
            cv.round_rect(px(4.0), px(6.0), plate, 235);
        }
        let mut x = pad;
        self.hits.clear();
        for p in &pieces {
            if let Some(dev) = p.dev {
                // Each device owns its pieces plus half of the gap around them.
                match self.hits.last_mut() {
                    Some(h) if h.2 == dev => h.1 = x + p.m.w + p.gap_after / 2,
                    _ => {
                        let from = self.hits.last().map(|h| h.1).unwrap_or(0);
                        self.hits.push((from, x + p.m.w + p.gap_after / 2, dev));
                    }
                }
            }
            let y0 = ((h - p.m.h) / 2 + px(self.offset_y as f32)).clamp(0, (h - p.m.h).max(0));
            for yy in 0..p.m.h {
                for xx in 0..p.m.w {
                    let a = p.m.a[(yy * p.m.w + xx) as usize] as u32 * p.alpha / 255;
                    cv.blend(x + xx, y0 + yy, p.color, a);
                }
            }
            x += p.m.w + p.gap_after;
        }
        if let Some(last) = self.hits.last_mut() {
            last.1 = w;
        }
        self.size = SIZE { cx: w, cy: h };
        self.pixels = cv.px;
        self.push(self.strip);
        for i in 0..self.mirrors.len() {
            self.push(self.mirrors[i].1);
        }
        self.last_place.2 = -1; // force a reposition with the new width
        self.place();
    }

    /// Hands the last render to a strip window (the main strip or a mirror).
    fn push(&self, strip: HWND) {
        let (w, h) = (self.size.cx, self.size.cy);
        if strip.0.is_null() || self.pixels.len() != (w * h) as usize {
            return;
        }
        unsafe {
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            if let Ok(bmp) = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
                std::ptr::copy_nonoverlapping(self.pixels.as_ptr(), bits as *mut u32, self.pixels.len());
                let ob = SelectObject(mem, HGDIOBJ(bmp.0));
                let src = POINT::default();
                let bf = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                };
                let _ = UpdateLayeredWindow(
                    strip,
                    Some(screen),
                    None,
                    Some(&self.size),
                    Some(mem),
                    Some(&src),
                    COLORREF(0),
                    Some(&bf),
                    ULW_ALPHA,
                );
                SelectObject(mem, ob);
                let _ = DeleteObject(HGDIOBJ(bmp.0));
            }
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
        }
    }

    // ---- taskbar placement ---------------------------------------------------------------------

    /// What the strip has to be rendered for right now: taskbar height, DPI and theme.
    fn render_target(&self) -> (i32, u32, bool) {
        let mut client = RECT::default();
        unsafe {
            let _ = GetClientRect(self.taskbar, &mut client);
            (client.bottom - client.top, GetDpiForWindow(self.taskbar), self.light)
        }
    }

    /// Creates the strip as a child of the taskbar (again, after Explorer restarted). Returns false
    /// when the taskbar cannot host it right now: no taskbar / notification area (Explorer is
    /// restarting or uses an unknown layout), or a vertical taskbar.
    fn ensure_strip(&mut self) -> bool {
        unsafe {
            // The strip is only valid while it still is a window and a child of a live taskbar; a
            // handle that outlived Explorer can be reused by an unrelated window.
            let alive = !self.strip.0.is_null()
                && IsWindow(Some(self.strip)).as_bool()
                && IsWindow(Some(self.taskbar)).as_bool()
                && GetParent(self.strip).is_ok_and(|p| p == self.taskbar);
            if !alive {
                if is_our_strip(self.strip) {
                    let _ = DestroyWindow(self.strip);
                }
                self.strip = HWND::default();
                let Ok(taskbar) = FindWindowW(w!("Shell_TrayWnd"), None) else {
                    dbg_log("no Shell_TrayWnd");
                    return false;
                };
                if FindWindowExW(Some(taskbar), None, w!("TrayNotifyWnd"), None).is_err() {
                    dbg_log("no TrayNotifyWnd");
                    return false;
                }
                self.taskbar = taskbar;
            }
            let mut bar = RECT::default();
            let _ = GetWindowRect(self.taskbar, &mut bar);
            if bar.bottom - bar.top > bar.right - bar.left {
                if !self.strip.0.is_null() {
                    let _ = ShowWindow(self.strip, SW_HIDE);
                }
                return false; // vertical taskbar: handled by the notification-area fallback
            }
            if alive {
                return true;
            }
            let Some(strip) = create_strip(self.taskbar) else {
                return false;
            };
            dbg_log(&format!("strip created {:?} parent {:?}", strip, self.taskbar));
            self.strip = strip;
            self.last_place = Default::default();
            // Shown only on another monitor: the new main strip stays hidden from the start (no
            // flash on the main taskbar before the first sync hides it).
            self.primary_hidden = self.chosen_bars().1;
        }
        if !self.fallback {
            self.paint();
            if !self.primary_hidden {
                unsafe {
                    let _ = ShowWindow(self.strip, SW_SHOWNOACTIVATE);
                }
            }
        }
        true
    }

    /// Destroys any other strip window of this process on the taskbar (only `self.strip` may exist).
    fn remove_orphan_strips(&self) {
        unsafe {
            let mut after: Option<HWND> = None;
            while let Ok(h) = FindWindowExW(Some(self.taskbar), after, STRIP_CLASS, None) {
                if h.0.is_null() {
                    break;
                }
                after = Some(h);
                if h != self.strip && is_our_strip(h) {
                    dbg_log(&format!("removing orphaned strip {h:?}"));
                    let _ = DestroyWindow(h);
                }
            }
        }
    }

    /// Switches between the taskbar strip and notification-area icons.
    fn set_fallback(&mut self, on: bool) {
        if self.fallback == on {
            return;
        }
        dbg_log(&format!("fallback to notification-area icons: {on}"));
        self.fallback = on;
        if on {
            self.hide_tip();
            self.drop_mirrors();
            if !self.strip.0.is_null() {
                unsafe {
                    let _ = ShowWindow(self.strip, SW_HIDE);
                }
            }
            self.sync_tray();
        } else {
            self.remove_tray();
            self.last_place = Default::default();
            self.paint();
        }
    }

    /// One tick of the 1 s placement timer: keep the strip in place, or fall back.
    fn tick(&mut self) {
        if std::env::var_os("BH_FORCE_FALLBACK").is_some() {
            return;
        }
        if self.ensure_strip() {
            self.remove_orphan_strips();
            self.fail_ticks = 0;
            if self.fallback {
                self.set_fallback(false);
            }
            // Re-render when the taskbar changed height, DPI or theme since the last render
            // (e.g. after leaving a full-screen game).
            if self.render_target() != self.painted_for {
                self.paint();
            } else {
                self.place();
            }
            self.sync_mirrors();
        } else {
            self.fail_ticks += 1;
            if self.fail_ticks >= FALLBACK_AFTER_TICKS {
                self.set_fallback(true);
            }
        }
    }

    // ---- other monitors ("Show widget on") -----------------------------------------------------

    /// Keeps one copy of the strip on the taskbar of each extra monitor the user chose (left to
    /// right), next to that taskbar's clock. Taskbars of other monitors exist only when Windows
    /// shows the taskbar on all displays.
    fn sync_mirrors(&mut self) {
        if self.fallback || self.strip.0.is_null() {
            self.drop_mirrors();
            return;
        }
        let (bars, hide_primary) = self.chosen_bars();
        self.set_primary_hidden(hide_primary);
        // Drop copies whose taskbar is gone or no longer wanted.
        let mut keep = Vec::new();
        for (bar, strip) in std::mem::take(&mut self.mirrors) {
            let alive = unsafe { bars.contains(&bar) && IsWindow(Some(strip)).as_bool() && GetParent(strip).is_ok_and(|p| p == bar) };
            if alive {
                keep.push((bar, strip));
            } else if is_our_strip(strip) {
                unsafe {
                    let _ = DestroyWindow(strip);
                }
            }
        }
        self.mirrors = keep;
        for bar in bars {
            if self.mirrors.iter().any(|m| m.0 == bar) {
                continue;
            }
            let Some(strip) = create_strip(bar) else { continue };
            dbg_log(&format!("mirror strip {strip:?} on {bar:?}"));
            self.mirrors.push((bar, strip));
            self.push(strip);
        }
        // Stray copies (e.g. left by an Explorer restart) on any secondary taskbar.
        for bar in secondary_taskbars() {
            unsafe {
                let mut after: Option<HWND> = None;
                while let Ok(h) = FindWindowExW(Some(bar), after, STRIP_CLASS, None) {
                    if h.0.is_null() {
                        break;
                    }
                    after = Some(h);
                    if !self.mirrors.iter().any(|m| m.1 == h) && is_our_strip(h) {
                        let _ = DestroyWindow(h);
                    }
                }
            }
        }
        for i in 0..self.mirrors.len() {
            let (bar, strip) = self.mirrors[i];
            self.place_mirror(bar, strip);
        }
    }

    /// Taskbars of other monitors that show a copy of the widget, and whether the main taskbar's
    /// strip is hidden (the widget is shown only on one other monitor that is connected).
    fn chosen_bars(&self) -> (Vec<HWND>, bool) {
        let others = secondary_taskbars();
        match self.choice.as_str() {
            "all" => (others, false),
            "main" => (Vec::new(), false),
            id => {
                let ids: Vec<String> = others.iter().map(|b| monitor_id(*b)).collect();
                match find_monitor(&ids, id) {
                    Some(i) => (vec![others[i]], true),
                    None => (Vec::new(), false), // that monitor is not connected: main taskbar meanwhile
                }
            }
        }
    }

    /// Hides the main taskbar's strip while the widget is shown only on another monitor.
    fn set_primary_hidden(&mut self, hide: bool) {
        if self.strip.0.is_null() {
            self.primary_hidden = hide;
            return;
        }
        unsafe {
            if hide {
                if IsWindowVisible(self.strip).as_bool() {
                    let _ = ShowWindow(self.strip, SW_HIDE);
                }
                if !self.primary_hidden {
                    self.hide_tip();
                    // Windows 10 layout: give the task buttons their full width back.
                    if let (Ok(rebar), Ok(tray)) = (
                        FindWindowExW(Some(self.taskbar), None, w!("ReBarWindow32"), None),
                        FindWindowExW(Some(self.taskbar), None, w!("TrayNotifyWnd"), None),
                    ) {
                        let (mut rr, mut tr) = (RECT::default(), RECT::default());
                        let _ = GetWindowRect(rebar, &mut rr);
                        let _ = GetWindowRect(tray, &mut tr);
                        let mut tl = POINT { x: rr.left, y: rr.top };
                        let mut tp = POINT { x: tr.left, y: tr.top };
                        let _ = ScreenToClient(self.taskbar, &mut tl);
                        let _ = ScreenToClient(self.taskbar, &mut tp);
                        if tp.x > tl.x {
                            let _ = MoveWindow(rebar, tl.x, tl.y, tp.x - tl.x, rr.bottom - rr.top, true);
                        }
                    }
                }
            } else if self.primary_hidden {
                self.last_place = Default::default();
                self.primary_hidden = false;
                self.place();
            }
        }
        self.primary_hidden = hide;
    }

    fn drop_mirrors(&mut self) {
        self.set_primary_hidden(false);
        for (_, strip) in std::mem::take(&mut self.mirrors) {
            if is_our_strip(strip) {
                unsafe {
                    let _ = DestroyWindow(strip);
                }
            }
        }
    }

    /// Places a copy just left of the secondary taskbar's clock (Windows 10 / ExplorerPatcher
    /// layout), or near its right end when there is no clock window (Windows 11 draws it itself).
    fn place_mirror(&self, bar: HWND, strip: HWND) {
        unsafe {
            let mut br = RECT::default();
            let _ = GetWindowRect(bar, &mut br);
            if br.bottom - br.top > br.right - br.left {
                let _ = ShowWindow(strip, SW_HIDE);
                return; // vertical
            }
            let dpi = GetDpiForWindow(bar);
            let s = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
            let right = match FindWindowExW(Some(bar), None, w!("ClockButton"), None) {
                Ok(clock) if !clock.0.is_null() && IsWindowVisible(clock).as_bool() => {
                    let mut cr = RECT::default();
                    let _ = GetWindowRect(clock, &mut cr);
                    cr.left
                }
                _ => br.right - (130.0 * s) as i32,
            };
            let mut p = POINT { x: right, y: br.top };
            let _ = ScreenToClient(bar, &mut p);
            let mut left = p.x - self.size.cx - (2.0 * s) as i32;
            if self.offset > 0 {
                left -= (self.offset as f32 * s) as i32;
            }
            let mut client = RECT::default();
            let _ = GetClientRect(bar, &mut client);
            let top = ((client.bottom - self.size.cy) / 2).max(0);
            // Narrow the task buttons so they never slide under the copy.
            if let Ok(worker) = FindWindowExW(Some(bar), None, w!("WorkerW"), None) {
                let mut wr = RECT::default();
                let _ = GetWindowRect(worker, &mut wr);
                let mut tl = POINT { x: wr.left, y: wr.top };
                let _ = ScreenToClient(bar, &mut tl);
                if tl.x + (wr.right - wr.left) > left && left > tl.x {
                    let _ = MoveWindow(worker, tl.x, tl.y, left - tl.x, wr.bottom - wr.top, true);
                }
            }
            let mut cur = RECT::default();
            let _ = GetWindowRect(strip, &mut cur);
            let mut cp = POINT { x: cur.left, y: cur.top };
            let _ = ScreenToClient(bar, &mut cp);
            let covered = GetWindow(strip, GW_HWNDPREV).is_ok_and(|h| !h.0.is_null()) || !IsWindowVisible(strip).as_bool();
            let same = cp.x == left && cp.y == top && cur.right - cur.left == self.size.cx && cur.bottom - cur.top == self.size.cy;
            if covered || !same {
                let _ = SetWindowPos(strip, Some(HWND_TOP), left, top, self.size.cx, self.size.cy, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            }
        }
    }

    // ---- notification-area fallback ------------------------------------------------------------

    fn tray_data(&self, uid: u32) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.main,
            uID: uid,
            ..Default::default()
        }
    }

    /// One icon per device with its level drawn in it; the tooltip is the same as on the strip.
    fn sync_tray(&mut self) {
        let light = self.light;
        let normal = if light { rgb(20, 20, 20) } else { rgb(255, 255, 255) };
        let size = unsafe { windows::Win32::UI::HiDpi::GetSystemMetricsForDpi(SM_CXSMICON, windows::Win32::UI::HiDpi::GetDpiForSystem()) };
        let devices = self.visible();
        let mut wanted: Vec<(String, String, COLORREF, i32, String)> = devices
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let th = settings::threshold(SETTINGS_KEY, &d.key);
                let text = if d.percent >= 0 { d.percent.to_string() } else { "?".into() };
                (
                    d.key.clone(),
                    text,
                    ui::level_color(d, light, normal, th),
                    d.percent,
                    self.tip_text(i),
                )
            })
            .collect();
        if wanted.is_empty() {
            wanted.push(("\u{0}none".into(), "-".into(), normal, -1, tr("BatteryHub: no devices found").into()));
        }
        let keep: HashSet<String> = wanted.iter().map(|w| w.0.clone()).collect();
        let stale: Vec<String> = self.tray.keys().filter(|k| !keep.contains(*k)).cloned().collect();
        for k in stale {
            self.remove_tray_icon(&k);
        }
        for (key, text, color, level, tip) in wanted {
            let Some(icon) = ui::number_icon(&text, color, level, size, light) else {
                continue;
            };
            let uid = match self.tray.get(&key) {
                Some((u, _)) => *u,
                None => {
                    self.next_uid += 1;
                    self.next_uid
                }
            };
            let mut d = self.tray_data(uid);
            d.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE | NIF_SHOWTIP;
            d.uCallbackMessage = WM_TRAY;
            d.hIcon = icon;
            ui::fill_wide(&mut d.szTip, &tip);
            unsafe {
                let known = self.tray.contains_key(&key);
                if !known || !Shell_NotifyIconW(NIM_MODIFY, &d).as_bool() {
                    let _ = Shell_NotifyIconW(NIM_ADD, &d);
                }
            }
            if let Some((_, old)) = self.tray.insert(key, (uid, icon)) {
                unsafe {
                    let _ = DestroyIcon(old);
                }
            }
        }
    }

    fn remove_tray_icon(&mut self, key: &str) {
        if let Some((uid, icon)) = self.tray.remove(key) {
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &self.tray_data(uid));
                let _ = DestroyIcon(icon);
            }
        }
    }

    fn remove_tray(&mut self) {
        let keys: Vec<String> = self.tray.keys().cloned().collect();
        for k in keys {
            self.remove_tray_icon(&k);
        }
    }

    /// Keeps the strip just left of the notification area (checked every second, like the original).
    fn place(&mut self) {
        if self.strip.0.is_null() || self.primary_hidden {
            return;
        }
        unsafe {
            let Ok(tray) = FindWindowExW(Some(self.taskbar), None, w!("TrayNotifyWnd"), None) else {
                return;
            };
            let (mut tr, mut bar) = (RECT::default(), RECT::default());
            let _ = GetWindowRect(tray, &mut tr);
            let _ = GetWindowRect(self.taskbar, &mut bar);
            if bar.bottom - bar.top > bar.right - bar.left {
                return; // vertical taskbar: tick() switches to the notification-area fallback
            }
            let mut p = POINT { x: tr.left, y: bar.top };
            let _ = ScreenToClient(self.taskbar, &mut p);
            let mut left = p.x - self.size.cx - (2.0 * self.scale()) as i32;
            // Optional extra gap (DPI-independent px), e.g. to sit next to another taskbar widget.
            if self.offset > 0 {
                left -= (self.offset as f32 * self.scale()) as i32;
            }
            let mut client = RECT::default();
            let _ = GetClientRect(self.taskbar, &mut client);
            let top = ((client.bottom - self.size.cy) / 2).max(0);

            // Windows 10 / ExplorerPatcher taskbar: narrow the task buttons so they never slide under us.
            if let Ok(rebar) = FindWindowExW(Some(self.taskbar), None, w!("ReBarWindow32"), None) {
                let mut rr = RECT::default();
                let _ = GetWindowRect(rebar, &mut rr);
                let mut tl = POINT { x: rr.left, y: rr.top };
                let _ = ScreenToClient(self.taskbar, &mut tl);
                if tl.x + (rr.right - rr.left) > left && left > tl.x {
                    let _ = MoveWindow(rebar, tl.x, tl.y, left - tl.x, rr.bottom - rr.top, true);
                }
            }
            let covered = GetWindow(self.strip, GW_HWNDPREV).is_ok_and(|h| !h.0.is_null()) || !IsWindowVisible(self.strip).as_bool();
            let key = (tr, bar, left);
            if !covered && self.last_place == key {
                return;
            }
            self.last_place = key;
            let _ = SetWindowPos(
                self.strip,
                Some(HWND_TOP),
                left,
                top,
                self.size.cx,
                self.size.cy,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }

    // ---- hover tooltip -------------------------------------------------------------------------

    fn create_tooltip(&mut self) {
        unsafe {
            let Ok(tip) = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                TOOLTIPS_CLASSW,
                None,
                WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP | TTS_NOPREFIX),
                0,
                0,
                0,
                0,
                Some(self.main),
                None,
                GetModuleHandleW(None).ok().map(|m| m.into()),
                None,
            ) else {
                return;
            };
            let mut ti = tool_info(self.main, &mut [0u16]);
            SendMessageW(tip, TTM_ADDTOOLW, None, Some(LPARAM(&mut ti as *mut _ as isize)));
            SendMessageW(tip, TTM_SETMAXTIPWIDTH, None, Some(LPARAM(480)));
            self.tip = tip;
        }
    }

    /// Text shown when hovering a device, short: "PRO 2 LIGHTSPEED: 96%" (+ charging / asleep).
    fn tip_text(&self, i: usize) -> String {
        let devices = self.visible();
        let Some(d) = devices.get(i) else {
            return tr("BatteryHub: no devices found").into();
        };
        let level = d.level();
        let numeric = level.ends_with('%');
        let state = if d.asleep {
            tr(" (asleep)")
        } else if d.is_full() && numeric {
            tr(" (full)")
        } else if d.charging && numeric {
            tr(" (charging)")
        } else {
            ""
        };
        format!("{}: {level}{state}", d.name)
    }

    fn hover(&mut self, on: HWND, x: i32) {
        if self.tip.0.is_null() {
            return;
        }
        if on != self.tip_on {
            self.tip_on = on;
            self.tracking = false;
            self.tip_for = None;
        }
        unsafe {
            if !self.tracking {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: on,
                    dwHoverTime: 0,
                };
                self.tracking = TrackMouseEvent(&mut tme).is_ok();
            }
            let hit = self.hits.iter().find(|h| x >= h.0 && x < h.1).map(|h| (h.2, h.0, h.1));
            let key = hit.map(|h| h.0).unwrap_or(usize::MAX);
            if self.tip_for == Some(key) {
                return;
            }
            self.tip_for = Some(key);
            // Same colours as the taskbar (re-checked each time, the theme can change at any time).
            ui::set_tooltip_theme(self.tip, !self.light);
            let mut text: Vec<u16> = self.tip_text(key).encode_utf16().chain(std::iter::once(0)).collect();
            let mut ti = tool_info(self.main, &mut text);
            SendMessageW(self.tip, TTM_UPDATETIPTEXTW, None, Some(LPARAM(&mut ti as *mut _ as isize)));
            SendMessageW(
                self.tip,
                TTM_TRACKACTIVATE,
                Some(WPARAM(1)),
                Some(LPARAM(&mut ti as *mut _ as isize)),
            );
            // Above the strip, centred on the hovered device.
            let size = SendMessageW(self.tip, TTM_GETBUBBLESIZE, None, Some(LPARAM(&mut ti as *mut _ as isize))).0;
            let (tw, th) = ((size & 0xFFFF) as i32, ((size >> 16) & 0xFFFF) as i32);
            let mut rc = RECT::default();
            let _ = GetWindowRect(on, &mut rc);
            let (from, to) = hit.map(|h| (h.1, h.2)).unwrap_or((0, self.size.cx));
            let cx = rc.left + (from + to) / 2 - tw / 2;
            let cy = rc.top - th - 4;
            let pos = ((cy as u32 & 0xFFFF) << 16) | (cx as u32 & 0xFFFF);
            SendMessageW(self.tip, TTM_TRACKPOSITION, None, Some(LPARAM(pos as isize)));
        }
    }

    fn hide_tip(&mut self) {
        self.tracking = false;
        self.tip_for = None;
        if !self.tip.0.is_null() {
            unsafe {
                let mut ti = tool_info(self.main, &mut [0u16]);
                SendMessageW(
                    self.tip,
                    TTM_TRACKACTIVATE,
                    Some(WPARAM(0)),
                    Some(LPARAM(&mut ti as *mut _ as isize)),
                );
            }
        }
    }

    // ---- polling -------------------------------------------------------------------------------

    fn start_poll(&mut self) {
        if self.polling {
            // A read (full or quick) is running: if this one is for a device change, do it right
            // after, instead of losing it.
            if self.devchange || self.forget {
                self.pending_change = true;
            }
            return;
        }
        self.polling = true;
        // Enumerate the devices again only when something may have changed; a periodic read
        // reuses the device list (and still notices added / removed HID devices).
        let rescan = !self.scanned || self.devchange || self.forget;
        self.scanned = true;
        dbg_log(&format!(
            "poll started (device change: {}, rescan: {rescan}, interval: {} min)",
            self.devchange,
            refresh_minutes()
        ));
        ui::poll_async(self.main, WM_POLL_DONE, rescan);
    }

    /// Quick read of headset dongles and of receivers whose device is off or asleep (switching a
    /// headset on / off or waking
    /// a mouse is not a device change for Windows).
    fn start_quick(&mut self) {
        if self.polling || !refresh_on_change() {
            return;
        }
        let mut ids: HashSet<(u16, u16)> = self.offline_hw.clone();
        ids.extend(self.devices.values().filter(|t| t.status.asleep).filter_map(|t| t.status.hw));
        // Headsets on a 2.4 GHz dongle are switched off and on all the time, and the dongle stays
        // plugged in, so Windows reports nothing: check them too (one short status report each).
        ids.extend(
            self.devices
                .values()
                .filter(|t| t.status.kind == DeviceKind::Headset)
                .filter_map(|t| t.status.hw),
        );
        if ids.is_empty() {
            return;
        }
        self.polling = true;
        let ids: Vec<(u16, u16)> = ids.into_iter().collect();
        dbg_log(&format!("quick read of {ids:04X?}"));
        let ids2 = ids.clone();
        QUICK_IDS.with(|q| *q.borrow_mut() = ids);
        ui::run_async(self.main, WM_QUICK_DONE, move || batteryhub::device::poll_hardware(&ids2));
    }

    /// Merges a quick read: only devices behind the receivers in `ids` are updated or removed.
    fn on_quick_result(&mut self, fresh: Vec<DeviceStatus>, ids: &[(u16, u16)]) {
        self.polling = false;
        let keys: HashSet<String> = fresh.iter().map(|s| s.key.clone()).collect();
        self.devices.retain(|k, t| {
            let Some(hw) = t.status.hw else { return true };
            if !ids.contains(&hw) || keys.contains(k) {
                return true;
            }
            let present = batteryhub::device::hid_present(hw);
            match batteryhub::device::missing_device(Some(hw), present, t.status.grace_sec, t.seen.elapsed().as_secs(), MIN_GRACE_SEC) {
                Missing::Remove => false,
                Missing::Asleep => {
                    t.status.asleep = true;
                    true
                }
                Missing::Keep => true,
            }
        });
        for id in ids {
            self.offline_hw.remove(id);
        }
        self.merge(fresh);
        if std::mem::take(&mut self.pending_change) {
            self.devchange = true;
            self.start_poll();
        }
    }

    /// Adds / updates the devices of a read; devices reporting "off" are removed and their
    /// receiver is remembered for quick checks.
    fn merge(&mut self, fresh: Vec<DeviceStatus>) {
        let now = Instant::now();
        for s in fresh {
            if !self.order.contains(&s.key) {
                self.order.push(s.key.clone());
            }
            if !s.connected {
                if let Some(hw) = s.hw {
                    self.offline_hw.insert(hw);
                }
                self.devices.remove(&s.key);
                continue;
            }
            self.devices.insert(s.key.clone(), Tracked { status: s, seen: now });
        }
        self.order.retain(|k| self.devices.contains_key(k));
        self.paint();
    }

    /// Menu refresh: `devices` = full rescan that drops devices that are gone right away;
    /// otherwise only the battery levels are read again (devices keep their grace period).
    fn manual_refresh(&mut self, devices: bool) {
        self.forget |= devices;
        self.manual = true;
        self.start_poll();
        self.paint(); // shows the ↻ indicator
    }

    fn on_result(&mut self, fresh: Vec<DeviceStatus>) {
        self.polling = false;
        self.loaded = true;
        self.manual = false;
        self.devchange = false;
        if std::mem::take(&mut self.forget) {
            self.devices.clear();
            self.order.clear();
        }
        let keys: HashSet<String> = fresh.iter().map(|s| s.key.clone()).collect();
        // A device missing from this read:
        //  * read through a receiver / cable that is gone (unplugged): removed at once;
        //  * read through hardware that is still there: the device itself sleeps or missed one
        //    answer - keep the last value (dimmed while it sleeps) for its grace period;
        //  * Bluetooth / BLE / XInput devices are only reported while connected, so missing
        //    means disconnected: removed at once.
        self.devices.retain(|k, t| {
            if keys.contains(k) {
                return true;
            }
            let present = t.status.hw.is_some_and(batteryhub::device::hid_present);
            match batteryhub::device::missing_device(t.status.hw, present, t.status.grace_sec, t.seen.elapsed().as_secs(), MIN_GRACE_SEC) {
                Missing::Remove => false,
                Missing::Asleep => {
                    t.status.asleep = true;
                    true
                }
                Missing::Keep => true,
            }
        });
        self.offline_hw.clear();
        self.merge(fresh);
        if std::mem::take(&mut self.pending_change) {
            self.devchange = true;
            self.start_poll();
        }
    }
}

impl App {
    /// Re-reads the taskbar theme and TaskbarOffset (start-up and WM_SETTINGCHANGE only).
    fn refresh_settings(&mut self) {
        self.light = ui::taskbar_is_light();
        self.offset = batteryhub::util::reg_get_dword(SETTINGS_KEY, "TaskbarOffset").unwrap_or(0);
        self.offset_y = batteryhub::util::reg_get_dword(SETTINGS_KEY, "TaskbarOffsetY").unwrap_or(0) as i32;
        self.background = batteryhub::util::reg_get_dword(SETTINGS_KEY, "WidgetBackground").unwrap_or(0) != 0;
        self.choice = widget_choice();
    }
}

// ---- check for updates ---------------------------------------------------------------------

/// A check or download is running (the menu item is greyed out meanwhile).
static UPDATE_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const UPDATE_TITLE: &str = "BatteryHub - Check for updates";

/// Runs `work` on a worker thread and posts its boxed result to `hwnd` as `msg`.
fn post_from_thread<T: Send + 'static>(hwnd: HWND, msg: u32, work: impl FnOnce() -> T + Send + 'static) {
    let h = hwnd.0 as usize;
    std::thread::spawn(move || {
        let ptr = Box::into_raw(Box::new(work()));
        unsafe {
            if PostMessageW(Some(HWND(h as *mut c_void)), msg, WPARAM(0), LPARAM(ptr as isize)).is_err() {
                drop(Box::from_raw(ptr));
            }
        }
    });
}

/// The running check was started automatically: only "a new update is available" is shown.
static UPDATE_SILENT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The running download is a silent install: no message boxes, the setup runs without a window.
static UPDATE_QUIET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check_for_updates(main: HWND, silent: bool) {
    if UPDATE_BUSY.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    UPDATE_SILENT.store(silent, std::sync::atomic::Ordering::Relaxed);
    post_from_thread(main, WM_UPDATE_CHECKED, batteryhub::update::check);
}

fn message(owner: HWND, text: &str, style: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    unsafe {
        let _ = SetForegroundWindow(owner);
        MessageBoxW(
            Some(owner),
            &windows::core::HSTRING::from(text),
            &windows::core::HSTRING::from(tr(UPDATE_TITLE)),
            style | MB_SETFOREGROUND | MB_TOPMOST,
        )
    }
}

fn on_update_checked(main: HWND, check: batteryhub::update::Check) {
    use batteryhub::install::{label_of, version_label};
    use batteryhub::update::Check;
    trim_memory();
    let silent = UPDATE_SILENT.swap(false, std::sync::atomic::Ordering::Relaxed);
    if silent && !matches!(check, Check::Available(_)) {
        UPDATE_BUSY.store(false, std::sync::atomic::Ordering::Relaxed);
        if matches!(check, Check::Failed(_)) && AUTO_UPDATE_TRIES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < AUTO_UPDATE_RETRIES {
            unsafe {
                SetTimer(Some(main), TIMER_AUTO_UPDATE, AUTO_UPDATE_RETRY_MS, None);
            }
        }
        return;
    }
    match check {
        Check::UpToDate { latest } => {
            UPDATE_BUSY.store(false, std::sync::atomic::Ordering::Relaxed);
            message(
                main,
                &trf(
                    "No new updates.\n\nYou have the latest version ({0}).\nLatest release on GitHub: {1}.",
                    &[&version_label(), &label_of(&latest)],
                ),
                MB_OK | MB_ICONINFORMATION,
            );
        }
        Check::Failed(e) => {
            UPDATE_BUSY.store(false, std::sync::atomic::Ordering::Relaxed);
            message(
                main,
                &trf(
                    "Could not check for updates:\n{0}\n\nNew versions are also on GitHub:\nhttps://github.com/{1}/releases",
                    &[&e, &batteryhub::update::REPO],
                ),
                MB_OK | MB_ICONWARNING,
            );
        }
        // Automatic check with "Install updates silently": download and install, show nothing.
        Check::Available(r) if silent && silent_install() => {
            UPDATE_QUIET.store(true, std::sync::atomic::Ordering::Relaxed);
            post_from_thread(main, WM_UPDATE_DOWNLOADED, move || batteryhub::update::download(&r));
        }
        Check::Available(r) => {
            let text = trf(
                "A new update is available:\n\n        {0}  ->  {1}\n\nDo you want to update now?\n\nThe setup is downloaded from GitHub, checked against the release's SHA-256 checksum and started; it keeps your settings and autostart.",
                &[&version_label(), &label_of(&r.version)],
            );
            if message(main, &text, MB_YESNO | MB_ICONQUESTION) == IDYES {
                post_from_thread(main, WM_UPDATE_DOWNLOADED, move || batteryhub::update::download(&r));
            } else {
                UPDATE_BUSY.store(false, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
}

fn on_update_downloaded(main: HWND, result: Result<std::path::PathBuf, String>) {
    UPDATE_BUSY.store(false, std::sync::atomic::Ordering::Relaxed);
    trim_memory();
    if UPDATE_QUIET.swap(false, std::sync::atomic::Ordering::Relaxed) {
        // No console window (CREATE_NO_WINDOW) and no "Update finished" box afterwards. A failed
        // download or start is not shown either; the next start tries again.
        if let Ok(setup) = result {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new(&setup).args(["/update", "/S", "/nonotify"]).creation_flags(0x0800_0000).spawn();
        }
        return;
    }
    match result {
        // The setup closes this program (and starts the new version) itself.
        Ok(setup) => {
            if let Err(e) = std::process::Command::new(&setup).arg("/update").spawn() {
                message(
                    main,
                    &trf("Could not start the setup:\n{0}\n\n{1}", &[&e, &setup.display()]),
                    MB_OK | MB_ICONWARNING,
                );
            }
        }
        Err(e) => {
            message(main, &trf("The update could not be downloaded:\n{0}", &[&e]), MB_OK | MB_ICONWARNING);
        }
    }
}

/// Hands memory that was only needed for a moment (a read, a Bluetooth check, a render) back to
/// Windows, so between those moments the program keeps well under 1 MB resident. Pages it needs
/// again come back from the standby list (soft faults), which costs microseconds.
fn trim_memory() {
    unsafe {
        let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

/// Battery refresh interval in minutes (menu choice, default 1 minute).
fn refresh_minutes() -> u32 {
    batteryhub::util::reg_get_dword(SETTINGS_KEY, "RefreshMinutes")
        .filter(|m| INTERVALS.iter().any(|(v, _)| v == m))
        .unwrap_or(1)
}

/// Refresh when a device connects or disconnects (default on). Read from the registry once and
/// cached (it is checked every few seconds); the menu updates the cache when it is toggled.
static ON_CHANGE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2); // 2 = not read yet

fn refresh_on_change() -> bool {
    use std::sync::atomic::Ordering;
    match ON_CHANGE.load(Ordering::Relaxed) {
        2 => {
            let on = batteryhub::util::reg_get_dword(SETTINGS_KEY, "RefreshOnDeviceChange").unwrap_or(1) != 0;
            ON_CHANGE.store(on as u8, Ordering::Relaxed);
            on
        }
        v => v != 0,
    }
}

/// Check for updates when BatteryHub starts (default off).
fn auto_update() -> bool {
    batteryhub::util::reg_get_dword(SETTINGS_KEY, "AutoCheckUpdates").unwrap_or(0) != 0
}

/// With the automatic check: install a new version in the background without asking (default off).
fn silent_install() -> bool {
    batteryhub::util::reg_get_dword(SETTINGS_KEY, "SilentUpdates").unwrap_or(0) != 0
}

fn autostart_name() -> &'static str {
    batteryhub::install::TASKBAR.run_value.unwrap_or("BatteryHub Taskbar")
}

fn save_dword(name: &str, value: u32) {
    if let Some(k) = batteryhub::install::create(batteryhub::install::HKCU, SETTINGS_KEY) {
        k.set_dword(name, value);
    }
}

/// Creates a (hidden) strip window as a child of a taskbar.
fn create_strip(taskbar: HWND) -> Option<HWND> {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let r = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_NOACTIVATE,
            STRIP_CLASS,
            w!(""),
            WS_CHILD | WS_CLIPSIBLINGS,
            0,
            0,
            1,
            1,
            Some(taskbar),
            None,
            Some(hinst.into()),
            None,
        );
        if r.is_err() {
            dbg_log(&format!("CreateWindowExW failed: {:?}", windows::Win32::Foundation::GetLastError()));
        }
        r.ok()
    }
}

/// Taskbars of the other monitors, left to right.
fn secondary_taskbars() -> Vec<HWND> {
    let mut out: Vec<(i32, i32, HWND)> = Vec::new();
    unsafe {
        let mut after: Option<HWND> = None;
        while let Ok(h) = FindWindowExW(None, after, w!("Shell_SecondaryTrayWnd"), None) {
            if h.0.is_null() {
                break;
            }
            after = Some(h);
            if !IsWindowVisible(h).as_bool() {
                continue;
            }
            let mut r = RECT::default();
            let _ = GetWindowRect(h, &mut r);
            out.push((r.left, r.top, h));
        }
    }
    out.sort_by_key(|o| (o.0, o.1));
    out.into_iter().map(|o| o.2).collect()
}

/// "Show widget on": "all", "main" (default) or the id of one other monitor. v1.2 stored a number
/// of monitors instead (1 = main only, more = all).
fn widget_choice() -> String {
    if let Some(v) = batteryhub::util::reg_get_string(SETTINGS_KEY, "WidgetMonitor").filter(|v| !v.is_empty()) {
        return v;
    }
    match batteryhub::util::reg_get_dword(SETTINGS_KEY, "WidgetMonitors") {
        Some(n) if n >= 2 => "all".into(),
        _ => "main".into(),
    }
}

fn monitor_info(bar: HWND) -> Option<MONITORINFOEXW> {
    unsafe {
        let mon = MonitorFromWindow(bar, MONITOR_DEFAULTTONULL);
        if mon.is_invalid() {
            return None;
        }
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        GetMonitorInfoW(mon, &mut mi.monitorInfo as *mut MONITORINFO).as_bool().then_some(mi)
    }
}

/// A monitor's lasting identity: its device interface name (survives reboots, Explorer restarts and
/// Windows renumbering the displays), else "\\.\DISPLAYn".
fn monitor_id(bar: HWND) -> String {
    let Some(mi) = monitor_info(bar) else { return String::new() };
    let device = batteryhub::util::from_wide(&mi.szDevice);
    unsafe {
        let mut dd = DISPLAY_DEVICEW { cb: size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
        if EnumDisplayDevicesW(&windows::core::HSTRING::from(device.as_str()), 0, &mut dd, 1 /* EDD_GET_DEVICE_INTERFACE_NAME */).as_bool() {
            let id = batteryhub::util::from_wide(&dd.DeviceID);
            if !id.is_empty() {
                return id;
            }
        }
    }
    device
}

/// "Display 2 (left)": the number Windows gives the display and where it is next to the main one.
fn monitor_label(bar: HWND) -> String {
    let Some(mi) = monitor_info(bar) else { return tr("Other monitor").into() };
    let device = batteryhub::util::from_wide(&mi.szDevice);
    let mut label = match device.find("DISPLAY") {
        Some(i) => trf("Display {0}", &[&&device[i + 7..]]),
        None => tr("Other monitor").into(),
    };
    unsafe {
        let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut pm = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(mon, &mut pm).as_bool() {
            let (m, p) = (mi.monitorInfo.rcMonitor, pm.rcMonitor);
            let dx = (m.left + m.right - p.left - p.right) / 2;
            let dy = (m.top + m.bottom - p.top - p.bottom) / 2;
            let side = if dx.abs() >= dy.abs() {
                if dx < 0 { "left" } else { "right" }
            } else if dy < 0 {
                "above"
            } else {
                "below"
            };
            label += &format!(" ({})", tr(side));
        }
    }
    label
}

/// The model part of a monitor id ("AUO7EAD" in \\?\DISPLAY#AUO7EAD#4&...#{...}).
fn monitor_model(id: &str) -> &str {
    let Some(a) = id.find("DISPLAY#").map(|a| a + 8) else { return "" };
    id[a..].find('#').map(|b| &id[a..a + b]).unwrap_or("")
}

/// Index of the chosen monitor among `ids`, or None when it is not connected. A saved id that
/// matches none exactly still matches the only connected monitor of the same model (a laptop
/// panel's id changes when a MUX switch moves it to the other GPU).
fn find_monitor(ids: &[String], choice: &str) -> Option<usize> {
    if let Some(i) = ids.iter().position(|i| i == choice) {
        return Some(i);
    }
    let model = monitor_model(choice);
    if model.is_empty() {
        return None;
    }
    let same: Vec<usize> = (0..ids.len()).filter(|&i| monitor_model(&ids[i]) == model).collect();
    (same.len() == 1).then(|| same[0])
}

/// A strip window created by this process (a handle that outlived Explorer may have been reused by
/// someone else's window, which must never be destroyed).
fn is_our_strip(h: HWND) -> bool {
    unsafe {
        if h.0.is_null() || !IsWindow(Some(h)).as_bool() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        let mut class = [0u16; 64];
        let n = GetClassNameW(h, &mut class) as usize;
        pid == std::process::id() && String::from_utf16_lossy(&class[..n]) == "BatteryHub.Taskbar.Strip"
    }
}

fn tool_info(owner: HWND, text: &mut [u16]) -> TTTOOLINFOW {
    TTTOOLINFOW {
        cbSize: size_of::<TTTOOLINFOW>() as u32,
        uFlags: TTF_TRACK | TTF_ABSOLUTE,
        hwnd: owner,
        uId: 1,
        lpszText: windows::core::PWSTR(text.as_mut_ptr()),
        ..Default::default()
    }
}

fn dbg_log(msg: &str) {
    if std::env::var_os("BH_DEBUG").is_some() {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(std::env::temp_dir().join("bh2.log"))
        {
            let _ = writeln!(f, "{msg}");
        }
    }
}

/// The right-click menu (strip and fallback icons): per-device warning levels, refresh options, exit.
unsafe fn show_menu() {
    unsafe {
        let (main, devices) = with_app(|a| {
            a.hide_tip();
            (a.main, a.visible())
        })
        .unwrap_or_default();
        if let Ok(menu) = CreatePopupMenu() {
            // One submenu per device: its charge state and the red warning level (50 ... 5%).
            settings::append_device_menus(menu, &devices, SETTINGS_KEY);
            if !devices.is_empty() {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, windows::core::PCWSTR::null());
            }
            fn s(t: &str) -> windows::core::HSTRING {
                windows::core::HSTRING::from(t)
            }
            let _ = AppendMenuW(menu, MF_STRING, ID_REFRESH_DEVICES, &s(tr("Refresh devices")));
            let _ = AppendMenuW(menu, MF_STRING, ID_REFRESH_LEVELS, &s(tr("Refresh battery levels")));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, windows::core::PCWSTR::null());
            // How often the battery levels are read, and whether a device change triggers a read.
            if let Ok(sub) = CreatePopupMenu() {
                let current = refresh_minutes();
                for (j, (m, label)) in INTERVALS.iter().enumerate() {
                    let check = if *m == current { MF_CHECKED } else { MF_UNCHECKED };
                    let _ = AppendMenuW(sub, MF_STRING | check, ID_INTERVAL_BASE + j, &s(tr(*label)));
                }
                let _ = AppendMenuW(menu, MF_STRING | MF_POPUP, sub.0 as usize, &s(tr("Refresh battery every")));
            }
            let on_change = if refresh_on_change() { MF_CHECKED } else { MF_UNCHECKED };
            let _ = AppendMenuW(
                menu,
                MF_STRING | on_change,
                ID_ON_CHANGE,
                &s(tr("Refresh when a device connects or disconnects")),
            );
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, windows::core::PCWSTR::null());
            let check = |on: bool| if on { MF_CHECKED } else { MF_UNCHECKED };
            // Look of the widget: plate behind it, position, and which monitors show it.
            let (choice, offset, background, offset_y) = with_app(|a| (a.choice.clone(), a.offset, a.background, a.offset_y)).unwrap_or_default();
            let _ = AppendMenuW(
                menu,
                MF_STRING | check(background),
                ID_BACKGROUND,
                &s(tr("Background behind taskbar text (for translucent taskbars)")),
            );
            if let Ok(sub) = CreatePopupMenu() {
                let mut checked = ID_POSITION_CUSTOM;
                for (i, px) in POSITION_OFFSETS.iter().enumerate() {
                    let label = if *px == 0 { tr("Next to the notification area").to_string() } else { trf("{0} px to the left", &[px]) };
                    let _ = AppendMenuW(sub, MF_STRING, ID_POSITION_FIRST + i, &s(&label));
                    if *px == offset {
                        checked = ID_POSITION_FIRST + i;
                    }
                }
                if checked == ID_POSITION_CUSTOM {
                    // A value set by hand in the registry (TaskbarOffset).
                    let label = trf("Custom ({0} px, TaskbarOffset in the registry)", &[&offset]);
                    let _ = AppendMenuW(sub, MF_STRING | MF_GRAYED, ID_POSITION_CUSTOM, &s(&label));
                }
                let _ = CheckMenuRadioItem(sub, ID_POSITION_FIRST as u32, ID_POSITION_CUSTOM as u32, checked as u32, MF_BYCOMMAND.0);
                let _ = AppendMenuW(menu, MF_STRING | MF_POPUP, sub.0 as usize, &s(tr("Position on the taskbar")));
            }
            if let Ok(sub) = CreatePopupMenu() {
                let mut checked = ID_HEIGHT_CUSTOM;
                for (i, px) in HEIGHT_OFFSETS.iter().enumerate() {
                    let label = match *px {
                        0 => tr("Centred").to_string(),
                        p if p < 0 => trf("{0} px up", &[&-p]),
                        p => trf("{0} px down", &[&p]),
                    };
                    let _ = AppendMenuW(sub, MF_STRING, ID_HEIGHT_FIRST + i, &s(&label));
                    if *px == offset_y {
                        checked = ID_HEIGHT_FIRST + i;
                    }
                }
                if checked == ID_HEIGHT_CUSTOM {
                    let label = trf("Custom ({0} px, TaskbarOffsetY in the registry)", &[&offset_y]);
                    let _ = AppendMenuW(sub, MF_STRING | MF_GRAYED, ID_HEIGHT_CUSTOM, &s(&label));
                }
                let _ = CheckMenuRadioItem(sub, ID_HEIGHT_FIRST as u32, ID_HEIGHT_CUSTOM as u32, checked as u32, MF_BYCOMMAND.0);
                let _ = AppendMenuW(menu, MF_STRING | MF_POPUP, sub.0 as usize, &s(tr("Height on the taskbar")));
            }
            // With more than one monitor: all of them, the main one only, or one other monitor only.
            let others: Vec<(String, String)> =
                secondary_taskbars().into_iter().take(ID_MONITOR_GONE - ID_MONITOR_FIRST).map(|b| (monitor_id(b), monitor_label(b))).collect();
            if !others.is_empty() {
                if let Ok(sub) = CreatePopupMenu() {
                    let _ = AppendMenuW(sub, MF_STRING, ID_MONITOR_ALL, &s(tr("All monitors")));
                    let _ = AppendMenuW(sub, MF_STRING, ID_MONITOR_MAIN, &s(tr("Main monitor only")));
                    let ids: Vec<String> = others.iter().map(|o| o.0.clone()).collect();
                    let checked = match choice.as_str() {
                        "all" => ID_MONITOR_ALL,
                        "main" => ID_MONITOR_MAIN,
                        id => find_monitor(&ids, id).map(|i| ID_MONITOR_FIRST + i).unwrap_or(ID_MONITOR_GONE),
                    };
                    for (i, (_, label)) in others.iter().enumerate() {
                        let _ = AppendMenuW(sub, MF_STRING, ID_MONITOR_FIRST + i, &s(&trf("{0} only", &[label])));
                    }
                    if checked == ID_MONITOR_GONE {
                        let _ = AppendMenuW(sub, MF_STRING | MF_GRAYED, ID_MONITOR_GONE, &s(tr("Chosen monitor (not connected, shown on main)")));
                    }
                    let _ = CheckMenuRadioItem(sub, ID_MONITOR_ALL as u32, ID_MONITOR_GONE as u32, checked as u32, MF_BYCOMMAND.0);
                    let _ = AppendMenuW(menu, MF_STRING | MF_POPUP, sub.0 as usize, &s(tr("Show widget on")));
                }
            }
            // Interface language: English / Lietuvių (each named in its own language).
            if let Ok(sub) = CreatePopupMenu() {
                let current = i18n::current();
                for (i, l) in i18n::Lang::ALL.iter().enumerate() {
                    let _ = AppendMenuW(sub, MF_STRING, ID_LANG_FIRST + i, &s(l.native_name()));
                    if *l == current {
                        let id = (ID_LANG_FIRST + i) as u32;
                        let _ = CheckMenuRadioItem(sub, id, id, id, MF_BYCOMMAND.0);
                    }
                }
                let _ = AppendMenuW(menu, MF_STRING | MF_POPUP, sub.0 as usize, &s(tr("Language")));
            }
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, windows::core::PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING | check(ui::autostart_enabled(autostart_name())), ID_AUTOSTART, &s(tr("Automatically start with Windows")));
            let _ = AppendMenuW(menu, MF_STRING | check(auto_update()), ID_AUTO_UPDATE, &s(tr("Automatically check for updates")));
            let _ = AppendMenuW(
                menu,
                MF_STRING | check(silent_install()) | if auto_update() { MF_ENABLED } else { MF_GRAYED },
                ID_SILENT_UPDATE,
                &s(tr("Install updates silently")),
            );
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, windows::core::PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, ID_REQUEST, &s(tr("Request support for a device...")));
            let busy = UPDATE_BUSY.load(std::sync::atomic::Ordering::Relaxed);
            let label = if busy { "Checking for updates..." } else { "Check for updates..." };
            let _ = AppendMenuW(
                menu,
                MF_STRING | if busy { MF_GRAYED } else { MF_ENABLED },
                ID_CHECK_UPDATES,
                &s(tr(label)),
            );
            let _ = AppendMenuW(menu, MF_STRING, ID_EXIT, &s(tr("Exit")));
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            // Menu colours follow the taskbar: dark on a dark taskbar, light on a light one.
            ui::set_menu_theme(main, !ui::taskbar_is_light());
            let _ = SetForegroundWindow(main);
            let cmd = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
                pt.x,
                pt.y,
                None,
                main,
                None,
            );
            let _ = PostMessageW(Some(main), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(menu);
            match cmd.0 as usize {
                ID_EXIT => {
                    let _ = DestroyWindow(main);
                }
                ID_REFRESH_DEVICES => {
                    with_app(|a| a.manual_refresh(true));
                }
                ID_REFRESH_LEVELS => {
                    with_app(|a| a.manual_refresh(false));
                }
                ID_CHECK_UPDATES => check_for_updates(main, false),
                ID_BACKGROUND => {
                    with_app(|a| {
                        a.background = !a.background;
                        save_dword("WidgetBackground", a.background as u32);
                        a.paint();
                    });
                }
                id if (ID_POSITION_FIRST..ID_POSITION_FIRST + POSITION_OFFSETS.len()).contains(&id) => {
                    let px = POSITION_OFFSETS[id - ID_POSITION_FIRST];
                    if px == 0 {
                        batteryhub::util::reg_delete_value(SETTINGS_KEY, "TaskbarOffset");
                    } else {
                        save_dword("TaskbarOffset", px);
                    }
                    with_app(|a| {
                        a.offset = px;
                        a.last_place = Default::default();
                        a.place();
                        a.sync_mirrors();
                    });
                }
                id if (ID_HEIGHT_FIRST..ID_HEIGHT_FIRST + HEIGHT_OFFSETS.len()).contains(&id) => {
                    let px = HEIGHT_OFFSETS[id - ID_HEIGHT_FIRST];
                    if px == 0 {
                        batteryhub::util::reg_delete_value(SETTINGS_KEY, "TaskbarOffsetY");
                    } else {
                        save_dword("TaskbarOffsetY", px as u32);
                    }
                    with_app(|a| {
                        a.offset_y = px;
                        a.paint();
                        a.sync_mirrors();
                    });
                }
                id if id == ID_MONITOR_ALL || id == ID_MONITOR_MAIN || (ID_MONITOR_FIRST..ID_MONITOR_FIRST + others.len()).contains(&id) => {
                    let v = match id {
                        ID_MONITOR_ALL => "all".to_string(),
                        ID_MONITOR_MAIN => "main".to_string(),
                        i => others[i - ID_MONITOR_FIRST].0.clone(),
                    };
                    batteryhub::util::reg_set_string(SETTINGS_KEY, "WidgetMonitor", &v);
                    with_app(|a| {
                        a.choice = v;
                        a.sync_mirrors();
                    });
                }
                ID_AUTOSTART => {
                    let on = !ui::autostart_enabled(autostart_name());
                    ui::set_autostart(autostart_name(), on);
                    if on {
                        // A "disabled" state left in Task Manager would keep it from starting.
                        batteryhub::util::reg_delete_value(batteryhub::install::STARTUP_APPROVED_KEY, autostart_name());
                    }
                }
                ID_AUTO_UPDATE => {
                    let on = !auto_update();
                    save_dword("AutoCheckUpdates", on as u32);
                    if on {
                        message(
                            main,
                            tr("Automatic update check is on.\n\nBatteryHub looks for a new version only when it starts (at sign-in or when you start it), not while it runs. When there is one, it asks you whether to update (or, with \"Install updates silently\", installs it in the background)."),
                            MB_OK | MB_ICONINFORMATION,
                        );
                    }
                }
                ID_SILENT_UPDATE => {
                    let on = !silent_install();
                    save_dword("SilentUpdates", on as u32);
                    if on {
                        message(
                            main,
                            tr("Silent updates are on.\n\nWhen BatteryHub starts and finds a new version, it downloads it from GitHub, checks its SHA-256 checksum and installs it in the background, without asking and without showing anything. Your settings and autostart are kept."),
                            MB_OK | MB_ICONINFORMATION,
                        );
                    }
                }
                ID_REQUEST => batteryhub::report_window::show(),
                id if (ID_LANG_FIRST..ID_LANG_FIRST + i18n::Lang::ALL.len()).contains(&id) => {
                    // The menu and messages are built each time, so the new language shows at once;
                    // the tooltip text is refreshed with the next paint.
                    i18n::choose(i18n::Lang::ALL[id - ID_LANG_FIRST]);
                    with_app(|a| a.paint());
                }
                ID_ON_CHANGE => {
                    let on = !refresh_on_change();
                    save_dword("RefreshOnDeviceChange", on as u32);
                    ON_CHANGE.store(on as u8, std::sync::atomic::Ordering::Relaxed);
                }
                id if (ID_INTERVAL_BASE..ID_INTERVAL_BASE + INTERVALS.len()).contains(&id) => {
                    let minutes = INTERVALS[id - ID_INTERVAL_BASE].0;
                    save_dword("RefreshMinutes", minutes);
                    SetTimer(Some(main), TIMER_POLL, minutes * 60_000, None);
                }
                id => {
                    if let Some((dev, level)) = settings::decode_menu_id(id) {
                        if let Some(d) = devices.get(dev) {
                            settings::set_threshold(SETTINGS_KEY, &d.key, level);
                            with_app(|a| a.paint());
                        }
                    }
                }
            }
        }
    }
}

// ---- window procedures -------------------------------------------------------------------------

extern "system" fn strip_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEMOVE => {
                with_app(|a| a.hover(hwnd, (lp.0 & 0xFFFF) as i16 as i32));
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                with_app(|a| a.hide_tip());
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                show_menu();
                LRESULT(0)
            }
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_POLL_DONE => {
                let devices = ui::take_result(lp);
                with_app(|a| a.on_result(devices));
                // Hand pages used only during the read (enumeration, Bluetooth, fonts) back to
                // Windows until the next read, instead of keeping them resident for a minute or more.
                trim_memory();
                LRESULT(0)
            }
            WM_TIMER => {
                with_app(|a| match wp.0 {
                    TIMER_POLL => a.start_poll(),
                    TIMER_DEVCHANGE => {
                        let _ = KillTimer(Some(hwnd), TIMER_DEVCHANGE);
                        a.devchange = true;
                        a.start_poll();
                    }
                    TIMER_QUICK => a.start_quick(),
                    TIMER_UPDATED => {
                        let _ = KillTimer(Some(hwnd), TIMER_UPDATED);
                        if let Some(old) = a.updated_from.take() {
                            let text = trf(
                                "Update finished.\n\nBatteryHub was updated:  {0}  ->  {1}\n\nYour settings and autostart were kept.",
                                &[&batteryhub::install::label_of(&old), &batteryhub::install::version_label()],
                            );
                            post_from_thread(hwnd, WM_SHOW_UPDATED, move || text);
                        }
                    }
                    TIMER_AUTO_UPDATE => {
                        let _ = KillTimer(Some(hwnd), TIMER_AUTO_UPDATE);
                        check_for_updates(hwnd, true);
                    }
                    TIMER_BTWATCH => {
                        // Bluetooth connect / disconnect does not add or remove device nodes, so
                        // Windows sends no device-change message for it: compare a signature.
                        if refresh_on_change() {
                            let sig = batteryhub::drivers::bluetooth::connection_signature();
                            if sig != a.bt_sig {
                                a.bt_sig = sig;
                                SetTimer(Some(hwnd), TIMER_DEVCHANGE, 1500, None);
                            }
                            trim_memory();
                        }
                    }
                    _ => a.tick(),
                });
                LRESULT(0)
            }
            // A device was plugged in / removed (USB receiver, cable, Bluetooth link) or the PC woke
            // up: read again once things settle, if the user left that option on.
            // A device was plugged in or removed (HID interface arrival / removal, or any device
            // node change): read again once things settle, if the user left that option on.
            WM_DEVICECHANGE_MSG if matches!(wp.0, DBT_DEVNODES_CHANGED | DBT_DEVICEARRIVAL | DBT_DEVICEREMOVECOMPLETE) => {
                if refresh_on_change() {
                    SetTimer(Some(hwnd), TIMER_DEVCHANGE, 1500, None);
                }
                LRESULT(1)
            }
            WM_QUICK_DONE => {
                let devices = ui::take_result(lp);
                let ids = QUICK_IDS.with(|q| std::mem::take(&mut *q.borrow_mut()));
                with_app(|a| a.on_quick_result(devices, &ids));
                trim_memory();
                LRESULT(0)
            }
            WM_SHOW_UPDATED => {
                // Shown outside the timer handler, so the app is not borrowed while the box is open.
                let text = *Box::from_raw(lp.0 as *mut String);
                message(hwnd, &text, MB_OK | MB_ICONINFORMATION);
                LRESULT(0)
            }
            WM_UPDATE_CHECKED => {
                let check = *Box::from_raw(lp.0 as *mut batteryhub::update::Check);
                on_update_checked(hwnd, check);
                LRESULT(0)
            }
            WM_UPDATE_DOWNLOADED => {
                let result = *Box::from_raw(lp.0 as *mut Result<std::path::PathBuf, String>);
                on_update_downloaded(hwnd, result);
                LRESULT(0)
            }
            WM_TRAY => {
                if matches!((lp.0 & 0xFFFF) as u32, WM_RBUTTONUP | WM_CONTEXTMENU) {
                    show_menu();
                }
                LRESULT(0)
            }
            WM_POWERBROADCAST if wp.0 == PBT_APMRESUMEAUTOMATIC => {
                if refresh_on_change() {
                    SetTimer(Some(hwnd), TIMER_DEVCHANGE, 5000, None);
                }
                LRESULT(1)
            }
            WM_SETTINGCHANGE | WM_DISPLAYCHANGE => {
                with_app(|a| {
                    a.refresh_settings(); // light/dark taskbar
                    a.paint(); // or DPI changed
                });
                LRESULT(0)
            }
            WM_DESTROY => {
                with_app(|a| {
                    if !a.strip.0.is_null() {
                        let _ = DestroyWindow(a.strip);
                    }
                    a.remove_tray();
                });
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => {
                let tc = with_app(|a| a.taskbar_created).unwrap_or(0);
                if tc != 0 && msg == tc {
                    // Explorer restarted. The 1 s timer may already have put a new strip on the new
                    // taskbar, so the strip is not thrown away blindly (that left a second, orphaned
                    // strip behind): tick() keeps a live one and recreates a dead one. Fallback
                    // icons have to be added again.
                    with_app(|a| {
                        a.last_place = Default::default();
                        a.painted_for = Default::default();
                        a.tray.clear();
                        a.tick();
                        if a.fallback {
                            a.sync_tray();
                        }
                    });
                    return LRESULT(0);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
    }
}

fn main() {
    unsafe {
        // `--report`: only the "Request support for a device" window (also while BatteryHub runs).
        if std::env::args().any(|a| a == "--report") {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            batteryhub::report_window::run_standalone();
            return;
        }
        // `--listen VID:PID`: diagnostic recording of a device's HID reports (see listen.rs).
        let args: Vec<String> = std::env::args().collect();
        if let Some(i) = args.iter().position(|a| a == "--listen") {
            batteryhub::listen::run(args.get(i + 1).map(|s| s.as_str()));
            return;
        }
        let _mutex = CreateMutexW(None, true, w!("Local\\BatteryHub.Taskbar"));
        // One instance only (diagnostic runs with BH_DEBUG may start a second one for testing).
        let second_ok = std::env::var_os("BH_DEBUG").is_some() || std::env::var_os("BH_SECOND_INSTANCE").is_some();
        if GetLastError() == ERROR_ALREADY_EXISTS && !second_ok {
            return;
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // No text input anywhere: skip loading the IME / text services framework.
        let _ = windows::Win32::UI::Input::Ime::ImmDisableIME(u32::MAX);

        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let arrow = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
        for (class, proc_) in [
            (
                MAIN_CLASS,
                main_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
            ),
            (STRIP_CLASS, strip_proc),
        ] {
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc_),
                hInstance: hinst.into(),
                hCursor: arrow,
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassExW(&wc);
        }
        let Ok(main) = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            MAIN_CLASS,
            w!("BatteryHub Taskbar"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst.into()),
            None,
        ) else {
            return;
        };
        let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
        // Arrival / removal of any HID device (USB receivers, cables, Bluetooth HID links) is
        // reported to this window even though it is hidden.
        let mut filter = DEV_BROADCAST_DEVICEINTERFACE_W {
            dbcc_size: size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32,
            dbcc_devicetype: DBT_DEVTYP_DEVICEINTERFACE.0,
            dbcc_classguid: windows::Win32::Devices::HumanInterfaceDevice::HidD_GetHidGuid(),
            ..Default::default()
        };
        let _ = RegisterDeviceNotificationW(HANDLE(main.0), &mut filter as *mut _ as *const c_void, DEVICE_NOTIFY_WINDOW_HANDLE);
        with_app(|a| {
            a.main = main;
            a.taskbar_created = taskbar_created;
            a.refresh_settings();
            a.create_tooltip();
            // BH_FORCE_FALLBACK (diagnostics): start with notification-area icons. Otherwise the
            // fallback is decided by tick() after FALLBACK_AFTER_TICKS, so a taskbar that is not
            // ready yet at sign-in does not make icons flash up for a second.
            if std::env::var_os("BH_FORCE_FALLBACK").is_some() {
                a.set_fallback(true);
            } else {
                a.ensure_strip();
            }
            a.bt_sig = batteryhub::drivers::bluetooth::connection_signature();
            a.start_poll();
        });
        SetTimer(Some(main), TIMER_POLL, refresh_minutes() * 60_000, None);
        SetTimer(Some(main), TIMER_PLACE, 1000, None);
        SetTimer(Some(main), TIMER_BTWATCH, 3000, None);
        SetTimer(Some(main), TIMER_QUICK, QUICK_MS, None);
        if auto_update() {
            SetTimer(Some(main), TIMER_AUTO_UPDATE, AUTO_UPDATE_DELAY_MS, None);
        }
        // Started by the setup after an update: say so once.
        if let Some(old) = batteryhub::util::reg_get_string(SETTINGS_KEY, "UpdatedFrom") {
            batteryhub::util::reg_delete_value(SETTINGS_KEY, "UpdatedFrom");
            if batteryhub::install::label_of(&old) != batteryhub::install::version_label() {
                with_app(|a| a.updated_from = Some(old));
                SetTimer(Some(main), TIMER_UPDATED, 2000, None);
            }
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            // Tab / Enter / Esc in the "Request support for a device" window.
            if batteryhub::report_window::pre_translate(&msg) {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
