//! OS facts std does not expose: hostname, memory, boot time, user ids.
//!
//! This is the only unsafe code in the tree, and all of it is FFI to a handful
//! of libc calls behind cfg(unix). Every one of them has a fallback: an empty
//! string or a zero beats a panic inside a runtime whose job is to keep
//! serving. Windows paths read the environment instead and report zeros for
//! what it cannot answer.

use std::time::{SystemTime, UNIX_EPOCH};

/// Number of cores the scheduler is willing to give us.
pub fn cpu_count() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
}

#[cfg(unix)]
fn field(raw: &[libc::c_char]) -> String {
    unsafe { std::ffi::CStr::from_ptr(raw.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(unix)]
fn uname() -> Option<libc::utsname> {
    let mut name = std::mem::MaybeUninit::<libc::utsname>::zeroed();
    let ok = unsafe { libc::uname(name.as_mut_ptr()) } == 0;
    if ok { Some(unsafe { name.assume_init() }) } else { None }
}

#[cfg(unix)]
pub fn system_name() -> String {
    uname().map(|name| field(&name.sysname)).unwrap_or_default()
}

#[cfg(unix)]
pub fn release() -> String {
    uname().map(|name| field(&name.release)).unwrap_or_default()
}

#[cfg(unix)]
pub fn machine() -> String {
    uname().map(|name| field(&name.machine)).unwrap_or_default()
}

#[cfg(unix)]
pub fn hostname() -> String {
    let mut buffer = [0 as libc::c_char; 256];
    let ok = unsafe { libc::gethostname(buffer.as_mut_ptr(), buffer.len()) } == 0;
    if !ok {
        return String::new();
    }
    let bytes: Vec<u8> = buffer
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(unix)]
pub fn uid() -> u32 {
    unsafe { libc::getuid() }
}

#[cfg(unix)]
pub fn gid() -> u32 {
    unsafe { libc::getgid() }
}

#[cfg(not(unix))]
pub fn system_name() -> String {
    "Windows_NT".to_string()
}

#[cfg(not(unix))]
pub fn release() -> String {
    String::new()
}

#[cfg(not(unix))]
pub fn machine() -> String {
    std::env::consts::ARCH.to_string()
}

#[cfg(not(unix))]
pub fn hostname() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_default()
}

#[cfg(not(unix))]
pub fn uid() -> u32 {
    0
}

#[cfg(not(unix))]
pub fn gid() -> u32 {
    0
}

#[cfg(target_os = "macos")]
fn sysctl_raw(name: &str, out: *mut libc::c_void, size: &mut usize) -> bool {
    let Ok(key) = std::ffi::CString::new(name) else {
        return false;
    };
    unsafe {
        libc::sysctlbyname(
            key.as_ptr(),
            out,
            size,
            std::ptr::null_mut(),
            0,
        ) == 0
    }
}

#[cfg(target_os = "macos")]
fn sysctl_u64(name: &str) -> Option<u64> {
    let mut value: u64 = 0;
    let mut size = std::mem::size_of::<u64>();
    let out = (&mut value) as *mut u64 as *mut libc::c_void;
    if sysctl_raw(name, out, &mut size) {
        Some(value)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
pub fn total_memory() -> u64 {
    sysctl_u64("hw.memsize").unwrap_or(0)
}

/// Free pages times the page size. Not the same thing as "available" memory:
/// the OS keeps a lot of it in caches it will happily give back.
#[cfg(target_os = "macos")]
pub fn free_memory() -> u64 {
    let pages = sysctl_u64("vm.page_free_count").unwrap_or(0);
    let size = sysctl_u64("hw.pagesize").unwrap_or(4096);
    pages.saturating_mul(size)
}

#[cfg(target_os = "macos")]
pub fn uptime() -> f64 {
    let mut boot = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
    let mut size = std::mem::size_of::<libc::timeval>();
    let out = (&mut boot) as *mut libc::timeval as *mut libc::c_void;
    if !sysctl_raw("kern.boottime", out, &mut size) {
        return 0.0;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    now.saturating_sub(boot.tv_sec as u64) as f64
}

#[cfg(target_os = "macos")]
pub fn loadavg() -> [f64; 3] {
    let mut values = [0.0f64; 3];
    let count = unsafe { libc::getloadavg(values.as_mut_ptr(), 3) };
    if count == 3 { values } else { [0.0; 3] }
}

#[cfg(target_os = "linux")]
fn meminfo(key: &str) -> u64 {
    let Ok(text) = std::fs::read_to_string("/proc/meminfo") else {
        return 0;
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(key) {
            let digits: String = rest.chars().filter(char::is_ascii_digit).collect();
            return digits.parse::<u64>().unwrap_or(0) * 1024;
        }
    }
    0
}

#[cfg(target_os = "linux")]
pub fn total_memory() -> u64 {
    meminfo("MemTotal:")
}

#[cfg(target_os = "linux")]
pub fn free_memory() -> u64 {
    meminfo("MemAvailable:")
}

#[cfg(target_os = "linux")]
pub fn uptime() -> f64 {
    let Ok(text) = std::fs::read_to_string("/proc/uptime") else {
        return 0.0;
    };
    text.split_whitespace()
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0.0)
}

#[cfg(target_os = "linux")]
pub fn loadavg() -> [f64; 3] {
    let Ok(text) = std::fs::read_to_string("/proc/loadavg") else {
        return [0.0; 3];
    };
    let mut values = [0.0f64; 3];
    for (slot, field) in values.iter_mut().zip(text.split_whitespace()) {
        *slot = field.parse().unwrap_or(0.0);
    }
    values
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "linux")))]
pub fn total_memory() -> u64 {
    let pages = unsafe { libc::sysconf(libc::_SC_PHYS_PAGES) };
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if pages <= 0 || size <= 0 {
        return 0;
    }
    pages as u64 * size as u64
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "linux")))]
pub fn free_memory() -> u64 {
    let pages = unsafe { libc::sysconf(libc::_SC_AVPHYS_PAGES) };
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if pages <= 0 || size <= 0 {
        return 0;
    }
    pages as u64 * size as u64
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "linux")))]
pub fn uptime() -> f64 {
    0.0
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "linux")))]
pub fn loadavg() -> [f64; 3] {
    let mut values = [0.0f64; 3];
    let count = unsafe { libc::getloadavg(values.as_mut_ptr(), 3) };
    if count == 3 { values } else { [0.0; 3] }
}

#[cfg(not(unix))]
pub fn total_memory() -> u64 {
    0
}

#[cfg(not(unix))]
pub fn free_memory() -> u64 {
    0
}

#[cfg(not(unix))]
pub fn uptime() -> f64 {
    0.0
}

#[cfg(not(unix))]
pub fn loadavg() -> [f64; 3] {
    [0.0; 3]
}

/// Peak resident set size, in bytes. This is the high-water mark the kernel
/// keeps, not a live reading: macOS counts bytes, Linux counts kilobytes.
#[cfg(unix)]
pub fn rss() -> u64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    let ok = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } == 0;
    if !ok {
        return 0;
    }
    let usage = unsafe { usage.assume_init() };
    let raw = usage.ru_maxrss.max(0) as u64;
    if cfg!(target_os = "macos") { raw } else { raw * 1024 }
}

#[cfg(not(unix))]
pub fn rss() -> u64 {
    0
}

#[cfg(unix)]
pub fn parent_pid() -> u32 {
    unsafe { libc::getppid() as u32 }
}

#[cfg(not(unix))]
pub fn parent_pid() -> u32 {
    0
}

/// Whether a descriptor is a terminal, as tty.isatty reports it.
pub fn isatty(fd: i32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: isatty only reads the descriptor's file status.
        unsafe { libc::isatty(fd) == 1 }
    }
    #[cfg(not(unix))]
    {
        use std::io::IsTerminal;
        match fd {
            0 => std::io::stdin().is_terminal(),
            1 => std::io::stdout().is_terminal(),
            2 => std::io::stderr().is_terminal(),
            _ => false,
        }
    }
}

/// Terminal size as (columns, rows); None when the descriptor is not a
/// terminal or the kernel will not say.
pub fn window_size(fd: i32) -> Option<(u16, u16)> {
    #[cfg(unix)]
    {
        // SAFETY: TIOCGWINSZ fills in a winsize the kernel owns the layout of.
        let mut size: libc::winsize = unsafe { std::mem::zeroed() };
        let result = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut size) };
        if result == 0 && size.ws_col > 0 {
            Some((size.ws_col, size.ws_row))
        } else {
            None
        }
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        None
    }
}
