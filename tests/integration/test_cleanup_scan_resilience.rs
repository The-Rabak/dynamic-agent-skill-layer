use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, Utc};
use maintenance::PendingWarningScanner;

fn fresh_sandbox(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    let sandbox = std::env::temp_dir().join(format!("{prefix}-{nonce}"));
    std::fs::create_dir_all(&sandbox).expect("sandbox should be creatable");
    sandbox
}

#[test]
fn legacy_scan_continues_when_malformed_pending_exists() {
    let sandbox = fresh_sandbox("cleanup-scan-resilience");
    let proposal_root = sandbox.join(".skills");
    std::fs::create_dir_all(&proposal_root).expect("proposal root should be created");
    let healthy_pending_path = proposal_root.join("healthy/SKILL.md.pending");
    let malformed_pending_path = proposal_root.join("malformed/SKILL.md.pending");
    std::fs::create_dir_all(
        healthy_pending_path
            .parent()
            .expect("healthy pending path should have parent"),
    )
    .expect("healthy proposal directory should be created");
    std::fs::create_dir_all(
        malformed_pending_path
            .parent()
            .expect("malformed pending path should have parent"),
    )
    .expect("malformed proposal directory should be created");

    std::fs::write(
        &healthy_pending_path,
        "---\ncreated_at: 2026-01-01T00:00:00Z\nwarning_at: 2026-01-02T00:00:00Z\norigin: session_extraction\n---\n",
    )
    .expect("healthy pending file should be written");
    std::fs::write(
        &malformed_pending_path,
        "---\ncreated_at: [malformed\nwarning_at: 2026-01-02T00:00:00Z\n---\n",
    )
    .expect("malformed pending file should be written");

    let scanner = PendingWarningScanner::new(10).expect("warning threshold should be valid");
    let now = DateTime::parse_from_rfc3339("2026-01-03T00:00:00Z")
        .expect("timestamp should parse")
        .with_timezone(&Utc);

    let warnings = scanner
        .scan(std::slice::from_ref(&sandbox), now)
        .expect("scan should skip malformed pending files and continue");

    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].pending_path, healthy_pending_path);
    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}
