// Good fixture: try_update derives 'applied' from the return value of rcu (prev Arc),
// not from a variable mutated inside the closure.
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

    /// Conditionally swap to new_snap if its version is newer.
    /// Derives 'applied' from the rcu return value (the previous Arc),
    /// not from any mutation inside the closure.
    pub fn try_update(&self, new_snap: Arc<Snapshot>) -> bool {
        let mut guard = self.current.write().unwrap();
        let prev = Arc::clone(&*guard);
        // rcu body: no external mutation — pure function of argument
        let next = if new_snap.version > prev.version {
            Arc::clone(&new_snap)
        } else {
            Arc::clone(&prev)
        };
        *guard = next;
        // Derive outcome from the previous Arc's version, outside the closure
        new_snap.version > prev.version
    }
}
