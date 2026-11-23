<p align="center"><img src="res/batteryhub.png" width="112" alt="BatteryHub icon"></p>

<h1 align="center">BatteryHub v1.4.2</h1>

<p align="center">
<b>An ultra-lightweight Rust implementation of BatteryHub for Windows.</b><br>
Battery levels of your wireless mouse, headset, keyboard and controller, right on the taskbar,<br>
in about <b>0.2 MB of RAM</b>.
</p>

<p align="center">
<a href="../../releases/latest"><img src="https://img.shields.io/github/v/release/misiukasalgirdas-code/Wireless-Device-Battery-Monitor?label=download&logo=windows" alt="Latest release"></a>
<a href="../../actions/workflows/ci.yml"><img src="../../actions/workflows/ci.yml/badge.svg" alt="CI"></a>
<img src="https://img.shields.io/badge/written%20in-Rust-dea584?logo=rust" alt="Written in Rust">
<img src="https://img.shields.io/badge/license-GPL--3.0-blue" alt="GPL-3.0">
</p>

<p align="center">
<img src="res/screenshot-taskbar.png" width="440" alt="BatteryHub on the taskbar: mouse 86% charging (green bolt), headset 80%"><br>
<img src="res/screenshot-tooltip.png" width="430" alt="Hover tooltip: device name, level, charge state, connection and warning level">
</p>

> [!IMPORTANT]
> **This is a recode of krisgfx9/Wireless-Device-Battery-Monitor** (the C++ BatteryHub by krisgfx9), not a fork and not the original.
> It rewrites the original in Rust. The idea, the look of the taskbar strip and the device protocols all come from the original project, and the credit for them goes to its authors.
>
> **The original is currently unavailable on GitHub.** On 22 November 2026 the krisgfx9 account was suspended by GitHub, which took the original repository offline with it. According to the original author, no reason was given; they have appealed, so the original may come back. Until then, the original author shares downloads through the project's Discord.
>
> This recode is still **not the original**, but it **is supported and will stay supported**: it keeps being developed and updated, with new devices added from [device requests](../../issues).

Developed by [@ferloom](https://github.com/ferloom) with the help of Claude (AI).
This fork by [@misiukasalgirdas-code](https://github.com/misiukasalgirdas-code) adds a **Lithuanian** user interface (see [Languages](#languages)); updates and device requests go to this fork.

## Languages

The interface is available in **English** and **Lithuanian** (lietuvių). On a Lithuanian Windows it starts in Lithuanian, otherwise in English; switch any time with **Language** in the right-click menu. The choice is saved as `Language` (`en` / `lt`) under `HKCU\Software\BatteryHub\Taskbar`. The setup follows the same choice. Device requests sent to GitHub stay in English.

Sąsaja yra **anglų** ir **lietuvių** kalbomis. Lietuviškoje Windows programa pasileidžia lietuviškai; kalbą galima pakeisti bet kada dešiniuoju pelės mygtuku atidarius meniu **Kalba**.

---

## Contents

[Download](#download) · [Languages](#languages) · [Problem with a new version?](#problem-with-a-new-version-downgrade-and-report-it) · [Features](#features) · [Using it](#using-it) · [Setup](#setup-install--update--uninstall) · [Resource usage vs. the original](#resource-usage-vs-the-original) · [Taskbar compatibility](#taskbar-compatibility) · [Supported devices](#supported-devices) · [Testing](#testing) · [Building](#building-from-source) · [License](#license)

## Download

Get **`BatteryHub-Taskbar-Setup-v1.4.2.exe`** from the [**latest release**](../../releases/latest), run it and choose **[1] Install**.

- No administrator rights needed (per-user install).
- Already installed? Use *Check for updates...* in the right-click menu, or run the new setup and choose **[2] Update**.
- It does not interfere with the original C++ BatteryHub if you have that installed too.
- Windows may show a SmartScreen warning because the file is not code-signed: *More info* → *Run anyway*.

## Problem with a new version? Downgrade and report it

If something breaks after an update, you can always go back to an older version by hand:

1. Open [**Releases**](../../releases) and download the setup of an earlier version (e.g. `BatteryHub-Taskbar-Setup-v1.0.1.1.exe`).
2. Run it. It finds the newer version you have installed and offers **[1] Reinstall**: choose it, and it replaces the program with its own (older) version. Your warning levels and refresh options are kept; it asks again whether BatteryHub should start with Windows.

*Check for updates* never offers an older version, so the downgrade stays in place until you update again. Versions before v1.1 do not have *Check for updates*; to come back later, run the newer setup and choose **[2] Update**.

**Please also report the problem** in [**Issues**](../../issues/new) so it can be fixed in the next version. It helps to include:

- the version that has the problem and the one that works (both are shown at the top of the setup window);
- Windows version and taskbar (Windows 11, Windows 10, ExplorerPatcher: normal or small taskbar);
- your devices (e.g. "Logitech PRO 2 LIGHTSPEED on its receiver", "AirPods over Bluetooth");
- what happened and what you expected, ideally with a screenshot of the taskbar.

## Features

- **On the taskbar, next to the clock:** one icon and percentage per device (`🎧 27%  🖱 96%`), in the original's look (Segoe Fluent Icons, Segoe UI Variable, ClearType), following light / dark mode and DPI.
- **Hover a device** for a short summary: `PRO 2 LIGHTSPEED: 96%` (plus *charging*, *full* or *asleep*).
- **Charging at a glance:** green ⚡ while charging, blue ⚡ at 100%, red percentage at or below the device's warning level.
- **Per-device warning level:** 50 / 40 / 30 / 20 / 15 / 10 / 5%.
- **Reacts at once** when a receiver, cable or Bluetooth device connects or disconnects, and within ~10 s when a headset on a 2.4 GHz dongle is switched on or off.
- **Refresh interval** of 1 / 5 / 10 / 30 minutes or 1 hour.
- **More monitors:** show the widget on all monitors, the main one only, or only one other monitor.
- **Position and background:** move the widget away from the clock (e.g. to make room for another taskbar widget), and an optional plate behind it for transparent taskbars.
- **Start with Windows** and **automatic update check** switched on or off right from the menu.
- **Check for updates** from the menu, with a SHA-256-verified download.
- **Request support for a device** from the menu: pick a device that is not shown, and a GitHub issue opens filled in with what is needed to add it.
- **Tiny:** about 0.2 MB of RAM when idle, about 0.1 s of CPU per minute, a 500 KB program.
- **Safe fallback** to notification-area icons on a vertical taskbar or an unknown Explorer layout.

## Using it

| On the strip | Meaning |
|---|---|
| **Red** percentage | At or below the device's warning level (default 15%) |
| **Green ⚡** | Plugged in and charging |
| **Blue ⚡** | Plugged in and charged to 100% |
| Dimmed | Asleep (receiver still plugged in), showing the last known value |

Left click does nothing (on purpose: the strip stays out of the way). **Right click** opens the menu:

| Menu item | What it does |
|---|---|
| *Device name (level, state)* ▸ | *Red warning at or below* 50 / 40 / 30 / 20 / **15** / 10 / 5% for that device |
| *Refresh devices* | Full rescan; devices that are gone disappear |
| *Refresh battery levels* | Reads the levels again |
| *Refresh battery every* ▸ | **1 minute** / 5 / 10 / 30 minutes / 1 hour |
| *Refresh when a device connects or disconnects* | **On** by default; see below |
| *Background behind taskbar text (for translucent taskbars)* | **Off** by default. A rounded plate in the taskbar's colour behind the widget, so it stays readable on a transparent taskbar (TranslucentTB, Windhawk Taskbar Styler...) |
| *Position on the taskbar* ▸ | **Next to the notification area** or 20 to 200 px to the left of it (in 20 px steps), e.g. to make room for another taskbar widget (iCUE, network meters...). Also applies on other monitors. A value set by hand (`TaskbarOffset`) shows as *Custom* |
| *Height on the taskbar* ▸ | 5 / 10 / 15 px up, **Centred**, or 5 / 10 / 15 px down. Also applies on other monitors. A value set by hand (`TaskbarOffsetY`) shows as *Custom* |
| *Show widget on* ▸ | Only with more than one monitor: *All monitors* / **Main monitor only** / *Display 2 (left) only*... The monitor is remembered by its identity, not its number; while it is unplugged the widget is on the main taskbar. Other monitors need *Show my taskbar on all displays* in Windows (taskbars of other programs, e.g. DisplayFusion, are not supported) |
| *Automatically start with Windows* | Adds or removes BatteryHub's autostart entry (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run\BatteryHub Taskbar`) |
| *Automatically check for updates* | **Off** by default. When on, BatteryHub checks GitHub once, about 2 s after it starts (not while it runs; if the network is not up yet it tries again every 15 s, up to 8 times); if there is a new version you get the same question as with *Check for updates...* |
| *Install updates silently* | **Off** by default; needs the automatic check. When the check finds a new version, BatteryHub downloads it, verifies its SHA-256 checksum and installs it in the background: no question, no setup window, no "Update finished" message. Settings and autostart are kept. |
| *Request support for a device...* | Lists every device BatteryHub can see (HID, USB and Bluetooth; vendor-specific and audio devices first). Pick yours, check the name and type, add notes, and *Send* opens a new GitHub issue in your browser, filled in with exactly what the window shows: name, type, your name (optional, the issue is public), notes, vendor / product id, HID and USB interface summary, Windows build. You submit it there (a GitHub account is needed); nothing is sent from the app. *Copy full report* copies the same plus the list of all devices. Also available as `batteryhub-taskbar.exe --report` |
| *Check for updates...* | "No new updates", or "A new update is available: v1.1 → v1.x - update now?". *Yes* downloads the setup, checks it against the release's SHA-256 checksum and starts it; it updates and restarts BatteryHub, keeping your settings and autostart |
| *Exit* | Closes BatteryHub |

Tooltip and menu follow the taskbar theme (dark or light); the *Request support* window follows the Windows app theme.

**When a device comes or goes** (with *Refresh when a device connects or disconnects* on):

| Event | Shown after |
|---|---|
| USB receiver / cable plugged in or removed | ~1–3 s (Windows device notifications; every read also checks whether the set of HID devices changed) |
| Bluetooth device (AirPods, headphones, controllers) connects or disconnects | a few seconds (connections are checked every 3 s); AirPods only while connected to this PC |
| Headset on a 2.4 GHz dongle switched on or off | ~10 s (headset dongles are checked every 10 s) |
| Mouse / keyboard asleep, receiver still plugged in | stays, dimmed, with its last level |
| PC wakes up | a few seconds |

## Setup: Install / Update / Uninstall

The setup is a console window with a detailed log. Before it shows any options, it **scans your PC**:

- whether BatteryHub is installed, which version, and where;
- the program file (size and date), the shortcuts, autostart, and whether it is running;
- whether the original C++ BatteryHub is installed (reported, never touched).

| Option | When | What it does |
|---|---|---|
| **[1] Install** / **Reinstall** | always | Installs, creates desktop and Start menu shortcuts and asks *"Add to Windows autostart? [Y/n]"*. Logs every block written (with CRC-32 check) and every file and registry entry it creates |
| **[2] Update** | an older version is installed | Replaces the program and keeps your settings and autostart; restarts it if it was running |
| **[3] Uninstall** | installed | Stops the program, checks every autostart location (HKCU / HKLM Run, RunOnce, Task Manager startup state, Startup folder) for entries that start it and removes them, then the shortcuts, files and settings. Lists everything it removed |
| **[0] Exit** | always | Changes nothing |

BatteryHub also appears in **Settings → Apps → Installed apps**, where *Uninstall* runs the same uninstall.

At the end it thanks you, links to the project (a ⭐ helps!) and closes itself after 5 seconds. On an error it stays open.

Command line for scripts: `/install`, `/update`, `/uninstall`, `/S` (silent), `/autostart=yes|no`, `/nolaunch`, `/nonotify` (no "Update finished" message afterwards).

| Where the files go | |
|---|---|
| Program | `%LOCALAPPDATA%\Programs\BatteryHub\Taskbar\batteryhub-taskbar.exe`, plus `BatteryHub-Setup.exe` (a copy of the setup that *Uninstall* in Settings runs) |
| Settings → Apps entry | `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\BatteryHub Taskbar` |
| Shortcuts | Desktop and `Start Menu\Programs\BatteryHub\BatteryHub Taskbar.lnk` |
| Settings | `HKCU\Software\BatteryHub\Taskbar`: `Thresholds` (per device), `RefreshMinutes`, `RefreshOnDeviceChange`, `TaskbarOffset` (DWORD, gap from the clock in pixels), `TaskbarOffsetY` (DWORD, up / down shift in pixels, + = down), `WidgetMonitor` (`all`, `main` or a monitor id), `WidgetBackground`, `AutoCheckUpdates`, `SilentUpdates` |
| Autostart | `HKCU\...\Run` → `BatteryHub Taskbar` (if you chose it) |

The original C++ BatteryHub uses different names (`Program Files\BatteryHub`, `HKCU\Software\WirelessBattery`, autostart `WirelessBattery`) and is never touched.

## Resource usage vs. the original

Both programs running **side by side on the same PC**, the same devices connected (Logitech LIGHTSPEED mouse, SteelSeries 2.4 GHz headset dongle, Bluetooth), default settings (refresh every minute), sampled every 2 s for 3 minutes after start-up. Method, raw samples and older runs: [bench/RESULTS.md](bench/RESULTS.md).

| | **BatteryHub (Rust) v1.1** | BatteryHub (C++) 1.1.0 | |
|---|---:|---:|---:|
| **RAM in Task Manager** ("Memory" column), median | **0.20 MB** | 4.66 MB | **23× less** |
| RAM in Task Manager, average / max | 0.33 / 0.82 MB | 4.65 / 4.92 MB | |
| Working set incl. shared system DLLs | **1.3 MB** | 27.5 MB | 21× less |
| Private bytes (commit) | **4.6 MB** | 6.0 MB | |
| CPU time per minute | **0.11 s** | 0.88 s | 8× less |
| Threads | **9** | 15 | |
| Handles | 367 | 375 | |
| Program file | **500 KB** | 1,958 KB | 4× smaller |

What keeps it small: memory needed only for a moment (a read, a Bluetooth check) is handed back to Windows right after; settings are cached; Bluetooth LE is listened to only while reading; the device list is cached and rescanned only when it changes; only Bluetooth device nodes are enumerated; the Xbox input APIs and WinHTTP are loaded only when needed.

**To be fair:** the original does a lot more (about 100 device drivers, battery history, brand colours and more options). Part of the difference is less code doing less work, not only the language.

## Taskbar compatibility

The strip is a small layered window placed inside the taskbar, left of the notification area, the same technique the original uses. It depends on Explorer's window layout, so BatteryHub checks it every second and falls back safely:

| Situation | What BatteryHub does |
|---|---|
| Windows 11, Windows 10, ExplorerPatcher (also the small Windows 10 taskbar) | Strip next to the clock; on the Windows 10 layout the task buttons are narrowed so they never slide under it |
| Explorer restarts | Recreates exactly one strip on the new taskbar (stray strip windows are removed every second) |
| Taskbar height, DPI or theme changes (e.g. after leaving full screen) | Re-renders the strip |
| Vertical taskbar, unknown Explorer layout, or the strip cannot be placed for 3 s | **One notification-area icon per device** (level drawn in the icon, same tooltip and menu), back to the strip when possible |

## Supported devices

| Family | Notes |
|---|---|
| Logitech | HID++ 1.0 / 2.0 over Unifying, Bolt, LIGHTSPEED, Bluetooth and USB, plus G headsets (voltage curves) |
| SteelSeries | Arctis 1 / 7 / 9 / Pro, Arctis Nova (3P, 5, 7, 7+, Pro), GameBuds, Aerox, Prime, Rival |
| Razer | Mice, keyboards and receivers (90-byte feature report protocol); BlackShark V3 headset over its HyperSpeed dongle `1532:057A` or USB cable `1532:0579` (protocol from OpenRazer PR #2794 and Joule) |
| WLMOUSE | Beast X, Beast X Mini, Mini Pro and Max over their receivers (protocol from [HaloBattery](https://github.com/HeyOkay/HaloBattery), MIT) |
| ROCCAT | Syn Max Air headset over its 2.4 GHz receiver (voltage curve; coarse level while charging) |
| Attack Shark | Mice (e.g. X1, X11) on the Xenta 2.4 GHz receiver `1D57:FA60` (protocol from [Attack Shark Battery KDE](https://gitlab.com/gabriel17166/Attack-Shark-Battery-KDE) and [attackshark-battery-windows](https://github.com/daeoxide/attackshark-battery-windows), both MIT) |
| Sony | DualShock 4, DualSense, DualSense Edge (USB and Bluetooth) |
| Apple | AirPods and Beats, from BLE proximity broadcasts (L / R / case) |
| Bluetooth | Any device Windows knows a battery level for, plus the Google Fast Pair L / R / case split |
| Generic HID | Devices exposing the standard Battery Strength usage |
| Xbox | Windows.Gaming.Input (exact %) and XInput (coarse levels) |

Device not shown? Use *Request support for a device...* in the right-click menu (see above). More brands from the original project can be added as one file each in `src/drivers/`; pull requests are welcome (the ROCCAT driver came from one). Devices can be excluded with `HKCU\Software\WirelessBattery\IgnoreDevices` (`VID:PID` list, the same setting as the original).

## Testing

- **Unit tests** (`cargo test --lib`) decode recorded device reports without the hardware: SteelSeries Arctis Nova status blocks, DualSense / DualShock 4 input reports, AirPods proximity broadcasts, Google Fast Pair battery fields, Logitech voltage curves and receiver grouping. They also cover when a device is removed or shown as asleep, warning levels, colours, menu ids, the GitHub release parser, SHA-256 and version comparison.
- The **setup** has its own tests (autostart matching never touches the original app, version handling).
- **CI** ([GitHub Actions](.github/workflows/ci.yml)) runs all tests and builds the program and the setup on every push.
- **Resource usage** is measured with [bench/compare.ps1](bench/compare.ps1) (two programs side by side) and [bench/measure.ps1](bench/measure.ps1).

## Building from source

Requirements: Rust 1.99 or newer (stable, MSVC toolchain; the crates use the 2024 edition) and Visual Studio Build Tools with the Windows SDK (`rc.exe` embeds the icon).

```powershell
.\build-release.ps1
```

This builds the program, then the setup with the program embedded (`installer/`, `BH_PAYLOAD_DIR`), and writes `release\BatteryHub-Taskbar-Setup-v1.4.2.exe` and `SHA256SUMS.txt`. The version comes from the `VERSION` file (up to four parts, e.g. `1.0.1.1`).

Releases are made by GitHub Actions (`.github/workflows/release.yml`): set the new version in `VERSION`, add its section to `RELEASE_NOTES.md`, commit, then push a tag with the same version (`git tag v1.4.2`, `git push origin v1.4.2`). The workflow runs `build-release.ps1` and publishes the setup and `SHA256SUMS.txt` with `RELEASE_NOTES.md` as the description.

| Path | Contents |
|---|---|
| `src/` | Library: HID layer, device drivers, BLE watcher, settings, update check, UI and setup helpers |
| `src/bin/taskbar.rs` | The taskbar program |
| `installer/` | The setup (`src/setup.rs`) |
| `res/` | Icon (`make_icon.py` draws it), manifest, resource script |
| `bench/` | Resource measurement scripts and results |
| `.github/workflows/ci.yml` | CI: tests and builds on every push |
| `examples/list.rs` | Console diagnostics: `cargo run --example list -- --dump --ble`, `-- --update` |

## License

GPL-3.0-or-later, the same as the original project. See [LICENSE](LICENSE).
