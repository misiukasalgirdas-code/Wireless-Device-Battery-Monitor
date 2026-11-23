//! "Request support for a device": the device list and the text of the request.
//!
//! The window (report_window.rs) lets the user pick a device that is not shown, and *Send* opens a
//! new GitHub issue on the project, filled in with what is shown in the window. Nothing is sent from
//! the app itself: the user checks the issue in the browser and submits it there.
//!
//! Privacy: the request holds the device name, its type, the user's own notes and a short technical
//! summary (vendor / product id, HID usage pages and report sizes, USB interface classes, the Windows
//! build). No serial numbers, device paths or Bluetooth addresses.
use std::collections::{BTreeMap, HashMap, HashSet};

use windows::Win32::Devices::Properties::{
    DEVPKEY_Device_BusReportedDeviceDesc, DEVPKEY_Device_Class, DEVPKEY_Device_DeviceDesc, DEVPKEY_Device_FriendlyName,
    DEVPKEY_Device_Service,
};

use crate::drivers::sdi::{DevNode, DevSet, CLASS_OF_DEVICE};
use crate::hid::{self, HidColl};

pub const REPO: &str = crate::update::REPO;

/// Types the user can choose (sent in English).
pub const TYPES: [&str; 5] = ["mouse", "keyboard", "headset", "controller", "other"];

/// One physical device the user can pick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Candidate {
    pub vid: u16,
    pub pid: u16,
    pub name: String,
    /// "if2 FFFF:0001 65/65/65" per HID collection (input / output / feature report length).
    pub hid: Vec<String>,
    /// "HIDClass (HidUsb)", "MEDIA (usbaudio)" ... -> count.
    pub usb: BTreeMap<String, u32>,
    /// Bluetooth device without vendor / product id (e.g. headphones with no HID interface).
    pub bluetooth: bool,
    pub vendor: bool,
    pub mouse: bool,
    pub keyboard: bool,
    pub gamepad: bool,
    pub audio: bool,
}

impl Candidate {
    /// Best guess of the type, as an index into TYPES.
    pub fn type_index(&self) -> usize {
        if self.gamepad {
            3
        } else if self.audio {
            2
        } else if self.mouse {
            0
        } else if self.keyboard {
            1
        } else {
            4
        }
    }

    pub fn ids(&self) -> String {
        format!("{:04X}:{:04X}", self.vid, self.pid)
    }

    /// The line shown in the device list.
    pub fn label(&self) -> String {
        if self.bluetooth {
            format!("{}  (Bluetooth)", self.name)
        } else {
            format!("{}  ({})", self.name, self.ids())
        }
    }

    fn add_hid(&mut self, c: &HidColl) {
        self.hid.push(format!("if{} {:04X}:{:04X} {}/{}/{}", c.iface, c.page(), c.usage(), c.in_len(), c.out_len(), c.feat_len()));
        self.vendor |= c.page() >= 0xFF00;
        if c.page() == 0x01 {
            self.mouse |= c.usage() == 0x02;
            self.keyboard |= c.usage() == 0x06;
            self.gamepad |= c.usage() == 0x04 || c.usage() == 0x05;
        }
        self.audio |= c.page() == 0x0B; // telephony page: headset buttons
    }
}

/// Vendor / product id from a USB instance id or HID path ("VID_046D&PID_C547") or a Bluetooth HID
/// path ("_VID&0002046D_PID&B023", where the first four digits name the id's source).
pub fn parse_ids(id: &str) -> Option<(u16, u16)> {
    let u = id.to_ascii_uppercase();
    let hex = |s: &str, at: usize| u16::from_str_radix(s.get(at..at + 4)?, 16).ok();
    if let (Some(v), Some(p)) = (u.find("VID_"), u.find("PID_")) {
        return Some((hex(&u, v + 4)?, hex(&u, p + 4)?)).filter(|(v, _)| *v != 0);
    }
    if let (Some(v), Some(p)) = (u.find("VID&"), u.find("PID&")) {
        return Some((hex(&u, v + 8)?, hex(&u, p + 4)?)).filter(|(v, _)| *v != 0);
    }
    None
}

fn interface_of(path: &str) -> i32 {
    let u = path.to_ascii_uppercase();
    u.find("&MI_")
        .and_then(|i| i32::from_str_radix(u.get(i + 4..i + 6)?, 16).ok())
        .unwrap_or(-1)
}

fn node_name(n: &DevNode) -> String {
    // What the device itself reports (e.g. "Pulsar 8K Dongle"), else what Windows calls it.
    n.string(&DEVPKEY_Device_BusReportedDeviceDesc)
        .or_else(|| n.string(&DEVPKEY_Device_FriendlyName))
        .or_else(|| n.string(&DEVPKEY_Device_DeviceDesc))
        .unwrap_or_default()
}

/// Every device the user could mean: HID devices (also the ones another program holds), every USB
/// device (also audio-only or WinUSB dongles without a HID interface) and the Bluetooth devices
/// Windows knows. Devices with a vendor-defined interface or audio come first: that is where battery
/// protocols live.
pub fn scan() -> Vec<Candidate> {
    let mut by_id: HashMap<(u16, u16), Candidate> = HashMap::new();
    let colls = hid::enum_hid();
    for c in colls.iter().filter(|c| c.vid != 0) {
        let d = by_id.entry((c.vid, c.pid)).or_insert_with(|| Candidate { vid: c.vid, pid: c.pid, ..Default::default() });
        if d.name.is_empty() {
            d.name = clean_name(&hid::product_string(c));
        }
        d.add_hid(c);
    }
    // HID interfaces that could not be opened (held exclusively by another program).
    let opened: HashSet<String> = colls.iter().map(|c| c.path.to_ascii_uppercase()).collect();
    for path in hid::interface_paths() {
        if opened.contains(&path.to_ascii_uppercase()) {
            continue;
        }
        if let Some((vid, pid)) = parse_ids(&path) {
            let d = by_id.entry((vid, pid)).or_insert_with(|| Candidate { vid, pid, ..Default::default() });
            d.hid.push(format!("if{} (could not open)", interface_of(&path)));
        }
    }
    // USB devices and their interfaces.
    let mut hubs = HashSet::new();
    if let Some(set) = DevSet::present(Some("USB")) {
        for n in set.nodes() {
            let Some((vid, pid)) = parse_ids(&n.instance_id) else { continue };
            let class = n.string(&DEVPKEY_Device_Class).unwrap_or_default();
            let service = n.string(&DEVPKEY_Device_Service).unwrap_or_default();
            let label = format!(
                "{}{}",
                if class.is_empty() { "unknown" } else { &class },
                if service.is_empty() { String::new() } else { format!(" ({service})") }
            );
            let d = by_id.entry((vid, pid)).or_insert_with(|| Candidate { vid, pid, ..Default::default() });
            let is_interface = n.instance_id.to_ascii_uppercase().contains("&MI_");
            if !is_interface {
                if d.name.is_empty() {
                    d.name = clean_name(&node_name(&n));
                }
                if service.to_ascii_uppercase().contains("HUB") {
                    hubs.insert((vid, pid));
                }
                if class.eq_ignore_ascii_case("USB") {
                    continue; // composite parent: its interfaces follow
                }
            }
            d.audio |= class.eq_ignore_ascii_case("MEDIA");
            *d.usb.entry(label).or_default() += 1;
        }
    }
    let mut out: Vec<Candidate> = by_id.into_values().filter(|d| !hubs.contains(&(d.vid, d.pid))).collect();
    // Bluetooth devices Windows knows (paired), by name: headphones often have no HID interface.
    let known: HashSet<String> = out.iter().map(|d| d.name.to_ascii_lowercase()).collect();
    for e in ["BTHENUM", "BTHLE"] {
        let Some(set) = DevSet::present(Some(e)) else { continue };
        for n in set.nodes() {
            if !n.instance_id.to_ascii_uppercase().starts_with(&format!("{e}\\DEV_")) {
                continue;
            }
            let name = clean_name(&node_name(&n));
            if name.is_empty() || known.contains(&name.to_ascii_lowercase()) || out.iter().any(|d| d.bluetooth && d.name == name) {
                continue;
            }
            let mut d = Candidate { name, bluetooth: true, ..Default::default() };
            if let Some(cod) = n.u32(&CLASS_OF_DEVICE) {
                apply_class_of_device(&mut d, cod);
            } else {
                apply_kind(&mut d);
            }
            out.push(d);
        }
    }
    for d in &mut out {
        if d.name.is_empty() {
            d.name = "Unknown device".into();
        }
    }
    sort(&mut out);
    out
}

/// A device name without control characters; empty when nothing readable is left (some devices
/// return garbage as their HID product string, then the USB name is used).
pub fn clean_name(s: &str) -> String {
    let t: String = s.chars().filter(|c| !c.is_control()).collect();
    let t = t.trim();
    if t.chars().any(|c| c.is_alphanumeric()) {
        t.to_string()
    } else {
        String::new()
    }
}

fn apply_kind(d: &mut Candidate) {
    use crate::device::DeviceKind;
    match crate::util::guess_kind(&d.name) {
        DeviceKind::Mouse => d.mouse = true,
        DeviceKind::Keyboard => d.keyboard = true,
        DeviceKind::Headset => d.audio = true,
        DeviceKind::Gamepad => d.gamepad = true,
        _ => {}
    }
}

/// Bluetooth class of device: major class audio (4) or peripheral (5, with keyboard / pointing /
/// gamepad in the minor class).
pub fn apply_class_of_device(d: &mut Candidate, cod: u32) {
    let major = (cod >> 8) & 0x1F;
    let minor = (cod >> 2) & 0x3F;
    match major {
        4 => d.audio = true,
        5 => {
            d.keyboard |= minor & 0x10 != 0;
            d.mouse |= minor & 0x20 != 0;
            d.gamepad |= matches!(minor & 0x0F, 1 | 2);
        }
        _ => apply_kind(d),
    }
}

pub fn sort(list: &mut [Candidate]) {
    list.sort_by(|a, b| {
        let ia = a.vendor || a.audio;
        let ib = b.vendor || b.audio;
        ib.cmp(&ia).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// Windows build with update revision, e.g. "26300.9974".
pub fn windows_build() -> String {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ};
    let key = w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
    let mut build = [0u16; 32];
    let mut size = (build.len() * 2) as u32;
    let mut ubr = 0u32;
    let mut ubr_size = 4u32;
    unsafe {
        let _ = RegGetValueW(HKEY_LOCAL_MACHINE, key, w!("CurrentBuild"), RRF_RT_REG_SZ, None, Some(build.as_mut_ptr().cast()), Some(&mut size));
        let _ = RegGetValueW(HKEY_LOCAL_MACHINE, key, w!("UBR"), RRF_RT_REG_DWORD, None, Some((&mut ubr as *mut u32).cast()), Some(&mut ubr_size));
    }
    format!("{}.{}", crate::util::from_wide(&build), ubr)
}

/// What the user filled in.
#[derive(Clone, Debug, Default)]
pub struct Request {
    pub model: String,
    /// Optional; shown in the (public) issue.
    pub name: String,
    pub kind: String,
    pub notes: String,
}

/// Short technical summary of the picked device (or of none).
pub fn tech_summary(c: Option<&Candidate>) -> Vec<String> {
    let Some(c) = c else { return vec!["Device not in the app's list.".into()] };
    let mut out = Vec::new();
    if c.bluetooth {
        out.push("Bluetooth device, no vendor / product id from Windows.".into());
        return out;
    }
    out.push(format!("IDs: `{}`", c.ids()));
    if c.hid.is_empty() {
        out.push("HID: none".into());
    } else {
        let shown: Vec<String> = c.hid.iter().take(16).map(|h| format!("`{h}`")).collect();
        let more = if c.hid.len() > 16 { format!(" (+{} more)", c.hid.len() - 16) } else { String::new() };
        out.push(format!("HID (if page:usage in/out/feature): {}{more}", shown.join(", ")));
    }
    if !c.usb.is_empty() {
        let parts: Vec<String> = c.usb.iter().map(|(l, n)| if *n > 1 { format!("{l} x{n}") } else { l.clone() }).collect();
        out.push(format!("USB: {}", parts.join(", ")));
    }
    out
}

pub fn issue_title(r: &Request, c: Option<&Candidate>) -> String {
    match c.filter(|c| !c.bluetooth) {
        Some(c) => format!("Device request: {} ({})", r.model, c.ids()),
        None => format!("Device request: {}", r.model),
    }
}

/// Body of the GitHub issue (Markdown, \n line ends).
pub fn issue_body(r: &Request, c: Option<&Candidate>, windows_build: &str, app_version: &str) -> String {
    let mut s = format!("### Device\n{}\n\n### Type\n{}\n\n", r.model, r.kind);
    if !r.name.trim().is_empty() {
        s += &format!("### Requested by\n{}\n\n", r.name.trim());
    }
    if !r.notes.trim().is_empty() {
        s += &format!("### Notes\n{}\n\n", r.notes.trim());
    }
    s += "### Technical summary\n";
    for line in tech_summary(c) {
        s += &format!("- {line}\n");
    }
    s += &format!("- Windows build {windows_build}, BatteryHub {app_version}\n\n");
    s += "_Sent from the BatteryHub app (Request support for a device)._\n";
    s
}

/// Percent-encodes everything except the unreserved URL characters.
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out += &format!("%{b:02X}");
        }
    }
    out
}

/// Link to GitHub's "new issue" form, filled in.
pub fn issue_url(title: &str, body: &str) -> String {
    format!("https://github.com/{REPO}/issues/new?title={}&body={}", url_encode(title), url_encode(body))
}

/// Everything the scan found, for "Copy full report".
pub fn full_report(list: &[Candidate], picked: Option<&Candidate>, title: &str, body: &str) -> String {
    let mut s = format!("{title}\n\n{body}\n---\nAll devices BatteryHub sees:\n");
    for c in list {
        let mark = if picked == Some(c) { "> " } else { "  " };
        s += &format!("{mark}{}\n", c.label());
        for line in tech_summary(Some(c)).iter().skip(1) {
            s += &format!("      {}\n", line.replace('`', ""));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roccat() -> Candidate {
        let mut c = Candidate { vid: 0x1E7D, pid: 0x3A3F, name: "ROCCAT Syn Max Air".into(), ..Default::default() };
        c.hid = vec!["if3 FF00:0001 64/64/0".into(), "if4 000C:0001 3/0/0".into()];
        c.usb.insert("HIDClass (HidUsb)".into(), 2);
        c.usb.insert("MEDIA (usbaudio)".into(), 1);
        c.vendor = true;
        c.audio = true;
        c
    }

    #[test]
    fn ids_from_paths() {
        assert_eq!(parse_ids(r"USB\VID_046D&PID_C547&MI_02\7&1"), Some((0x046D, 0xC547)));
        assert_eq!(parse_ids(r"\\?\hid#{00001124-0000}_vid&0002046d_pid&b023#9"), Some((0x046D, 0xB023)));
        assert_eq!(parse_ids(r"USB\ROOT_HUB30\4&1"), None);
        assert_eq!(interface_of(r"\\?\hid#vid_1e7d&pid_3a3f&mi_03#8"), 3);
    }

    #[test]
    fn types() {
        assert_eq!(TYPES[roccat().type_index()], "headset");
        let mut d = Candidate::default();
        apply_class_of_device(&mut d, 0x002580); // peripheral, pointing device
        assert_eq!(TYPES[d.type_index()], "mouse");
        let mut d = Candidate::default();
        apply_class_of_device(&mut d, 0x240404); // audio, headset
        assert_eq!(TYPES[d.type_index()], "headset");
    }

    #[test]
    fn issue_text() {
        let c = roccat();
        let r = Request { model: "ROCCAT Syn Max Air".into(), kind: "headset".into(), name: String::new(), notes: "  works with Swarm ".into() };
        assert_eq!(issue_title(&r, Some(&c)), "Device request: ROCCAT Syn Max Air (1E7D:3A3F)");
        let body = issue_body(&r, Some(&c), "26300.9974", "v1.2");
        assert!(body.starts_with("### Device\nROCCAT Syn Max Air\n\n### Type\nheadset\n\n### Notes\nworks with Swarm\n\n"));
        assert!(body.contains("- IDs: `1E7D:3A3F`\n"));
        assert!(body.contains("`if3 FF00:0001 64/64/0`, `if4 000C:0001 3/0/0`"));
        assert!(body.contains("- USB: HIDClass (HidUsb) x2, MEDIA (usbaudio)\n"));
        assert!(body.contains("- Windows build 26300.9974, BatteryHub v1.2\n"));
        let none = issue_body(&Request { model: "X".into(), kind: "other".into(), name: "Kris".into(), notes: String::new() }, None, "1.0", "v1.2");
        assert!(!none.contains("### Notes"));
        assert!(none.contains("### Requested by\nKris\n\n"));
        assert!(none.contains("- Device not in the app's list.\n"));
    }

    #[test]
    fn url() {
        assert_eq!(url_encode("a b&c=ü/#"), "a%20b%26c%3D%C3%BC%2F%23");
        assert_eq!(
            issue_url("T 1", "x\ny"),
            "https://github.com/misiukasalgirdas-code/Wireless-Device-Battery-Monitor/issues/new?title=T%201&body=x%0Ay"
        );
    }

    #[test]
    fn names() {
        assert_eq!(clean_name("\u{2}"), "");
        assert_eq!(clean_name(" LCS USB Audio\0 "), "LCS USB Audio");
    }

    #[test]
    fn vendor_first() {
        let mut l = vec![
            Candidate { name: "b keyboard".into(), ..Default::default() },
            Candidate { name: "Z dongle".into(), vendor: true, ..Default::default() },
            Candidate { name: "a mouse".into(), ..Default::default() },
        ];
        sort(&mut l);
        let names: Vec<&str> = l.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Z dongle", "a mouse", "b keyboard"]);
    }
}
