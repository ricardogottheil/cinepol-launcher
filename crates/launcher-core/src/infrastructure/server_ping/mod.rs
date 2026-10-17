//! Server List Ping: the live status of a Minecraft server (SRV lookup, handshake, status JSON, latency).

mod codec;
mod pinger;
mod srv;
mod status_json;

pub use pinger::{DEFAULT_TIMEOUT, MinecraftServerPinger};
pub use srv::{SrvRecord, SrvResolver, SystemSrvResolver};
