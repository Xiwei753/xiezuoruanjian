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

use std::collections::BTreeMap;
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
    /// Conditions that must be checked against the real remote baseline by the
    /// final commit, not against the in-round overlay.
    checks: Vec<BatchMutation>,
}

/// Result of the single remote publication at the end of a round.
pub struct RoundFlushOutcome {
    /// Whether a commit/tree change was actually published.
    pub committed: bool,
    pub result: BatchCommitResult,
}

/// Provider wrapper that defers all mutations until [`Self::flush`].
pub struct RoundProvider<'a> {
    inner: &'a dyn SyncProvider,
    base_revision: Option<RemoteVersion>,
    state: Mutex<RoundState>,
}

impl<'a> RoundProvider<'a> {
    pub fn new(inner: &'a dyn SyncProvider, base_revision: Option<RemoteVersion>) -> Self {
        Self {
            inner,
            base_revision,
            state: Mutex::new(RoundState {
                entries: BTreeMap::new(),
                checks: Vec::new(),
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
                let mut state = self.lock_state()?;
                state.checks.push(BatchMutation::CheckObject {
                    path: path.clone(),
                    expected_version: Some(version.clone()),
                });
                state.entries.insert(path.clone(), None);
                Ok(())
            }
            BatchMutation::CheckObject {
                path,
                expected_version,
            } => {
                let mut state = self.lock_state()?;
                state.checks.push(BatchMutation::CheckObject {
                    path: path.clone(),
                    expected_version: expected_version.clone(),
                });
                Ok(())
            }
            BatchMutation::CheckPrefix {
                prefix,
                expected_entries,
            } => {
                let mut state = self.lock_state()?;
                state.checks.push(BatchMutation::CheckPrefix {
                    prefix: prefix.clone(),
                    expected_entries: expected_entries.clone(),
                });
                Ok(())
            }
        }
    }

    /// Number of buffered entries awaiting the final publication.
    pub fn pending_len(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.entries.len())
            .unwrap_or(0)
    }

    /// Drop every buffered write and check, leaving the remote untouched.
    pub fn discard(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.entries.clear();
            state.checks.clear();
        }
    }

    /// Publish every buffered write as one atomic remote commit.
    ///
    /// A non-batch provider receives the same mutations sequentially; the
    /// wrapper exists for one-push semantics, not for provider-specific
    /// batching.
    pub fn flush(&self) -> Result<RoundFlushOutcome, ProviderError> {
        let (entries, checks) = {
            let mut state = self.lock_state()?;
            (
                std::mem::take(&mut state.entries),
                std::mem::take(&mut state.checks),
            )
        };
        if entries.is_empty() && checks.is_empty() {
            return Ok(RoundFlushOutcome {
                committed: false,
                result: BatchCommitResult {
                    revision: self
                        .base_revision
                        .clone()
                        .unwrap_or_else(|| RemoteVersion::new("")),
                    touched_paths: Vec::new(),
                },
            });
        }

        let mut mutations: Vec<BatchMutation> = entries
            .into_iter()
            .filter_map(|(path, entry)| match entry {
                Some(BufferedValue::Content(content)) => Some(BatchMutation::Put { path, content }),
                Some(BufferedValue::Version(version)) => {
                    Some(BatchMutation::ReuseVersion { path, version })
                }
                None => Some(BatchMutation::Delete { path }),
            })
            .collect();
        mutations.extend(checks);

        if self.inner.capabilities().batch {
            let result = self.inner.commit_batch_at(
                self.base_revision.as_ref(),
                &mutations,
                &format!("sujian: sync round ({} mutations)", mutations.len()),
            )?;
            let committed = Some(&result.revision) != self.base_revision.as_ref();
            return Ok(RoundFlushOutcome { committed, result });
        }

        // Non-batch fallback: run baseline checks first, then apply mutations
        // one by one.  This path is not used by the built-in providers.
        for mutation in &mutations {
            match mutation {
                BatchMutation::CheckObject {
                    path,
                    expected_version,
                } => {
                    let actual = self.inner.read(path)?.map(|object| object.version);
                    if &actual != expected_version {
                        return Err(ProviderError::PreconditionFailed {
                            path: path.clone(),
                            reason: "remote object changed before final publish".to_string(),
                        });
                    }
                }
                BatchMutation::CheckPrefix {
                    prefix,
                    expected_entries,
                } => {
                    let actual = self.inner.list(prefix)?;
                    if &actual != expected_entries {
                        return Err(ProviderError::PreconditionFailed {
                            path: prefix.clone(),
                            reason: "remote prefix changed before final publish".to_string(),
                        });
                    }
                }
                _ => {}
            }
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
        let committed = !touched_paths.is_empty();
        Ok(RoundFlushOutcome {
            committed,
            result: BatchCommitResult {
                revision: RemoteVersion::new("round"),
                touched_paths,
            },
        })
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
                // The target path is new (for example a stable path replacing
                // an old generation path).  Resolve the reused blob by finding
                // a currently reachable path with the same version; delegating
                // `read(new_path)` would incorrectly return None.
                let entries = self.inner.list("")?;
                let source = entries.into_iter().find(|entry| entry.version == version);
                let Some(source) = source else {
                    return Err(ProviderError::PreconditionFailed {
                        path: path.to_string(),
                        reason: format!(
                            "reused blob version {} is not reachable",
                            version.as_str()
                        ),
                    });
                };
                let object = self.inner.read(&source.path)?;
                Ok(object.map(|mut object| {
                    object.path = path.to_string();
                    object.version = version;
                    object
                }))
            }
            Some(None) => Ok(None),
            None => self.inner.read(path),
        }
    }

    fn write(
        &self,
        path: &str,
        content: &[u8],
        precondition: WritePrecondition,
    ) -> Result<RemoteVersion, ProviderError> {
        let _ = precondition;
        let version = Self::buffered_version(content);
        let mut state = self.lock_state()?;
        state.entries.insert(
            path.to_string(),
            Some(BufferedValue::Content(content.to_vec())),
        );
        Ok(version)
    }

    fn delete(&self, path: &str, precondition: DeletePrecondition) -> Result<(), ProviderError> {
        let _ = precondition;
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
        let round = RoundProvider::new(&inner, None);

        round
            .write("a.txt", b"a", WritePrecondition::CreateNew)
            .unwrap();
        assert!(inner.read("a.txt").unwrap().is_none());
        assert_eq!(round.read("a.txt").unwrap().unwrap().content, b"a".to_vec());

        round.flush().unwrap();
        assert_eq!(inner.read("a.txt").unwrap().unwrap().content, b"a".to_vec());
    }
}
