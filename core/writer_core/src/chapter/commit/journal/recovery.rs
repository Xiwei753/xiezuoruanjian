use super::super::publish::{publish_decided, publish_deleted};
use super::*;

/// 保留 Superseded 事务的真实材料、pending 事件和冲突记录。
fn preserve_superseded_conflict(
    project_root: &Path,
    tx_dir: &Path,
    manifest: &ChapterCommitManifest,
) -> Result<String> {
    let conflict_dir = journal_root(project_root)
        .join(CONFLICT_DIR)
        .join(&manifest.transaction_id);
    fs::create_dir_all(&conflict_dir)?;
    let _ = fs::copy(
        tx_dir.join("manifest.json"),
        conflict_dir.join("manifest.json"),
    );
    if tx_dir.join(BODY_FILE).exists() {
        let _ = fs::copy(tx_dir.join(BODY_FILE), conflict_dir.join(BODY_FILE));
    }
    for plan in &manifest.additional_files {
        let staged = tx_dir.join(&plan.staging_filename);
        if staged.exists() {
            let _ = fs::copy(&staged, conflict_dir.join(&plan.staging_filename));
        }
    }
    let actual = read_current(project_root, &manifest.key)
        .ok()
        .map(|(_, _, version)| version);
    let record = serde_json::json!({
        "transactionId": manifest.transaction_id,
        "key": manifest.key,
        "before": manifest.before,
        "after": manifest.after,
        "actual": actual,
        "reason": manifest.superseded_reason,
        "pendingProjections": manifest.pending_projections,
    });
    crate::storage::atomic_write_bytes(
        &conflict_dir.join("conflict.json"),
        serde_json::to_string_pretty(&record)?.as_bytes(),
    )?;
    crate::storage::sync_dir(&conflict_dir)?;
    Ok(conflict_dir.to_string_lossy().to_string())
}

/// 列出当前项目内已发布正文、但仍有证明投影未确认的 journal。
pub fn pending_chapter_proof_transactions(
    project_root: &Path,
) -> Result<Vec<super::PendingChapterProofTransaction>> {
    let root = journal_root(project_root);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut transactions = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let manifest_path = entry.path().join("manifest.json");
        let Ok(raw) = fs::read_to_string(&manifest_path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<ChapterCommitManifest>(&raw) else {
            continue;
        };
        if manifest.phase == ChapterCommitPhase::Applied && !manifest.pending_projections.is_empty()
        {
            transactions.push((
                manifest.commit_sequence,
                manifest.created_at_ms,
                super::PendingChapterProofTransaction {
                    transaction_id: manifest.transaction_id,
                    key: manifest.key,
                    commit_sequence: manifest.commit_sequence,
                },
            ));
        }
    }
    // read_dir 顺序不稳定；证明投影必须按发生顺序推进，否则同章后一笔会越过
    // 尚未完成的证明事件。时间戳相同的极端情况再用事务 ID 稳定排序。
    transactions.sort_by(|left, right| {
        (
            left.0,
            left.1,
            &left.2.key.chapter_id,
            &left.2.transaction_id,
        )
            .cmp(&(
                right.0,
                right.1,
                &right.2.key.chapter_id,
                &right.2.transaction_id,
            ))
    });
    Ok(transactions.into_iter().map(|(_, _, item)| item).collect())
}

/// 读取 journal 中尚未确认的证明事件（供投影 worker 使用）。
pub fn pending_chapter_proof_events(
    project_root: &Path,
    transaction_id: &str,
) -> Result<Vec<super::ChapterProofProjection>> {
    let manifest_path = journal_root(project_root)
        .join(transaction_id)
        .join("manifest.json");
    if !manifest_path.exists() {
        return Ok(Vec::new());
    }
    let manifest: ChapterCommitManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    if manifest.phase != ChapterCommitPhase::Applied {
        return Ok(Vec::new());
    }
    // 只返回仍未确认的事件。journal 保留了完整批次供恢复诊断，但 worker
    // 重放时只能投影 pending_projections，不能把已 ack 的事件重复写入。
    let pending: std::collections::HashSet<&str> = manifest
        .pending_projections
        .iter()
        .map(String::as_str)
        .collect();
    Ok(manifest
        .proof_events
        .into_iter()
        .filter(|projection| pending.contains(projection.event_id()))
        .collect())
}

/// 读取 journal 中尚未确认的同步正文事实证明。
pub fn pending_chapter_sync_proof_facts(
    project_root: &Path,
    transaction_id: &str,
) -> Result<Vec<super::ChapterSyncProofFact>> {
    let manifest_path = journal_root(project_root)
        .join(transaction_id)
        .join("manifest.json");
    if !manifest_path.exists() {
        return Ok(Vec::new());
    }
    let manifest: ChapterCommitManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    if manifest.phase != ChapterCommitPhase::Applied {
        return Ok(Vec::new());
    }
    let pending: std::collections::HashSet<&str> = manifest
        .pending_projections
        .iter()
        .map(String::as_str)
        .collect();
    Ok(manifest
        .sync_proof_facts
        .into_iter()
        .filter(|fact| pending.contains(fact.event_id.as_str()))
        .collect())
}

/// 收集项目内所有 journal 尚未确认的同步正文事实 ID。
///
/// 同步层在正文落盘后原本会立即独立写证明；若该事实已随正文 journal
/// durable，就必须交给投影 worker 按 commit sequence 结算，不能重复写。
pub fn pending_chapter_sync_proof_fact_ids(
    project_root: &Path,
) -> Result<std::collections::HashSet<String>> {
    let root = journal_root(project_root);
    let mut ids = std::collections::HashSet::new();
    if !root.exists() {
        return Ok(ids);
    }
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Ok(raw) = fs::read_to_string(entry.path().join("manifest.json")) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<ChapterCommitManifest>(&raw) else {
            continue;
        };
        if manifest.phase != ChapterCommitPhase::Applied {
            continue;
        }
        for fact in &manifest.sync_proof_facts {
            if manifest
                .pending_projections
                .iter()
                .any(|id| id == &fact.event_id)
            {
                ids.insert(fact.event_id.clone());
            }
        }
    }
    Ok(ids)
}
/// 扫描并恢复未完成 journal。按 commit sequence 顺序处理；只处理未完成事务，
/// 不遍历作品正文。调用方应持有工作区写锁（或传入 app_data_root）。
/// 重放事务目录里记录的待同步 mutation 事实。
///
/// 投影在章节提交返回前进行；写盘失败或进程崩溃时事务目录里的 outbox 是
/// 唯一可重放来源，成功追加后才删除。失败保留文件等下一次恢复。
fn replay_sync_mutation_outbox(project_root: &Path, tx_dir: &Path) {
    let outbox = tx_dir.join("sync-mutations.json");
    let Ok(text) = fs::read_to_string(&outbox) else {
        return;
    };
    let Ok(records) =
        serde_json::from_str::<Vec<crate::sync::local_mutations::LocalMutationRecord>>(&text)
    else {
        log::warn!(
            "chapter commit mutation outbox unreadable, keeping for diagnosis: {}",
            outbox.display()
        );
        return;
    };
    match crate::sync::local_mutations::append_batch(project_root, &records) {
        Ok(()) => {
            let _ = fs::remove_file(&outbox);
        }
        Err(error) => log::warn!(
            "chapter commit mutation outbox replay failed (will retry): {error}"
        ),
    }
}

pub fn recover_pending_chapter_commits(project_root: &Path) -> Result<Vec<ChapterCommitRecovery>> {
    let root = journal_root(project_root);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let dir = entry.path();
        if dir.file_name().and_then(|name| name.to_str()) == Some(CONFLICT_DIR) {
            continue;
        }
        // 上次提交已 durable 但 mutation 投影失败的残留：先重放到账本再继续，
        // 成功后才删除 outbox（复审 #5）。
        replay_sync_mutation_outbox(project_root, &dir);
        let manifest_path = dir.join("manifest.json");
        if !manifest_path.exists() {
            // 材料都还没落盘完的目录：没有决议，删除自己的未发布材料。
            let _ = fs::remove_dir_all(&dir);
            continue;
        }
        match serde_json::from_str::<ChapterCommitManifest>(&fs::read_to_string(&manifest_path)?) {
            Ok(manifest) => entries.push((dir, manifest)),
            Err(error) => {
                log::warn!(
                    "chapter commit journal manifest parse failed, preserving: {}: {}",
                    manifest_path.display(),
                    error
                );
            }
        }
    }
    entries.sort_by_key(|(_, manifest)| (manifest.commit_sequence, manifest.created_at_ms));

    let mut recovered = Vec::new();
    for (tx_dir, mut manifest) in entries {
        match manifest.phase {
            ChapterCommitPhase::Prepared => {
                // 未决议：live 从未被触碰。保留目录供人工诊断，不能删除后
                // 继续写 Finished manifest；pending 探针会由空闲清理移除。
                continue;
            }
            ChapterCommitPhase::Decided => {
                let published = if manifest.deleted {
                    publish_deleted(project_root, &tx_dir, &mut manifest)?
                } else {
                    publish_decided(project_root, &tx_dir, &mut manifest)?
                };
                if !published {
                    let conflict_dir =
                        preserve_superseded_conflict(project_root, &tx_dir, &manifest)?;
                    recovered.push(ChapterCommitRecovery {
                        transaction_id: manifest.transaction_id.clone(),
                        key: manifest.key.clone(),
                        after: manifest.after.clone(),
                        pending_projections: manifest.pending_projections.clone(),
                        draft_ack_projections: manifest.draft_ack_projections.clone(),
                        superseded: true,
                        conflict_dir: Some(conflict_dir),
                    });
                    continue;
                }
            }
            ChapterCommitPhase::Applied => {}
            ChapterCommitPhase::Finished | ChapterCommitPhase::Superseded => {
                if manifest.phase == ChapterCommitPhase::Finished {
                    let _ = remove_transaction_dir(&tx_dir);
                }
                continue;
            }
        }

        if manifest.pending_projections.is_empty() && manifest.draft_ack_projections.is_empty() {
            manifest.phase = ChapterCommitPhase::Finished;
            write_manifest(&tx_dir, &manifest)?;
            let _ = remove_transaction_dir(&tx_dir);
            continue;
        }
        recovered.push(ChapterCommitRecovery {
            transaction_id: manifest.transaction_id.clone(),
            key: manifest.key.clone(),
            after: manifest.after.clone(),
            pending_projections: manifest.pending_projections.clone(),
            draft_ack_projections: manifest.draft_ack_projections.clone(),
            superseded: false,
            conflict_dir: None,
        });
    }
    clear_pending_if_idle(project_root)?;
    Ok(recovered)
}

/// 列出 Core 已 durable、但平台草稿尚未确认的原始证明事件。
///
/// Issue #1062：Qt 本地 pending-acks 只是缓存；跨进程确认事实源是这里。
/// 平台启动/进入作品时按 commit_sequence 顺序重放，再回写
/// `mark_chapter_commit_draft_ack`。
///
/// 没有 manifest 的残留目录按 Prepared 跳过；manifest 存在但不可读或
/// JSON 损坏时返回 `Err`，此时无法判定是否仍有待确认事项，调用方必须
/// 保持保存闸门关闭。
pub fn pending_chapter_draft_acks(
    project_root: &Path,
) -> Result<Vec<super::PendingChapterDraftAck>> {
    let root = journal_root(project_root);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut acks = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let tx_dir = entry.path();
        let tx_name = entry.file_name().to_string_lossy().into_owned();
        if tx_name == CONFLICT_DIR {
            continue;
        }
        let manifest_path = tx_dir.join("manifest.json");
        let raw = match fs::read_to_string(&manifest_path) {
            Ok(raw) => raw,
            Err(error) => {
                // 只有残留材料、没有 durable 决议的目录按 Prepared 跳过；
                // manifest 存在却读不出来说明无法判定是否仍有待确认事项，
                // 必须向上报错，不能把它当成“无待确认”放行保存闸门。
                if !manifest_path.exists() {
                    continue;
                }
                return Err(crate::error::Error::Io(std::io::Error::new(
                    error.kind(),
                    format!(
                        "chapter commit journal manifest unreadable: transaction={tx_name}, path={}, error={error}",
                        manifest_path.display()
                    ),
                )));
            }
        };
        let manifest = match serde_json::from_str::<ChapterCommitManifest>(&raw) {
            Ok(manifest) => manifest,
            Err(error) => {
                return Err(crate::error::Error::Json(
                    <serde_json::Error as serde::de::Error>::custom(format!(
                        "chapter commit journal manifest corrupt: transaction={tx_name}, path={}, error={error}",
                        manifest_path.display()
                    )),
                ));
            }
        };
        if manifest.draft_ack_projections.is_empty()
            || !matches!(
                manifest.phase,
                ChapterCommitPhase::Applied | ChapterCommitPhase::Decided
            )
        {
            continue;
        }
        acks.push((
            manifest.commit_sequence,
            manifest.created_at_ms,
            super::PendingChapterDraftAck {
                transaction_id: manifest.transaction_id.clone(),
                key: manifest.key.clone(),
                event_ids: manifest.draft_ack_projections.clone(),
                after: manifest.after.clone(),
                commit_sequence: manifest.commit_sequence,
            },
        ));
    }
    acks.sort_by(|left, right| {
        (
            left.0,
            left.1,
            &left.2.key.chapter_id,
            &left.2.transaction_id,
        )
            .cmp(&(
                right.0,
                right.1,
                &right.2.key.chapter_id,
                &right.2.transaction_id,
            ))
    });
    Ok(acks.into_iter().map(|(_, _, ack)| ack).collect())
}
