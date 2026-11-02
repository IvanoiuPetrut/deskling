//! Battery levels: this PC's own battery (laptops), and devices that report one to Windows,
//! like Bluetooth mice, keyboards and headsets: the same numbers Windows shows in
//! Settings → Bluetooth & devices.

/// The name used for this PC's own battery.
pub const THIS_PC: &str = "This PC";

#[derive(Clone, PartialEq)]
pub struct Battery {
    /// The device's name, e.g. "G309"; "This PC" for the laptop's own battery. Also the key the
    /// widget options use for hiding and renaming.
    pub name: String,
    /// 0–100.
    pub level: u8,
    pub charging: bool,
    /// False for a paired device that's switched off or out of range: `level` is then the last
    /// one Windows heard, possibly days old.
    pub connected: bool,
}

/// This PC's battery first (if it has one), then the devices by name.
pub fn read() -> Vec<Battery> {
    let mut batteries: Vec<Battery> = Vec::new();
    for device in win32::devices() {
        // A device paired twice (e.g. a mouse with two Bluetooth channels) shows up twice under
        // the same name; keep the connected one.
        match batteries.iter_mut().find(|known| known.name == device.name) {
            Some(known) if device.connected && !known.connected => *known = device,
            Some(_) => {}
            None => batteries.push(device),
        }
    }
    batteries.sort_by_key(|battery| battery.name.to_lowercase());
    if let Some(pc) = win32::this_pc() {
        batteries.insert(0, pc);
    }
    batteries
}

/// Headsets show up as their hands-free (phone call) part, e.g. "Moondrop Hands-Free AG".
fn clean_name(name: &str) -> String {
    let name = name.trim();
    [" Hands-Free AG", " Hands-Free"]
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(name)
        .to_string()
}

#[cfg(windows)]
mod win32 {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        DIGCF_ALLCLASSES, DIGCF_PRESENT, HDEVINFO, SP_DEVINFO_DATA, SetupDiDestroyDeviceInfoList,
        SetupDiEnumDeviceInfo, SetupDiGetClassDevsW, SetupDiGetDevicePropertyW,
    };
    use windows::Win32::Devices::Properties::{DEVPKEY_Device_FriendlyName, DEVPROPTYPE};
    use windows::Win32::Foundation::DEVPROPKEY;
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    use windows::core::{GUID, PCWSTR};

    use super::{Battery, THIS_PC, clean_name};

    /// The battery level Windows shows in Settings (one byte, 0–100).
    const BATTERY_LEVEL: DEVPROPKEY =
        DEVPROPKEY { fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5), pid: 2 };
    /// Whether a Bluetooth device is connected right now (a boolean). It isn't in the SDK
    /// headers; found by comparing a connected and a switched-off device's properties.
    const CONNECTED: DEVPROPKEY =
        DEVPROPKEY { fmtid: GUID::from_u128(0x83da6326_97a6_4088_9453_a1923f573b29), pid: 15 };

    pub fn this_pc() -> Option<Battery> {
        let mut status = SYSTEM_POWER_STATUS::default();
        unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
        // Flag 128 = no battery (a desktop PC), 255 = unknown; the % is 255 when unknown.
        if status.BatteryFlag & 128 != 0 || status.BatteryFlag == 255 || status.BatteryLifePercent > 100 {
            return None;
        }
        Some(Battery {
            name: THIS_PC.into(),
            level: status.BatteryLifePercent,
            charging: status.BatteryFlag & 8 != 0,
            connected: true,
        })
    }

    /// Every device Windows knows a battery level for.
    pub fn devices() -> Vec<Battery> {
        let mut found = Vec::new();
        unsafe {
            let Ok(set) = SetupDiGetClassDevsW(None, PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_ALLCLASSES) else {
                return found;
            };
            let mut info = SP_DEVINFO_DATA { cbSize: size_of::<SP_DEVINFO_DATA>() as u32, ..Default::default() };
            let mut index = 0;
            while SetupDiEnumDeviceInfo(set, index, &mut info).is_ok() {
                index += 1;
                let Some(&[level]) = property(set, &info, &BATTERY_LEVEL).as_deref() else { continue };
                let Some(name) = property(set, &info, &DEVPKEY_Device_FriendlyName).map(|bytes| utf16(&bytes))
                else {
                    continue;
                };
                found.push(Battery {
                    name: clean_name(&name),
                    level: level.min(100),
                    charging: false, // Windows doesn't say for devices
                    // Booleans are 0 or 0xFF. Devices without the property are taken as connected.
                    connected: property(set, &info, &CONNECTED).is_none_or(|bytes| bytes.first() != Some(&0)),
                });
            }
            let _ = SetupDiDestroyDeviceInfoList(set);
        }
        found
    }

    /// One property of one device as raw bytes, or `None` if the device doesn't have it.
    unsafe fn property(set: HDEVINFO, info: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<Vec<u8>> {
        let mut kind = DEVPROPTYPE::default();
        let mut size = 0u32;
        // The first call fails on purpose and says how big the value is.
        let _ = unsafe { SetupDiGetDevicePropertyW(set, info, key, &mut kind, None, Some(&mut size), 0) };
        if size == 0 {
            return None;
        }
        let mut bytes = vec![0u8; size as usize];
        unsafe { SetupDiGetDevicePropertyW(set, info, key, &mut kind, Some(&mut bytes), None, 0) }.ok()?;
        Some(bytes)
    }

    /// A UTF-16 string ending in a 0, as Windows stores names.
    fn utf16(bytes: &[u8]) -> String {
        let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
        String::from_utf16_lossy(&units).trim_end_matches('\0').to_string()
    }
}

#[cfg(not(windows))]
mod win32 {
    use super::Battery;

    pub fn this_pc() -> Option<Battery> {
        None
    }

    pub fn devices() -> Vec<Battery> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::clean_name;

    #[test]
    fn headset_names_lose_the_hands_free_part() {
        assert_eq!(clean_name("Moondrop Space Travel Hands-Free AG"), "Moondrop Space Travel");
        assert_eq!(clean_name("Razer Barracuda X (BT) Hands-Free"), "Razer Barracuda X (BT)");
        assert_eq!(clean_name("G309"), "G309");
    }
}
