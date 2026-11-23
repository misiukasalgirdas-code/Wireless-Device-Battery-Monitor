//! BatteryHub (Rust): a recode of github.com/krisgfx9/Wireless-Device-Battery-Monitor (C++, GPL-3.0), rewritten in Rust.
//! Shared core used by both editions: device model, Win32 HID layer and the drivers.
#![cfg(windows)]

pub mod ble;
pub mod device;
pub mod drivers;
pub mod hid;
pub mod i18n;
pub mod install;
pub mod listen;
pub mod report;
pub mod report_window;
pub mod settings;
pub mod ui;
pub mod update;
pub mod util;

pub use device::{poll_all_devices, DeviceKind, DeviceStatus};
