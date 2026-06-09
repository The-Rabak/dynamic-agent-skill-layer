use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use domain::{
    ACTIVE_SKILL_FILE_NAME, PENDING_SKILL_FILE_NAME, REJECTED_SKILL_FILE_NAME,
    RETIRED_SKILL_FILE_NAME, ScopeRoot, ScopeType, has_lifecycle_file_name,
};
use notify::{Config, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{Debouncer, FileIdMap, new_debouncer_opt};
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillFileChangeKind {
    Created,
    Modified,
    Deleted,
    ApprovedRename,
    RejectedRename,
    RetiredRename,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileChangeSource {
    Direct,
    PendingApproval,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillFileChange {
    pub scope_id: String,
    pub scope_type: ScopeType,
    pub file_path: PathBuf,
    pub kind: SkillFileChangeKind,
    pub source: FileChangeSource,
    pub content_hash: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillFileFingerprint {
    pub content_hash: String,
    pub modified_nanos: u128,
}

pub type SkillSnapshot = BTreeMap<PathBuf, SkillFileFingerprint>;

#[derive(Debug, Error)]
pub enum WatcherError {
    #[error("scope root does not exist: {0}")]
    MissingScopeRoot(String),
    #[error("watcher setup failed: {0}")]
    WatchSetup(String),
    #[error("watcher IO failure: {0}")]
    Io(#[from] std::io::Error),
}

/// Watches skill directories and reports canonical file-change events.
pub struct SkillWatcher {
    scopes: Vec<ScopeRoot>,
    _debouncer: Debouncer<RecommendedWatcher, FileIdMap>,
    previous_snapshot: SkillSnapshot,
    current_snapshot: SkillSnapshot,
}

impl SkillWatcher {
    pub fn new(scopes: Vec<ScopeRoot>) -> Result<Self, WatcherError> {
        let mut debouncer = new_debouncer_opt::<_, RecommendedWatcher, FileIdMap>(
            std::time::Duration::from_secs(1),
            None,
            |_: Result<Vec<_>, _>| {},
            FileIdMap::new(),
            Config::default(),
        )
        .map_err(|error| WatcherError::WatchSetup(error.to_string()))?;

        for scope in &scopes {
            if !scope.root.exists() {
                return Err(WatcherError::MissingScopeRoot(
                    scope.root.display().to_string(),
                ));
            }
            // Watch the scope root non-recursively, then add a recursive watch for
            // each non-ignored immediate child. The scope root is frequently the
            // repository root; recursively watching it whole made the debouncer's
            // FileIdMap walk `target/`/`.git` (gigabytes of build artifacts and VCS
            // objects) at boot, pegging the process at 100% CPU before it could
            // ever serve /health. Pruning those subtrees keeps watch setup bounded
            // to real source/skill trees. (Change detection itself is the polled
            // `build_snapshot`, which applies the same `is_ignored_walk_dir`
            // filter — so skipping these here loses no detection coverage.)
            debouncer
                .watch(&scope.root, RecursiveMode::NonRecursive)
                .map_err(|error| WatcherError::WatchSetup(error.to_string()))?;
            for entry in fs::read_dir(&scope.root)
                .map_err(|error| WatcherError::WatchSetup(error.to_string()))?
            {
                let entry = entry.map_err(|error| WatcherError::WatchSetup(error.to_string()))?;
                if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                    continue;
                }
                if entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| matches!(name, ".git" | "target" | "node_modules"))
                {
                    continue;
                }
                debouncer
                    .watch(entry.path(), RecursiveMode::Recursive)
                    .map_err(|error| WatcherError::WatchSetup(error.to_string()))?;
            }
        }

        Ok(Self {
            scopes,
            _debouncer: debouncer,
            previous_snapshot: SkillSnapshot::new(),
            current_snapshot: SkillSnapshot::new(),
        })
    }

    pub fn scopes(&self) -> Vec<ScopeRoot> {
        self.scopes.clone()
    }

    pub fn previous_snapshot(&self) -> SkillSnapshot {
        self.previous_snapshot.clone()
    }

    pub fn current_snapshot(&self) -> SkillSnapshot {
        self.current_snapshot.clone()
    }

    pub fn collect_file_changes(&mut self) -> Result<Vec<SkillFileChange>, WatcherError> {
        let discovered = build_snapshot(&self.scopes)?;
        let changes = diff_skill_snapshots(&self.scopes, &self.current_snapshot, &discovered);
        self.previous_snapshot = self.current_snapshot.clone();
        self.current_snapshot = discovered;
        Ok(changes)
    }
}

pub fn build_snapshot(scopes: &[ScopeRoot]) -> Result<SkillSnapshot, WatcherError> {
    let mut snapshot = SkillSnapshot::new();
    for scope in scopes {
        for entry in WalkDir::new(&scope.root)
            .into_iter()
            .filter_entry(|entry| !is_ignored_walk_dir(entry))
        {
            let entry = entry.map_err(|error| WatcherError::Io(std::io::Error::other(error)))?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if !is_skill_file(path) {
                continue;
            }
            let content = fs::read(path)?;
            let metadata = fs::metadata(path)?;
            let modified = metadata
                .modified()
                .unwrap_or(SystemTime::UNIX_EPOCH)
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            snapshot.insert(
                path.to_path_buf(),
                SkillFileFingerprint {
                    content_hash: blake3::hash(&content).to_hex().to_string(),
                    modified_nanos: modified,
                },
            );
        }
    }
    Ok(snapshot)
}

pub fn diff_skill_snapshots(
    scopes: &[ScopeRoot],
    previous: &SkillSnapshot,
    current: &SkillSnapshot,
) -> Vec<SkillFileChange> {
    let scope_index: HashMap<PathBuf, (String, ScopeType)> = scopes
        .iter()
        .map(|scope| {
            (
                scope.root.clone(),
                (scope.scope_id.clone(), scope.scope_type.to_owned()),
            )
        })
        .collect();
    let mut changes = Vec::new();

    let mut removed_pending_by_hash = HashMap::<String, Vec<PathBuf>>::new();
    let mut removed_active_by_hash = HashMap::<String, Vec<PathBuf>>::new();
    for (path, old_fp) in previous {
        if !current.contains_key(path) {
            if is_pending_file(path) {
                push_removed_path(
                    &mut removed_pending_by_hash,
                    old_fp.content_hash.clone(),
                    path.clone(),
                );
            } else if is_active_skill_file(path) {
                push_removed_path(
                    &mut removed_active_by_hash,
                    old_fp.content_hash.clone(),
                    path.clone(),
                );
            } else if let Some((scope_id, scope_type)) = scope_for_path(path, &scope_index) {
                changes.push(build_change(
                    scope_id,
                    scope_type,
                    path.clone(),
                    SkillFileChangeKind::Deleted,
                    FileChangeSource::Direct,
                    old_fp.content_hash.clone(),
                ));
            }
        }
    }

    for (path, new_fp) in current {
        match previous.get(path) {
            None => {
                if is_active_skill_file(path)
                    && take_removed_path_for_directory(
                        &mut removed_pending_by_hash,
                        &new_fp.content_hash,
                        path,
                    )
                    .is_some()
                {
                    if let Some((scope_id, scope_type)) = scope_for_path(path, &scope_index) {
                        changes.push(build_change(
                            scope_id,
                            scope_type,
                            path.clone(),
                            SkillFileChangeKind::ApprovedRename,
                            FileChangeSource::PendingApproval,
                            new_fp.content_hash.clone(),
                        ));
                    }
                } else if is_rejected_file(path)
                    && take_removed_path_for_directory(
                        &mut removed_pending_by_hash,
                        &new_fp.content_hash,
                        path,
                    )
                    .is_some()
                {
                    if let Some((scope_id, scope_type)) = scope_for_path(path, &scope_index) {
                        changes.push(build_change(
                            scope_id,
                            scope_type,
                            path.clone(),
                            SkillFileChangeKind::RejectedRename,
                            FileChangeSource::PendingApproval,
                            new_fp.content_hash.clone(),
                        ));
                    }
                } else if is_retired_file(path)
                    && take_removed_path_for_directory(
                        &mut removed_active_by_hash,
                        &new_fp.content_hash,
                        path,
                    )
                    .is_some()
                {
                    if let Some((scope_id, scope_type)) = scope_for_path(path, &scope_index) {
                        changes.push(build_change(
                            scope_id,
                            scope_type,
                            path.clone(),
                            SkillFileChangeKind::RetiredRename,
                            FileChangeSource::Direct,
                            new_fp.content_hash.clone(),
                        ));
                    }
                } else if let Some((scope_id, scope_type)) = scope_for_path(path, &scope_index) {
                    let source = if is_pending_file(path) {
                        FileChangeSource::PendingApproval
                    } else {
                        FileChangeSource::Direct
                    };
                    changes.push(build_change(
                        scope_id,
                        scope_type,
                        path.clone(),
                        SkillFileChangeKind::Created,
                        source,
                        new_fp.content_hash.clone(),
                    ));
                }
            }
            Some(previous_fp) if previous_fp.content_hash != new_fp.content_hash => {
                if let Some((scope_id, scope_type)) = scope_for_path(path, &scope_index) {
                    let source = if is_pending_file(path) {
                        FileChangeSource::PendingApproval
                    } else {
                        FileChangeSource::Direct
                    };
                    changes.push(build_change(
                        scope_id,
                        scope_type,
                        path.clone(),
                        SkillFileChangeKind::Modified,
                        source,
                        new_fp.content_hash.clone(),
                    ));
                }
            }
            _ => {}
        }
    }

    for (content_hash, pending_paths) in removed_pending_by_hash {
        for pending_path in pending_paths {
            if let Some((scope_id, scope_type)) = scope_for_path(&pending_path, &scope_index) {
                changes.push(build_change(
                    scope_id,
                    scope_type,
                    pending_path,
                    SkillFileChangeKind::Deleted,
                    FileChangeSource::PendingApproval,
                    content_hash.clone(),
                ));
            }
        }
    }
    for (content_hash, active_paths) in removed_active_by_hash {
        for active_path in active_paths {
            if let Some((scope_id, scope_type)) = scope_for_path(&active_path, &scope_index) {
                changes.push(build_change(
                    scope_id,
                    scope_type,
                    active_path,
                    SkillFileChangeKind::Deleted,
                    FileChangeSource::Direct,
                    content_hash.clone(),
                ));
            }
        }
    }

    changes.sort_by(|left, right| left.idempotency_key.cmp(&right.idempotency_key));
    changes
}

fn scope_for_path(
    path: &Path,
    scope_index: &HashMap<PathBuf, (String, ScopeType)>,
) -> Option<(String, ScopeType)> {
    scope_index.iter().find_map(|(root, scope)| {
        if path.starts_with(root) {
            Some(scope.clone())
        } else {
            None
        }
    })
}

fn build_change(
    scope_id: String,
    scope_type: ScopeType,
    path: PathBuf,
    kind: SkillFileChangeKind,
    source: FileChangeSource,
    content_hash: String,
) -> SkillFileChange {
    let idempotency_key = format!("{}:{}", path.display(), content_hash);
    SkillFileChange {
        scope_id,
        scope_type,
        file_path: path,
        kind,
        source,
        content_hash,
        idempotency_key,
    }
}

/// Directory names pruned from skill-file scans and filesystem watches.
///
/// Two groups:
/// - **Build / VCS noise** (`target`, `.git`, `node_modules`): the project scope
///   root is frequently the repository root, and recursively walking/watching
///   these gigabyte-scale trees pegged graph-builder at 100% CPU before it could
///   ever serve `/health`.
/// - **Coding-harness provider skill homes** (`.github`, `.claude`, `.opencode`,
///   …): each harness ships its OWN built-in skills under these dotdirs. The
///   skill layer manages its own skills — the dedicated project `.skills/` dir and
///   the configured global root — and must NEVER ingest, merge, or *retire* a
///   harness's built-in skills as if they were layer-managed (doing so wrote
///   `.retired` markers across the harness skill trees and "retired" the user's
///   harness skills). Pruning the provider dirs keeps the managed corpus to the
///   layer's own skills. Extend this list as new harnesses appear.
const IGNORED_DIR_NAMES: &[&str] = &[
    // Build / VCS noise.
    ".git",
    "target",
    "node_modules",
    // Coding-harness provider skill homes (built-in skills the layer must not manage).
    ".github",
    ".claude",
    ".opencode",
    ".cursor",
    ".continue",
    ".aider",
    ".codeium",
    ".windsurf",
    ".gemini",
    ".amazonq",
];

/// Whether a `WalkDir`/watch entry is a directory we must never descend into when
/// scanning for skill files (see [`IGNORED_DIR_NAMES`]).
///
/// Used via `WalkDir::filter_entry`, which prunes the entire subtree when this
/// returns `true`. Only directories are matched, so files are always considered.
pub(crate) fn is_ignored_walk_dir(entry: &walkdir::DirEntry) -> bool {
    entry.file_type().is_dir()
        && entry
            .file_name()
            .to_str()
            .is_some_and(|name| IGNORED_DIR_NAMES.contains(&name))
}

pub(crate) fn is_active_skill_file(path: &Path) -> bool {
    has_lifecycle_file_name(path, ACTIVE_SKILL_FILE_NAME)
}

pub(crate) fn is_pending_file(path: &Path) -> bool {
    has_lifecycle_file_name(path, PENDING_SKILL_FILE_NAME)
}

fn is_retired_file(path: &Path) -> bool {
    has_lifecycle_file_name(path, RETIRED_SKILL_FILE_NAME)
}

fn is_rejected_file(path: &Path) -> bool {
    has_lifecycle_file_name(path, REJECTED_SKILL_FILE_NAME)
}

fn same_skill_directory(left: &Path, right: &Path) -> bool {
    left.parent() == right.parent()
}

fn push_removed_path(
    removed_paths_by_hash: &mut HashMap<String, Vec<PathBuf>>,
    content_hash: String,
    path: PathBuf,
) {
    removed_paths_by_hash
        .entry(content_hash)
        .or_default()
        .push(path);
}

/// Consumes one removed path for the same content hash in the same skill directory.
fn take_removed_path_for_directory(
    removed_paths_by_hash: &mut HashMap<String, Vec<PathBuf>>,
    content_hash: &str,
    target_path: &Path,
) -> Option<PathBuf> {
    let mut matched_path = None;
    let mut should_remove_hash = false;

    if let Some(candidates) = removed_paths_by_hash.get_mut(content_hash)
        && let Some(position) = candidates
            .iter()
            .position(|candidate| same_skill_directory(candidate, target_path))
    {
        matched_path = Some(candidates.remove(position));
        should_remove_hash = candidates.is_empty();
    }

    if should_remove_hash {
        removed_paths_by_hash.remove(content_hash);
    }

    matched_path
}

/// Returns true when a path is one of the filesystem skill-state contract files.
pub(crate) fn is_skill_file(path: &Path) -> bool {
    is_active_skill_file(path)
        || is_pending_file(path)
        || is_retired_file(path)
        || is_rejected_file(path)
}
