//! On-demand native process-memory and allocator audit for the diagnostic
//! export (#1344 follow-up).
//!
//! Observation only. This module reads OS counters and allocator statistics; it
//! never triggers GC, allocator trim/purge, cache eviction, or allocation
//! tracking, never inspects allocation contents, and never spawns a process. It
//! samples exactly once per call — no periodic or per-frame sampling lives here.
//!
//! Coverage is the **native application process only**. WebContent/GPU footprint
//! is not measured and is reported as `unavailable`, so a reader never adds these
//! numbers to a renderer footprint.
//!
//! Every measurement is an `Option`: `None` serializes as the closed token
//! `unavailable`, never as a fabricated zero, and its measurement group carries a
//! `*_status` token that separates `unavailable` (no cheap API, or an absent
//! symbol) from `failed` (the call ran and did not return a usable value).
//!
//! Platform notes:
//! - macOS: `proc_pid_rusage` (`ri_phys_footprint`, `ri_resident_size`) and
//!   `malloc_zone_statistics` (libmalloc in-use/allocated).
//! - Linux/glibc: the `/proc/self/status` RSS breakdown, plus `mallinfo2` arena
//!   statistics resolved through `dlsym`. A runtime without the symbol (pre
//!   glibc 2.33) reports `unavailable` instead of failing to load, and the
//!   legacy overflowing `mallinfo` is deliberately not used as a fallback.
//! - Windows: `GetProcessMemoryInfo` working-set and private-commit bytes.
//! - Any other target: everything unavailable.

use std::sync::OnceLock;
use std::time::Instant;

use koushi_diagnostics::{DiagnosticEvent, DiagnosticField, DiagnosticLevel};

/// Schema version of the `desktop.process_memory` audit event.
pub const MEMORY_AUDIT_SCHEMA_VERSION: u64 = 1;

const SOURCE_UNAVAILABLE: &str = "unavailable";
const STATUS_AVAILABLE: &str = "available";
const STATUS_UNAVAILABLE: &str = "unavailable";
const STATUS_FAILED: &str = "failed";

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

/// Record the desktop process entry so `uptime_ms` measures elapsed time since
/// this process started rather than since the first diagnostic sample.
pub fn mark_process_start() {
    let _ = PROCESS_START.set(Instant::now());
}

/// Native-process memory for the platform that has a cheap API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeProcessMemory {
    pub source: &'static str,
    pub status: &'static str,
    /// Physical footprint (macOS `ri_phys_footprint`).
    pub footprint_bytes: Option<u64>,
    /// Resident bytes (macOS resident size, Linux `VmRSS`, Windows working set).
    pub resident_bytes: Option<u64>,
    /// Linux `RssAnon`.
    pub anonymous_bytes: Option<u64>,
    /// Linux `RssFile`.
    pub file_bytes: Option<u64>,
    /// Linux `RssShmem`.
    pub shared_bytes: Option<u64>,
    /// Windows `PROCESS_MEMORY_COUNTERS_EX.PrivateUsage`; private commit is not
    /// resident memory.
    pub private_commit_bytes: Option<u64>,
}

impl NativeProcessMemory {
    /// Nothing was measured for this group.
    pub const fn unavailable() -> Self {
        Self {
            source: SOURCE_UNAVAILABLE,
            status: STATUS_UNAVAILABLE,
            footprint_bytes: None,
            resident_bytes: None,
            anonymous_bytes: None,
            file_bytes: None,
            shared_bytes: None,
            private_commit_bytes: None,
        }
    }

    const fn failed(source: &'static str) -> Self {
        Self {
            source,
            status: STATUS_FAILED,
            ..Self::unavailable()
        }
    }
}

/// Allocator statistics. Field meanings follow the `source` token; groups are
/// not comparable across providers and never sum with process memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AllocatorMemory {
    pub source: &'static str,
    pub status: &'static str,
    /// libmalloc `size_in_use` / glibc `mallinfo2.uordblks`.
    pub in_use_bytes: Option<u64>,
    /// libmalloc `size_allocated`.
    pub allocated_bytes: Option<u64>,
    /// glibc `mallinfo2.fordblks`.
    pub free_bytes: Option<u64>,
    /// glibc `mallinfo2.hblkhd` (mmap'd arena bytes).
    pub mmap_bytes: Option<u64>,
}

impl AllocatorMemory {
    /// Nothing was measured for this group.
    pub const fn unavailable() -> Self {
        Self {
            source: SOURCE_UNAVAILABLE,
            status: STATUS_UNAVAILABLE,
            in_use_bytes: None,
            allocated_bytes: None,
            free_bytes: None,
            mmap_bytes: None,
        }
    }
}

/// One on-demand sample of native process memory and allocator statistics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryAuditSample {
    /// Elapsed time since the desktop process entry, when it was marked.
    pub uptime_ms: Option<u64>,
    /// Duration of the native and allocator sampling below. It excludes the
    /// existing cache summaries and the diagnostic serialization.
    pub native_sample_duration_us: u64,
    pub native: NativeProcessMemory,
    pub allocator: AllocatorMemory,
}

/// Sample native process memory and allocator statistics once.
pub fn sample_process_memory_audit() -> MemoryAuditSample {
    let started = Instant::now();
    let native = sample_native_process_memory();
    let allocator = sample_allocator_memory();
    MemoryAuditSample {
        uptime_ms: process_uptime_ms(),
        native_sample_duration_us: duration_micros(started.elapsed()),
        native,
        allocator,
    }
}

/// Build the content-free diagnostic record for one sample.
pub fn process_memory_audit_event(sample: &MemoryAuditSample) -> DiagnosticEvent {
    DiagnosticEvent::new(DiagnosticLevel::Info, "desktop.process_memory", "audit")
        .field(DiagnosticField::count(
            "memory_audit_schema",
            MEMORY_AUDIT_SCHEMA_VERSION,
        ))
        .field(DiagnosticField::token("os", target_os_token()))
        .field(DiagnosticField::token("arch", target_arch_token()))
        .field(DiagnosticField::token(
            "native_memory_scope",
            "desktop_process_only",
        ))
        .field(DiagnosticField::token(
            "renderer_gpu_footprint",
            STATUS_UNAVAILABLE,
        ))
        .field(optional_count("uptime_ms", sample.uptime_ms))
        .field(DiagnosticField::count(
            "native_sample_duration_us",
            sample.native_sample_duration_us,
        ))
        .field(DiagnosticField::token(
            "native_memory_source",
            sample.native.source,
        ))
        .field(DiagnosticField::token(
            "native_memory_status",
            sample.native.status,
        ))
        .field(optional_count(
            "footprint_bytes",
            sample.native.footprint_bytes,
        ))
        .field(optional_count(
            "resident_bytes",
            sample.native.resident_bytes,
        ))
        .field(optional_count(
            "anonymous_bytes",
            sample.native.anonymous_bytes,
        ))
        .field(optional_count("file_bytes", sample.native.file_bytes))
        .field(optional_count("shared_bytes", sample.native.shared_bytes))
        .field(optional_count(
            "private_commit_bytes",
            sample.native.private_commit_bytes,
        ))
        .field(DiagnosticField::token(
            "allocator_source",
            sample.allocator.source,
        ))
        .field(DiagnosticField::token(
            "allocator_status",
            sample.allocator.status,
        ))
        .field(optional_count(
            "allocator_in_use_bytes",
            sample.allocator.in_use_bytes,
        ))
        .field(optional_count(
            "allocator_allocated_bytes",
            sample.allocator.allocated_bytes,
        ))
        .field(optional_count(
            "allocator_free_bytes",
            sample.allocator.free_bytes,
        ))
        .field(optional_count(
            "allocator_mmap_bytes",
            sample.allocator.mmap_bytes,
        ))
}

/// A missing measurement is an explicit closed token, never a zero.
fn optional_count(key: &'static str, value: Option<u64>) -> DiagnosticField {
    match value {
        Some(value) => DiagnosticField::count(key, value),
        None => DiagnosticField::token(key, STATUS_UNAVAILABLE),
    }
}

fn process_uptime_ms() -> Option<u64> {
    PROCESS_START.get().map(|start| {
        start
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX))
            .try_into()
            .expect("clamped to u64::MAX")
    })
}

fn duration_micros(duration: std::time::Duration) -> u64 {
    duration
        .as_micros()
        .min(u128::from(u64::MAX))
        .try_into()
        .expect("clamped to u64::MAX")
}

const fn target_os_token() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(windows) {
        "windows"
    } else {
        "other"
    }
}

const fn target_arch_token() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        "other"
    }
}

#[cfg(target_os = "macos")]
fn sample_native_process_memory() -> NativeProcessMemory {
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
    // SAFETY: `proc_pid_rusage` writes the flavor-sized struct we pass for our
    // own pid; the layout comes from the `libc` binding for that flavor.
    let result = unsafe {
        libc::proc_pid_rusage(
            libc::getpid(),
            libc::RUSAGE_INFO_V2,
            info.as_mut_ptr().cast(),
        )
    };
    if result != 0 {
        return NativeProcessMemory::failed("proc_pid_rusage");
    }
    // SAFETY: a zero return means the round trip filled the struct.
    let info = unsafe { info.assume_init() };
    NativeProcessMemory {
        source: "proc_pid_rusage",
        status: STATUS_AVAILABLE,
        footprint_bytes: Some(info.ri_phys_footprint),
        resident_bytes: Some(info.ri_resident_size),
        ..NativeProcessMemory::unavailable()
    }
}

#[cfg(target_os = "linux")]
fn sample_native_process_memory() -> NativeProcessMemory {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return NativeProcessMemory::failed("proc_self_status");
    };
    let memory = parse_linux_status(&status);
    if memory.is_empty() {
        return NativeProcessMemory::failed("proc_self_status");
    }
    NativeProcessMemory {
        source: "proc_self_status",
        status: STATUS_AVAILABLE,
        resident_bytes: memory.rss_bytes,
        anonymous_bytes: memory.anonymous_bytes,
        file_bytes: memory.file_bytes,
        shared_bytes: memory.shared_bytes,
        ..NativeProcessMemory::unavailable()
    }
}

#[cfg(windows)]
fn sample_native_process_memory() -> NativeProcessMemory {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    let buffer_bytes = counters.cb;
    // SAFETY: `counters` is a correctly sized, zeroed counter struct for the
    // process handle the API expects; the API writes at most `buffer_bytes`.
    let ok = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            std::ptr::from_mut(&mut counters).cast(),
            buffer_bytes,
        )
    };
    if ok == 0 {
        return NativeProcessMemory::failed("process_memory_counters");
    }
    NativeProcessMemory {
        source: "process_memory_counters",
        status: STATUS_AVAILABLE,
        resident_bytes: Some(counters.WorkingSetSize as u64),
        private_commit_bytes: Some(counters.PrivateUsage as u64),
        ..NativeProcessMemory::unavailable()
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn sample_native_process_memory() -> NativeProcessMemory {
    NativeProcessMemory::unavailable()
}

#[cfg(target_os = "macos")]
fn sample_allocator_memory() -> AllocatorMemory {
    let mut stats = std::mem::MaybeUninit::<libc::malloc_statistics_t>::zeroed();
    // SAFETY: a null zone aggregates all zones; `libc` owns the struct layout.
    unsafe { libc::malloc_zone_statistics(std::ptr::null_mut(), stats.as_mut_ptr()) };
    // SAFETY: `malloc_zone_statistics` fills the struct we pass.
    let stats = unsafe { stats.assume_init() };
    AllocatorMemory {
        source: "libmalloc_malloc_zone_statistics",
        status: STATUS_AVAILABLE,
        in_use_bytes: Some(stats.size_in_use as u64),
        allocated_bytes: Some(stats.size_allocated as u64),
        // libmalloc exposes no free-byte statistic; do not derive one.
        ..AllocatorMemory::unavailable()
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn sample_allocator_memory() -> AllocatorMemory {
    let Some(mallinfo2) = resolve_mallinfo2() else {
        // The runtime libc is older than glibc 2.33 and has no symbol.
        return AllocatorMemory::unavailable();
    };
    // SAFETY: `resolve_mallinfo2` only returns glibc's `mallinfo2` symbol, which
    // takes no arguments and fills allocator-owned statistics.
    let info = unsafe { mallinfo2() };
    AllocatorMemory {
        source: "glibc_mallinfo2",
        status: STATUS_AVAILABLE,
        in_use_bytes: Some(info.uordblks as u64),
        allocated_bytes: None,
        free_bytes: Some(info.fordblks as u64),
        mmap_bytes: Some(info.hblkhd as u64),
    }
}

#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
fn sample_allocator_memory() -> AllocatorMemory {
    AllocatorMemory::unavailable()
}

/// Resolve `mallinfo2` at runtime so an older libc reports `unavailable`
/// instead of failing at load time for a diagnostic-only measurement.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn resolve_mallinfo2() -> Option<unsafe extern "C" fn() -> libc::mallinfo2> {
    // SAFETY: `dlsym` only looks up a symbol name in the default scope.
    let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"mallinfo2".as_ptr()) };
    if symbol.is_null() {
        return None;
    }
    // SAFETY: the resolved symbol is glibc's `mallinfo2`, which takes no
    // arguments and returns `struct mallinfo2` by value.
    Some(unsafe {
        std::mem::transmute::<*mut libc::c_void, unsafe extern "C" fn() -> libc::mallinfo2>(symbol)
    })
}

/// `/proc/self/status` RSS fields, in bytes. `RssAnon`/`RssFile`/`RssShmem`
/// were added in Linux 4.5, so an older kernel leaves them `None`.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct LinuxStatusMemory {
    rss_bytes: Option<u64>,
    anonymous_bytes: Option<u64>,
    file_bytes: Option<u64>,
    shared_bytes: Option<u64>,
}

#[cfg(target_os = "linux")]
impl LinuxStatusMemory {
    const fn is_empty(&self) -> bool {
        self.rss_bytes.is_none()
            && self.anonymous_bytes.is_none()
            && self.file_bytes.is_none()
            && self.shared_bytes.is_none()
    }
}

#[cfg(target_os = "linux")]
fn parse_linux_status(status: &str) -> LinuxStatusMemory {
    LinuxStatusMemory {
        rss_bytes: status_kib(status, "VmRSS"),
        anonymous_bytes: status_kib(status, "RssAnon"),
        file_bytes: status_kib(status, "RssFile"),
        shared_bytes: status_kib(status, "RssShmem"),
    }
}

#[cfg(target_os = "linux")]
fn status_kib(status: &str, key: &str) -> Option<u64> {
    for line in status.lines() {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        if name.trim() != key {
            continue;
        }
        let value = rest.split_whitespace().next()?.parse::<u64>().ok()?;
        return Some(value.saturating_mul(1024));
    }
    None
}

#[cfg(test)]
mod tests;
