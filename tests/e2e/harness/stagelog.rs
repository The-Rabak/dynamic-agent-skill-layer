/// Per-run, per-stage durable file logger for the real-infra E2E harness.
///
/// Every test run creates a directory under `tests/e2e/reports/<run_id>/<scenario>/`.
/// For each pipeline stage, `log_stage` writes:
///   - `NN-<name>.json` — full input, output, infra snapshot, and RFC3339 timestamp.
///   - appends a human-readable section to `<scenario>.md`.
///
/// At the end of a run, `emit_report` serialises an `E2EReport` (the existing
/// `report.rs` schema) to `tests/e2e/reports/<scenario>__<YYYYMMDDHHMMSS>.json`
/// and also links the per-stage tree under `<run_id>/`.
///
/// # Path layout
/// ```
/// tests/e2e/reports/
///   <run_id>/                          ← per-run tree
///     <scenario>/
///       00-ingest_input.json
///       01-approval.json
///       ...
///       <scenario>.md
///   <scenario>__<YYYYMMDDHHMMSS>.json  ← E2EReport for the aggregator
/// ```
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::super::report::{
    ContractAssertion, ReportBuilder, ReportedAction,
};

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
    /// Root `tests/e2e/reports/` directory.
    reports_root: PathBuf,
    /// Unique identifier for this run (e.g. `golden-path-20260604-123456789`).
    run_id: String,
    /// Scenario name (e.g. `"golden-path"`).
    scenario: String,
    /// Per-stage directory: `reports/<run_id>/<scenario>/`.
    stage_dir: PathBuf,
    /// Counter for naming stage files `00-`, `01-`, etc.
    stage_counter: Arc<Mutex<usize>>,
    /// Report builder accumulates sections and contract assertions.
    report_builder: Arc<Mutex<ReportBuilder>>,
    /// Start time of this run.
    started_at: std::time::Instant,
}

impl StageLogger {
    /// Creates a new logger for `scenario`, rooted under the canonical
    /// `tests/e2e/reports/` directory (resolved relative to `CARGO_MANIFEST_DIR`).
    ///
    /// The `<run_id>/<scenario>/` directory tree is created immediately.
    pub fn new(scenario: &str) -> Self {
        let reports_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/e2e/reports")
            .canonicalize()
            .unwrap_or_else(|_| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/reports")
            });

        let now = chrono::Utc::now();
        let run_id = format!("{scenario}-{}", now.format("%Y%m%d-%H%M%S-%3f"));

        let stage_dir = reports_root.join(&run_id).join(scenario);
        fs::create_dir_all(&stage_dir).expect("StageLogger: should create stage directory");

        Self {
            reports_root,
            run_id,
            scenario: scenario.to_owned(),
            stage_dir,
            stage_counter: Arc::new(Mutex::new(0)),
            report_builder: Arc::new(Mutex::new(ReportBuilder::new(scenario))),
            started_at: std::time::Instant::now(),
        }
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
    /// `tests/e2e/reports/<scenario>__<YYYYMMDDHHMMSS>.json`.
    ///
    /// Returns the path to the written report file.
    pub fn emit_report(self) -> PathBuf {
        let builder = Arc::try_unwrap(self.report_builder)
            .expect("StageLogger: should be the sole owner of report_builder at emit time")
            .into_inner()
            .expect("StageLogger: report_builder mutex should not be poisoned");

        let report = builder.build();
        let report_filename = format!("{}__{}.json", self.scenario, report.test_id);
        let report_path = self.reports_root.join(&report_filename);

        let report_json =
            serde_json::to_string_pretty(&report).expect("E2EReport should serialize");
        fs::write(&report_path, &report_json).unwrap_or_else(|e| {
            eprintln!("StageLogger: failed to write report {report_path:?}: {e}");
        });

        // Write a symlink-style reference in the run_id directory pointing to the
        // flat report (informational only; not all platforms support symlinks).
        let run_report_path = self
            .stage_dir
            .parent()
            .unwrap_or(&self.reports_root)
            .join("report.json");
        fs::write(&run_report_path, &report_json).unwrap_or_default();

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
