pub(crate) mod dictation;
pub(crate) mod placement;

use windows::core::{w, HSTRING, PCWSTR};
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
