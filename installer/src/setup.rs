//! BatteryHub setup: offers Install / Update / Uninstall after scanning what is already on this PC:
//! install folder, version, files, shortcuts, autostart, running copy and the original C++ BatteryHub.
//! Every file and registry entry that is created or removed is logged with its full path.
//!
//! Command line: /install | /update | /uninstall, /S (silent), /autostart=yes|no, /nolaunch,
//! /nonotify (no "Update finished" message from the updated program)
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use batteryhub::i18n::{tr, trf};
use batteryhub::install::{self as inst, Edition, Log, HKCU, HKLM};
use batteryhub::{device, hid};

// ---- options -----------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum Action {
    Install,
    Update,
    Uninstall,
}

struct Opts {
    action: Option<Action>,
    silent: bool,
    autostart: Option<bool>,
    launch: bool,
    notify: bool,
}

fn opts() -> Opts {
    let mut o = Opts { action: None, silent: false, autostart: None, launch: true, notify: true };
    for a in std::env::args().skip(1) {
        match a.to_lowercase().as_str() {
            "/install" | "--install" => o.action = Some(Action::Install),
            "/update" | "--update" => o.action = Some(Action::Update),
            "/uninstall" | "--uninstall" => o.action = Some(Action::Uninstall),
            "/s" | "--silent" => o.silent = true,
            "/autostart=yes" | "--autostart" => o.autostart = Some(true),
            "/autostart=no" | "--no-autostart" => o.autostart = Some(false),
            "/nolaunch" | "--no-launch" => o.launch = false,
            "/nonotify" | "--no-notify" => o.notify = false,
            _ => {}
        }
    }
    o
}

// ---- helpers -----------------------------------------------------------------------------------

/// "2026-11-21 19:42 UTC" from a file time (no time-zone database needed).
fn fmt_time(t: SystemTime) -> String {
    let secs = t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (H. Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, rem % 3600 / 60)
}

fn file_line(p: &Path) -> String {
    match std::fs::metadata(p) {
        Ok(m) => trf(
            "{0}  ({1}, modified {2})",
            &[&p.display(), &inst::fmt_bytes(m.len()), &m.modified().map(fmt_time).unwrap_or_default()],
        ),
        Err(_) => trf("{0}  (missing)", &[&p.display()]),
    }
}

fn choose(log: &Log, valid: &[char]) -> char {
    loop {
        print!("{}", trf("\n  Choose an option [{0}]: ", &[&valid.iter().map(|c| c.to_string()).collect::<Vec<_>>().join("/")]));
        let _ = std::io::stdout().flush();
        let mut s = String::new();
        // Error or end of input (closed console / empty pipe): treat as Exit instead of looping.
        if !matches!(std::io::stdin().read_line(&mut s), Ok(n) if n > 0) {
            return '0';
        }
        if let Some(c) = s.trim().chars().next() {
            if valid.contains(&c) {
                return c;
            }
        }
        log.warn(tr("Please type one of the numbers shown."));
    }
}

// ---- state -------------------------------------------------------------------------------------

/// What is on this PC for one edition.
struct State {
    dir: PathBuf,
    exe: PathBuf,
    exe_exists: bool,
    version: Option<String>,
    installed: bool,
    desktop: bool,
    start_menu: bool,
    autostart: Option<String>,
    running: bool,
}

fn read_state(ed: &Edition) -> State {
    let key = inst::open(HKCU, ed.settings_key, false);
    let dir = key.as_ref().and_then(|k| k.get("InstallLocation")).map(PathBuf::from).unwrap_or_else(|| ed.default_dir());
    let exe = dir.join(ed.exe);
    // The version saved at install time, else the one in the program file's version resource.
    let version = key.as_ref().and_then(|k| k.get("Version")).or_else(|| inst::file_version(&exe));
    let exe_exists = exe.exists();
    let autostart = ed.run_value.and_then(|v| inst::open(HKCU, inst::RUN_KEY, false).and_then(|k| k.get(v)));
    let running = unsafe {
        windows::Win32::UI::WindowsAndMessaging::FindWindowW(&windows::core::HSTRING::from(ed.window_class), None)
            .is_ok_and(|h| !h.0.is_null())
    };
    State {
        installed: exe_exists,
        dir,
        exe,
        exe_exists,
        version,
        desktop: ed.desktop_shortcut().exists(),
        start_menu: ed.start_menu_shortcut().exists(),
        autostart,
        running,
    }
}

/// Autostart line: where the entry is in the registry (not the program path it starts).
fn autostart_line(ed: &Edition, st: &State) -> String {
    match (ed.run_value, &st.autostart) {
        (Some(v), Some(_)) => trf("on (HKCU\\{0})", &[&format!("{}\\{v}", inst::RUN_KEY)]),
        _ => tr("off").into(),
    }
}

fn yes_no(b: bool) -> &'static str {
    tr(if b { "yes" } else { "no" })
}

/// Summary line: a label (padded to 22 columns) and its value.
fn row(label: &'static str, value: impl std::fmt::Display) {
    println!("  {:<22} {value}", tr(label));
}

/// Scans the PC (with a detailed log) and prints a summary. Returns this edition's state.
fn scan(ed: &Edition, log: &Log) -> State {
    log.step(tr("Scanning this PC for existing BatteryHub installations"));
    log.info(trf("Reading HKCU\\{0}", &[&ed.settings_key]));
    match inst::open(HKCU, ed.settings_key, false) {
        Some(k) => {
            for (n, _, d) in k.values() {
                log.detail(format!("{n:<16} = {d}"));
            }
        }
        None => log.detail(tr("(not present)")),
    }
    let st = read_state(ed);
    log.info(trf("Checking install folder {0}", &[&st.dir.display()]));
    match std::fs::read_dir(&st.dir) {
        Ok(entries) => {
            for e in entries.flatten() {
                log.detail(file_line(&e.path()));
            }
        }
        Err(_) => log.detail(tr("(folder does not exist)")),
    }
    log.info(trf("Checking desktop shortcut {0}", &[&ed.desktop_shortcut().display()]));
    log.detail(tr(if st.desktop { "found" } else { "not found" }));
    log.info(trf("Checking Start menu shortcut {0}", &[&ed.start_menu_shortcut().display()]));
    log.detail(tr(if st.start_menu { "found" } else { "not found" }));
    if let Some(v) = ed.run_value {
        log.info(trf("Checking autostart HKCU\\{0}", &[&format!("{}\\{v}", inst::RUN_KEY)]));
        log.detail(st.autostart.as_deref().unwrap_or(tr("not set")));
    }
    log.info(trf("Checking whether {0} is running", &[&ed.name]));
    log.detail(tr(if st.running { "running" } else { "not running" }));

    // The original C++ app: reported, never touched.
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    let original = [
        PathBuf::from(&pf).join(r"BatteryHub\BatteryHub.exe"),
        PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join(r"Programs\BatteryHub\BatteryHub.exe"),
    ];
    log.info(tr("Checking for the original BatteryHub (C++)"));
    let orig = original.iter().find(|p| p.exists());
    log.detail(orig.map(|p| trf("found: {0}", &[&p.display()])).unwrap_or_else(|| tr("not found").into()));

    // Summary.
    println!();
    log.banner(tr("CURRENT STATE"));
    let status = match (&st.installed, &st.version) {
        (true, Some(v)) => trf("INSTALLED  v{0}", &[v]),
        (true, None) => tr("INSTALLED  (unknown version)").into(),
        (false, _) => tr("NOT INSTALLED").into(),
    };
    println!("  {:<22} {status}", ed.name);
    row("This setup contains", format!("v{}", inst::VERSION));
    row("Install folder", st.dir.display());
    row("Program file", if st.exe_exists { file_line(&st.exe) } else { tr("(not present)").into() });
    row("Desktop shortcut", if st.desktop { ed.desktop_shortcut().display().to_string() } else { tr("(none)").into() });
    row("Start menu shortcut", if st.start_menu { ed.start_menu_shortcut().display().to_string() } else { tr("(none)").into() });
    row("Settings", format!("HKCU\\{}", ed.settings_key));
    if ed.run_value.is_some() {
        row("Autostart", autostart_line(ed, &st));
    }
    row("Running now", yes_no(st.running));
    row(
        "Original BatteryHub",
        orig.map(|p| trf("{0} (separate - not affected)", &[&p.display()])).unwrap_or_else(|| tr("not installed").into()),
    );
    st
}

// ---- actions -----------------------------------------------------------------------------------

fn extract(log: &Log, path: &Path, data: &[u8]) -> bool {
    log.info(trf("Writing {0} ({1})", &[&path.display(), &inst::fmt_bytes(data.len() as u64)]));
    log.detail(format!("source CRC-32 0x{:08X}", inst::crc32(data)));
    let mut file = None;
    for attempt in 1..=20 {
        match std::fs::File::create(path) {
            Ok(f) => {
                file = Some(f);
                break;
            }
            Err(e) => {
                log.warn(trf("file is in use ({0}), retrying ({1}/20)...", &[&e, &attempt]));
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        }
    }
    let Some(mut f) = file else {
        log.error(trf("Cannot write {0}", &[&path.display()]));
        return false;
    };
    const CHUNK: usize = 32 * 1024;
    for (i, chunk) in data.chunks(CHUNK).enumerate() {
        if let Err(e) = f.write_all(chunk) {
            log.error(trf("Write error: {0}", &[&e]));
            return false;
        }
        let done = (i * CHUNK + chunk.len()) as u64;
        log.detail(format!(
            "block {:>2}  {:>15} / {}  ({:>3}%)",
            i + 1,
            inst::fmt_bytes(done),
            inst::fmt_bytes(data.len() as u64),
            done * 100 / data.len() as u64
        ));
    }
    drop(f);
    let written = std::fs::read(path).unwrap_or_default();
    let crc = inst::crc32(&written);
    if written.len() == data.len() && crc == inst::crc32(data) {
        log.ok(trf("Verified (CRC-32 0x{0})", &[&format!("{crc:08X}")]));
        true
    } else {
        log.error(trf("Verification failed (CRC-32 0x{0})", &[&format!("{crc:08X}")]));
        false
    }
}

fn save_info(ed: &Edition, log: &Log, dir: &Path, created: &mut Vec<String>) {
    log.info(trf("Saving install information to HKCU\\{0}", &[&ed.settings_key]));
    match inst::create(HKCU, ed.settings_key) {
        Some(k) => {
            for (n, v) in [
                ("InstallLocation", dir.display().to_string()),
                ("Version", inst::VERSION.to_string()),
                ("InstalledOn", fmt_time(SystemTime::now())),
            ] {
                if k.set_string(n, &v) {
                    log.detail(format!("set {n:<16} = {v}"));
                }
            }
            created.push(format!("registry  HKCU\\{}", ed.settings_key));
        }
        None => log.warn(tr("Could not save install information.")),
    }
}

/// Copy of the setup kept in the install folder: "Uninstall" in Windows Settings runs it.
const SETUP_COPY: &str = "BatteryHub-Setup.exe";

/// Entry in Settings > Apps (also what winget reads to see the installed version).
fn apps_key(ed: &Edition) -> String {
    format!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{}", ed.name)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Keeps a copy of this setup next to the program and registers the app in Settings > Apps.
fn register_app(ed: &Edition, log: &Log, dir: &Path, exe: &Path, created: &mut Vec<String>) {
    let setup = dir.join(SETUP_COPY);
    match std::env::current_exe() {
        Ok(me) if same_file(&me, &setup) => log.detail(trf("running from {0}", &[&setup.display()])),
        Ok(me) => match std::fs::copy(&me, &setup) {
            Ok(n) => {
                log.ok(trf("Copied the setup to {0} ({1})", &[&setup.display(), &inst::fmt_bytes(n)]));
                created.push(format!("setup     {}", setup.display()));
            }
            Err(e) => {
                log.warn(trf("Could not copy the setup to {0}: {1} (no entry in Settings > Apps)", &[&setup.display(), &e]));
                return;
            }
        },
        Err(e) => {
            log.warn(trf("Cannot find this setup's own file: {0} (no entry in Settings > Apps)", &[&e]));
            return;
        }
    }
    let key = apps_key(ed);
    log.info(trf("Registering in Settings > Apps: HKCU\\{0}", &[&key]));
    let Some(k) = inst::create(HKCU, &key) else {
        log.warn(tr("Could not create the Settings > Apps entry."));
        return;
    };
    let quoted = format!("\"{}\"", setup.display());
    for (n, v) in [
        ("DisplayName", ed.name.to_string()),
        ("DisplayVersion", inst::VERSION.to_string()),
        ("Publisher", "misiukasalgirdas-code".to_string()),
        ("DisplayIcon", format!("{},0", exe.display())),
        ("InstallLocation", dir.display().to_string()),
        ("UninstallString", format!("{quoted} /uninstall")),
        ("QuietUninstallString", format!("{quoted} /uninstall /S")),
        ("URLInfoAbout", format!("https://github.com/{}", batteryhub::update::REPO)),
        ("HelpLink", format!("https://github.com/{}/issues", batteryhub::update::REPO)),
    ] {
        if k.set_string(n, &v) {
            log.detail(format!("set {n:<20} = {v}"));
        }
    }
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let size_kb = ((size(exe) + size(&setup)) / 1024) as u32;
    for (n, v) in [("NoModify", 1u32), ("NoRepair", 1), ("EstimatedSize", size_kb)] {
        k.set_dword(n, v);
        log.detail(format!("set {n:<20} = {v}"));
    }
    created.push(format!("registry  HKCU\\{key}"));
}

/// Removes the Settings > Apps entry and the setup copy. When this setup *is* that copy (started
/// from Settings), it cannot delete itself: a hidden command deletes it a few seconds after exit.
fn unregister_app(ed: &Edition, log: &Log, dir: &Path, removed: &mut Vec<String>) {
    let key = apps_key(ed);
    if inst::open(HKCU, &key, false).is_some() && inst::delete_tree(HKCU, &key) {
        log.ok(trf("Deleted HKCU\\{0}", &[&key]));
        removed.push(format!("registry  HKCU\\{key}"));
    }
    let setup = dir.join(SETUP_COPY);
    if !setup.exists() {
        return;
    }
    let me = std::env::current_exe().ok();
    if me.as_deref().is_some_and(|m| same_file(m, &setup)) {
        use std::os::windows::process::CommandExt;
        let cmd = format!(
            "ping -n 9 127.0.0.1 >nul & del /f /q \"{}\" & rmdir \"{}\" & rmdir \"{}\"",
            setup.display(),
            dir.display(),
            Edition::root_dir().display()
        );
        // CREATE_NO_WINDOW | DETACHED_PROCESS
        match std::process::Command::new("cmd").arg("/c").raw_arg(&cmd).creation_flags(0x0800_0000 | 0x0000_0008).spawn() {
            Ok(_) => {
                log.ok(trf("{0} will be deleted a few seconds after this window closes", &[&setup.display()]));
                removed.push(format!("setup     {}", setup.display()));
            }
            Err(e) => log.warn(trf("Could not schedule deleting {0}: {1}", &[&setup.display(), &e])),
        }
    } else {
        remove_file(log, &setup, removed, "setup    ");
    }
}

fn make_shortcuts(ed: &Edition, log: &Log, exe: &Path, created: &mut Vec<String>) {
    let start_dir = Edition::start_menu_folder();
    if !start_dir.exists() {
        let _ = std::fs::create_dir_all(&start_dir);
        log.detail(trf("created folder {0}", &[&start_dir.display()]));
    }
    for (what, lnk) in [("Desktop", ed.desktop_shortcut()), ("Start menu", ed.start_menu_shortcut())] {
        log.info(trf("Creating {0} shortcut", &[&tr(what)]));
        match inst::create_shortcut(&lnk, exe, ed.description) {
            Ok(_) => {
                log.ok(format!("{} -> {}", lnk.display(), exe.display()));
                created.push(format!("shortcut  {}", lnk.display()));
            }
            Err(e) => log.warn(trf("Could not create {0}: {1}", &[&lnk.display(), &e])),
        }
    }
}

fn set_autostart(ed: &Edition, log: &Log, exe: &Path, on: bool, created: &mut Vec<String>) {
    let Some(name) = ed.run_value else { return };
    let Some(run) = inst::open(HKCU, inst::RUN_KEY, true).or_else(|| inst::create(HKCU, inst::RUN_KEY)) else {
        log.error(tr("Cannot open the autostart registry key."));
        return;
    };
    if on {
        let cmd = format!("\"{}\"", exe.display());
        if run.set_string(name, &cmd) {
            log.ok(trf("Autostart on: HKCU\\{0} = {1}", &[&format!("{}\\{name}", inst::RUN_KEY), &cmd]));
            created.push(format!("autostart HKCU\\{}\\{name}", inst::RUN_KEY));
        }
        // A "disabled" state left in Task Manager would keep it from starting.
        if let Some(sa) = inst::open(HKCU, inst::STARTUP_APPROVED_KEY, true) {
            if sa.delete_value(name) {
                log.detail(tr("cleared an old Task Manager 'disabled' state"));
            }
        }
    } else if run.delete_value(name) {
        log.ok(tr("Autostart off: removed the existing autostart entry."));
    } else {
        log.ok(tr("Autostart off."));
    }
}

fn device_scan(log: &Log) {
    log.step(tr("Scanning for supported wireless devices (this can take a few seconds)"));
    let colls = hid::enum_hid();
    let mut ids: Vec<(u16, u16)> = colls.iter().map(|c| (c.vid, c.pid)).collect();
    ids.sort();
    ids.dedup();
    log.detail(trf("{0} HID collections on {1} USB / Bluetooth devices", &[&colls.len(), &ids.len()]));
    for (v, p) in &ids {
        let name = colls
            .iter()
            .find(|c| c.vid == *v && c.pid == *p)
            .map(hid::product_string)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| tr("(no product name)").into());
        log.detail(format!("HID {v:04X}:{p:04X}  {name}"));
    }
    let devices = device::poll_all_devices();
    if devices.is_empty() {
        log.warn(tr("No supported device with a battery is connected right now. BatteryHub will pick them up later."));
    }
    for d in &devices {
        log.ok(format!("{:<10} {:<38} {:<12} {} {}", d.kind.label(), d.name, d.status_text(), tr("via"), d.via));
    }
}

fn launch(ed: &Edition, log: &Log, exe: &Path) {
    use std::process::Stdio;
    match std::process::Command::new(exe)
        .current_dir(exe.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => log.ok(trf("{0} started (process id {1}).", &[&ed.name, &c.id()])),
        Err(e) => log.error(trf("Could not start {0}: {1}", &[&ed.name, &e])),
    }
}

fn print_list(log: &Log, title: &str, items: &[String]) {
    println!();
    log.banner(title);
    if items.is_empty() {
        println!("{}", tr("  (nothing)"));
    }
    for i in items {
        println!("  {i}");
    }
}

fn install(ed: &Edition, payload: &[u8], log: &Log, o: &Opts, st: &State, update: bool) -> bool {
    let mut created = Vec::new();
    let step1 = if update {
        "[1/6] Updating {0} v{1} - stopping a running copy"
    } else {
        "[1/6] Installing {0} v{1} - stopping a running copy"
    };
    log.step(trf(step1, &[&ed.name, &inst::VERSION]));
    let was_running = inst::stop_running(ed, log);

    log.step(tr("[2/6] Preparing the install folder"));
    let dir = if update { st.dir.clone() } else { ed.default_dir() };
    if dir.exists() {
        log.detail(trf("{0} already exists", &[&dir.display()]));
    } else if let Err(e) = std::fs::create_dir_all(&dir) {
        log.error(trf("Cannot create {0}: {1}", &[&dir.display(), &e]));
        return false;
    } else {
        log.ok(trf("Created {0}", &[&dir.display()]));
    }

    log.step(tr("[3/6] Installing the program file"));
    let exe = dir.join(ed.exe);
    if update {
        log.detail(trf("old: {0}", &[&file_line(&exe)]));
    }
    if !extract(log, &exe, payload) {
        return false;
    }
    created.push(format!("program   {}", exe.display()));

    log.step(tr("[4/6] Saving install information"));
    save_info(ed, log, &dir, &mut created);
    register_app(ed, log, &dir, &exe, &mut created);

    log.step(tr("[5/6] Creating shortcuts (desktop and Start menu)"));
    make_shortcuts(ed, log, &exe, &mut created);

    log.step(tr("[6/6] Autostart"));
    if ed.run_value.is_none() {
        log.info(trf("{0} does not run in the background - no autostart needed.", &[&ed.name]));
    } else if update {
        match &st.autostart {
            Some(_) => {
                log.info(tr("Autostart was on - keeping it (path refreshed)."));
                set_autostart(ed, log, &exe, true, &mut created);
            }
            None => log.info(tr("Autostart was off - keeping it off.")),
        }
    } else {
        let on = match o.autostart {
            Some(v) => v,
            None if o.silent => true,
            None => log.ask(&trf("Add {0} to Windows autostart (start automatically when you sign in)?", &[&ed.name]), true),
        };
        set_autostart(ed, log, &exe, on, &mut created);
    }

    device_scan(log);
    print_list(log, tr(if update { "FILES UPDATED / CREATED" } else { "FILES CREATED" }), &created);

    println!();
    let start = o.launch && (was_running || o.silent || log.ask(&trf("Start {0} now?", &[&ed.name]), true));
    if start {
        // The updated program says "Update finished: vOLD -> vNEW" once when it starts.
        if update && o.notify && ed.run_value.is_some() {
            if let (Some(old), Some(k)) = (&st.version, inst::create(HKCU, ed.settings_key)) {
                k.set_string("UpdatedFrom", old);
            }
        }
        launch(ed, log, &exe);
        if ed.run_value.is_some() {
            log.info(tr("Look at the taskbar, next to the clock. Right-click it for Refresh / Exit."));
        }
    }
    true
}

/// Does an autostart entry start BatteryHub? Strict on purpose: the original
/// C++ BatteryHub ("WirelessBattery" -> BatteryHub.exe) must never match.
fn starts_edition(ed: &Edition, dir: &Path, name: &str, command: &str) -> bool {
    let c = command.to_lowercase();
    let d = dir.display().to_string().to_lowercase();
    ed.run_value.is_some_and(|v| name.eq_ignore_ascii_case(v)) || c.contains(&ed.exe.to_lowercase()) || (!d.is_empty() && c.contains(&d))
}

fn scan_autostart(ed: &Edition, log: &Log, dir: &Path, removed: &mut Vec<String>) {
    log.info(trf("Looking for autostart entries that start {0}", &[&ed.exe]));
    for (hive, path) in [(HKCU, inst::RUN_KEY), (HKCU, inst::RUN_ONCE_KEY), (HKLM, inst::RUN_KEY)] {
        let where_ = format!("{}\\{path}", inst::hive_name(hive));
        log.info(trf("Scanning {0}", &[&where_]));
        let Some(key) = inst::open(hive, path, false) else {
            log.detail(tr("(key does not exist)"));
            continue;
        };
        for (name, _, data) in key.values() {
            if !starts_edition(ed, dir, &name, &data) {
                log.detail(format!("skip   {name:<28} -> {data}"));
                continue;
            }
            log.warn(trf("Found: \"{0}\" -> {1}", &[&name, &data]));
            match inst::open(hive, path, true) {
                Some(w) if w.delete_value(&name) => {
                    log.ok(trf("Removed \"{0}\" from {1}", &[&name, &where_]));
                    removed.push(format!("autostart {where_}\\{name}"));
                }
                _ if hive == HKLM => log.error(trf("Cannot remove \"{0}\" from {1}: run setup as administrator.", &[&name, &where_])),
                _ => log.error(trf("Failed to remove \"{0}\"", &[&name])),
            }
        }
    }
    if let Some(v) = ed.run_value {
        log.info(trf("Scanning HKCU\\{0} (Task Manager startup state)", &[&inst::STARTUP_APPROVED_KEY]));
        match inst::open(HKCU, inst::STARTUP_APPROVED_KEY, true) {
            Some(k) if k.delete_value(v) => {
                log.ok(trf("Removed startup state \"{0}\"", &[&v]));
                removed.push(format!("autostart HKCU\\{}\\{v}", inst::STARTUP_APPROVED_KEY));
            }
            _ => log.detail(tr("no entry")),
        }
    }
    let startup = inst::startup_dir();
    log.info(trf("Scanning Startup folder {0}", &[&startup.display()]));
    if let Ok(entries) = std::fs::read_dir(&startup) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let bytes = std::fs::read(e.path()).unwrap_or_default();
            let wide: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            let text = String::from_utf16_lossy(&wide) + &String::from_utf8_lossy(&bytes);
            if starts_edition(ed, dir, "", &text) {
                match std::fs::remove_file(e.path()) {
                    Ok(_) => {
                        log.ok(trf("Deleted startup shortcut {0}", &[&name]));
                        removed.push(format!("shortcut  {}", e.path().display()));
                    }
                    Err(err) => log.error(trf("Could not delete {0}: {1}", &[&name, &err])),
                }
            } else {
                log.detail(format!("skip   {name}"));
            }
        }
    }
}

fn remove_file(log: &Log, p: &Path, removed: &mut Vec<String>, label: &str) {
    if !p.exists() {
        log.detail(trf("{0} (not present)", &[&p.display()]));
        return;
    }
    let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    for _ in 0..20 {
        if std::fs::remove_file(p).is_ok() {
            log.ok(trf("Deleted {0} ({1})", &[&p.display(), &inst::fmt_bytes(size)]));
            removed.push(format!("{label} {}", p.display()));
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    log.error(trf("Could not delete {0}", &[&p.display()]));
}

fn remove_dir_if_empty(log: &Log, p: &Path, removed: &mut Vec<String>) {
    if !p.exists() {
        return;
    }
    match std::fs::remove_dir(p) {
        Ok(_) => {
            log.ok(trf("Removed empty folder {0}", &[&p.display()]));
            removed.push(format!("folder    {}", p.display()));
        }
        Err(_) => log.detail(trf("kept folder (not empty): {0}", &[&p.display()])),
    }
}

/// Leftovers of the pre-release "BatteryHub 2" test builds (taskbar edition only).
fn remove_legacy(log: &Log, removed: &mut Vec<String>) {
    let mut any = false;
    if let Some(run) = inst::open(HKCU, inst::RUN_KEY, true) {
        if run.delete_value("BatteryHub2") {
            removed.push(format!("autostart HKCU\\{}\\BatteryHub2", inst::RUN_KEY));
            any = true;
        }
    }
    for key in [r"Software\BatteryHub2", r"Software\Microsoft\Windows\CurrentVersion\Uninstall\BatteryHub2"] {
        if inst::open(HKCU, key, false).is_some() && inst::delete_tree(HKCU, key) {
            removed.push(format!("registry  HKCU\\{key}"));
            any = true;
        }
    }
    let old_dir = Edition::root_dir().with_file_name("BatteryHub2");
    for f in ["batteryhub-tray.exe", "uninstall.exe"] {
        let p = old_dir.join(f);
        if p.exists() && std::fs::remove_file(&p).is_ok() {
            removed.push(format!("file      {}", p.display()));
            any = true;
        }
    }
    if old_dir.exists() && std::fs::remove_dir(&old_dir).is_ok() {
        removed.push(format!("folder    {}", old_dir.display()));
    }
    let old_lnk = inst::known_folder(&windows::Win32::UI::Shell::FOLDERID_Programs).join("BatteryHub 2.lnk");
    if old_lnk.exists() && std::fs::remove_file(&old_lnk).is_ok() {
        removed.push(format!("shortcut  {}", old_lnk.display()));
        any = true;
    }
    if any {
        log.ok(tr("Removed leftovers of an old BatteryHub 2 test build."));
    } else {
        log.detail(tr("none found"));
    }
}

fn uninstall(ed: &Edition, log: &Log, st: &State) -> bool {
    let mut removed = Vec::new();
    log.step(trf("[1/6] Uninstalling {0} - stopping a running copy", &[&ed.name]));
    inst::stop_running(ed, log);

    log.step(tr("[2/6] Checking autostart (whether it added itself anywhere)"));
    scan_autostart(ed, log, &st.dir, &mut removed);

    log.step(tr("[3/6] Removing shortcuts"));
    remove_file(log, &ed.desktop_shortcut(), &mut removed, "shortcut ");
    remove_file(log, &ed.start_menu_shortcut(), &mut removed, "shortcut ");
    remove_dir_if_empty(log, &Edition::start_menu_folder(), &mut removed);

    log.step(tr("[4/6] Removing program files"));
    remove_file(log, &st.exe, &mut removed, "program  ");
    unregister_app(ed, log, &st.dir, &mut removed);
    remove_dir_if_empty(log, &st.dir, &mut removed);
    remove_dir_if_empty(log, &Edition::root_dir(), &mut removed);

    log.step(tr("[5/6] Removing settings"));
    if let Some(k) = inst::open(HKCU, ed.settings_key, false) {
        for (n, _, d) in k.values() {
            log.detail(format!("{n} = {d}"));
        }
        drop(k);
        if inst::delete_tree(HKCU, ed.settings_key) {
            log.ok(trf("Deleted HKCU\\{0}", &[&ed.settings_key]));
            removed.push(format!("registry  HKCU\\{}", ed.settings_key));
        }
    } else {
        log.detail(tr("(no settings stored)"));
    }
    if inst::delete_key_if_empty(HKCU, inst::ROOT_KEY) {
        log.ok(trf("Deleted empty HKCU\\{0}", &[&inst::ROOT_KEY]));
    } else {
        log.detail(trf("kept HKCU\\{0} (not empty, or already gone)", &[&inst::ROOT_KEY]));
    }

    log.step(tr("[6/6] Cleaning up leftovers"));
    if ed.run_value.is_some() {
        remove_legacy(log, &mut removed);
    } else {
        log.detail(tr("none"));
    }

    print_list(log, tr("REMOVED"), &removed);
    true
}

/// Thank-you note with a link to star the project, then the window closes by itself after 5 s.
fn goodbye(log: &Log, silent: bool) {
    println!();
    log.banner(tr("THANK YOU"));
    println!("{}", tr("  Thanks for your trust in BatteryHub!"));
    println!("{}", tr("  If you like it, please give the project a star on GitHub - it really helps:"));
    println!("  https://github.com/{}", batteryhub::update::REPO);
    println!();
    if silent {
        return;
    }
    for left in (1..=5).rev() {
        print!("{}", trf("\r  This window closes in {0} s...  ", &[&left]));
        let _ = std::io::stdout().flush();
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    println!();
}

// ---- entry point -------------------------------------------------------------------------------

pub fn run(ed: Edition, payload: &'static [u8]) {
    let o = opts();
    let title = trf("{0} {1} - Setup", &[&ed.name, &inst::version_label()]);
    let log = Log::new(&title);
    log.banner(&trf("{0}   (per-user, no administrator rights needed)", &[&title]));

    let st = scan(&ed, &log);
    let installed_ver = st.version.clone().unwrap_or_else(|| "0".into());
    let can_update = st.installed && inst::version_cmp(&installed_ver, inst::VERSION).is_lt();
    let same = st.installed && !can_update;

    let action = match o.action {
        Some(a) => Some(a),
        None if o.silent => Some(if can_update { Action::Update } else { Action::Install }),
        None => {
            println!();
            log.banner(tr("WHAT DO YOU WANT TO DO?"));
            let label = |avail: bool, s: String| if avail { s } else { format!("\x1b[90m{s}\x1b[0m") };
            let install_txt = if same {
                trf("[1] Reinstall   - write v{0} again (repair files and shortcuts)", &[&inst::VERSION])
            } else if st.installed {
                trf("[1] Install     - fresh install of v{0}", &[&inst::VERSION])
            } else {
                trf("[1] Install     - install v{0} to {1}", &[&inst::VERSION, &ed.default_dir().display()])
            };
            println!("  {}", label(true, install_txt));
            println!(
                "  {}",
                label(
                    can_update,
                    if can_update {
                        trf("[2] Update      - v{0} -> v{1} (keeps your settings and autostart)", &[&installed_ver, &inst::VERSION])
                    } else if st.installed {
                        trf("[2] Update      - not available: v{0} is already up to date", &[&st.version.as_deref().unwrap_or("?")])
                    } else {
                        tr("[2] Update      - not available: not installed yet").into()
                    }
                )
            );
            println!(
                "  {}",
                label(
                    st.installed,
                    if st.installed {
                        trf("[3] Uninstall   - remove {0} from this PC", &[&ed.name])
                    } else {
                        tr("[3] Uninstall   - not available: not installed").into()
                    }
                )
            );
            println!("  {}", tr("[0] Exit"));
            let mut valid = vec!['1', '0'];
            if can_update {
                valid.push('2');
            }
            if st.installed {
                valid.push('3');
            }
            valid.sort();
            match choose(&log, &valid) {
                '1' => Some(Action::Install),
                '2' => Some(Action::Update),
                '3' => Some(Action::Uninstall),
                _ => None,
            }
        }
    };

    println!();
    let ok = match action {
        Some(Action::Install) => install(&ed, payload, &log, &o, &st, false),
        Some(Action::Update) if st.installed => install(&ed, payload, &log, &o, &st, true),
        Some(Action::Update) => {
            log.error(tr("Nothing to update: not installed."));
            false
        }
        Some(Action::Uninstall) if !o.silent && !log.ask(&trf("Really uninstall {0}?", &[&ed.name]), false) => {
            log.info(tr("Cancelled. Nothing was changed."));
            true
        }
        Some(Action::Uninstall) => uninstall(&ed, &log, &st),
        None => {
            log.info(tr("Exit. Nothing was changed."));
            true
        }
    };

    if action.is_some() {
        println!();
        let after = read_state(&ed);
        log.banner(tr("RESULT"));
        if ok {
            let now = if after.installed {
                trf("installed (v{0}) in {1}", &[&inst::VERSION, &after.dir.display()])
            } else {
                tr("not installed").into()
            };
            log.ok(trf("{0} is now {1}", &[&ed.name, &now]));
            if after.installed {
                row("Program file", file_line(&after.exe));
                row("Desktop shortcut", if after.desktop { ed.desktop_shortcut().display().to_string() } else { tr("(none)").into() });
                row("Start menu shortcut", if after.start_menu { ed.start_menu_shortcut().display().to_string() } else { tr("(none)").into() });
                if ed.run_value.is_some() {
                    row("Autostart", autostart_line(&ed, &after));
                }
            }
        } else {
            log.error(tr("Setup did not complete. See the messages above."));
        }
    }
    if ok {
        goodbye(&log, o.silent);
    } else {
        // Keep the window open on an error, so the messages above can be read.
        if !o.silent {
            log.pause();
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autostart_matching_is_strict() {
        let t = inst::TASKBAR;
        let dir = t.default_dir();
        assert!(starts_edition(&t, &dir, "BatteryHub Taskbar", "\"x.exe\""));
        assert!(starts_edition(&t, &dir, "x", &format!("\"{}\"", dir.join(t.exe).display())));
        assert!(starts_edition(&t, &dir, "x", r#""D:\Tools\batteryhub-taskbar.exe""#));
        // The original C++ app and unrelated programs never match.
        assert!(!starts_edition(&t, &dir, "WirelessBattery", r#""C:\Program Files\BatteryHub\BatteryHub.exe""#));
        assert!(!starts_edition(&t, &dir, "LGHUB", r#""C:\Program Files\LGHUB\lghub_system_tray.exe" --minimized"#));
    }

    #[test]
    fn versions() {
        assert!(inst::version_cmp("0.9", "1.0.0").is_lt());
        assert!(inst::version_cmp("1.0", "1.0.0").is_eq());
        assert!(inst::version_cmp("1.0.1", "1.0.0").is_gt());
        assert_eq!(fmt_time(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_795_283_000)), "2026-11-21 17:43 UTC");
    }
}
