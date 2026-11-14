//! Resident memory of this backend process, including shared pages and every loaded project.
//! The peak comes from the OS, not from the UI's polling samples or the JavaScript heap.
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub pid: u32,
    pub current_bytes: Option<u64>,
    pub peak_bytes: Option<u64>,
    pub metric: &'static str,
    pub peak_source: Option<&'static str>,
    pub error: Option<String>,
}

fn measured(metric: &'static str, sample: Result<(u64, u64), String>) -> Snapshot {
    let (current_bytes, peak_bytes, error) = match sample {
        Ok((current, peak)) if peak >= current => (Some(current), Some(peak), None),
        Ok((current, _)) => (Some(current), None, Some("The OS reported a peak below current memory; peak is unavailable for this sample.".into())),
        Err(error) => (None, None, Some(error)),
    };
    Snapshot { pid: std::process::id(), current_bytes, peak_bytes, metric, peak_source: peak_bytes.map(|_| "os"), error }
}

pub fn snapshot() -> Snapshot {
    #[cfg(target_os = "windows")]
    { measured("working-set", platform::read()) }
    #[cfg(target_os = "linux")]
    { measured("rss", std::fs::read_to_string("/proc/self/status").map_err(|e| format!("Process RSS is unavailable: {e}")).and_then(|s| parse_status(&s))) }
    #[cfg(target_os = "macos")]
    { measured("rss", platform::read()) }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    { measured("unavailable", Err("Process resident memory is unavailable on this operating system.".into())) }
}

#[cfg(any(target_os = "linux", test))]
fn parse_status(status: &str) -> Result<(u64, u64), String> {
    let field = |name: &str| -> Result<u64, String> {
        let line = status.lines().find_map(|line| line.strip_prefix(name)).ok_or_else(|| format!("Process memory field {name} is unavailable"))?;
        let mut parts = line.split_whitespace();
        let value = parts.next().and_then(|n| n.parse::<u64>().ok()).ok_or("Invalid process memory value")?;
        if parts.next() != Some("kB") || parts.next().is_some() { return Err("Invalid process memory unit".into()); }
        value.checked_mul(1024).ok_or_else(|| "Process memory value overflow".into())
    };
    Ok((field("VmRSS:")?, field("VmHWM:")?))
}

#[cfg(target_os = "windows")]
mod platform {
    use std::{ffi::c_void, mem::size_of};
    // https://learn.microsoft.com/windows/win32/api/psapi/ns-psapi-process_memory_counters
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32, faults: u32, peak_working_set: usize, working_set: usize,
        peak_paged_pool: usize, paged_pool: usize, peak_nonpaged_pool: usize,
        nonpaged_pool: usize, pagefile: usize, peak_pagefile: usize,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn K32GetProcessMemoryInfo(process: *mut c_void, counters: *mut Counters, size: u32) -> i32;
    }
    pub fn read() -> Result<(u64, u64), String> {
        let mut counters = Counters { cb: size_of::<Counters>() as u32, ..Counters::default() };
        // The pseudo handle is this process; the C-layout output has the documented size.
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size_of::<Counters>() as u32) };
        if ok == 0 { return Err(format!("Process working set is unavailable: {}", std::io::Error::last_os_error())); }
        Ok((counters.working_set as u64, counters.peak_working_set as u64))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::mem::size_of;
    // Apple's exported mach/task_info.h uses #pragma pack(push, 4), MACH_TASK_BASIC_INFO=20.
    // https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/mach/task_info.h
    #[repr(C, packed(4))]
    #[derive(Default)]
    struct BasicInfo {
        virtual_size: u64, resident_size: u64, resident_size_max: u64,
        user_time: [i32; 2], system_time: [i32; 2], policy: i32, suspend_count: i32,
    }
    #[link(name = "System")]
    extern "C" {
        static mach_task_self_: u32;
        fn task_info(task: u32, flavor: u32, info: *mut i32, count: *mut u32) -> i32;
    }
    pub fn read() -> Result<(u64, u64), String> {
        let mut info = BasicInfo::default();
        let expected = (size_of::<BasicInfo>() / size_of::<i32>()) as u32;
        let mut count = expected;
        // The task port belongs to this process; count is in 32-bit natural_t words, not bytes.
        let result = unsafe { task_info(mach_task_self_, 20, (&mut info as *mut BasicInfo).cast(), &mut count) };
        if result != 0 || count < expected { return Err(format!("Process RSS is unavailable (Mach status {result})")); }
        Ok((info.resident_size, info.resident_size_max))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_status_uses_resident_bytes_and_checks_units_and_overflow() {
        assert_eq!(parse_status("VmPeak: 999999 kB\nVmSize: 888888 kB\nVmHWM: 2048 kB\nVmRSS: 1024 kB\n").unwrap(), (1024*1024, 2048*1024));
        assert!(parse_status("VmHWM: 2 kB\nVmRSS: 1 MB").is_err());
        assert!(parse_status("VmRSS: 1 kB").is_err());
        assert!(parse_status("VmHWM: 18446744073709551615 kB\nVmRSS: 1 kB").is_err());
    }

    #[test]
    fn unavailable_or_inconsistent_peak_is_never_invented() {
        let failed = serde_json::to_value(measured("rss", Err("unavailable".into()))).unwrap();
        assert!(failed["currentBytes"].is_null()); assert!(failed["peakBytes"].is_null()); assert!(failed["peakSource"].is_null());
        let inconsistent = measured("rss", Ok((20, 10)));
        assert_eq!(inconsistent.current_bytes, Some(20)); assert_eq!(inconsistent.peak_bytes, None); assert!(inconsistent.error.is_some());
    }

    #[test]
    fn live_process_sample_has_os_peak_and_does_not_create_jobs() {
        let owner = format!("memory-test-{}", std::process::id());
        let reply = crate::jobs::with_owner(Some(&owner), || crate::dispatch_free("memory", &serde_json::json!({})).unwrap().unwrap());
        assert_eq!(reply["data"]["pid"], std::process::id());
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        {
            let data = &reply["data"];
            let current = data["currentBytes"].as_u64().expect("OS current resident memory");
            let peak = data["peakBytes"].as_u64().expect("OS peak resident memory");
            assert!(current > 0); assert!(peak >= current); assert_eq!(data["peakSource"], "os"); assert!(data["error"].is_null());
        }
        assert_eq!(crate::jobs::list_owned(&owner), serde_json::json!([]));
    }
}
