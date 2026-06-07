/// Panic-safe RAII cleanup guard for volume-seeded skills.
///
/// # Purpose
/// Tests that seed skills into the Docker volumes (`test-global-skills`,
/// `test-project-skills`) via the sidecar pattern MUST remove those skills
/// after the test completes — even when the test panics. Without cleanup,
/// leftover skills contaminate subsequent tests that share the same global
/// scope (e.g. the retrieval-quality harness, which measures ranking quality
/// against a controlled corpus).
///
/// # Design (mirrors `NamespaceGuard::Drop` from `tests/integration/env_guard.rs`)
/// The guard records every `(scope, slug)` pair seeded by a test.  On
/// `Drop` — which runs on both the happy path AND during a panic unwind —
/// the guard spawns a dedicated thread, builds a single-threaded tokio
/// runtime on that thread, and calls `seed::remove` for every recorded
/// slug synchronously, then joins.  The join ensures the volumes are clean
/// before the drop returns.
///
/// On the happy path callers should prefer the `cleanup()` method (which
/// runs the same removal without the thread-spawn overhead and without the
/// redundant join), and then set `cleaned = true` so the `Drop` impl is a
/// no-op.  If `cleanup()` is never called (panic path or early return), the
/// `Drop` impl reclaims the skills automatically.
///
/// # Failure policy
/// Removal failures are logged loudly to stderr — they are NOT swallowed.
/// A failed removal means the volume is still contaminated; the next test
/// run will encounter foreign skills. Loud logging surfaces this gap
/// immediately rather than hiding it behind a silent swallow.
///
/// # Example
/// ```rust,ignore
/// let guard = SeededSkillGuard::new();
/// seed::seed_and_approve(SkillScope::Global, "my-slug", &content)?;
/// guard.record(SkillScope::Global, "my-slug");
/// // … run test …
/// guard.cleanup(); // removes the skill; Drop is a no-op afterwards.
/// ```
use super::seed::{self, SkillScope};

/// An entry tracked by the guard: one seeded skill that must be removed.
#[derive(Debug, Clone)]
struct SeededSkill {
    scope: SkillScope,
    slug: String,
}

/// RAII guard that removes volume-seeded skills on drop (happy path or panic).
///
/// See the module-level documentation for the full design rationale.
pub struct SeededSkillGuard {
    /// Skills registered with [`SeededSkillGuard::record`] that must be
    /// removed when the guard is dropped.
    skills: Vec<SeededSkill>,
    /// Set to `true` by [`SeededSkillGuard::cleanup`] so the `Drop` fallback
    /// knows the work is already done and skips the thread-spawn.
    cleaned: bool,
}

impl SeededSkillGuard {
    /// Creates an empty guard with no skills registered yet.
    pub fn new() -> Self {
        Self {
            skills: Vec::new(),
            cleaned: false,
        }
    }

    /// Registers a seeded skill for cleanup.
    ///
    /// Call this immediately after a successful `seed::approve` (or
    /// `seed::seed_and_approve`) so the guard's cleanup list stays in sync
    /// with the actual volume state.  Calling `record` for a slug that was
    /// never successfully seeded is harmless — `seed::remove` on a
    /// non-existent directory is a no-op.
    pub fn record(&mut self, scope: SkillScope, slug: &str) {
        self.skills.push(SeededSkill {
            scope,
            slug: slug.to_owned(),
        });
    }

    /// Removes all registered skills from their volumes (happy-path teardown).
    ///
    /// Logs each removal failure loudly to stderr — does not swallow errors.
    /// After this call the guard is marked `cleaned` so the `Drop` fallback
    /// is a no-op and the thread-spawn overhead is avoided.
    ///
    /// Prefer this over relying on `Drop`: it runs synchronously on the
    /// caller's thread without the dedicated-thread overhead, which matters
    /// for test suites that call cleanup inside an `async fn`.
    pub fn cleanup(mut self) {
        remove_all(&self.skills);
        self.cleaned = true;
        // self drops here; Drop sees cleaned=true and is a no-op.
    }
}

impl Default for SeededSkillGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SeededSkillGuard {
    /// Panic-safe synchronous fallback: spawns a dedicated thread with its
    /// own tokio runtime and removes all registered skills.
    ///
    /// # Why a dedicated thread?
    /// `Drop` is synchronous. We may be executing inside a tokio
    /// `#[test]` that is already running a runtime, so we cannot call
    /// `block_on` on the current thread (that would panic). Instead, we
    /// spawn a fresh thread (which has no runtime), build one via
    /// `Builder::new_current_thread()`, and run the removals there.
    ///
    /// The `join()` ensures the volumes are clean before `Drop` returns —
    /// this is the same pattern used by `NamespaceGuard::Drop`.
    fn drop(&mut self) {
        if self.cleaned || self.skills.is_empty() {
            return;
        }
        let skills = self.skills.clone();
        if let Ok(handle) = std::thread::Builder::new()
            .name("seeded-skill-guard-cleanup".to_owned())
            .spawn(move || {
                remove_all(&skills);
            })
        {
            // Block until removal finishes so the volumes are clean before Drop returns.
            let _ = handle.join();
        }
    }
}

/// Calls `seed::remove` for every skill in `skills`, logging failures loudly.
///
/// Intentionally non-fatal: if a remove fails, the test outcome already
/// determined; the only cost of a missed removal is volume contamination for
/// the next run. Logging gives the operator a clear signal that manual
/// cleanup is needed.
fn remove_all(skills: &[SeededSkill]) {
    for entry in skills {
        if let Err(e) = seed::remove(entry.scope, &entry.slug) {
            eprintln!(
                "[SeededSkillGuard] WARN: remove({:?}, {:?}) failed: {e} \
                 — the volume may be contaminated; run `docker run --rm \
                 -v {volume}:{mount} alpine:3.23.4 rm -rf {mount}/{slug}` to clean manually",
                entry.scope,
                entry.slug,
                volume = entry.scope.volume_name(),
                mount = entry.scope.mount_path(),
                slug = entry.slug,
            );
        }
    }
}
