//! Infrastructure layer: adapters implementing the domain ports
//! (reqwest, keyring, file system, tokio processes, Mojang/loader APIs...).

pub mod archive;
pub mod cinepol;
pub mod curseforge;
pub mod fs;
pub mod http;
pub mod java;
pub mod keychain;
pub mod loaders;
pub mod mod_catalogs;
pub mod modrinth;
pub mod mojang;
pub mod msa;
pub mod nbt;
pub mod packwiz;
pub mod process;
pub mod server_ping;
pub mod time;
