pub(crate) mod dictation;
pub(crate) mod placement;

use std::path::Path;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Registry::{
    RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    RRF_SUBKEY_WOW6464KEY,
};
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};

/// Whether the taskbar uses the light system theme. Windows defaults to a
/// dark taskbar, which is also assumed when the setting cannot be read.
pub(crate) fn light_taskbar() -> bool {
    registry_dword(
        HKEY_CURRENT_USER,
        w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
        w!("SystemUsesLightTheme"),
    )
    .is_some_and(|value| value != 0)
}

/// Whether a privacy setting keeps desktop apps from the microphone: the
/// device-wide switch, the user's switch, or the one for desktop apps.
/// Settings that are missing have never been turned off.
pub(crate) fn microphone_blocked() -> bool {
    const STORE: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
    let denied = |root: HKEY, key: &str| {
        let key = HSTRING::from(key);
        registry_string(root, PCWSTR(key.as_ptr()), w!("Value"))
            .is_some_and(|value| value.eq_ignore_ascii_case("Deny"))
    };
    denied(HKEY_LOCAL_MACHINE, STORE)
        || denied(HKEY_CURRENT_USER, STORE)
        || denied(HKEY_CURRENT_USER, &format!(r"{STORE}\NonPackaged"))
}

/// The ID Windows gives this installation, which survives new user accounts.
#[cfg_attr(not(feature = "licensing"), allow(dead_code))]
pub(crate) fn machine_guid() -> Option<String> {
    registry_string(
        HKEY_LOCAL_MACHINE,
        w!(r"SOFTWARE\Microsoft\Cryptography"),
        w!("MachineGuid"),
    )
    .map(|id| id.trim().to_string())
    .filter(|id| !id.is_empty())
}

/// A number Windows raises on every boot, and the seconds since that boot,
/// counting time asleep.
#[cfg_attr(not(feature = "licensing"), allow(dead_code))]
pub(crate) fn boot_clock() -> Option<(u32, u64)> {
    let boot = registry_dword(
        HKEY_LOCAL_MACHINE,
        w!(
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters"
        ),
        w!("BootId"),
    )?;
    let millis = unsafe { windows::Win32::System::SystemInformation::GetTickCount64() };
    Some((boot, millis / 1000))
}

fn registry_dword(root: HKEY, key: PCWSTR, value: PCWSTR) -> Option<u32> {
    let mut data = 0u32;
    let mut size = size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            root,
            key,
            value,
            RRF_RT_REG_DWORD | RRF_SUBKEY_WOW6464KEY,
            None,
            Some((&mut data as *mut u32).cast()),
            Some(&mut size),
        )
    };
    status.is_ok().then_some(data)
}

fn registry_string(root: HKEY, key: PCWSTR, value: PCWSTR) -> Option<String> {
    let flags = RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY;
    let mut size = 0u32;
    unsafe { RegGetValueW(root, key, value, flags, None, None, Some(&mut size)) }
        .ok()
        .ok()?;
    let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
    unsafe {
        RegGetValueW(
            root,
            key,
            value,
            flags,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    }
    .ok()
    .ok()?;
    let len = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..len]))
}

/// A string from an executable's version resource, such as its
/// `FileDescription`, in the first language the resource lists.
pub(crate) fn version_string(path: &Path, key: &str) -> Option<String> {
    let path = HSTRING::from(path);
    let size = unsafe { GetFileVersionInfoSizeW(&path, None) };
    if size == 0 {
        return None;
    }
    let mut data = vec![0u8; size as usize];
    unsafe { GetFileVersionInfoW(&path, None, size, data.as_mut_ptr().cast()) }.ok()?;
    let translation = version_value(&data, w!(r"\VarFileInfo\Translation"), false)?;
    let [language_lo, language_hi, code_page_lo, code_page_hi, ..] = translation[..] else {
        return None;
    };
    let language = u16::from_le_bytes([language_lo, language_hi]);
    let code_page = u16::from_le_bytes([code_page_lo, code_page_hi]);
    let query = HSTRING::from(format!(
        r"\StringFileInfo\{language:04x}{code_page:04x}\{key}"
    ));
    let value = version_value(&data, PCWSTR(query.as_ptr()), true)?;
    let units: Vec<u16> = value
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .take_while(|&unit| unit != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}

/// A value from a version resource. Strings report their length in UTF-16
/// units and binary values in bytes.
fn version_value(data: &[u8], key: PCWSTR, text: bool) -> Option<Vec<u8>> {
    let mut value = std::ptr::null_mut();
    let mut len = 0u32;
    let found = unsafe { VerQueryValueW(data.as_ptr().cast(), key, &mut value, &mut len) };
    if !found.as_bool() || value.is_null() || len == 0 {
        return None;
    }
    let start = (value as usize).checked_sub(data.as_ptr() as usize)?;
    let bytes = if text { len as usize * 2 } else { len as usize };
    let end = (start + bytes).min(data.len());
    data.get(start..end).map(<[u8]>::to_vec)
}

/// Sets up COM on the calling thread. GPUI's main thread already has it, and
/// a background thread joins the multithreaded apartment, which UI
/// Automation and the shell prefer for callers without a message loop.
pub(crate) fn com_ready() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

pub(crate) fn automation() -> Option<IUIAutomation> {
    com_ready();
    unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok() }
}
