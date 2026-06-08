use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::error::Result;

const TRANSACTIONS_DIR: &str = "app-meta/transactions";
const MANIFEST_FILENAME: &str = "manifest.json";
const COMMIT_MARKER: &str = "committed";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionManifest {
    pub transaction_id: String,
    pub created_at_ms: i64,
    pub entries: Vec<TransactionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionEntry {
    pub staging_filename: String,
    pub target_relative: String,
}

pub struct SaveTransaction {
    workspace_path: PathBuf,
    transaction_id: String,
    tx_dir: PathBuf,
    entries: Vec<TransactionEntry>,
    committed: bool,
}

impl SaveTransaction {
    pub fn new(workspace_path: &Path) -> Self {
        let transaction_id = Uuid::new_v4().to_string();
        let tx_dir = workspace_path.join(TRANSACTIONS_DIR).join(&transaction_id);
        Self {
            workspace_path: workspace_path.to_path_buf(),
            transaction_id,
            tx_dir,
            entries: Vec::new(),
            committed: false,
        }
    }

    pub fn transaction_id(&self) -> &str {
        &self.transaction_id
    }

    pub fn add_file(&mut self, target_relative: &str, content: &str) -> Result<()> {
        fs::create_dir_all(&self.tx_dir)?;
        let idx = self.entries.len();
        let staging_filename = format!("file_{}", idx);
        let staging_path = self.tx_dir.join(&staging_filename);
        crate::storage::atomic_write_string(&staging_path, content)?;
        self.entries.push(TransactionEntry {
            staging_filename,
            target_relative: target_relative.to_string(),
        });
        Ok(())
    }

    pub fn commit(&mut self) -> Result<()> {
        if self.entries.is_empty() {
            self.cleanup();
            return Ok(());
        }

        let manifest = TransactionManifest {
            transaction_id: self.transaction_id.clone(),
            created_at_ms: chrono::Utc::now().timestamp_millis(),
            entries: self.entries.clone(),
        };
        let manifest_json = serde_json::to_string_pretty(&manifest)?;
        let manifest_path = self.tx_dir.join(MANIFEST_FILENAME);
        crate::storage::atomic_write_string(&manifest_path, &manifest_json)?;

        for entry in &self.entries {
            let staging_path = self.tx_dir.join(&entry.staging_filename);
            let target_path = self.workspace_path.join(&entry.target_relative);
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&staging_path, &target_path)?;
        }

        let commit_marker = self.tx_dir.join(COMMIT_MARKER);
        let _ = fs::write(&commit_marker, b"ok");

        self.committed = true;
        self.cleanup();
        Ok(())
    }

    fn cleanup(&self) {
        let _ = fs::remove_dir_all(&self.tx_dir);
    }
}

impl Drop for SaveTransaction {
    fn drop(&mut self) {
        if self.committed {
            self.cleanup();
        }
    }
}

pub fn recover_pending_transactions(workspace_path: &Path) -> Vec<TransactionRecovery> {
    let tx_base = workspace_path.join(TRANSACTIONS_DIR);
    if !tx_base.exists() {
        return Vec::new();
    }

    let mut results = Vec::new();
    let entries = match fs::read_dir(&tx_base) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let tx_dir = entry.path();
        if !tx_dir.is_dir() {
            continue;
        }

        let commit_marker = tx_dir.join(COMMIT_MARKER);
        if commit_marker.exists() {
            let _ = fs::remove_dir_all(&tx_dir);
            continue;
        }

        let manifest_path = tx_dir.join(MANIFEST_FILENAME);
        if !manifest_path.exists() {
            let _ = fs::remove_dir_all(&tx_dir);
            continue;
        }

        let manifest: TransactionManifest = match fs::read_to_string(&manifest_path) {
            Ok(s) => match serde_json::from_str(&s) {
                Ok(m) => m,
                Err(_) => {
                    let _ = fs::remove_dir_all(&tx_dir);
                    continue;
                }
            },
            Err(_) => {
                let _ = fs::remove_dir_all(&tx_dir);
                continue;
            }
        };

        let mut recovered_files = Vec::new();
        let mut missing_files = Vec::new();

        for tx_entry in &manifest.entries {
            let staging_path = tx_dir.join(&tx_entry.staging_filename);
            if staging_path.exists() {
                let target_path = workspace_path.join(&tx_entry.target_relative);
                if let Some(parent) = target_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                match fs::rename(&staging_path, &target_path) {
                    Ok(()) => recovered_files.push(tx_entry.target_relative.clone()),
                    Err(e) => {
                        eprintln!(
                            "[transaction] recovery rename failed: {} -> {}: {}",
                            tx_entry.staging_filename, tx_entry.target_relative, e
                        );
                        missing_files.push(tx_entry.target_relative.clone());
                    }
                }
            } else {
                missing_files.push(tx_entry.target_relative.clone());
            }
        }

        if recovered_files.is_empty() && !missing_files.is_empty() {
            eprintln!(
                "[transaction] all staging files lost for tx={}, dropping",
                manifest.transaction_id
            );
        }

        let _ = fs::remove_dir_all(&tx_dir);

        results.push(TransactionRecovery {
            transaction_id: manifest.transaction_id,
            recovered_files,
            missing_files,
        });
    }

    results
}

#[derive(Debug)]
pub struct TransactionRecovery {
    pub transaction_id: String,
    pub recovered_files: Vec<String>,
    pub missing_files: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_transaction_commit_all_files() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path();

        let mut tx = SaveTransaction::new(ws);
        tx.add_file(
            "projects/p1/volumes/v1/chapters/c1/chapter.md",
            "hello world",
        )
        .unwrap();
        tx.add_file(
            "projects/p1/volumes/v1/chapters/c1/chapter.meta.json",
            r#"{"word_count":2}"#,
        )
        .unwrap();
        tx.commit().unwrap();

        let md =
            fs::read_to_string(ws.join("projects/p1/volumes/v1/chapters/c1/chapter.md")).unwrap();
        assert_eq!(md, "hello world");

        let meta =
            fs::read_to_string(ws.join("projects/p1/volumes/v1/chapters/c1/chapter.meta.json"))
                .unwrap();
        assert_eq!(meta, r#"{"word_count":2}"#);

        let tx_base = ws.join(TRANSACTIONS_DIR);
        assert!(!tx_base.exists() || fs::read_dir(&tx_base).unwrap().count() == 0);
    }

    #[test]
    fn test_transaction_recovery_staging_exists() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path();

        let mut tx = SaveTransaction::new(ws);
        tx.add_file("projects/p1/volumes/v1/chapters/c1/chapter.md", "recovered")
            .unwrap();
        tx.add_file(
            "projects/p1/volumes/v1/chapters/c1/chapter.meta.json",
            r#"{"word_count":1}"#,
        )
        .unwrap();

        let tx_dir = ws.join(TRANSACTIONS_DIR).join(tx.transaction_id());
        fs::create_dir_all(&tx_dir).unwrap();

        let manifest = TransactionManifest {
            transaction_id: tx.transaction_id().to_string(),
            created_at_ms: chrono::Utc::now().timestamp_millis(),
            entries: tx.entries.clone(),
        };
        let manifest_json = serde_json::to_string_pretty(&manifest).unwrap();
        fs::write(tx_dir.join(MANIFEST_FILENAME), &manifest_json).unwrap();

        for entry in &tx.entries {
            let staging_path = tx_dir.join(&entry.staging_filename);
            let target_path = ws.join(&entry.target_relative);
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(
                &staging_path,
                fs::read_to_string(target_path).unwrap_or_default(),
            )
            .unwrap();
        }

        let recovered = recover_pending_transactions(ws);
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].recovered_files.len(), 2);
        assert!(recovered[0].missing_files.is_empty());
    }

    #[test]
    fn test_transaction_recovery_staging_lost() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path();

        let tx_dir = ws.join(TRANSACTIONS_DIR).join("test-tx-id");
        fs::create_dir_all(&tx_dir).unwrap();

        let manifest = TransactionManifest {
            transaction_id: "test-tx-id".to_string(),
            created_at_ms: chrono::Utc::now().timestamp_millis(),
            entries: vec![TransactionEntry {
                staging_filename: "file_0".to_string(),
                target_relative: "projects/p1/volumes/v1/chapters/c1/chapter.md".to_string(),
            }],
        };
        let manifest_json = serde_json::to_string_pretty(&manifest).unwrap();
        fs::write(tx_dir.join(MANIFEST_FILENAME), &manifest_json).unwrap();

        let recovered = recover_pending_transactions(ws);
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].recovered_files.len(), 0);
        assert_eq!(recovered[0].missing_files.len(), 1);
    }

    #[test]
    fn test_transaction_empty_commit() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path();

        let mut tx = SaveTransaction::new(ws);
        tx.commit().unwrap();

        assert!(tx.entries.is_empty());
    }
}
