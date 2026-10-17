//! Application layer: use cases, one per file.
//!
//! Use cases depend only on `domain` (entities and ports). They never import
//! `infrastructure` (enforced by `tests/architecture.rs`).

pub mod run_progress_demo;
