//! Application layer: use cases, one per file.
//!
//! Use cases depend only on `domain` (entities and ports). They never import
//! `infrastructure` (enforced by `tests/architecture.rs`).

pub mod check_server_status;
pub mod create_instance;
pub mod create_offline_profile;
pub mod get_settings;
pub mod install_instance;
pub mod launch_instance;
pub mod list_accounts;
pub mod list_game_versions;
pub mod list_instances;
pub mod load_server_config;
pub mod play_on_cinepol;
pub mod prepare_cinepol_instance;
pub mod run_progress_demo;
pub mod select_account;
pub mod update_settings;
