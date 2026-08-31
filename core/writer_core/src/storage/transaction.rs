//! # 多文件事务写入
//!
//! 当一次保存需要同时写入多个文件（如章节正文 `chapter.md` + 元数据 `chapter.meta.json`）时，
//! 使用本模块确保"全部成功或全部不变"。
//!
//! ## 两阶段提交协议
//!
//! 1. **暂存阶段**（`add_file`）：将每个文件内容写入事务目录下的临时文件
//! 2. **提交阶段**（`commit`）：
//!    a. 写入 `manifest.json`（记录每个暂存文件到目标路径的映射）
//!    b. 逐个 `fs::rename` 暂存文件到最终目标路径
//!    c. 写入 `committed` 标记文件
//!    d. 清理事务目录
//!
//! ## 崩溃恢复
//!
//! 启动时调用 `recover_pending_transactions`：
//! - 存在 `committed` 标记 → 事务已完成，清理目录
//! - 存在 `manifest.json` 但无 `committed` → 事务中断，尝试将暂存文件重命名到目标路径
//! - 两者都不存在 → 无效事务目录，直接清理
//!
//! ## 不变量
//!
//! - `committed` 标记存在意味着所有 `rename` 已完成；恢复时只需清理
//! - 无 `committed` 标记时，`manifest.json` 记录了完整的暂存→目标映射，可部分恢复
//! - `Drop` 实现只在 `committed == true` 时清理，未提交的事务目录留给恢复流程处理

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
    /// #644 评论 5473105049 第2节：支持 delete 操作。
    /// `true` 表示 commit 时应删除 `target_relative`（staging_filename 忽略）。
    /// 旧 manifest 中无此字段时反序列化为 `false`（向后兼容）。
    #[serde(default)]
    pub is_delete: bool,
}

/// 多文件保存事务。
///
/// 生命周期：`new` → `add_file`/`add_bytes`×N → `commit`。
/// `Drop` 只在 `committed == true` 时清理事务目录；未提交的事务留给 `recover_pending_transactions`。
///
/// #644 评论 5462823517 第4节：内部字段/参数名 `target_root`（原 `project_root`）—
/// full sync Commit 用这一套 staging + manifest + rename 提交，target_root 可以是
/// 任意根（project root / app-data root / staging root），不限于 project。
pub struct SaveTransaction {
    target_root: PathBuf,
    transaction_id: String,
    tx_dir: PathBuf,
    entries: Vec<TransactionEntry>,
    committed: bool,
}

impl SaveTransaction {
    pub fn new(target_root: &Path) -> Self {
        let transaction_id = Uuid::new_v4().to_string();
        let tx_dir = target_root.join(TRANSACTIONS_DIR).join(&transaction_id);
        Self {
            target_root: target_root.to_path_buf(),
            transaction_id,
            tx_dir,
            entries: Vec::new(),
            committed: false,
        }
    }

    pub fn transaction_id(&self) -> &str {
        &self.transaction_id
    }

    /// #644 评论 5462823517 第4节：原子写入字节内容到事务暂存区。
    /// `add_file(&str)` 转成 bytes 后委托本方法。full sync Commit 用本方法提交
    /// staging 里已就绪的字节内容，不新建第二套 SyncTransaction。
    pub fn add_bytes(&mut self, target_relative: &str, content: &[u8]) -> Result<()> {
        fs::create_dir_all(&self.tx_dir)?;
        let idx = self.entries.len();
        let staging_filename = format!("file_{}", idx);
        let staging_path = self.tx_dir.join(&staging_filename);
        crate::storage::atomic_write_bytes(&staging_path, content)?;
        self.entries.push(TransactionEntry {
            staging_filename,
            target_relative: target_relative.to_string(),
            is_delete: false,
        });
        Ok(())
    }

    pub fn add_file(&mut self, target_relative: &str, content: &str) -> Result<()> {
        self.add_bytes(target_relative, content.as_bytes())
    }

    /// #644 评论 5473105049 第2节：记录一条删除操作。
    /// commit 时会删除 `target_relative` 对应的文件；崩溃恢复时也会重放删除。
    pub fn add_delete(&mut self, target_relative: &str) {
        self.entries.push(TransactionEntry {
            staging_filename: String::new(),
            target_relative: target_relative.to_string(),
            is_delete: true,
        });
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn commit(&mut self) -> Result<()> {
        if self.entries.is_empty() {
            self.cleanup();
            return Ok(());
        }

        // 写入 manifest：记录暂存文件名 → 目标相对路径的映射，供崩溃恢复使用
        let manifest = TransactionManifest {
            transaction_id: self.transaction_id.clone(),
            created_at_ms: chrono::Utc::now().timestamp_millis(),
            entries: self.entries.clone(),
        };
        let manifest_json = serde_json::to_string_pretty(&manifest)?;
        let manifest_path = self.tx_dir.join(MANIFEST_FILENAME);
        crate::storage::atomic_write_string(&manifest_path, &manifest_json)?;

        // 逐个 rename 暂存文件到最终目标路径（write）或删除目标文件（delete）。
        // rename 在同一文件系统上是原子的，但跨 N 个文件不保证原子性；
        // committed 标记在全部 rename 完成后才写入，恢复时据此判断。
        let mut created_dirs = std::collections::HashSet::new();
        for entry in &self.entries {
            let target_path = self.target_root.join(&entry.target_relative);
            if entry.is_delete {
                // 删除操作：直接删除目标文件（不存在时忽略）
                let _ = fs::remove_file(&target_path);
            } else {
                let staging_path = self.tx_dir.join(&entry.staging_filename);
                if let Some(parent) = target_path.parent() {
                    if !created_dirs.contains(parent) {
                        fs::create_dir_all(parent)?;
                        created_dirs.insert(parent.to_path_buf());
                    }
                }
                fs::rename(&staging_path, &target_path)?;
            }
        }

        // committed 标记是事务完成的唯一判据：存在即表示所有 rename 已成功
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
        // 只在已提交时清理。未提交的事务目录留给 recover_pending_transactions 处理，
        // 避免在 Drop 中意外删除可能需要恢复的暂存文件。
        if self.committed {
            self.cleanup();
        }
    }
}

/// 扫描事务目录，恢复未完成的事务。
///
/// 判定逻辑：
/// - `committed` 标记存在 → 事务已成功完成，清理目录
/// - `manifest.json` 存在但无 `committed` → 中断的事务，尝试将暂存文件 rename 到目标
/// - 两者都不存在 → 无效目录，清理
// TODO(#597): 既有代码可读性技术债，待后续重构拆分
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless,
    deprecated
)]
pub fn recover_pending_transactions(target_root: &Path) -> Vec<TransactionRecovery> {
    let tx_base = target_root.join(TRANSACTIONS_DIR);
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
        let mut created_dirs = std::collections::HashSet::new();

        for tx_entry in &manifest.entries {
            if tx_entry.is_delete {
                // 恢复删除操作：直接删除目标文件
                let target_path = target_root.join(&tx_entry.target_relative);
                let _ = fs::remove_file(&target_path);
                recovered_files.push(tx_entry.target_relative.clone());
            } else {
                let staging_path = tx_dir.join(&tx_entry.staging_filename);
                if staging_path.exists() {
                    let target_path = target_root.join(&tx_entry.target_relative);
                    if let Some(parent) = target_path.parent() {
                        if !created_dirs.contains(parent) && fs::create_dir_all(parent).is_ok() {
                            created_dirs.insert(parent.to_path_buf());
                        }
                    }
                    match fs::rename(&staging_path, &target_path) {
                        Ok(()) => recovered_files.push(tx_entry.target_relative.clone()),
                        Err(e) => {
                            log::warn!(
                                "[transaction] recovery rename failed: {} -> {}: {}",
                                tx_entry.staging_filename,
                                tx_entry.target_relative,
                                e
                            );
                            missing_files.push(tx_entry.target_relative.clone());
                        }
                    }
                } else {
                    missing_files.push(tx_entry.target_relative.clone());
                }
            }
        }

        if recovered_files.is_empty() && !missing_files.is_empty() {
            log::warn!(
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
                is_delete: false,
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

    #[test]
    fn test_transaction_commit_with_delete() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path();

        // 先创建一个文件
        fs::create_dir_all(ws.join("sub")).unwrap();
        fs::write(ws.join("sub/to_delete.txt"), "will be deleted").unwrap();
        fs::write(ws.join("sub/to_keep.txt"), "will be kept").unwrap();

        let mut tx = SaveTransaction::new(ws);
        tx.add_file("sub/new_file.txt", "new content").unwrap();
        tx.add_delete("sub/to_delete.txt");
        tx.commit().unwrap();

        // new_file.txt 应该存在
        assert_eq!(
            fs::read_to_string(ws.join("sub/new_file.txt")).unwrap(),
            "new content"
        );
        // to_delete.txt 应该被删除
        assert!(!ws.join("sub/to_delete.txt").exists());
        // to_keep.txt 应该保留
        assert_eq!(
            fs::read_to_string(ws.join("sub/to_keep.txt")).unwrap(),
            "will be kept"
        );
    }
}
