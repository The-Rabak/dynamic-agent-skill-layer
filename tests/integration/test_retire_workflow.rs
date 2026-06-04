use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::SystemTime,
};

use chrono::{Duration, TimeZone, Utc};
use domain::{ScopeType, ScopeType::Project};
use graph_builder::{
    ScopeRoot,
    graph::build::build_skills_from_scope_roots,
    watcher::build_snapshot,
};
use maintenance::{
    MaintenanceAuditError, MaintenanceAuditEvent, MaintenanceAuditSink, RetirementConfig,
    RetirementProposalWriter, SkillSnapshot, UsageSample,
};

#[derive(Clone, Default)]
struct RecordingMaintenanceAuditSink {
    events: Arc<Mutex<Vec<MaintenanceAuditEvent>>>,
}

impl RecordingMaintenanceAuditSink {
    fn emitted_events(&self) -> Vec<MaintenanceAuditEvent> {
        self.events
            .lock()
            .expect("audit event storage should be lockable")
            .clone()
    }
}

impl MaintenanceAuditSink for RecordingMaintenanceAuditSink {
    fn emit(&self, event: MaintenanceAuditEvent) -> Result<(), MaintenanceAuditError> {
        self.events
            .lock()
            .expect("audit event storage should be lockable")
            .push(event);
        Ok(())
    }
}

fn fresh_sandbox(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let sandbox = std::env::temp_dir().join(format!("{prefix}-{nonce}"));
    std::fs::create_dir_all(&sandbox).expect("sandbox should be created");
    sandbox
}

fn write_active_skill(path: &PathBuf, name: &str) {
    std::fs::create_dir_all(path.parent().expect("skill path should have parent"))
        .expect("skill directory should be created");
    std::fs::write(
        path,
        format!("# {name}\n\n{name} description\n\n## Procedures\n- {name} step"),
    )
    .expect("skill file should be written");
}

fn skill_snapshot(skill_id: &str, source_path: PathBuf) -> SkillSnapshot {
    SkillSnapshot {
        id: skill_id.to_owned(),
        name: skill_id.to_owned(),
        description: format!("{skill_id} description"),
        scope: ScopeType::Project,
        source_path,
        tags: vec!["maintenance".to_owned()],
        subunits: vec!["step".to_owned()],
        embedding: vec![1.0, 0.0, 0.0],
    }
}

#[test]
fn retirement_workflow_creates_non_destructive_retired_proposal_marker() {
    let sandbox = fresh_sandbox("retire-workflow");
    let stale_skill_path = sandbox.join("project/stale/SKILL.md");
    let healthy_skill_path = sandbox.join("project/healthy/SKILL.md");
    write_active_skill(&stale_skill_path, "stale-skill");
    write_active_skill(&healthy_skill_path, "healthy-skill");

    let stale_skill = skill_snapshot("stale-skill", stale_skill_path.clone());
    let healthy_skill = skill_snapshot("healthy-skill", healthy_skill_path.clone());
    let audit_sink = RecordingMaintenanceAuditSink::default();
    let writer =
        RetirementProposalWriter::with_audit_sink(RetirementConfig::default(), &audit_sink);
    let now = Utc
        .with_ymd_and_hms(2026, 5, 26, 13, 14, 15)
        .single()
        .expect("valid fixed audit timestamp");

    let retirement_proposals = writer
        .propose(
            &[stale_skill.clone(), healthy_skill.clone()],
            &[UsageSample {
                skill_id: "healthy-skill".to_owned(),
                used_at: now - Duration::days(2),
                usage_count: 15,
            }],
            now,
        )
        .expect("retirement workflow should run");

    assert_eq!(retirement_proposals.len(), 1);
    let proposal = &retirement_proposals[0];
    assert_eq!(proposal.skill_id, "stale-skill");
    assert!(proposal.retired_path.exists());
    assert!(stale_skill_path.exists());
    assert_eq!(
        proposal
            .retired_path
            .file_name()
            .and_then(std::ffi::OsStr::to_str),
        Some("SKILL.md.retired")
    );
    assert!(healthy_skill_path.exists());

    let scope_root = ScopeRoot::new("project", Project, sandbox.join("project"));
    let snapshot = build_snapshot(std::slice::from_ref(&scope_root))
        .expect("watcher snapshot should include active and retired files");
    assert!(
        snapshot.contains_key(&proposal.retired_path),
        "retired artifacts remain filesystem-observable"
    );

    let embedder = graph_builder::graph::embeddings::DeterministicEmbeddingService::default();
    let active_skills = tokio::runtime::Runtime::new()
        .expect("tokio runtime should build")
        .block_on(build_skills_from_scope_roots(
            std::slice::from_ref(&scope_root),
            &embedder,
        ))
        .expect("graph build should process active skills");
    let mut active_names = active_skills
        .iter()
        .map(|skill| skill.name.as_str())
        .collect::<Vec<_>>();
    active_names.sort_unstable();
    assert_eq!(active_names, vec!["healthy-skill"]);

    let emitted_events = audit_sink.emitted_events();
    assert_eq!(emitted_events.len(), 1);
    let maintenance::MaintenanceAuditEvent::RetirementProposalWritten(event) = &emitted_events[0]
    else {
        panic!("expected retirement proposal audit event");
    };
    assert_eq!(
        event.correlation_id,
        "maintenance.retirement_proposal:stale-skill"
    );
    assert_eq!(event.happened_at, now);
    assert_eq!(event.skill_id, "stale-skill");
    assert_eq!(event.source_path, stale_skill_path);
    assert_eq!(event.proposal_path, proposal.retired_path);
    assert_eq!(event.usage_score_per_month, proposal.usage_score_per_month);
}

#[test]
fn retirement_workflow_rejects_non_skill_filename_paths() {
    let sandbox = fresh_sandbox("retire-workflow-invalid-source-name");
    let invalid_skill_path = sandbox.join("project/stale/not-a-skill.md");
    write_active_skill(&invalid_skill_path, "stale-skill");
    let stale_skill = skill_snapshot("stale-skill", invalid_skill_path);
    let writer = RetirementProposalWriter::new(RetirementConfig::default());

    let result = writer.propose(&[stale_skill], &[], Utc::now());

    assert!(
        matches!(
            result,
            Err(maintenance::RetirementError::InvalidActiveSkillPath(_))
        ),
        "retirement writer should fail closed when source path is not SKILL.md"
    );
}

#[test]
#[cfg(unix)]
fn retirement_workflow_rejects_existing_retired_symlink_without_clobbering_target() {
    let sandbox = fresh_sandbox("retire-workflow-symlink-clobber");
    let stale_skill_path = sandbox.join("project/stale/SKILL.md");
    let outside_target = sandbox.join("outside-target.retired");
    write_active_skill(&stale_skill_path, "stale-skill");
    std::fs::write(&outside_target, "preserve-me").expect("outside target should be seeded");

    let retired_marker_path = stale_skill_path.with_file_name("SKILL.md.retired");
    std::os::unix::fs::symlink(&outside_target, &retired_marker_path)
        .expect("retired marker symlink should be created");

    let writer = RetirementProposalWriter::new(RetirementConfig::default());
    let result = writer.propose(
        &[skill_snapshot("stale-skill", stale_skill_path)],
        &[],
        Utc::now(),
    );

    assert!(
        matches!(
            result,
            Err(maintenance::RetirementError::RetiredPathOutsideSkillRoot { .. })
        ),
        "existing symlink at retired marker path should be rejected"
    );
    let preserved_target =
        std::fs::read_to_string(&outside_target).expect("outside target should still be readable");
    assert_eq!(
        preserved_target, "preserve-me",
        "retirement writer must not clobber symlink target content"
    );
}
