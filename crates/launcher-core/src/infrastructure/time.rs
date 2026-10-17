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

/// `Clock` backed by the system wall clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl crate::domain::ports::Clock for SystemClock {
    fn now(&self) -> std::time::SystemTime {
        std::time::SystemTime::now()
    }
}
