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
    /// 必须在 `commit()` 之前调用，使 recovery record 与 manifest 一起原子写入。
    /// 写入 manifest 供重启后独立恢复。
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
        // #644 评论 5475805198 第1节 + #644 评论 5483239422 问题1 +
        // #644 评论 5483920624 问题1：不在 phase 落盘前单独删 backup_dir。
        // 正确顺序：1.Git rollback → 2.live 文件恢复 → 3.原子写 RolledBack phase
        // → 4.phase 成功后 cleanup 整个 tx_dir（含 backup）。
        // 若 phase 写盘失败，backup 仍在，下次启动重试 rollback 可用。
        let manifest_path = self.tx_dir.join(MANIFEST_FILENAME);
        self.write_manifest_phase(&manifest_path, TransactionPhase::RolledBack)?;
        self.finished = true;
        self.cleanup();
        Ok(())
    }

    /// #644 评论 5475413230 第1节 + #644 评论 5475805198 第1节 +
    /// #644 评论 5483239422 问题1：
    /// Git finalize 成功后调用，更新 manifest phase 为 Finished 并清理事务目录。
    ///
    /// `backup_mode` 时 `commit()` 不再自动 cleanup；调用方必须在 Git finalize
    /// 完成后显式调用 `finish()`。非 backup_mode 时 `commit()` 已 cleanup，
    /// `finish()` 是空操作。
    ///
    /// #644 评论 5483239422 问题1：返回 `Result<()>`，`write_manifest_phase(Finished)`
    /// 失败时返回 Err，**不**设 finished、**不**调 cleanup，保留 tx_dir 给下次恢复。
    /// 调用方（sync_ops）只有 finish 成功后才允许清 owner marker。
    pub fn finish(&mut self) -> Result<()> {
        // #644 评论 5475805198 第1节 + #644 评论 5483239422 问题1：
        // 更新 manifest phase 为 Finished。用 `?` 传播错误，不吞错。
        let manifest_path = self.tx_dir.join(MANIFEST_FILENAME);
        self.write_manifest_phase(&manifest_path, TransactionPhase::Finished)?;
        // Finished 已持久化成功，才设 finished 并 cleanup。
        self.finished = true;
        self.cleanup();
        Ok(())
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn commit(&mut self) -> Result<()> {
        // #644 评论 5477439446 问题1：纯 Git metadata 变化时（entries 为空但
        // git_finalize_recovery 存在），仍需创建 metadata-only transaction barrier。
        // 让 manifest 落盘（Prepared -> FilesCommittedPendingGit），保留事务到
        // finish()，不在这里 cleanup。这样 finalize 中途崩溃后启动恢复有持久化
        // snapshot 可用。
        if self.entries.is_empty() && self.git_finalize_recovery.is_none() {
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
    /// manifest 不存在时（首次写入）创建最小 manifest。
    ///
    /// #644 评论 5483239422 问题1：manifest 存在但反序列化失败时返回 Err，
    /// **不**自动重建最小 manifest——重建会丢掉 backup_entries/git_finalize/plan
    /// 崩溃恢复材料。调用方收到 Err 应保留 tx_dir 给下次恢复或显式修复。
    ///
    /// #644 评论 5476546134 第2节：同时写入 git_finalize recovery record。
    fn write_manifest_phase(&self, manifest_path: &Path, phase: TransactionPhase) -> Result<()> {
        let mut manifest = if manifest_path.exists() {
            let content = fs::read_to_string(manifest_path)?;
            serde_json::from_str::<TransactionManifest>(&content).map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "write_manifest_phase: manifest at {} is corrupted (deserialize failed: {}); \
                     refusing to rebuild minimal manifest to preserve backup_entries/git_finalize/plan",
                    manifest_path.display(),
                    e
                )))
            })?
        } else {
            // manifest 不存在：首次写入，创建最小 manifest。
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

    /// #644 评论 5483239422 问题1：cleanup 不完全吞错。
    ///
    /// 调用方（finish/rollback/commit）已确保 phase 持久化成功，cleanup 只是尽力
    /// 删除 tx_dir。删除失败时 log::warn，不影响业务正确性——下次启动
    /// `recover_pending_transactions` 看到 Finished/RolledBack phase 会再删一次。
    fn cleanup(&self) {
        if let Err(e) = fs::remove_dir_all(&self.tx_dir) {
            log::warn!(
                "[transaction] cleanup: failed to remove tx_dir {}: {} \
                 (phase already persisted, next recover will retry)",
                self.tx_dir.display(),
                e
            );
        }
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

/// #644 评论 5476546134 第3节：统一回滚 full-sync 事务入口。
///
/// 满足：
/// - Git rollback 成功 AND 文件 rollback 成功 → phase=RolledBack → cleanup
/// - 任意一步失败 → 返回 Err → 保留 manifest + backup + transaction 目录
///
/// 不吞错，给下次恢复留机会。
fn rollback_full_sync_transaction(
    tx_dir: &Path,
    target_root: &Path,
    manifest: &TransactionManifest,
) -> Result<()> {
    // 1. 回滚 Git metadata（如果有 recovery record）。
    if let Some(ref git_rec) = manifest.git_finalize {
        // #644 评论 5476546134 第3节：删除 NotGitRepo 特判，
        // 统一调用 rollback_git_finalize，让 repo_existed 决定恢复还是删除。
        // #644 评论 5480360027：使用 write-ahead plan 做 rollback，
        // 不再依赖 mutation_log（旧 manifest 反序列化时 plan 为 default，rollback 是 no-op）。
        // #644 评论 5482310913 问题3：rollback_git_finalize 返回 GitRollbackOutcome，
        // 区分 Reverted / ConcurrentChanged / RepoInstallCommitted。
        let _seed_state = git_rec.seed_state.to_seed_state().map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "rollback_full_sync_transaction: invalid seed state: {}",
                e
            )))
        })?;
        let outcome = crate::sync::git_commit::rollback_git_finalize(
            target_root,
            &git_rec.metadata_snapshot,
            &git_rec.plan,
        )?;
        match outcome {
            crate::sync::git_commit::GitRollbackOutcome::Reverted => {
                // Git metadata 已回滚，继续恢复文件 backup（下方逻辑）。
            }
            crate::sync::git_commit::GitRollbackOutcome::ConcurrentChanged => {
                // #644 评论 5482310913 问题3：检测到并发变更，保留 transaction，
                // 不恢复文件 backup，不删除 transaction 目录，返回 Err 让上层
                // `recover_pending_transactions` 保留事务给下次恢复。
                return Err(crate::Error::Io(std::io::Error::other(
                    "rollback_full_sync_transaction: git rollback detected concurrent \
                     change (ownership mismatch or external repo), preserving transaction \
                     for next recovery",
                )));
            }
            crate::sync::git_commit::GitRollbackOutcome::RepoInstallCommitted => {
                // #644 评论 5482310913 问题2：NotGitRepo 已完成 owner-matched .git rename。
                // 按 commit-point 逻辑收尾：不回滚文件（文件已是新版），标记 Finished
                // 并清理 transaction 目录。
                let manifest_path = tx_dir.join(MANIFEST_FILENAME);
                write_manifest_phase_static(&manifest_path, TransactionPhase::Finished, manifest)?;
                fs::remove_dir_all(tx_dir)?;
                return Ok(());
            }
        }
    }

    // 2. 回滚 live 文件（用 backup_entries）。
    // #644 评论 5477439446 问题3：manifest 要求 RestoreFile 但 backup 文件不存在时，
    // 视为恢复失败并返回 Err。只要任意一个 BackupEntry 没有完成恢复，就保留整份
    // transaction，不能写 RolledBack，不能删除 backup/manifest/tx_dir。
    let backup_dir = tx_dir.join("backup");
    for entry in &manifest.backup_entries {
        match entry {
            BackupEntry::RestoreFile {
                target_relative,
                backup_filename,
            } => {
                let target_path = target_root.join(target_relative);
                let backup_path = backup_dir.join(backup_filename);
                if !backup_path.exists() {
                    // manifest 要求 RestoreFile 但 backup 文件缺失 → 恢复失败。
                    // 返回 Err，保留整份 transaction 给下次恢复机会。
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "rollback_full_sync_transaction: backup file missing for RestoreFile {}: {}",
                        target_relative,
                        backup_path.display()
                    ))));
                }
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&backup_path, &target_path)?;
            }
            BackupEntry::RemoveCreated { target_relative } => {
                let target_path = target_root.join(target_relative);
                if target_path.exists() {
                    fs::remove_file(&target_path)?;
                }
            }
        }
    }

    // #644 评论 5483920624 问题1：不在 phase 落盘前单独删 backup_dir。
    // 正确顺序：1.Git rollback → 2.live 文件恢复 → 3.原子写 RolledBack phase
    // → 4.phase 成功后 cleanup 整个 tx_dir（含 backup）。
    // 若 phase 写盘失败，backup 仍在，下次启动重试 rollback 可用。

    // 3. 标记 RolledBack。
    let manifest_path = tx_dir.join(MANIFEST_FILENAME);
    write_manifest_phase_static(&manifest_path, TransactionPhase::RolledBack, manifest)?;

    // 4. 清理事务目录（含 backup）。
    fs::remove_dir_all(tx_dir)?;

    Ok(())
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
                    Err(e) => {
                        // #644 评论 5483239422 问题4：manifest 解析失败时保留 tx_dir。
                        // transaction 目录含 backup_entries + GitMetadataSnapshot +
                        // GitFinalizePlan 崩溃恢复材料，删除即销毁恢复证据。
                        // 记录错误，等下次启动重试或显式修复入口。
                        log::warn!(
                            "[transaction] recover: manifest parse failed for tx_dir={}: {} \
                             — preserving tx_dir (contains backup_entries/git_finalize/plan \
                             recovery material), will retry next startup",
                            tx_dir.display(),
                            e
                        );
                        continue;
                    }
                },
                Err(e) => {
                    // #644 评论 5483239422 问题4：manifest 读取失败时保留 tx_dir。
                    log::warn!(
                        "[transaction] recover: manifest read failed for tx_dir={}: {} \
                         — preserving tx_dir (contains backup_entries/git_finalize/plan \
                         recovery material), will retry next startup",
                        tx_dir.display(),
                        e
                    );
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
                    if manifest.git_finalize.is_some() {
                        log::warn!(
                            "[transaction] found FilesCommittedPendingGit tx={}, rolling back",
                            manifest.transaction_id
                        );
                        // #644 评论 5476546134 第3/4节：统一回滚入口，不吞错。
                        if let Err(e) =
                            rollback_full_sync_transaction(&tx_dir, target_root, &manifest)
                        {
                            log::warn!(
                                "[transaction] rollback_full_sync_transaction failed for tx={}: {}",
                                manifest.transaction_id,
                                e
                            );
                            // #644 评论 5476546134 第4节：失败保留事务目录，给下次恢复机会。
                            continue;
                        }
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
                    // #644 评论 5476546134 第2节：区分两类事务的 Prepared 阶段语义。
                    // - 有 git_finalize recovery record：backup_mode + Git finalize 事务，
                    //   应该回滚（不尝试向前完成），下一次 full-sync 重跑。
                    // - 无 git_finalize：普通保存事务，继续重放 rename。
                    if manifest.git_finalize.is_some() {
                        log::warn!(
                            "[transaction] found Prepared tx={} with git_finalize, rolling back",
                            manifest.transaction_id
                        );
                        // #644 评论 5476546134 第3/4节：统一回滚入口，不吞错。
                        if let Err(e) =
                            rollback_full_sync_transaction(&tx_dir, target_root, &manifest)
                        {
                            log::warn!(
                                "[transaction] rollback_full_sync_transaction failed for tx={}: {}",
                                manifest.transaction_id,
                                e
                            );
                            // #644 评论 5476546134 第4节：失败保留事务目录，给下次恢复机会。
                            continue;
                        }
                        continue;
                    }
                    // 事务中断在 Prepared 阶段（无 git_finalize），尝试恢复 rename。
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
        // #644 评论 5483239422 问题4：重读 manifest 失败时保留 tx_dir，不删恢复证据。
        let manifest: TransactionManifest = match fs::read_to_string(&manifest_path) {
            Ok(s) => match serde_json::from_str(&s) {
                Ok(m) => m,
                Err(e) => {
                    log::warn!(
                        "[transaction] recover: manifest re-read parse failed for tx_dir={}: {} \
                         — preserving tx_dir, will retry next startup",
                        tx_dir.display(),
                        e
                    );
                    continue;
                }
            },
            Err(e) => {
                log::warn!(
                    "[transaction] recover: manifest re-read failed for tx_dir={}: {} \
                     — preserving tx_dir, will retry next startup",
                    tx_dir.display(),
                    e
                );
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

    /// #644 评论 5483239422 问题1：`SaveTransaction::finish()` 吞掉
    /// `write_manifest_phase(Finished)` 的错误，随后仍调用 `cleanup()` 删除 tx_dir。
    ///
    /// 复现策略：构造 backup_mode 事务 commit 成功（phase=FilesCommittedPendingGit），
    /// 然后使 manifest 读取失败（把 manifest 文件替换为同名目录，使
    /// `fs::read_to_string` 返回 Err）。此时调用 `finish()`：
    /// - 当前行为：`let _ = write_manifest_phase(...)` 吞错，`finished=true`，
    ///   `cleanup()` 执行 `remove_dir_all(tx_dir)`，tx_dir 被删，恢复证据丢失。
    ///   调用方（sync_ops）完全不知道 Finished 没写成功，仍会删 owner marker。
    /// - 预期行为：`finish()` 应返回 `Err`，不调用 `cleanup()`，tx_dir 保留，
    ///   manifest 仍停在 FilesCommittedPendingGit，下次恢复可重试。
    ///
    /// 此测试断言预期行为（tx_dir 应保留），当前代码下断言失败。
    #[test]
    fn finish_should_preserve_tx_dir_when_manifest_write_fails() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path();

        let mut tx = SaveTransaction::new(ws);
        tx.enable_backup_mode();
        tx.add_file("a.txt", "hello").unwrap();
        tx.commit().unwrap();
        // 此时 phase = FilesCommittedPendingGit，tx_dir 存在，manifest 存在。

        // 使 manifest 读取失败：删除 manifest 文件，创建同名目录。
        // fs::read_to_string(目录) 会返回 Err，write_manifest_phase 返回 Err。
        let manifest_path = tx.tx_dir.join(MANIFEST_FILENAME);
        fs::remove_file(&manifest_path).unwrap();
        fs::create_dir(&manifest_path).unwrap();

        // 当前：finish() 返回 ()，吞错，cleanup() 删 tx_dir。
        // 预期：finish() 应返回 Err，不 cleanup，tx_dir 保留。
        let finish_result = tx.finish();
        assert!(
            finish_result.is_err(),
            "finish() should return Err when write_manifest_phase(Finished) fails; \
             current code swallows the error via `let _ = ...` and returns ()"
        );

        // 预期：tx_dir 应保留（manifest 写失败，不应 cleanup）。
        assert!(
            tx.tx_dir.exists(),
            "finish() should NOT cleanup tx_dir when write_manifest_phase(Finished) fails; \
             current code swallows the error via `let _ = ...` and deletes tx_dir, \
             losing recovery evidence while sync_ops still removes owner marker"
        );
    }

    /// #644 评论 5483239422 问题4：`recover_pending_transactions()` 在 manifest
    /// 读/解析失败时直接 `remove_dir_all(tx_dir)`，销毁崩溃恢复材料。
    ///
    /// 复现策略：构造 tx_dir 含损坏 manifest（无效 JSON）+ backup 恢复材料
    /// （backup_entries + staging 文件），调用 `recover_pending_transactions`。
    /// - 当前行为：manifest 解析失败，`remove_dir_all(tx_dir)`，恢复证据被销毁。
    /// - 预期行为：记录错误并保留 tx_dir，不继续改 live，不删除 backup，
    ///   等下次启动重试或显式修复入口。
    ///
    /// 此测试断言预期行为（tx_dir 应保留），当前代码下断言失败。
    #[test]
    fn recover_should_preserve_tx_dir_when_manifest_corrupted() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path();

        let tx_dir = ws.join(TRANSACTIONS_DIR).join("corrupted-tx-644");
        fs::create_dir_all(&tx_dir).unwrap();

        // 写入损坏的 manifest（无效 JSON）。
        fs::write(tx_dir.join(MANIFEST_FILENAME), "{ this is not valid json").unwrap();

        // 放入 full-sync 崩溃恢复材料：backup + staging。
        let backup_dir = tx_dir.join("backup");
        fs::create_dir_all(&backup_dir).unwrap();
        fs::write(backup_dir.join("backup_0"), "old file content").unwrap();
        fs::write(tx_dir.join("file_0"), "staged content").unwrap();

        // 当前：manifest 解析失败 → remove_dir_all(tx_dir) 销毁恢复证据。
        // 预期：保留 tx_dir，等下次启动重试。
        let _ = recover_pending_transactions(ws);

        assert!(
            tx_dir.exists(),
            "recover must NOT delete tx_dir when manifest is corrupted; \
             tx_dir contains backup_entries + GitMetadataSnapshot + GitFinalizePlan \
             recovery material; current code remove_dir_all(tx_dir), destroying \
             last recovery evidence"
        );
    }
}
