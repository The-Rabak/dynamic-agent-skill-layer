//! Writer → reader SKILL.md format contract (#224 regression).
//!
//! The extraction **writer** (`session_extractor`) and the graph-builder
//! **reader** (`graph_builder::extraction`) must agree on one on-disk SKILL.md
//! format. Before #224 they did not: the writer emitted YAML frontmatter while
//! the reader scanned the body as if there were none, so it captured the opening
//! `---` fence as the `description` (229/234 corpus skills landed with
//! `description = "---"`) and leaked frontmatter list items in as subunits.
//!
//! The defect was invisible because the e2e quality harness seeded skills in the
//! reader's hand-authored format, never via the real writer. This test closes
//! that gap: it drives the **real** `PendingDraftWriter` to produce a SKILL.md
//! on disk, then feeds that exact file to the **real** reader and asserts the
//! authoritative fields survive the round trip. No hand-authored fixture stands
//! in for the writer's output — that substitution is precisely what hid the bug.

use std::{env, fs, sync::Mutex};

use domain::{DomainId, ExtractedSkillCandidate, ExtractionResult, PENDING_SKILL_FILE_NAME};
use graph_builder::extraction::extract_skill;
use session_extractor::{ExtractSessionRequest, writer::PendingDraftWriter};

/// `write_pending_drafts` reads `SKILL_GLOBAL_ALLOWED_ROOTS` from the process
/// environment; serialize the env-mutating tests so they cannot race.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn unique_sandbox(label: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let path = env::temp_dir().join(format!(
        "dasl-roundtrip-{label}-{}-{nanos}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("sandbox should be creatable");
    path
}

#[test]
fn writer_output_is_read_back_with_description_tags_and_procedures_intact() {
    let _guard = ENV_LOCK.lock().expect("env lock poisoned");

    let sandbox = unique_sandbox("intact");
    let project_root = sandbox.join("project");
    fs::create_dir_all(&project_root).expect("project root creatable");

    // The candidate carries a real description, tags, and ≥2 procedures so the
    // structural extractor is satisfied (no Ollama fallback needed).
    let candidate = ExtractedSkillCandidate {
        name: "http-router-security-defaults".to_owned(),
        description: "Apply mandatory security defaults to every HTTP router so handlers \
                      cannot opt out of protections."
            .to_owned(),
        tags: vec!["security".to_owned(), "http".to_owned(), "rust".to_owned()],
        procedures: vec![
            "Set secure response headers (HSTS, X-Content-Type-Options) globally".to_owned(),
            "Reject state-changing requests that lack a valid CSRF token".to_owned(),
            "Default to deny: routes must explicitly opt in to public access".to_owned(),
        ],
        conventions: vec!["Security middleware is registered before any route handler".to_owned()],
        assets: vec![],
        confidence: 0.92,
        generality: Some("general".to_owned()),
        generality_rationale: Some("No project-specific identifiers in the procedures.".to_owned()),
        // Multi-view fields left empty for this test — absent-fields case is covered
        // by `candidate_without_multiview_fields_round_trips_unchanged` below.
        use_when: vec![],
        avoid_when: vec![],
        artifacts: vec![],
        tools: vec![],
        invariants: vec![],
        requires: vec![],
        produces: vec![],
        skill_type: None,
        evidence: vec![],
    };
    let extraction_result = ExtractionResult {
        source_session_id: DomainId::new_unchecked("session-roundtrip"),
        candidates: vec![candidate.clone()],
        provider: "claude-code".to_owned(),
        assessment: None,
    };
    let request = ExtractSessionRequest {
        transcript_ref: "session-roundtrip.jsonl".to_owned(),
        transcript_inline: None,
        session_id: "session-roundtrip".to_owned(),
        repo_path: Some(project_root.to_str().unwrap().to_owned()),
    };

    // Drive the REAL writer. `new_unbounded_for_tests` skips the write-guard, but
    // scope resolution still reads SKILL_GLOBAL_ALLOWED_ROOTS, so set it.
    unsafe {
        env::set_var("SKILL_GLOBAL_ALLOWED_ROOTS", sandbox.display().to_string());
        env::remove_var("SKILL_GLOBAL_WRITE_ROOTS");
    }
    let writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);
    let written = writer
        .write_pending_drafts(&extraction_result, &request, "claude-code")
        .expect("writer must persist the pending draft");
    unsafe {
        env::remove_var("SKILL_GLOBAL_ALLOWED_ROOTS");
    }

    assert_eq!(written.len(), 1, "exactly one draft expected");
    let pending_path = &written[0];
    assert!(
        pending_path.ends_with(PENDING_SKILL_FILE_NAME),
        "writer should emit a {PENDING_SKILL_FILE_NAME} file, got {pending_path:?}"
    );

    // Read back the EXACT bytes the writer produced and parse with the real reader.
    let on_disk = fs::read_to_string(pending_path).expect("pending file should read");
    let extraction = extract_skill(pending_path, &on_disk);

    // The #224 contract: the reader recovers the authoritative metadata.
    assert_eq!(
        extraction.description, candidate.description,
        "description must round-trip from the writer's frontmatter; \
         got {:?}\n--- file ---\n{on_disk}",
        extraction.description
    );
    assert_ne!(
        extraction.description, "---",
        "the #224 fence-as-description bug must not recur"
    );
    assert_eq!(
        extraction.skill_name, candidate.name,
        "name must round-trip from the frontmatter / H1"
    );
    assert_eq!(
        extraction.tags, candidate.tags,
        "tags must round-trip from the frontmatter (canonical `tags` key)"
    );

    // Procedures from the body must survive; frontmatter YAML list items
    // (e.g. `- security`) must NOT leak in as subunits.
    let procedure_count = extraction
        .subunits
        .iter()
        .filter(|s| s.kind == domain::SubunitType::Procedure)
        .count();
    assert_eq!(
        procedure_count,
        candidate.procedures.len(),
        "all {} body procedures must be extracted as subunits, got {procedure_count}; \
         subunits = {:?}",
        candidate.procedures.len(),
        extraction.subunits
    );
    assert!(
        !extraction.used_ollama_fallback,
        "structural extraction should be sufficient — no fallback expected"
    );
    // No subunit content should be a bare YAML key or fence.
    for subunit in &extraction.subunits {
        assert_ne!(
            subunit.content, "security",
            "frontmatter tag leaked as subunit"
        );
        assert!(
            !subunit.content.starts_with("generality:"),
            "frontmatter key leaked as subunit: {:?}",
            subunit.content
        );
    }

    let _ = fs::remove_dir_all(&sandbox);
}

/// Multi-view fields round-trip through the real writer and real reader.
///
/// Proves that when a candidate carries `use_when`, `avoid_when`, `artifacts`,
/// `tools`, `invariants`, `requires`, and `produces`, those fields survive the
/// full write→read cycle and do NOT leak into body subunits.
#[test]
fn multiview_fields_round_trip_through_real_writer_and_reader() {
    let _guard = ENV_LOCK.lock().expect("env lock poisoned");

    let sandbox = unique_sandbox("multiview");
    let project_root = sandbox.join("project");
    fs::create_dir_all(&project_root).expect("project root creatable");

    let candidate = ExtractedSkillCandidate {
        name: "docker-compose-local-dev".to_owned(),
        description: "Manage local dev services with docker compose.".to_owned(),
        tags: vec!["docker".to_owned(), "dev".to_owned()],
        procedures: vec![
            "Run docker compose up -d to start all services.".to_owned(),
            "Run docker compose down to stop and remove containers.".to_owned(),
        ],
        conventions: vec!["Always pin service image tags in docker-compose.yml.".to_owned()],
        assets: vec![],
        confidence: 0.85,
        generality: Some("general".to_owned()),
        generality_rationale: Some("No project-specific paths referenced.".to_owned()),
        use_when: vec![
            "Starting local dev environment".to_owned(),
            "Spinning up dependencies for integration tests".to_owned(),
        ],
        avoid_when: vec!["Deploying to production".to_owned()],
        artifacts: vec!["docker-compose.yml".to_owned(), ".env.local".to_owned()],
        tools: vec!["docker".to_owned(), "docker compose".to_owned()],
        invariants: vec![
            "All services must pass healthchecks before the stack is considered ready".to_owned(),
        ],
        requires: vec!["Docker Desktop >= 4.x installed".to_owned()],
        produces: vec!["Running local service stack accessible on localhost ports".to_owned()],
        skill_type: Some("procedure".to_owned()),
        evidence: vec!["docker compose up -d".to_owned()],
    };
    let extraction_result = ExtractionResult {
        source_session_id: DomainId::new_unchecked("session-multiview"),
        candidates: vec![candidate.clone()],
        provider: "claude-code".to_owned(),
        assessment: None,
    };
    let request = ExtractSessionRequest {
        transcript_ref: "session-multiview.jsonl".to_owned(),
        transcript_inline: None,
        session_id: "session-multiview".to_owned(),
        repo_path: Some(project_root.to_str().unwrap().to_owned()),
    };

    unsafe {
        env::set_var("SKILL_GLOBAL_ALLOWED_ROOTS", sandbox.display().to_string());
        env::remove_var("SKILL_GLOBAL_WRITE_ROOTS");
    }
    let writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);
    let written = writer
        .write_pending_drafts(&extraction_result, &request, "claude-code")
        .expect("writer must persist the pending draft");
    unsafe {
        env::remove_var("SKILL_GLOBAL_ALLOWED_ROOTS");
    }

    assert_eq!(written.len(), 1, "exactly one draft expected");
    let pending_path = &written[0];
    let on_disk = fs::read_to_string(pending_path).expect("pending file should read");
    let extraction = extract_skill(pending_path, &on_disk);

    // Core metadata still survives.
    assert_eq!(
        extraction.description, candidate.description,
        "description must survive multi-view round-trip; file:\n{on_disk}"
    );
    assert_eq!(extraction.tags, candidate.tags, "tags must survive");

    // Multi-view fields must survive losslessly.
    assert_eq!(
        extraction.use_when, candidate.use_when,
        "use_when must round-trip; file:\n{on_disk}"
    );
    assert_eq!(
        extraction.avoid_when, candidate.avoid_when,
        "avoid_when must round-trip"
    );
    assert_eq!(
        extraction.artifacts, candidate.artifacts,
        "artifacts must round-trip"
    );
    assert_eq!(extraction.tools, candidate.tools, "tools must round-trip");
    assert_eq!(
        extraction.invariants, candidate.invariants,
        "invariants must round-trip"
    );
    assert_eq!(
        extraction.requires, candidate.requires,
        "requires must round-trip"
    );
    assert_eq!(
        extraction.produces, candidate.produces,
        "produces must round-trip"
    );

    // Multi-view frontmatter items must NOT leak into body subunits.
    let subunit_bodies: Vec<&str> = extraction
        .subunits
        .iter()
        .map(|s| s.content.as_str())
        .collect();
    let multi_view_values = [
        "Starting local dev environment",
        "Deploying to production",
        "docker-compose.yml",
        "Docker Desktop >= 4.x installed",
    ];
    for mv_value in &multi_view_values {
        assert!(
            !subunit_bodies.contains(mv_value),
            "multi-view value '{mv_value}' must not leak into subunits; subunits = {subunit_bodies:?}"
        );
    }

    // Only the 2 body procedures should be subunits.
    let procedure_count = extraction
        .subunits
        .iter()
        .filter(|s| s.kind == domain::SubunitType::Procedure)
        .count();
    assert_eq!(
        procedure_count, 2,
        "only 2 body procedures should be subunits; got {procedure_count}"
    );

    let _ = fs::remove_dir_all(&sandbox);
}

/// A candidate with NO multi-view fields still round-trips cleanly with empty Vecs.
///
/// Proves backward compatibility: old skills or providers that do not emit the
/// optional fields produce a `SkillExtraction` with empty multi-view fields —
/// never a parse failure.
#[test]
fn candidate_without_multiview_fields_round_trips_unchanged() {
    let _guard = ENV_LOCK.lock().expect("env lock poisoned");

    let sandbox = unique_sandbox("no-multiview");
    let project_root = sandbox.join("project");
    fs::create_dir_all(&project_root).expect("project root creatable");

    // Candidate with NO multi-view fields (all empty Vecs) — simulates a provider
    // that only emits the core fields, as Ollama/gemma often does.
    let candidate = ExtractedSkillCandidate {
        name: "cargo-test-workflow".to_owned(),
        description: "Run and interpret cargo test output for a Rust workspace.".to_owned(),
        tags: vec!["rust".to_owned(), "testing".to_owned()],
        procedures: vec![
            "Run cargo test --workspace from the crate root.".to_owned(),
            "Grep output for FAILED and check the error count.".to_owned(),
        ],
        conventions: vec![],
        assets: vec![],
        confidence: 0.80,
        generality: Some("general".to_owned()),
        generality_rationale: None,
        use_when: vec![],
        avoid_when: vec![],
        artifacts: vec![],
        tools: vec![],
        invariants: vec![],
        requires: vec![],
        produces: vec![],
        skill_type: None,
        evidence: vec![],
    };
    let extraction_result = ExtractionResult {
        source_session_id: DomainId::new_unchecked("session-no-multiview"),
        candidates: vec![candidate.clone()],
        provider: "claude-code".to_owned(),
        assessment: None,
    };
    let request = ExtractSessionRequest {
        transcript_ref: "session-no-multiview.jsonl".to_owned(),
        transcript_inline: None,
        session_id: "session-no-multiview".to_owned(),
        repo_path: Some(project_root.to_str().unwrap().to_owned()),
    };

    unsafe {
        env::set_var("SKILL_GLOBAL_ALLOWED_ROOTS", sandbox.display().to_string());
        env::remove_var("SKILL_GLOBAL_WRITE_ROOTS");
    }
    let writer = PendingDraftWriter::new_unbounded_for_tests(vec![sandbox.clone()]);
    let written = writer
        .write_pending_drafts(&extraction_result, &request, "claude-code")
        .expect("writer must persist the pending draft");
    unsafe {
        env::remove_var("SKILL_GLOBAL_ALLOWED_ROOTS");
    }

    assert_eq!(written.len(), 1, "exactly one draft expected");
    let pending_path = &written[0];
    let on_disk = fs::read_to_string(pending_path).expect("pending file should read");

    // When all multi-view fields are empty, the writer must NOT emit them (no
    // spurious empty-list YAML keys in the on-disk file).
    assert!(
        !on_disk.contains("use_when:"),
        "empty use_when must not appear in on-disk YAML; file:\n{on_disk}"
    );
    assert!(
        !on_disk.contains("avoid_when:"),
        "empty avoid_when must not appear in on-disk YAML"
    );
    assert!(
        !on_disk.contains("artifacts:"),
        "empty artifacts must not appear in on-disk YAML"
    );
    assert!(
        !on_disk.contains("tools:"),
        "empty tools must not appear in on-disk YAML"
    );
    assert!(
        !on_disk.contains("invariants:"),
        "empty invariants must not appear in on-disk YAML"
    );
    assert!(
        !on_disk.contains("requires:"),
        "empty requires must not appear in on-disk YAML"
    );
    assert!(
        !on_disk.contains("produces:"),
        "empty produces must not appear in on-disk YAML"
    );

    // The reader must parse cleanly and yield empty Vecs, not fail.
    let extraction = extract_skill(pending_path, &on_disk);
    assert_eq!(
        extraction.description, candidate.description,
        "description must survive"
    );
    assert!(
        extraction.use_when.is_empty(),
        "absent use_when must parse as empty Vec"
    );
    assert!(
        extraction.avoid_when.is_empty(),
        "absent avoid_when must parse as empty Vec"
    );
    assert!(
        extraction.artifacts.is_empty(),
        "absent artifacts must parse as empty Vec"
    );
    assert!(
        extraction.tools.is_empty(),
        "absent tools must parse as empty Vec"
    );
    assert!(
        extraction.invariants.is_empty(),
        "absent invariants must parse as empty Vec"
    );
    assert!(
        extraction.requires.is_empty(),
        "absent requires must parse as empty Vec"
    );
    assert!(
        extraction.produces.is_empty(),
        "absent produces must parse as empty Vec"
    );

    let _ = fs::remove_dir_all(&sandbox);
}
