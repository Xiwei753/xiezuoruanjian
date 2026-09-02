use std::fs;
use std::path::Path;
use uuid::Uuid;

use super::model::*;
use crate::error::Result;

/// 多文件保存事务。
///
/// 生命周期：`new` → `add_file`/`add_bytes`×N → `commit`。
/// `Drop` 只在 `committed == true` 时清理事务目录；未提交的事务留给 `recover_pending_transactions`。
///
/// #644 评论 5462823517 第4节：内部字段/参数名 `target_root`（原 `project_root`）—
/// full sync Commit 用这一套 staging + manifest + rename 提交，target_root 可以是
/// 任意根（project root / app-data root / staging root），不限于 project。
pub struct SaveTransaction {
    target_root: std::path::PathBuf,
    transaction_id: String,
    pub(crate) tx_dir: std::path::PathBuf,
    pub(crate) entries: Vec<TransactionEntry>,
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
    git_finalize_recovery: Option<crate::sync::git::GitFinalizeRecoveryRecord>,
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
        record: crate::sync::git::GitFinalizeRecoveryRecord,
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

        // #644 评论 5488655439 问题2 + #644 评论 5488871385 问题1：
        // 在第一笔 durable_copy_file/remove_file 前先检查完 self.backed_up_files，
        // 不能恢复到一半才发现后面的 backup 缺失。
        // 用 File::open() 确认真正可读，不用 metadata() 冒充。
        for entry in &self.backed_up_files {
            if let BackupEntry::RestoreFile {
                target_relative,
                backup_filename,
            } = entry
            {
                let backup_path = backup_dir.join(backup_filename);
                match std::fs::File::open(&backup_path) {
                    Ok(file) => {
                        drop(file);
                    }
                    Err(e) => {
                        return Err(crate::Error::Io(std::io::Error::other(format!(
                            "rollback: backup file not readable for RestoreFile {}: {}: {}",
                            target_relative,
                            backup_path.display(),
                            e
                        ))));
                    }
                }
            }
        }

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
                    // #644 评论 5484539222 缺陷2：durable copy + fsync 父目录。
                    crate::storage::durable_copy_file(&backup_path, &target_path)?;
                }
                BackupEntry::RemoveCreated { target_relative } => {
                    let target_path = self.target_root.join(target_relative);
                    if let Err(e) = fs::remove_file(&target_path) {
                        if e.kind() != std::io::ErrorKind::NotFound {
                            return Err(crate::Error::Io(e));
                        }
                    } else {
                        // #644 评论 5484539222 缺陷2：remove 后 fsync 父目录持久化目录项。
                        crate::storage::sync_parent(&target_path)?;
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
                    // #644 评论 5484539222 缺陷2：durable copy（copy + fsync backup 文件 + 父目录）。
                    crate::storage::durable_copy_file(
                        &target_path,
                        &backup_dir.join(&backup_name),
                    )?;
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

            // #644 评论 5484539222 缺陷2：所有 backup entry 完成后 fsync backup/ 目录，
            // 持久化 backup 目录项，再允许写 Prepared phase。
            crate::storage::sync_dir(&backup_dir)?;

            // 原子写入 Prepared + backup_entries + git_finalize recovery record。
            let manifest_path = self.tx_dir.join(MANIFEST_FILENAME);
            self.write_manifest_phase(&manifest_path, TransactionPhase::Prepared)?;

            // 现在才开始 rename/delete live 文件。
            let mut created_dirs = std::collections::HashSet::new();
            for entry in &self.entries {
                let target_path = self.target_root.join(&entry.target_relative);
                if entry.is_delete {
                    match fs::remove_file(&target_path) {
                        Ok(()) => {
                            // #644 评论 5484539222 缺陷2：remove 后 fsync 父目录。
                            crate::storage::sync_parent(&target_path)?;
                        }
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
                    // #644 评论 5484539222 缺陷2：rename 后 fsync 目标父目录持久化目录项。
                    // staging 文件已由 atomic_write_bytes fsync，rename 原子，但目录项需 fsync 父目录。
                    crate::storage::sync_parent(&target_path)?;
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
                    Ok(()) => {
                        // #644 评论 5484539222 缺陷2：remove 后 fsync 父目录。
                        crate::storage::sync_parent(&target_path)?;
                    }
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
                // #644 评论 5484539222 缺陷2：rename 后 fsync 目标父目录。
                crate::storage::sync_parent(&target_path)?;
            }
        }

        self.write_manifest_phase(&manifest_path, TransactionPhase::FilesCommitted)?;
        self.committed = true;
        self.cleanup();
        Ok(())
    }

    /// #644 评论 5488871385 问题1：rollback 前的 material preflight。
    ///
    /// 在真正进入 rollback 路径之前调用。对每个 `RestoreFile` 做 `File::open()`
    /// 确认可读（不用 `metadata()` 冒充"可读"）。
    /// 一个不满足就不能碰 Git/index/live。
    ///
    /// 在 `sync_ops.rs` 中，调用 `try_commit_git_finalize()` 之前调用本方法，
    /// 确保 backup material 完好，然后才进入 Git finalize。
    /// 如果 finalize 失败需要 rollback，backup 已确认可用。
    #[allow(clippy::excessive_nesting)]
    pub fn preflight_rollback_material(&self) -> Result<()> {
        if !self.backup_mode {
            return Ok(());
        }
        let backup_dir = self.tx_dir.join("backup");
        for entry in &self.backed_up_files {
            if let BackupEntry::RestoreFile {
                target_relative,
                backup_filename,
            } = entry
            {
                let backup_path = backup_dir.join(backup_filename);
                // 用 File::open() 确认真正可读，不用 metadata() 冒充。
                match std::fs::File::open(&backup_path) {
                    Ok(file) => {
                        drop(file);
                    }
                    Err(e) => {
                        return Err(crate::Error::Io(std::io::Error::other(format!(
                            "preflight_rollback_material: backup file not readable \
                             for RestoreFile {}: {}: {}",
                            target_relative,
                            backup_path.display(),
                            e
                        ))));
                    }
                }
            }
        }
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
