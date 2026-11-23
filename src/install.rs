//! Shared pieces of the setup and of the program itself: names, folders and registry keys,
//! registry access, shortcuts and a verbose, timestamped console log. Everything lives in its own
//! folders, keys, shortcuts, window classes and autostart entry, so the original C++ BatteryHub
//! (Program Files\BatteryHub, HKCU\Software\WirelessBattery) is never touched.
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use crate::i18n::{tr, trf};

use windows::core::{Interface, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Console::{
    GetConsoleMode, GetStdHandle, SetConsoleMode, SetConsoleOutputCP, SetConsoleTitleW, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
    STD_OUTPUT_HANDLE,
};
use windows::Win32::System::Registry::*;
pub use windows::Win32::System::Registry::{HKEY_CURRENT_USER as HKCU, HKEY_LOCAL_MACHINE as HKLM};
use windows::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Programs, IShellLinkW, SHGetKnownFolderPath, ShellLink, KF_FLAG_DEFAULT};

pub const PRODUCT: &str = "BatteryHub";
/// App version from the VERSION file (set by build.rs; may have four parts, e.g. 1.0.1.1).
pub const VERSION: &str = env!("BH_VERSION");
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const RUN_ONCE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";
pub const STARTUP_APPROVED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
/// Parent of the settings key (removed when empty).
pub const ROOT_KEY: &str = r"Software\BatteryHub";

/// "v1.0" for this build's version "1.0.0".
pub fn version_label() -> String {
    label_of(VERSION)
}

/// "v1.5" for "1.5.0", "v1.0.1.1" for "1.0.1.1".
pub fn label_of(version: &str) -> String {
    let mut parts: Vec<&str> = version.trim_start_matches('v').split('.').collect();
    while parts.len() > 2 && parts.last() == Some(&"0") {
        parts.pop();
    }
    format!("v{}", parts.join("."))
}

/// The installable program (names, folders and keys in one place).
#[derive(Clone, Copy, Debug)]
pub struct Edition {
    /// "BatteryHub Taskbar"
    pub name: &'static str,
    /// "Taskbar"
    pub short: &'static str,
    pub exe: &'static str,
    /// HKCU key with InstallLocation / Version (and the program's own settings).
    pub settings_key: &'static str,
    /// Class of the program's main window (used to ask a running copy to exit).
    pub window_class: &'static str,
    /// Value name in HKCU\...\Run (taskbar edition only).
    pub run_value: Option<&'static str>,
    pub description: &'static str,
}

pub const TASKBAR: Edition = Edition {
    name: "BatteryHub Taskbar",
    short: "Taskbar",
    exe: "batteryhub-taskbar.exe",
    settings_key: r"Software\BatteryHub\Taskbar",
    window_class: "BatteryHub.Taskbar.Main",
    run_value: Some("BatteryHub Taskbar"),
    description: "Battery levels of your wireless devices, right on the taskbar",
};

impl Edition {
    /// %LOCALAPPDATA%\Programs\BatteryHub (shared parent, removed only when empty).
    pub fn root_dir() -> PathBuf {
        let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
        PathBuf::from(base).join("Programs").join(PRODUCT)
    }
    /// %LOCALAPPDATA%\Programs\BatteryHub\Taskbar
    pub fn default_dir(&self) -> PathBuf {
        Self::root_dir().join(self.short)
    }
    pub fn start_menu_folder() -> PathBuf {
        known_folder(&FOLDERID_Programs).join(PRODUCT)
    }
    pub fn start_menu_shortcut(&self) -> PathBuf {
        Self::start_menu_folder().join(format!("{}.lnk", self.name))
    }
    pub fn desktop_shortcut(&self) -> PathBuf {
        known_folder(&FOLDERID_Desktop).join(format!("{}.lnk", self.name))
    }
}

/// A shell known folder (handles OneDrive-redirected Desktop etc.).
pub fn known_folder(id: &windows::core::GUID) -> PathBuf {
    unsafe {
        match SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) {
            Ok(p) => {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const _));
                PathBuf::from(s)
            }
            Err(_) => PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()),
        }
    }
}

pub fn startup_dir() -> PathBuf {
    known_folder(&FOLDERID_Programs).join("Startup")
}

// ---- console log -------------------------------------------------------------------------------

pub struct Log {
    start: Instant,
    color: bool,
}

const RESET: &str = "\x1b[0m";

impl Log {
    /// Sets up the console: UTF-8 output, ANSI colours, window title.
    pub fn new(title: &str) -> Self {
        let mut color = false;
        unsafe {
            let _ = SetConsoleOutputCP(65001);
            let _ = SetConsoleTitleW(&HSTRING::from(title));
            if let Ok(h) = GetStdHandle(STD_OUTPUT_HANDLE) {
                let mut mode = Default::default();
                if GetConsoleMode(h, &mut mode).is_ok() {
                    color = SetConsoleMode(h, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING).is_ok();
                }
            }
        }
        Self { start: Instant::now(), color }
    }

    fn line(&self, tag: &str, code: &str, msg: &str) {
        let t = self.start.elapsed().as_secs_f64();
        let mut out = std::io::stdout().lock();
        if self.color {
            let _ = writeln!(out, "\x1b[90m[{t:8.3}]{RESET} {code}{tag:<5}{RESET} {msg}");
        } else {
            let _ = writeln!(out, "[{t:8.3}] {tag:<5} {msg}");
        }
        let _ = out.flush();
        drop(out);
        // A short pause per line so the log visibly scrolls instead of appearing all at once.
        std::thread::sleep(std::time::Duration::from_millis(if tag == "STEP" { 120 } else { 25 }));
    }

    pub fn info(&self, msg: impl AsRef<str>) {
        self.line("INFO", "\x1b[36m", msg.as_ref());
    }
    pub fn step(&self, msg: impl AsRef<str>) {
        self.line("STEP", "\x1b[1;97m", msg.as_ref());
    }
    pub fn ok(&self, msg: impl AsRef<str>) {
        self.line("OK", "\x1b[32m", msg.as_ref());
    }
    pub fn warn(&self, msg: impl AsRef<str>) {
        self.line("WARN", "\x1b[33m", msg.as_ref());
    }
    pub fn error(&self, msg: impl AsRef<str>) {
        self.line("ERROR", "\x1b[31m", msg.as_ref());
    }
    pub fn detail(&self, msg: impl AsRef<str>) {
        self.line("", "", &format!("    {}", msg.as_ref()));
    }

    pub fn banner(&self, text: &str) {
        let bar = "=".repeat(64);
        if self.color {
            println!("\x1b[1;96m{bar}\n  {text}\n{bar}{RESET}");
        } else {
            println!("{bar}\n  {text}\n{bar}");
        }
    }

    /// Asks a yes/no question on the console; Enter picks `default`.
    pub fn ask(&self, question: &str, default: bool) -> bool {
        loop {
            let hint = tr(if default { "[Y/n]" } else { "[y/N]" });
            if self.color {
                print!("\x1b[1;93m  ?  {question} {hint}: {RESET}");
            } else {
                print!("  ?  {question} {hint}: ");
            }
            let _ = std::io::stdout().flush();
            let mut s = String::new();
            if !matches!(std::io::stdin().read_line(&mut s), Ok(n) if n > 0) {
                return default;
            }
            match s.trim().to_lowercase().as_str() {
                "" => return default,
                "y" | "yes" | "t" | "tak" | "taip" => return true,
                "n" | "no" | "nie" | "ne" => return false,
                _ => println!("{}", tr("     Please answer y or n.")),
            }
        }
    }

    pub fn pause(&self) {
        print!("{}", tr("\nPress Enter to close this window..."));
        let _ = std::io::stdout().flush();
        let mut s = String::new();
        let _ = std::io::stdin().read_line(&mut s);
    }
}

// ---- registry ----------------------------------------------------------------------------------

pub struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

pub fn hive_name(h: HKEY) -> &'static str {
    if h == HKEY_LOCAL_MACHINE {
        "HKLM"
    } else {
        "HKCU"
    }
}

pub fn open(hive: HKEY, path: &str, write: bool) -> Option<Key> {
    let mut k = HKEY::default();
    let sam = if write { KEY_READ | KEY_WRITE } else { KEY_READ };
    let r = unsafe { RegOpenKeyExW(hive, &HSTRING::from(path), None, sam, &mut k) };
    (r == ERROR_SUCCESS).then_some(Key(k))
}

pub fn create(hive: HKEY, path: &str) -> Option<Key> {
    let mut k = HKEY::default();
    let r = unsafe {
        RegCreateKeyExW(hive, &HSTRING::from(path), None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_READ | KEY_WRITE, None, &mut k, None)
    };
    (r == ERROR_SUCCESS).then_some(Key(k))
}

impl Key {
    /// Every value as (name, type, text form of the data).
    pub fn values(&self) -> Vec<(String, u32, String)> {
        let mut out = Vec::new();
        for i in 0.. {
            let mut name = vec![0u16; 512];
            let mut name_len = name.len() as u32;
            let mut ty = 0u32;
            let mut data = vec![0u8; 4096];
            let mut data_len = data.len() as u32;
            let r = unsafe {
                RegEnumValueW(self.0, i, Some(PWSTR(name.as_mut_ptr())), &mut name_len, None, Some(&mut ty), Some(data.as_mut_ptr()), Some(&mut data_len))
            };
            if r != ERROR_SUCCESS {
                break;
            }
            let n = String::from_utf16_lossy(&name[..name_len as usize]);
            let text = match REG_VALUE_TYPE(ty) {
                REG_SZ | REG_EXPAND_SZ => {
                    let w: Vec<u16> = data[..data_len as usize].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                    crate::util::from_wide(&w)
                }
                REG_DWORD if data_len >= 4 => format!("0x{:08X}", u32::from_le_bytes([data[0], data[1], data[2], data[3]])),
                _ => data[..data_len as usize].iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" "),
            };
            out.push((n, ty, text));
        }
        out
    }

    pub fn set_string(&self, name: &str, value: &str) -> bool {
        let w = crate::util::wide(value);
        let bytes = unsafe { std::slice::from_raw_parts(w.as_ptr() as *const u8, w.len() * 2) };
        unsafe { RegSetValueExW(self.0, &HSTRING::from(name), None, REG_SZ, Some(bytes)) == ERROR_SUCCESS }
    }

    pub fn set_dword(&self, name: &str, value: u32) -> bool {
        unsafe { RegSetValueExW(self.0, &HSTRING::from(name), None, REG_DWORD, Some(&value.to_le_bytes())) == ERROR_SUCCESS }
    }

    pub fn delete_value(&self, name: &str) -> bool {
        unsafe { RegDeleteValueW(self.0, &HSTRING::from(name)) == ERROR_SUCCESS }
    }
}

pub fn delete_tree(hive: HKEY, path: &str) -> bool {
    unsafe { RegDeleteTreeW(hive, &HSTRING::from(path)) == ERROR_SUCCESS }
}

// ---- shortcut ----------------------------------------------------------------------------------

/// Creates a .lnk file pointing at `target`.
pub fn create_shortcut(lnk: &std::path::Path, target: &std::path::Path, description: &str) -> windows::core::Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(target.as_os_str()))?;
        if let Some(dir) = target.parent() {
            link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
        }
        link.SetDescription(&HSTRING::from(description))?;
        let file: IPersistFile = link.cast()?;
        file.Save(&HSTRING::from(lnk.as_os_str()), true)
    }
}

/// CRC-32 (IEEE), shown in the logs to verify extracted files.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

pub fn fmt_bytes(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    trf("{0} bytes", &[&out])
}

/// Asks a running copy of `ed` to exit (WM_CLOSE to its main window); force-kills it if it is
/// still there after ~3 s. Returns whether one was running.
pub fn stop_running(ed: &Edition, log: &Log) -> bool {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_CLOSE};
    let class = HSTRING::from(ed.window_class);
    let find = || unsafe { FindWindowW(&class, None).ok().filter(|h| !h.0.is_null()) };
    log.info(trf("Looking for a running {0} (window class \"{1}\")", &[&ed.name, &ed.window_class]));
    let Some(h) = find() else {
        log.detail(tr("not running"));
        return false;
    };
    log.info(trf("{0} is running (window {1}), asking it to exit...", &[&ed.name, &format!("{:?}", h.0)]));
    unsafe {
        let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
    for i in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if find().is_none() {
            log.ok(trf("{0} exited after {1} ms.", &[&ed.name, &((i + 1) * 100)]));
            std::thread::sleep(std::time::Duration::from_millis(300)); // let the process release its exe
            return true;
        }
    }
    log.warn(trf("{0} did not exit in time, terminating {1}...", &[&ed.name, &ed.exe]));
    use std::os::windows::process::CommandExt;
    let st = std::process::Command::new("taskkill").args(["/F", "/IM", ed.exe]).creation_flags(0x0800_0000).output();
    match st {
        Ok(o) if o.status.success() => log.ok(tr("Process terminated.")),
        _ => log.error(tr("Could not terminate the process.")),
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    true
}

/// Compares dotted versions ("1.0.0" < "1.0.1" < "1.1").
pub fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |v: &str| -> Vec<u32> { v.trim_start_matches('v').split('.').map(|x| x.parse().unwrap_or(0)).collect() };
    let (mut x, mut y) = (parse(a), parse(b));
    let n = x.len().max(y.len());
    x.resize(n, 0);
    y.resize(n, 0);
    x.cmp(&y)
}

/// Deletes a key only when it has no subkeys left (e.g. HKCU\Software\BatteryHub after both
/// editions were removed). Returns whether it was deleted.
pub fn delete_key_if_empty(hive: HKEY, path: &str) -> bool {
    unsafe { RegDeleteKeyW(hive, &HSTRING::from(path)) == ERROR_SUCCESS }
}

impl Key {
    pub fn get(&self, name: &str) -> Option<String> {
        self.values().into_iter().find(|(n, _, _)| n.eq_ignore_ascii_case(name)).map(|v| v.2)
    }
}

/// Product version from an exe's version resource ("1.0.1" or "1.0.1.1"), if it has one.
pub fn file_version(path: &std::path::Path) -> Option<String> {
    use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO};
    let name = HSTRING::from(path.as_os_str());
    unsafe {
        let size = GetFileVersionInfoSizeW(&name, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&name, None, size, data.as_mut_ptr().cast()).ok()?;
        let mut info: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        if !VerQueryValueW(data.as_ptr().cast(), windows::core::w!("\\"), &mut info, &mut len).as_bool() || info.is_null() {
            return None;
        }
        let f = &*(info as *const VS_FIXEDFILEINFO);
        let v = format!(
            "{}.{}.{}.{}",
            f.dwProductVersionMS >> 16,
            f.dwProductVersionMS & 0xFFFF,
            f.dwProductVersionLS >> 16,
            f.dwProductVersionLS & 0xFFFF
        );
        // "1.0.1.0" -> "1.0.1"; a fourth part is kept when set ("1.0.1.1").
        Some(v.strip_suffix(".0").map(str::to_string).unwrap_or(v))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_the_version_resource_of_an_exe() {
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
        let v = super::file_version(&std::path::Path::new(&windir).join(r"System32\notepad.exe"));
        assert!(v.is_some_and(|v| v.split('.').count() >= 3));
        assert!(super::file_version(std::path::Path::new(r"C:\does\not\exist.exe")).is_none());
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    #[test]
    fn create_write_read_delete() {
        let path = r"Software\BatteryHub-Test-Registry";
        let k = create(HKCU, path).expect("create");
        assert!(k.set_dword("D", 7), "set_dword");
        assert!(k.set_string("S", "abc"), "set_string");
        drop(k);
        let r = open(HKCU, path, false).expect("open");
        assert_eq!(r.get("S").as_deref(), Some("abc"));
        assert_eq!(r.get("D").as_deref(), Some("0x00000007"));
        drop(r);
        assert!(delete_tree(HKCU, path));
    }
}
