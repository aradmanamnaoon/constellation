//! Bounded in-memory conditional store, suitable for deterministic tests and
//! local examples. No durable state or network transport is provided.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use crate::{CasResult, PutResult, RevisionId, RevisionStore, StoreError, MAX_ENVELOPE_BYTES};

const MAX_OBJECTS: usize = 1024;

#[derive(Default)]
struct State {
    head: Option<RevisionId>,
    objects: BTreeMap<RevisionId, Vec<u8>>,
}

/// Clones represent independent devices targeting one shared atomic store.
#[derive(Clone)]
pub struct MemoryStore {
    state: Arc<Mutex<State>>,
    cas_supported: bool,
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            cas_supported: true,
        }
    }

    /// Simulates providers with no guaranteed conditional-write feature.
    pub fn without_cas() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            cas_supported: false,
        }
    }

    pub fn revisions_len(&self) -> std::result::Result<usize, StoreError> {
        self.state
            .lock()
            .map(|state| state.objects.len())
            .map_err(|_| StoreError::Poisoned)
    }
}

impl RevisionStore for MemoryStore {
    fn supports_verified_cas(&self) -> bool {
        self.cas_supported
    }

    fn head(&self) -> std::result::Result<Option<RevisionId>, StoreError> {
        self.state
            .lock()
            .map(|state| state.head)
            .map_err(|_| StoreError::Poisoned)
    }

    fn get_revision(
        &self,
        id: RevisionId,
        max_bytes: usize,
    ) -> std::result::Result<Option<Vec<u8>>, StoreError> {
        let state = self.state.lock().map_err(|_| StoreError::Poisoned)?;
        if let Some(bytes) = state.objects.get(&id) {
            if bytes.len() > max_bytes {
                return Err(StoreError::SizeLimit);
            }
            Ok(Some(bytes.clone()))
        } else {
            Ok(None)
        }
    }

    fn put_if_absent(
        &self,
        id: RevisionId,
        bytes: &[u8],
    ) -> std::result::Result<PutResult, StoreError> {
        if bytes.len() > MAX_ENVELOPE_BYTES {
            return Err(StoreError::SizeLimit);
        }
        if !self.cas_supported {
            return Err(StoreError::UnsupportedCas);
        }
        let mut state = self.state.lock().map_err(|_| StoreError::Poisoned)?;
        if let Some(existing) = state.objects.get(&id) {
            return if existing == bytes {
                Ok(PutResult::AlreadyPresent)
            } else {
                Err(StoreError::ImmutableConflict)
            };
        }
        if state.objects.len() >= MAX_OBJECTS {
            return Err(StoreError::SizeLimit);
        }
        state.objects.insert(id, bytes.to_vec());
        Ok(PutResult::Created)
    }

    fn compare_and_swap_head(
        &self,
        expected: Option<RevisionId>,
        next: RevisionId,
    ) -> std::result::Result<CasResult, StoreError> {
        if !self.cas_supported {
            return Err(StoreError::UnsupportedCas);
        }
        let mut state = self.state.lock().map_err(|_| StoreError::Poisoned)?;
        if !state.objects.contains_key(&next) {
            return Err(StoreError::Failed);
        }
        if state.head != expected {
            return Ok(CasResult::Conflict {
                current: state.head,
            });
        }
        state.head = Some(next);
        Ok(CasResult::Swapped)
    }
}
