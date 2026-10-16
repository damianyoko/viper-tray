// HID battery query for Razer Viper V3 Pro.
// Opens the mouse interface with desired_access=0 to bypass mouclass exclusive lock,
// sends Razer feature report TID=0x1F cmd 0x07/0x80, reads response,
// battery byte at offset 10 scaled 0..255 -> 0..100.

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::thread::sleep;
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Devices::DeviceAndDriverInstallation::*;
use windows::Win32::Devices::HumanInterfaceDevice::*;
use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;

const VID_RAZER: u16 = 0x1532;
const PID_VIPER_V3_PRO_WIRED: u16 = 0x00C0;
const PID_VIPER_V3_PRO_WIRELESS: u16 = 0x00C1;
const RAZER_TID: u8 = 0x1F;
const REPORT_LEN: usize = 91;

pub fn read_battery_percent() -> Option<u8> {
    let paths = enumerate_hid_paths().ok()?;
    for path in paths {
        if !is_target_razer(&path) {
            continue;
        }
        if let Some(pct) = try_query(&path) {
            return Some(pct);
        }
    }
    None
}

fn is_target_razer(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains(&format!("vid_{:04x}", VID_RAZER))
        && (lower.contains(&format!("pid_{:04x}", PID_VIPER_V3_PRO_WIRED))
            || lower.contains(&format!("pid_{:04x}", PID_VIPER_V3_PRO_WIRELESS)))
}

fn enumerate_hid_paths() -> Result<Vec<String>, String> {
    unsafe {
        let hid_guid = HidD_GetHidGuid();

        let dev_info = SetupDiGetClassDevsW(
            Some(&hid_guid),
            PCWSTR::null(),
            None,
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
        .map_err(|e| format!("SetupDiGetClassDevs: {e}"))?;

        let mut paths = Vec::new();
        let mut idx = 0u32;
        loop {
            let mut iface = SP_DEVICE_INTERFACE_DATA::default();
            iface.cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
            if SetupDiEnumDeviceInterfaces(dev_info, None, &hid_guid, idx, &mut iface).is_err() {
                break;
            }
            idx += 1;

            // Get required size
            let mut required: u32 = 0;
            let _ = SetupDiGetDeviceInterfaceDetailW(
                dev_info,
                &iface,
                None,
                0,
                Some(&mut required),
                None,
            );
            if required == 0 {
                continue;
            }

            // Allocate buffer for SP_DEVICE_INTERFACE_DETAIL_DATA_W (cbSize + path)
            let mut buf = vec![0u8; required as usize];
            let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;

            if SetupDiGetDeviceInterfaceDetailW(
                dev_info,
                &iface,
                Some(detail),
                required,
                None,
                None,
            )
            .is_err()
            {
                continue;
            }

            // Path follows cbSize field — wide-char null-terminated
            let path_ptr = (*detail).DevicePath.as_ptr();
            let path = wide_to_string(path_ptr);
            paths.push(path);
        }
        let _ = SetupDiDestroyDeviceInfoList(dev_info);
        Ok(paths)
    }
}

unsafe fn wide_to_string(p: *const u16) -> String {
    let mut len = 0usize;
    while *p.add(len) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(p, len);
    OsString::from_wide(slice).to_string_lossy().into_owned()
}

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn try_query(path: &str) -> Option<u8> {
    unsafe {
        let wide = to_wide(path);
        // desired_access = 0 to bypass mouclass exclusive lock
        let handle = CreateFileW(
            PCWSTR(wide.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
        .ok()?;

        if handle.is_invalid() {
            return None;
        }

        // Get HID caps to filter for the mouse interface (UP=1, Usage=2, FeatLen>=91)
        let mut preparsed: PHIDP_PREPARSED_DATA = PHIDP_PREPARSED_DATA::default();
        if !HidD_GetPreparsedData(handle, &mut preparsed).as_bool() {
            let _ = CloseHandle(handle);
            return None;
        }
        let mut caps: HIDP_CAPS = std::mem::zeroed();
        let cap_status = HidP_GetCaps(preparsed, &mut caps);
        let _ = HidD_FreePreparsedData(preparsed);
        if cap_status != HIDP_STATUS_SUCCESS {
            let _ = CloseHandle(handle);
            return None;
        }
        if caps.UsagePage != 0x01 || caps.Usage != 0x02 {
            let _ = CloseHandle(handle);
            return None;
        }
        if (caps.FeatureReportByteLength as usize) < REPORT_LEN {
            let _ = CloseHandle(handle);
            return None;
        }

        // Build Razer feature report
        let mut req = [0u8; REPORT_LEN];
        req[0] = 0x00; // Report ID
        req[2] = RAZER_TID; // transaction_id
        req[6] = 0x02; // data_size
        req[7] = 0x07; // command_class (power)
        req[8] = 0x80; // command_id (get battery level)

        // CRC: XOR of bytes 3..=88
        let mut crc: u8 = 0;
        for i in 3..=88 {
            crc ^= req[i];
        }
        req[89] = crc;

        // Retry loop: each attempt is a full SetFeature -> sleep -> GetFeature cycle,
        // with escalating sleep so the dongle has time to finish its 2.4GHz round-trip
        // to the mouse. Only status 0x02 is accepted as success; 0x01 means busy.
        // A successful report with raw battery 0 usually means the mouse is asleep
        // or not ready yet, so retry it instead of immediately surfacing offline.
        let sleeps_ms = [80u64, 150, 250, 500];
        let mut result: Option<u8> = None;
        for &sleep_ms in &sleeps_ms {
            let ok = HidD_SetFeature(handle, req.as_ptr() as *mut _, REPORT_LEN as u32);
            if !ok.as_bool() {
                sleep(Duration::from_millis(sleep_ms));
                continue;
            }
            sleep(Duration::from_millis(sleep_ms));

            let mut resp = [0u8; REPORT_LEN];
            let ok = HidD_GetFeature(handle, resp.as_mut_ptr() as *mut _, REPORT_LEN as u32);
            if !ok.as_bool() {
                continue;
            }

            if resp[1] == 0x02 {
                let raw = resp[10];
                if raw == 0 {
                    // Mouse asleep / not-yet-ready. The firmware will beep and the
                    // scroll wheel turns red long before it actually hits 0%.
                    continue;
                }
                let pct = ((raw as u32 * 100) / 255) as u8;
                result = Some(pct.min(100));
                break;
            }
            // 0x01 = busy: retry with a longer sleep. Anything else: bail.
            if resp[1] != 0x01 {
                break;
            }
        }
        let _ = CloseHandle(handle);
        result
    }
}
