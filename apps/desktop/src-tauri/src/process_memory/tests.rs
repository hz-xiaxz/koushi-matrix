use super::*;
use koushi_diagnostics::DiagnosticValue;

fn audit_sample(native: NativeProcessMemory, allocator: AllocatorMemory) -> MemoryAuditSample {
    MemoryAuditSample {
        uptime_ms: Some(1_234),
        native_sample_duration_us: 7,
        native,
        allocator,
    }
}

fn field<'a>(event: &'a DiagnosticEvent, key: &str) -> &'a DiagnosticValue {
    event
        .fields
        .iter()
        .find(|field| field.key == key)
        .map(|field| &field.value)
        .unwrap_or_else(|| panic!("missing field {key}"))
}

#[test]
fn audit_event_labels_context_sources_and_units() {
    let sample = audit_sample(
        NativeProcessMemory {
            source: "proc_self_status",
            status: STATUS_AVAILABLE,
            resident_bytes: Some(2048),
            anonymous_bytes: Some(1024),
            ..NativeProcessMemory::unavailable()
        },
        AllocatorMemory {
            source: "glibc_mallinfo2",
            status: STATUS_AVAILABLE,
            in_use_bytes: Some(4096),
            free_bytes: Some(8192),
            ..AllocatorMemory::unavailable()
        },
    );
    let event = process_memory_audit_event(&sample);

    assert_eq!(event.source, "desktop.process_memory");
    assert_eq!(event.stage, "audit");
    assert_eq!(
        field(&event, "memory_audit_schema"),
        &DiagnosticValue::Count(MEMORY_AUDIT_SCHEMA_VERSION)
    );
    assert_eq!(
        field(&event, "os"),
        &DiagnosticValue::Token(target_os_token())
    );
    assert_eq!(
        field(&event, "arch"),
        &DiagnosticValue::Token(target_arch_token())
    );
    // The numbers never imply renderer coverage.
    assert_eq!(
        field(&event, "native_memory_scope"),
        &DiagnosticValue::Token("desktop_process_only")
    );
    assert_eq!(
        field(&event, "renderer_gpu_footprint"),
        &DiagnosticValue::Token("unavailable")
    );
    assert_eq!(
        field(&event, "native_memory_source"),
        &DiagnosticValue::Token("proc_self_status")
    );
    assert_eq!(
        field(&event, "native_memory_status"),
        &DiagnosticValue::Token("available")
    );
    assert_eq!(
        field(&event, "allocator_source"),
        &DiagnosticValue::Token("glibc_mallinfo2")
    );
    assert_eq!(
        field(&event, "allocator_status"),
        &DiagnosticValue::Token("available")
    );
    assert_eq!(
        field(&event, "resident_bytes"),
        &DiagnosticValue::Count(2048)
    );
    assert_eq!(
        field(&event, "allocator_in_use_bytes"),
        &DiagnosticValue::Count(4096)
    );
    assert_eq!(field(&event, "uptime_ms"), &DiagnosticValue::Count(1_234));
    assert_eq!(
        field(&event, "native_sample_duration_us"),
        &DiagnosticValue::Count(7)
    );
}

#[test]
fn missing_measurements_are_explicit_unavailable_not_zero() {
    let sample = audit_sample(
        NativeProcessMemory::failed("proc_pid_rusage"),
        AllocatorMemory::unavailable(),
    );
    let event = process_memory_audit_event(&sample);

    for key in [
        "footprint_bytes",
        "resident_bytes",
        "anonymous_bytes",
        "file_bytes",
        "shared_bytes",
        "private_commit_bytes",
        "allocator_in_use_bytes",
        "allocator_allocated_bytes",
        "allocator_free_bytes",
        "allocator_mmap_bytes",
    ] {
        assert_eq!(
            field(&event, key),
            &DiagnosticValue::Token("unavailable"),
            "{key} must stay unavailable"
        );
    }
    assert_eq!(
        field(&event, "native_memory_status"),
        &DiagnosticValue::Token("failed")
    );
    assert_eq!(
        field(&event, "allocator_status"),
        &DiagnosticValue::Token("unavailable")
    );
}

#[test]
fn measured_zero_stays_a_measured_zero() {
    let sample = audit_sample(
        NativeProcessMemory {
            source: "proc_self_status",
            status: STATUS_AVAILABLE,
            resident_bytes: Some(0),
            ..NativeProcessMemory::unavailable()
        },
        AllocatorMemory::unavailable(),
    );
    let event = process_memory_audit_event(&sample);

    assert_eq!(field(&event, "resident_bytes"), &DiagnosticValue::Count(0));
}

#[test]
fn unmarked_process_start_leaves_uptime_unavailable() {
    // Another test may have marked the shared process start; build a sample
    // directly so the unmeasured path is exercised deterministically.
    let sample = MemoryAuditSample {
        uptime_ms: None,
        native_sample_duration_us: 0,
        native: NativeProcessMemory::unavailable(),
        allocator: AllocatorMemory::unavailable(),
    };
    let event = process_memory_audit_event(&sample);

    assert_eq!(
        field(&event, "uptime_ms"),
        &DiagnosticValue::Token("unavailable")
    );
}

#[test]
fn marking_the_process_start_publishes_a_uptime() {
    mark_process_start();
    let sample = sample_process_memory_audit();
    assert!(sample.uptime_ms.is_some());
}

#[test]
fn event_serialization_stays_content_free() {
    let sample = audit_sample(
        NativeProcessMemory {
            source: "proc_self_status",
            status: STATUS_AVAILABLE,
            resident_bytes: Some(2048),
            ..NativeProcessMemory::unavailable()
        },
        AllocatorMemory::unavailable(),
    );
    let serialized = serde_json::to_string(&process_memory_audit_event(&sample)).unwrap();

    for forbidden in [
        "!room:synthetic.invalid",
        "@user:synthetic.invalid",
        "$event:synthetic.invalid",
        "/Users/alice/private",
        "synthetic message body",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "serialized audit leaked {forbidden}"
        );
    }
    assert!(serialized.contains("resident_bytes"));
}

#[cfg(target_os = "linux")]
#[test]
fn linux_status_parsing_reads_the_rss_breakdown_in_bytes() {
    let status = "\
Name:\tkoushi-desktop
VmRSS:\t   4096 kB
RssAnon:\t   2048 kB
RssFile:\t   1536 kB
RssShmem:\t    512 kB
Threads:\t12
";
    assert_eq!(
        parse_linux_status(status),
        LinuxStatusMemory {
            rss_bytes: Some(4_194_304),
            anonymous_bytes: Some(2_097_152),
            file_bytes: Some(1_572_864),
            shared_bytes: Some(524_288),
        }
    );
}

#[cfg(target_os = "linux")]
#[test]
fn linux_status_parsing_keeps_missing_and_malformed_fields_unavailable() {
    // A kernel without the 4.5 breakdown still has VmRSS.
    assert_eq!(
        parse_linux_status("VmRSS:\t 1024 kB\n"),
        LinuxStatusMemory {
            rss_bytes: Some(1_048_576),
            ..LinuxStatusMemory::default()
        }
    );
    // Garbage never becomes a fabricated byte count.
    let malformed = parse_linux_status("VmRSS: not-a-number kB\nRssAnon: 1 kB\n");
    assert_eq!(malformed.rss_bytes, None);
    assert_eq!(malformed.anonymous_bytes, Some(1024));
    assert!(parse_linux_status("Name:\tkoushi\n").is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn linux_audit_reports_resident_bytes_from_the_live_process() {
    let sample = sample_process_memory_audit();

    assert_eq!(sample.native.source, "proc_self_status");
    assert_eq!(sample.native.status, "available");
    assert!(sample.native.resident_bytes.is_some_and(|bytes| bytes > 0));
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[test]
fn glibc_allocator_statistics_are_resolved_or_explicitly_absent() {
    let sample = sample_process_memory_audit();

    match sample.allocator.status {
        "available" => {
            assert_eq!(sample.allocator.source, "glibc_mallinfo2");
            assert!(sample.allocator.in_use_bytes.is_some());
        }
        "unavailable" => assert_eq!(sample.allocator.source, "unavailable"),
        other => panic!("unexpected allocator status {other}"),
    }
}

#[cfg(target_os = "macos")]
#[test]
fn macos_audit_reports_footprint_and_allocator_bytes() {
    let sample = sample_process_memory_audit();

    assert_eq!(sample.native.source, "proc_pid_rusage");
    assert_eq!(sample.native.status, "available");
    assert!(sample.native.footprint_bytes.is_some_and(|bytes| bytes > 0));
    assert!(sample.native.resident_bytes.is_some_and(|bytes| bytes > 0));
    assert_eq!(sample.allocator.source, "libmalloc_malloc_zone_statistics");
    assert_eq!(sample.allocator.status, "available");
    assert!(sample.allocator.in_use_bytes.is_some_and(|bytes| bytes > 0));
    assert!(
        sample
            .allocator
            .allocated_bytes
            .is_some_and(|bytes| bytes > 0)
    );
    // libmalloc exposes no free-byte statistic; an invented one would be a bug.
    assert_eq!(sample.allocator.free_bytes, None);
}

#[cfg(windows)]
#[test]
fn windows_audit_reports_working_set_and_private_commit() {
    let sample = sample_process_memory_audit();

    assert_eq!(sample.native.source, "process_memory_counters");
    assert_eq!(sample.native.status, "available");
    assert!(sample.native.resident_bytes.is_some_and(|bytes| bytes > 0));
    assert!(
        sample
            .native
            .private_commit_bytes
            .is_some_and(|bytes| bytes > 0)
    );
    assert_eq!(sample.allocator.source, "unavailable");
    assert_eq!(sample.allocator.status, "unavailable");
}

/// Manual collection-overhead measurement for the #1344 acceptance criterion.
///
/// Workload: 2000 iterations of the one collection step this change adds to a
/// diagnostic export, `sample_process_memory_audit()` — one
/// `/proc/self/status` read plus one `mallinfo2` call on Linux, one
/// `proc_pid_rusage` plus one `malloc_zone_statistics` call on macOS, one
/// `GetProcessMemoryInfo` call on Windows. The pre-existing named-cache
/// summaries and the diagnostic ring serialization are unchanged by this PR
/// and are not part of the measured figure. It is not an end-to-end export
/// benchmark: the Tauri IPC round trip is excluded.
///
/// Configuration: Linux/x86_64, 64 logical cores, single test thread
/// (`--test-threads=1`), `ci` profile (debug, `opt-level = 0`), so a packaged
/// release build is faster. Measured results across two runs: 53 and 55 us per
/// sample.
///
/// Run with
/// `cargo test --profile ci -p koushi-desktop --lib measure_native_sample_overhead -- --ignored --nocapture --test-threads=1`.
#[test]
#[ignore = "manual collection-overhead measurement"]
fn measure_native_sample_overhead() {
    const ITERATIONS: u32 = 2_000;
    let _ = sample_process_memory_audit();
    let started = Instant::now();
    for _ in 0..ITERATIONS {
        std::hint::black_box(sample_process_memory_audit());
    }
    let elapsed = started.elapsed();
    eprintln!(
        "native_sample_overhead iterations={ITERATIONS} total_us={} per_sample_us={:.3}",
        elapsed.as_micros(),
        elapsed.as_secs_f64() * 1_000_000.0 / f64::from(ITERATIONS)
    );
}
