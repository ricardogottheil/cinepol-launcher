//! Timer adapters.

use std::time::Duration;

use crate::domain::ports::Delay;

/// `Delay` backed by the tokio timer.
#[derive(Debug, Default, Clone, Copy)]
pub struct TokioDelay;

impl Delay for TokioDelay {
    async fn wait(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}
