//! Cross-cutting ports: traits the application layer depends on and
//! infrastructure implements. Feature-specific ports (repositories, sources)
//! live next to their entities, e.g. `domain::account::AccountRepository`.

mod clock;
mod delay;
mod progress;
mod secrets;

pub use clock::{Clock, unix_seconds};
pub use delay::Delay;
pub use progress::{ProgressReporter, ProgressUpdate};
pub use secrets::{Secret, SecretKey, SecretStore, SecretStoreError};
