#!/usr/bin/env python3
"""Unit tests for T22 Unit B teach-session document delivery (harness-side).

Run: python3 tests/e2e/efficacy/clband/test_teach_delivery.py   (or via pytest)
Deterministic, no network, no extractor — proves the harness injects the knowledge
document as a leading user turn and is a safe no-op for unknown contexts.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import teach_delivery  # noqa: E402


def test_unknown_context_is_noop():
    raw = '{"type":"user","message":{"role":"user","content":"hi"}}\n'
    assert teach_delivery.materialize("not-a-context", raw) == raw


def test_flywheel_injects_document_as_leading_user_turn():
    raw = '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}\n'
    out = teach_delivery.materialize("flywheel-assembly-agent", raw)
    lines = out.splitlines()
    # First line must be a user turn carrying the doc; original transcript preserved after.
    first = json.loads(lines[0])
    assert first["type"] == "user"
    assert first["message"]["role"] == "user"
    body = first["message"]["content"]
    # Verbatim operative rules from the flywheel SOP must be present in the injected turn.
    for sentinel in ("next size up", "firm shake", "spin test", "Forklift"):
        assert sentinel.lower() in body.lower(), f"missing operative sentinel: {sentinel}"
    # Original transcript line is preserved verbatim as the tail.
    assert lines[1] == raw.strip()


def test_aether_injects_spec_as_user_turn():
    raw = '{"type":"user","message":{"role":"user","content":"translate"}}\n'
    out = teach_delivery.materialize("aether-language", raw)
    first = json.loads(out.splitlines()[0])
    body = first["message"]["content"]
    for sentinel in ("conduit", "swirl"):
        assert sentinel.lower() in body.lower(), f"missing aether sentinel: {sentinel}"


def test_delivery_does_not_use_system_speaker():
    # The doc must NOT be delivered as a system-impersonating speaker (would be dropped by the
    # suspicious-speaker injection filter, and would weaken the trust boundary). User role only.
    out = teach_delivery.materialize("flywheel-assembly-agent", "{}\n")
    first = json.loads(out.splitlines()[0])
    assert first["message"]["role"] == "user"
    assert "system" not in first["message"]["role"].lower()


if __name__ == "__main__":
    tests = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    failed = 0
    for t in tests:
        try:
            t()
            print(f"PASS {t.__name__}")
        except AssertionError as e:
            failed += 1
            print(f"FAIL {t.__name__}: {e}")
    print(f"\n{len(tests) - failed}/{len(tests)} passed")
    sys.exit(1 if failed else 0)
