use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, Utc};
use domain::{
    DomainId, ExtractedSkillCandidate, ExtractionResult, LifecycleStatus, ScopeRoot, ScopeType,
    pending_default_expires_at, pending_default_warning_at,
};
use graph_builder::{SkillFileChangeKind, SkillWatcher};
use maintenance::{PendingWarningScanner, cleanup::ReproposalBlock};
use serde_yaml::Value;
use session_extractor::{
    ExtractSessionRequest,
    writer::{PendingDraftWriter, WriterError},
};

fn fresh_sandbox(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    let sandbox = std::env::temp_dir().join(format!("{prefix}-{nonce}"));
    std::fs::create_dir_all(&sandbox).expect("sandbox should be creatable");
    sandbox
}

fn sample_candidate(name: &str) -> ExtractedSkillCandidate {
    ExtractedSkillCandidate {
        name: name.to_owned(),
        description: "Reusable skill description".to_owned(),
        tags: vec!["rust".to_owned()],
        procedures: vec!["Step 1".to_owned()],
        conventions: vec!["Constrain scope".to_owned()],
        assets: vec!["docs/skill.md".to_owned()],
        confidence: 0.9,
        generality: None,
        generality_rationale: None,
        ..Default::default()
    }
}

fn frontmatter_from_markdown(markdown: &str) -> Value {
    let frontmatter = markdown
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map(|(frontmatter, _)| frontmatter)
        .expect("markdown should include yaml frontmatter");
    serde_yaml::from_str(frontmatter).expect("frontmatter should parse")
}

#[test]
fn pending_writer_emits_lifecycle_frontmatter_with_provenance() {
    let sandbox = fresh_sandbox("pending-lifecycle-writer");
    let writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);
    let extraction_result = ExtractionResult {
        source_session_id: DomainId::new_unchecked("session-123"),
        candidates: vec![sample_candidate("Rust File IO Setup")],
        provider: "claude".to_owned(),
        assessment: None,
    };

    let written_paths = writer
        .write_pending_drafts(
            &extraction_result,
            &ExtractSessionRequest {
                transcript_ref: "sample.jsonl".to_owned(),
                transcript_inline: None,
                session_id: "session-123".to_owned(),
                repo_path: None,
            },
            "claude",
        )
        .expect("pending writer should persist proposals");
    let pending_body =
        std::fs::read_to_string(&written_paths[0]).expect("written pending markdown should read");
    let frontmatter = frontmatter_from_markdown(&pending_body);

    assert_eq!(frontmatter["origin"], "session_extraction");
    assert_eq!(frontmatter["source_session_id"], "session-123");
    assert_eq!(frontmatter["source_provider"], "claude");
    let created_at = DateTime::parse_from_rfc3339(
        frontmatter["created_at"]
            .as_str()
            .expect("created_at should be serialized"),
    )
    .expect("created_at should be RFC3339");
    let warning_at = DateTime::parse_from_rfc3339(
        frontmatter["warning_at"]
            .as_str()
            .expect("warning_at should be serialized"),
    )
    .expect("warning_at should be RFC3339");
    let expires_at = DateTime::parse_from_rfc3339(
        frontmatter["expires_at"]
            .as_str()
            .expect("expires_at should be serialized"),
    )
    .expect("expires_at should be RFC3339");
    assert!(warning_at > created_at);
    assert!(expires_at > warning_at);
    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}

#[test]
fn pending_writer_uses_shared_lifecycle_policy_defaults() {
    let sandbox = fresh_sandbox("pending-lifecycle-shared-policy");
    let writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);
    let extraction_result = ExtractionResult {
        source_session_id: DomainId::new_unchecked("session-shared-policy"),
        candidates: vec![sample_candidate("Shared Lifecycle Policy Skill")],
        provider: "claude".to_owned(),
        assessment: None,
    };

    let written_paths = writer
        .write_pending_drafts(
            &extraction_result,
            &ExtractSessionRequest {
                transcript_ref: "sample.jsonl".to_owned(),
                transcript_inline: None,
                session_id: "session-shared-policy".to_owned(),
                repo_path: None,
            },
            "claude",
        )
        .expect("pending writer should persist proposals");
    let pending_body =
        std::fs::read_to_string(&written_paths[0]).expect("written pending markdown should read");
    let frontmatter = frontmatter_from_markdown(&pending_body);

    let created_at = DateTime::parse_from_rfc3339(
        frontmatter["created_at"]
            .as_str()
            .expect("created_at should be serialized"),
    )
    .expect("created_at should parse")
    .with_timezone(&Utc);
    let warning_at = DateTime::parse_from_rfc3339(
        frontmatter["warning_at"]
            .as_str()
            .expect("warning_at should be serialized"),
    )
    .expect("warning_at should parse")
    .with_timezone(&Utc);
    let expires_at = DateTime::parse_from_rfc3339(
        frontmatter["expires_at"]
            .as_str()
            .expect("expires_at should be serialized"),
    )
    .expect("expires_at should parse")
    .with_timezone(&Utc);

    assert_eq!(warning_at, pending_default_warning_at(created_at));
    assert_eq!(expires_at, pending_default_expires_at(created_at));
    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}

#[test]
fn cleanup_warning_scan_reports_stale_pending_without_deletion() {
    let sandbox = fresh_sandbox("pending-lifecycle-warning");
    let proposal_root = sandbox.join(".skills");
    std::fs::create_dir_all(&proposal_root).expect("proposal root should be created");
    let pending_path = proposal_root.join("stale/SKILL.md.pending");
    std::fs::create_dir_all(
        pending_path
            .parent()
            .expect("pending lifecycle test file should have parent"),
    )
    .expect("pending lifecycle proposal directory should exist");
    std::fs::write(
        &pending_path,
        "---\norigin: session_extraction\ncreated_at: 2026-01-01T00:00:00Z\nwarning_at: 2026-01-02T00:00:00Z\nexpires_at: 2026-04-01T00:00:00Z\nsource_session_id: session-123\n---\n",
    )
    .expect("pending proposal should be written");
    let scanner = PendingWarningScanner::new(30).expect("warning threshold should initialize");

    let warnings = scanner
        .scan(
            std::slice::from_ref(&sandbox),
            DateTime::parse_from_rfc3339("2026-02-15T00:00:00Z")
                .expect("timestamp should parse")
                .with_timezone(&Utc),
        )
        .expect("warning scan should succeed");

    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].pending_path, pending_path);
    assert!(
        pending_path.exists(),
        "warning scan must not delete pending files"
    );
    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}

#[test]
fn cleanup_warning_scan_reports_malformed_pending_without_aborting() {
    let sandbox = fresh_sandbox("pending-lifecycle-warning-malformed");
    let proposal_root = sandbox.join(".skills");
    std::fs::create_dir_all(&proposal_root).expect("proposal root should be created");
    let healthy_pending_path = proposal_root.join("healthy/SKILL.md.pending");
    let malformed_pending_path = proposal_root.join("malformed/SKILL.md.pending");
    std::fs::create_dir_all(
        healthy_pending_path
            .parent()
            .expect("healthy pending lifecycle test file should have parent"),
    )
    .expect("healthy pending lifecycle proposal directory should exist");
    std::fs::create_dir_all(
        malformed_pending_path
            .parent()
            .expect("malformed pending lifecycle test file should have parent"),
    )
    .expect("malformed pending lifecycle proposal directory should exist");
    std::fs::write(
        &healthy_pending_path,
        "---\norigin: session_extraction\ncreated_at: 2026-01-01T00:00:00Z\nwarning_at: 2026-01-02T00:00:00Z\nexpires_at: 2026-04-01T00:00:00Z\nsource_session_id: session-healthy\n---\n",
    )
    .expect("healthy pending proposal should be written");
    std::fs::write(
        &malformed_pending_path,
        "---\norigin: session_extraction\ncreated_at: [malformed\nwarning_at: 2026-01-02T00:00:00Z\n---\n",
    )
    .expect("malformed pending proposal should be written");
    let scanner = PendingWarningScanner::new(30).expect("warning threshold should initialize");

    let report = scanner
        .scan_with_diagnostics(
            std::slice::from_ref(&sandbox),
            DateTime::parse_from_rfc3339("2026-02-15T00:00:00Z")
                .expect("timestamp should parse")
                .with_timezone(&Utc),
        )
        .expect("warning scan should continue when one file is malformed");

    assert_eq!(report.warnings.len(), 1);
    assert_eq!(report.warnings[0].pending_path, healthy_pending_path);
    assert_eq!(report.malformed_pending_files.len(), 1);
    assert_eq!(
        report.malformed_pending_files[0].pending_path,
        malformed_pending_path
    );
    assert!(
        !report.malformed_pending_files[0].parse_error.is_empty(),
        "malformed diagnostics should include parse details"
    );

    let warnings_only = scanner
        .scan(
            std::slice::from_ref(&sandbox),
            DateTime::parse_from_rfc3339("2026-02-15T00:00:00Z")
                .expect("timestamp should parse")
                .with_timezone(&Utc),
        )
        .expect("legacy scan should also continue when one file is malformed");
    assert_eq!(warnings_only.len(), 1);
    assert_eq!(warnings_only[0].pending_path, healthy_pending_path);
    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}

#[test]
fn rejected_tombstone_blocks_reproposal_until_tombstone_pruning() {
    let sandbox = fresh_sandbox("pending-lifecycle-tombstone");
    let proposal_root = sandbox.join(".skills");
    std::fs::create_dir_all(&proposal_root).expect("proposal root should be created");
    let tombstone_path = proposal_root.join("rust-file-io-setup/SKILL.md.rejected");
    std::fs::create_dir_all(
        tombstone_path
            .parent()
            .expect("tombstone lifecycle test file should have parent"),
    )
    .expect("tombstone lifecycle proposal directory should exist");
    std::fs::write(
        &tombstone_path,
        "---\nis_tombstone: true\ncreated_at: 2026-01-01T00:00:00Z\norigin: manual\n---\n",
    )
    .expect("tombstone should be written");
    let writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);
    let extraction_result = ExtractionResult {
        source_session_id: DomainId::new_unchecked("session-456"),
        candidates: vec![sample_candidate("Rust File IO Setup")],
        provider: "claude".to_owned(),
        assessment: None,
    };

    let blocked_write = writer.write_pending_drafts(
        &extraction_result,
        &ExtractSessionRequest {
            transcript_ref: "sample.jsonl".to_owned(),
            transcript_inline: None,
            session_id: "session-456".to_owned(),
            repo_path: None,
        },
        "claude",
    );
    assert!(
        matches!(blocked_write, Err(WriterError::RejectedTombstonePresent(_))),
        "existing .rejected tombstone should fail closed and block immediate reproposal"
    );

    let scanner = PendingWarningScanner::new(30).expect("warning threshold should initialize");
    let active_block = scanner
        .reproposal_block(
            std::slice::from_ref(&sandbox),
            "rust-file-io-setup",
            DateTime::parse_from_rfc3339("2026-01-10T00:00:00Z")
                .expect("timestamp should parse")
                .with_timezone(&Utc),
            30,
        )
        .expect("reproposal guard should run");
    assert_eq!(
        active_block,
        Some(ReproposalBlock {
            tombstone_path: tombstone_path.clone(),
            age_days: 9,
        })
    );

    let pruned_tombstones = scanner
        .prune_expired_tombstones(
            std::slice::from_ref(&sandbox),
            DateTime::parse_from_rfc3339("2026-02-15T00:00:00Z")
                .expect("timestamp should parse")
                .with_timezone(&Utc),
            30,
        )
        .expect("pruning should succeed");
    assert_eq!(pruned_tombstones, vec![tombstone_path.clone()]);
    assert!(
        !tombstone_path.exists(),
        "expired tombstone should be pruned"
    );

    let unblocked_write = writer.write_pending_drafts(
        &extraction_result,
        &ExtractSessionRequest {
            transcript_ref: "sample.jsonl".to_owned(),
            transcript_inline: None,
            session_id: "session-456".to_owned(),
            repo_path: None,
        },
        "claude",
    );
    assert!(
        unblocked_write.is_ok(),
        "proposal should be writable again after tombstone pruning"
    );
    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}

#[test]
fn malformed_rejected_tombstone_is_pruned_and_unblocks_reproposal() {
    let sandbox = fresh_sandbox("pending-lifecycle-malformed-tombstone");
    let proposal_root = sandbox.join(".skills");
    std::fs::create_dir_all(&proposal_root).expect("proposal root should be created");
    let tombstone_path = proposal_root.join("rust-file-io-setup/SKILL.md.rejected");
    std::fs::create_dir_all(
        tombstone_path
            .parent()
            .expect("tombstone lifecycle test file should have parent"),
    )
    .expect("tombstone lifecycle proposal directory should exist");
    std::fs::write(
        &tombstone_path,
        "---\ncreated_at: [malformed\norigin: manual\n---\n",
    )
    .expect("malformed tombstone should be written");
    let writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);
    let extraction_result = ExtractionResult {
        source_session_id: DomainId::new_unchecked("session-789"),
        candidates: vec![sample_candidate("Rust File IO Setup")],
        provider: "claude".to_owned(),
        assessment: None,
    };

    let blocked_write = writer.write_pending_drafts(
        &extraction_result,
        &ExtractSessionRequest {
            transcript_ref: "sample.jsonl".to_owned(),
            transcript_inline: None,
            session_id: "session-789".to_owned(),
            repo_path: None,
        },
        "claude",
    );
    assert!(
        matches!(blocked_write, Err(WriterError::RejectedTombstonePresent(_))),
        "writer should fail closed when rejected tombstone file exists"
    );

    let scanner = PendingWarningScanner::new(30).expect("warning threshold should initialize");
    let pruned_tombstones = scanner
        .prune_expired_tombstones(
            std::slice::from_ref(&sandbox),
            Utc::now() + chrono::Duration::days(2),
            1,
        )
        .expect("malformed tombstone should still be pruneable");
    assert_eq!(pruned_tombstones, vec![tombstone_path.clone()]);

    let unblocked_write = writer.write_pending_drafts(
        &extraction_result,
        &ExtractSessionRequest {
            transcript_ref: "sample.jsonl".to_owned(),
            transcript_inline: None,
            session_id: "session-789".to_owned(),
            repo_path: None,
        },
        "claude",
    );
    assert!(
        unblocked_write.is_ok(),
        "reproposal should become writable once malformed tombstone is pruned"
    );
    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}

#[test]
fn watcher_classifies_approval_and_retirement_renames_for_auditability() {
    let sandbox = fresh_sandbox("pending-lifecycle-watcher");
    let scope_root = sandbox.join("project");
    let skill_root = scope_root.join("rename-skill");
    std::fs::create_dir_all(&skill_root).expect("skill root should exist");
    let scopes = vec![ScopeRoot::new(
        "project",
        ScopeType::Project,
        scope_root.clone(),
    )];
    let mut watcher = SkillWatcher::new(scopes).expect("watcher should initialize");

    let pending_path = skill_root.join("SKILL.md.pending");
    std::fs::write(&pending_path, "# rename-skill\n\npending body").expect("pending file writes");
    let _ = watcher
        .collect_file_changes()
        .expect("pending create should be observed");

    let active_path = skill_root.join("SKILL.md");
    std::fs::rename(&pending_path, &active_path).expect("pending should rename to active");
    let approval_changes = watcher
        .collect_file_changes()
        .expect("approval rename should be detected");
    assert!(approval_changes.iter().any(|change| {
        change.file_path == active_path && change.kind == SkillFileChangeKind::ApprovedRename
    }));

    let retired_path = skill_root.join("SKILL.md.retired");
    std::fs::rename(&active_path, &retired_path).expect("active should rename to retired");
    let retirement_changes = watcher
        .collect_file_changes()
        .expect("retired rename should be detected");
    assert!(retirement_changes.iter().any(|change| {
        change.file_path == retired_path && change.kind == SkillFileChangeKind::RetiredRename
    }));
    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}

#[test]
fn watcher_matches_duplicate_content_renames_by_skill_directory() {
    let sandbox = fresh_sandbox("pending-lifecycle-duplicate-content");
    let scope_root = sandbox.join("project");
    let alpha_skill_root = scope_root.join("alpha-skill");
    let beta_skill_root = scope_root.join("beta-skill");
    std::fs::create_dir_all(&alpha_skill_root).expect("alpha skill root should exist");
    std::fs::create_dir_all(&beta_skill_root).expect("beta skill root should exist");
    let mut watcher = SkillWatcher::new(vec![ScopeRoot::new(
        "project",
        ScopeType::Project,
        scope_root,
    )])
    .expect("watcher should initialize");

    let duplicate_pending_body = "# duplicate-skill\n\nsame content hash";
    let alpha_pending_path = alpha_skill_root.join("SKILL.md.pending");
    let beta_pending_path = beta_skill_root.join("SKILL.md.pending");
    std::fs::write(&alpha_pending_path, duplicate_pending_body)
        .expect("alpha pending should write");
    std::fs::write(&beta_pending_path, duplicate_pending_body).expect("beta pending should write");
    let _ = watcher
        .collect_file_changes()
        .expect("pending creates should be observed");

    let alpha_active_path = alpha_skill_root.join("SKILL.md");
    let beta_rejected_path = beta_skill_root.join("SKILL.md.rejected");
    std::fs::rename(&alpha_pending_path, &alpha_active_path).expect("alpha should approve");
    std::fs::rename(&beta_pending_path, &beta_rejected_path).expect("beta should reject");
    let rename_changes = watcher
        .collect_file_changes()
        .expect("duplicate-content renames should be classified");

    assert!(rename_changes.iter().any(|change| {
        change.file_path == alpha_active_path && change.kind == SkillFileChangeKind::ApprovedRename
    }));
    assert!(rename_changes.iter().any(|change| {
        change.file_path == beta_rejected_path && change.kind == SkillFileChangeKind::RejectedRename
    }));
    assert!(
        !rename_changes.iter().any(|change| {
            (change.file_path == alpha_active_path || change.file_path == beta_rejected_path)
                && change.kind == SkillFileChangeKind::Created
        }),
        "renamed lifecycle files should not be emitted as plain created events",
    );

    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}

#[test]
fn lifecycle_vocabulary_covers_pending_contract_states() {
    let lifecycle_states = [
        LifecycleStatus::Draft,
        LifecycleStatus::Active,
        LifecycleStatus::Retired,
        LifecycleStatus::Rejected,
        LifecycleStatus::Deleted,
    ];
    assert_eq!(lifecycle_states.len(), 5);
}
