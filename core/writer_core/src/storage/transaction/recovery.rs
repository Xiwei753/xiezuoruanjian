use std::fs;
use std::path::Path;

use super::model::*;

/// 扫描事务目录，恢复未完成的事务，并保留所有无法判定的事务错误。
///
/// 与 [`recover_pending_transactions`] 相比，本函数不会吞掉 manifest 读/解析
/// 失败：证明意图恢复必须知道“事务状态未知”，不能在未知时删除意图。
/// 每笔错误都绑定到具体事务（[`TransactionRecoveryError`]），调用方不能靠
/// 字符串猜测错误属于哪笔事务。
pub(crate) fn scan_pending_transactions(target_root: &Path) -> TransactionRecoveryScan {
    let tx_base = target_root.join(TRANSACTIONS_DIR);
    let mut scan = TransactionRecoveryScan::default();
    if !tx_base.exists() {
        return scan;
    }

    let entries = match fs::read_dir(&tx_base) {
        Ok(e) => e,
        Err(error) => {
            scan.errors.push(TransactionRecoveryError {
                transaction_id: None,
                message: format!("read transaction dir {} failed: {error}", tx_base.display()),
            });
            return scan;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(error) => {
                scan.errors.push(TransactionRecoveryError {
                    transaction_id: None,
                    message: format!("read transaction entry failed: {error}"),
                });
                continue;
            }
        };
        let tx_dir = entry.path();
        if !tx_dir.is_dir() {
            continue;
        }
        recover_transaction_dir(target_root, tx_dir, &mut scan);
    }

    scan
}

/// 只按事务 ID 扫描并重放一个事务目录。
///
/// 事务目录名就是 `SaveTransaction` 的 `transaction_id`（见
/// `SaveTransaction::new`）。只处理该目录，不读取、不清理同项目里的其它
/// 事务，避免把无关 Prepared 事务的缺项算到本次操作头上。目录不存在时
/// 返回空结果（视为已经清理完成）。
pub(crate) fn scan_pending_transaction(
    target_root: &Path,
    transaction_id: &str,
) -> TransactionRecoveryScan {
    let mut scan = TransactionRecoveryScan::default();
    let tx_dir = target_root.join(TRANSACTIONS_DIR).join(transaction_id);
    if !tx_dir.is_dir() {
        return scan;
    }
    recover_transaction_dir(target_root, tx_dir, &mut scan);
    scan
}

/// 重放单个事务目录：读取 manifest、按阶段收敛文件，并在完成后清理目录。
#[derive(Debug)]
enum RecoveryTargetState {
    Before,
    After,
    Conflict(String),
}

fn recover_transaction_dir(
    target_root: &Path,
    tx_dir: std::path::PathBuf,
    scan: &mut TransactionRecoveryScan,
) {
    // 目录名即事务 ID；manifest 解析失败时仍用它把错误绑定到具体事务。
    let dir_transaction_id = tx_dir
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToString::to_string);
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
                    record_scan_error(scan, dir_transaction_id, message);
                    return;
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
                record_scan_error(scan, dir_transaction_id, message);
                return;
            }
        };

        match manifest.phase {
            TransactionPhase::RolledBack => {
                let _ = fs::remove_dir_all(&tx_dir);
                return;
            }
            TransactionPhase::FilesCommitted | TransactionPhase::Finished
                if manifest.proof_intent_ids.is_empty() =>
            {
                let _ = fs::remove_dir_all(&tx_dir);
                return;
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
                    target_root: target_root.to_path_buf(),
                });
                return;
            }
            TransactionPhase::Prepared => {
                // Prepared 阶段：尝试重放 rename。
            }
        }
    } else {
        let commit_marker = tx_dir.join(COMMIT_MARKER);
        if commit_marker.exists() {
            let _ = fs::remove_dir_all(&tx_dir);
            return;
        }
        let _ = fs::remove_dir_all(&tx_dir);
        return;
    }

    fn file_md5(path: &Path) -> std::io::Result<Option<String>> {
        match fs::read(path) {
            Ok(bytes) => Ok(Some(format!("{:x}", md5::compute(&bytes)))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
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
                record_scan_error(scan, dir_transaction_id, message);
                return;
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
            record_scan_error(scan, dir_transaction_id, message);
            return;
        }
    };

    let mut recovered_files = Vec::new();
    let mut missing_files = Vec::new();
    let mut created_dirs = std::collections::HashSet::new();

    for tx_entry in &manifest.entries {
        let target_path = target_root.join(&tx_entry.target_relative);
        let current = match file_md5(&target_path) {
            Ok(value) => value,
            Err(error) => {
                log::warn!(
                    "[transaction] recovery read target failed, preserving tx: {}: {}",
                    tx_entry.target_relative,
                    error
                );
                missing_files.push(tx_entry.target_relative.clone());
                continue;
            }
        };
        let before = tx_entry.expected_before_hash.as_deref();
        let after = tx_entry.expected_after_hash.as_deref();
        let state = match (current.as_deref(), before, after) {
            (Some(current), _, Some(after)) if current == after => RecoveryTargetState::After,
            (Some(current), Some(before), _) if current == before => RecoveryTargetState::Before,
            (None, None, Some(_)) if !tx_entry.is_delete => RecoveryTargetState::Before,
            (None, None, None) if tx_entry.is_delete => RecoveryTargetState::After,
            (None, Some(_), None) if tx_entry.is_delete => RecoveryTargetState::After,
            (Some(current), None, None) => RecoveryTargetState::Conflict(format!(
                "legacy manifest without before/after hash, target={current}"
            )),
            (None, None, _) => RecoveryTargetState::Conflict(
                "legacy manifest without before/after hash, target missing".to_string(),
            ),
            (Some(current), before, after) => RecoveryTargetState::Conflict(format!(
                "target changed: current={current}, before={before:?}, after={after:?}"
            )),
            (None, before, after) => RecoveryTargetState::Conflict(format!(
                "target missing: before={before:?}, after={after:?}"
            )),
        };

        match state {
            RecoveryTargetState::Conflict(reason) => {
                log::warn!(
                    "[transaction] recovery conflict, preserving materials: {}: {}",
                    tx_entry.target_relative,
                    reason
                );
                missing_files.push(tx_entry.target_relative.clone());
            }
            RecoveryTargetState::After => {
                if let Err(e) = crate::storage::sync_parent(&target_path) {
                    log::warn!(
                        "[transaction] recovery ack sync_parent failed: {}: {}",
                        tx_entry.target_relative,
                        e
                    );
                    missing_files.push(tx_entry.target_relative.clone());
                } else {
                    recovered_files.push(tx_entry.target_relative.clone());
                }
            }
            RecoveryTargetState::Before => {
                if tx_entry.is_delete {
                    match fs::remove_file(&target_path) {
                        Ok(()) => {
                            if let Err(e) = crate::storage::sync_parent(&target_path) {
                                log::warn!(
                                    "[transaction] recovery delete sync_parent failed: {}: {}",
                                    tx_entry.target_relative,
                                    e
                                );
                                missing_files.push(tx_entry.target_relative.clone());
                            } else {
                                recovered_files.push(tx_entry.target_relative.clone());
                            }
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
                    continue;
                }
                let staging_path = tx_dir.join(&tx_entry.staging_filename);
                if !staging_path.exists() {
                    log::warn!(
                        "[transaction] recovery staging missing, preserving: {}",
                        tx_entry.target_relative
                    );
                    missing_files.push(tx_entry.target_relative.clone());
                    continue;
                }
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
                        } else {
                            recovered_files.push(tx_entry.target_relative.clone());
                        }
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
        if missing_files.is_empty() {
            // 全部条目重放成功，事务可判定完成，manifest/staging 不再需要。
            let _ = fs::remove_dir_all(&tx_dir);
        } else {
            // Issue #1051 复审：无 proof intent 不等于恢复完成。只要有
            // 条目重放失败，staged 文件与 manifest 就是唯一的恢复依据，
            // 必须保留 Prepared 目录供下次启动继续幂等重放。
            log::warn!(
                "[transaction] recover: tx={} has {} unreplayed file(s), \
                 preserving Prepared manifest and staging for retry",
                manifest.transaction_id,
                missing_files.len()
            );
        }
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
                    target_root: target_root.to_path_buf(),
                });
                return;
            }
            Err(error) => {
                let message = format!(
                    "[transaction] recover: persist FilesCommitted failed for tx={} at {}: {error}",
                    manifest.transaction_id,
                    tx_dir.display()
                );
                log::warn!("{message}");
                record_scan_error(scan, Some(manifest.transaction_id.clone()), message);
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
        target_root: target_root.to_path_buf(),
    });
}

/// 记录一条绑定到具体事务的扫描错误。
fn record_scan_error(
    scan: &mut TransactionRecoveryScan,
    transaction_id: Option<String>,
    message: String,
) {
    scan.errors.push(TransactionRecoveryError {
        transaction_id,
        message,
    });
}

/// 兼容旧调用方：只返回已识别的事务，不暴露扫描错误。
#[cfg(test)]
pub fn recover_pending_transactions(target_root: &Path) -> Vec<TransactionRecovery> {
    scan_pending_transactions(target_root).transactions
}
