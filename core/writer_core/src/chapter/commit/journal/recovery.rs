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

/// 扫描并恢复未完成 journal。按 `created_at_ms` 顺序处理；只处理未完成事务，
/// 不遍历作品正文。调用方应持有工作区写锁（或传入 app_data_root）。
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
                let published = publish_decided(project_root, &tx_dir, &mut manifest)?;
                if !published {
                    let conflict_dir =
                        preserve_superseded_conflict(project_root, &tx_dir, &manifest)?;
                    recovered.push(ChapterCommitRecovery {
                        transaction_id: manifest.transaction_id.clone(),
                        key: manifest.key.clone(),
                        after: manifest.after.clone(),
                        pending_projections: manifest.pending_projections.clone(),
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

        if manifest.pending_projections.is_empty() {
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
            superseded: false,
            conflict_dir: None,
        });
    }
    clear_pending_if_idle(project_root)?;
    Ok(recovered)
}
