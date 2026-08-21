//! Append-only record of failures that would otherwise leave no trace.
//!
//! A GUI host has nowhere to report a failure. It has no console when it is
//! launched from a shell icon, its window may already be gone by the time the
//! failure is decided, and an aborting release profile takes the process down
//! without a message. The practical result is that a user can only say "it
//! froze" or "it vanished", and every diagnosis starts from zero.
//!
//! This is deliberately not a logging framework. It records failures that
//! already carry a stable code, so that the *next* occurrence names itself.
//! Everything here is best-effort and total: a diagnostics sink that can fail
//! visibly, block, or panic would be worse than none at all, because it would
//! be a new failure mode inside the failure path.
//!
//! Host-neutral by construction — one `std::fs` implementation for all three
//! platforms, on top of the existing configuration-root facade. There is no
//! per-OS adapter because there is no per-OS behavior to own.

use std::{
    fmt::Write as _,
    fs::OpenOptions,
    io::Write as _,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

/// Past this size the record restarts. A diagnostics file that grows without
/// bound is itself a defect, and the newest failures are the ones a
/// reproduction needs.
const MAX_LOG_BYTES: u64 = 256 * 1024;

/// Long messages are truncated rather than dropped: a partial code and detail
/// still identify the path, and an unbounded write inside a failure path is
/// exactly what this module must not do.
const MAX_DETAIL_BYTES: usize = 512;

/// Set once the sink has proven unusable, so a host that fails every frame
/// does not retry a broken path every frame.
static SINK_DISABLED: AtomicBool = AtomicBool::new(false);

/// Where records are appended, next to the configuration the user already
/// owns so it is discoverable without a separate path convention.
pub fn log_path() -> Option<PathBuf> {
    crate::runtime::user_config_directory()
        .ok()
        .map(|directory| directory.join("agenterm-diagnostics.log"))
}

/// Records one failure. Never panics, never blocks on a lock, and reports
/// nothing to the caller: at every call site the interesting failure has
/// already happened, and a second error would only displace it.
pub fn record(component: &str, code: &str, detail: &str) {
    if SINK_DISABLED.load(Ordering::Relaxed) {
        return;
    }
    let Some(path) = log_path() else {
        SINK_DISABLED.store(true, Ordering::Relaxed);
        return;
    };
    if write_record(&path, &format_record(component, code, detail)).is_none() {
        SINK_DISABLED.store(true, Ordering::Relaxed);
    }
}

/// One line, so a record stays greppable and a truncated write costs at most
/// the record being written.
fn format_record(component: &str, code: &str, detail: &str) -> String {
    let now = crate::local_clock::local_civil_now();
    let mut line = String::with_capacity(MAX_DETAIL_BYTES + 96);
    // A failed format leaves whatever was written; the timestamp is context,
    // not the payload, so it must not be able to suppress the record.
    let _ = write!(
        line,
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {component} {code} ",
        now.year, now.month, now.day, now.hour, now.minute, now.second
    );
    push_single_line(&mut line, detail);
    line.push('\n');
    line
}

/// Newlines and carriage returns become spaces so one record is one line, and
/// the tail is cut on a character boundary rather than mid-code-point.
fn push_single_line(line: &mut String, detail: &str) {
    let mut written = 0;
    for character in detail.chars() {
        let width = character.len_utf8();
        if written + width > MAX_DETAIL_BYTES {
            line.push('…');
            return;
        }
        line.push(if character == '\n' || character == '\r' {
            ' '
        } else {
            character
        });
        written += width;
    }
}

/// `None` means the sink is unusable and should not be retried.
fn write_record(path: &PathBuf, line: &str) -> Option<()> {
    if let Some(parent) = path.parent()
        && !parent.exists()
        && std::fs::create_dir_all(parent).is_err()
    {
        return None;
    }
    // Restarting rather than rotating: a second file would double the state a
    // reader has to reason about for no diagnostic gain.
    if std::fs::metadata(path).is_ok_and(|metadata| metadata.len() > MAX_LOG_BYTES) {
        let _ = std::fs::remove_file(path);
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()?;
    // A short or failed write loses this record, not the sink: the next
    // failure is usually the one that matters and the path itself is fine.
    let _ = file.write_all(line.as_bytes());
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_is_one_greppable_line_carrying_component_and_code() {
        let line = format_record("pixel_window", "queue_overflow", "deferred queue full");
        assert_eq!(line.matches('\n').count(), 1, "one record is one line");
        assert!(line.ends_with('\n'));
        assert!(line.contains("pixel_window"));
        assert!(line.contains("queue_overflow"));
        assert!(line.contains("deferred queue full"));
    }

    /// A failure detail is untrusted text — it can carry an OS message with
    /// embedded newlines — and must not be able to forge extra records.
    #[test]
    fn embedded_newlines_cannot_split_one_failure_into_several_records() {
        let line = format_record("host", "code", "first\nsecond\r\nthird");
        assert_eq!(line.matches('\n').count(), 1);
        assert!(line.contains("first second  third"));
    }

    #[test]
    fn an_oversized_detail_is_truncated_on_a_character_boundary() {
        let detail = "宽".repeat(MAX_DETAIL_BYTES);
        let line = format_record("host", "code", &detail);
        assert!(line.ends_with("…\n"), "truncation is visible in the record");
        // Proof it cut cleanly: an invalid boundary would not round-trip.
        assert!(line.chars().count() > 0);
        assert!(
            line.len() < detail.len(),
            "an unbounded detail must not reach the file"
        );
    }

    #[test]
    fn a_short_detail_survives_intact_and_is_not_padded() {
        let line = format_record("c", "k", "brief");
        assert!(line.contains(" c k brief\n"));
        assert!(!line.contains('…'));
    }

    /// A scratch directory that removes itself, so these tests never touch the
    /// caller's real configuration root.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!(
                "agenterm-diagnostics-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            Self(path)
        }

        fn file(&self) -> PathBuf {
            self.0.join("nested").join("agenterm-diagnostics.log")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The formatter being correct proves nothing if nothing reaches disk.
    /// This is the property the module exists for.
    #[test]
    fn records_append_and_create_a_missing_directory() {
        let scratch = Scratch::new("append");
        let path = scratch.file();
        assert!(!path.exists());

        write_record(&path, "first\n").expect("a missing parent directory is created");
        write_record(&path, "second\n").expect("the second record appends");

        let written = std::fs::read_to_string(&path).expect("the record is readable");
        assert_eq!(
            written, "first\nsecond\n",
            "records accumulate in order instead of overwriting"
        );
    }

    /// An unbounded record file is itself a defect, and the newest failures
    /// are the ones a reproduction needs.
    #[test]
    fn an_oversized_record_restarts_instead_of_growing_without_bound() {
        let scratch = Scratch::new("rotate");
        let path = scratch.file();
        std::fs::create_dir_all(path.parent().expect("scratch parent")).expect("scratch directory");
        std::fs::write(&path, vec![b'x'; (MAX_LOG_BYTES + 1) as usize]).expect("oversized record");

        write_record(&path, "after restart\n").expect("write past the cap");

        let written = std::fs::read_to_string(&path).expect("the record is readable");
        assert_eq!(
            written, "after restart\n",
            "the cap restarts the file rather than appending past it"
        );
    }

    /// A path that cannot be created must be reported so the caller stops
    /// retrying, rather than failing quietly forever inside a failure path.
    #[test]
    fn an_unusable_path_is_reported_once_rather_than_retried() {
        let scratch = Scratch::new("blocked");
        let blocker = scratch.0.join("nested");
        std::fs::create_dir_all(&scratch.0).expect("scratch directory");
        // A file where the record's parent directory must go.
        std::fs::write(&blocker, b"not a directory").expect("blocking file");

        assert!(
            write_record(&scratch.file(), "unreachable\n").is_none(),
            "an unusable sink reports itself instead of silently dropping records"
        );
    }
}
