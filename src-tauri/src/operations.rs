//! Registry of running long operations, so the UI can cancel them by id.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use launcher_core::domain::instance::InstanceId;
use launcher_core::domain::launch::RunningGames;
use tokio_util::sync::CancellationToken;

use crate::error::AppError;

const MAX_ID_LEN: usize = 64;

#[derive(Debug, Default)]
pub struct OperationRegistry {
    running: Mutex<HashMap<String, CancellationToken>>,
}

impl OperationRegistry {
    /// Registers an operation id chosen by the frontend and returns its token.
    pub fn start(&self, id: &str) -> Result<CancellationToken, AppError> {
        let token = CancellationToken::new();
        self.adopt(id, token.clone())?;
        Ok(token)
    }

    /// Registers `id` with a token that already exists, so cancelling `id`
    /// also stops whatever that token stops.
    pub fn adopt(&self, id: &str, token: CancellationToken) -> Result<(), AppError> {
        validate_id(id)?;
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        if running.contains_key(id) {
            return Err(AppError::invalid_input(format!(
                "operation {id} is already running"
            )));
        }
        running.insert(id.to_owned(), token);
        Ok(())
    }

    /// Forgets a finished operation.
    pub fn finish(&self, id: &str) {
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
    }

    /// Cancels a running operation. Returns `false` when the id is unknown.
    pub fn cancel(&self, id: &str) -> bool {
        let running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        running.get(id).map(CancellationToken::cancel).is_some()
    }
}

/// The running-games registry, keyed by instance id, seen by the core.
impl RunningGames for OperationRegistry {
    fn claim(&self, instance: &InstanceId, kill: &CancellationToken) -> bool {
        self.adopt(instance.as_str(), kill.clone()).is_ok()
    }

    fn release(&self, instance: &InstanceId) {
        self.finish(instance.as_str());
    }
}

/// Lets one session of a kind run at a time (one Play on CINEPOL, so two
/// presses never race to create the managed instance twice).
#[derive(Debug, Default)]
pub struct SessionGate {
    busy: AtomicBool,
}

impl SessionGate {
    /// `None` while another session holds the gate.
    pub fn try_enter(&self) -> Option<SessionPass<'_>> {
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| SessionPass { gate: self })
    }
}

/// Holds the gate until dropped.
pub struct SessionPass<'a> {
    gate: &'a SessionGate,
}

impl Drop for SessionPass<'_> {
    fn drop(&mut self) {
        self.gate.busy.store(false, Ordering::Release);
    }
}

fn validate_id(id: &str) -> Result<(), AppError> {
    let valid = !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if valid {
        Ok(())
    } else {
        Err(AppError::invalid_input(
            "operation id must be 1-64 characters of [A-Za-z0-9-]",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn cancels_a_running_operation() {
        let registry = OperationRegistry::default();
        let token = registry.start("op-1").unwrap();
        assert!(registry.cancel("op-1"));
        assert!(token.is_cancelled());
    }

    #[test]
    fn unknown_or_finished_operations_cannot_be_cancelled() {
        let registry = OperationRegistry::default();
        assert!(!registry.cancel("missing"));
        registry.start("op-1").unwrap();
        registry.finish("op-1");
        assert!(!registry.cancel("op-1"));
    }

    #[test]
    fn rejects_duplicate_and_malformed_ids() {
        let registry = OperationRegistry::default();
        registry.start("op-1").unwrap();
        for id in ["op-1", "", "../etc", &"a".repeat(65)] {
            assert_eq!(
                registry.start(id).unwrap_err().code,
                ErrorCode::InvalidInput,
                "{id}"
            );
        }
    }

    #[test]
    fn a_claimed_instance_is_stopped_through_the_games_registry() {
        let games = OperationRegistry::default();
        let id = InstanceId::parse("cinepol").unwrap();
        let session = CancellationToken::new();
        assert!(games.claim(&id, &session));
        assert!(
            games.start("cinepol").is_err(),
            "a launch from the instances page is refused"
        );
        assert!(!games.claim(&id, &CancellationToken::new()));
        assert!(games.cancel("cinepol"), "kill_instance reaches the session");
        assert!(session.is_cancelled());
        games.release(&id);
        assert!(games.start("cinepol").is_ok());
    }

    #[test]
    fn the_session_gate_lets_one_session_in_at_a_time() {
        let gate = SessionGate::default();
        let first = gate.try_enter();
        assert!(first.is_some());
        assert!(
            gate.try_enter().is_none(),
            "a second session waits for the first"
        );
        drop(first);
        assert!(gate.try_enter().is_some(), "free again once the first ends");
    }
}
