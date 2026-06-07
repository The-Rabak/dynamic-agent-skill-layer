---
name: rust-file-io
description: Asynchronous and synchronous file I/O patterns for Rust applications, covering buffered reads, streaming writes, atomic file replacement, temporary file management, and cross-platform path handling.
tags:
- rust
- file
- io
- tokio
- error-handling
- performance
---

# rust-file-io

Asynchronous and synchronous file I/O patterns for Rust applications, covering buffered reads, streaming writes, atomic file replacement, temporary file management, and cross-platform path handling.

## Procedures

### Async File Read with Tokio
- Use `tokio::fs::read_to_string` for small files that fit in memory; this reads the entire file in one syscall and returns `Result<String>`.
- For large files, use `tokio::fs::File::open` paired with `tokio::io::BufReader` and `.read_line()` in a loop to stream lines without holding the whole file in memory.
- Always await inside the async context: `let contents = tokio::fs::read_to_string("data.json").await?;`
- When reading binary data, prefer `tokio::fs::read` which returns `Vec<u8>` without utf-8 validation overhead.
- Set appropriate read buffer sizes via `BufReader::with_capacity` for files where the default 8KB buffer is insufficient.

### Synchronous Fallback Patterns
- Use `std::fs::read_to_string` for initialization code, configuration loading during startup, or when running outside a tokio runtime.
- Wrap synchronous I/O in `tokio::task::spawn_blocking` when calling from an async context to avoid blocking the event loop: `tokio::task::spawn_blocking(|| std::fs::read_to_string(path)).await??`.
- Never call `std::fs::read` directly inside `async fn` without `spawn_blocking` — this blocks the entire worker thread.
- Prefer `std::fs::File::open` and manual buffering when you need precise control over OS file handles.

### Atomic File Writes
- Write to a temporary file first, then atomically rename: `tokio::fs::write(temp_path, content).await?; tokio::fs::rename(temp_path, final_path).await?;`
- Use `tempfile::NamedTempFile` for automatic cleanup if the write fails mid-flight.
- On Unix, `rename` is atomic within the same filesystem; on Windows, use `std::fs::rename` which may not be atomic — consider `ReplaceFileW` via the `windows` crate for true atomicity.
- Always `flush` or `sync_all` before rename to ensure data is on disk before the atomic swap.

### Error Propagation and Context
- Return `anyhow::Result<T>` or `Result<T, io::Error>` from I/O functions — never unwrap or panic in library code.
- Attach context with `.context("failed to read config file")?` using the `anyhow` crate to produce actionable error messages.
- For libraries, define a custom error enum with `#[from] io::Error` to preserve the underlying filesystem error.
- Handle `ErrorKind::NotFound` explicitly for optional files: `if err.kind() == io::ErrorKind::NotFound { return Ok(None); }`.
- Distinguish transient errors (permission denied during high load) from permanent ones (file missing) and retry only the former.

### Directory Traversal and Path Safety
- Use `tokio::fs::canonicalize` to resolve symlinks and relative paths before operating on files.
- Validate that resolved paths stay within allowed roots: `if !canonical.starts_with(&allowed_root) { return Err(...); }`.
- Never construct paths from raw user input without sanitization — use `Path::join` and `Path::file_name` to strip traversal sequences.
- Prefer `camino::Utf8Path` or `std::path::Path` methods over string concatenation for platform-agnostic path construction.

### Large File Streaming
- For files > 100MB, use `tokio::io::BufReader` with `read_exact` or `read_buf` in a loop rather than buffering the entire file.
- Stream writes with `tokio::io::BufWriter` to batch small writes into fewer syscalls; flush explicitly at logical boundaries.
- Monitor memory via `tokio::io::copy` which uses an internal 8KB buffer and streams efficiently between reader and writer.
- Set `BufWriter::with_capacity` based on the underlying storage block size (typically 64KB for SSDs).

### Temporary File Patterns
- Use `tempfile::tempfile()` for anonymous temporary files that need no filesystem path.
- Use `tempfile::NamedTempFile` when the temp file needs to be renamed into place atomically.
- Clean up temporary files in a `Drop` guard or `defer!` macro to prevent disk leakage on panic.
- Create temp files in `/dev/shm` on Linux for maximum throughput when the data is short-lived and fits in RAM.

### Concurrency and File Locks
- Use `tokio::sync::RwLock` to coordinate read/write access to file paths within the same process.
- For cross-process coordination, use `fs2::FileExt::lock_exclusive` or `fd-lock` crate for advisory file locks.
- Implement a write-ahead log pattern for multi-step file mutations: write intent, commit, then rename.
- Avoid holding file locks across `.await` points — release the lock before yielding.

## Conventions

- All I/O functions must be async-first when running inside a tokio runtime; synchronous equivalents provided via `blocking` feature gate.
- File paths are always validated against an allowed root directory before any read or write operation.
- Error types must implement `std::error::Error + Send + Sync` for compatibility with `anyhow` and `eyre`.
- Use `#[instrument]` from `tracing` on all public I/O functions to capture file paths and operation durations in spans.
- Prefer `BufReader` and `BufWriter` over raw file handles — unbuffered I/O is almost never appropriate.
- File format detection uses magic bytes, not file extensions, for security-sensitive parsing.
- Filesystem operations are tested with `tempfile::TempDir` to avoid polluting the working directory.
- Maximum file size is validated before allocation to prevent OOM from malformed inputs.

## Assets

```rust
// Robust async file read with size validation and error context
async fn read_config(path: &Path, max_size: u64) -> anyhow::Result<String> {
    let metadata = tokio::fs::metadata(path).await
        .with_context(|| format!("failed to stat {}", path.display()))?;
    anyhow::ensure!(metadata.len() <= max_size, "config exceeds {} bytes", max_size);
    tokio::fs::read_to_string(path).await
        .with_context(|| format!("failed to read {}", path.display()))
}

// Atomic file write with tempfile
async fn atomic_write(path: &Path, content: &str) -> anyhow::Result<()> {
    let dir = path.parent().context("no parent directory")?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tokio::fs::write(tmp.path(), content).await?;
    tmp.persist(path)?;
    Ok(())
}

// Path validation within allowed root
fn within_root(path: &Path, root: &Path) -> bool {
    path.canonicalize()
        .map(|canonical| canonical.starts_with(root))
        .unwrap_or(false)
}
```