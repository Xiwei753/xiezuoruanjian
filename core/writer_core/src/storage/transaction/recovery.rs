use std::fs;
use std::path::Path;

use super::model::*;

/// 扫描事务目录，恢复未完成的事务。
///
/// 判定逻辑：
/// - manifest 存在且 phase 为 `FilesCommitted`/`Finished`/`RolledBack` → 清理目录
/// - manifest 存在且 phase 为 `Prepared` → 尝试将暂存文件 rename 到目标
/// - 旧格式：`committed` 标记存在 → 清理目录（向后兼容）
/// - 两者都不存在 → 无效目录，清理
///
/// #645 评论 5504296097 问题5(b)：`FilesCommittedPendingGit` 变体已移除。
/// 旧 manifest 中 `"files_committed_pending_git"` 在反序列化时映射到
/// `FilesCommitted`，因此会走 `FilesCommitted` 分支（清理目录）。
/// 这与之前"直接 rollback"不同，但新代码不再产生 `FilesCommittedPendingGit`，
/// 旧遗留事务的文件已 rename 完成（phase=FilesCommittedPendingGit 表示
/// 文件已 commit 但 Git finalize 未完成），清理目录是安全的行为——
/// 文件已在 live，无需 rollback。
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
