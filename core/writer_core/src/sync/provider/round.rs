//! One-round deferred provider writes.
//!
//! A full sync round reads one fixed remote snapshot, decides every object with
//! LWW, and publishes the complete result once.  This wrapper collects the
//! provider writes performed while executing the planned targets and emits a
//! single `commit_batch` (or a sequential fallback for non-batch providers) at
//! the end of the round.
//!
//! Reads and `list` calls see the buffered overlay, so the catalog CAS helpers
//! keep working against the latest in-round state without network round trips.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use crate::sync::provider::capabilities::SyncCapabilities;
use crate::sync::provider::error::ProviderError;
use crate::sync::provider::model::{
    BatchCommitResult, BatchMutation, DeletePrecondition, RemoteEntry, RemoteObject, RemoteVersion,
    WritePrecondition,
};
use crate::sync::provider::{ProviderNetworkMetrics, SyncProvider};

#[derive(Clone)]
enum BufferedValue {
    Content(Vec<u8>),
    Version(RemoteVersion),
}

struct RoundState {
    entries: BTreeMap<String, Option<BufferedValue>>,
    /// Versions observed by `read`/`list` during this round.  They let later
    /// conditional writes check against the same snapshot without another read.
    observed_versions: HashMap<String, Option<RemoteVersion>>,
}

/// Provider wrapper that defers all mutations until [`Self::flush`].
pub struct RoundProvider<'a> {
    inner: &'a dyn SyncProvider,
    state: Mutex<RoundState>,
}

impl<'a> RoundProvider<'a> {
    pub fn new(inner: &'a dyn SyncProvider) -> Self {
        Self {
            inner,
            state: Mutex::new(RoundState {
                entries: BTreeMap::new(),
                observed_versions: HashMap::new(),
            }),
        }
    }

    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, RoundState>, ProviderError> {
        self.state.lock().map_err(|_| ProviderError::Other {
            reason: "sync round buffer lock poisoned".to_string(),
        })
    }

    fn buffered_version(content: &[u8]) -> RemoteVersion {
        RemoteVersion::new(format!("round:{}", crate::sync::hash::content_md5(content)))
    }

    fn entry_value(entry: &Option<BufferedValue>) -> Option<RemoteVersion> {
        entry.as_ref().map(|value| match value {
            BufferedValue::Content(content) => Self::buffered_version(content),
            BufferedValue::Version(version) => version.clone(),
        })
    }

    fn current_version(&self, path: &str) -> Result<Option<RemoteVersion>, ProviderError> {
        let buffered = {
            let state = self.lock_state()?;
            if let Some(entry) = state.entries.get(path) {
                Some(Self::entry_value(entry))
            } else {
                state.observed_versions.get(path).cloned()
            }
        };
        if let Some(version) = buffered {
            return Ok(version);
        }
        let object = self.inner.read(path)?;
        let version = object.map(|object| object.version);
        let mut state = self.lock_state()?;
        state
            .observed_versions
            .insert(path.to_string(), version.clone());
        Ok(version)
    }

    fn overlay_entries(&self, prefix: &str) -> Result<Vec<RemoteEntry>, ProviderError> {
        let mut entries: BTreeMap<String, RemoteVersion> = self
            .inner
            .list(prefix)?
            .into_iter()
            .map(|entry| (entry.path, entry.version))
            .collect();
        let state = self.lock_state()?;
        let needle = if prefix.is_empty() {
            String::new()
        } else {
            format!("{prefix}/")
        };
        for (path, entry) in &state.entries {
            let relative = if needle.is_empty() {
                path.as_str()
            } else if let Some(relative) = path.strip_prefix(&needle) {
                relative
            } else {
                continue;
            };
            match entry {
                Some(value) => {
                    let version = match value {
                        BufferedValue::Content(content) => Self::buffered_version(content),
                        BufferedValue::Version(version) => version.clone(),
                    };
                    entries.insert(relative.to_string(), version);
                }
                None => {
                    entries.remove(relative);
                }
            }
        }
        Ok(entries
            .into_iter()
            .map(|(path, version)| RemoteEntry { path, version })
            .collect())
    }

    fn check_write(
        &self,
        path: &str,
        precondition: &WritePrecondition,
    ) -> Result<(), ProviderError> {
        let current = self.current_version(path)?;
        let matches = match precondition {
            WritePrecondition::Unconditional => true,
            WritePrecondition::CreateNew => current.is_none(),
            WritePrecondition::IfMatch(expected) => current.as_ref() == Some(expected),
        };
        if matches {
            Ok(())
        } else {
            Err(ProviderError::PreconditionFailed {
                path: path.to_string(),
                reason: "buffered write precondition failed".to_string(),
            })
        }
    }

    fn check_delete(
        &self,
        path: &str,
        precondition: &DeletePrecondition,
    ) -> Result<(), ProviderError> {
        let current = self.current_version(path)?;
        let matches = match precondition {
            DeletePrecondition::Unconditional => true,
            DeletePrecondition::IfMatch(expected) => current.as_ref() == Some(expected),
        };
        if matches {
            Ok(())
        } else {
            Err(ProviderError::PreconditionFailed {
                path: path.to_string(),
                reason: "buffered delete precondition failed".to_string(),
            })
        }
    }

    fn apply_mutation(&self, mutation: &BatchMutation) -> Result<(), ProviderError> {
        match mutation {
            BatchMutation::Put { path, content } => {
                let mut state = self.lock_state()?;
                state
                    .entries
                    .insert(path.clone(), Some(BufferedValue::Content(content.clone())));
                Ok(())
            }
            BatchMutation::ReuseVersion { path, version } => {
                let mut state = self.lock_state()?;
                state
                    .entries
                    .insert(path.clone(), Some(BufferedValue::Version(version.clone())));
                Ok(())
            }
            BatchMutation::Delete { path } => {
                let mut state = self.lock_state()?;
                state.entries.insert(path.clone(), None);
                Ok(())
            }
            BatchMutation::DeleteIfMatch { path, version } => {
                self.check_delete(path, &DeletePrecondition::IfMatch(version.clone()))?;
                let mut state = self.lock_state()?;
                state.entries.insert(path.clone(), None);
                Ok(())
            }
            BatchMutation::CheckObject {
                path,
                expected_version,
            } => {
                let current = self.current_version(path)?;
                if &current == expected_version {
                    Ok(())
                } else {
                    Err(ProviderError::PreconditionFailed {
                        path: path.clone(),
                        reason: "buffered object precondition failed".to_string(),
                    })
                }
            }
            BatchMutation::CheckPrefix {
                prefix,
                expected_entries,
            } => {
                let actual = self.overlay_entries(prefix)?;
                if &actual == expected_entries {
                    Ok(())
                } else {
                    Err(ProviderError::PreconditionFailed {
                        path: prefix.clone(),
                        reason: "buffered prefix precondition failed".to_string(),
                    })
                }
            }
        }
    }

    /// Number of paths changed in this round before the final commit.
    pub fn pending_len(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.entries.len())
            .unwrap_or(0)
    }

    /// Publish every buffered write as one atomic remote commit.
    ///
    /// Returns `None` when the round produced no mutation.  A non-batch
    /// provider receives the same mutations sequentially; the wrapper exists
    /// for one-push semantics, not for provider-specific batching.
    pub fn flush(&self) -> Result<Option<BatchCommitResult>, ProviderError> {
        let entries = {
            let mut state = self.lock_state()?;
            std::mem::take(&mut state.entries)
        };
        if entries.is_empty() {
            return Ok(None);
        }

        let mutations: Vec<BatchMutation> = entries
            .into_iter()
            .filter_map(|(path, entry)| match entry {
                Some(BufferedValue::Content(content)) => Some(BatchMutation::Put { path, content }),
                Some(BufferedValue::Version(version)) => {
                    Some(BatchMutation::ReuseVersion { path, version })
                }
                None => Some(BatchMutation::Delete { path }),
            })
            .collect();

        if self.inner.capabilities().batch {
            let result = self.inner.commit_batch(
                &mutations,
                &format!("sujian: sync round ({} changes)", mutations.len()),
            )?;
            return Ok(Some(result));
        }

        let mut touched_paths = Vec::new();
        for mutation in &mutations {
            match mutation {
                BatchMutation::Put { path, content } => {
                    self.inner
                        .write(path, content, WritePrecondition::Unconditional)?;
                    touched_paths.push(path.clone());
                }
                BatchMutation::ReuseVersion { path, .. } => {
                    // A non-batch provider cannot reuse a remote blob directly;
                    // the caller must have supplied new content instead.
                    return Err(ProviderError::Other {
                        reason: format!("non-batch provider cannot reuse version for path {path}"),
                    });
                }
                BatchMutation::Delete { path } => {
                    self.inner.delete(path, DeletePrecondition::Unconditional)?;
                    touched_paths.push(path.clone());
                }
                BatchMutation::DeleteIfMatch { .. }
                | BatchMutation::CheckObject { .. }
                | BatchMutation::CheckPrefix { .. } => {}
            }
        }
        Ok(Some(BatchCommitResult {
            revision: RemoteVersion::new("round"),
            touched_paths,
        }))
    }
}

impl SyncProvider for RoundProvider<'_> {
    fn set_operation_id(&self, operation_id: Option<&str>) {
        self.inner.set_operation_id(operation_id);
    }

    fn network_metrics(&self) -> ProviderNetworkMetrics {
        self.inner.network_metrics()
    }

    fn capabilities(&self) -> SyncCapabilities {
        self.inner.capabilities()
    }

    fn remote_revision(&self) -> Result<Option<String>, ProviderError> {
        self.inner.remote_revision()
    }

    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, ProviderError> {
        self.overlay_entries(prefix)
    }

    fn read(&self, path: &str) -> Result<Option<RemoteObject>, ProviderError> {
        let buffered = {
            let state = self.lock_state()?;
            state.entries.get(path).cloned()
        };
        match buffered {
            Some(Some(BufferedValue::Content(content))) => {
                let version = Self::buffered_version(&content);
                Ok(Some(RemoteObject {
                    path: path.to_string(),
                    content,
                    version,
                }))
            }
            Some(Some(BufferedValue::Version(version))) => {
                let object = self.inner.read(path)?;
                Ok(object.map(|mut object| {
                    object.version = version;
                    object
                }))
            }
            Some(None) => Ok(None),
            None => {
                let object = self.inner.read(path)?;
                let version = object.as_ref().map(|object| object.version.clone());
                let mut state = self.lock_state()?;
                state.observed_versions.insert(path.to_string(), version);
                Ok(object)
            }
        }
    }

    fn write(
        &self,
        path: &str,
        content: &[u8],
        precondition: WritePrecondition,
    ) -> Result<RemoteVersion, ProviderError> {
        self.check_write(path, &precondition)?;
        let version = Self::buffered_version(content);
        let mut state = self.lock_state()?;
        state.entries.insert(
            path.to_string(),
            Some(BufferedValue::Content(content.to_vec())),
        );
        Ok(version)
    }

    fn delete(&self, path: &str, precondition: DeletePrecondition) -> Result<(), ProviderError> {
        self.check_delete(path, &precondition)?;
        let mut state = self.lock_state()?;
        state.entries.insert(path.to_string(), None);
        Ok(())
    }

    fn commit_batch(
        &self,
        mutations: &[BatchMutation],
        _message: &str,
    ) -> Result<BatchCommitResult, ProviderError> {
        for mutation in mutations {
            self.apply_mutation(mutation)?;
        }
        Ok(BatchCommitResult {
            revision: RemoteVersion::new("round"),
            touched_paths: mutations
                .iter()
                .filter(|mutation| {
                    !matches!(
                        mutation,
                        BatchMutation::CheckObject { .. } | BatchMutation::CheckPrefix { .. }
                    )
                })
                .map(|mutation| mutation.path().to_string())
                .collect(),
        })
    }

    fn diagnose(&self) -> Result<crate::sync::types::SyncDiagnosticsResult, ProviderError> {
        self.inner.diagnose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::provider::memory::MemoryProvider;
    use crate::sync::provider::SyncProvider;

    #[test]
    fn writes_are_deferred_until_flush_and_visible_to_reads() {
        let inner = MemoryProvider::new();
        let round = RoundProvider::new(&inner);

        round
            .write("a.txt", b"a", WritePrecondition::CreateNew)
            .unwrap();
        assert!(inner.read("a.txt").unwrap().is_none());
        assert_eq!(round.read("a.txt").unwrap().unwrap().content, b"a".to_vec());

        round.flush().unwrap();
        assert_eq!(inner.read("a.txt").unwrap().unwrap().content, b"a".to_vec());
    }
}
