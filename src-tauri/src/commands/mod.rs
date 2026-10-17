//! Thin `#[tauri::command]` functions.
//!
//! Each command validates its input, calls exactly one use case, and maps
//! the result or error to a DTO. No business logic lives here. Every command
//! is registered in `crate::bindings::commands` for the TypeScript map.

pub mod app;
pub mod demo;
