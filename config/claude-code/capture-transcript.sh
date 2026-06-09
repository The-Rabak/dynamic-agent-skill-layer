#!/usr/bin/env bash
#
# capture-transcript.sh — host-side transcript capture for the skill layer
# (todo 103, Option 4).
#
# Claude Code invokes this as a `command`-type hook for SessionEnd and
# PreCompact. The hook payload arrives as JSON on stdin and includes an
# ABSOLUTE `transcript_path` that is valid on the host but NOT inside the
# server container. Rather than ship that path across the container boundary
# (which the path validator rejects and the container cannot resolve anyway),
# this script reads the transcript HERE — where the path is native — and POSTs
# its CONTENT to the localhost ingest endpoint. The server merely enqueues a row
# into the durable `transcript_ingest_queue`; the maintenance worker drains it
# into `.pending` drafts later, fully in the background.
#
# EXECUTION MODEL — non-blocking by design:
#   * The hook reads stdin (it must, before the pipe closes) and then RETURNS
#     IMMEDIATELY. The transcript read and the network POST are handed off to a
#     DETACHED background worker (setsid), so the session is never blocked on
#     transcript IO or the network — not even the sub-second localhost POST.
#   * The actual skill extraction (the slow, fallible LLM work) happens even
#     later, in the maintenance worker's queue drain — never in this hook.
#   * Best-effort by contract: every failure is swallowed and every path exits 0,
#     so a capture problem can never break or slow the harness.
#
# Usage (from a hook):   capture-transcript.sh <source>
#   <source> is "session_end" or "pre_compact" (default: session_end).
#
# Configuration (env):
#   SKILL_LAYER_INGEST_URL     ingest endpoint (default http://127.0.0.1:3001/ingest/transcript)
#   SKILL_LAYER_INGEST_SECRET  shared secret sent as the X-Ingest-Secret header (optional)
set -uo pipefail

# --- Detached delivery worker (re-invoked by the hook entrypoint below) --------
# Reads the transcript file named in the hook JSON and POSTs its content. Runs
# out-of-band so nothing here is on the user's critical path.
if [ "${1:-}" = "--deliver" ]; then
    # python3 keeps JSON handling correct (escaping, large content) without a jq
    # dependency; the repo already relies on python3 for the e2e harness.
    body="$(SOURCE="${SKILL_LAYER_SOURCE:-session_end}" python3 - "${SKILL_LAYER_HOOK_JSON:-}" <<'PY'
import json, os, sys

try:
    hook = json.loads(sys.argv[1] or "{}")
except Exception:
    sys.exit(0)

transcript_path = (hook.get("transcript_path") or "").strip()
session_id = (hook.get("session_id") or "").strip()
# Claude Code passes the working directory as `cwd`; fall back to `repo_path`.
repo_path = (hook.get("cwd") or hook.get("repo_path") or "").strip()

if not transcript_path or not session_id:
    sys.exit(0)

try:
    with open(transcript_path, "r", encoding="utf-8") as handle:
        content = handle.read()
except OSError:
    sys.exit(0)

if not content.strip():
    sys.exit(0)

payload = {
    "session_id": session_id,
    "source": os.environ["SOURCE"],
    "content": content,
}
if repo_path:
    payload["repo_path"] = repo_path

json.dump(payload, sys.stdout)
PY
)" || exit 0

    [ -z "$body" ] && exit 0

    # Write the optional secret header to a mode-600 temp file and pass it via
    # `curl -H @file` (curl 7.55+) so the secret never appears in the process
    # argument vector (/proc/<pid>/cmdline), which is readable by any same-UID
    # process. The file is wiped on any exit path via the trap below.
    secret_header_file="$(mktemp)"
    chmod 600 "$secret_header_file"
    trap 'rm -f "$secret_header_file"' EXIT

    if [ -n "${SKILL_LAYER_INGEST_SECRET:-}" ]; then
        printf 'X-Ingest-Secret: %s\r\n' "$SKILL_LAYER_INGEST_SECRET" >"$secret_header_file"
    fi

    # -m 5 bounds the call; failures are swallowed (the durable queue +
    # maintenance backstop tolerate a missed push, and nothing is waiting on us).
    curl -sS -m 5 -X POST "${SKILL_LAYER_INGEST_URL:-http://127.0.0.1:3001/ingest/transcript}" \
        -H "Content-Type: application/json" \
        ${SKILL_LAYER_INGEST_SECRET:+-H @"$secret_header_file"} \
        --data-binary "$body" >/dev/null 2>&1 || true
    exit 0
fi

# --- Hook entrypoint -----------------------------------------------------------
SOURCE="${1:-session_end}"

# Read the hook payload from stdin NOW: the pipe closes once this hook returns,
# so this is the one thing that cannot be deferred. It is small (hook metadata;
# the transcript itself is read from disk by the detached worker).
hook_json="$(cat)"

# Hand the config + payload to the detached worker via the environment. The
# transcript CONTENT is never put in the environment — the worker reads it from
# the file path — so there is no env-size limit concern for large transcripts.
export SKILL_LAYER_SOURCE="$SOURCE"
export SKILL_LAYER_HOOK_JSON="$hook_json"
export SKILL_LAYER_INGEST_URL="${SKILL_LAYER_INGEST_URL:-http://127.0.0.1:3001/ingest/transcript}"
export SKILL_LAYER_INGEST_SECRET="${SKILL_LAYER_INGEST_SECRET:-}"

# Detach so the hook returns immediately. setsid divorces the worker from this
# session/process group so it survives the hook returning (e.g. on SessionEnd
# teardown); `&` returns control instantly. nohup is the fallback if setsid is
# unavailable.
if command -v setsid >/dev/null 2>&1; then
    setsid bash "$0" --deliver </dev/null >/dev/null 2>&1 &
else
    nohup bash "$0" --deliver </dev/null >/dev/null 2>&1 &
fi
disown 2>/dev/null || true

exit 0
