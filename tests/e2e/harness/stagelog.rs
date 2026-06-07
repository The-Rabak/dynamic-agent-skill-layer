/// Per-run, per-stage durable file logger for the real-infra E2E harness.
///
/// Every test run creates a directory under the run-scoped report root.
/// When the `E2E_RUN_REPORT_DIR` environment variable is set (always the case
/// when invoked through `scripts/run-e2e-tests.sh`), all output lands under
/// that directory, making each run's artifacts fully isolated from all prior
/// runs.  When the variable is absent (a developer running a single
/// `cargo test … -- --ignored` directly), output falls back to the canonical
/// `tests/e2e/reports/` directory so the dev loop is not broken.
///
/// For each pipeline stage, `log_stage` writes:
///   - `NN-<name>.json` — full input, output, infra snapshot, and RFC3339 timestamp.
///   - appends a human-readable section to `<scenario>.md`.
///
/// At the end of a run, `emit_report` serialises an `E2EReport` (the existing
/// `report.rs` schema) to `<run_report_root>/<scenario>__<test_id>.json`
/// (the flat file the aggregator globs) and also copies it to
/// `<run_report_root>/<run_id>/report.json` for tree navigation.
///
/// # Path layout — with `E2E_RUN_REPORT_DIR` set (wrapper script always sets this)
/// ```
/// $E2E_RUN_REPORT_DIR/
///   <run_id>/
///     <scenario>/
///       00-ingest_input.json
///       01-approval.json
///       ...
///       <scenario>.md
///     report.json              ← per-run copy for tree navigation
///   <scenario>__<test_id>.json ← flat file for the aggregator glob
/// ```
///
/// # Fallback layout — no `E2E_RUN_REPORT_DIR` (developer direct cargo test)
/// ```
/// tests/e2e/reports/
///   <run_id>/
///     <scenario>/…
///     report.json
///   <scenario>__<test_id>.json
/// ```
use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::super::report::{ContractAssertion, ReportBuilder, ReportedAction};

/// A single stage log entry persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageEntry {
    /// RFC3339 timestamp when this stage was logged.
    pub timestamp: String,
    /// Stage name (e.g. `"ingest_input"`, `"approval"`, `"snapshot_swap"`).
    pub stage: String,
    /// Full input to this stage (request body, file content, etc.).
    pub input: Value,
    /// Full output from this stage (response body, observed values, etc.).
    pub output: Value,
    /// Point-in-time snapshot of PG / Qdrant / Redis at this stage.
    pub infra_snapshot: Value,
}

/// Per-run, per-scenario stage logger.
///
/// Construct with [`StageLogger::new`] at the start of each test and call
/// [`StageLogger::log_stage`] for each pipeline stage.  At the end of the test,
/// call [`StageLogger::emit_report`] to write the summary `E2EReport` JSON.
pub struct StageLogger {
    /// Root directory where flat `<scenario>__<test_id>.json` reports are written.
    ///
    /// Set to `E2E_RUN_REPORT_DIR` when that variable is present; otherwise
    /// falls back to the canonical `tests/e2e/reports/` tree.
    run_report_root: PathBuf,
    /// Unique identifier for this run (e.g. `golden-path-20260604-123456789`).
    run_id: String,
    /// Scenario name (e.g. `"golden-path"`).
    scenario: String,
    /// Per-stage directory: `<run_report_root>/<run_id>/<scenario>/`.
    stage_dir: PathBuf,
    /// Counter for naming stage files `00-`, `01-`, etc.
    stage_counter: Arc<Mutex<usize>>,
    /// Report builder accumulates sections and contract assertions.
    report_builder: Arc<Mutex<ReportBuilder>>,
    /// Start time of this run.
    started_at: std::time::Instant,
}

impl StageLogger {
    /// Creates a new logger for `scenario`.
    ///
    /// When `E2E_RUN_REPORT_DIR` is set in the process environment, all output
    /// is written under that directory.  The variable must point to a path that
    /// either already exists or can be created; if the directory cannot be
    /// created the process panics immediately with a clear message (fail-loud,
    /// not silent fallback).
    ///
    /// When `E2E_RUN_REPORT_DIR` is absent the logger falls back to the
    /// canonical `tests/e2e/reports/` directory resolved via `CARGO_MANIFEST_DIR`.
    ///
    /// The `<run_id>/<scenario>/` directory tree is created before this function
    /// returns.
    pub fn new(scenario: &str) -> Self {
        let run_report_root = Self::resolve_run_report_root();

        let now = chrono::Utc::now();
        let run_id = format!("{scenario}-{}", now.format("%Y%m%d-%H%M%S-%3f"));

        let stage_dir = run_report_root.join(&run_id).join(scenario);
        fs::create_dir_all(&stage_dir).unwrap_or_else(|e| {
            panic!(
                "StageLogger: cannot create stage directory {stage_dir:?}: {e}. \
                 Check that E2E_RUN_REPORT_DIR (if set) is writable.",
            );
        });

        Self {
            run_report_root,
            run_id,
            scenario: scenario.to_owned(),
            stage_dir,
            stage_counter: Arc::new(Mutex::new(0)),
            report_builder: Arc::new(Mutex::new(ReportBuilder::new(scenario))),
            started_at: std::time::Instant::now(),
        }
    }

    /// Resolves the directory under which all flat `*.json` report files are
    /// written for this run.
    ///
    /// Priority:
    /// 1. `E2E_RUN_REPORT_DIR` (absolute or repo-relative) — set by the
    ///    wrapper script to keep each run's artifacts isolated.
    /// 2. `CARGO_MANIFEST_DIR/../../tests/e2e/reports` — the historical
    ///    fallback for bare `cargo test` invocations.
    ///
    /// Panics loudly if `E2E_RUN_REPORT_DIR` is set but its value cannot be
    /// resolved to a creatable directory — a malformed variable must not
    /// silently fall through to the broad shared directory.
    fn resolve_run_report_root() -> PathBuf {
        if let Ok(raw) = env::var("E2E_RUN_REPORT_DIR") {
            let path = PathBuf::from(&raw);
            // Accept absolute paths directly; make repo-relative paths absolute.
            let resolved = if path.is_absolute() {
                path
            } else {
                // Resolve relative to the current working directory, which for
                // `cargo test` is the workspace root.
                env::current_dir()
                    .expect("StageLogger: cannot determine cwd for E2E_RUN_REPORT_DIR resolution")
                    .join(path)
            };
            fs::create_dir_all(&resolved).unwrap_or_else(|e| {
                panic!(
                    "StageLogger: E2E_RUN_REPORT_DIR={raw:?} exists but cannot be created: {e}. \
                     Fix the path or directory permissions before running E2E tests."
                );
            });
            return resolved;
        }

        // Fallback: canonical reports dir relative to the crate that owns the test.
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/e2e/reports")
            .canonicalize()
            .unwrap_or_else(|_| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/reports")
            })
    }

    /// Logs a single pipeline stage to disk.
    ///
    /// Writes `NN-<name>.json` to the stage directory and appends a
    /// human-readable section to `<scenario>.md`.
    ///
    /// `input`, `output`, and `infra_snapshot` are any JSON-serializable values.
    /// Pass `serde_json::Value::Null` for any field that does not apply to this stage.
    pub fn log_stage(
        &self,
        name: &str,
        input: impl Serialize,
        output: impl Serialize,
        infra_snapshot: impl Serialize,
    ) {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let counter = {
            let mut c = self.stage_counter.lock().expect("stage counter lock");
            let val = *c;
            *c += 1;
            val
        };

        let entry = StageEntry {
            timestamp: timestamp.clone(),
            stage: name.to_owned(),
            input: serde_json::to_value(&input).unwrap_or(Value::Null),
            output: serde_json::to_value(&output).unwrap_or(Value::Null),
            infra_snapshot: serde_json::to_value(&infra_snapshot).unwrap_or(Value::Null),
        };

        let json_path = self.stage_dir.join(format!("{counter:02}-{name}.json"));
        let json_text = serde_json::to_string_pretty(&entry).expect("StageEntry should serialize");
        fs::write(&json_path, &json_text).unwrap_or_else(|e| {
            eprintln!("StageLogger: failed to write {json_path:?}: {e}");
        });

        let md_path = self.stage_dir.join(format!("{}.md", self.scenario));
        let md_section = format_md_section(counter, name, &timestamp, &entry);
        let mut md_text = if md_path.exists() {
            fs::read_to_string(&md_path).unwrap_or_default()
        } else {
            format!(
                "# Stage Log: {}\n\nRun ID: `{}`\n\n",
                self.scenario, self.run_id
            )
        };
        md_text.push_str(&md_section);
        fs::write(&md_path, &md_text).unwrap_or_else(|e| {
            eprintln!("StageLogger: failed to write {md_path:?}: {e}");
        });
    }

    /// Records a contract assertion into the underlying `ReportBuilder`.
    ///
    /// This feeds `emit_report`'s overall outcome calculation.
    pub fn record_contract_assertion(&self, assertion: ContractAssertion) {
        if let Ok(mut builder) = self.report_builder.lock() {
            builder.add_contract_assertion(assertion);
        }
    }

    /// Convenience wrapper around `record_contract_assertion`.
    ///
    /// Returns `passed` so callers can chain: `assert!(logger.assert_contract(…));`.
    pub fn assert_contract(
        &self,
        name: &str,
        passed: bool,
        expected: &str,
        actual: &str,
        details: &str,
    ) -> bool {
        if let Ok(mut builder) = self.report_builder.lock() {
            builder.assert_contract(name, passed, expected, actual, details);
        }
        passed
    }

    /// Records a `ReportedAction` into the named section of the report builder.
    pub fn record_action(&self, section: &str, action: ReportedAction) {
        if let Ok(mut builder) = self.report_builder.lock() {
            builder.push_action(section, action);
        }
    }

    /// Builds the `E2EReport` and writes it to
    /// `<run_report_root>/<scenario>__<test_id>.json`.
    ///
    /// The flat file path is what the aggregator glob (`$E2E_RUN_REPORT_DIR/**/*.json`)
    /// picks up.  A copy is also written to `<run_report_root>/<run_id>/report.json`
    /// for per-run tree navigation.
    ///
    /// Returns the path to the flat report file.
    pub fn emit_report(self) -> PathBuf {
        let builder = Arc::try_unwrap(self.report_builder)
            .expect("StageLogger: should be the sole owner of report_builder at emit time")
            .into_inner()
            .expect("StageLogger: report_builder mutex should not be poisoned");

        let report = builder.build();
        let report_filename = format!("{}__{}.json", self.scenario, report.test_id);
        let report_path = self.run_report_root.join(&report_filename);

        let report_json =
            serde_json::to_string_pretty(&report).expect("E2EReport should serialize");
        fs::write(&report_path, &report_json).unwrap_or_else(|e| {
            eprintln!("StageLogger: failed to write report {report_path:?}: {e}");
        });

        // Write a copy in the run_id directory for tree navigation.
        let run_report_copy = self
            .stage_dir
            .parent()
            .unwrap_or(&self.run_report_root)
            .join("report.json");
        fs::write(&run_report_copy, &report_json).unwrap_or_default();

        report_path
    }

    /// Returns the path to the per-stage directory.
    pub fn stage_dir(&self) -> &Path {
        &self.stage_dir
    }

    /// Returns the run identifier.
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Returns elapsed time since logger construction.
    pub fn elapsed_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }
}

/// Formats a single stage entry as a Markdown section.
fn format_md_section(counter: usize, name: &str, timestamp: &str, entry: &StageEntry) -> String {
    let input_pretty = serde_json::to_string_pretty(&entry.input).unwrap_or_default();
    let output_pretty = serde_json::to_string_pretty(&entry.output).unwrap_or_default();
    let infra_pretty = serde_json::to_string_pretty(&entry.infra_snapshot).unwrap_or_default();

    format!(
        r#"
## Stage {counter:02}: `{name}`

**Timestamp:** {timestamp}

### Input
```json
{input_pretty}
```

### Output
```json
{output_pretty}
```

### Infra Snapshot
```json
{infra_pretty}
```

---
"#,
    )
}
