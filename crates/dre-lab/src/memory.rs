//! Peak memory, read from the operating system.
//!
//! Plan section 20 wants peak memory as a benchmark metric. Rust's standard
//! library exposes no portable way to ask, and the two obvious workarounds
//! are both worse than reading `/proc`:
//!
//! - A counting global allocator would need `unsafe impl GlobalAlloc`, and
//!   `unsafe_code` is **forbidden** workspace-wide. Forbidden is not merely
//!   denied — it cannot be lifted by a local allow, which is the whole
//!   point of choosing it. Weakening that setting to gain a benchmark
//!   number would be a poor trade.
//! - Counting allocations by hand in the engine would put a measurement
//!   inside the code being measured, and give the engine a number it could
//!   in principle branch on.
//!
//! So the reading is taken from outside the engine entirely, by the harness,
//! from the kernel's own accounting. Nothing the engine can reach changes.
//!
//! # What the number actually means
//!
//! `VmHWM` is the high-water mark of the process's resident set size, in
//! kibibytes, since the process started. Three consequences worth stating
//! plainly, because a benchmark figure that is quietly measuring something
//! else is worse than no figure:
//!
//! 1. It covers the **whole process**, harness included — the binary's own
//!    text and the CLI's buffers are in there, not only the reasoning.
//! 2. It never decreases, so it cannot be sampled per iteration. What a
//!    benchmark loop can honestly report is the peak at the end and how
//!    much it grew across the loop, which is the leak signal anyway.
//! 3. It is resident memory, not allocated memory. Pages the allocator
//!    holds but has never touched do not count.
//!
//! # Portability
//!
//! Linux only, by way of `/proc/self/status`. Everywhere else this returns
//! `None` and callers report the metric as unavailable rather than
//! substituting a zero — a zero would read as "used no memory", which is
//! a false claim, where "unavailable" is a true one. Development and CI both
//! run on the Ubuntu dev VM, so in practice the number is always there.

use std::path::Path;

/// Where the kernel publishes per-process memory accounting.
const STATUS_PATH: &str = "/proc/self/status";

/// Resident memory at one instant, in kibibytes.
///
/// Both fields come from a single read. Reading them separately looks
/// harmless and is not: the process can grow between the two reads, so a
/// `current` sampled after a `peak` can legitimately exceed it, and the
/// invariant the pair is supposed to have quietly stops holding. Taking one
/// snapshot makes `current <= peak` true by construction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Snapshot {
    /// Resident set size right now.
    pub current_kib: u64,
    /// Highest resident set size since the process started.
    pub peak_kib: u64,
}

/// Read both figures at once.
///
/// `None` when the platform does not publish them.
#[must_use]
pub fn snapshot() -> Option<Snapshot> {
    read_snapshot(Path::new(STATUS_PATH))
}

/// Peak resident set size of this process, in kibibytes.
///
/// `None` when the platform does not publish it.
#[must_use]
pub fn peak_rss_kib() -> Option<u64> {
    snapshot().map(|snapshot| snapshot.peak_kib)
}

/// Current resident set size of this process, in kibibytes.
#[must_use]
pub fn current_rss_kib() -> Option<u64> {
    snapshot().map(|snapshot| snapshot.current_kib)
}

/// Render a kibibyte count for display, or say it is unavailable.
///
/// Centralised so every report words the missing case the same way, and so
/// that no caller is tempted to default it to zero.
#[must_use]
pub fn show_kib(value: Option<u64>) -> String {
    value.map_or_else(
        || "unavailable on this platform".to_owned(),
        |kib| format!("{kib} KiB"),
    )
}

/// Read a status file once and pull both figures out of it.
///
/// Split into a path-taking helper so the parser can be tested against a
/// fixture rather than only against whatever the running kernel happens to
/// report — a parser exercised solely by live data is one whose failure
/// modes are never seen.
fn read_snapshot(path: &Path) -> Option<Snapshot> {
    let text = std::fs::read_to_string(path).ok()?;
    parse_snapshot(&text)
}

/// Pull both figures out of one `/proc/self/status` reading.
fn parse_snapshot(text: &str) -> Option<Snapshot> {
    Some(Snapshot {
        current_kib: parse_field(text, "VmRSS:")?,
        peak_kib: parse_field(text, "VmHWM:")?,
    })
}

/// Parse a `VmHWM:     12345 kB` line out of `/proc/self/status` text.
fn parse_field(text: &str, field: &str) -> Option<u64> {
    text.lines()
        .find_map(|line| line.strip_prefix(field))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|number| number.parse().ok())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    const SAMPLE: &str = "Name:\tdre\nVmPeak:\t   18004 kB\nVmSize:\t   18004 kB\nVmHWM:\t    9216 kB\nVmRSS:\t    8192 kB\n";

    #[test]
    fn reads_the_high_water_mark() {
        assert_eq!(parse_field(SAMPLE, "VmHWM:"), Some(9_216));
        assert_eq!(parse_field(SAMPLE, "VmRSS:"), Some(8_192));
    }

    #[test]
    fn does_not_confuse_a_prefix_for_the_field() {
        // `VmHWM` and `VmH` would both match a naive `contains`, and
        // `VmPeak` sits directly above `VmHWM` in the real file.
        assert_eq!(parse_field(SAMPLE, "VmPeak:"), Some(18_004));
        assert_eq!(parse_field(SAMPLE, "VmH:"), None);
    }

    #[test]
    fn an_absent_field_is_none_not_zero() {
        assert_eq!(parse_field("Name:\tdre\n", "VmHWM:"), None);
        assert_eq!(read_snapshot(Path::new("/definitely/not/here")), None);
    }

    #[test]
    fn a_snapshot_is_internally_consistent() {
        // The property that only holds because both fields come from one
        // read. Two separate reads of the live process can and do report a
        // current larger than an earlier peak.
        let taken = parse_snapshot(SAMPLE).expect("both fields present");
        assert_eq!(taken.peak_kib, 9_216);
        assert_eq!(taken.current_kib, 8_192);
        assert!(taken.current_kib <= taken.peak_kib);

        // A kernel that published only one of the two is reported as
        // unavailable rather than as a half-filled snapshot.
        assert_eq!(parse_snapshot("VmRSS:\t 8192 kB\n"), None);
    }

    #[test]
    fn unavailable_never_renders_as_a_quantity() {
        assert_eq!(show_kib(Some(9_216)), "9216 KiB");
        assert!(!show_kib(None).contains('0'));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn the_live_reading_is_present_and_sane() {
        let taken = snapshot().expect("Linux publishes VmRSS and VmHWM");
        assert!(taken.peak_kib > 0, "a running process has a non-zero peak");
        assert!(
            taken.current_kib <= taken.peak_kib,
            "current {} exceeds the high-water mark {}",
            taken.current_kib,
            taken.peak_kib
        );
    }
}
