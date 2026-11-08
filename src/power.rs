//! The computer's power and the app's memory: the window apps stop their animations on battery
//! saver or a low battery (under 20% and not charging), and the performance badge shows memory.

/// Battery saver is on, or the battery is under 20% and not charging. `false` on a desktop.
pub fn saving() -> bool {
    imp::saving()
}

/// The app's memory in use (megabytes); 0 when it can't be read.
pub fn ram_mb() -> f32 {
    imp::ram_mb()
}

#[cfg(windows)]
mod imp {
    #[repr(C)]
    #[derive(Default)]
    struct SystemPowerStatus {
        ac_line_status: u8,
        battery_flag: u8,
        battery_life_percent: u8,
        system_status_flag: u8,
        battery_life_time: u32,
        battery_full_life_time: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    // Other crates declare these with their own handle types; the calls are the same.
    #[allow(clashing_extern_declarations)]
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetSystemPowerStatus(status: *mut SystemPowerStatus) -> i32;
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn K32GetProcessMemoryInfo(process: *mut std::ffi::c_void, counters: *mut ProcessMemoryCounters, cb: u32) -> i32;
    }

    pub fn saving() -> bool {
        let mut s = SystemPowerStatus::default();
        // SAFETY: a plain struct the call fills.
        if unsafe { GetSystemPowerStatus(&mut s) } == 0 {
            return false;
        }
        // 1: battery saver on. 0 on the line: on battery; 255: unknown percent.
        s.system_status_flag == 1 || (s.ac_line_status == 0 && s.battery_life_percent < 20)
    }

    pub fn ram_mb() -> f32 {
        let mut c = ProcessMemoryCounters { cb: std::mem::size_of::<ProcessMemoryCounters>() as u32, ..Default::default() };
        // SAFETY: the pseudo-handle of this process, and a struct of the size given.
        if unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) } == 0 {
            return 0.0;
        }
        c.working_set_size as f32 / (1024.0 * 1024.0)
    }
}

#[cfg(not(windows))]
mod imp {
    use std::fs;

    pub fn saving() -> bool {
        // The power profile "low-power" (power-profiles-daemon / ACPI), or a low battery.
        if fs::read_to_string("/sys/firmware/acpi/platform_profile").is_ok_and(|p| p.trim() == "low-power") {
            return true;
        }
        let Ok(dir) = fs::read_dir("/sys/class/power_supply") else { return false };
        for e in dir.flatten() {
            let p = e.path();
            if fs::read_to_string(p.join("type")).is_ok_and(|t| t.trim() == "Battery") {
                let level = fs::read_to_string(p.join("capacity")).ok().and_then(|c| c.trim().parse::<u32>().ok()).unwrap_or(100);
                let discharging = fs::read_to_string(p.join("status")).is_ok_and(|s| s.trim() == "Discharging");
                if discharging && level < 20 {
                    return true;
                }
            }
        }
        false
    }

    pub fn ram_mb() -> f32 {
        // statm's second field: pages in memory.
        fs::read_to_string("/proc/self/statm")
            .ok()
            .and_then(|s| s.split_whitespace().nth(1).and_then(|n| n.parse::<f32>().ok()))
            .map_or(0.0, |pages| pages * 4096.0 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn memory_is_read() {
        assert!(super::ram_mb() > 1.0);
        // Whatever the battery, it answers.
        let _ = super::saving();
    }
}
