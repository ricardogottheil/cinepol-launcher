//! Typed app-wide events (game exited, account expired...).
//!
//! Progress of long-running operations goes through `tauri::ipc::Channel`
//! instead of global events.

/// Emitted with a `LaunchOutcomeDto` when a game started by the launcher exits.
pub const GAME_EXITED: &str = "game-exited";
