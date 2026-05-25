# rust-file-io
tags: rust, io

Read and write files in Rust safely.

## Procedures
- Use `std::fs::read_to_string` for simple text reads.
- Return explicit errors instead of panicking.

## Conventions
- Keep I/O helpers deterministic for testability.
