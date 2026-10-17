//! Registry of running long operations, so the UI can cancel them by id.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

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
        validate_id(id)?;
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        if running.contains_key(id) {
            return Err(AppError::invalid_input(format!(
                "operation {id} is already running"
            )));
        }
        let token = CancellationToken::new();
        running.insert(id.to_owned(), token.clone());
        Ok(token)
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
}
