pub mod claude;
pub(crate) mod http;
pub(crate) mod limits;
pub mod ollama;
pub mod prompt_contract;

// # Extraction Prompt Strategy (T14 Hardening — 2026-05-29)
//
// ## Decision: Path A — Claude endpoint owns prompting, Ollama owns local prompt
//
// ### Current Prompt Inventory
//
// | Provider | Prompt Ownership | Transport | Prompt Content |
// |----------|-----------------|-----------|----------------|
// | Claude   | External endpoint (`CLAUDE_EXTRACTION_ENDPOINT`, default `http://127.0.0.1:8080/extract`) | HTTP POST with `{model, session_id, transcript: [{speaker, content}]}` | No prompt string sent — just raw transcript data. The external endpoint applies its own prompt engineering (potentially Claude `strict: true` tool-calling). |
// | Ollama   | Within `OllamaExtractor` (local prompt builder) | HTTP POST to Ollama `/api/generate` with `{model, stream: false, format: "json", prompt}` | A full natural-language prompt instructing the local model to produce structured JSON matching `ExtractedSkillCandidate`. |
//
// ### Provider Asymmetry Analysis
//
// The Claude endpoint is an external extraction service that OWNS its own prompt engineering.
// This is architecturally intentional, not accidental:
//
// 1. The endpoint URL defaults to `http://127.0.0.1:8080/extract` — a standalone service,
//    not a passthrough to the Anthropic API. The service is expected to apply Claude's
//    `strict: true` tool-calling (per research doc Section 4) with a structured schema,
//    providing schema conformance guarantees that Claude is uniquely positioned to deliver.
//
// 2. Sending raw transcript data (no prompt) is correct: the endpoint knows its model
//    capabilities and can apply optimal prompt engineering. Embedding a prompt in the
//    request body would risk conflicting with the endpoint's own strategy.
//
// 3. Ollama, conversely, has no external service between the extractor and the model.
//    The OllamaExtractor MUST own its prompt because the `/api/generate` endpoint
//    is a raw model interface — it doesn't apply any extraction-specific prompting.
//    Ollama also lacks `tool_choice` support, so schema guidance must be in-prompt
//    alongside `format: "json"`.
//
// ### Shared Prompt Contract
//
// While the actual prompt text differs between providers (endpoint-owned vs local),
// both must conform to a shared *semantic contract* documented in `prompt_contract.rs`:
//
// | Contract Dimension | Requirement |
// |---|---|
// | Extraction Target   | Extract project rules, conventions, best practices, repeatable workflows, error handling patterns, and tool usage conventions — not just "repeatable actions" |
// | Quality Gate        | Each candidate must encode Failure Mechanism Encoding (FME), Actionable Specificity, and High-Risk Action Blacklist where applicable |
// | Output Schema       | `{ "candidates": [{ "name", "description", "tags", "procedures", "conventions", "assets", "confidence" }] }` — matching `ExtractedSkillCandidate` domain type |
// | Anti-patterns       | NO generic skills ("use git"), NO context-dependent skills, NO skills without actionable procedures |
// | Confidence          | 0.0-1.0 float indicating extraction confidence; candidates with confidence < 0.5 should generally not be emitted |
//
// This contract preserves extraction parity (SC-3) while respecting provider-specific
// prompting constraints.
//
// ### Rationale for Path A over Path B (Unified Local Prompt Ownership)
//
// Path B would embed the same prompt in ClaudeExtractor's request body, but:
// - The external endpoint may ignore or conflict with an embedded prompt
// - Claude's `strict: true` tool-calling (research doc §4) is the recommended approach for
//   schema-guaranteed extraction and is best owned by the endpoint, not injected from a client
// - The endpoint may evolve its prompt independently (model upgrades, schema changes) —
//   coupling it to a client-sent prompt would create a distributed coordination problem
//
// ### What This Means for Prompt Evolution
//
// - **Ollama prompt changes**: Edit `ollama.rs` `extract()` method; also update `prompt_contract.rs`
//   if the semantic contract changes
// - **Claude extraction behavior changes**: Update the external endpoint service (out of scope
//   for this crate); `prompt_contract.rs` documents what behavior to expect
//
// ### Ollama Prompt Quality (Post-T14 Enhancement)
//
// The original Ollama prompt was a single line: `"Extract reusable skill candidates as JSON..."`
// This was inadequate. The enhanced prompt (see `ollama.rs`) now includes:
// 1. Project-context instruction: extract rules, conventions, guidelines, not just actions
// 2. Quality rubric: FME, actionable specificity, high-risk blacklist
// 3. Output format specification matching `ExtractedSkillCandidate` schema
// 4. Anti-pattern warning: avoid generic, context-dependent, non-actionable skills
// 5. Confidence scoring guidance
