# T14 Efficacy Task Battery — Invented-Rule Tasks

This directory contains the 10-task battery for the T14 efficacy A/B experiment.
Each task encodes a **project-specific invented rule** sourced from a real skill in
the T10 262-corpus. The rule is (a) present as a corpus skill the ON arm can retrieve
and (b) verifiably absent from model pretraining — a generic best-practices solve
will fail the verifier.

## Task Index

| Task ID | Rule (one line) | Corpus Slug | Skill ID |
|---------|-----------------|-------------|----------|
| `all-migrations-dual-registration` | Every migration SQL file must be wired into BOTH a const AND the MIGRATIONS array | `all-migrations-must-be-registered-in-both-the-preference` | `ca229ea9-96fb-44e4-87d7-c7bf46804d33` |
| `backend-selector-fail-loud` | Backend selector arms must return Err/unreachable, never silently clone another arm's result | `backend-selector-arm-must-fail-loud-not-passthrough` | `9835e70f-3047-4504-9d4e-41853b804b1a` |
| `arcswap-rcu-return-value` | ArcSwap rcu() outcome must come from the return value (prev Arc), not a closure-mutated variable | `arcswap-rcu-use-return-value-not-closure-mutation` | `49319f72-c248-4969-861f-12424f7802b2` |
| `cargo-required-features-check` | cargo test for required-features targets must pass --features or the tests silently skip | `cargo-all-targets-silently-skips-required-features` | `1f3a12ae-2593-48c9-bdcf-e8a930340219` |
| `env-var-fail-loud-all-binaries` | Fail-loud env-var validation must cover ALL workspace binaries, not just the server binary | `apply-fail-loud-env-var-checks-to-all-binaries-not-just-the-server` | `d776c274-37a9-406d-a7e7-9fdb7846e941` |
| `claude-cli-fence-stripping` | Strip triple-backtick fences from claude CLI `.result` before JSON parsing | `claude-cli-headless-json-fence-stripping` | `73fdf717-a77d-4d9c-8e73-0d3e18ae46f8` |
| `cold-start-guard-retirement` | Retirement workers must exclude zero-usage items (cold-start guard) | `cold-start-guard-for-retirement-workers` | `6e46aae8-1adf-4974-97b1-2320f37715a2` |
| `rrf-score-not-exposed` | RRF fusion value must not be the exposed relevance score; expose pre-fusion semantic score | `diagnose-rrf-artifact-score-exposure` | `ff166fb6-353e-40ed-a9bb-a9dce68c63c3` |
| `anthropic-forced-tool-use` | Anthropic API structured output must use forced tool_use, not free-text parsing | `anthropic-api-forced-tool-use-for-structured-json` | `5bab5349-3508-42b0-bebb-5445f864c302` |
| `blank-env-treat-as-absent` | docker-compose ${VAR:-} emits Ok("") — must be treated as absent, same as Err(NotPresent) | `blank-docker-compose-env-var-rust-empty-string` | `fa3244d6-53b0-46a3-b32d-a35348398f9a` |

## Absent-From-Pretraining Rationale

Each rule satisfies the absent-from-pretraining criterion if: a coding agent without
the skill layer (the OFF or PLACEBO arm) would plausibly produce a solve that fails
the verifier, because the rule is project-specific or a non-obvious library/toolchain quirk.

| Task ID | Why pretraining cannot know this |
|---------|----------------------------------|
| `all-migrations-dual-registration` | Project uses a compile-time array migration runner (not dir-scan); general Rust/SQL knowledge teaches Flyway/Diesel-style auto-discovery. Generic agents will add the SQL file but not wire the array entry, or vice-versa. |
| `backend-selector-fail-loud` | Fail-fast is generic, but the specific rule that enum arms must return `Err`/`unreachable!` rather than silently cloning another arm's result is a project-failure-mode. Generic agents leave silent delegation as "harmless" or add a TODO comment. |
| `arcswap-rcu-return-value` | The `arc-swap` crate's `rcu()` CAS-retry behavior is a crate-specific quirk. Pretraining on general Rust concurrency does not surface that the closure runs multiple times; the mutated-bool pattern looks syntactically correct and generic agents accept it. |
| `cargo-required-features-check` | The `cargo --all-targets` silent-skip of `required-features` targets is a non-obvious cargo behavior not prominently documented. Generic CI script fixers add `--test` or `-- --ignored` but do not know to audit for the `required-features` omission. |
| `env-var-fail-loud-all-binaries` | The rule that ALL workspace binaries must share the same fail-loud discipline — plus the callout that port 15432 is a test-infrastructure port — comes from a concrete production incident. Generic agents apply fail-loud to the "main server" only. |
| `claude-cli-fence-stripping` | The CLI wraps `.result` in markdown fences is an observed implementation detail, not in API docs. Generic agents write `json.loads(envelope["result"])` directly since the outer envelope is already JSON and they requested JSON output. |
| `cold-start-guard-retirement` | The cold-start guard (zero usage = new item, not abandoned) is a domain-discovered failure mode. Generic retirement worker implementations use `WHERE usage < threshold` only and miss the zero-usage edge case. |
| `rrf-score-not-exposed` | RRF exposing 1/(rrf_k+rank) as the score field is discovered by live observation of discrete score clustering. Generic retrieval implementations naturally use the final computed fusion score as the exposed value. |
| `anthropic-forced-tool-use` | While the Anthropic API supports tool_use, the rule that ALL structured extraction must use forced tool_use (never free-text parsing) is project-mandated. Generic agents instructed to "return JSON" add `"Return only JSON"` to the system prompt and parse the text block. |
| `blank-env-treat-as-absent` | The docker-compose `${VAR:-}` → `Ok("")` interaction is a discovered edge case between docker-compose passthrough syntax and Rust `std::env::var`. Generic agents check `Err(NotPresent)` only, not `Ok("")`. |

## Skill ID Resolution

Skill IDs were resolved from the live mcp-server at `http://127.0.0.1:3001` using the
`find_skill` MCP tool with targeted prompts matching each skill's description. All IDs
confirmed by semantic score ≥ 0.76 on the matching slug. Resolution timestamp: 2026-06-12.

If a skill ID cannot be confirmed against the live server (e.g. after corpus rebuild),
the task JSON will still name the slug for human lookup — do NOT invent an ID, fail loud
in the run report per CONTRACT.md instructions.

## Verifier Offline Test Results

All 10 verifiers confirmed discriminating behavior offline (good→exit0, bad→non-zero)
as required by the TDD contract. Run the validation sweep:

```bash
bash -c '
fail=0
for v in tests/e2e/efficacy/verifiers/*.sh; do
  id=$(basename "$v" .sh)
  "$v" "tests/e2e/efficacy/fixtures/$id/good" || { echo "FAIL good: $id"; fail=1; }
  "$v" "tests/e2e/efficacy/fixtures/$id/bad" && { echo "FAIL bad did not fail: $id"; fail=1; } || true
done
exit $fail
'
```

## Scope Fence

- These files are consumed by the runner in `scripts/efficacy_ab.py` (Unit 3).
- Do NOT modify this battery after any measured run — changes void the pre-registration
  per the LOCKED block in `docs/tickets/.../14-efficacy-task-outcome-ab-harness.md`.
- Workspace `kind: scratch` tasks use only the setup commands in the JSON; no live
  server, Postgres, or Qdrant access is required to run the verifiers.
