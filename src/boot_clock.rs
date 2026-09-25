//! Elapsed time within one boot session, including time while Whisple is closed.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub boot_id: String,
    pub seconds: u64,
}

#[cfg(target_os = "macos")]
pub fn snapshot() -> Option<Snapshot> {
    let output = std::process::Command::new("/usr/sbin/sysctl")
        .args(["-n", "kern.bootsessionuuid"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let boot_id = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if boot_id.is_empty() {
        return None;
    }

    #[repr(C)]
    struct TimebaseInfo {
        numer: u32,
        denom: u32,
    }
    extern "C" {
        fn mach_continuous_time() -> u64;
        fn mach_timebase_info(info: *mut TimebaseInfo) -> i32;
    }
    let mut timebase = TimebaseInfo { numer: 0, denom: 0 };
    // These macOS APIs return ticks since boot and the factor for converting
    // them to nanoseconds. Continuous time also advances while the Mac sleeps.
    if unsafe { mach_timebase_info(&mut timebase) } != 0 || timebase.denom == 0 {
        return None;
    }
    let ticks = unsafe { mach_continuous_time() };
    let seconds = (u128::from(ticks) * u128::from(timebase.numer)
        / u128::from(timebase.denom)
        / 1_000_000_000) as u64;
    Some(Snapshot { boot_id, seconds })
}

#[cfg(target_os = "linux")]
pub fn snapshot() -> Option<Snapshot> {
    let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()?
        .trim()
        .to_string();
    let seconds = std::fs::read_to_string("/proc/uptime")
        .ok()?
        .split_whitespace()
        .next()?
        .split('.')
        .next()?
        .parse()
        .ok()?;
    (!boot_id.is_empty()).then_some(Snapshot { boot_id, seconds })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn snapshot() -> Option<Snapshot> {
    None
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;

    #[test]
    fn boot_clock_advances_within_the_same_boot() {
        let first = snapshot().expect("boot clock is available");
        let second = snapshot().expect("boot clock is available");
        assert_eq!(first.boot_id, second.boot_id);
        assert!(second.seconds >= first.seconds);
    }
}
