pub mod claude;
pub(crate) mod http;
pub(crate) mod limits;
pub mod ollama;
pub mod prompt_contract;

// # Extraction Prompt Strategy (V1.5 — 2026-05-31; supersedes T14 Path A)
//
// ## Decision: both providers call the model directly with the same contract
//
// Ollama is the default local provider; Claude is a first-class opt-in. Both now
// talk to a real model endpoint directly — there is no external `:8080/extract`
// passthrough (that was the graph-builder admin port, a confused-deputy/SSRF
// risk, removed in V1.5).
//
// ### Current Prompt Inventory
//
// | Provider | Prompt Ownership | Transport | Prompt Content |
// |----------|-----------------|-----------|----------------|
// | Claude   | `ClaudeExtractor` (this crate) | HTTPS POST to the Anthropic Messages API (`{ANTHROPIC_BASE_URL}/v1/messages`) with a forced `emit_candidates` `tool_use` | Static instruction block as the cacheable `system` prompt (`cache_control: ephemeral`) + transcript as the user message; tool `input_schema` is the candidate schema. Keyed by `ANTHROPIC_API_KEY`. |
// | Ollama   | `OllamaExtractor` (this crate) | HTTP POST to Ollama `/api/generate` with `{model, stream: false, format: "json", prompt}` | A full natural-language prompt instructing the local model to produce structured JSON matching `ExtractedSkillCandidate`. |
//
// ### Provider Symmetry
//
// Both providers source their instructions from the shared semantic contract in
// `prompt_contract.rs` (`build_extraction_system_prompt` for Claude,
// `build_ollama_extraction_prompt` for Ollama). The text differs only because of
// transport: Claude uses a forced tool-call for schema conformance, while Ollama
// (which lacks `tool_choice`) keeps schema guidance in-prompt alongside
// `format: "json"`. This preserves extraction parity (SC-3), not a redesign.
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
