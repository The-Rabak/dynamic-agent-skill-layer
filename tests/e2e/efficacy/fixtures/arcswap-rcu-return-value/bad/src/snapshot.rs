// Bad fixture: try_update mutates `applied` inside the closure body (the bug).
use std::sync::Arc;

pub struct Snapshot {
    pub version: u64,
    pub data: Vec<u8>,
}

pub struct SnapshotStore {
    current: std::sync::RwLock<Arc<Snapshot>>,
}

impl SnapshotStore {
    pub fn new(initial: Arc<Snapshot>) -> Self {
        Self { current: std::sync::RwLock::new(initial) }
    }

    pub fn try_update(&self, new_snap: Arc<Snapshot>) -> bool {
        let mut applied = false;
        let mut guard = self.current.write().unwrap();
        let current = Arc::clone(&*guard);
        // BUG: mutating `applied` inside the closure — unreliable under CAS retry
        let next = if new_snap.version > current.version {
            applied = true;
            Arc::clone(&new_snap)
        } else {
            Arc::clone(&current)
        };
        *guard = next;
        applied
    }
}
