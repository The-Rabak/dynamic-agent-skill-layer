//! Retrieval-quality harness — measures the REAL shipped retrieval pipeline
//! against a graded relevance corpus on the live containerized stack.
//!
//! # Why this exists
//! The product is named for SkillRAE semantic multi-level retrieval. Until #172
//! the β (subunit-evidence) term was lexical token overlap; it is now real
//! semantic cosine. But "we deliver semantic retrieval" was a claim about code
//! structure, never about *results*. This harness turns it into a measured
//! number: precision@k, recall@k, MRR, MAP, nDCG against ground truth, plus a
//! head-to-head against an in-harness pure-lexical baseline so we can prove (or
//! disprove) that semantic ranking beats keyword matching.
//!
//! # No fakes
//! Every query goes over HTTP to the running `mcp-server`; every skill enters
//! through the real sidecar→approve→graph-builder→snapshot loop; embeddings are
//! real Ollama `nomic-embed-text`. The only test-side computation is the lexical
//! baseline (a measurement reference) and the metric math (pure, unit-tested).
//! Test volumes/DB (`skill_layer_test`, `test-*-skills`) keep prod data clean.
//!
//! # Isolation (#199)
//! The 4 live tests serialize behind `RQ_MEASUREMENT_LOCK` (NOT the bring-up
//! lock from `stack.rs`) so each test's seed→measure→cleanup critical section
//! is atomic with respect to the other 3.  At the start of each critical section
//! the global skills volume is pruned to only this run's corpus (foreign leftovers
//! from other suites are evicted) and the graph is polled until it reflects
//! exactly the corpus size before any query is issued.
//!
//! # Running
//! ```sh
//! ./scripts/run-e2e-tests.sh --include-dream   # brings the stack up first
//! # or, against an already-running stack:
//! cargo test -p mcp-server --features test-utils --test test_retrieval_quality -- --ignored
//! ```
//!
//! The non-ignored tests in this binary are the pure metric/corpus unit tests in
//! the included modules — they run without containers.

#[path = "report.rs"]
mod report;

#[path = "harness/mod.rs"]
mod harness;

#[path = "quality/metrics.rs"]
mod metrics;

#[path = "quality/labeled_corpus.rs"]
mod labeled_corpus;

use std::collections::BTreeSet;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use harness::{
    app::{CompileContextArgs, McpClient},
    guard::SeededSkillGuard,
    observe::PgObserver,
    poll::wait_for_rebuild,
    seed::{self, SkillScope},
    stack::Stack,
    stagelog::StageLogger,
};
use labeled_corpus::{LabeledCorpus, LabeledQuery, lexical_baseline_ranking, load};
use metrics::{aggregate, query_metrics};
use report::{AssertionResult, ContractAssertion};
use serde_json::json;
use tokio::sync::Mutex;

/// Top-k cutoff. The server serves `max_results` (default 3), so k=3 measures
/// exactly what the product actually injects.
const K: usize = 3;

/// Process-wide mutex that serializes the 4 live measurement tests.
///
/// Each test's seed→measure→cleanup critical section is atomic with respect to
/// the others: only ONE test may be running its measurement at a time.  Bring-up
/// parallelism (from `stack.rs::BRINGUP_LOCK`) is intentionally untouched — the
/// 4 tests still bring the stack up concurrently, they just wait here before
/// touching the shared global-skills volume.
///
/// IMPORTANT: do NOT reuse or reference `stack::BRINGUP_LOCK` for this purpose.
/// The bring-up and the measurement are independent concerns with different
/// lifetimes.
static RQ_MEASUREMENT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Per-run namespace so concurrent/repeat runs never collide or dedup.
fn run_namespace() -> String {
    format!("rq{}", chrono::Utc::now().timestamp_millis())
}

/// Returns a set of all slugs expected for this namespace+corpus combination.
fn corpus_slugs(ns: &str, corpus: &LabeledCorpus) -> BTreeSet<String> {
    corpus
        .skills
        .iter()
        .map(|s| format!("{ns}-{}", s.id))
        .collect()
}

/// Removes every global-skills directory that is NOT in `keep_slugs`.
///
/// This is the "isolation step": it evicts foreign leftovers (from other
/// suites or from a previous aborted run) before seeding the new corpus.
/// Per-slug `seed::remove` is used intentionally — a blanket `rm -rf` of
/// the whole volume would destroy the SKILL.md files of any other test
/// currently in its seeding phase (impossible when called inside the
/// measurement lock, but good hygiene regardless).
///
/// Errors are logged loudly; removal failures do NOT panic (the calling
/// test will either fail honestly when the graph count doesn't converge, or
/// measure against a slightly dirtier scope and surface that in the numbers).
fn evict_foreign_global_skills(keep_slugs: &BTreeSet<String>) {
    match seed::list(SkillScope::Global) {
        Err(e) => eprintln!(
            "[rq-isolation] WARN: could not list global skills volume: {e} \
             — proceeding without eviction (measurement may be contaminated)"
        ),
        Ok(present) => {
            for slug in present {
                if !keep_slugs.contains(&slug) {
                    eprintln!("[rq-isolation] evicting foreign skill: {slug}");
                    if let Err(e) = seed::remove(SkillScope::Global, &slug) {
                        eprintln!(
                            "[rq-isolation] WARN: failed to evict {slug}: {e} \
                             — scope may still be contaminated"
                        );
                    }
                }
            }
        }
    }
}

/// Polls Postgres until the `skills` table contains exactly `expected_count` rows,
/// or fails loudly when the deadline passes.
///
/// This is the convergence proof for the isolation step: once the graph-builder
/// reconciles all additions and deletions from the eviction+seeding step, the
/// `skills` count must equal `expected_count`.  A count mismatch means the graph
/// still reflects foreign or stale skills and the measurement would be invalid.
///
/// # Failure mode
/// Returns `Err` with a diagnostic when the deadline passes without convergence.
/// The caller MUST treat this as a hard failure — measuring against a dirty graph
/// produces meaningless numbers that would be reported as quality problems.
async fn poll_graph_skill_count(expected_count: i64, timeout: Duration) -> Result<(), String> {
    let pg = PgObserver::connect().await;
    let deadline = Instant::now() + timeout;
    let interval = Duration::from_millis(1_000);

    loop {
        let count = pg
            .row_count("skills")
            .await
            .map_err(|e| format!("poll_graph_skill_count: DB query failed: {e}"))?;

        if count == expected_count {
            return Ok(());
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "graph skill count did not converge to {expected_count} within {}s \
                 (current count = {count}). The graph-builder has not reconciled the \
                 eviction+seeding step. Aborting measurement to avoid reporting \
                 contaminated numbers.",
                timeout.as_secs()
            ));
        }

        tokio::time::sleep(interval).await;
    }
}

/// Seeds every corpus skill into the global scope under `{ns}-{id}`, approves
/// each, registers it in `guard`, then waits for the graph to rebuild so the
/// snapshot serves exactly this corpus.
///
/// Must be called inside the `RQ_MEASUREMENT_LOCK` critical section so the
/// eviction and seeding steps are atomic with respect to concurrent tests.
///
/// # Isolation guarantee
/// Before seeding, calls `evict_foreign_global_skills` to remove any directories
/// in the global-skills volume that are NOT part of this run's corpus.  Then
/// polls `poll_graph_skill_count` until the DB `skills` table reflects exactly
/// `corpus.skills.len()` rows.  If convergence fails within `timeout`, panics
/// loudly — the measurement must not proceed against a dirty graph.
async fn seed_isolated_corpus_and_wait(
    ns: &str,
    corpus: &LabeledCorpus,
    guard: &mut SeededSkillGuard,
    timeout: Duration,
) {
    let expected_slugs = corpus_slugs(ns, corpus);
    let expected_count = corpus.skills.len() as i64;

    // Step 1: evict every global-skills dir that isn't part of this corpus.
    evict_foreign_global_skills(&expected_slugs);

    // Step 2: read the baseline graph_version so wait_for_rebuild can detect
    // when the graph has processed our seeding.
    let pg = PgObserver::connect().await;
    let prev_version = pg
        .graph_version()
        .await
        .expect("[rq-isolation] must read baseline graph_version before seeding");

    // Step 3: seed the corpus.
    for skill in &corpus.skills {
        let slug = format!("{ns}-{}", skill.id);
        let md = skill.skill_md(&slug);
        seed::write_pending(SkillScope::Global, &slug, &md)
            .unwrap_or_else(|e| panic!("[rq-isolation] seed write_pending({slug}) failed: {e}"));
        seed::approve(SkillScope::Global, &slug)
            .unwrap_or_else(|e| panic!("[rq-isolation] seed approve({slug}) failed: {e}"));
        guard.record(SkillScope::Global, &slug);
    }

    // Step 4: wait for graph-builder to pick up the seeded skills AND reconcile
    // the deletions from the eviction step.
    wait_for_rebuild(prev_version, timeout)
        .await
        .unwrap_or_else(|e| {
            panic!("[rq-isolation] graph did not rebuild after seeding the corpus: {e}")
        });

    // Step 5: assert the graph's skills table shows exactly the corpus size.
    // A surplus means eviction deletions haven't propagated yet; a deficit means
    // some skills failed to embed/ingest.  Either is a hard failure for measurement.
    poll_graph_skill_count(expected_count, Duration::from_secs(60))
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[rq-isolation] graph did not converge to isolated corpus: {e}\n\
                 Current volume contents: {:?}",
                seed::list(SkillScope::Global)
            )
        });

    eprintln!(
        "[rq-isolation] corpus isolated: {expected_count} skills in graph, \
         volume contains only this run's slugs"
    );
}

/// Parses the ranked skill ids out of a compiled `additional_context` markdown.
///
/// The compiler emits one `## Skill: <name>` heading per result in rank order
/// (`crates/compiler/src/template.rs`). Names are namespaced (`{ns}-{id}`); this
/// strips the namespace and keeps only ids belonging to THIS run, so leftover or
/// concurrent skills from other namespaces cannot pollute the measurement.
fn parse_ranked_ids(additional_context: &str, ns: &str) -> Vec<String> {
    let prefix = format!("{ns}-");
    additional_context
        .lines()
        .filter_map(|line| line.trim().strip_prefix("## Skill: "))
        .filter_map(|name| name.trim().strip_prefix(&prefix).map(str::to_owned))
        .collect()
}

/// Runs one query against the live server and returns the ranked base ids it served.
async fn live_ranking(
    client: &McpClient,
    ns: &str,
    query: &LabeledQuery,
) -> (Vec<String>, String, u64) {
    // A monotonic per-process counter guarantees unique session ids within a tight
    // loop so duplicate-suppression never collapses two probe calls.
    static PROBE_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = PROBE_SEQ.fetch_add(1, Ordering::Relaxed);
    let session = format!("{ns}-{}-{seq}", query.id);
    let resp = client
        .compile_context(CompileContextArgs {
            prompt: query.text.clone(),
            session_id: session,
            repo_path: "/tmp".to_owned(),
            trigger: None,
        })
        .await
        .unwrap_or_else(|e| panic!("compile_context failed for query {}: {e}", query.id));

    let ctx = resp.additional_context.clone().unwrap_or_default();
    (parse_ranked_ids(&ctx, ns), resp.status, resp.latency_ms)
}

/// Computes p50/p95 of a latency sample (milliseconds), nearest-rank method.
fn percentiles(mut samples: Vec<u64>) -> (u64, u64) {
    if samples.is_empty() {
        return (0, 0);
    }
    samples.sort_unstable();
    let pick = |p: f64| {
        let rank = ((p * samples.len() as f64).ceil() as usize).max(1);
        samples[rank.min(samples.len()) - 1]
    };
    (pick(0.50), pick(0.95))
}

// ────────────────────────────────────────────────────────────────────────────
// Live tests (require the containerized stack)
// ────────────────────────────────────────────────────────────────────────────

/// Measures aggregate retrieval quality of the shipped pipeline against ground
/// truth and asserts it clears the honest bars in the fixture.
///
/// EXPECTED TO FAIL LOUDLY if retrieval under-delivers — do not relax the
/// thresholds to make it green; fix retrieval.
#[tokio::test]
#[ignore = "requires live containers"]
async fn retrieval_quality_meets_thresholds_on_live_stack() {
    Stack::up().await;
    let corpus = load();
    let ns = run_namespace();
    let logger = StageLogger::new("retrieval-quality");
    let client = McpClient::new();

    // Serialize the seed→measure→cleanup section so sibling tests don't
    // contaminate the global-skills volume while we are measuring.
    let lock = RQ_MEASUREMENT_LOCK.get_or_init(|| Mutex::new(()));
    let _measurement_guard = lock.lock().await;

    let mut seeded_guard = SeededSkillGuard::new();
    seed_isolated_corpus_and_wait(&ns, &corpus, &mut seeded_guard, Duration::from_secs(180)).await;

    // Score every non-negative query (negatives are checked separately).
    let mut results: Vec<(Vec<String>, BTreeSet<String>)> = Vec::new();
    for query in corpus.queries.iter().filter(|q| q.kind != "negative") {
        let (ranked, status, latency) = live_ranking(&client, &ns, query).await;
        let m = query_metrics(&ranked, &query.relevant_set(), K);
        logger.log_stage(
            "query",
            json!({"id": query.id, "kind": query.kind, "text": query.text}),
            json!({
                "status": status,
                "latency_ms": latency,
                "served": ranked,
                "relevant": query.relevant,
                "rr": m.reciprocal_rank,
                "precision_at_1": if ranked.first().map(|id| query.relevant.contains(id)).unwrap_or(false) {1.0} else {0.0},
                "ndcg_at_k": m.ndcg_at_k,
                "hit": m.hit,
            }),
            json!(null),
        );
        results.push((ranked, query.relevant_set()));
    }

    let agg = aggregate(&results, K);
    let t = &corpus.thresholds;

    logger.log_stage(
        "aggregate",
        json!({"k": K, "query_count": agg.query_count}),
        json!({
            "mrr": agg.mrr,
            "map": agg.map,
            "mean_precision_at_1": agg.mean_precision_at_1,
            "mean_precision_at_k": agg.mean_precision_at_k,
            "mean_recall_at_k": agg.mean_recall_at_k,
            "mean_ndcg_at_k": agg.mean_ndcg_at_k,
            "hit_rate": agg.hit_rate,
            "thresholds": {
                "mean_mrr_min": t.mean_mrr_min,
                "mean_precision_at_1_min": t.mean_precision_at_1_min,
                "mean_ndcg_at_3_min": t.mean_ndcg_at_3_min,
            }
        }),
        json!(null),
    );

    let mrr_ok = agg.mrr >= t.mean_mrr_min;
    let p1_ok = agg.mean_precision_at_1 >= t.mean_precision_at_1_min;
    let ndcg_ok = agg.mean_ndcg_at_k >= t.mean_ndcg_at_3_min;

    for (name, ok, got, min) in [
        ("mean_mrr", mrr_ok, agg.mrr, t.mean_mrr_min),
        (
            "mean_precision_at_1",
            p1_ok,
            agg.mean_precision_at_1,
            t.mean_precision_at_1_min,
        ),
        (
            "mean_ndcg_at_3",
            ndcg_ok,
            agg.mean_ndcg_at_k,
            t.mean_ndcg_at_3_min,
        ),
    ] {
        logger.record_contract_assertion(ContractAssertion {
            contract_name: format!("quality::{name}"),
            status: if ok {
                AssertionResult::Passed
            } else {
                AssertionResult::Failed {
                    expected: format!("{name} >= {min}"),
                    actual: format!("{name} = {got:.4}"),
                }
            },
            details: format!("k={K}, queries={}", agg.query_count),
        });
    }

    seeded_guard.cleanup();
    // Drop the measurement lock — the next test in this binary can now enter
    // its own seed→measure→cleanup critical section.
    drop(_measurement_guard);

    let path = logger.emit_report();
    println!("[retrieval-quality] report: {}", path.display());
    println!(
        "[retrieval-quality] MRR={:.3} P@1={:.3} nDCG@{K}={:.3} MAP={:.3} hit_rate={:.3}",
        agg.mrr, agg.mean_precision_at_1, agg.mean_ndcg_at_k, agg.map, agg.hit_rate
    );

    assert!(
        mrr_ok && p1_ok && ndcg_ok,
        "\n=== RETRIEVAL QUALITY BELOW BAR ===\n\
         MRR={:.3} (min {:.2}), P@1={:.3} (min {:.2}), nDCG@{K}={:.3} (min {:.2})\n\
         The shipped pipeline does not retrieve the labeled-relevant skill well enough.\n\
         Do NOT lower the thresholds — investigate scoring weights (α/β/γ), the\n\
         subunit-evidence aggregation, and the relevance threshold.\n\
         Report: {}\n",
        agg.mrr,
        t.mean_mrr_min,
        agg.mean_precision_at_1,
        t.mean_precision_at_1_min,
        agg.mean_ndcg_at_k,
        t.mean_ndcg_at_3_min,
        path.display(),
    );
}

/// The "does semantic beat grep?" test. On lexically-disjoint queries (which
/// share almost no literal tokens with their relevant skill), compares the live
/// semantic ranking's MAP against the in-harness pure-lexical baseline's MAP, and
/// proves each target is served in the top-k where keyword matching misses it.
///
/// This is the e2e counterpart to the #172 unit test — but against REAL
/// `nomic-embed-text` embeddings, so it proves the product thesis, not just the
/// scoring arithmetic.
#[tokio::test]
#[ignore = "requires live containers"]
async fn semantic_retrieval_beats_lexical_baseline_on_disjoint() {
    Stack::up().await;
    let corpus = load();
    let ns = run_namespace();
    let logger = StageLogger::new("retrieval-quality-semantic-vs-lexical");
    let client = McpClient::new();

    let lock = RQ_MEASUREMENT_LOCK.get_or_init(|| Mutex::new(()));
    let _measurement_guard = lock.lock().await;

    let mut seeded_guard = SeededSkillGuard::new();
    seed_isolated_corpus_and_wait(&ns, &corpus, &mut seeded_guard, Duration::from_secs(180)).await;

    let disjoint: Vec<&LabeledQuery> = corpus
        .queries
        .iter()
        .filter(|q| q.kind == "disjoint")
        .collect();

    let mut semantic_results: Vec<(Vec<String>, BTreeSet<String>)> = Vec::new();
    let mut lexical_results: Vec<(Vec<String>, BTreeSet<String>)> = Vec::new();
    let mut semantic_recall_hits = 0usize;

    for query in &disjoint {
        let relevant = query.relevant_set();
        let (semantic_ranked, status, latency) = live_ranking(&client, &ns, query).await;
        let lexical_ranked = lexical_baseline_ranking(&query.text, &corpus.skills);

        let semantic_top_k: Vec<String> = semantic_ranked.iter().take(K).cloned().collect();
        let lexical_top_k: Vec<String> = lexical_ranked.iter().take(K).cloned().collect();
        let semantic_found = semantic_top_k.iter().any(|id| relevant.contains(id));
        let lexical_found = lexical_top_k.iter().any(|id| relevant.contains(id));
        if semantic_found {
            semantic_recall_hits += 1;
        }

        logger.log_stage(
            "disjoint_query",
            json!({"id": query.id, "text": query.text, "relevant": query.relevant}),
            json!({
                "status": status,
                "latency_ms": latency,
                "semantic_top_k": semantic_top_k,
                "lexical_top_k": lexical_top_k,
                "semantic_found": semantic_found,
                "lexical_found": lexical_found,
                "lexical_rank_of_target": lexical_ranked.iter().position(|id| relevant.contains(id)),
            }),
            json!(null),
        );

        semantic_results.push((semantic_ranked, relevant.clone()));
        lexical_results.push((lexical_ranked, relevant));
    }

    let semantic_map = aggregate(&semantic_results, K).map;
    let lexical_map = aggregate(&lexical_results, K).map;
    let disjoint_recall = semantic_recall_hits as f64 / disjoint.len().max(1) as f64;
    let t = &corpus.thresholds;

    logger.log_stage(
        "comparison",
        json!({"disjoint_queries": disjoint.len()}),
        json!({
            "semantic_map": semantic_map,
            "lexical_baseline_map": lexical_map,
            "semantic_disjoint_recall_at_k": disjoint_recall,
            "recall_min": t.disjoint_recall_at_3_min,
        }),
        json!(null),
    );

    let beats = !t.semantic_must_beat_lexical_map_on_disjoint || semantic_map >= lexical_map;
    let recall_ok = disjoint_recall >= t.disjoint_recall_at_3_min;

    logger.record_contract_assertion(ContractAssertion {
        contract_name: "quality::semantic_beats_lexical_on_disjoint".to_owned(),
        status: if beats {
            AssertionResult::Passed
        } else {
            AssertionResult::Failed {
                expected: format!(
                    "semantic MAP ({semantic_map:.4}) >= lexical MAP ({lexical_map:.4})"
                ),
                actual: "lexical baseline matched or beat the semantic pipeline".to_owned(),
            }
        },
        details: "real nomic-embed-text vs token-overlap baseline on lexically-disjoint queries"
            .to_owned(),
    });
    logger.record_contract_assertion(ContractAssertion {
        contract_name: "quality::disjoint_recall_at_k".to_owned(),
        status: if recall_ok {
            AssertionResult::Passed
        } else {
            AssertionResult::Failed {
                expected: format!("disjoint recall@{K} >= {}", t.disjoint_recall_at_3_min),
                actual: format!("recall@{K} = {disjoint_recall:.3}"),
            }
        },
        details: format!(
            "{semantic_recall_hits}/{} disjoint targets served in top-{K}",
            disjoint.len()
        ),
    });

    seeded_guard.cleanup();
    drop(_measurement_guard);

    let path = logger.emit_report();
    println!(
        "[semantic-vs-lexical] semantic MAP={semantic_map:.3} lexical MAP={lexical_map:.3} disjoint recall@{K}={disjoint_recall:.3} report={}",
        path.display()
    );

    assert!(
        beats && recall_ok,
        "\n=== SEMANTIC RETRIEVAL DID NOT BEAT KEYWORD MATCHING ===\n\
         semantic MAP={semantic_map:.4} vs lexical baseline MAP={lexical_map:.4}\n\
         disjoint recall@{K}={disjoint_recall:.3} (min {:.2})\n\
         On lexically-disjoint queries the semantic β term must surface the right\n\
         skill where token overlap cannot. If this fails, real embeddings are not\n\
         bridging meaning — the product's core thesis is unproven. Report: {}\n",
        t.disjoint_recall_at_3_min,
        path.display(),
    );
}

/// Negative queries (about topics absent from the corpus) must NOT fabricate a
/// match: the system should return `no_match` rather than serve a spurious skill
/// above threshold. Measures the false-match rate and asserts it stays at/below
/// the fixture bar.
#[tokio::test]
#[ignore = "requires live containers"]
async fn negative_queries_do_not_fabricate_matches() {
    Stack::up().await;
    let corpus = load();
    let ns = run_namespace();
    let logger = StageLogger::new("retrieval-quality-negatives");
    let client = McpClient::new();

    let lock = RQ_MEASUREMENT_LOCK.get_or_init(|| Mutex::new(()));
    let _measurement_guard = lock.lock().await;

    let mut seeded_guard = SeededSkillGuard::new();
    seed_isolated_corpus_and_wait(&ns, &corpus, &mut seeded_guard, Duration::from_secs(180)).await;

    let negatives: Vec<&LabeledQuery> = corpus
        .queries
        .iter()
        .filter(|q| q.kind == "negative")
        .collect();
    let mut false_matches = 0usize;

    for query in &negatives {
        let (ranked, status, latency) = live_ranking(&client, &ns, query).await;
        // Any skill from THIS namespace served for an off-topic query is a fabricated match.
        let fabricated = !ranked.is_empty();
        if fabricated {
            false_matches += 1;
        }
        logger.log_stage(
            "negative_query",
            json!({"id": query.id, "text": query.text}),
            json!({"status": status, "latency_ms": latency, "served": ranked, "fabricated_match": fabricated}),
            json!(null),
        );
    }

    let false_match_rate = false_matches as f64 / negatives.len().max(1) as f64;
    let t = &corpus.thresholds;
    let ok = false_match_rate <= t.negative_max_false_match_rate;

    logger.record_contract_assertion(ContractAssertion {
        contract_name: "quality::negative_false_match_rate".to_owned(),
        status: if ok {
            AssertionResult::Passed
        } else {
            AssertionResult::Failed {
                expected: format!("false_match_rate <= {}", t.negative_max_false_match_rate),
                actual: format!(
                    "false_match_rate = {false_match_rate:.3} ({false_matches}/{})",
                    negatives.len()
                ),
            }
        },
        details: "off-topic queries must return no_match, not a spurious skill".to_owned(),
    });

    seeded_guard.cleanup();
    drop(_measurement_guard);

    let path = logger.emit_report();
    println!(
        "[negatives] false_match_rate={false_match_rate:.3} report={}",
        path.display()
    );

    assert!(
        ok,
        "\n=== SYSTEM FABRICATES MATCHES FOR OFF-TOPIC QUERIES ===\n\
         false_match_rate={false_match_rate:.3} (max {:.2}); {false_matches}/{} negatives served a skill.\n\
         The relevance threshold is too low — off-topic prompts cross it and inject\n\
         irrelevant context. Raise/justify relevance_threshold. Report: {}\n",
        t.negative_max_false_match_rate,
        negatives.len(),
        path.display(),
    );
}

/// Honest SLO probe: warm the snapshot, fire a burst of `compile_context` calls
/// over real HTTP, and assert the server-reported p95 latency stays within the
/// README's <500ms budget. EXPECTED to surface the real number — if p95 exceeds
/// budget the test fails loudly rather than letting the claim stand unverified.
#[tokio::test]
#[ignore = "requires live containers"]
async fn compile_context_latency_p50_p95_within_budget() {
    Stack::up().await;
    let corpus = load();
    let ns = run_namespace();
    let logger = StageLogger::new("retrieval-quality-latency");
    let client = McpClient::new();

    let lock = RQ_MEASUREMENT_LOCK.get_or_init(|| Mutex::new(()));
    let _measurement_guard = lock.lock().await;

    let mut seeded_guard = SeededSkillGuard::new();
    seed_isolated_corpus_and_wait(&ns, &corpus, &mut seeded_guard, Duration::from_secs(180)).await;

    // Warm the pipeline (snapshot + any lazy init) before measuring.
    let warm = &corpus.queries[0];
    for _ in 0..3 {
        let _ = live_ranking(&client, &ns, warm).await;
    }

    // Measure both server-reported and wall-clock latency over a burst.
    let prompts: Vec<&LabeledQuery> = corpus
        .queries
        .iter()
        .filter(|q| q.kind != "negative")
        .collect();
    let mut server_ms: Vec<u64> = Vec::new();
    let mut wall_ms: Vec<u64> = Vec::new();
    let rounds = 4;
    for _ in 0..rounds {
        for query in &prompts {
            let start = Instant::now();
            let (_ranked, _status, latency) = live_ranking(&client, &ns, query).await;
            wall_ms.push(start.elapsed().as_millis() as u64);
            server_ms.push(latency);
        }
    }

    let (server_p50, server_p95) = percentiles(server_ms.clone());
    let (wall_p50, wall_p95) = percentiles(wall_ms.clone());
    let budget = corpus.thresholds.latency_p95_ms_budget;

    logger.log_stage(
        "latency",
        json!({"samples": server_ms.len(), "budget_ms": budget}),
        json!({
            "server_p50_ms": server_p50, "server_p95_ms": server_p95,
            "wall_p50_ms": wall_p50, "wall_p95_ms": wall_p95,
        }),
        json!(null),
    );

    let within = server_p95 <= budget;
    logger.record_contract_assertion(ContractAssertion {
        contract_name: "quality::latency_p95_within_budget".to_owned(),
        status: if within {
            AssertionResult::Passed
        } else {
            AssertionResult::Failed {
                expected: format!("server p95 <= {budget}ms"),
                actual: format!("server p95 = {server_p95}ms (wall p95 = {wall_p95}ms)"),
            }
        },
        details: format!("{} samples over {rounds} rounds", server_ms.len()),
    });

    seeded_guard.cleanup();
    drop(_measurement_guard);

    let path = logger.emit_report();
    println!(
        "[latency] server p50={server_p50}ms p95={server_p95}ms | wall p50={wall_p50}ms p95={wall_p95}ms | budget={budget}ms report={}",
        path.display()
    );

    assert!(
        within,
        "\n=== compile_context p95 EXCEEDS THE 500ms BUDGET ===\n\
         server p95={server_p95}ms (budget {budget}ms), wall p95={wall_p95}ms.\n\
         The README sells <500ms warm. The dominant cost is the per-request prompt\n\
         embedding (real Ollama). Either cache/short-circuit the embedding for warm\n\
         repeats, move it off the hot path, or correct the public latency claim.\n\
         Report: {}\n",
        path.display(),
    );
}
