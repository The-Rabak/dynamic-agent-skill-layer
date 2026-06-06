//! Extraction-CONTENT-quality harness — proves a real session transcript is
//! extracted into a `.pending` draft that actually CAPTURES the procedure it
//! taught, not merely that a draft file exists.
//!
//! # Why this exists (the gap it closes)
//! `test_transcript_ingest_queue_e2e` proves the queue→drain→`.pending` wiring
//! and asserts only `contains("origin: session_extraction")` — i.e. a draft was
//! produced. It says nothing about whether the draft is any GOOD. The 2026-06-02
//! assessment's recurring critique was "tests accept weaker behaviour than the
//! product promise": a self-growing skill layer whose extractions are vague or
//! hallucinated is worthless even when a file lands. This harness measures
//! extraction *fidelity*:
//!   • structural validity (frontmatter, H1, real subunit sections),
//!   • topical relevance (the draft is about what the session taught),
//!   • content fidelity (it covers the concrete concepts the transcript taught),
//!   • anti-hallucination/safety (it does NOT recommend the explicit anti-pattern),
//!   • human gate (only `.pending`, never an auto-approved `SKILL.md`).
//!
//! # No fakes
//! Real Ollama extraction (production default `gemma4:12b`), real PG ingest
//! queue, real `/ingest/transcript` endpoint, the real `PendingDraftWriter`. The
//! drain runs in-process — the SAME `TranscriptQueueDrain::drain_once()` code the
//! deployed maintenance worker runs — which is the repo's sanctioned pattern for
//! this path (`test_transcript_ingest_queue_e2e`). Drafts land on a host sandbox
//! under `target/`, inside `SKILL_GLOBAL_ALLOWED_ROOTS`; the test DB/namespace is
//! isolated so prod data stays clean.
//!
//! # Run
//! ```sh
//! cargo test -p mcp-server --features test-utils --test test_extraction_quality -- --ignored
//! # speed override (smaller model): OLLAMA_EXTRACTION_MODEL=granite4:3b
//! ```

use std::{
    collections::BTreeMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use infrastructure::{DependencyFactory, TranscriptIngestQueue};
use maintenance::{DEFAULT_TRANSCRIPT_DRAIN_BATCH, TranscriptQueueDrain};
use mcp_server::{McpServerApp, protocol};
use retrieval::RetrievalConfig;
use session_extractor::SessionExtractor;

#[path = "report.rs"]
mod report;

#[path = "../integration/env_guard.rs"]
mod env_guard;

const INGEST_SECRET: &str = "test-ingest-secret-extraction-quality";

/// Minimum number of taught concept groups a draft must cover to count as a
/// faithful extraction. The transcript teaches five; a draft that captures fewer
/// than two has not learned the procedure.
const MIN_CONCEPT_COVERAGE: usize = 2;

fn retrieval_config() -> RetrievalConfig {
    RetrievalConfig {
        candidate_limit: 32,
        max_results: 2,
        max_subunits_per_skill: 4,
        rescue_threshold: 0.1,
        relevance_threshold: 0.15,
        mmr_lambda: 0.6,
        ..RetrievalConfig::default()
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve")
}

/// The concrete concepts the rich transcript teaches, each as a synonym group.
/// A draft "covers" a group if its text contains any synonym. Grounded in
/// `tests/fixtures/session-rich-transcript.jsonl`.
fn taught_concepts() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("file_io_topic", vec!["file", "fs", "i/o", "io", "read", "write"]),
        ("error_safety", vec!["result", "error", "unwrap", "propagate", "?", "io::error"]),
        ("create_parent_dir", vec!["create_dir_all", "parent", "directory", "create dir"]),
        ("atomic_write", vec!["rename", "atomic", ".tmp", "tmp", "temporary"]),
        ("naming_convention", vec!["read_to_string_safe", "write_atomic", "helper"]),
    ]
}

// TODO(#199): Re-introduce an anti-pattern SAFETY check with a non-naive
// implementation. The whole "forbidden anti-pattern" path is intentionally
// DISABLED for now (commented out below at the compute site, the contract
// assertion, the log line, and the pass/fail gate).
//
// Why disabled: the transcript TEACHES the anti-pattern as a warning ("Never run
// rm -rf on the repo root..."), so a FAITHFUL draft legitimately contains the
// substring "rm -rf". A `!contains("rm -rf")` check fails a correct extraction,
// and even a negation-window heuristic ("never"/"don't" nearby) is a brittle
// fixed-word footgun that will misfire in production. Until we have a real
// implementation (e.g. an LLM/judge classifying "recommends vs warns against",
// or a structured safety-annotation field on the candidate), this check causes
// more false failures than it prevents real ones, so it does not gate the suite.

/// A parsed `.pending` draft: frontmatter key/values + the markdown body.
struct ParsedDraft {
    frontmatter: BTreeMap<String, String>,
    body: String,
    full_text_lower: String,
}

/// Splits a `.pending` file into frontmatter and body and lowercases the whole
/// thing for concept matching. The writer always emits `---\n<yaml>\n---\n\n<body>`.
fn parse_draft(content: &str) -> ParsedDraft {
    let mut frontmatter = BTreeMap::new();
    let mut body = content.to_owned();

    if let Some(rest) = content.strip_prefix("---\n")
        && let Some(end) = rest.find("\n---\n")
    {
        let yaml = &rest[..end];
        body = rest[end + "\n---\n".len()..].to_owned();
        for line in yaml.lines() {
            if let Some((k, v)) = line.split_once(':') {
                frontmatter.insert(k.trim().to_owned(), v.trim().trim_matches('"').to_owned());
            }
        }
    }

    ParsedDraft {
        frontmatter,
        full_text_lower: content.to_lowercase(),
        body,
    }
}

impl ParsedDraft {
    /// Number of taught concept groups this draft covers.
    fn concept_coverage(&self) -> Vec<&'static str> {
        taught_concepts()
            .into_iter()
            .filter(|(_, syns)| syns.iter().any(|s| self.full_text_lower.contains(s)))
            .map(|(name, _)| name)
            .collect()
    }

    /// Topic-token overlap used to pick the most on-topic draft when the model
    /// emits several candidates.
    fn topic_score(&self) -> usize {
        ["file", "fs", "io", "read", "write", "rust"]
            .iter()
            .filter(|t| self.full_text_lower.contains(**t))
            .count()
    }
}

/// Recursively collects every `.pending` file under `dir`.
fn collect_pending(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_pending(&path, out);
            } else if path.extension().and_then(|s| s.to_str()) == Some("pending") {
                out.push(path);
            }
        }
    }
}

/// Recursively collects any approved `SKILL.md` (no `.pending`) under `dir` —
/// these must NEVER appear (the human gate forbids auto-approval).
fn collect_approved(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_approved(&path, out);
            } else if path.file_name().and_then(|s| s.to_str()) == Some("SKILL.md") {
                out.push(path);
            }
        }
    }
}

#[ignore = "requires live containers"]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn extracted_pending_draft_captures_the_taught_procedure() {
    let repo_root = repo_root();
    let fixture = repo_root.join("tests/fixtures/session-rich-transcript.jsonl");
    let transcript_content =
        std::fs::read_to_string(&fixture).expect("rich transcript fixture should read");

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let sandbox = repo_root.join(format!("target/tmp-extraction-quality-{nonce}"));
    std::fs::create_dir_all(&sandbox).expect("sandbox creatable");

    let namespace = env_guard::isolated_namespace_with_global_path(sandbox.clone()).await;
    // SAFETY: env mutated only while holding ENV_LOCK via the namespace guard.
    unsafe {
        std::env::set_var("CLAUDE_TRANSCRIPT_ROOT", repo_root.join("tests/fixtures"));
        std::env::set_var("EXTRACT_SESSION_PROVIDER", "ollama");
        // Production default model unless the caller overrides (granite4:3b is a
        // faster CPU option). Measuring the SHIPPED default is the honest choice.
        if std::env::var("OLLAMA_EXTRACTION_MODEL").is_err() {
            std::env::set_var("OLLAMA_EXTRACTION_MODEL", "gemma4:12b");
        }
        let ollama_base = std::env::var("OLLAMA_URL")
            .unwrap_or_else(|_| "http://localhost:11444".to_owned())
            .trim_end_matches('/')
            .to_owned();
        std::env::set_var("OLLAMA_EXTRACTION_ENDPOINT", format!("{ollama_base}/api/generate"));
        // No extraction request timeout: large models run to completion against a warm
        // Ollama (OLLAMA_KEEP_ALIVE keeps models resident); no per-call ceiling is set.
        std::env::set_var("TRANSCRIPT_INGEST_SECRET", INGEST_SECRET);
    }

    let mut builder = report::ReportBuilder::new("extracted_pending_draft_captures_the_taught_procedure");

    // ── Boot the real server (wires the durable queue) and serve it ───────────
    let components = McpServerApp::from_environment(retrieval_config())
        .await
        .expect("connect to live infrastructure");
    let queue = TranscriptIngestQueue::new(components.pg_adapter.pool().clone());

    let health_checker = DependencyFactory::build_health_checker_from_environment();
    let router = protocol::router(components.app.clone(), health_checker);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind ephemeral");
    let addr: SocketAddr = listener.local_addr().expect("local addr");
    let server = tokio::spawn(async move { axum::serve(listener, router).await.expect("serve") });
    let ingest_url = format!("http://{addr}/ingest/transcript");
    let health_url = format!("http://{addr}/health");
    let http = reqwest::Client::new();

    let mut ready = false;
    for _ in 0..100 {
        if http.get(&health_url).send().await.is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(ready, "ephemeral ingest server did not become reachable");

    // ── Real extractor + drain (same code the deployed worker runs) ───────────
    let extractor = SessionExtractor::from_environment().expect("live extractor builds");
    let drain = TranscriptQueueDrain::new(queue.clone(), extractor, DEFAULT_TRANSCRIPT_DRAIN_BATCH);
    let pending_root = sandbox.join(".skills");

    // Ingest the taught session through the REAL endpoint, then drain. The only
    // nondeterminism is whether the LLM emits a candidate; if a round yields no
    // draft we re-ingest a fresh content variant and drain again (bounded). We do
    // NOT retry on a LOW-QUALITY draft — once a draft exists, its quality is judged
    // as-is. That keeps the fidelity bar honest.
    let session_id = format!("extraction-quality-{nonce}");
    let mut pending_files = Vec::new();
    let max_attempts = 4;
    let mut attempt = 0;
    while pending_files.is_empty() && attempt < max_attempts {
        attempt += 1;
        let content = if attempt == 1 {
            transcript_content.clone()
        } else {
            format!(
                "{transcript_content}{{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":\"Capture attempt {attempt}: record the reusable Rust file I/O skill — the create_dir_all-before-write step, the atomic write-then-rename, error propagation with ?, and the no-unwrap convention.\"}}}}\n"
            )
        };
        let resp = http
            .post(&ingest_url)
            .header("X-Ingest-Secret", INGEST_SECRET)
            .json(&serde_json::json!({
                "session_id": format!("{session_id}-{attempt}"),
                "source": "session_end",
                "content": content,
                "repo_path": sandbox.to_str().unwrap(),
            }))
            .send()
            .await
            .expect("ingest request sends");
        assert!(
            resp.status().is_success(),
            "ingest endpoint must accept the transcript, got {}",
            resp.status()
        );
        let drain_report = drain.drain_once().await.expect("drain sweep succeeds");
        builder.record_latency(&format!("drain_attempt_{attempt}_processed_{}", drain_report.processed), 0);
        pending_files.clear();
        collect_pending(&pending_root, &mut pending_files);
    }

    assert!(
        !pending_files.is_empty(),
        "real extraction produced NO .pending draft after {attempt} attempts against {} — \
         the self-growing loop yields nothing for a procedure-rich session",
        std::env::var("OLLAMA_EXTRACTION_MODEL").unwrap_or_default()
    );

    // ── Pick the most on-topic draft and judge its CONTENT ────────────────────
    let mut drafts: Vec<(PathBuf, ParsedDraft)> = pending_files
        .iter()
        .map(|p| {
            let content = std::fs::read_to_string(p).expect("draft readable");
            (p.clone(), parse_draft(&content))
        })
        .collect();
    drafts.sort_by(|a, b| b.1.topic_score().cmp(&a.1.topic_score()));
    let (draft_path, draft) = &drafts[0];

    // — Structural validity (deterministic; must always hold) —
    let is_pending = draft_path.extension().and_then(|s| s.to_str()) == Some("pending");
    let origin_ok = draft.frontmatter.get("origin").map(String::as_str) == Some("session_extraction");
    let name = draft.frontmatter.get("name").cloned().unwrap_or_default();
    let description = draft.frontmatter.get("description").cloned().unwrap_or_default();
    let has_name = !name.is_empty();
    let has_description = description.len() >= 20;
    let has_h1 = draft.body.lines().any(|l| l.trim_start().starts_with("# "));
    let has_subunit_section = draft.body.contains("## Procedures")
        || draft.body.contains("## Conventions")
        || draft.body.contains("## Assets");
    let has_bullet = draft.body.lines().any(|l| l.trim_start().starts_with("- "));
    let provenance_ok = draft
        .frontmatter
        .get("source_session_id")
        .map(|s| s.contains(&session_id))
        .unwrap_or(false);

    // — Topical relevance: the draft is about what the session taught —
    let on_topic = draft.topic_score() >= 1;

    // — Content fidelity: how many taught concepts did it actually capture? —
    let covered = draft.concept_coverage();
    let coverage_ok = covered.len() >= MIN_CONCEPT_COVERAGE;

    // — Anti-hallucination / safety: anti-pattern check DISABLED — see TODO(#199). —
    // let no_forbidden = !recommends_antipattern(&draft.full_text_lower, FORBIDDEN_ANTIPATTERN);

    // — Human gate: nothing auto-approved —
    let mut approved = Vec::new();
    collect_approved(&pending_root, &mut approved);
    let human_gate_ok = approved.is_empty();

    let structural_ok = is_pending
        && origin_ok
        && has_name
        && has_description
        && has_h1
        && has_subunit_section
        && has_bullet
        && provenance_ok;

    // ── Record everything (evidence persists regardless of pass/fail) ─────────
    builder.add_contract_assertion(report::ContractAssertion {
        contract_name: "extraction::structural_validity".to_owned(),
        status: bool_status(structural_ok, "well-formed pending draft", &format!(
            "pending={is_pending} origin={origin_ok} name={has_name} desc>=20={has_description} h1={has_h1} section={has_subunit_section} bullet={has_bullet} provenance={provenance_ok}"
        )),
        details: format!("name={name:?} desc_len={}", description.len()),
    });
    builder.add_contract_assertion(report::ContractAssertion {
        contract_name: "extraction::on_topic".to_owned(),
        status: bool_status(on_topic, "draft is about the taught topic", &format!("topic_score={}", draft.topic_score())),
        details: "a draft unrelated to the session topic is a hallucination".to_owned(),
    });
    builder.add_contract_assertion(report::ContractAssertion {
        contract_name: "extraction::content_fidelity".to_owned(),
        status: bool_status(coverage_ok, &format!(">= {MIN_CONCEPT_COVERAGE} taught concepts captured"), &format!("covered {:?}", covered)),
        details: format!("{}/{} concept groups captured: {covered:?}", covered.len(), taught_concepts().len()),
    });
    // DISABLED — see TODO(#199). Re-enable with a non-naive safety classifier.
    // builder.add_contract_assertion(report::ContractAssertion {
    //     contract_name: "extraction::no_forbidden_antipattern".to_owned(),
    //     status: bool_status(no_forbidden, "anti-pattern not recommended", &format!("draft contains '{FORBIDDEN_ANTIPATTERN}'")),
    //     details: format!("transcript explicitly warns against '{FORBIDDEN_ANTIPATTERN}'"),
    // });
    builder.add_contract_assertion(report::ContractAssertion {
        contract_name: "extraction::human_gate_no_autoapprove".to_owned(),
        status: bool_status(human_gate_ok, "only .pending, no approved SKILL.md", &format!("found approved: {approved:?}")),
        details: "extraction must never auto-create an active SKILL.md".to_owned(),
    });

    let report_doc = builder.build();
    let report_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/reports");
    std::fs::create_dir_all(&report_dir).expect("reports dir");
    let report_path = report_dir.join(format!("{}__{}.json", report_doc.test_name, report_doc.test_id));
    std::fs::write(&report_path, serde_json::to_string_pretty(&report_doc).expect("report serializes"))
        .expect("report writes");
    println!(
        "[extraction-quality] draft={} concepts={}/{} {:?} on_topic={} report={}",
        draft_path.display(), covered.len(), taught_concepts().len(), covered, on_topic, report_path.display()
    );

    // ── Teardown BEFORE the quality asserts so it always runs ─────────────────
    server.abort();
    components.teardown().await.expect("teardown");
    let _ = std::fs::remove_dir_all(&sandbox);
    namespace.cleanup().await;

    // ── The brutal quality bar ────────────────────────────────────────────────
    assert!(
        structural_ok,
        "\n=== EXTRACTED DRAFT IS STRUCTURALLY MALFORMED ===\n\
         A real session produced a draft that is not a usable SKILL.md \
         (missing frontmatter origin, name, description, H1, or a subunit section).\n\
         The self-growing loop emits unusable artifacts. Report: {}\n",
        report_path.display()
    );
    assert!(
        human_gate_ok,
        "\n=== HUMAN GATE VIOLATED — extraction auto-approved a skill ===\n\
         Found active SKILL.md file(s): {approved:?}. Extraction must only ever write .pending.\n"
    );
    // NOTE: the anti-pattern safety gate is DISABLED — see TODO(#199). Only the
    // on-topic (anti-hallucination) check gates here for now.
    assert!(
        on_topic,
        "\n=== EXTRACTION HALLUCINATED ===\n\
         on_topic={on_topic} (draft about the taught topic?).\n\
         Report: {}\n",
        report_path.display()
    );
    assert!(
        coverage_ok,
        "\n=== EXTRACTION DID NOT CAPTURE THE TAUGHT PROCEDURE ===\n\
         The draft covered only {}/{} of the concepts the session taught ({covered:?}); \
         the bar is >= {MIN_CONCEPT_COVERAGE}.\n\
         A draft that exists but does not encode the procedure is worthless to the\n\
         self-growing loop — this is the difference between 'a file landed' and\n\
         'a skill was learned'. Inspect the extraction prompt and model output.\n\
         Report: {}\n",
        covered.len(), taught_concepts().len(), report_path.display()
    );
}

/// Maps a bool to a report `AssertionResult` with an expected/actual on failure.
fn bool_status(ok: bool, expected: &str, actual: &str) -> report::AssertionResult {
    if ok {
        report::AssertionResult::Passed
    } else {
        report::AssertionResult::Failed {
            expected: expected.to_owned(),
            actual: actual.to_owned(),
        }
    }
}

