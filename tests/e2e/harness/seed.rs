/// Sidecar-based skill file writer and approver for the real-infra E2E harness.
///
/// The test-global-skills and test-project-skills volumes are mounted `:ro` in
/// the `graph-builder` and `mcp-server` containers, so the harness cannot write
/// directly.  Instead it runs a transient `alpine:3.23.4` sidecar that mounts
/// the same volume read-write and performs the write/rename via `docker run --rm`.
///
/// # SKILL.md format (unified)
/// YAML frontmatter is the authoritative source for name / description / tags;
/// the markdown body carries the title, description prose, and subunit sections.
/// ```text
/// ---
/// name: <name>
/// description: <description>
/// tags:
/// - a
/// - b
/// ---
///
/// # <name>
///
/// <description>
///
/// ## Procedures
/// - ...
///
/// ## Conventions
/// - ...
/// ```
///
/// # Human gate
/// `write_pending` drops `SKILL.md.pending`; `approve` renames it to `SKILL.md`
/// via a second sidecar call.  The graph-builder only picks up approved files
/// (`SKILL.md`, not `.pending`).
use std::process::Command;

use super::stack::{GLOBAL_SKILLS_VOLUME, PROJECT_SKILLS_VOLUME};

/// Which skills volume to target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillScope {
    Global,
    Project,
}

impl SkillScope {
    /// Returns the Docker volume name for this scope.
    pub fn volume_name(&self) -> &'static str {
        match self {
            SkillScope::Global => GLOBAL_SKILLS_VOLUME,
            SkillScope::Project => PROJECT_SKILLS_VOLUME,
        }
    }

    /// Returns the mount path inside the sidecar container.
    pub fn mount_path(&self) -> &'static str {
        match self {
            SkillScope::Global => "/skills/global",
            SkillScope::Project => "/skills/project",
        }
    }
}

/// Writes `skill_md` content as `SKILL.md.pending` under `<mount>/<slug>/`.
///
/// The pending file is intentionally NOT picked up by graph-builder; call
/// `approve` to rename it to `SKILL.md` once any pre-approval checks are done.
///
/// Returns `Err` with diagnostics when the sidecar command fails.
pub fn write_pending(scope: SkillScope, slug: &str, skill_md: &str) -> Result<(), String> {
    let volume = scope.volume_name();
    let mount = scope.mount_path();
    let pending_path = format!("{mount}/{slug}/SKILL.md.pending");

    // Build the inline shell script: create directory then write file content.
    // The skill_md content is passed as an env var to avoid shell quoting issues.
    let script =
        format!("mkdir -p {mount}/{slug} && printf '%s' \"$SKILL_CONTENT\" > {pending_path}");

    let output = Command::new("docker")
        .args(["run", "--rm"])
        .arg(format!("-v={volume}:{mount}"))
        .arg("-e=SKILL_CONTENT")
        .env("SKILL_CONTENT", skill_md)
        .args(["alpine:3.23.4", "sh", "-c", &script])
        .output()
        .map_err(|e| format!("failed to spawn sidecar for write_pending({slug}): {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        Err(format!(
            "sidecar write_pending({slug}) failed ({})\nstdout: {stdout}\nstderr: {stderr}",
            output.status
        ))
    }
}

/// Renames `SKILL.md.pending` → `SKILL.md` inside the named volume.
///
/// This is the "human gate" rename that causes graph-builder to pick up the
/// skill on its next poll cycle (`GRAPH_BUILDER_POLL_INTERVAL_MS=5000`).
///
/// Returns `Err` when the sidecar `mv` command fails (e.g. the pending file
/// does not exist yet).
pub fn approve(scope: SkillScope, slug: &str) -> Result<(), String> {
    let volume = scope.volume_name();
    let mount = scope.mount_path();
    let pending = format!("{mount}/{slug}/SKILL.md.pending");
    let approved = format!("{mount}/{slug}/SKILL.md");

    let script = format!("mv {pending} {approved}");

    let output = Command::new("docker")
        .args(["run", "--rm"])
        .arg(format!("-v={volume}:{mount}"))
        .args(["alpine:3.23.4", "sh", "-c", &script])
        .output()
        .map_err(|e| format!("failed to spawn sidecar for approve({slug}): {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        Err(format!(
            "sidecar approve({slug}) failed ({})\nstdout: {stdout}\nstderr: {stderr}",
            output.status
        ))
    }
}

/// Removes the entire `<slug>/` directory from the named volume.
///
/// Safe to call for both `SKILL.md` and `SKILL.md.pending` — the whole slug
/// directory is deleted regardless of which files it contains.
///
/// Returns `Err` when the sidecar `rm -rf` command fails.
pub fn remove(scope: SkillScope, slug: &str) -> Result<(), String> {
    let volume = scope.volume_name();
    let mount = scope.mount_path();
    let dir = format!("{mount}/{slug}");

    let script = format!("rm -rf {dir}");

    let output = Command::new("docker")
        .args(["run", "--rm"])
        .arg(format!("-v={volume}:{mount}"))
        .args(["alpine:3.23.4", "sh", "-c", &script])
        .output()
        .map_err(|e| format!("failed to spawn sidecar for remove({slug}): {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        Err(format!(
            "sidecar remove({slug}) failed ({})\nstdout: {stdout}\nstderr: {stderr}",
            output.status
        ))
    }
}

/// Convenience: writes a pending file and immediately approves it in one call.
///
/// Equivalent to `write_pending(scope, slug, skill_md)` followed by
/// `approve(scope, slug)`.  Returns the first error encountered.
pub fn seed_and_approve(scope: SkillScope, slug: &str, skill_md: &str) -> Result<(), String> {
    write_pending(scope, slug, skill_md)?;
    approve(scope, slug)
}

/// Lists all slug directories currently present in the named volume.
///
/// Runs a transient alpine sidecar and executes `ls` on the mount root.
/// Returns each directory name as a `String`.  Directories whose names
/// end with `.pending` are NOT excluded — the caller is responsible for
/// filtering if needed.
///
/// Returns `Err` when the sidecar command fails or the output is not
/// valid UTF-8.
pub fn list(scope: SkillScope) -> Result<Vec<String>, String> {
    let volume = scope.volume_name();
    let mount = scope.mount_path();

    // `ls -1` outputs one entry per line; non-zero exit (empty dir) still
    // succeeds — so we interpret an empty stdout as an empty list.
    let script = format!("ls -1 {mount} 2>/dev/null || true");

    let output = Command::new("docker")
        .args(["run", "--rm"])
        .arg(format!("-v={volume}:{mount}"))
        .args(["alpine:3.23.4", "sh", "-c", &script])
        .output()
        .map_err(|e| format!("failed to spawn sidecar for list({scope:?}): {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "sidecar list({scope:?}) failed ({})\nstderr: {stderr}",
            output.status
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let slugs = stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();

    Ok(slugs)
}
