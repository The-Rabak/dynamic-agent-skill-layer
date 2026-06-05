use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::SystemTime,
};

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use domain::{ScopeType, pending_default_expires_at, pending_default_warning_at};
use maintenance::{
    MaintenanceAuditError, MaintenanceAuditEvent, MaintenanceAuditSink, MergeConfig,
    MergeProposalWriter, MergeSemanticVerifier, NoopMaintenanceAuditSink, SeededSkillProjection,
    SkillSnapshot,
};
use serde_yaml::Value;

#[derive(Clone)]
struct EquivalentSemanticVerifier;

#[async_trait]
impl MergeSemanticVerifier for EquivalentSemanticVerifier {
    async fn are_equivalent(
        &self,
        _left: &SkillSnapshot,
        _right: &SkillSnapshot,
    ) -> Result<bool, maintenance::MergeError> {
        Ok(true)
    }
}

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

fn seeded_skill_projection(
    skill_id: &str,
    name: &str,
    scope: ScopeType,
    source_path: PathBuf,
    embedding: Vec<f32>,
) -> SeededSkillProjection {
    std::fs::create_dir_all(
        source_path
            .parent()
            .expect("source path for seeded skill should have parent"),
    )
    .expect("source directory should be created");
    std::fs::write(&source_path, format!("# {name}\n\n{name} description"))
        .expect("source skill file should be written");

    SeededSkillProjection {
        skill_id: skill_id.to_owned(),
        skill_name: name.to_owned(),
        skill_description: format!("{name} description"),
        scope,
        source_paths: vec![source_path],
        tags: vec!["rust".to_owned(), "auth".to_owned()],
        subunit_contents: vec!["Investigate auth workflow".to_owned()],
        embedding,
    }
}

fn frontmatter_from_pending_markdown(pending_markdown: &str) -> Value {
    let frontmatter_boundary = "\n---\n";
    let frontmatter_section = pending_markdown
        .strip_prefix("---\n")
        .and_then(|remaining| remaining.split_once(frontmatter_boundary))
        .map(|(frontmatter, _)| frontmatter)
        .expect("pending proposal should start with YAML frontmatter");
    serde_yaml::from_str(frontmatter_section).expect("frontmatter should be valid YAML")
}

#[tokio::test]
async fn merge_workflow_writes_pending_proposal_with_scope_provenance_and_human_gate() {
    let sandbox = fresh_sandbox("merge-workflow");
    let project_skill_path = sandbox.join("project/auth/SKILL.md");
    let global_skill_path = sandbox.join("global/auth/SKILL.md");
    let seeded_project = seeded_skill_projection(
        "skill-project-auth",
        "project-auth-flow",
        ScopeType::Project,
        project_skill_path.clone(),
        vec![1.0, 1.0, 0.0],
    );
    let seeded_global = seeded_skill_projection(
        "skill-global-auth",
        "global-auth-patterns",
        ScopeType::Global,
        global_skill_path.clone(),
        vec![0.9, 1.0, 0.0],
    );
    let snapshots = vec![
        SkillSnapshot::from_seeded_skill_projection(seeded_project),
        SkillSnapshot::from_seeded_skill_projection(seeded_global),
    ];
    let audit_sink = RecordingMaintenanceAuditSink::default();
    let writer = MergeProposalWriter::with_audit_sink(
        MergeConfig::default(),
        EquivalentSemanticVerifier,
        &audit_sink,
    );
    let now = Utc
        .with_ymd_and_hms(2026, 5, 26, 12, 34, 56)
        .single()
        .expect("valid fixed audit timestamp");

    let proposals = writer
        .propose(&snapshots, now)
        .await
        .expect("merge workflow should propose duplicate merge");

    assert_eq!(proposals.len(), 1);
    let proposal = &proposals[0];
    assert!(proposal.pending_path.exists());
    let proposal_directory_name = proposal
        .pending_path
        .parent()
        .expect("proposal path should include a parent directory")
        .file_name()
        .expect("proposal directory should include a name")
        .to_string_lossy();
    assert!(
        proposal_directory_name.contains("skill-project-auth")
            && proposal_directory_name.contains("skill-global-auth"),
        "proposal directory name should retain source skill IDs for traceability"
    );
    assert_eq!(proposal.canonical_scope, ScopeType::Project);
    assert_eq!(
        proposal.merged_from_scopes,
        vec![ScopeType::Project, ScopeType::Global]
    );
    assert!(
        proposal
            .pending_path
            .starts_with(sandbox.join("project/auth/.skills")),
        "project/global merge should write proposal under project root to match canonical scope"
    );
    let pending_body = std::fs::read_to_string(&proposal.pending_path)
        .expect("pending proposal should be readable");
    let frontmatter = frontmatter_from_pending_markdown(&pending_body);
    assert_eq!(frontmatter["origin"], "merge_proposal");
    assert_eq!(frontmatter["canonical_scope"], "project");
    assert_eq!(
        frontmatter["merged_from_scopes"],
        Value::Sequence(vec![Value::from("project"), Value::from("global")])
    );
    let created_at = chrono::DateTime::parse_from_rfc3339(
        frontmatter["created_at"]
            .as_str()
            .expect("created_at should be serialized"),
    )
    .expect("created_at should parse")
    .with_timezone(&Utc);
    let warning_at = chrono::DateTime::parse_from_rfc3339(
        frontmatter["warning_at"]
            .as_str()
            .expect("warning_at should be serialized"),
    )
    .expect("warning_at should parse")
    .with_timezone(&Utc);
    let expires_at = chrono::DateTime::parse_from_rfc3339(
        frontmatter["expires_at"]
            .as_str()
            .expect("expires_at should be serialized"),
    )
    .expect("expires_at should parse")
    .with_timezone(&Utc);
    assert_eq!(warning_at, pending_default_warning_at(created_at));
    assert_eq!(expires_at, pending_default_expires_at(created_at));

    let emitted_events = audit_sink.emitted_events();
    assert_eq!(emitted_events.len(), 1);
    let maintenance::MaintenanceAuditEvent::MergeProposalWritten(event) = &emitted_events[0] else {
        panic!("expected merge proposal audit event");
    };
    assert_eq!(
        event.correlation_id,
        "maintenance.merge_proposal:skill-global-auth:skill-project-auth"
    );
    assert_eq!(event.happened_at, now);
    assert_eq!(event.proposal_path, proposal.pending_path);
    assert_eq!(event.canonical_scope, ScopeType::Project);
    assert_eq!(
        event.merged_from_skill_ids,
        vec![
            "skill-global-auth".to_owned(),
            "skill-project-auth".to_owned()
        ]
    );
    assert_eq!(
        event.merged_from_scopes,
        vec![ScopeType::Project, ScopeType::Global]
    );
    assert_eq!(event.merged_from_paths, proposal.merged_from_paths);

    assert!(
        project_skill_path.exists(),
        "source project skill remains active until human approval"
    );
    assert!(
        global_skill_path.exists(),
        "source global skill remains active until human approval"
    );
}

#[tokio::test]
async fn merge_workflow_serializes_frontmatter_with_special_characters_and_newlines() {
    let sandbox = fresh_sandbox("merge-workflow-frontmatter-escaping");
    let project_skill_path = sandbox.join("project/auth/SKILL.md");
    let global_skill_path = sandbox.join("global/auth/SKILL.md");
    let seeded_project = seeded_skill_projection(
        "skill-project-auth",
        "project:auth\n\"flow\"",
        ScopeType::Project,
        project_skill_path.clone(),
        vec![1.0, 1.0, 0.0],
    );
    let seeded_global = seeded_skill_projection(
        "skill-global-auth",
        "global auth: [patterns]",
        ScopeType::Global,
        global_skill_path.clone(),
        vec![0.9, 1.0, 0.0],
    );
    let snapshots = vec![
        SkillSnapshot::from_seeded_skill_projection(seeded_project),
        SkillSnapshot::from_seeded_skill_projection(seeded_global),
    ];
    let writer = MergeProposalWriter::with_audit_sink(MergeConfig::default(), EquivalentSemanticVerifier, &NoopMaintenanceAuditSink);

    let proposals = writer
        .propose(&snapshots, Utc::now())
        .await
        .expect("merge workflow should propose duplicate merge");

    let pending_body = std::fs::read_to_string(&proposals[0].pending_path)
        .expect("pending proposal should be readable");
    let frontmatter = frontmatter_from_pending_markdown(&pending_body);

    let merged_name = frontmatter["name"]
        .as_str()
        .expect("merged name should be serialized as string");
    assert!(merged_name.contains("project:auth\n\"flow\""));
    assert!(merged_name.contains("global auth: [patterns]"));
    let merged_description = frontmatter["description"]
        .as_str()
        .expect("merged description should be serialized as string");
    assert!(merged_description.contains("project:auth\n\"flow\" description"));
    assert!(merged_description.contains("global auth: [patterns] description"));
    let merged_from_paths = frontmatter["merged_from"]
        .as_sequence()
        .expect("merged_from should be serialized as a sequence")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    let project_path = project_skill_path.display().to_string();
    let global_path = global_skill_path.display().to_string();
    assert!(merged_from_paths.contains(&project_path.as_str()));
    assert!(merged_from_paths.contains(&global_path.as_str()));
}

#[tokio::test]
async fn merge_workflow_writes_global_scoped_proposal_under_global_root_for_team_global_pair() {
    let sandbox = fresh_sandbox("merge-workflow-team-global");
    let team_skill_path = sandbox.join("team/auth/SKILL.md");
    let global_skill_path = sandbox.join("global/auth/SKILL.md");
    let seeded_team = seeded_skill_projection(
        "a-skill-team-auth",
        "team-auth-flow",
        ScopeType::Team,
        team_skill_path,
        vec![1.0, 1.0, 0.0],
    );
    let seeded_global = seeded_skill_projection(
        "z-skill-global-auth",
        "global-auth-patterns",
        ScopeType::Global,
        global_skill_path,
        vec![0.9, 1.0, 0.0],
    );
    let snapshots = vec![
        SkillSnapshot::from_seeded_skill_projection(seeded_team),
        SkillSnapshot::from_seeded_skill_projection(seeded_global),
    ];
    let writer = MergeProposalWriter::with_audit_sink(MergeConfig::default(), EquivalentSemanticVerifier, &NoopMaintenanceAuditSink);

    let proposals = writer
        .propose(&snapshots, Utc::now())
        .await
        .expect("merge workflow should propose duplicate merge");

    assert_eq!(proposals.len(), 1);
    let proposal = &proposals[0];
    assert_eq!(proposal.canonical_scope, ScopeType::Global);
    assert!(
        proposal
            .pending_path
            .starts_with(sandbox.join("global/auth/.skills")),
        "team/global merge should write proposal under global root to match canonical scope"
    );
}

#[tokio::test]
async fn merge_workflow_rejects_filename_collision_without_overwriting_existing_proposal() {
    let sandbox = fresh_sandbox("merge-workflow-collision");
    let project_skill_path = sandbox.join("project/auth/SKILL.md");
    let global_skill_path = sandbox.join("global/auth/SKILL.md");
    let seeded_project = seeded_skill_projection(
        "skill-project-auth",
        "project-auth-flow",
        ScopeType::Project,
        project_skill_path,
        vec![1.0, 1.0, 0.0],
    );
    let seeded_global = seeded_skill_projection(
        "skill-global-auth",
        "global-auth-patterns",
        ScopeType::Global,
        global_skill_path,
        vec![0.9, 1.0, 0.0],
    );
    let snapshots = vec![
        SkillSnapshot::from_seeded_skill_projection(seeded_project),
        SkillSnapshot::from_seeded_skill_projection(seeded_global),
    ];
    let writer = MergeProposalWriter::with_audit_sink(MergeConfig::default(), EquivalentSemanticVerifier, &NoopMaintenanceAuditSink);
    let now = Utc::now();

    let first_proposals = writer
        .propose(&snapshots, now)
        .await
        .expect("first proposal generation should succeed");
    let pending_path = first_proposals[0].pending_path.clone();
    std::fs::write(&pending_path, "human-note: keep this review state")
        .expect("human annotation should be written");

    let second_attempt = writer.propose(&snapshots, now).await;

    assert!(
        matches!(
            second_attempt,
            Err(maintenance::MergeError::WriteFailure { .. })
        ),
        "second proposal generation should fail with explicit write error instead of overwriting"
    );
    let preserved_content =
        std::fs::read_to_string(&pending_path).expect("proposal file should remain readable");
    assert_eq!(
        preserved_content, "human-note: keep this review state",
        "existing proposal content should be preserved on filename collision"
    );
}

#[tokio::test]
async fn merge_workflow_rejects_unsafe_pending_directory_component() {
    let sandbox = fresh_sandbox("merge-workflow-unsafe-pending-dir");
    let project_skill_path = sandbox.join("project/auth/SKILL.md");
    let global_skill_path = sandbox.join("global/auth/SKILL.md");
    let seeded_project = seeded_skill_projection(
        "skill-project-auth",
        "project-auth-flow",
        ScopeType::Project,
        project_skill_path,
        vec![1.0, 1.0, 0.0],
    );
    let seeded_global = seeded_skill_projection(
        "skill-global-auth",
        "global-auth-patterns",
        ScopeType::Global,
        global_skill_path,
        vec![0.9, 1.0, 0.0],
    );
    let snapshots = vec![
        SkillSnapshot::from_seeded_skill_projection(seeded_project),
        SkillSnapshot::from_seeded_skill_projection(seeded_global),
    ];
    let writer = MergeProposalWriter::with_audit_sink(
        MergeConfig {
            pending_directory_name: "../escape".to_owned(),
            ..MergeConfig::default()
        },
        EquivalentSemanticVerifier,
        &NoopMaintenanceAuditSink,
    );

    let result = writer.propose(&snapshots, Utc::now()).await;

    assert!(
        matches!(
            result,
            Err(maintenance::MergeError::InvalidPendingDirectoryName(_))
        ),
        "relative traversal component in pending directory name should be rejected"
    );
}

#[tokio::test]
async fn merge_workflow_handles_high_cardinality_input_without_lookup_regressions() {
    let sandbox = fresh_sandbox("merge-workflow-high-cardinality");
    let pair_count = 24usize;
    let embedding_dims = pair_count;
    let mut snapshots = Vec::with_capacity(pair_count * 2);

    for index in 0..pair_count {
        let mut embedding = vec![0.0_f32; embedding_dims];
        embedding[index] = 1.0;

        let project_projection = seeded_skill_projection(
            &format!("skill-project-{index}"),
            &format!("project-skill-{index}"),
            ScopeType::Project,
            sandbox.join(format!("project/skill-{index}/SKILL.md")),
            embedding.clone(),
        );
        let global_projection = seeded_skill_projection(
            &format!("skill-global-{index}"),
            &format!("global-skill-{index}"),
            ScopeType::Global,
            sandbox.join(format!("global/skill-{index}/SKILL.md")),
            embedding,
        );
        snapshots.push(SkillSnapshot::from_seeded_skill_projection(
            project_projection,
        ));
        snapshots.push(SkillSnapshot::from_seeded_skill_projection(
            global_projection,
        ));
    }

    let writer = MergeProposalWriter::with_audit_sink(MergeConfig::default(), EquivalentSemanticVerifier, &NoopMaintenanceAuditSink);
    let proposals = writer
        .propose(&snapshots, Utc::now())
        .await
        .expect("high-cardinality proposal generation should succeed");

    assert_eq!(
        proposals.len(),
        pair_count,
        "one proposal per matching project/global embedding should be generated"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn merge_workflow_rejects_symlinked_pending_root_that_escapes_scope_root() {
    let sandbox = fresh_sandbox("merge-workflow-symlink-escape");
    let project_skill_path = sandbox.join("project/auth/SKILL.md");
    let global_skill_path = sandbox.join("global/auth/SKILL.md");
    let outside_root = sandbox.join("outside");
    std::fs::create_dir_all(&outside_root).expect("outside root should be created");

    let seeded_project = seeded_skill_projection(
        "skill-project-auth",
        "project-auth-flow",
        ScopeType::Project,
        project_skill_path.clone(),
        vec![1.0, 1.0, 0.0],
    );
    let seeded_global = seeded_skill_projection(
        "skill-global-auth",
        "global-auth-patterns",
        ScopeType::Global,
        global_skill_path,
        vec![0.9, 1.0, 0.0],
    );
    let snapshots = vec![
        SkillSnapshot::from_seeded_skill_projection(seeded_project),
        SkillSnapshot::from_seeded_skill_projection(seeded_global),
    ];

    let project_root = project_skill_path
        .parent()
        .expect("project skill should have parent");
    let pending_symlink_path = project_root.join(".skills");
    std::os::unix::fs::symlink(&outside_root, &pending_symlink_path)
        .expect("pending symlink should be created");

    let writer = MergeProposalWriter::with_audit_sink(MergeConfig::default(), EquivalentSemanticVerifier, &NoopMaintenanceAuditSink);
    let result = writer.propose(&snapshots, Utc::now()).await;

    assert!(
        matches!(
            result,
            Err(maintenance::MergeError::WritePathOutsideScopeRoot { .. })
        ),
        "pending symlink that resolves outside project root should be rejected"
    );
}

/// Proves `LiveMergePassRunner` implements `MergePassRunner` both by value and trait object.
///
/// The inner functions are intentionally defined but not called — their existence is
/// the compile-time proof that the trait bounds are satisfied.
#[test]
fn live_merge_runner_exists_and_implements_merge_pass_runner_trait() {
    use maintenance::cron::MergePassRunner;
    // Import is the proof that the type is publicly exported.
    let _: Option<maintenance::LiveMergePassRunner> = None;

    #[allow(dead_code)]
    fn assert_merge_runner(_r: &impl MergePassRunner) {}
    #[allow(dead_code)]
    fn assert_merge_runner_object_safe(_r: &dyn MergePassRunner) {}
}

/// Proves `LiveRetirementPassRunner` implements `RetirementPassRunner` both by value and
/// trait object.
#[test]
fn live_retirement_runner_exists_and_implements_retirement_pass_runner_trait() {
    use maintenance::cron::RetirementPassRunner;
    // Import is the proof that the type is publicly exported.
    let _: Option<maintenance::LiveRetirementPassRunner> = None;

    #[allow(dead_code)]
    fn assert_retirement_runner(_r: &impl RetirementPassRunner) {}
    #[allow(dead_code)]
    fn assert_retirement_runner_object_safe(_r: &dyn RetirementPassRunner) {}
}

#[test]
fn maintenance_runtime_noop_runners_dont_compromise_trait_existence() {
    use maintenance::cron::{MergePassRunner, RetirementPassRunner};
    use maintenance::{NoopMergePassRunner, NoopRetirementPassRunner};

    fn assert_merge_runner(_r: &impl MergePassRunner) {}
    fn assert_retirement_runner(_r: &impl RetirementPassRunner) {}

    assert_merge_runner(&NoopMergePassRunner);
    assert_retirement_runner(&NoopRetirementPassRunner);
}
