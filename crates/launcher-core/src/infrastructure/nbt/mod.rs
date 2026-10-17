//! NBT storage: the per-instance `servers.dat` (uncompressed), preserving unknown tags.

mod codec;
mod store;

pub use store::NbtServerListStore;
