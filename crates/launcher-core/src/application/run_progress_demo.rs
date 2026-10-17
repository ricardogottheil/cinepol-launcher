//! Demo use case for the IPC skeleton: reports `steps` progress updates,
//! one per `interval`, and stops when cancelled.

use std::time::Duration;

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::domain::ports::{Delay, ProgressReporter, ProgressUpdate};

/// Upper bound on demo steps, so a caller cannot start an endless operation.
pub const MAX_STEPS: u32 = 100;
/// Upper bound on the interval between steps.
pub const MAX_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressDemoInput {
    pub steps: u32,
    pub interval: Duration,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProgressDemoError {
    #[error("steps must be between 1 and {MAX_STEPS}, got {0}")]
    InvalidSteps(u32),
    #[error("interval must be at most {} ms", MAX_INTERVAL.as_millis())]
    InvalidInterval,
    #[error("the operation was cancelled")]
    Cancelled,
}

pub async fn run_progress_demo<R, D>(
    input: ProgressDemoInput,
    reporter: &R,
    delay: &D,
    cancel: &CancellationToken,
) -> Result<(), ProgressDemoError>
where
    R: ProgressReporter,
    D: Delay,
{
    if input.steps == 0 || input.steps > MAX_STEPS {
        return Err(ProgressDemoError::InvalidSteps(input.steps));
    }
    if input.interval > MAX_INTERVAL {
        return Err(ProgressDemoError::InvalidInterval);
    }

    let total = u64::from(input.steps);
    for done in 1..=total {
        let waited = cancel.run_until_cancelled(delay.wait(input.interval)).await;
        if waited.is_none() || cancel.is_cancelled() {
            return Err(ProgressDemoError::Cancelled);
        }
        reporter.report(ProgressUpdate {
            done,
            total: Some(total),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct RecordingReporter {
        updates: Mutex<Vec<ProgressUpdate>>,
    }

    impl RecordingReporter {
        fn updates(&self) -> Vec<ProgressUpdate> {
            self.updates.lock().unwrap().clone()
        }
    }

    impl ProgressReporter for RecordingReporter {
        fn report(&self, update: ProgressUpdate) {
            self.updates.lock().unwrap().push(update);
        }
    }

    /// Returns immediately; optionally cancels a token after `cancel_after` waits.
    struct InstantDelay {
        waits: Mutex<u32>,
        cancel_after: Option<(u32, CancellationToken)>,
    }

    impl InstantDelay {
        fn new() -> Self {
            Self {
                waits: Mutex::new(0),
                cancel_after: None,
            }
        }

        fn cancelling(after: u32, token: CancellationToken) -> Self {
            Self {
                waits: Mutex::new(0),
                cancel_after: Some((after, token)),
            }
        }
    }

    impl Delay for InstantDelay {
        async fn wait(&self, _duration: Duration) {
            let mut waits = self.waits.lock().unwrap();
            *waits += 1;
            if let Some((after, token)) = &self.cancel_after
                && *waits == *after
            {
                token.cancel();
            }
        }
    }

    fn input(steps: u32) -> ProgressDemoInput {
        ProgressDemoInput {
            steps,
            interval: Duration::from_millis(10),
        }
    }

    #[tokio::test]
    async fn reports_one_update_per_step_with_the_total() {
        let reporter = RecordingReporter::default();
        let result = run_progress_demo(
            input(3),
            &reporter,
            &InstantDelay::new(),
            &CancellationToken::new(),
        )
        .await;

        assert_eq!(result, Ok(()));
        assert_eq!(
            reporter.updates(),
            (1..=3)
                .map(|done| ProgressUpdate {
                    done,
                    total: Some(3)
                })
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn stops_with_cancelled_when_the_token_is_cancelled() {
        let reporter = RecordingReporter::default();
        let token = CancellationToken::new();
        let delay = InstantDelay::cancelling(2, token.clone());

        let result = run_progress_demo(input(5), &reporter, &delay, &token).await;

        assert_eq!(result, Err(ProgressDemoError::Cancelled));
        assert_eq!(reporter.updates().len(), 1);
    }

    #[tokio::test]
    async fn rejects_zero_and_too_many_steps() {
        let reporter = RecordingReporter::default();
        let token = CancellationToken::new();
        for steps in [0, MAX_STEPS + 1] {
            let result =
                run_progress_demo(input(steps), &reporter, &InstantDelay::new(), &token).await;
            assert_eq!(result, Err(ProgressDemoError::InvalidSteps(steps)));
        }
        assert!(reporter.updates().is_empty());
    }

    #[tokio::test]
    async fn rejects_an_interval_above_the_maximum() {
        let reporter = RecordingReporter::default();
        let too_slow = ProgressDemoInput {
            steps: 1,
            interval: MAX_INTERVAL + Duration::from_millis(1),
        };
        let result = run_progress_demo(
            too_slow,
            &reporter,
            &InstantDelay::new(),
            &CancellationToken::new(),
        )
        .await;
        assert_eq!(result, Err(ProgressDemoError::InvalidInterval));
    }
}
