//! Keeping the app's memory down on Windows: memory the program freed (covers, songs that were
//! playing while they downloaded) goes back to the system, and the pages it isn't using right
//! now leave its working set (they come back on their own if needed). Called every few songs and
//! whenever the app goes over a limit.

/// Over this (MB of working set), memory is given back at once.
pub const LIMIT_MB: u64 = 250;

#[cfg(windows)]
mod sys {
    #[repr(C)]
    #[derive(Default)]
    pub struct Counters {
        pub cb: u32,
        pub page_fault_count: u32,
        pub peak_working_set_size: usize,
        pub working_set_size: usize,
        pub quota_peak_paged_pool_usage: usize,
        pub quota_paged_pool_usage: usize,
        pub quota_peak_non_paged_pool_usage: usize,
        pub quota_non_paged_pool_usage: usize,
        pub pagefile_usage: usize,
        pub peak_pagefile_usage: usize,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn GetCurrentProcess() -> isize;
        pub fn GetProcessHeap() -> isize;
        pub fn HeapCompact(heap: isize, flags: u32) -> usize;
        pub fn SetProcessWorkingSetSize(process: isize, min: usize, max: usize) -> i32;
        pub fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }
}

/// Gives freed memory back to the system and trims the working set.
pub fn trim() {
    #[cfg(windows)]
    // SAFETY: plain Win32 calls on this process's own handle and heap.
    unsafe {
        sys::HeapCompact(sys::GetProcessHeap(), 0);
        sys::SetProcessWorkingSetSize(sys::GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

/// The app's working set in MB (what the Task Manager shows), when known.
pub fn working_set_mb() -> Option<u64> {
    #[cfg(windows)]
    {
        let mut c = sys::Counters { cb: std::mem::size_of::<sys::Counters>() as u32, ..Default::default() };
        // SAFETY: `c` is a valid, correctly sized PROCESS_MEMORY_COUNTERS for the call to fill.
        let ok = unsafe { sys::K32GetProcessMemoryInfo(sys::GetCurrentProcess(), &mut c, c.cb) };
        return (ok != 0).then_some(c.working_set_size as u64 / (1 << 20));
    }
    #[allow(unreachable_code)]
    None
}

/// Trims when the app is over [`LIMIT_MB`]. Returns the size before, when it trimmed.
pub fn trim_if_over_limit() -> Option<u64> {
    let mb = working_set_mb()?;
    (mb > LIMIT_MB).then(|| {
        trim();
        mb
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_and_trims() {
        let before = super::working_set_mb();
        super::trim();
        if cfg!(windows) {
            assert!(before.is_some_and(|mb| mb > 0));
            assert!(super::working_set_mb().is_some());
        }
    }
}
