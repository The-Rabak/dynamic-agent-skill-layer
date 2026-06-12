#!/usr/bin/env python3
"""Unit tests for the clband AUTO-GATE scope guard (T23 Unit B safety boundary).

These are pure-logic tests — NO docker, NO live stack, NO model calls. They prove that
`assert_clband_path` / `scope_dir_name` / `scope_path` refuse to touch anything outside a
`/skills/project/clband-<...>/` scope, which is what keeps the production human gate and the 262
dogfood corpus untouchable while the band auto-accepts in clband scopes only.

Run: python3 -m pytest tests/e2e/efficacy/clband/test_scope_rebuild.py -q
"""
from __future__ import annotations

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import scope_rebuild as sr  # noqa: E402


# ── assert_clband_path ACCEPTS only paths strictly under a clband scope ────────

@pytest.mark.parametrize("good", [
    "/skills/project/clband-flywheel-assembly-agent/.skills/x/SKILL.md.pending",
    "/skills/project/clband-flywheel-assembly-agent/.skills/x/SKILL.md",
    "/skills/project/clband-t23-canary-probe/.skills/a/b/SKILL.md.pending",
    "/skills/project/clband-aether-language",
])
def test_accepts_clband_paths(good):
    sr.assert_clband_path(good)  # must NOT raise


# ── assert_clband_path REJECTS everything else (fail loud) ─────────────────────

@pytest.mark.parametrize("bad", [
    "/skills/project/.skills/dogfood-skill/SKILL.md.pending",   # the 262 dogfood corpus
    "/skills/project/.skills/dogfood-skill/SKILL.md",
    "/skills/global/some-global/SKILL.md",                      # global scope
    "/skills/project/notclband/SKILL.md",                       # sibling dir, not clband-
    "/skills/project/clband",                                   # prefix without the dash boundary
    "/etc/passwd",                                              # outside the tree entirely
    "/skills/project/clband-foo/../.skills/dogfood/SKILL.md",   # path traversal escape
    "",
])
def test_rejects_non_clband_paths(bad):
    with pytest.raises(ValueError):
        sr.assert_clband_path(bad)


# ── scope_dir_name / scope_path build guarded paths and reject unsafe names ────

def test_scope_path_builds_guarded_path():
    assert sr.scope_path("flywheel") == "/skills/project/clband-flywheel"
    assert sr.scope_dir_name("flywheel") == "clband-flywheel"
    # The built path must itself pass the guard.
    sr.assert_clband_path(sr.scope_path("material-handler-sops"))


@pytest.mark.parametrize("unsafe", [
    "../evil", "foo/bar", "foo/../bar", "", ".", "..", "/abs", "UPPER", "foo bar", "a;b",
])
def test_rejects_unsafe_scope_names(unsafe):
    with pytest.raises(ValueError):
        sr.scope_dir_name(unsafe)


def test_canary_scope_name_is_clband_prefixed():
    # The live canary uses a clband-prefixed scope dir, so it is always guarded.
    assert sr.scope_dir_name("t23-canary-probe").startswith(sr.CLBAND_PREFIX)


if __name__ == "__main__":
    sys.exit(pytest.main([__file__, "-q"]))
