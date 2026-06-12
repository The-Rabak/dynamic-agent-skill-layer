#!/usr/bin/env bash
set -euo pipefail
# Bad fixture: --features flag omitted; test silently skips (exit 0, no tests run)
cargo test -p mcp-server --test test_live_data_plane_roundtrip -- --ignored
