use super::super::publish::{publish_decided, publish_deleted};
use super::*;

/// 把一份冲突材料原子写入归档目录（含目标文件与父目录 fsync）。
///
/// 先读后写走既有 `atomic_write_bytes`，不直接 `fs::copy`：复制成功但目标
/// 内容未落盘时，恢复入口不能把 Superseded 当成已完成归档。
fn copy_conflict_material(source: &Path, target: &Path) -> Result<()> {
    let bytes = fs::read(source).map_err(|error| {
        Error::Other(format!(
            "failed to read chapter commit material {} for conflict archive: {error}",
            source.display()
        ))
    })?;
    crate::storage::atomic_write_bytes(target, &bytes).map_err(|error| {
        Error::Other(format!(
            "failed to preserve chapter commit material {} -> {}: {error}",
            source.display(),
            target.display()
        ))
    })
}

/// 保留 Superseded 事务的真实材料、pending 事件和冲突记录。
///
/// 幂等：归档目录里已经存在的文件直接复用；恢复入口对 Superseded 事务每次
/// 都会调用本函数补齐缺失材料，复制失败时保留 tx_dir 供下次重试。
pub(crate) fn preserve_superseded_conflict(
    project_root: &Path,
    tx_dir: &Path,
    manifest: &ChapterCommitManifest,
) -> Result<String> {
    let conflict_dir = journal_root(project_root)
        .join(CONFLICT_DIR)
        .join(&manifest.transaction_id);
    fs::create_dir_all(&conflict_dir)?;
    // conflict.json 是最后一步原子写入的完成标记：它存在说明材料已完整
    // 归档，恢复轮直接复用，不重复复制/写盘。
    if conflict_dir.join("conflict.json").is_file() {
        return Ok(conflict_dir.to_string_lossy().to_string());
    }
    // 归档清单用当前内存中的真实事务状态写入（调用方可能刚标记
    // Superseded，尚未回写 tx_dir 清单），不能复制旧阶段文件。
    write_manifest(&conflict_dir, manifest).map_err(|error| {
        Error::Other(format!(
            "failed to archive chapter commit manifest {} -> {}: {error}",
            tx_dir.display(),
            conflict_dir.display()
        ))
    })?;
    let staged_body = tx_dir.join(BODY_FILE);
    let archived_body = conflict_dir.join(BODY_FILE);
    if manifest.deleted {
        // 删除冲突没有待发布正文；事务里若保留了正文材料则一并归档。
        if staged_body.exists() && !archived_body.exists() {
            copy_conflict_material(&staged_body, &archived_body)?;
        }
    } else if !archived_body.exists() {
        // 正文提交冲突：正文材料必须 durable，缺它说明事务材料被外部破坏。
        copy_conflict_material(&staged_body, &archived_body)?;
    }
    for plan in &manifest.additional_files {
        let target = conflict_dir.join(&plan.staging_filename);
        if target.exists() {
            continue;
        }
        copy_conflict_material(&tx_dir.join(&plan.staging_filename), &target)?;
    }
    let actual = match read_current(project_root, &manifest.key) {
        Ok((_, _, version)) => Some(version),
        Err(Error::ChapterNotFound) => None,
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to read current chapter while preserving conflict {}: {error}",
                manifest.transaction_id
            )))
        }
    };
    let record = serde_json::json!({
        "transactionId": manifest.transaction_id,
        "key": manifest.key,
        "before": manifest.before,
        "after": manifest.after,
        "actual": actual,
        "reason": manifest.superseded_reason,
        "pendingProjections": manifest.pending_projections,
    });
    let conflict_json = conflict_dir.join("conflict.json");
    crate::storage::atomic_write_bytes(
        &conflict_json,
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
    match fs::metadata(&root) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to inspect chapter commit journal root {}: {error}",
                root.display()
            )))
        }
    }
    let mut transactions = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let manifest_path = entry.path().join("manifest.json");
        let raw = match fs::read_to_string(&manifest_path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(Error::Other(format!(
                    "failed to read chapter commit manifest {}: {error}",
                    manifest_path.display()
                )))
            }
        };
        let manifest = match serde_json::from_str::<ChapterCommitManifest>(&raw) {
            Ok(manifest) => manifest,
            Err(error) => {
                // 损坏清单保留在磁盘上供诊断；恢复轮必须显式报告该事务，
                // 但不把它当成可投影事实，也不阻断其它事务。
                log::error!(
                    "chapter commit manifest {} is corrupt, transaction kept and skipped: {error}",
                    manifest_path.display()
                );
                continue;
            }
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
    let raw = match fs::read_to_string(&manifest_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to read chapter commit manifest {}: {error}",
                manifest_path.display()
            )))
        }
    };
    let manifest: ChapterCommitManifest = serde_json::from_str(&raw)?;
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
    let raw = match fs::read_to_string(&manifest_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to read chapter commit manifest {}: {error}",
                manifest_path.display()
            )))
        }
    };
    let manifest: ChapterCommitManifest = serde_json::from_str(&raw)?;
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
    match fs::metadata(&root) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(ids),
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to inspect chapter commit journal root {}: {error}",
                root.display()
            )))
        }
    }
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let manifest_path = entry.path().join("manifest.json");
        let raw = match fs::read_to_string(&manifest_path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(Error::Other(format!(
                    "failed to read chapter commit manifest {}: {error}",
                    manifest_path.display()
                )))
            }
        };
        let manifest = match serde_json::from_str::<ChapterCommitManifest>(&raw) {
            Ok(manifest) => manifest,
            Err(error) => {
                log::error!(
                    "chapter commit manifest {} is corrupt, transaction kept and skipped for proof facts: {error}",
                    manifest_path.display()
                );
                continue;
            }
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
/// 按 durable 决议把章节提交要投影的同步对象写进账本（复审 #1）。
///
/// 对象集合/op/业务版本来自 manifest 的 `sync_mutations`（Decided 之前就已
/// 落盘），发布后的真实字节摘要在这里从目标文件读取。成功后把
/// `sync_projection_done` 写回同一份 manifest，进程在下一次恢复时据此幂等
/// 重放；失败时保持 false 并保留事务目录，绝不当作已收敛。
pub(super) fn project_manifest_sync_mutations(
    project_root: &Path,
    tx_dir: &Path,
    manifest: &mut ChapterCommitManifest,
) -> Result<bool> {
    if manifest.sync_projection_done {
        return Ok(true);
    }
    if manifest.sync_mutations.is_empty() {
        manifest.sync_projection_done = true;
        write_manifest(tx_dir, manifest)?;
        return Ok(true);
    }
    let mut records = Vec::with_capacity(manifest.sync_mutations.len());
    for plan in &manifest.sync_mutations {
        let (object_type, object_id) = crate::sync::local_mutations::object_identity(&plan.path);
        let content_hash = if plan.op == "upsert" {
            match fs::read(project_root.join(&plan.path)) {
                Ok(bytes) => crate::sync::hash::content_md5(&bytes),
                Err(error) => {
                    log::warn!(
                        "chapter sync projection target unreadable, keeping journal: {}: {error}",
                        plan.path
                    );
                    return Ok(false);
                }
            }
        } else {
            String::new()
        };
        records.push(crate::sync::local_mutations::LocalMutationRecord {
            schema_version: 1,
            transaction_id: manifest.transaction_id.clone(),
            sync_root: project_root.to_string_lossy().into_owned(),
            path: plan.path.clone(),
            object_type,
            object_id,
            op: plan.op.clone(),
            base_hash: None,
            content_hash,
            updated_at_ms: plan.updated_at_ms,
            deleted_at_ms: plan.deleted_at_ms,
            device_id: plan.device_id.clone(),
            object_revision: plan.object_revision.clone(),
            seq: 0,
        });
    }
    match crate::sync::local_mutations::append_batch(project_root, &records) {
        Ok(()) => {
            manifest.sync_projection_done = true;
            write_manifest(tx_dir, manifest)?;
            Ok(true)
        }
        Err(error) => {
            log::warn!(
                "chapter sync projection failed, keeping durable decision for replay: tx={}: {error}",
                manifest.transaction_id
            );
            Ok(false)
        }
    }
}

/// 重放所有事务目录里尚未 durable 投影的同步事实（同步入口在任何 fast path
/// 之前调用）。返回成功投影的事务数；仍有未完成事务时返回错误，调用方据此
/// 保持对应 target 为待处理（复审 #3、#4）。
pub fn replay_pending_sync_mutations(project_root: &Path) -> Result<usize> {
    let root = journal_root(project_root);
    match fs::metadata(&root) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to inspect chapter commit journal root {}: {error}",
                root.display()
            )))
        }
    }
    let mut replayed = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if entry.file_name().to_string_lossy() == CONFLICT_DIR {
            continue;
        }
        let tx_dir = entry.path();
        let manifest_path = tx_dir.join("manifest.json");
        let raw = match fs::read_to_string(&manifest_path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(Error::Other(format!(
                    "failed to read chapter commit manifest {}: {error}",
                    manifest_path.display()
                )))
            }
        };
        let mut manifest = match serde_json::from_str::<ChapterCommitManifest>(&raw) {
            Ok(manifest) => manifest,
            Err(error) => {
                // 决议读不出来就无法判定投影是否完成：保留目录并报错，
                // 不能当成本轮已收敛。
                failures.push(format!(
                    "{}: manifest unreadable: {error}",
                    tx_dir.display()
                ));
                continue;
            }
        };
        if manifest.sync_projection_done {
            // 投影已完成但目录还在：恢复责任可以结算，残留目录由后续清理处理。
            continue;
        }
        // 只重放已发布（Applied）的决议；Decided 还没碰 live，由章节恢复
        // 自己发布后再投影，不能提前把不存在的对象记成事实。
        if manifest.phase != ChapterCommitPhase::Applied {
            continue;
        }
        match project_manifest_sync_mutations(project_root, &tx_dir, &mut manifest)? {
            true => {
                replayed += 1;
            }
            false => failures.push(format!(
                "{}: sync projection not durable",
                manifest.transaction_id
            )),
        }
    }
    if failures.is_empty() {
        Ok(replayed)
    } else {
        Err(crate::error::Error::Other(format!(
            "chapter sync projection replay incomplete: {}",
            failures.join("; ")
        )))
    }
}

/// 按事务 ID 重放单笔 journal 的同步投影（journal 自身定位，不扫描全目录）。
///
/// 返回是否重放了投影；事务不存在/已完成时返回 `Ok(false)`（幂等）。
pub fn replay_pending_sync_mutation(project_root: &Path, transaction_id: &str) -> Result<bool> {
    if transaction_id.is_empty() || transaction_id.contains(['/', '\\']) {
        return Err(crate::error::Error::Other(format!(
            "invalid chapter commit transaction id: {transaction_id}"
        )));
    }
    let tx_dir = journal_root(project_root).join(transaction_id);
    let manifest_path = tx_dir.join("manifest.json");
    let raw = match fs::read_to_string(&manifest_path) {
        Ok(raw) => raw,
        // 意图已登记但 journal 没有落盘（写失败窗口）：清掉这一笔悬空责任。
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(false);
        }
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to read chapter commit manifest {}: {error}",
                manifest_path.display()
            )))
        }
    };
    let mut manifest: ChapterCommitManifest = serde_json::from_str(&raw)?;
    if manifest.sync_projection_done || manifest.phase != ChapterCommitPhase::Applied {
        return Ok(false);
    }
    let projected = project_manifest_sync_mutations(project_root, &tx_dir, &mut manifest)?;
    if projected {
    }
    Ok(projected)
}

pub fn recover_pending_chapter_commits(project_root: &Path) -> Result<Vec<ChapterCommitRecovery>> {
    let root = journal_root(project_root);
    match fs::metadata(&root) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to inspect chapter commit journal root {}: {error}",
                root.display()
            )))
        }
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
        let raw = match fs::read_to_string(&manifest_path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // 清单从未落盘：live 从未被触碰，删除自己的未发布材料。
                if let Err(error) = fs::remove_dir_all(&dir) {
                    log::warn!(
                        "failed to clean chapter commit directory without manifest {}: {error}",
                        dir.display()
                    );
                }
                continue;
            }
            Err(error) => {
                // 清单存在但不可读：保留事务目录并把真实错误交回调用方。
                return Err(Error::Other(format!(
                    "failed to read chapter commit manifest {}: {error}",
                    manifest_path.display()
                )));
            }
        };
        match serde_json::from_str::<ChapterCommitManifest>(&raw) {
            Ok(manifest) => entries.push((dir, manifest)),
            Err(error) => {
                // 损坏事务保留原目录并显式报告；不能静默当成已完成，也不
                // 阻断其它章节的恢复。
                log::error!(
                    "chapter commit journal manifest parse failed, transaction preserved: {}: {}",
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
                    let conflict_dir = match preserve_superseded_conflict(
                        project_root,
                        &tx_dir,
                        &manifest,
                    ) {
                        Ok(conflict_dir) => conflict_dir,
                        Err(error) => {
                            // 归档未 durable：保留 Decided/Superseded 事务，
                            // 下一次恢复继续补齐；不能用复制失败阻断无关章节。
                            log::error!(
                                "chapter conflict archive incomplete, journal retained: {}: {error}",
                                tx_dir.display()
                            );
                            continue;
                        }
                    };
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
                    if let Err(error) = remove_transaction_dir(&tx_dir) {
                        log::warn!(
                            "failed to clean finished chapter commit {}: {error}",
                            tx_dir.display()
                        );
                    }
                } else {
                    // Superseded 在发布失败时先落盘、归档随后才做；恢复时继续
                    // 补齐冲突材料（已存在的文件复用），失败则保留事务下次再试。
                    if let Err(error) =
                        preserve_superseded_conflict(project_root, &tx_dir, &manifest)
                    {
                        log::error!(
                            "superseded chapter conflict archive incomplete, journal retained: {}: {error}",
                            tx_dir.display()
                        );
                    }
                }
                continue;
            }
        }

        // 同步投影必须 durable 之后才允许 Finished/清理目录（复审 #1）。
        if !project_manifest_sync_mutations(project_root, &tx_dir, &mut manifest)? {
            recovered.push(ChapterCommitRecovery {
                transaction_id: manifest.transaction_id.clone(),
                key: manifest.key.clone(),
                after: manifest.after.clone(),
                pending_projections: manifest.pending_projections.clone(),
                draft_ack_projections: manifest.draft_ack_projections.clone(),
                superseded: false,
                conflict_dir: None,
            });
            continue;
        }
        if manifest.pending_projections.is_empty() && manifest.draft_ack_projections.is_empty() {
            manifest.phase = ChapterCommitPhase::Finished;
            write_manifest(&tx_dir, &manifest)?;
            if let Err(error) = remove_transaction_dir(&tx_dir) {
                log::warn!(
                    "failed to clean finished chapter commit {}: {error}",
                    tx_dir.display()
                );
            }
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
    match fs::metadata(&root) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to inspect chapter commit journal root {}: {error}",
                root.display()
            )))
        }
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
                if error.kind() == std::io::ErrorKind::NotFound {
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
