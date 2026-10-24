//! Application layer: use cases, one per file.
//!
//! Use cases depend only on `domain` (entities and ports). They never import
//! `infrastructure` (enforced by `tests/architecture.rs`).

pub mod account_capes;
pub mod account_skin;
pub mod change_skin;
pub mod check_app_update;
pub mod check_premium_name;
pub mod check_server_status;
pub mod create_instance;
pub mod create_offline_profile;
pub mod delete_instance;
pub mod duplicate_instance;
pub mod edit_server_list;
pub mod export_mrpack;
pub mod get_modpack_details;
pub mod get_settings;
pub mod install_app_update;
pub mod install_curseforge;
pub mod install_instance;
pub mod install_mod;
pub mod install_modpack;
pub mod install_packwiz;
pub mod launch_instance;
pub mod list_accounts;
pub mod list_game_versions;
pub mod list_instance_mods;
pub mod list_instances;
pub mod list_loader_versions;
pub mod list_saved_servers;
pub mod load_server_config;
pub mod login_microsoft;
pub mod mod_inventory;
pub mod modpack_files;
pub mod ping_server;
pub mod plan_mod_install;
pub mod play_on_cinepol;
pub mod prepare_cinepol_instance;
pub mod refresh_session;
pub mod remove_account;
pub mod remove_saved_server;
pub mod resolve_manual_downloads;
pub mod run_progress_demo;
pub mod save_server;
pub mod search_modpacks;
pub mod select_account;
pub mod set_cape;
pub mod update_curseforge;
pub mod update_instance_settings;
pub mod update_modpack;
pub mod update_packwiz;
pub mod update_settings;
