//! Thin `#[tauri::command]` functions.
//!
//! Each command validates its input, calls exactly one use case, and maps
//! the result or error to a DTO. No business logic lives here.
