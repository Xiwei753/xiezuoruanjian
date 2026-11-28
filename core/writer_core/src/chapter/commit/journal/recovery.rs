use super::super::publish::{publish_decided, publish_deleted, sha256_bytes};
use super::*;

/// 已归档材料是否与 manifest 的确定清单逐项一致（按字节校验，不看存在性）。
///
/// `conflict.json` 或目标文件“存在”都不能证明归档完成：历史失败会留下只有
/// 回执、没有材料的旧目录。这里以 durable manifest（before/after 身份、正文与
/// 附加文件摘要）加真实字节摘要判定；身份不符、半套材料或旧回执一律按
/// “未完成”处理，由 [`preserve_superseded_conflict`] 覆盖重建。
fn archived_conflict_complete(
    conflict_dir: &Path,
    manifest: &ChapterCommitManifest,
) -> Result<bool> {
    let archived_manifest = match fs::read_to_string(conflict_dir.join("manifest.json")) {
        Ok(raw) => match serde_json::from_str::<ChapterCommitManifest>(&raw) {
            Ok(archived) => archived,
            // 归档清单损坏：不能作为完成证明，下面整份覆盖重建。
            Err(_) => return Ok(false),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if archived_manifest.transaction_id != manifest.transaction_id
        || archived_manifest.key != manifest.key
        || archived_manifest.before != manifest.before
        || archived_manifest.after != manifest.after
        || archived_manifest.phase != ChapterCommitPhase::Superseded
    {
        return Ok(false);
    }
    // 删除冲突没有待发布正文；正文提交冲突的正文材料必须与决议摘要一致。
    if !manifest.deleted
        && !file_digest_matches(&conflict_dir.join(BODY_FILE), &manifest.after.content_sha256)?
    {
        return Ok(false);
    }
    for plan in &manifest.additional_files {
        if !file_digest_matches(&conflict_dir.join(&plan.staging_filename), &plan.sha256)? {
            return Ok(false);
        }
    }
    let receipt = ChapterConflictReceipt::read(conflict_dir)?;
    Ok(receipt.is_some_and(|receipt| receipt.matches_manifest(manifest)))
}

/// 文件存在且摘要与决议一致；缺失返回 false，读取失败向上返回真实错误。
fn file_digest_matches(path: &Path, expected_sha256: &str) -> Result<bool> {
    match fs::read(path) {
        Ok(bytes) => Ok(sha256_bytes(&bytes) == expected_sha256),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// 保留 Superseded 事务的真实材料、pending 事件和冲突记录。
///
/// 幂等且自校验：归档目录里已有的旧 `conflict.json` / 半套材料不能当完成
/// 证明，必须按 manifest 的确定清单逐项核对字节摘要。缺材料时先把全部源材料
/// 读进内存并校验（源缺失/摘要不符时只复用已通过校验的归档副本；两者都不可信
/// 就整体失败，不写半套），再用 `atomic_write` 一次覆盖补齐；材料 durable 后
/// 才写回执（真实完成标记），最后再次整份校验，仍不完整就保留事务交下一次
/// 恢复重试。
pub(crate) fn preserve_superseded_conflict(
    project_root: &Path,
    tx_dir: &Path,
    manifest: &ChapterCommitManifest,
) -> Result<String> {
    let conflict_dir = conflict_dir_path(project_root, &manifest.transaction_id);
    fs::create_dir_all(&conflict_dir)?;
    if archived_conflict_complete(&conflict_dir, manifest)? {
        return Ok(conflict_dir.to_string_lossy().to_string());
    }

    let mut materials: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    let archived_body = conflict_dir.join(BODY_FILE);
    let mut body_ready = false;
    match fs::read(tx_dir.join(BODY_FILE)) {
        Ok(bytes) if sha256_bytes(&bytes) == manifest.after.content_sha256 => {
            materials.push((archived_body.clone(), bytes));
            body_ready = true;
        }
        Ok(_) => {
            // 事务源与决议摘要不符：不能用它覆盖归档，退回已校验的归档副本。
            log::warn!(
                "chapter conflict archive source body digest mismatch, keeping verified copy: tx={}",
                manifest.transaction_id
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    // 删除冲突没有待发布正文；正文提交冲突必须有可校验的正文材料。
    if !body_ready
        && !manifest.deleted
        && !file_digest_matches(&archived_body, &manifest.after.content_sha256)?
    {
        return Err(Error::Other(format!(
            "chapter conflict archive is missing body material: tx={}",
            manifest.transaction_id
        )));
    }
    for plan in &manifest.additional_files {
        let target = conflict_dir.join(&plan.staging_filename);
        let mut additional_ready = false;
        match fs::read(tx_dir.join(&plan.staging_filename)) {
            Ok(bytes) if sha256_bytes(&bytes) == plan.sha256 => {
                materials.push((target.clone(), bytes));
                additional_ready = true;
            }
            Ok(_) => {
                log::warn!(
                    "chapter conflict archive source digest mismatch, keeping verified copy: tx={}, file={}",
                    manifest.transaction_id,
                    plan.staging_filename
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if !additional_ready && !file_digest_matches(&target, &plan.sha256)? {
            return Err(Error::Other(format!(
                "chapter conflict archive is missing additional material: tx={}, file={}",
                manifest.transaction_id, plan.staging_filename
            )));
        }
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
    // 一次覆盖补齐清单里的全部确定材料。
    for (target, bytes) in &materials {
        crate::storage::atomic_write_bytes(target, bytes).map_err(|error| {
            Error::Other(format!(
                "failed to preserve chapter commit material {}: {error}",
                target.display()
            ))
        })?;
    }
    crate::storage::sync_dir(&conflict_dir)?;
    // 材料齐备之后才写回执；回执本身也按同一 durable manifest 生成。
    ChapterConflictReceipt::from_manifest(manifest).write(&conflict_dir)?;
    crate::storage::sync_dir(&conflict_dir)?;
    if !archived_conflict_complete(&conflict_dir, manifest)? {
        return Err(Error::Other(format!(
            "chapter conflict archive still incomplete after rewrite: tx={}",
            manifest.transaction_id
        )));
    }
    Ok(conflict_dir.to_string_lossy().to_string())
}

/// 列出当前项目内已发布正文、但仍有证明投影未确认的 journal。
pub struct PendingChapterProofScan {
    pub transactions: Vec<super::PendingChapterProofTransaction>,
    pub failures: Vec<String>,
}

pub fn pending_chapter_proof_transactions(
    project_root: &Path,
) -> Result<PendingChapterProofScan> {
    let root = journal_root(project_root);
    match fs::metadata(&root) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PendingChapterProofScan {
                transactions: Vec::new(),
                failures: Vec::new(),
            })
        }
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to inspect chapter commit journal root {}: {error}",
                root.display()
            )))
        }
    }
    let mut transactions = Vec::new();
    let mut failures = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        // 冲突归档目录不是未决事务，永不进入 pending/sequence 扫描。
        if entry.file_name().to_string_lossy() == CONFLICT_DIR {
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
                // 损坏清单保留在磁盘上供诊断；把真实失败带回恢复结果，
                // 但不把它当成可投影事实，也不阻断其它可读事务。
                log::error!(
                    "chapter commit manifest {} is corrupt, transaction kept and skipped: {error}",
                    manifest_path.display()
                );
                failures.push(format!(
                    "corrupt chapter commit manifest {}: {error}",
                    manifest_path.display()
                ));
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
    for (_, manifest) in chapter_transaction_manifests(project_root)? {
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
    Ok(PendingChapterProofScan {
        transactions: transactions.into_iter().map(|(_, _, item)| item).collect(),
        failures,
    })
}

/// 恢复冲突时补写仍可取得的三方正文材料（base 取本批首条证明的基线）。
fn write_recovered_conflict_materials(
    project_root: &Path,
    tx_dir: &Path,
    manifest: &ChapterCommitManifest,
    conflict_dir: &Path,
) -> Result<()> {
    let mut wrote = false;
    if let Some(base) = manifest
        .proof_events
        .first()
        .and_then(|projection| projection.baseline_text.as_deref())
    {
        crate::storage::atomic_write_bytes(&conflict_dir.join("base.md"), base.as_bytes())?;
        wrote = true;
    }
    if let Ok(local) = fs::read_to_string(manifest.key.body_path(project_root)) {
        crate::storage::atomic_write_bytes(&conflict_dir.join("local.md"), local.as_bytes())?;
        wrote = true;
    }
    if let Ok(incoming) = fs::read_to_string(tx_dir.join(BODY_FILE)) {
        crate::storage::atomic_write_bytes(&conflict_dir.join("incoming.md"), incoming.as_bytes())?;
        wrote = true;
    }
    if wrote {
        crate::storage::sync_dir(conflict_dir)?;
    }
    Ok(())
}

/// 读取本章事务目录中所有已发布、待证明的事务，按 commit_sequence 排序。
pub(crate) fn pending_chapter_proof_transactions_for_key(
    project_root: &Path,
    key: &ChapterKey,
) -> Result<Vec<super::PendingChapterProofTransaction>> {
    let dir = chapter_transactions_dir(project_root, key);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to read chapter transaction dir {}: {error}",
                dir.display()
            )))
        }
    };
    let mut transactions = Vec::new();
    for entry in entries {
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
        let manifest: ChapterCommitManifest = serde_json::from_str(&raw).map_err(|error| {
            Error::Other(format!(
                "chapter commit manifest {} is corrupt: {error}",
                manifest_path.display()
            ))
        })?;
        if manifest.key != *key
            || manifest.phase != ChapterCommitPhase::Applied
            || manifest.pending_projections.is_empty()
        {
            continue;
        }
        transactions.push(super::PendingChapterProofTransaction {
            transaction_id: manifest.transaction_id,
            key: manifest.key,
            commit_sequence: manifest.commit_sequence,
        });
    }
    transactions.sort_by_key(|transaction| transaction.commit_sequence);
    Ok(transactions)
}

/// 由本次事务找到其章，再返回该章从早到晚的未确认证明队列。
pub(crate) fn pending_chapter_proof_transactions_for_transaction(
    project_root: &Path,
    transaction_id: &str,
) -> Result<Vec<super::PendingChapterProofTransaction>> {
    let tx_dir = find_transaction_dir(project_root, transaction_id)?;
    let manifest_path = tx_dir.join("manifest.json");
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
    let manifest: ChapterCommitManifest = serde_json::from_str(&raw).map_err(|error| {
        Error::Other(format!(
            "chapter commit manifest {} is corrupt: {error}",
            manifest_path.display()
        ))
    })?;
    pending_chapter_proof_transactions_for_key(project_root, &manifest.key)
}

/// 读取 journal 中尚未确认的证明事件（供投影 worker 使用）。
pub fn pending_chapter_proof_events(
    project_root: &Path,
    transaction_id: &str,
) -> Result<Vec<super::ChapterProofProjection>> {
    let manifest_path = find_transaction_dir(project_root, transaction_id)?.join("manifest.json");
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
    let manifest_path = find_transaction_dir(project_root, transaction_id)?.join("manifest.json");
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
        // 冲突归档目录不是未决事务，永不进入 pending/sequence 扫描。
        if entry.file_name().to_string_lossy() == CONFLICT_DIR {
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
    for (_, manifest) in chapter_transaction_manifests(project_root)? {
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
    let tx_dir = find_transaction_dir(project_root, transaction_id)?;
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
    recover_pending_chapter_commits_filtered(project_root, None)
}

/// 读取/提交当前章时的恢复入口：只处理该章尚未完成的正文决议。
///
/// 已发布正文、只剩统计投影或 draft ack 的事务不参与逐章读写闸门；启动或
/// 显式恢复仍调用 [`recover_pending_chapter_commits`] 处理全部待办。
pub(crate) fn recover_pending_chapter_commits_for_key(
    project_root: &Path,
    key: &ChapterKey,
) -> Result<Vec<ChapterCommitRecovery>> {
    recover_pending_chapter_commits_filtered(project_root, Some(key))
}

fn recover_pending_chapter_commits_filtered(
    project_root: &Path,
    only_key: Option<&ChapterKey>,
) -> Result<Vec<ChapterCommitRecovery>> {
    if let Some(key) = only_key {
        return recover_chapter_transactions(project_root, key);
    }
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
    migrate_legacy_transactions(project_root)?;
    let mut entries = Vec::new();
    let chapters_root = root.join(CHAPTERS_DIR);
    if let Ok(volumes) = fs::read_dir(&chapters_root) {
        for volume in volumes {
            let volume = volume?;
            if !volume.file_type()?.is_dir() {
                continue;
            }
            for chapter in fs::read_dir(volume.path())? {
                let chapter = chapter?;
                if !chapter.file_type()?.is_dir() {
                    continue;
                }
                for transaction in fs::read_dir(chapter.path())? {
                    let transaction = transaction?;
                    if !transaction.file_type()?.is_dir() {
                        continue;
                    }
                    let dir = transaction.path();
                    let manifest_path = dir.join("manifest.json");
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
                    match serde_json::from_str::<ChapterCommitManifest>(&raw) {
                        Ok(manifest) => entries.push((dir, manifest)),
                        Err(error) => {
                            log::error!(
                                "chapter commit journal manifest parse failed, transaction preserved: {}: {}",
                                manifest_path.display(),
                                error
                            );
                        }
                    }
                }
            }
        }
    }
    recover_chapter_commit_entries(project_root, entries, true)
}

/// 读取每章事务目录中的 manifest（新布局）。
pub(super) fn chapter_transaction_manifests(
    project_root: &Path,
) -> Result<Vec<(PathBuf, ChapterCommitManifest)>> {
    let root = journal_root(project_root).join(CHAPTERS_DIR);
    let mut manifests = Vec::new();
    let volumes = match fs::read_dir(&root) {
        Ok(volumes) => volumes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(manifests),
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to read chapter transaction root {}: {error}",
                root.display()
            )))
        }
    };
    for volume in volumes {
        let volume = volume?;
        if !volume.file_type()?.is_dir() {
            continue;
        }
        for chapter in fs::read_dir(volume.path())? {
            let chapter = chapter?;
            if !chapter.file_type()?.is_dir() {
                continue;
            }
            for transaction in fs::read_dir(chapter.path())? {
                let transaction = transaction?;
                if !transaction.file_type()?.is_dir() {
                    continue;
                }
                let manifest_path = transaction.path().join("manifest.json");
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
                match serde_json::from_str::<ChapterCommitManifest>(&raw) {
                    Ok(manifest) => manifests.push((transaction.path(), manifest)),
                    Err(error) => {
                        log::error!(
                            "chapter commit manifest {} is corrupt, skipped: {error}",
                            manifest_path.display()
                        );
                    }
                }
            }
        }
    }
    Ok(manifests)
}

/// 启动恢复时把旧的扁平事务目录一次性迁入每章目录；旧布局只在这里出现。
fn migrate_legacy_transactions(project_root: &Path) -> Result<()> {
    let root = journal_root(project_root);
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to read chapter commit journal root {}: {error}",
                root.display()
            )))
        }
    };
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let dir = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == CHAPTERS_DIR || name == CONFLICT_DIR {
            continue;
        }
        let manifest_path = dir.join("manifest.json");
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
        let manifest: ChapterCommitManifest = match serde_json::from_str(&raw) {
            Ok(manifest) => manifest,
            Err(error) => {
                log::error!(
                    "legacy chapter commit manifest {} is corrupt, left in place: {error}",
                    manifest_path.display()
                );
                continue;
            }
        };
        let target = chapter_transaction_dir(project_root, &manifest.key, &manifest.transaction_id);
        if target.exists() {
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Err(error) = fs::rename(&dir, &target) {
            log::warn!(
                "failed to migrate legacy chapter commit {} -> {}: {error}",
                dir.display(),
                target.display()
            );
        }
    }
    Ok(())
}

/// 读取/提交当前章时只读该章事务目录，找到未发布的正文决议。
fn recover_chapter_transactions(
    project_root: &Path,
    key: &ChapterKey,
) -> Result<Vec<ChapterCommitRecovery>> {
    let dir = chapter_transactions_dir(project_root, key);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to read chapter transaction dir {}: {error}",
                dir.display()
            )))
        }
    };
    let mut transactions = Vec::new();
    for entry in entries {
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
        let manifest: ChapterCommitManifest = serde_json::from_str(&raw).map_err(|error| {
            Error::Other(format!(
                "chapter commit manifest {} is corrupt: {error}",
                manifest_path.display()
            ))
        })?;
        if manifest.key != *key {
            return Err(Error::Other(format!(
                "chapter transaction {} contains a different chapter key",
                entry.path().display()
            )));
        }
        if manifest.phase == ChapterCommitPhase::Decided {
            transactions.push((entry.path(), manifest));
        }
    }
    transactions.sort_by_key(|(_, manifest)| (manifest.commit_sequence, manifest.created_at_ms));
    recover_chapter_commit_entries(project_root, transactions, false)
}

fn recover_chapter_commit_entries(
    project_root: &Path,
    mut entries: Vec<(PathBuf, ChapterCommitManifest)>,
    clear_pending: bool,
) -> Result<Vec<ChapterCommitRecovery>> {
    entries.sort_by_key(|(_, manifest)| (manifest.commit_sequence, manifest.created_at_ms));

    let mut recovered = Vec::new();
    for (tx_dir, mut manifest) in entries {
        match manifest.phase {
            ChapterCommitPhase::Prepared => {
                // 未决议：live 从未被触碰。保留目录供人工诊断，不发布。
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
                    write_recovered_conflict_materials(
                        project_root,
                        &tx_dir,
                        &manifest,
                        Path::new(&conflict_dir),
                    )?;
                    recovered.push(ChapterCommitRecovery {
                        transaction_id: manifest.transaction_id.clone(),
                        key: manifest.key.clone(),
                        after: manifest.after.clone(),
                        pending_projections: manifest.pending_projections.clone(),
                        draft_ack_projections: manifest.draft_ack_projections.clone(),
                        superseded: true,
                        conflict_dir: Some(conflict_dir),
                    });
                    if let Err(error) = remove_transaction_dir(&tx_dir) {
                        log::warn!(
                            "failed to clean delivered superseded chapter commit {}: {error}",
                            tx_dir.display()
                        );
                    }
                    continue;
                }
            }
            ChapterCommitPhase::Applied => {
                // 正文已发布，只剩证明投影；事务目录由同章证明队列读取。
            }
            ChapterCommitPhase::Finished => {
                if let Err(error) = remove_transaction_dir(&tx_dir) {
                    log::warn!(
                        "failed to clean finished chapter commit {}: {error}",
                        tx_dir.display()
                    );
                }
                continue;
            }
            ChapterCommitPhase::Superseded => {
                // Superseded 可能只落盘了阶段、归档还没做完（崩溃在材料复制
                // 与 Superseded 写入之间，或旧版本只留下半套材料）。先按
                // manifest 补齐并自校验，再交付恢复回执——不能直接 continue，
                // 否则 Qt 拿不到这笔已放弃事务的真实 transaction_id/conflict_dir。
                match preserve_superseded_conflict(project_root, &tx_dir, &manifest) {
                    Ok(conflict_dir) => {
                        write_recovered_conflict_materials(
                            project_root,
                            &tx_dir,
                            &manifest,
                            Path::new(&conflict_dir),
                        )?;
                        recovered.push(ChapterCommitRecovery {
                            transaction_id: manifest.transaction_id.clone(),
                            key: manifest.key.clone(),
                            after: manifest.after.clone(),
                            pending_projections: manifest.pending_projections.clone(),
                            draft_ack_projections: manifest.draft_ack_projections.clone(),
                            superseded: true,
                            conflict_dir: Some(conflict_dir),
                        });
                        if let Err(error) = remove_transaction_dir(&tx_dir) {
                            log::warn!(
                                "failed to clean delivered superseded chapter commit {}: {error}",
                                tx_dir.display()
                            );
                        }
                    }
                    Err(error) => {
                        // 归档不完整：保留事务等下一次恢复，不交付指向残件的回执。
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
    if clear_pending {
        clear_pending_if_idle(project_root)?;
    }
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
    for (_, manifest) in chapter_transaction_manifests(project_root)? {
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
