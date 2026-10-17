//! Application layer: use cases, one per file.
//!
//! Use cases depend only on `domain` (entities and ports). They never import
//! `infrastructure` (enforced by `tests/architecture.rs`).

pub mod create_instance;
pub mod create_offline_profile;
pub mod install_instance;
pub mod launch_instance;
pub mod list_accounts;
pub mod list_game_versions;
pub mod list_instances;
pub mod run_progress_demo;
pub mod select_account;
