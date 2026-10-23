//! Domain layer: entities, value objects, ports and domain errors.
//!
//! No I/O lives here. Nothing in this module may import `application` or
//! `infrastructure` (enforced by `tests/architecture.rs`).

pub mod account;
pub mod instance;
pub mod integrity;
pub mod java;
pub mod launch;
pub mod loader;
pub mod modpack;
pub mod ports;
pub mod saved_server;
pub mod server;
pub mod server_list;
pub mod settings;
pub mod update;
