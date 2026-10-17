use std::future::Future;
use std::time::Duration;

/// Waits for a duration. Lets use cases pace work without depending on a
/// runtime timer, and lets tests run instantly.
pub trait Delay: Send + Sync {
    fn wait(&self, duration: Duration) -> impl Future<Output = ()> + Send;
}
