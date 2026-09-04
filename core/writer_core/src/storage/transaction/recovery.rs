use std::fs;
use std::path::Path;

use super::model::*;
use crate::error::Result;

/// 在真正进入 rollback 路径之前，先检查所有 BackupEntry::RestoreFile 是否可读。
///
/// 必须在任何 live 文件修改之前调用。一个不满足就不能碰 live。
#[cfg(test)]
fn preflight_backup_entries(tx_dir: &Path, manifest: &TransactionManifest) -> Result<()> {
    let backup_dir = tx_dir.join("backup");
    for entry in &manifest.backup_entries {
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
                        "preflight_backup_entries: backup file not readable \
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

/// 回滚文件事务 — 恢复 backup 文件到目标路径。
///
/// 满足：
/// - 文件 rollback 成功 → phase=RolledBack → cleanup
/// - 任意一步失败 → 返回 Err → 保留 manifest + backup + transaction 目录
#[cfg(test)]
#[allow(clippy::too_many_lines, clippy::excessive_nesting)]
fn rollback_file_transaction(
    tx_dir: &Path,
    target_root: &Path,
    manifest: &TransactionManifest,
) -> Result<()> {
    preflight_backup_entries(tx_dir, manifest)?;

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
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "rollback_file_transaction: backup file missing for RestoreFile {}: {}",
                        target_relative,
                        backup_path.display()
                    ))));
                }
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                crate::storage::durable_copy_file(&backup_path, &target_path)?;
            }
            BackupEntry::RemoveCreated { target_relative } => {
                let target_path = target_root.join(target_relative);
                if target_path.exists() {
                    fs::remove_file(&target_path)?;
                    crate::storage::sync_parent(&target_path)?;
                }
            }
        }
    }

    let manifest_path = tx_dir.join(MANIFEST_FILENAME);
    write_manifest_phase_static(&manifest_path, TransactionPhase::RolledBack, manifest)?;

    fs::remove_dir_all(tx_dir)?;

    Ok(())
}

/// 静态版本的 manifest phase 更新，供恢复流程使用。
#[cfg(test)]
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
/// 判定逻辑：
/// - manifest 存在且 phase 为 `FilesCommitted`/`Finished`/`RolledBack` → 清理目录
/// - manifest 存在且 phase 为 `FilesCommittedPendingGit` → 旧遗留，直接回滚
/// - manifest 存在且 phase 为 `Prepared` → 尝试将暂存文件 rename 到目标
/// - 旧格式：`committed` 标记存在 → 清理目录（向后兼容）
/// - 两者都不存在 → 无效目录，清理
// TODO(#597): 既有代码可读性技术债，待后续重构拆分
#[cfg(test)]
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
) -> Vec<TransactionRecovery> {
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

        let manifest_path = tx_dir.join(MANIFEST_FILENAME);

        if manifest_path.exists() {
            let manifest: TransactionManifest = match fs::read_to_string(&manifest_path) {
                Ok(s) => match serde_json::from_str(&s) {
                    Ok(m) => m,
                    Err(e) => {
                        log::warn!(
                            "[transaction] recover: manifest parse failed for tx_dir={}: {} \
                             — preserving tx_dir, will retry next startup",
                            tx_dir.display(),
                            e
                        );
                        continue;
                    }
                },
                Err(e) => {
                    log::warn!(
                        "[transaction] recover: manifest read failed for tx_dir={}: {} \
                         — preserving tx_dir, will retry next startup",
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
                    let _ = fs::remove_dir_all(&tx_dir);
                    continue;
                }
                TransactionPhase::FilesCommittedPendingGit => {
                    log::warn!(
                        "[transaction] found legacy FilesCommittedPendingGit tx={}, rolling back",
                        manifest.transaction_id
                    );
                    if let Err(e) =
                        rollback_file_transaction(&tx_dir, target_root, &manifest)
                    {
                        log::warn!(
                            "[transaction] rollback_file_transaction failed for tx={}: {}",
                            manifest.transaction_id,
                            e
                        );
                        continue;
                    }
                    continue;
                }
                TransactionPhase::Prepared => {
                    // Prepared 阶段：尝试重放 rename。
                }
            }
        } else {
            let commit_marker = tx_dir.join(COMMIT_MARKER);
            if commit_marker.exists() {
                let _ = fs::remove_dir_all(&tx_dir);
                continue;
            }
            let _ = fs::remove_dir_all(&tx_dir);
            continue;
        }

        // Prepared 阶段恢复：重放 rename。
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
                let target_path = target_root.join(&tx_entry.target_relative);
                match fs::remove_file(&target_path) {
                    Ok(()) => {
                        if let Err(e) = crate::storage::sync_parent(&target_path) {
                            log::warn!(
                                "[transaction] recovery delete sync_parent failed: {}: {}",
                                tx_entry.target_relative,
                                e
                            );
                            missing_files.push(tx_entry.target_relative.clone());
                            continue;
                        }
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
                        Ok(()) => {
                            if let Err(e) = crate::storage::sync_parent(&target_path) {
                                log::warn!(
                                    "[transaction] recovery rename sync_parent failed: {} -> {}: {}",
                                    tx_entry.staging_filename,
                                    tx_entry.target_relative,
                                    e
                                );
                                missing_files.push(tx_entry.target_relative.clone());
                                continue;
                            }
                            recovered_files.push(tx_entry.target_relative.clone());
                        }
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
