#!/usr/bin/env bash
set -euo pipefail

COMPOSE_FILE="docker-compose.test.yml"
POSTGRES_DB="${POSTGRES_DB:-skill_layer_test}"
POSTGRES_USER="${POSTGRES_USER:-skill_layer}"

cleanup() {
  docker compose --ansi never -f "${COMPOSE_FILE}" down --remove-orphans >/dev/null 2>&1 || true
}

wait_for_postgres() {
  for _ in {1..30}; do
    if docker exec "${POSTGRES_CONTAINER}" pg_isready -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" >/dev/null 2>&1; then
      return 0
    fi
    sleep 2
  done
  return 1
}

wait_for_redis() {
  for _ in {1..30}; do
    if [[ "$(docker exec "${REDIS_CONTAINER}" redis-cli --raw ping 2>/dev/null || true)" == "PONG" ]]; then
      return 0
    fi
    sleep 2
  done
  return 1
}

sql_bool() {
  local query="$1"
  docker exec "${POSTGRES_CONTAINER}" psql -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" -tAc "${query}"
}

sql_value() {
  local query="$1"
  docker exec "${POSTGRES_CONTAINER}" psql -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" -tAc "${query}"
}

assert_sql_true() {
  local label="$1"
  local query="$2"
  local result
  result="$(sql_bool "${query}")"
  if [[ "${result}" != "t" ]]; then
    echo "Assertion failed: ${label}"
    echo "Query: ${query}"
    exit 1
  fi
}

trap cleanup EXIT

echo "==> Running workspace tests"
cargo test --workspace

echo "==> Starting infrastructure test stack"
docker compose --ansi never -f "${COMPOSE_FILE}" up -d postgres redis qdrant ollama

POSTGRES_CONTAINER="$(docker compose --ansi never -f "${COMPOSE_FILE}" ps -q postgres)"
REDIS_CONTAINER="$(docker compose --ansi never -f "${COMPOSE_FILE}" ps -q redis)"

if [[ -z "${POSTGRES_CONTAINER}" || -z "${REDIS_CONTAINER}" ]]; then
  echo "Failed to resolve postgres/redis container IDs from compose stack"
  exit 1
fi

echo "==> Waiting for postgres and redis readiness"
wait_for_postgres || { echo "Postgres did not become ready"; exit 1; }
wait_for_redis || { echo "Redis did not become ready"; exit 1; }

echo "==> Verifying full service topology reachability"
docker compose --ansi never -f "${COMPOSE_FILE}" run --rm --no-deps topology-check >/dev/null

echo "==> Applying infrastructure baseline migration"
docker exec -i "${POSTGRES_CONTAINER}" \
  psql -v ON_ERROR_STOP=1 -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" \
  < crates/infrastructure/migrations/001_initial_schema.sql >/dev/null

echo "==> Running PostgreSQL hard assertions"
assert_sql_true "outbox_events table exists" \
  "SELECT to_regclass('public.outbox_events') IS NOT NULL;"
assert_sql_true "rebuild_locks table exists" \
  "SELECT to_regclass('public.rebuild_locks') IS NOT NULL;"
assert_sql_true "graph_state singleton is seeded" \
  "SELECT EXISTS (SELECT 1 FROM graph_state WHERE singleton = TRUE);"
assert_sql_true "outbox updated_at trigger exists" \
  "SELECT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'trg_outbox_events_set_updated_at' AND NOT tgisinternal);"
assert_sql_true "outbox idempotency uniqueness constraint exists" \
  "SELECT EXISTS (SELECT 1 FROM pg_constraint c JOIN pg_class t ON c.conrelid = t.oid WHERE t.relname = 'outbox_events' AND c.contype = 'u' AND pg_get_constraintdef(c.oid) ILIKE '%idempotency_key%');"

echo "==> Verifying migration rollback/restore contract"
ROLLBACK_DB="${POSTGRES_DB}_rollback_probe"
ROLLBACK_DUMP="/tmp/${ROLLBACK_DB}_pre.dump"

docker exec "${POSTGRES_CONTAINER}" dropdb --if-exists -U "${POSTGRES_USER}" "${ROLLBACK_DB}" >/dev/null
docker exec "${POSTGRES_CONTAINER}" createdb -U "${POSTGRES_USER}" "${ROLLBACK_DB}" >/dev/null
docker exec "${POSTGRES_CONTAINER}" pg_dump -Fc -U "${POSTGRES_USER}" -d "${ROLLBACK_DB}" -f "${ROLLBACK_DUMP}"

docker exec -i "${POSTGRES_CONTAINER}" \
  psql -v ON_ERROR_STOP=1 -U "${POSTGRES_USER}" -d "${ROLLBACK_DB}" \
  < crates/infrastructure/migrations/001_initial_schema.sql >/dev/null

rollback_migrated="$(docker exec "${POSTGRES_CONTAINER}" psql -U "${POSTGRES_USER}" -d "${ROLLBACK_DB}" -tAc "SELECT to_regclass('public.outbox_events') IS NOT NULL;")"
if [[ "${rollback_migrated}" != "t" ]]; then
  echo "Expected rollback probe database to contain migrated schema before restore"
  exit 1
fi

docker exec "${POSTGRES_CONTAINER}" dropdb --if-exists -U "${POSTGRES_USER}" "${ROLLBACK_DB}" >/dev/null
docker exec "${POSTGRES_CONTAINER}" createdb -U "${POSTGRES_USER}" "${ROLLBACK_DB}" >/dev/null
docker exec "${POSTGRES_CONTAINER}" pg_restore -U "${POSTGRES_USER}" -d "${ROLLBACK_DB}" "${ROLLBACK_DUMP}" >/dev/null

rollback_restored="$(docker exec "${POSTGRES_CONTAINER}" psql -U "${POSTGRES_USER}" -d "${ROLLBACK_DB}" -tAc "SELECT to_regclass('public.outbox_events') IS NULL;")"
if [[ "${rollback_restored}" != "t" ]]; then
  echo "Expected rollback probe database restore to remove migrated schema artifacts"
  exit 1
fi

docker exec "${POSTGRES_CONTAINER}" dropdb --if-exists -U "${POSTGRES_USER}" "${ROLLBACK_DB}" >/dev/null
docker exec "${POSTGRES_CONTAINER}" rm -f "${ROLLBACK_DUMP}" >/dev/null

docker exec "${POSTGRES_CONTAINER}" psql -v ON_ERROR_STOP=1 -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" \
  -c "DELETE FROM outbox_events WHERE idempotency_key = 'e2e-dup-key';" >/dev/null

docker exec "${POSTGRES_CONTAINER}" psql -v ON_ERROR_STOP=1 -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" \
  -c "INSERT INTO outbox_events (event_id, event_type, correlation_id, idempotency_key, schema_version, payload, status, occurred_at, available_at) VALUES ('00000000-0000-0000-0000-000000000001', 'graph.updated', '00000000-0000-0000-0000-000000000111', 'e2e-dup-key', 1, '{\"ok\":true}'::jsonb, 'pending', NOW(), NOW());" >/dev/null

if docker exec "${POSTGRES_CONTAINER}" psql -v ON_ERROR_STOP=1 -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" \
  -c "INSERT INTO outbox_events (event_id, event_type, correlation_id, idempotency_key, schema_version, payload, status, occurred_at, available_at) VALUES ('00000000-0000-0000-0000-000000000002', 'graph.updated', '00000000-0000-0000-0000-000000000222', 'e2e-dup-key', 1, '{\"ok\":true}'::jsonb, 'pending', NOW(), NOW());" >/dev/null 2>&1; then
  echo "Expected duplicate outbox idempotency_key insert to fail, but it succeeded"
  exit 1
fi

echo "==> Running outbox state-machine hard assertions"
docker exec "${POSTGRES_CONTAINER}" psql -v ON_ERROR_STOP=1 -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" \
  -c "DELETE FROM outbox_events WHERE idempotency_key = 'e2e-state-machine-key';" >/dev/null

docker exec "${POSTGRES_CONTAINER}" psql -v ON_ERROR_STOP=1 -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" \
  -c "INSERT INTO outbox_events (event_id, event_type, correlation_id, idempotency_key, schema_version, payload, status, attempts, occurred_at, available_at) VALUES ('00000000-0000-0000-0000-000000000010', 'graph.updated', '00000000-0000-0000-0000-000000000110', 'e2e-state-machine-key', 1, '{\"ok\":true}'::jsonb, 'pending', 0, NOW(), NOW());" >/dev/null

published_without_processing="$(sql_value "WITH updated AS (UPDATE outbox_events SET status = 'published', stream_id = 'stream-1', published_at = NOW(), updated_at = NOW() WHERE event_id = '00000000-0000-0000-0000-000000000010' AND status = 'processing' RETURNING 1) SELECT COUNT(*) FROM updated;")"
if [[ "${published_without_processing}" != "0" ]]; then
  echo "Expected guarded publish transition from pending to affect 0 rows, got ${published_without_processing}"
  exit 1
fi

for attempt in 1 2 3; do
  docker exec "${POSTGRES_CONTAINER}" psql -v ON_ERROR_STOP=1 -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" \
    -c "UPDATE outbox_events SET status = 'processing', updated_at = NOW() WHERE event_id = '00000000-0000-0000-0000-000000000010';" >/dev/null

  docker exec "${POSTGRES_CONTAINER}" psql -v ON_ERROR_STOP=1 -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" \
    -c "UPDATE outbox_events SET status = CASE WHEN attempts + 1 >= 3 THEN 'failed' ELSE 'pending' END, attempts = attempts + 1, last_error = 'e2e failure', available_at = CASE WHEN attempts + 1 >= 3 THEN available_at ELSE NOW() + INTERVAL '10 seconds' END, updated_at = NOW() WHERE event_id = '00000000-0000-0000-0000-000000000010' AND status = 'processing';" >/dev/null

  current_state="$(sql_value "SELECT status || ':' || attempts::text FROM outbox_events WHERE event_id = '00000000-0000-0000-0000-000000000010';")"
  if [[ "${attempt}" -lt 3 && "${current_state}" != "pending:${attempt}" ]]; then
    echo "Expected retry attempt ${attempt} to yield pending:${attempt}, got ${current_state}"
    exit 1
  fi
  if [[ "${attempt}" -eq 3 && "${current_state}" != "failed:3" ]]; then
    echo "Expected retry attempt ${attempt} to yield failed:3, got ${current_state}"
    exit 1
  fi
done

docker exec "${POSTGRES_CONTAINER}" psql -v ON_ERROR_STOP=1 -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" \
  -c "DELETE FROM rebuild_locks WHERE lock_name = 'e2e-lock';" >/dev/null

docker exec "${POSTGRES_CONTAINER}" psql -v ON_ERROR_STOP=1 -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" \
  -c "INSERT INTO rebuild_locks (lock_name, owner_id, acquired_at, expires_at) VALUES ('e2e-lock', '00000000-0000-0000-0000-000000000333', NOW(), NOW() + INTERVAL '5 minutes');" >/dev/null

if docker exec "${POSTGRES_CONTAINER}" psql -v ON_ERROR_STOP=1 -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" \
  -c "INSERT INTO rebuild_locks (lock_name, owner_id, acquired_at, expires_at) VALUES ('e2e-lock', '00000000-0000-0000-0000-000000000444', NOW(), NOW() + INTERVAL '5 minutes');" >/dev/null 2>&1; then
  echo "Expected duplicate rebuild lock insert to fail, but it succeeded"
  exit 1
fi

echo "==> Running Redis stream hard assertions"
docker exec "${REDIS_CONTAINER}" redis-cli DEL skill-layer-events processed:e2e-idempotency >/dev/null
docker exec "${REDIS_CONTAINER}" redis-cli XGROUP DESTROY skill-layer-events skill-layer >/dev/null 2>&1 || true
docker exec "${REDIS_CONTAINER}" redis-cli XGROUP CREATE skill-layer-events skill-layer 0 MKSTREAM >/dev/null

busygroup_output="$(docker exec "${REDIS_CONTAINER}" redis-cli --raw XGROUP CREATE skill-layer-events skill-layer 0 MKSTREAM 2>&1 || true)"
if ! grep -q "BUSYGROUP" <<<"${busygroup_output}"; then
  echo "Expected deterministic BUSYGROUP error on duplicate consumer-group initialization"
  echo "${busygroup_output}"
  exit 1
fi

event_id="$(docker exec "${REDIS_CONTAINER}" redis-cli --raw XADD skill-layer-events '*' envelope '{\"event\":\"graph.rebuilt\",\"idempotency_key\":\"e2e-stream-key\"}')"
read_output="$(docker exec "${REDIS_CONTAINER}" redis-cli --raw XREADGROUP GROUP skill-layer worker-1 COUNT 1 STREAMS skill-layer-events '>')"

if ! grep -q "${event_id}" <<<"${read_output}"; then
  echo "Expected consumer group read to contain event ID ${event_id}"
  exit 1
fi

pending_reclaim_output="$(docker exec "${REDIS_CONTAINER}" redis-cli --raw XREADGROUP GROUP skill-layer worker-1 COUNT 1 STREAMS skill-layer-events 0)"
if ! grep -q "${event_id}" <<<"${pending_reclaim_output}"; then
  echo "Expected pending reclaim read to contain unacked event ID ${event_id}"
  exit 1
fi

ack_count="$(docker exec "${REDIS_CONTAINER}" redis-cli --raw XACK skill-layer-events skill-layer "${event_id}")"
if [[ "${ack_count}" != "1" ]]; then
  echo "Expected XACK to acknowledge one message, got ${ack_count}"
  exit 1
fi

docker exec "${REDIS_CONTAINER}" redis-cli SETEX processed:e2e-idempotency 60 1 >/dev/null
exists="$(docker exec "${REDIS_CONTAINER}" redis-cli --raw EXISTS processed:e2e-idempotency)"
if [[ "${exists}" != "1" ]]; then
  echo "Expected processed:e2e-idempotency key to exist after SETEX"
  exit 1
fi

echo "==> T02 infrastructure hard E2E checks passed"
