use std::time::Duration;

use async_trait::async_trait;
use domain::{
    ExtractionError, ExtractionResult, SessionTranscript, TranscriptSkillExtractionService,
};
use tokio::io::AsyncWriteExt as _;
use tokio::process::Command;
use tokio::time::timeout;

use crate::extraction::{
    limits::{validate_extraction_config, validate_transcript_limits},
    prompt_contract::build_ollama_extraction_prompt,
};

// # ClaudeCodeExtractor — Claude Code CLI Subprocess Adapter
//
// ClaudeCodeExtractor shells out to the local `claude` CLI in headless mode
// (`claude -p --output-format json ...`), feeding the extraction prompt through
// stdin to avoid OS argument-length limits on large transcripts. It is the
// subscription-based alternative to `ClaudeExtractor` (Anthropic Messages API):
// no ANTHROPIC_API_KEY is needed — the CLI uses the user's Claude Code login.
//
// Prompt strategy: reuses `build_ollama_extraction_prompt` (text→JSON), the same
// function used by OllamaExtractor. Both the Ollama and Claude-Code providers
// follow the same text-in/JSON-out pattern, so the shared prompt is the correct
// owner rather than the forced `tool_use` strategy of `ClaudeExtractor`.
//
// **Environment constraint:** This adapter does NOT read, store, or pass any
// credentials. It only spawns the `claude` binary, which uses whatever login
// already exists in its environment (`~/.claude`). The requirement is simply that
// the `claude` CLI is installed and already authenticated where this process runs —
// true on a host where `claude` has been used interactively, but NOT in the stock
// compose container (no CLI, no login). In containerised deployments use
// `EXTRACT_SESSION_PROVIDER=claude-api` (API key) or `=ollama` (local). The compose
// default remains `ollama`.
//
// See `extraction/mod.rs` for the full prompt strategy rationale.

/// Default extraction model shared by both Claude paths. Overridable via `EXTRACT_SESSION_MODEL`.
pub(crate) const DEFAULT_CLAUDE_MODEL: &str = "claude-sonnet-4-6";

/// Default inner timeout (ms). Local CLI + cloud inference; 120s mirrors the
/// Ollama CPU-inference default and gives the model ample time to respond.
const DEFAULT_TIMEOUT_MS: u64 = 120_000;

/// JSON-only system-prompt enforcer passed via `--system-prompt` to prevent the
/// model from prefixing its reply with prose.
const JSON_ENFORCER_SYSTEM_PROMPT: &str = "Respond with valid JSON only. Do not include any prose, markdown code fences, or explanation outside the JSON object.";

/// Tools to block so the headless call is a pure text completion that never
/// triggers an interactive permission prompt.
const DISALLOWED_TOOLS: &str =
    "Bash,Edit,Write,Read,WebFetch,WebSearch,Task,Glob,Grep,NotebookEdit,TodoWrite";

#[derive(Debug, Clone)]
pub struct ClaudeCodeExtractionConfig {
    /// Path to the `claude` CLI binary. Defaults to `claude` (resolved via `$PATH`).
    /// Override via `CLAUDE_CLI_PATH`.
    pub cli_path: String,
    /// Model identifier to pass with `--model`. Default: `claude-sonnet-4-6`.
    /// Override via `EXTRACT_SESSION_MODEL`.
    pub model: String,
    /// Inner per-call timeout in milliseconds. Default: 120 000.
    /// Override via `CLAUDE_CODE_EXTRACTION_TIMEOUT_MS`.
    pub timeout_ms: u64,
    pub max_entries: usize,
    pub max_entry_chars: usize,
    pub max_total_chars: usize,
}

impl Default for ClaudeCodeExtractionConfig {
    fn default() -> Self {
        Self {
            cli_path: "claude".to_owned(),
            model: DEFAULT_CLAUDE_MODEL.to_owned(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_entries: 2_000,
            max_entry_chars: 8_192,
            max_total_chars: 1_000_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClaudeCodeExtractor {
    config: ClaudeCodeExtractionConfig,
}

impl ClaudeCodeExtractor {
    /// Builds a `ClaudeCodeExtractor`.
    ///
    /// Fails loudly at construction time when the configured limits are invalid.
    /// The CLI binary is NOT probed here (its absence surfaces at extraction time
    /// as `ExtractionError::ProviderUnavailable` — a deliberate fail-at-use-time
    /// choice matching the Ollama provider's pattern of not pinging the endpoint
    /// on construction).
    pub fn new(config: ClaudeCodeExtractionConfig) -> Result<Self, ExtractionError> {
        if config.cli_path.trim().is_empty() || config.model.trim().is_empty() {
            return Err(ExtractionError::InvalidTranscript(
                "ClaudeCodeExtractor: cli_path and model must not be blank".to_owned(),
            ));
        }

        validate_extraction_config(
            config.timeout_ms,
            config.max_entries,
            config.max_entry_chars,
            config.max_total_chars,
        )?;

        Ok(Self { config })
    }
}

/// Parsed shape of the `--output-format json` envelope emitted by the `claude` CLI.
///
/// Confirmed envelope shape from a real run:
/// `{"type":"result","subtype":"success","is_error":false,"result":"…","session_id":"…"}`
#[derive(Debug, serde::Deserialize)]
struct ClaudeCliEnvelope {
    #[serde(rename = "type")]
    envelope_type: String,
    #[serde(default)]
    subtype: String,
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    result: String,
}

/// Structured JSON body expected inside the `result` field after fence stripping.
#[derive(Debug, serde::Deserialize)]
struct CandidatesBody {
    #[serde(default)]
    candidates: Vec<domain::ExtractedSkillCandidate>,
}

#[async_trait]
impl TranscriptSkillExtractionService for ClaudeCodeExtractor {
    async fn extract(
        &self,
        transcript: &SessionTranscript,
    ) -> Result<ExtractionResult, ExtractionError> {
        validate_transcript_limits(
            transcript,
            self.config.max_entries,
            self.config.max_entry_chars,
            self.config.max_total_chars,
        )?;

        let transcript_lines = render_transcript_lines(transcript);
        let prompt = build_ollama_extraction_prompt(&transcript_lines);

        let candidates = timeout(
            Duration::from_millis(self.config.timeout_ms),
            self.invoke_cli(&prompt),
        )
        .await
        .map_err(|_| ExtractionError::Timeout {
            timeout_ms: self.config.timeout_ms,
        })??;

        Ok(ExtractionResult {
            source_session_id: transcript.session_id.clone(),
            candidates,
            provider: "claude-code".to_owned(),
        })
    }
}

impl ClaudeCodeExtractor {
    /// Spawns the `claude` CLI as a subprocess, writes the prompt to stdin, and
    /// parses the JSON envelope from stdout.
    ///
    /// The working directory is set to `std::env::temp_dir()` so the subprocess
    /// does NOT load this repository's `CLAUDE.md` / `.mcp.json` project context,
    /// keeping the extraction call completely context-free.
    async fn invoke_cli(
        &self,
        prompt: &str,
    ) -> Result<Vec<domain::ExtractedSkillCandidate>, ExtractionError> {
        let mut child = Command::new(&self.config.cli_path)
            .args([
                "--print",
                "--output-format",
                "json",
                "--model",
                &self.config.model,
                "--system-prompt",
                JSON_ENFORCER_SYSTEM_PROMPT,
                "--exclude-dynamic-system-prompt-sections",
                "--disallowed-tools",
                DISALLOWED_TOOLS,
            ])
            // Route stdin so we can write the prompt directly.
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // Neutral working directory: do not load project CLAUDE.md / .mcp.json.
            .current_dir(std::env::temp_dir())
            .spawn()
            .map_err(|error| {
                ExtractionError::ProviderUnavailable(format!(
                    "failed to spawn claude CLI ({cli_path:?}): {error}",
                    cli_path = self.config.cli_path
                ))
            })?;

        // Feed the extraction prompt through stdin.
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(prompt.as_bytes()).await.map_err(|error| {
                ExtractionError::ProviderUnavailable(format!(
                    "failed to write prompt to claude CLI stdin: {error}"
                ))
            })?;
            // Explicit close signals EOF to the child process.
        }

        let output = child.wait_with_output().await.map_err(|error| {
            ExtractionError::ProviderUnavailable(format!(
                "error waiting for claude CLI process: {error}"
            ))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ExtractionError::ProviderUnavailable(format!(
                "claude CLI exited with status {}: {stderr}",
                output.status
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_cli_output(&stdout)
    }
}

/// Renders transcript entries as `speaker: content` lines for the shared prompt.
fn render_transcript_lines(transcript: &SessionTranscript) -> String {
    let mut lines = String::new();
    for entry in &transcript.entries {
        lines.push_str(&entry.speaker);
        lines.push_str(": ");
        lines.push_str(&entry.content);
        lines.push('\n');
    }
    lines
}

/// Parses the JSON envelope emitted by `claude --output-format json`.
///
/// On success (`subtype == "success"` and `is_error == false`), the `result`
/// string is unwrapped: markdown code fences (` ```json … ``` ` or ` ``` … ``` `)
/// are stripped, then the first balanced top-level `{ … }` object is extracted
/// and parsed as `{ "candidates": [...] }`.
///
/// On a non-success envelope, returns `ExtractionError::ProviderUnavailable` or
/// `ExtractionError::Unexpected` with the envelope detail so the failure is loud
/// and actionable.
pub(crate) fn parse_cli_output(
    raw_stdout: &str,
) -> Result<Vec<domain::ExtractedSkillCandidate>, ExtractionError> {
    // The CLI may emit multiple newline-delimited JSON objects (e.g. stream-json
    // mode). With `--output-format json` only one envelope is expected, but we
    // search for the last non-empty line in case a newline trailer is present.
    let envelope_json = raw_stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .next_back()
        .ok_or_else(|| {
            ExtractionError::ProviderUnavailable("claude CLI produced no output".to_owned())
        })?;

    let envelope: ClaudeCliEnvelope = serde_json::from_str(envelope_json).map_err(|error| {
        ExtractionError::Unexpected(format!(
            "failed to parse claude CLI output envelope: {error}; raw={envelope_json}"
        ))
    })?;

    if envelope.is_error || envelope.subtype != "success" {
        let detail = if envelope.result.is_empty() {
            format!(
                "type={}, subtype={}",
                envelope.envelope_type, envelope.subtype
            )
        } else {
            format!(
                "type={}, subtype={}, result={}",
                envelope.envelope_type, envelope.subtype, envelope.result
            )
        };
        return Err(ExtractionError::ProviderUnavailable(format!(
            "claude CLI returned a non-success envelope: {detail}"
        )));
    }

    let stripped = strip_markdown_fences(&envelope.result);
    let json_object = extract_first_json_object(stripped).ok_or_else(|| {
        ExtractionError::Unexpected(format!(
            "no JSON object found in claude CLI result: {}",
            envelope.result
        ))
    })?;

    let body: CandidatesBody = serde_json::from_str(json_object).map_err(|error| {
        ExtractionError::Unexpected(format!(
            "failed to parse candidates body from claude CLI result: {error}; json={json_object}"
        ))
    })?;

    Ok(body.candidates)
}

/// Strips leading/trailing markdown code fences from a string.
///
/// Handles both ` ```json\n…\n``` ` and bare ` ```\n…\n``` ` forms. Returns a
/// reference into the original string where possible; trims whitespace from both ends.
fn strip_markdown_fences(s: &str) -> &str {
    let trimmed = s.trim();
    // Try ```json ... ``` first, then generic ``` ... ```.
    for prefix in &["```json", "```"] {
        if let Some(after_open) = trimmed.strip_prefix(prefix)
            && let Some(before_close) = after_open.strip_suffix("```")
        {
            return before_close.trim();
        }
    }
    trimmed
}

/// Extracts the first balanced top-level `{ … }` JSON object from `text`.
///
/// This handles the case where the model wraps the JSON in surrounding prose
/// despite the system-prompt instruction. Only the first complete object is
/// returned; everything before or after is discarded.
fn extract_first_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let slice = &text[start..];
    let mut depth: usize = 0;
    let mut end: Option<usize> = None;
    for (byte_offset, ch) in slice.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    end = Some(byte_offset + ch.len_utf8());
                    break;
                }
            }
            _ => {}
        }
    }
    end.map(|e| &slice[..e])
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{DomainId, TranscriptEntry};

    // --- Config and construction tests ---

    #[test]
    fn default_config_uses_sonnet_and_120s_timeout() {
        let config = ClaudeCodeExtractionConfig::default();
        assert_eq!(
            config.model, "claude-sonnet-4-6",
            "default model must be claude-sonnet-4-6"
        );
        assert_eq!(config.cli_path, "claude");
        assert_eq!(config.timeout_ms, 120_000);
    }

    #[test]
    fn blank_cli_path_fails_at_construction() {
        let config = ClaudeCodeExtractionConfig {
            cli_path: "   ".to_owned(),
            ..ClaudeCodeExtractionConfig::default()
        };
        let error = ClaudeCodeExtractor::new(config).expect_err("blank cli_path must fail loudly");
        assert!(matches!(error, ExtractionError::InvalidTranscript(_)));
    }

    #[test]
    fn blank_model_fails_at_construction() {
        let config = ClaudeCodeExtractionConfig {
            model: String::new(),
            ..ClaudeCodeExtractionConfig::default()
        };
        let error = ClaudeCodeExtractor::new(config).expect_err("blank model must fail loudly");
        assert!(matches!(error, ExtractionError::InvalidTranscript(_)));
    }

    #[tokio::test]
    async fn extract_rejects_empty_transcript() {
        let extractor =
            ClaudeCodeExtractor::new(ClaudeCodeExtractionConfig::default()).expect("valid config");
        let transcript = SessionTranscript {
            session_id: DomainId::new_unchecked("cc-empty"),
            entries: vec![],
        };
        let error = extractor
            .extract(&transcript)
            .await
            .expect_err("empty transcript must fail");
        assert!(matches!(error, ExtractionError::InvalidTranscript(_)));
    }

    // --- Envelope parsing unit tests ---

    #[test]
    fn parse_success_envelope_with_fenced_json_yields_candidates() {
        let fenced_result = "```json\n{\"candidates\": []}\n```".to_owned();
        let envelope = serde_json::json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "result": fenced_result,
        });
        let raw = envelope.to_string();
        let candidates = parse_cli_output(&raw).expect("must parse successfully");
        assert!(candidates.is_empty());
    }

    #[test]
    fn parse_success_envelope_with_one_candidate() {
        let result_json = serde_json::json!({
            "candidates": [{
                "name": "test-skill",
                "description": "A test skill.",
                "tags": ["test"],
                "procedures": ["1. Do the thing."],
                "conventions": [],
                "assets": [],
                "confidence": 0.9
            }]
        })
        .to_string();
        let fenced = format!("```json\n{result_json}\n```");
        let envelope = serde_json::json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "result": fenced,
        });
        let candidates = parse_cli_output(&envelope.to_string()).expect("must parse candidate");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name, "test-skill");
    }

    #[test]
    fn parse_is_error_true_returns_provider_unavailable() {
        let envelope = serde_json::json!({
            "type": "result",
            "subtype": "error",
            "is_error": true,
            "result": "something went wrong",
        });
        let error =
            parse_cli_output(&envelope.to_string()).expect_err("is_error=true must be an error");
        assert!(
            matches!(error, ExtractionError::ProviderUnavailable(_)),
            "got {error:?}"
        );
    }

    #[test]
    fn parse_non_success_subtype_returns_provider_unavailable() {
        let envelope = serde_json::json!({
            "type": "result",
            "subtype": "error_during_execution",
            "is_error": false,
            "result": "",
        });
        let error = parse_cli_output(&envelope.to_string())
            .expect_err("non-success subtype must be an error");
        assert!(matches!(error, ExtractionError::ProviderUnavailable(_)));
    }

    #[test]
    fn strip_fences_handles_json_prefixed_fence() {
        let s = "```json\n{\"key\": 1}\n```";
        assert_eq!(strip_markdown_fences(s), "{\"key\": 1}");
    }

    #[test]
    fn strip_fences_handles_generic_fence() {
        let s = "```\n{\"key\": 1}\n```";
        assert_eq!(strip_markdown_fences(s), "{\"key\": 1}");
    }

    #[test]
    fn strip_fences_is_idempotent_on_plain_json() {
        let s = "{\"candidates\": []}";
        assert_eq!(strip_markdown_fences(s), s);
    }

    #[test]
    fn extract_json_object_finds_first_balanced_brace() {
        let text = "Here is the data: {\"a\": 1} followed by more text.";
        assert_eq!(extract_first_json_object(text), Some("{\"a\": 1}"));
    }

    #[test]
    fn extract_json_object_handles_nested_braces() {
        let text = r#"{"outer": {"inner": 1}}"#;
        assert_eq!(
            extract_first_json_object(text),
            Some(r#"{"outer": {"inner": 1}}"#)
        );
    }

    #[test]
    fn extract_json_object_returns_none_on_no_brace() {
        assert_eq!(extract_first_json_object("no braces here"), None);
    }

    #[test]
    fn parse_prose_wrapped_json_still_extracts_candidates() {
        // Model wraps the JSON in surrounding prose despite instructions.
        let result = "Sure, here are the candidates:\n{\"candidates\": []}\nDone.";
        let envelope = serde_json::json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "result": result,
        });
        let candidates =
            parse_cli_output(&envelope.to_string()).expect("prose-wrapped JSON must parse");
        assert!(candidates.is_empty());
    }

    #[test]
    fn parse_empty_stdout_returns_provider_unavailable() {
        let error = parse_cli_output("").expect_err("empty stdout must be an error");
        assert!(matches!(error, ExtractionError::ProviderUnavailable(_)));
    }

    // --- Real subprocess test (fake CLI script) ---
    //
    // This test writes a small shell script that echoes a canned success envelope
    // to stdout, then exercises the full spawn + parse path without network.

    #[tokio::test]
    async fn fake_cli_subprocess_parses_canned_success_envelope() {
        use std::os::unix::fs::PermissionsExt as _;

        let tmp = std::env::temp_dir().join(format!(
            "fake-claude-{}-{}.sh",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));

        // Canned success envelope matching the confirmed real-run shape.
        let canned_result = r#"{\"candidates\": []}"#;
        let script = format!(
            "#!/bin/sh\ncat > /dev/null\nprintf '{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"{canned_result}\"}}\\n'\n"
        );
        std::fs::write(&tmp, &script).expect("write fake CLI script");
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .expect("make fake CLI executable");

        let config = ClaudeCodeExtractionConfig {
            cli_path: tmp.to_str().unwrap().to_owned(),
            ..ClaudeCodeExtractionConfig::default()
        };
        let extractor = ClaudeCodeExtractor::new(config).expect("valid config");

        let transcript = SessionTranscript {
            session_id: DomainId::new_unchecked("fake-cli-test"),
            entries: vec![TranscriptEntry {
                speaker: "user".to_owned(),
                content: "test content for fake CLI".to_owned(),
            }],
        };

        let result = extractor
            .extract(&transcript)
            .await
            .expect("fake CLI extraction must succeed");

        assert_eq!(result.provider, "claude-code");
        assert!(result.candidates.is_empty());

        // Clean up.
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn fake_cli_subprocess_parses_candidate_from_fenced_result() {
        use std::os::unix::fs::PermissionsExt as _;

        let tmp_dir = std::env::temp_dir().join(format!(
            "fake-claude-cand-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&tmp_dir).expect("create tmp dir");

        // Write the JSON response envelope to a separate file so the shell
        // script can cat it without complex quoting/escaping concerns.
        let response_file = tmp_dir.join("response.json");
        let candidates_inner = serde_json::json!({
            "candidates": [{
                "name": "test-skill",
                "description": "A test skill.",
                "tags": ["test"],
                "procedures": ["1. Do it."],
                "conventions": [],
                "assets": [],
                "confidence": 0.9_f64
            }]
        });
        let fenced_result = format!("```json\n{candidates_inner}\n```");
        let envelope = serde_json::json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "result": fenced_result,
        });
        std::fs::write(&response_file, envelope.to_string()).expect("write response file");

        let script_path = tmp_dir.join("fake-claude.sh");
        let script = format!(
            "#!/bin/sh\ncat > /dev/null\ncat {response_file}\necho\n",
            response_file = response_file.display()
        );
        std::fs::write(&script_path, &script).expect("write fake CLI script");
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .expect("make fake CLI executable");

        let config = ClaudeCodeExtractionConfig {
            cli_path: script_path.to_str().unwrap().to_owned(),
            ..ClaudeCodeExtractionConfig::default()
        };
        let extractor = ClaudeCodeExtractor::new(config).expect("valid config");

        let transcript = SessionTranscript {
            session_id: DomainId::new_unchecked("fake-cli-cand-test"),
            entries: vec![TranscriptEntry {
                speaker: "user".to_owned(),
                content: "test content for candidate parsing".to_owned(),
            }],
        };

        let result = extractor
            .extract(&transcript)
            .await
            .expect("fake CLI extraction with candidate must succeed");

        assert_eq!(result.provider, "claude-code");
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].name, "test-skill");

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    // --- Live integration test (ignore-gated: requires real `claude` CLI) ---

    #[tokio::test]
    #[ignore = "live test: requires the claude CLI authenticated with a subscription"]
    async fn live_claude_cli_extraction_succeeds() {
        let config = ClaudeCodeExtractionConfig::default();
        let extractor = ClaudeCodeExtractor::new(config).expect("valid config");

        let transcript = SessionTranscript {
            session_id: DomainId::new_unchecked("live-cli-test"),
            entries: vec![TranscriptEntry {
                speaker: "user".to_owned(),
                content: "When using git, always write conventional commit messages: type(scope): description. Types: feat, fix, docs, style, refactor, test, chore.".to_owned(),
            }],
        };

        let result = extractor
            .extract(&transcript)
            .await
            .expect("live claude CLI extraction must succeed");

        assert_eq!(result.provider, "claude-code");
        println!(
            "live extraction returned {} candidates",
            result.candidates.len()
        );
    }
}
