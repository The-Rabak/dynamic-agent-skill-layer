use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use domain::{DomainId, ExtractedSkillCandidate, ExtractionResult};
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

fn request_for(session_id: &str) -> ExtractSessionRequest {
    ExtractSessionRequest {
        transcript_ref: "sample.jsonl".to_owned(),
        transcript_inline: None,
        session_id: session_id.to_owned(),
        repo_path: None,
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
fn pending_frontmatter_includes_created_warning_and_expiry_timestamps() {
    let sandbox = fresh_sandbox("pending-frontmatter-contract");
    let writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);
    let extraction_result = ExtractionResult {
        source_session_id: DomainId::new_unchecked("session-123"),
        candidates: vec![sample_candidate("Rust File IO Setup")],
        provider: "claude".to_owned(),
        assessment: None,
    };

    let written_paths = writer
        .write_pending_drafts(&extraction_result, &request_for("session-123"), "claude")
        .expect("writer should persist pending proposal");
    let pending_body =
        std::fs::read_to_string(&written_paths[0]).expect("pending proposal should be readable");

    assert!(
        pending_body.contains("created_at:"),
        "pending proposal must include created_at timestamp"
    );
    assert!(
        pending_body.contains("warning_at:"),
        "pending proposal must include warning_at timestamp"
    );
    assert!(
        pending_body.contains("expires_at:"),
        "pending proposal must include expires_at timestamp"
    );
    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}

#[test]
fn pending_writer_batch_failure_does_not_leave_partial_drafts() {
    let sandbox = fresh_sandbox("pending-frontmatter-atomicity");
    let proposal_root = sandbox.join(".skills");
    std::fs::create_dir_all(&proposal_root).expect("proposal root should be created");

    let blocked_tombstone = proposal_root.join("blocked-skill/SKILL.md.rejected");
    std::fs::create_dir_all(
        blocked_tombstone
            .parent()
            .expect("blocked tombstone should have proposal parent"),
    )
    .expect("blocked proposal directory should exist");
    std::fs::write(
        &blocked_tombstone,
        "---\nis_tombstone: true\ncreated_at: 2026-01-01T00:00:00Z\norigin: manual\n---\n",
    )
    .expect("blocked tombstone should be written");

    let writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);
    let extraction_result = ExtractionResult {
        source_session_id: DomainId::new_unchecked("session-atomic"),
        candidates: vec![
            sample_candidate("Allowed Skill"),
            sample_candidate("Blocked Skill"),
        ],
        provider: "claude".to_owned(),
        assessment: None,
    };

    let write_result =
        writer.write_pending_drafts(&extraction_result, &request_for("session-atomic"), "claude");
    assert!(
        matches!(write_result, Err(WriterError::RejectedTombstonePresent(_))),
        "writer should fail closed when one candidate is blocked by a rejected tombstone"
    );
    assert!(
        !proposal_root
            .join("allowed-skill/SKILL.md.pending")
            .exists(),
        "candidate batch failures must not leave partial pending files behind"
    );
    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}

#[test]
fn pending_frontmatter_serializes_multiline_and_special_characters_safely() {
    let sandbox = fresh_sandbox("pending-frontmatter-escaping");
    let writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);
    let extraction_result = ExtractionResult {
        source_session_id: DomainId::new_unchecked("session-frontmatter"),
        candidates: vec![ExtractedSkillCandidate {
            name: "YAML Safety Skill".to_owned(),
            description: "Line one\nsource_provider: injected\nLine \"quoted\" content".to_owned(),
            tags: vec![
                "security".to_owned(),
                "quoted \"tag\"".to_owned(),
                "yaml:colon".to_owned(),
            ],
            procedures: vec!["Handle multiline safely".to_owned()],
            conventions: vec!["Fail closed".to_owned()],
            assets: vec!["docs/yaml.md".to_owned()],
            confidence: 0.88,
            generality: None,
            generality_rationale: None,
            ..Default::default()
        }],
        provider: "claude".to_owned(),
        assessment: None,
    };

    let written_paths = writer
        .write_pending_drafts(
            &extraction_result,
            &request_for("session-frontmatter"),
            "claude",
        )
        .expect("writer should persist proposal with escaped frontmatter");
    let pending_body =
        std::fs::read_to_string(&written_paths[0]).expect("pending proposal should be readable");
    let frontmatter = frontmatter_from_markdown(&pending_body);

    assert_eq!(
        frontmatter["description"],
        Value::String("Line one\nsource_provider: injected\nLine \"quoted\" content".to_owned())
    );
    assert_eq!(frontmatter["source_provider"], "claude");
    assert_eq!(
        frontmatter["tags"],
        Value::Sequence(vec![
            Value::String("security".to_owned()),
            Value::String("quoted \"tag\"".to_owned()),
            Value::String("yaml:colon".to_owned()),
        ])
    );
    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}
