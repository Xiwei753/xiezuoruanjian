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
    /// #644 评论 5475805198 第1节：事务生命周期阶段。
    /// 旧 manifest 中无此字段时反序列化为 `FilesCommitted`（向后兼容）。
    #[serde(default = "default_phase")]
    pub phase: TransactionPhase,
    /// #644 评论 5475805198 第1节：backup 模式的备份条目，写入 manifest 供崩溃恢复。
    /// 旧 manifest 中无此字段时反序列化为空 Vec（向后兼容）。
    #[serde(default)]
    pub backup_entries: Vec<BackupEntry>,
    /// #644 评论 5476546134 第2节：Git finalize 崩溃恢复记录。
    /// 仅 backup_mode 且需要 Git finalize 时有值。
    /// 旧 manifest 中无此字段时反序列化为 None（向后兼容）。
    #[serde(default)]
    pub git_finalize: Option<crate::sync::git_commit::GitFinalizeRecoveryRecord>,
}

fn default_phase() -> TransactionPhase {
    TransactionPhase::FilesCommitted
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

/// #644 评论 5475805198 第1节：事务生命周期阶段。
///
/// 写入 manifest，供崩溃恢复判断事务进度：
/// - `Prepared`：manifest 已写入，文件尚未 rename。
/// - `FilesCommitted`：所有 rename 完成，无需 Git finalize（非 backup_mode）。
/// - `FilesCommittedPendingGit`：rename 完成，等待 Git metadata finalize。
/// - `Finished`：Git finalize 成功，可以清理。
/// - `RolledBack`：rollback 已执行，可以清理。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionPhase {
    Prepared,
    FilesCommitted,
    FilesCommittedPendingGit,
    Finished,
    RolledBack,
}

/// #644 评论 5475413230 第1节：备份条目，替代空字符串哨兵。
///
/// `String::new()` 表示"旧文件不存在"时，`backup_dir.join("")` 就是 backup 目录本身，
/// `exists()` 为 true，随后 `fs::copy(目录, 文件)` 会失败。
/// 用明确枚举消除歧义。
///
/// #644 评论 5475805198 第1节：可序列化，写入 manifest 供崩溃恢复使用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BackupEntry {
    /// 旧文件存在，rollback 时从备份恢复。
    RestoreFile {
        target_relative: String,
        backup_filename: String,
    },
    /// 旧文件不存在（commit 新建了它），rollback 时删除。
    RemoveCreated { target_relative: String },
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
    /// #644 评论 5475110422 第3节：备份模式标志。
    /// 为 `true` 时，commit 前先把被覆盖/删除的旧文件备份到 `tx_dir/backup/`，
    /// 使 `rollback()` 能恢复到 commit 前的状态。
    backup_mode: bool,
    /// #644 评论 5475110422 第3节 + #644 评论 5475413230 第1节：
    /// commit 时备份的旧文件列表。用 `BackupEntry` 枚举替代空字符串哨兵。
    backed_up_files: Vec<BackupEntry>,
    /// #644 评论 5475413230 第1节：backup_mode 时 commit 不再 cleanup，
    /// 必须等 Git finalize 完成后由调用方显式调用 `finish()` 清理。
    finished: bool,
    /// #644 评论 5476546134 第2节：Git finalize 崩溃恢复记录。
    /// 写入 manifest 供重启后独立恢复。
    git_finalize_recovery: Option<crate::sync::git_commit::GitFinalizeRecoveryRecord>,
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
            backup_mode: false,
            backed_up_files: Vec::new(),
            finished: false,
            git_finalize_recovery: None,
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

    /// #644 评论 5475110422 第3节：启用备份模式。
    ///
    /// commit 前先把被覆盖/删除的旧文件备份到事务目录下的 `backup/` 子目录，
    /// 使 `rollback()` 能恢复到 commit 前的状态。
    /// 用于 Git finalize 原子性：先提交 SaveTransaction，再更新 Git ref/index；
    /// 若 Git finalize 失败，rollback 恢复文件到旧版本。
    pub fn enable_backup_mode(&mut self) {
        self.backup_mode = true;
    }

    /// #644 评论 5476546134 第2节：设置 Git finalize 崩溃恢复记录。
    ///
    /// 写入 manifest，使重启后能独立从 live + transaction 目录完成恢复。
    pub fn set_git_finalize_recovery(
        &mut self,
        record: crate::sync::git_commit::GitFinalizeRecoveryRecord,
    ) {
        self.git_finalize_recovery = Some(record);
    }

    /// #644 评论 5475110422 第3节 + #644 评论 5475413230 第1节 +
    /// #644 评论 5475805198 第1节：
    /// 回滚 commit 写入的文件变更，更新 manifest phase 为 RolledBack 并清理事务目录。
    ///
    /// 仅在 `backup_mode` 且已 `commit` 后调用有效。
    /// 按 `BackupEntry` 明确恢复旧文件或删除本次新建文件。
    /// - `RestoreFile`：从备份恢复旧文件。
    /// - `RemoveCreated`：删除 commit 新建的文件；只有 `NotFound` 视为成功，
    ///   其它 IO 错误返回 `Err`。
    ///
    /// 恢复完成后更新 phase 并清理事务目录。
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting
    )]
    pub fn rollback(&mut self) -> Result<()> {
        if !self.committed || !self.backup_mode {
            return Ok(());
        }
        let backup_dir = self.tx_dir.join("backup");
        for entry in &self.backed_up_files {
            match entry {
                BackupEntry::RestoreFile {
                    target_relative,
                    backup_filename,
                } => {
                    let target_path = self.target_root.join(target_relative);
                    let backup_path = backup_dir.join(backup_filename);
                    if let Some(parent) = target_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::copy(&backup_path, &target_path)?;
                }
                BackupEntry::RemoveCreated { target_relative } => {
                    let target_path = self.target_root.join(target_relative);
                    if let Err(e) = fs::remove_file(&target_path) {
                        if e.kind() != std::io::ErrorKind::NotFound {
                            return Err(crate::Error::Io(e));
                        }
                    }
                }
            }
        }
        // 清理备份目录。
        let _ = fs::remove_dir_all(&backup_dir);
        // #644 评论 5475805198 第1节：更新 manifest phase 为 RolledBack 并清理。
        let manifest_path = self.tx_dir.join(MANIFEST_FILENAME);
        let _ = self.write_manifest_phase(&manifest_path, TransactionPhase::RolledBack);
        self.finished = true;
        self.cleanup();
        Ok(())
    }

    /// #644 评论 5475413230 第1节 + #644 评论 5475805198 第1节：
    /// Git finalize 成功后调用，更新 manifest phase 为 Finished 并清理事务目录。
    ///
    /// `backup_mode` 时 `commit()` 不再自动 cleanup；调用方必须在 Git finalize
    /// 完成后显式调用 `finish()`。非 backup_mode 时 `commit()` 已 cleanup，
    /// `finish()` 是空操作。
    pub fn finish(&mut self) {
        // #644 评论 5475805198 第1节：更新 manifest phase 为 Finished。
        let manifest_path = self.tx_dir.join(MANIFEST_FILENAME);
        let _ = self.write_manifest_phase(&manifest_path, TransactionPhase::Finished);
        self.finished = true;
        self.cleanup();
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

        // #644 评论 5476546134 第3节：backup_mode 时先完成全部备份，不改 live。
        // 备份信息 + git_finalize recovery record 一次原子写入 manifest。
        // manifest 持久化成功后，才开始 rename/delete live 文件。
        if self.backup_mode {
            let backup_dir = self.tx_dir.join("backup");
            fs::create_dir_all(&backup_dir)?;

            for (idx, entry) in self.entries.iter().enumerate() {
                let target_path = self.target_root.join(&entry.target_relative);
                // 备份被覆盖或被删除的旧文件。
                if target_path.exists() {
                    let backup_name = format!("backup_{}", idx);
                    fs::copy(&target_path, backup_dir.join(&backup_name))?;
                    self.backed_up_files.push(BackupEntry::RestoreFile {
                        target_relative: entry.target_relative.clone(),
                        backup_filename: backup_name,
                    });
                } else {
                    self.backed_up_files.push(BackupEntry::RemoveCreated {
                        target_relative: entry.target_relative.clone(),
                    });
                }
            }

            // 原子写入 Prepared + backup_entries + git_finalize recovery record。
            let manifest_path = self.tx_dir.join(MANIFEST_FILENAME);
            self.write_manifest_phase(&manifest_path, TransactionPhase::Prepared)?;

            // 现在才开始 rename/delete live 文件。
            let mut created_dirs = std::collections::HashSet::new();
            for entry in &self.entries {
                let target_path = self.target_root.join(&entry.target_relative);
                if entry.is_delete {
                    match fs::remove_file(&target_path) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(crate::Error::Io(e)),
                    }
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

            // rename 全部完成后更新 phase。
            self.write_manifest_phase(&manifest_path, TransactionPhase::FilesCommittedPendingGit)?;
            self.committed = true;
            // backup_mode 时不在 commit 内 cleanup。
            return Ok(());
        }

        // 非 backup_mode：原有逻辑。
        let manifest_path = self.tx_dir.join(MANIFEST_FILENAME);
        self.write_manifest_phase(&manifest_path, TransactionPhase::Prepared)?;

        let mut created_dirs = std::collections::HashSet::new();
        for entry in &self.entries {
            let target_path = self.target_root.join(&entry.target_relative);
            if entry.is_delete {
                match fs::remove_file(&target_path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(crate::Error::Io(e)),
                }
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

        self.write_manifest_phase(&manifest_path, TransactionPhase::FilesCommitted)?;
        self.committed = true;
        self.cleanup();
        Ok(())
    }

    /// #644 评论 5475805198 第1节：更新 manifest 中的 phase 字段。
    ///
    /// 读取现有 manifest（若存在），更新 phase 和 backup_entries，原子写回。
    /// manifest 不存在时（理论上不应发生）创建最小 manifest。
    ///
    /// #644 评论 5476546134 第2节：同时写入 git_finalize recovery record。
    fn write_manifest_phase(&self, manifest_path: &Path, phase: TransactionPhase) -> Result<()> {
        let mut manifest = if manifest_path.exists() {
            let content = fs::read_to_string(manifest_path)?;
            serde_json::from_str::<TransactionManifest>(&content).unwrap_or_else(|_| {
                TransactionManifest {
                    transaction_id: self.transaction_id.clone(),
                    created_at_ms: chrono::Utc::now().timestamp_millis(),
                    entries: self.entries.clone(),
                    phase: TransactionPhase::Prepared,
                    backup_entries: Vec::new(),
                    git_finalize: None,
                }
            })
        } else {
            TransactionManifest {
                transaction_id: self.transaction_id.clone(),
                created_at_ms: chrono::Utc::now().timestamp_millis(),
                entries: self.entries.clone(),
                phase: TransactionPhase::Prepared,
                backup_entries: Vec::new(),
                git_finalize: None,
            }
        };
        manifest.phase = phase;
        manifest.backup_entries = self.backed_up_files.clone();
        if self.git_finalize_recovery.is_some() {
            manifest.git_finalize = self.git_finalize_recovery.clone();
        }
        let json = serde_json::to_string_pretty(&manifest)?;
        crate::storage::atomic_write_string(manifest_path, &json)?;
        Ok(())
    }

    fn cleanup(&self) {
        let _ = fs::remove_dir_all(&self.tx_dir);
    }
}

impl Drop for SaveTransaction {
    fn drop(&mut self) {
        // #644 评论 5475413230 第1节：backup_mode 时 commit 不再自动 cleanup。
        // 只在以下情况清理：
        // - 非 backup_mode 且已 committed（原有行为）
        // - 已 finish()（finished == true）
        // backup_mode + committed 但未 finish 时，保留事务目录供 rollback 使用。
        if self.finished || (self.committed && !self.backup_mode) {
            self.cleanup();
        }
    }
}

/// #644 评论 5476546134 第2节：静态版本的 manifest phase 更新，供恢复流程使用。
///
/// 恢复时没有 `SaveTransaction` 实例，需要直接操作 manifest 文件。
fn write_manifest_phase_static(
    manifest_path: &Path,
    phase: TransactionPhase,
    existing: &TransactionManifest,
) -> Result<()> {
    let mut manifest = existing.clone();
    manifest.phase = phase;
    let json = serde_json::to_string_pretty(&manifest)?;
    crate::storage::atomic_write_string(manifest_path, &json)?;
    Ok(())
}

/// 扫描事务目录，恢复未完成的事务。
///
/// #644 评论 5475805198 第1节：基于 manifest phase 的状态机恢复。
///
/// 判定逻辑：
/// - manifest 存在且 phase 为 `FilesCommitted`/`Finished`/`RolledBack` → 清理目录
/// - manifest 存在且 phase 为 `FilesCommittedPendingGit` → 返回 `PendingGitRecovery`
/// - manifest 存在且 phase 为 `Prepared` → 尝试将暂存文件 rename 到目标
/// - 旧格式：`committed` 标记存在 → 清理目录（向后兼容）
/// - 两者都不存在 → 无效目录，清理
///
/// 返回 `(常规恢复列表, 待 Git finalize 的恢复列表)`。
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
pub fn recover_pending_transactions(
    target_root: &Path,
) -> (Vec<TransactionRecovery>, Vec<PendingGitTransactionRecovery>) {
    let tx_base = target_root.join(TRANSACTIONS_DIR);
    if !tx_base.exists() {
        return (Vec::new(), Vec::new());
    }

    let mut results = Vec::new();
    let mut pending_git = Vec::new();
    let entries = match fs::read_dir(&tx_base) {
        Ok(e) => e,
        Err(_) => return (Vec::new(), Vec::new()),
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

        let manifest_path = tx_dir.join(MANIFEST_FILENAME);

        // #644 评论 5475805198 第1节：优先读 manifest 获取 phase。
        if manifest_path.exists() {
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

            match manifest.phase {
                TransactionPhase::FilesCommitted
                | TransactionPhase::Finished
                | TransactionPhase::RolledBack => {
                    // 事务已完成，清理目录。
                    let _ = fs::remove_dir_all(&tx_dir);
                    continue;
                }
                TransactionPhase::FilesCommittedPendingGit => {
                    // #644 评论 5475805198 第1节：文件已 commit，Git finalize 未完成。
                    // #644 评论 5476546134 第2节：有 git_finalize recovery record 时，
                    // 直接回滚到同步前状态（Git metadata + live 文件），不尝试向前完成。
                    // 下一次正常 full sync 重新跑。
                    if let Some(ref git_rec) = manifest.git_finalize {
                        log::warn!(
                            "[transaction] found FilesCommittedPendingGit tx={}, \
                             rolling back with git_finalize recovery record",
                            manifest.transaction_id
                        );
                        // 1. 回滚 Git metadata。
                        if let Ok(seed_state) = git_rec.seed_state.to_seed_state() {
                            if !matches!(
                                seed_state,
                                crate::sync::git_staging::GitSeedState::NotGitRepo
                            ) {
                                if let Err(e) = crate::sync::git_commit::rollback_git_finalize(
                                    target_root,
                                    &git_rec.metadata_snapshot,
                                ) {
                                    log::warn!(
                                        "[transaction] git metadata rollback failed for tx={}: {}",
                                        manifest.transaction_id,
                                        e
                                    );
                                }
                            }
                        }
                        // 2. 回滚 live 文件（用 backup_entries）。
                        let backup_dir = tx_dir.join("backup");
                        for entry in &manifest.backup_entries {
                            match entry {
                                BackupEntry::RestoreFile {
                                    target_relative,
                                    backup_filename,
                                } => {
                                    let target_path = target_root.join(target_relative);
                                    let backup_path = backup_dir.join(backup_filename);
                                    if backup_path.exists() {
                                        if let Some(parent) = target_path.parent() {
                                            let _ = fs::create_dir_all(parent);
                                        }
                                        if let Err(e) = fs::copy(&backup_path, &target_path) {
                                            log::warn!(
                                                "[transaction] backup restore failed for {}: {}",
                                                target_relative,
                                                e
                                            );
                                        }
                                    }
                                }
                                BackupEntry::RemoveCreated { target_relative } => {
                                    let target_path = target_root.join(target_relative);
                                    if target_path.exists() {
                                        if let Err(e) = fs::remove_file(&target_path) {
                                            if e.kind() != std::io::ErrorKind::NotFound {
                                                log::warn!(
                                                    "[transaction] remove created file failed for {}: {}",
                                                    target_relative, e
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // 3. 标记 RolledBack 并清理。
                        let _ = fs::remove_dir_all(&backup_dir);
                        let manifest_path = tx_dir.join(MANIFEST_FILENAME);
                        let _ = write_manifest_phase_static(
                            &manifest_path,
                            TransactionPhase::RolledBack,
                            &manifest,
                        );
                        let _ = fs::remove_dir_all(&tx_dir);
                        continue;
                    }

                    // 旧格式：无 git_finalize recovery record，返回给调用方处理。
                    log::warn!(
                        "[transaction] found FilesCommittedPendingGit tx={}, \
                         no git_finalize record, needs manual Git recovery",
                        manifest.transaction_id
                    );
                    pending_git.push(PendingGitTransactionRecovery {
                        transaction_id: manifest.transaction_id.clone(),
                        manifest,
                        target_root: target_root.to_path_buf(),
                        tx_dir: tx_dir.clone(),
                    });
                    continue;
                }
                TransactionPhase::Prepared => {
                    // 事务中断在 Prepared 阶段，尝试恢复 rename。
                }
            }
        } else {
            // 无 manifest：检查旧格式 committed marker（向后兼容）。
            let commit_marker = tx_dir.join(COMMIT_MARKER);
            if commit_marker.exists() {
                let _ = fs::remove_dir_all(&tx_dir);
                continue;
            }
            // 无 manifest 也无 committed marker → 无效目录。
            let _ = fs::remove_dir_all(&tx_dir);
            continue;
        }

        // Prepared 阶段恢复：重放 rename。
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
                // #644 评论 5473401065 第3节：NotFound 记 recovered（幂等）；
                // 其它错误记 missing_files + 日志，不能假装 recovered。
                let target_path = target_root.join(&tx_entry.target_relative);
                match fs::remove_file(&target_path) {
                    Ok(()) => {
                        recovered_files.push(tx_entry.target_relative.clone());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        recovered_files.push(tx_entry.target_relative.clone());
                    }
                    Err(e) => {
                        log::warn!(
                            "[transaction] recovery delete failed: {}: {}",
                            tx_entry.target_relative,
                            e
                        );
                        missing_files.push(tx_entry.target_relative.clone());
                    }
                }
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

    (results, pending_git)
}

#[derive(Debug)]
pub struct TransactionRecovery {
    pub transaction_id: String,
    pub recovered_files: Vec<String>,
    pub missing_files: Vec<String>,
}

/// #644 评论 5475805198 第1节：FilesCommittedPendingGit 状态的事务恢复信息。
///
/// 文件已 commit 到 live，但 Git metadata finalize 尚未完成。
/// 调用方需要决定：完成 Git finalize 或回滚文件。
pub struct PendingGitTransactionRecovery {
    pub transaction_id: String,
    pub manifest: TransactionManifest,
    pub target_root: PathBuf,
    pub tx_dir: PathBuf,
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
            phase: TransactionPhase::Prepared,
            backup_entries: Vec::new(),
            git_finalize: None,
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

        let (recovered, pending) = recover_pending_transactions(ws);
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].recovered_files.len(), 2);
        assert!(recovered[0].missing_files.is_empty());
        assert!(pending.is_empty());
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
            phase: TransactionPhase::Prepared,
            backup_entries: Vec::new(),
            git_finalize: None,
        };
        let manifest_json = serde_json::to_string_pretty(&manifest).unwrap();
        fs::write(tx_dir.join(MANIFEST_FILENAME), &manifest_json).unwrap();

        let (recovered, pending) = recover_pending_transactions(ws);
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].recovered_files.len(), 0);
        assert_eq!(recovered[0].missing_files.len(), 1);
        assert!(pending.is_empty());
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
