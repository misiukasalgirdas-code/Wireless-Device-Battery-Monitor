//! Device model and the driver chain (port of devices.h / devices.cpp / drivers.h).
use std::collections::BTreeSet;

use crate::drivers;
use crate::i18n::tr;
use crate::hid::{self, HidColl};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DeviceKind {
    #[default]
    Other,
    Mouse,
    Headset,
    Keyboard,
    Gamepad,
    Phone,
    Microphone,
}

impl DeviceKind {
    /// Segoe MDL2 / Fluent icon glyph for the kind (same font the original draws with).
    pub fn glyph(self) -> char {
        match self {
            DeviceKind::Mouse => '\u{E962}',
            DeviceKind::Headset => '\u{E7F6}',
            DeviceKind::Keyboard => '\u{E765}',
            DeviceKind::Gamepad => '\u{E7FC}',
            DeviceKind::Phone => '\u{E8EA}',
            DeviceKind::Microphone => '\u{E720}',
            DeviceKind::Other => '\u{E83F}',
        }
    }

    pub fn label(self) -> &'static str {
        crate::i18n::tr(match self {
            DeviceKind::Mouse => "Mouse",
            DeviceKind::Headset => "Headset",
            DeviceKind::Keyboard => "Keyboard",
            DeviceKind::Gamepad => "Controller",
            DeviceKind::Phone => "Phone",
            DeviceKind::Microphone => "Microphone",
            DeviceKind::Other => "Device",
        })
    }
}

/// One battery-powered device, as seen during a single poll.
#[derive(Clone, Debug)]
pub struct DeviceStatus {
    /// Stable identity across polls.
    pub key: String,
    pub name: String,
    /// "2.4 GHz receiver", "Bluetooth", "USB cable", ...
    pub via: String,
    pub kind: DeviceKind,
    /// 0..=100, or -1 when unknown right now.
    pub percent: i32,
    /// Extra breakdown, e.g. "L 80%, R 75%, case 40%".
    pub detail: String,
    /// Set when the device only reports coarse levels ("Low", "Full", ...).
    pub level_text: String,
    pub charging: bool,
    /// On power and fully charged.
    pub full: bool,
    pub asleep: bool,
    /// false: receiver present, device switched off.
    pub connected: bool,
    pub millivolts: i32,
    /// Keep showing the last value this long if the device stops answering.
    pub grace_sec: u32,
    pub vid: u16,
    /// The USB / HID product (vid, pid) the device is read through - its receiver, cable or BT HID
    /// link - or None when it is not read over HID (Bluetooth properties, BLE broadcasts, XInput).
    /// When that hardware is gone, the device is gone too (not "asleep").
    pub hw: Option<(u16, u16)>,
}

impl Default for DeviceStatus {
    fn default() -> Self {
        Self {
            key: String::new(),
            name: String::new(),
            via: String::new(),
            kind: DeviceKind::Other,
            percent: -1,
            detail: String::new(),
            level_text: String::new(),
            charging: false,
            full: false,
            asleep: false,
            connected: true,
            millivolts: 0,
            grace_sec: 0,
            vid: 0,
            hw: None,
        }
    }
}

impl DeviceStatus {
    /// Short status text, e.g. "93%", "45% ⚡", "Full ✓", "?".
    pub fn status_text(&self) -> String {
        if !self.connected {
            return tr("disconnected").into();
        }
        let mut t = self.level();
        if self.is_full() {
            t.push_str(" ✓");
        } else if self.charging {
            t.push_str(" ⚡");
        }
        if self.asleep {
            t.push_str(tr(" (asleep)"));
        }
        t
    }

    /// On power and fully charged: reported as full, or charging at 100%.
    pub fn is_full(&self) -> bool {
        self.connected && (self.full || (self.charging && self.percent >= 100))
    }

    /// Charging (on power, not yet full).
    pub fn is_charging(&self) -> bool {
        self.connected && self.charging && !self.is_full()
    }

    /// Level text: "32%", a coarse level ("Low"), or "?".
    pub fn level(&self) -> String {
        if self.percent >= 0 {
            format!("{}%", self.percent)
        } else if !self.level_text.is_empty() {
            crate::i18n::tr_any(&self.level_text).to_string()
        } else {
            "?".into()
        }
    }

    /// "32%, charging" / "100%, fully charged" / "32%, on battery" - for menus and tooltips.
    pub fn charge_text(&self) -> String {
        let state = if !self.connected {
            "disconnected"
        } else if self.is_full() {
            "fully charged"
        } else if self.charging {
            "charging"
        } else if self.asleep {
            "asleep, last known level"
        } else {
            "on battery"
        };
        format!("{}, {}", self.level(), tr(state))
    }
}

/// Shared state handed through the driver chain. A driver claims a (vid, pid) once it has handled
/// it, so the generic fallbacks further down do not report it twice.
#[derive(Default)]
pub struct PollContext {
    pub colls: Vec<HidColl>,
    pub claimed: BTreeSet<(u16, u16)>,
    /// Container ids the Bluetooth fallback already reported.
    pub bt_containers: BTreeSet<String>,
    pub out: Vec<DeviceStatus>,
}

impl PollContext {
    pub fn is_claimed(&self, vid: u16, pid: u16) -> bool {
        self.claimed.contains(&(vid, pid))
    }
    pub fn claim(&mut self, vid: u16, pid: u16) {
        self.claimed.insert((vid, pid));
    }
    pub fn add(&mut self, mut s: DeviceStatus, vid: u16, pid: u16) {
        if s.vid == 0 {
            s.vid = vid;
        }
        self.out.push(s);
        self.claim(vid, pid);
    }
    /// Distinct (vid, pid) pairs of one vendor that are not claimed yet.
    pub fn products(&self, vid: u16) -> Vec<(u16, u16)> {
        let set: BTreeSet<(u16, u16)> = self
            .colls
            .iter()
            .filter(|c| c.vid == vid && !self.is_claimed(c.vid, c.pid))
            .map(|c| (c.vid, c.pid))
            .collect();
        set.into_iter().collect()
    }
    /// Collections of one product matching `pred` (cloned so drivers can mutate the context).
    pub fn find(&self, vid: u16, pid: u16, pred: impl Fn(&HidColl) -> bool) -> Vec<HidColl> {
        self.colls
            .iter()
            .filter(|c| c.vid == vid && c.pid == pid && pred(c))
            .cloned()
            .collect()
    }
}

/// HKCU\Software\WirelessBattery\IgnoreDevices: "VID:PID" pairs (same setting as the original).
fn ignored_devices() -> Vec<(u16, u16)> {
    let Some(text) = crate::util::reg_get_string("Software\\WirelessBattery", "IgnoreDevices") else {
        return Vec::new();
    };
    text.split([',', ';', ' '])
        .filter_map(|p| {
            let (v, d) = p.trim().split_once(':')?;
            Some((u16::from_str_radix(v, 16).ok()?, u16::from_str_radix(d, 16).ok()?))
        })
        .collect()
}

/// HID collections from the last enumeration. Enumerating opens every HID device on the system,
/// so a periodic battery read reuses this list; it is rebuilt on a rescan (device change, manual
/// "Refresh devices", start-up) and at least every RESCAN_AFTER.
static HID_CACHE: std::sync::Mutex<Option<HidCache>> = std::sync::Mutex::new(None);
const RESCAN_AFTER: std::time::Duration = std::time::Duration::from_secs(10 * 60);

struct HidCache {
    at: std::time::Instant,
    /// Interface paths present at the last enumeration (sorted).
    paths: Vec<String>,
    colls: Vec<HidColl>,
}

/// The HID collections to read. A cached list is reused only while the set of HID interfaces is
/// unchanged (a cheap SetupAPI listing, no device opened), so a device plugged in or removed is
/// picked up by the next read even if the device-change message was missed.
fn hid_collections(rescan: bool) -> Vec<HidColl> {
    let mut cache = HID_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let paths = hid::interface_paths();
    if let Some(c) = cache.as_ref() {
        if !rescan && c.at.elapsed() < RESCAN_AFTER && c.paths == paths {
            return c.colls.clone();
        }
    }
    let colls = hid::enum_hid();
    *cache = Some(HidCache { at: std::time::Instant::now(), paths, colls: colls.clone() });
    crate::drivers::sdi::forget_containers();
    colls
}

/// Quick read of only the given receivers / cables (vid, pid): used to notice soon that a device
/// behind a plugged-in receiver was switched on or woke up, which Windows does not report.
/// Bluetooth, BLE and XInput are not touched.
pub fn poll_hardware(ids: &[(u16, u16)]) -> Vec<DeviceStatus> {
    let colls: Vec<HidColl> = hid_collections(false).into_iter().filter(|c| ids.contains(&(c.vid, c.pid))).collect();
    if colls.is_empty() {
        return Vec::new();
    }
    let mut ctx = PollContext { colls, ..Default::default() };
    drivers::steelseries::poll(&mut ctx);
    drivers::logitech::poll(&mut ctx);
    drivers::razer::poll(&mut ctx);
    drivers::roccat::poll(&mut ctx);
    drivers::attackshark::poll(&mut ctx);
    drivers::blackshark::poll(&mut ctx);
    drivers::wlmouse::poll(&mut ctx);
    drivers::playstation::poll(&mut ctx);
    drivers::generic::poll(&mut ctx);
    ctx.out
}

/// What to do with a device that was shown before but is missing from the latest read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Missing {
    /// Gone: unplugged receiver / cable, or a Bluetooth / BLE / XInput device that disconnected.
    Remove,
    /// Its receiver is still there and the device sleeps: keep the last value, dimmed.
    Asleep,
    /// Its hardware is still there and it missed one answer: keep the last value as it was.
    Keep,
}

/// Decides about a device missing from a read. `hw` is the hardware it is read through and
/// `hw_present` whether that hardware is still plugged in; `age_sec` is the time since it last
/// answered. Bluetooth / BLE / XInput devices (no `hw`) are only reported while connected, so a
/// missing one has disconnected.
pub fn missing_device(hw: Option<(u16, u16)>, hw_present: bool, grace_sec: u32, age_sec: u64, min_grace_sec: u64) -> Missing {
    if hw.is_none() || !hw_present {
        return Missing::Remove;
    }
    if grace_sec > 0 && age_sec < grace_sec as u64 {
        Missing::Asleep
    } else if age_sec < min_grace_sec {
        Missing::Keep
    } else {
        Missing::Remove
    }
}

#[cfg(test)]
mod missing_tests {
    use super::*;
    const RX: Option<(u16, u16)> = Some((0x046D, 0xC543));

    #[test]
    fn unplugged_receiver_removes_at_once() {
        assert_eq!(missing_device(RX, false, 1800, 5, 150), Missing::Remove);
    }

    #[test]
    fn sleeping_mouse_on_a_plugged_receiver_is_asleep() {
        assert_eq!(missing_device(RX, true, 1800, 120, 150), Missing::Asleep);
        assert_eq!(missing_device(RX, true, 1800, 1801, 150), Missing::Remove);
    }

    #[test]
    fn one_missed_answer_keeps_the_value_briefly() {
        assert_eq!(missing_device(RX, true, 0, 60, 150), Missing::Keep);
        assert_eq!(missing_device(RX, true, 0, 151, 150), Missing::Remove);
    }

    #[test]
    fn bluetooth_devices_disappear_when_disconnected() {
        assert_eq!(missing_device(None, false, 300, 1, 150), Missing::Remove);
    }
}

/// Whether a HID product (vid, pid) was present in the latest device enumeration.
pub fn hid_present(id: (u16, u16)) -> bool {
    let cache = HID_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.as_ref().is_some_and(|c| c.colls.iter().any(|c| (c.vid, c.pid) == id))
}

/// Queries every supported device, enumerating them first. Blocking (a second or two); call from a
/// worker thread.
pub fn poll_all_devices() -> Vec<DeviceStatus> {
    poll_devices(true)
}

/// Queries every supported device. `rescan` = enumerate the HID devices again; otherwise the list
/// from the last enumeration is reused (only the battery levels are read).
pub fn poll_devices(rescan: bool) -> Vec<DeviceStatus> {
    // WinRT (BLE watcher, Windows.Gaming.Input) needs the poll thread in the MTA; harmless if repeated.
    unsafe {
        let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
    }
    let out = poll_with(hid_collections(rescan));
    // Broadcast listening is only needed while reading; between reads it would keep waking the
    // process for every Bluetooth LE advertisement nearby.
    crate::ble::stop_watcher();
    out
}

fn poll_with(colls: Vec<HidColl>) -> Vec<DeviceStatus> {
    let mut ctx = PollContext { colls, ..Default::default() };
    let ignored = ignored_devices();
    if !ignored.is_empty() {
        ctx.colls.retain(|c| !ignored.contains(&(c.vid, c.pid)));
    }
    // Vendor drivers first, generic fallbacks last (same order as the original chain).
    drivers::steelseries::poll(&mut ctx);
    drivers::logitech::poll(&mut ctx);
    drivers::razer::poll(&mut ctx);
    drivers::roccat::poll(&mut ctx);
    drivers::attackshark::poll(&mut ctx);
    drivers::blackshark::poll(&mut ctx);
    drivers::wlmouse::poll(&mut ctx);
    drivers::playstation::poll(&mut ctx);
    drivers::airpods::poll(&mut ctx);
    drivers::bluetooth::poll(&mut ctx);
    drivers::generic::poll(&mut ctx);
    drivers::xinput::poll(&mut ctx);
    ctx.out
}

/// Prints every HID collection (to help adding support for new devices).
pub fn dump_hid_collections() -> String {
    let mut out = String::from("VID:PID    IF  UsagePg:Usage  In  Out Feat  Product\n");
    for c in hid::enum_hid() {
        out += &format!(
            "{:04X}:{:04X}  {:2}  {:04X}:{:04X}    {:3} {:3} {:3}   {}\n",
            c.vid,
            c.pid,
            c.iface,
            c.page(),
            c.usage(),
            c.in_len(),
            c.out_len(),
            c.feat_len(),
            hid::product_string(&c)
        );
    }
    out
}
