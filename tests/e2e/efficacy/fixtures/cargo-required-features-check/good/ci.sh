#!/usr/bin/env bash
set -euo pipefail
# Good fixture: --features test-utils is passed so the required-features target compiles and runs
cargo test -p mcp-server --test test_live_data_plane_roundtrip --features test-utils -- --ignored
