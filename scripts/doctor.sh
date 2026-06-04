#!/usr/bin/env bash
#
# doctor.sh — Stack diagnostic checker for the Dynamic Agent Skill Layer.
#
# Reports ok|warn|fail for each prerequisite. Exits non-zero only when a
# blocker prevents run-demo.sh from working. Non-blockers exit with a warning
# so the user gets a full picture without aborting early.
#
# Usage:
#   scripts/doctor.sh
#
# Canonical port/env contract (from run-e2e-tests.sh and docker-compose.test.yml):
#   MCP server    : http://127.0.0.1:3001  (MCP_SERVER_PORT or 3001)
#   Graph builder : http://127.0.0.1:8080  (GRAPH_BUILDER_PORT or 8080)
#   Qdrant REST   : http://127.0.0.1:16333 (QDRANT_HTTP_PORT or 16333)
#   Qdrant gRPC   : 127.0.0.1:16334        (QDRANT_GRPC_PORT or 16334, warn-only)
#   Postgres      : 127.0.0.1:15432        (POSTGRES_PORT or 15432)
#   Redis         : 127.0.0.1:16379        (REDIS_PORT or 16379)
#   Ollama        : http://127.0.0.1:11444  (OLLAMA_PORT or 11444)
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

# ---------------------------------------------------------------------------
# Port defaults — must stay in sync with docker-compose.test.yml
# ---------------------------------------------------------------------------
MCP_SERVER_PORT="${MCP_SERVER_PORT:-3001}"
GRAPH_BUILDER_PORT="${GRAPH_BUILDER_PORT:-8080}"
OLLAMA_PORT="${OLLAMA_PORT:-11444}"
QDRANT_HTTP_PORT="${QDRANT_HTTP_PORT:-16333}"
QDRANT_GRPC_PORT="${QDRANT_GRPC_PORT:-16334}"
POSTGRES_PORT="${POSTGRES_PORT:-15432}"
REDIS_PORT="${REDIS_PORT:-16379}"

OLLAMA_MODEL="${OLLAMA_MODEL:-nomic-embed-text}"

# ---------------------------------------------------------------------------
# Reporting helpers
# ---------------------------------------------------------------------------
FAIL_COUNT=0
WARN_COUNT=0
OK_COUNT=0

report_ok()   { echo "  ok    $*"; OK_COUNT=$((OK_COUNT + 1)); }
report_warn() { echo "  warn  $*"; WARN_COUNT=$((WARN_COUNT + 1)); }
report_fail() { echo "  FAIL  $*"; FAIL_COUNT=$((FAIL_COUNT + 1)); }

# Check a TCP port is open. Returns 0 (success) if reachable, 1 otherwise.
check_tcp_port() {
    local host="$1" port="$2"
    if command -v nc >/dev/null 2>&1; then
        nc -z -w2 "$host" "$port" >/dev/null 2>&1
    else
        # Fallback: bash /dev/tcp pseudo-device (not available in all shells)
        timeout 2 bash -c "echo >/dev/tcp/$host/$port" >/dev/null 2>&1
    fi
}

# HTTP GET; returns 0 if HTTP 2xx/3xx received, 1 otherwise.
check_http_ok() {
    local url="$1"
    curl -sSf --max-time 5 "$url" >/dev/null 2>&1
}

# ---------------------------------------------------------------------------
# 1. Docker / Docker Compose availability
# ---------------------------------------------------------------------------
echo ""
echo "==> [1] Docker / Compose"
if command -v docker >/dev/null 2>&1; then
    report_ok "docker found: $(docker --version 2>/dev/null | head -1)"
else
    report_fail "docker not found — install Docker Desktop or Docker Engine"
fi

if docker compose version >/dev/null 2>&1; then
    report_ok "docker compose plugin available"
elif command -v docker-compose >/dev/null 2>&1; then
    report_warn "docker-compose (standalone) found; 'docker compose' plugin preferred"
else
    report_fail "docker compose not found — run-demo.sh requires Docker Compose"
fi

# ---------------------------------------------------------------------------
# 2. Required environment variables
# ---------------------------------------------------------------------------
echo ""
echo "==> [2] Environment variables"

TRANSCRIPT_INGEST_SECRET="${TRANSCRIPT_INGEST_SECRET:-}"
if [ -n "$TRANSCRIPT_INGEST_SECRET" ]; then
    # Verify secret is non-trivial (not a common placeholder)
    if [ "$TRANSCRIPT_INGEST_SECRET" = "changeme" ] || [ "$TRANSCRIPT_INGEST_SECRET" = "secret" ]; then
        report_warn "TRANSCRIPT_INGEST_SECRET is set but looks like a placeholder — set a real value in .env"
    else
        report_ok "TRANSCRIPT_INGEST_SECRET is set (secret posture ok)"
    fi
else
    report_warn "TRANSCRIPT_INGEST_SECRET not set — ingest endpoint will reject authenticated requests (demo uses unauthenticated path if unset)"
fi

SKILL_GLOBAL_PATHS="${SKILL_GLOBAL_PATHS:-}"
if [ -n "$SKILL_GLOBAL_PATHS" ]; then
    report_ok "SKILL_GLOBAL_PATHS=$SKILL_GLOBAL_PATHS"
else
    report_warn "SKILL_GLOBAL_PATHS not set — mcp-server container uses its own default (/skills/global); demo will seed into a temp directory"
fi

# ---------------------------------------------------------------------------
# 3. Infrastructure endpoints: PG / Redis / Qdrant
# ---------------------------------------------------------------------------
echo ""
echo "==> [3] Infrastructure endpoints"

if check_tcp_port "127.0.0.1" "$POSTGRES_PORT"; then
    report_ok "Postgres reachable on port $POSTGRES_PORT"
else
    report_fail "Postgres not reachable on 127.0.0.1:$POSTGRES_PORT — run: docker compose -f docker-compose.test.yml up -d postgres"
fi

if check_tcp_port "127.0.0.1" "$REDIS_PORT"; then
    report_ok "Redis reachable on port $REDIS_PORT"
else
    report_fail "Redis not reachable on 127.0.0.1:$REDIS_PORT — run: docker compose -f docker-compose.test.yml up -d redis"
fi

if check_http_ok "http://127.0.0.1:${QDRANT_HTTP_PORT}/collections"; then
    report_ok "Qdrant REST reachable on port $QDRANT_HTTP_PORT"
else
    report_fail "Qdrant REST not reachable on http://127.0.0.1:$QDRANT_HTTP_PORT — run: docker compose -f docker-compose.test.yml up -d qdrant"
fi

# Qdrant gRPC is warn-only: read path (compile_context) does not use it.
if check_tcp_port "127.0.0.1" "$QDRANT_GRPC_PORT"; then
    report_ok "Qdrant gRPC reachable on port $QDRANT_GRPC_PORT"
else
    report_warn "Qdrant gRPC not reachable on 127.0.0.1:$QDRANT_GRPC_PORT — write path may be affected; read path (compile_context) is unaffected (Option A CQRS)"
fi

# ---------------------------------------------------------------------------
# 4. Ollama model availability
# ---------------------------------------------------------------------------
echo ""
echo "==> [4] Ollama"

if check_http_ok "http://127.0.0.1:${OLLAMA_PORT}/"; then
    report_ok "Ollama HTTP server reachable on port $OLLAMA_PORT"

    # Check the embedding model is available.
    OLLAMA_TAGS="$(curl -sSf --max-time 10 "http://127.0.0.1:${OLLAMA_PORT}/api/tags" 2>/dev/null || true)"
    if echo "$OLLAMA_TAGS" | grep -q "\"${OLLAMA_MODEL}\""; then
        report_ok "Ollama model '$OLLAMA_MODEL' is available"
    else
        report_warn "Ollama model '$OLLAMA_MODEL' not found — pull it with: curl -X POST http://127.0.0.1:${OLLAMA_PORT}/api/pull -d '{\"name\":\"${OLLAMA_MODEL}\"}'"
    fi
else
    report_fail "Ollama not reachable on http://127.0.0.1:$OLLAMA_PORT — run: docker compose -f docker-compose.test.yml up -d ollama"
fi

# ---------------------------------------------------------------------------
# 5. MCP server /health
# ---------------------------------------------------------------------------
echo ""
echo "==> [5] MCP server"

MCP_HEALTH="$(curl -sSf --max-time 10 "http://127.0.0.1:${MCP_SERVER_PORT}/health" 2>/dev/null || true)"
if [ -n "$MCP_HEALTH" ]; then
    MCP_HEALTHY="$(echo "$MCP_HEALTH" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('healthy',''))" 2>/dev/null || true)"
    if [ "$MCP_HEALTHY" = "True" ] || [ "$MCP_HEALTHY" = "true" ]; then
        report_ok "MCP server /health: healthy"
    else
        report_warn "MCP server /health: not fully healthy — check 'docker compose -f docker-compose.test.yml logs mcp-server'"
    fi
else
    report_fail "MCP server not reachable on http://127.0.0.1:$MCP_SERVER_PORT — run: docker compose -f docker-compose.test.yml up -d mcp-server"
fi

# ---------------------------------------------------------------------------
# 6. Transcript-ingest secret posture (loopback-only check)
# ---------------------------------------------------------------------------
echo ""
echo "==> [6] Ingest secret posture"

INGEST_PROBE="$(curl -sS --max-time 5 -o /dev/null -w "%{http_code}" \
    -X POST "http://127.0.0.1:${MCP_SERVER_PORT}/ingest/transcript" \
    -H "Content-Type: application/json" \
    -H "X-Ingest-Secret: wrong-probe-secret" \
    -d '{"session_id":"doctor-probe","source":"session_end","content":"probe"}' 2>/dev/null || true)"

if [ "$INGEST_PROBE" = "401" ]; then
    report_ok "Ingest endpoint rejects wrong secret with 401 (TRANSCRIPT_INGEST_SECRET enforced)"
elif [ "$INGEST_PROBE" = "202" ] || [ "$INGEST_PROBE" = "200" ]; then
    # Server accepted the request — TRANSCRIPT_INGEST_SECRET is not configured on
    # this server instance, so the endpoint is open to any caller on loopback.
    # This is a posture warning, not a blocker; loopback-only exposure limits risk.
    report_warn "Ingest endpoint is open (no TRANSCRIPT_INGEST_SECRET on server) — set TRANSCRIPT_INGEST_SECRET in the server environment to gate access"
elif [ "$INGEST_PROBE" = "000" ] || [ -z "$INGEST_PROBE" ]; then
    report_warn "Ingest endpoint not reachable — MCP server may not be running"
else
    report_warn "Ingest endpoint returned HTTP $INGEST_PROBE for a probe request"
fi

# ---------------------------------------------------------------------------
# 7. graph_version readability
# ---------------------------------------------------------------------------
echo ""
echo "==> [7] graph_version"

# Use docker compose exec to query Postgres — avoids requiring a local psql install.
GRAPH_VERSION_READABLE=0
if check_tcp_port "127.0.0.1" "$POSTGRES_PORT"; then
    GRAPH_VERSION="$(docker compose --ansi never -f "${REPO_ROOT}/docker-compose.test.yml" \
        exec -T postgres \
        psql -U "${POSTGRES_USER:-skill_layer}" \
             -d "${POSTGRES_DB:-skill_layer_test}" \
             -tAc "SELECT graph_version FROM graph_state WHERE singleton=true LIMIT 1" \
        2>/dev/null | tr -d '[:space:]' || true)"
    if [ -n "$GRAPH_VERSION" ]; then
        report_ok "graph_version readable from Postgres: $GRAPH_VERSION"
        GRAPH_VERSION_READABLE=1
    else
        report_warn "graph_version not readable (Postgres up but graph_state empty — mcp-server runs migrations on first start)"
    fi
else
    report_warn "graph_version check skipped — Postgres not reachable"
fi

# ---------------------------------------------------------------------------
# 8. Claude Code hook config presence
# ---------------------------------------------------------------------------
echo ""
echo "==> [8] Claude Code hook config"

HOOK_CONFIG="$HOME/.claude/settings.json"
HOOK_EXAMPLE="${REPO_ROOT}/config/claude-code/hooks.example.json"

if [ -f "$HOOK_CONFIG" ]; then
    if python3 -c "import json; d=json.load(open('$HOOK_CONFIG')); assert 'hooks' in d or 'mcpServers' in d" >/dev/null 2>&1; then
        report_ok "Claude Code hook config found at $HOOK_CONFIG with hooks/mcpServers key"
    else
        report_warn "$HOOK_CONFIG exists but missing 'hooks' or 'mcpServers' — copy from $HOOK_EXAMPLE"
    fi
else
    report_warn "Claude Code hook config not found at $HOOK_CONFIG — copy from $HOOK_EXAMPLE to enable session context injection"
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo "================================================================"
echo "  doctor.sh summary"
echo "================================================================"
echo "  ok   : $OK_COUNT checks passed"
echo "  warn : $WARN_COUNT non-blocking warnings"
echo "  FAIL : $FAIL_COUNT blockers"
echo ""

if [ "$FAIL_COUNT" -gt 0 ]; then
    echo "  ACTION: Fix the FAIL items above before running run-demo.sh."
    echo "  Quick start: docker compose -f docker-compose.test.yml up -d postgres redis qdrant ollama"
    echo ""
    exit 1
elif [ "$WARN_COUNT" -gt 0 ]; then
    echo "  Stack has warnings but is demo-ready. Run: scripts/run-demo.sh"
    echo ""
    exit 0
else
    echo "  Stack is fully healthy. Run: scripts/run-demo.sh"
    echo ""
    exit 0
fi
