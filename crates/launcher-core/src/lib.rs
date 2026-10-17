//! Cinepol Launcher core.
//!
//! Layering (dependencies point inwards only):
//! `infrastructure` -> `application` -> `domain`.
//!
//! This crate must never depend on `tauri`; the desktop shell in `src-tauri`
//! is a driving adapter that wires infrastructure into use cases.
#![forbid(unsafe_code)]

pub mod application;
pub mod domain;
pub mod infrastructure;
