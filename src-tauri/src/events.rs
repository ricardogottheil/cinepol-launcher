//! Typed app-wide events (game exited, account expired...).
//!
//! Progress of long-running operations goes through `tauri::ipc::Channel`
//! instead of global events.
