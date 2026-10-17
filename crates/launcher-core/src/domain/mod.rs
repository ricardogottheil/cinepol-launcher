//! Domain layer: entities, value objects, ports and domain errors.
//!
//! No I/O lives here. Nothing in this module may import `application` or
//! `infrastructure` (enforced by `tests/architecture.rs`).

pub mod ports;
