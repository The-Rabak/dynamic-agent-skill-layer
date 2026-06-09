//! Diagnostic trace: reproduce the EXACT prose-extraction request the orchestrated
//! pipeline sends to Ollama for a real session window, and capture what gemma
//! actually returns — including the model server's own `prompt_eval_count`.
//!
//! This is a diagnostic harness (an `examples/` binary), NOT production logic. It
//! reuses the REAL production functions end to end — `parse_session_events` (the
//! real transcript parser), `segment_session` (the real episodic segmenter),
//! `events_to_transcript`, `render_sanitized_transcript_lines`, and
//! `build_text_json_extraction_prompt` — so the prompt is byte-for-byte what the
//! prose extractor (`OllamaExtractor::extract`) builds. It then POSTs the exact
//! prod request shape (`format:"json"`, `think:false`, `temperature:0`) to the
//! real Ollama, A/B on `num_ctx`:
//!   - Arm A (CURRENT PROD): no `num_ctx` -> ollama uses its small default.
//!   - Arm B (CANDIDATE FIX): `num_ctx` sized to the window + headroom.
//!
//! The `prompt_eval_count` ollama returns is the smoking gun: if Arm A's count is
//! pinned near a small default while the real prompt is far larger, ollama
//! truncated the input — which makes the model emit malformed/empty JSON. Arm B
//! should evaluate the whole prompt and parse cleanly.
//!
//! Usage:
//!   TRACE_TRANSCRIPT=/path/to/session.jsonl OLLAMA_URL=http://127.0.0.1:11434 \
//!     cargo run -p session-extractor --example ollama_window_trace
//! Optional env: TRACE_MODEL (default gemma4:12b), TRACE_TOKEN_BUDGET (default
//! 8192, the real local tier budget), TRACE_WINDOW (probe one window index;
//! default = the largest window).

use std::env;
use std::io::Write;

use domain::{DomainId, events_to_transcript};
use infrastructure::{build_text_json_extraction_prompt, render_sanitized_transcript_lines};
use serde_json::{Value, json};
use session_extractor::segmentation::{SegmentationConfig, segment_session};
use session_extractor::transcripts::parse_session_events;

#[tokio::main]
async fn main() {
    let transcript_path = env::var("TRACE_TRANSCRIPT")
        .expect("set TRACE_TRANSCRIPT to a real .jsonl session transcript path");
    let ollama_url = env::var("OLLAMA_URL").unwrap_or_else(|_| "http://127.0.0.1:11434".to_owned());
    let endpoint = format!("{}/api/generate", ollama_url.trim_end_matches('/'));
    let model = env::var("TRACE_MODEL").unwrap_or_else(|_| "gemma4:12b".to_owned());
    let token_budget: usize = env::var("TRACE_TOKEN_BUDGET")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8_192);
    let only_window: Option<usize> = env::var("TRACE_WINDOW").ok().and_then(|s| s.parse().ok());

    let payload = std::fs::read_to_string(&transcript_path)
        .unwrap_or_else(|e| panic!("read {transcript_path}: {e}"));

    // ── REAL parse + REAL segmentation (local tier budget) ────────────────────
    let parsed = parse_session_events(&payload);
    let events = parsed.events;
    let config = SegmentationConfig::new(token_budget, 3);
    let windows = segment_session(&events, &config);
    let session_id = DomainId::new_unchecked("trace-session");

    let event_by_index: std::collections::HashMap<usize, &domain::SessionEvent> =
        events.iter().map(|ev| (ev.index(), ev)).collect();

    println!("== ollama_window_trace ==");
    println!("transcript: {transcript_path}");
    println!(
        "events: {}  windows: {}  token_budget: {token_budget}  model: {model}",
        events.len(),
        windows.len()
    );
    println!("endpoint: {endpoint}\n");

    // Build the REAL prose prompt for every window; record sizes.
    let mut built: Vec<(usize, usize, usize, String)> = Vec::new(); // (idx, chars, est_tokens, prompt)
    for (idx, window) in windows.iter().enumerate() {
        let window_events: Vec<domain::SessionEvent> = window
            .event_indices
            .iter()
            .filter_map(|i| event_by_index.get(i).copied().cloned())
            .collect();
        let transcript = events_to_transcript(session_id.clone(), &window_events);
        let lines = render_sanitized_transcript_lines(&transcript);
        let prompt = build_text_json_extraction_prompt(&lines);
        let chars = prompt.chars().count();
        let est_tokens = chars / 4;
        built.push((idx, chars, est_tokens, prompt));
    }

    println!("window sizes (real prose prompt):");
    for (idx, chars, est_tokens, _) in &built {
        println!("  window {idx:>3}: {chars:>7} chars  ~{est_tokens:>6} est tokens");
    }
    println!();

    // Choose targets: a specific window, or the largest (most likely to overflow).
    let targets: Vec<usize> = match only_window {
        Some(i) => vec![i],
        None => {
            let max_idx = built
                .iter()
                .max_by_key(|(_, c, _, _)| *c)
                .map(|(i, _, _, _)| *i)
                .unwrap_or(0);
            vec![max_idx]
        }
    };

    // Cap output tokens for diagnostic speed (the truncation effect is on the
    // INPUT prompt, independent of output length). Default 1024.
    let num_predict: u64 = env::var("TRACE_NUM_PREDICT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1024);

    let client = reqwest::Client::new();
    for target in targets {
        let (idx, chars, est_tokens, prompt) = &built[target];
        let num_ctx_fix = est_tokens + 2_048; // window + headroom for the JSON output
        say("──────────────────────────────────────────────────────────────");
        say(&format!(
            "WINDOW {idx}: {chars} chars  ~{est_tokens} est tokens"
        ));
        say(&format!(
            "  Arm A = current prod (NO num_ctx -> ollama default 4096)   Arm B = fix (num_ctx={num_ctx_fix})\n"
        ));

        // Arm A — exactly what prod sends today: no num_ctx.
        let arm_a = json!({
            "model": model, "stream": false, "format": "json",
            "prompt": prompt, "think": false,
            "options": { "temperature": 0.0, "num_predict": num_predict }
        });
        run_arm(&client, &endpoint, "A (prod, no num_ctx)", arm_a).await;

        // Arm B — candidate fix: num_ctx sized to the window.
        let arm_b = json!({
            "model": model, "stream": false, "format": "json",
            "prompt": prompt, "think": false,
            "options": { "temperature": 0.0, "num_ctx": num_ctx_fix, "num_predict": num_predict }
        });
        run_arm(
            &client,
            &endpoint,
            &format!("B (fix, num_ctx={num_ctx_fix})"),
            arm_b,
        )
        .await;
    }
}

/// Print + flush immediately (stdout is block-buffered when redirected to a file).
fn say(line: &str) {
    println!("{line}");
    let _ = std::io::stdout().flush();
}

async fn run_arm(client: &reqwest::Client, endpoint: &str, label: &str, body: Value) {
    let resp = match client.post(endpoint).json(&body).send().await {
        Ok(r) => r,
        Err(e) => {
            say(&format!("  [{label}] HTTP error: {e}"));
            return;
        }
    };
    let status = resp.status();
    let full: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            say(&format!(
                "  [{label}] body decode error (status {status}): {e}"
            ));
            return;
        }
    };
    let response_text = full.get("response").and_then(|v| v.as_str()).unwrap_or("");
    let prompt_eval = full.get("prompt_eval_count").and_then(|v| v.as_u64());
    let eval = full.get("eval_count").and_then(|v| v.as_u64());

    // Parse exactly as the prod extractor does: require a top-level `candidates` array.
    let (parse_ok, cand_count, parse_note): (bool, i64, String) =
        match serde_json::from_str::<Value>(response_text) {
            Ok(v) => match v.get("candidates").and_then(|c| c.as_array()) {
                Some(arr) => (
                    true,
                    arr.len() as i64,
                    "valid JSON, has candidates[]".to_owned(),
                ),
                None => {
                    let keys: Vec<String> = v
                        .as_object()
                        .map(|o| o.keys().take(8).cloned().collect())
                        .unwrap_or_default();
                    (
                        false,
                        -1,
                        format!("valid JSON but NO candidates key; top keys={keys:?}"),
                    )
                }
            },
            Err(e) => (false, -1, format!("INVALID JSON: {e}")),
        };

    say(&format!(
        "  [{label}] status={status} prompt_eval_count={prompt_eval:?} eval_count={eval:?} resp_len={}",
        response_text.chars().count()
    ));
    say(&format!(
        "    parse_ok={parse_ok} candidates={cand_count}  {parse_note}"
    ));
    let snippet: String = response_text.chars().take(700).collect();
    say(&format!("    raw response (first 700 chars): {snippet}\n"));
}
