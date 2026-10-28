use std::fs;
use std::path::Path;

use super::model::*;

/// 扫描事务目录，恢复未完成的事务，并保留所有无法判定的事务错误。
///
/// 与 [`recover_pending_transactions`] 相比，本函数不会吞掉 manifest 读/解析
/// 失败：证明意图恢复必须知道“事务状态未知”，不能在未知时删除意图。
pub(crate) fn scan_pending_transactions(target_root: &Path) -> TransactionRecoveryScan {
    let tx_base = target_root.join(TRANSACTIONS_DIR);
    let mut scan = TransactionRecoveryScan::default();
    if !tx_base.exists() {
        return scan;
    }

    let entries = match fs::read_dir(&tx_base) {
        Ok(e) => e,
        Err(error) => {
            scan.errors.push(format!(
                "read transaction dir {} failed: {error}",
                tx_base.display()
            ));
            return scan;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(error) => {
                scan.errors
                    .push(format!("read transaction entry failed: {error}"));
                continue;
            }
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
                        let message = format!(
                            "[transaction] recover: manifest parse failed for tx_dir={}: {} \
                             — preserving tx_dir, will retry next startup",
                            tx_dir.display(),
                            e
                        );
                        log::warn!("{message}");
                        scan.errors.push(message);
                        continue;
                    }
                },
                Err(e) => {
                    let message = format!(
                        "[transaction] recover: manifest read failed for tx_dir={}: {} \
                         — preserving tx_dir, will retry next startup",
                        tx_dir.display(),
                        e
                    );
                    log::warn!("{message}");
                    scan.errors.push(message);
                    continue;
                }
            };

            match manifest.phase {
                TransactionPhase::RolledBack => {
                    let _ = fs::remove_dir_all(&tx_dir);
                    continue;
                }
                TransactionPhase::FilesCommitted | TransactionPhase::Finished
                    if manifest.proof_intent_ids.is_empty() =>
                {
                    let _ = fs::remove_dir_all(&tx_dir);
                    continue;
                }
                TransactionPhase::FilesCommitted | TransactionPhase::Finished => {
                    // 正文已提交，但绑定的证明意图尚未写入证明事件链；
                    // 保留 manifest 供 proof_intent recovery 依据 phase 补记。
                    scan.transactions.push(TransactionRecovery {
                        transaction_id: manifest.transaction_id,
                        recovered_files: Vec::new(),
                        missing_files: Vec::new(),
                        phase: manifest.phase,
                        proof_intent_ids: manifest.proof_intent_ids,
                        tx_dir,
                    });
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
                    let message = format!(
                        "[transaction] recover: manifest re-read parse failed for tx_dir={}: {} \
                         — preserving tx_dir, will retry next startup",
                        tx_dir.display(),
                        e
                    );
                    log::warn!("{message}");
                    scan.errors.push(message);
                    continue;
                }
            },
            Err(e) => {
                let message = format!(
                    "[transaction] recover: manifest re-read failed for tx_dir={}: {} \
                     — preserving tx_dir, will retry next startup",
                    tx_dir.display(),
                    e
                );
                log::warn!("{message}");
                scan.errors.push(message);
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
                let target_path = target_root.join(&tx_entry.target_relative);
                if staging_path.exists() {
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
                } else if target_path.exists() {
                    // 崩溃可能发生在 rename 完成、manifest 尚未写成 FilesCommitted
                    // 的窗口内：staging 已消失但目标正文已就位，说明该条已提交。
                    if let Err(e) = crate::storage::sync_parent(&target_path) {
                        log::warn!(
                            "[transaction] recovery existing target sync_parent failed: {}: {}",
                            tx_entry.target_relative,
                            e
                        );
                        missing_files.push(tx_entry.target_relative.clone());
                        continue;
                    }
                    recovered_files.push(tx_entry.target_relative.clone());
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

        let phase = if manifest.proof_intent_ids.is_empty() {
            let _ = fs::remove_dir_all(&tx_dir);
            TransactionPhase::Prepared
        } else if missing_files.is_empty() {
            let mut restored = manifest.clone();
            restored.phase = TransactionPhase::FilesCommitted;
            match serde_json::to_string_pretty(&restored)
                .map_err(crate::Error::from)
                .and_then(|json| crate::storage::atomic_write_string(&manifest_path, &json))
            {
                Ok(()) => {
                    scan.transactions.push(TransactionRecovery {
                        transaction_id: restored.transaction_id,
                        recovered_files,
                        missing_files,
                        phase: TransactionPhase::FilesCommitted,
                        proof_intent_ids: restored.proof_intent_ids,
                        tx_dir,
                    });
                    continue;
                }
                Err(error) => {
                    let message = format!(
                        "[transaction] recover: persist FilesCommitted failed for tx={} at {}: {error}",
                        manifest.transaction_id,
                        tx_dir.display()
                    );
                    log::warn!("{message}");
                    scan.errors.push(message);
                    TransactionPhase::Prepared
                }
            }
        } else {
            // 重放材料不完整，不能宣称事务已提交；保留 Prepared 目录，让证明
            // 恢复保留意图并等待下一次启动重试。
            TransactionPhase::Prepared
        };

        scan.transactions.push(TransactionRecovery {
            transaction_id: manifest.transaction_id,
            recovered_files,
            missing_files,
            phase,
            proof_intent_ids: manifest.proof_intent_ids,
            tx_dir,
        });
    }

    scan
}

/// 兼容旧调用方：只返回已识别的事务，不暴露扫描错误。
#[cfg(test)]
pub fn recover_pending_transactions(target_root: &Path) -> Vec<TransactionRecovery> {
    scan_pending_transactions(target_root).transactions
}
