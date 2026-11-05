//! Cinepol Launcher core.
//!
//! Layering (dependencies point inwards only):
//! `infrastructure` -> `application` -> `domain`.
//!
//! This crate must never depend on `tauri`; the desktop shell in `src-tauri`
//! is a driving adapter that wires infrastructure into use cases.
//! Unsafe code is denied everywhere except the two FFI modules of the ICMP
//! hop prober (`infrastructure::diagnostics::icmp`), which opt in explicitly.
#![deny(unsafe_code)]

pub mod application;
pub mod domain;
pub mod infrastructure;

#[cfg(any(test, feature = "test-support"))]
pub mod testing;
