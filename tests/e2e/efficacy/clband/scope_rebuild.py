#!/usr/bin/env python3
"""clband scope rebuild + retrieval-readiness + AUTO-GATE (T23 Unit B core) — REAL mechanism.

This is the proven (smoke DP-2, owner-approved Option A) path that makes an accepted clband skill
RETRIEVABLE by the live mcp-server, isolated from the 262 dogfood corpus, then cleanly removable.

Mechanism (no code/ranking change; harness-only):
  - clband skills live in the named volume `dynamic-agent-skill-layer_test-project-skills` at
    `/skills/project/clband-<name>/{.git,.skills/...}`. The `.git` marker makes the mcp-server's
    FsMarkerProjectResolver resolve `repo_path=/skills/project/clband-<name>` to that subdir; retrieval
    then filters by `source_path.starts_with(scope_path)` (crates/retrieval/src/dual_scope.rs), so a
    clband-scoped compile_context returns ONLY that subdir's skills (dogfood + other clband scopes
    excluded). PROVEN live in the smoke.
  - The service mounts are `:ro`, so all volume writes go through a one-off
    `docker run --rm -v <vol>:/skills/project alpine` helper.
  - graph-builder polls the volume (GRAPH_BUILDER_POLL_INTERVAL_MS, default 15s), full-rebuilds on
    change → PG INSERT (source_path = the container path) + Qdrant upsert + publishes Redis
    `graph.rebuilt` → mcp-server graph_refresh_subscriber `reload_and_swap` (ArcSwap; NO restart).
    We never fixed-sleep for this; we POLL the real retrieval condition.

## THE AUTO-GATE SAFETY BOUNDARY (T23 fence)
`accept_all()` is the auto-accept mechanism. Before EVERY rename it asserts the target path lies under
`/skills/project/clband-<...>/` and the scope name matches `^[a-z0-9][a-z0-9_-]*$`. Any non-clband path
(the dogfood `/skills/project/.skills/...`, `/skills/global/...`, or anything else) FAILS LOUD
(raises). This is what keeps the production human gate + the 262 dogfood corpus untouchable while the
band auto-accepts in clband scopes only. Unit-tested in test_scope_rebuild.py.

Acceptance is the REAL structural action (rename `SKILL.md.pending` -> `SKILL.md`, the definition in
scripts/efficacy_draft_acceptance.py) inside the clband scope — never a DB insert or in-process shortcut.
"""
from __future__ import annotations

import re
import subprocess
import sys
import time
from pathlib import Path

# Reuse the validated live-server retrieval primitive (drives the REAL mcp-server over HTTP).
_CLBAND_DIR = Path(__file__).resolve().parent
_SCRIPTS = _CLBAND_DIR.parents[3] / "scripts"
if str(_SCRIPTS) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS))
import efficacy_ab  # noqa: E402

VOLUME = "dynamic-agent-skill-layer_test-project-skills"
PROJECT_ROOT_IN_VOL = "/skills/project"           # the container path the mcp-server resolves
CLBAND_PREFIX = "clband-"                          # the ONLY scope prefix auto-accept may touch
ALPINE = "alpine:3.23.4"
MCP_URL = "http://127.0.0.1:3001"

# A scope NAME (the part after "clband-") must be a safe slug: no slashes, no "..", lowercase-ish.
_SAFE_NAME = re.compile(r"^[a-z0-9][a-z0-9_-]*$")


# ── scope-guard (the safety boundary) ─────────────────────────────────────────

def scope_dir_name(name: str) -> str:
    """Return the guarded clband scope DIRECTORY name `clband-<name>`; raise on an unsafe name."""
    if not _SAFE_NAME.match(name):
        raise ValueError(f"unsafe clband scope name {name!r} (must match {_SAFE_NAME.pattern})")
    return f"{CLBAND_PREFIX}{name}"


def scope_path(name: str) -> str:
    """Return the guarded container path `/skills/project/clband-<name>`."""
    return f"{PROJECT_ROOT_IN_VOL}/{scope_dir_name(name)}"


def assert_clband_path(path: str) -> None:
    """FAIL LOUD unless `path` lies strictly under `/skills/project/clband-<...>/`.

    This is the auto-gate's safety assertion: it rejects the dogfood scope
    (`/skills/project/.skills/...`), the global scope (`/skills/global/...`), and any other path.
    """
    norm = path.strip()
    if ".." in norm.split("/"):
        raise ValueError(f"refusing path with '..' segment: {path!r}")
    guard = f"{PROJECT_ROOT_IN_VOL}/{CLBAND_PREFIX}"
    if not norm.startswith(guard):
        raise ValueError(
            f"SCOPE GUARD: refusing to touch non-clband path {path!r} "
            f"(must start with {guard!r}). The production gate + 262 dogfood corpus are untouchable."
        )


# ── volume helper (writes go through a one-off alpine container; mounts are :ro to services) ──

def _vol_sh(script: str, extra_mounts: list[tuple[str, str]] | None = None,
            check: bool = True) -> subprocess.CompletedProcess:
    """Run `sh -c <script>` in a throwaway alpine with the skills volume mounted rw at /skills/project.

    extra_mounts: list of (host_path, container_path) read-only bind mounts (e.g. a host .skills dir).
    """
    cmd = ["docker", "run", "--rm", "-v", f"{VOLUME}:{PROJECT_ROOT_IN_VOL}"]
    for host, cont in (extra_mounts or []):
        cmd += ["-v", f"{host}:{cont}:ro"]
    cmd += [ALPINE, "sh", "-c", script]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if check and proc.returncode != 0:
        raise RuntimeError(f"volume op failed (rc={proc.returncode}): {script}\n{proc.stderr}")
    return proc


# ── scope lifecycle ───────────────────────────────────────────────────────────

def place_pending(name: str, host_skills_dir: Path) -> int:
    """Copy a host `.skills` tree into the clband scope in the volume + write a `.git` marker.

    host_skills_dir is the on-host `.skills` directory produced by clband_extract.py (containing
    `<subdir>/SKILL.md.pending` drafts). Returns the number of `.pending` files placed.
    """
    sp = scope_path(name)
    assert_clband_path(sp)
    host = host_skills_dir.resolve()
    if not host.is_dir():
        raise FileNotFoundError(f"host skills dir not found: {host}")
    # Fresh scope dir; copy the .skills tree; create the marker that makes repo_path resolve here.
    script = (
        f"set -e; rm -rf '{sp}'; mkdir -p '{sp}/.skills' '{sp}/.git'; "
        f"echo 'ref: refs/heads/main' > '{sp}/.git/HEAD'; "
        f"cp -a /src/. '{sp}/.skills/' 2>/dev/null || true; "
        f"find '{sp}/.skills' -name 'SKILL.md.pending' | wc -l"
    )
    out = _vol_sh(script, extra_mounts=[(str(host), "/src")]).stdout.strip()
    return int(out or 0)


def accept_all(name: str) -> list[str]:
    """AUTO-GATE: rename every `SKILL.md.pending` -> `SKILL.md` inside the clband scope ONLY.

    Asserts the clband scope guard before every rename; fails loud on any non-clband path. Returns the
    list of accepted container paths (the new SKILL.md paths). This is the REAL acceptance action.
    """
    sp = scope_path(name)
    assert_clband_path(sp)
    listing = _vol_sh(f"find '{sp}/.skills' -name 'SKILL.md.pending' 2>/dev/null || true").stdout
    pendings = [p for p in listing.splitlines() if p.strip()]
    accepted: list[str] = []
    for pend in pendings:
        assert_clband_path(pend)                      # guard EVERY path before touching it
        target = pend[: -len(".pending")]
        assert_clband_path(target)
        _vol_sh(f"mv '{pend}' '{target}'")
        accepted.append(target)
    return accepted


def remove_scope(name: str) -> None:
    """Remove a clband scope from the volume (closeout). Scope-guarded; fails loud otherwise."""
    sp = scope_path(name)
    assert_clband_path(sp)
    _vol_sh(f"rm -rf '{sp}'")


def list_clband_scopes() -> list[str]:
    """List the clband-* scope directory names currently present in the volume."""
    out = _vol_sh(
        f"ls -1 '{PROJECT_ROOT_IN_VOL}' 2>/dev/null | grep '^{CLBAND_PREFIX}' || true"
    ).stdout
    return [s for s in out.splitlines() if s.strip()]


def project_total() -> int:
    """Total SKILL.md files under /skills/project in the volume (262 dogfood + any clband)."""
    return int(_vol_sh(f"find '{PROJECT_ROOT_IN_VOL}' -name SKILL.md | wc -l").stdout.strip() or 0)


def scope_skill_count(name: str) -> int:
    """Count accepted SKILL.md under one clband scope."""
    sp = scope_path(name)
    return int(_vol_sh(f"find '{sp}' -name SKILL.md 2>/dev/null | wc -l").stdout.strip() or 0)


def dogfood_total() -> int:
    """Count dogfood SKILL.md (directly under /skills/project/.skills, excluding clband scopes)."""
    return int(_vol_sh(
        f"find '{PROJECT_ROOT_IN_VOL}/.skills' -name SKILL.md 2>/dev/null | wc -l"
    ).stdout.strip() or 0)


# ── retrieval readiness (poll the REAL mcp-server; never fixed-sleep for the condition) ──

def probe(repo_path: str, prompt: str, session_tag: str = "probe") -> dict:
    """Call the live mcp-server compile_context scoped to repo_path. Returns the parsed result."""
    return efficacy_ab.compile_context_http(
        server_url=MCP_URL, prompt=prompt,
        session_id=f"clband-{session_tag}", repo_path=repo_path, timeout_s=60,
    )


def wait_retrievable(name: str, prompt: str, want_substr: str | None = None,
                     timeout_s: int = 300, interval_s: int = 8) -> dict:
    """Poll until the clband scope returns >=1 skill (or one whose name contains want_substr).

    Returns the final probe result. Raises TimeoutError if the rebuild/reload never surfaces a skill
    within timeout_s (a real stuck-detector deadline, NOT a work cap). The success condition is the
    REAL retrieval, not the clock.
    """
    sp = scope_path(name)
    deadline = time.time() + timeout_s
    last: dict = {}
    attempt = 0
    while time.time() < deadline:
        attempt += 1
        # Unique session per poll: compile_context suppresses duplicate (session, prompt) pairs
        # (status=duplicate_suppressed), which would mask readiness if we reused one session.
        last = probe(sp, prompt, session_tag=f"ready-{name}-{attempt}")
        names = last.get("skill_names", []) or []
        hit = bool(names) if want_substr is None else any(want_substr.lower() in n.lower() for n in names)
        print(f"  [wait_retrievable {name}] attempt {attempt}: status={last.get('raw', {}).get('status')} "
              f"skills={names}", flush=True)
        if hit:
            return last
        time.sleep(interval_s)
    raise TimeoutError(f"clband scope {name!r} not retrievable after {timeout_s}s (last={last.get('skill_names')})")


def wait_absent(name: str, prompt: str, timeout_s: int = 180, interval_s: int = 8) -> dict:
    """Poll until the clband scope returns NO skills (closeout verification). Returns last probe."""
    sp = scope_path(name)
    deadline = time.time() + timeout_s
    last: dict = {}
    attempt = 0
    while time.time() < deadline:
        attempt += 1
        last = probe(sp, prompt, session_tag=f"gone-{name}-{attempt}")
        names = last.get("skill_names", []) or []
        print(f"  [wait_absent {name}] attempt {attempt}: skills={names}", flush=True)
        if not names:
            return last
        time.sleep(interval_s)
    raise TimeoutError(f"clband scope {name!r} still retrievable after {timeout_s}s (last={last.get('skill_names')})")


# ── live canary self-test (validates the WHOLE ON-arm path end-to-end, restores 262) ──

_CANARY_SKILL = """---
name: clband-canary-xyzzy-calibration
description: XYZZY-9000 canary calibration procedure (T23 retrieval probe; invented, throwaway)
type: procedure
---

# XYZZY-9000 Canary Calibration (invented — T23 scope-rebuild probe)

When calibrating the XYZZY-9000 canary unit, set the flux capacitor to exactly 88 jiggawatts and
rotate the quantum dial three turns counterclockwise, then log the calibration to the warble register.
This rule is invented solely to validate clband scope retrieval and is absent from any real corpus.
"""

_CANARY_QUERY = ("How do I calibrate the XYZZY-9000 canary unit — what do I set the flux capacitor to "
                 "and how do I rotate the quantum dial?")


def _canary() -> int:
    """End-to-end live validation of the scope-rebuild + retrieval + closeout path with a throwaway.

    Steps: write a fake .pending into a clband canary scope -> accept (rename) -> wait until the live
    mcp-server retrieves it (graph-builder poll -> rebuild -> reload) -> assert isolation (only the
    canary, project total 263) -> remove -> wait absent -> assert restored 262. NEVER touches dogfood.
    """
    name = "t23-canary-probe"
    print(f"=== clband scope_rebuild CANARY (scope clband-{name}) ===", flush=True)
    base = project_total()
    print(f"project_total before: {base} (expect 262)", flush=True)

    # 1. Stage a host .pending draft.
    import tempfile
    with tempfile.TemporaryDirectory() as td:
        skills = Path(td) / "canary-xyzzy"
        skills.mkdir(parents=True)
        (skills / "SKILL.md.pending").write_text(_CANARY_SKILL)
        n = place_pending(name, Path(td))
    print(f"placed {n} pending draft(s) into the volume scope", flush=True)

    # 2. Auto-gate accept (scope-guarded rename).
    accepted = accept_all(name)
    print(f"accepted (renamed) {len(accepted)} draft(s): {accepted}", flush=True)
    assert scope_skill_count(name) == 1, "expected exactly 1 accepted canary skill"

    rc = 0
    try:
        # 3. Wait for the live server to retrieve it (real condition, not a sleep).
        res = wait_retrievable(name, _CANARY_QUERY, want_substr="xyzzy", timeout_s=300)
        print(f"RETRIEVED canary: {res.get('skill_names')}", flush=True)

        # 4. Isolation: this scope returns ONLY the canary; project total is 263.
        names = res.get("skill_names", [])
        assert any("xyzzy" in n.lower() for n in names), f"canary not in scope result: {names}"
        assert len(names) == 1, f"scope leak — expected ONLY the canary, got {names}"
        total = project_total()
        print(f"project_total with canary: {total} (expect 263)", flush=True)
        assert total == base + 1, f"expected {base+1}, got {total}"
        print("ISOLATION OK: clband scope returns only the canary; dogfood untouched.", flush=True)
    finally:
        # 5. Closeout: remove + verify absent + restored 262 (always runs).
        remove_scope(name)
        try:
            wait_absent(name, _CANARY_QUERY, timeout_s=180)
        except TimeoutError as e:
            print(f"WARNING: {e}", flush=True)
            rc = 1
        restored = project_total()
        print(f"project_total after closeout: {restored} (expect {base})", flush=True)
        if restored != base:
            print(f"ERROR: corpus not restored ({restored} != {base})", flush=True)
            rc = 1

    print(f"=== CANARY {'PASS' if rc == 0 else 'FAIL'} ===", flush=True)
    return rc


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--canary":
        sys.exit(_canary())
    print("usage: scope_rebuild.py --canary   (live end-to-end validation; restores 262)")
    print("  or import as a module: place_pending/accept_all/wait_retrievable/remove_scope/...")
