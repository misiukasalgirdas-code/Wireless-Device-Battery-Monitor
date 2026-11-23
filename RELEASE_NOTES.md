# BatteryHub v1.4.2 (Rust)

An ultra-lightweight Rust implementation of BatteryHub for Windows, a recode of [krisgfx9/Wireless-Device-Battery-Monitor](https://github.com/krisgfx9/Wireless-Device-Battery-Monitor). All credit for the original idea, look and device protocols goes to the original project.

## Download

| File | |
|---|---|
| [**`BatteryHub-Taskbar-Setup-v1.4.2.exe`**](https://github.com/misiukasalgirdas-code/Wireless-Device-Battery-Monitor/releases/download/v1.4.2/BatteryHub-Taskbar-Setup-v1.4.2.exe) | Setup: Install / Update / Uninstall (atsisiųsti / download) |
| [`SHA256SUMS.txt`](https://github.com/misiukasalgirdas-code/Wireless-Device-Battery-Monitor/releases/download/v1.4.2/SHA256SUMS.txt) | Checksum |

The files are also listed under **Assets** at the bottom of this page.

Already installed? Use *Check for updates...* in the right-click menu (v1.1 and later), or run the new setup and choose **[2] Update**. Your settings and autostart choice are kept.

## New in v1.4.2

- **Lithuanian interface** (lietuvių kalba): menus, tooltips, messages, the device request window and the setup. On a Lithuanian Windows it starts in Lithuanian; switch with the new **Language** / **Kalba** item in the right-click menu. English stays the default elsewhere.
- This fork's own updates: *Check for updates* and device requests now use [misiukasalgirdas-code/Wireless-Device-Battery-Monitor](https://github.com/misiukasalgirdas-code/Wireless-Device-Battery-Monitor).

## New in v1.4.1

- Built with the Rust 2024 edition (was 2021). No change in behaviour.

## New in v1.4

- **Install updates silently** (menu, with the automatic check): new versions install in the background without asking or showing anything.
- **Razer BlackShark V3** also over its USB cable.

## New in v1.3.4

- **WLMOUSE** Beast X / Mini / Mini Pro / Max mice.
- **Razer BlackShark V3** headset over its HyperSpeed dongle.

## New in v1.3.3

- **Attack Shark mice:** fixed the mouse not showing up (e.g. X1); now also shows when it is on power.
- **`--listen VID:PID`** diagnostic: records what a device sends, for device requests.

## New in v1.3.2

- **Attack Shark mice** (e.g. X1) on the Xenta 2.4 GHz receiver (1D57:FA60): battery level and *Charging*, shown as asleep while the mouse is off. Protocol from [Attack Shark Battery KDE](https://gitlab.com/gabriel17166/Attack-Shark-Battery-KDE) by Gabriel Marques Ferrarezi (MIT). Not yet confirmed on hardware under Windows; please report in [#2](https://github.com/ferloom/Wireless-Device-Battery-Monitor/issues/2) whether it works for you.

## Fixed in v1.3.1

- **Request support for a device:** the issue it opens showed the version as `vv1.3`; it now says `v1.3.1`.

## New in v1.3

- **New black-and-white icon** for the program, the setup and the shortcuts.
- **Position on the taskbar** now moves the widget in 20 px steps (20 to 200 px).
- **Height on the taskbar:** new menu to move the widget 5 / 10 / 15 px up or down.
- **Listed in Settings → Apps:** BatteryHub now shows up under *Installed apps* with its version, and *Uninstall* there removes it cleanly. The setup keeps a copy of itself in the program folder for this. This also prepares BatteryHub for winget.
- **No Visual C++ Redistributable needed:** the program and the setup now start on a clean Windows. Before, they needed `VCRUNTIME140.dll`, which most PCs have but some don't.
- **Faster automatic update check:** about 2 s after start instead of 20 s. If the network is not up yet (e.g. right after sign-in), it tries again every 15 s, up to 8 times.

## New in v1.2.2

- **Update finished:** after an update (from the menu or with the setup), BatteryHub says once which version it was updated from and to.
- **No flash on the main taskbar** at start-up when the widget is set to show only on another monitor.

## New in v1.2.1

- **Show widget on** (right-click menu, with more than one monitor), like the original: *All monitors*, *Main monitor only*, or *Display 2 (left) only* for any other monitor. The monitor is remembered by its identity; while it is unplugged, the widget is on the main taskbar.
- **Position on the taskbar:** next to the notification area or 40 / 80 / 120 / 160 / 200 px from it, to make room for other taskbar widgets (e.g. an iCUE sensor).
- **Background behind taskbar text (for translucent taskbars):** optional rounded plate in the taskbar's colour, off by default.

## New in v1.2

- **ROCCAT Syn Max Air** headset (1E7D:3A3F) over its 2.4 GHz receiver: level from the battery voltage, *Charging* / *Full* on the dock, disappears when switched off. Contributed by [BrooklynBanana](https://github.com/BrooklynBanana). ROCCAT Swarm must not run at the same time (it holds the receiver).
- **Request support for a device...** (right-click menu): lists every device BatteryHub can see (HID, USB, Bluetooth). Pick the one that is not shown, check its name and type, add notes, and *Send* opens a GitHub issue in your browser, filled in with exactly what the window shows (name, type, notes, vendor / product id, HID and USB interface summary, Windows build). You submit it there; nothing is sent from the app. *Copy full report* copies the same plus the list of all devices. Also `batteryhub-taskbar.exe --report`. Light or dark, following the Windows app theme.
- **Automatically start with Windows** (menu): switch the autostart entry on or off without the setup.
- **Automatically check for updates** (menu, off by default): checks once when BatteryHub starts and, if there is a new version, asks whether to update.
- **Shorter tooltip:** `PRO 2 LIGHTSPEED: 96%` (plus charging / full / asleep).
- **Setup:** the autostart line shows the registry entry (`HKCU\...\Run\BatteryHub Taskbar`) instead of the program path.

## v1.1

- **Check for updates** (right-click menu): asks GitHub for the latest release and shows "No new updates" or "A new update is available: v1.1 -> v1.x - update now?". On *Yes* the setup is downloaded, verified against the release's SHA-256 checksum and started; it updates and restarts BatteryHub.
- **About 0.2 MB of RAM when idle** (Task Manager "Memory" column; was about 1.1 MB): memory that is only needed for a moment is handed back to Windows after every read and Bluetooth check, and settings are cached instead of read from the registry every second.
- **Setup:** when it is done (install, update, uninstall or exit), it thanks you, links to the project for a star and closes itself after 5 seconds. On an error it stays open.

## Fixes since v1.0 (v1.0.1 / v1.0.1.1)

- Unplugged receiver / cable: its device disappears at once instead of showing "asleep".
- Bluetooth devices (AirPods...) disappear / appear right when they disconnect / connect; AirPods only while connected to this PC.
- Headsets switched on or off on a 2.4 GHz dongle, and mice waking up behind a receiver, show within ~10 s.
- No duplicate, garbled strip after an Explorer restart (seen on the small Windows 10 taskbar of ExplorerPatcher).
- Safe fallback to notification-area icons on a vertical taskbar or an unknown Explorer layout; unit tests and CI.

Windows may show a SmartScreen warning because the file is not code-signed: click *More info* -> *Run anyway*.
