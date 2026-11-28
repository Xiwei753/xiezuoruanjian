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
pub fn pending_chapter_proof_transactions(
    project_root: &Path,
) -> Result<Vec<super::PendingChapterProofTransaction>> {
    let mut transactions: Vec<_> = chapter_transaction_manifests(project_root)?
        .into_iter()
        .filter(|(_, manifest)| {
            manifest.phase == ChapterCommitPhase::Applied
                && !manifest.pending_projections.is_empty()
        })
        .map(|(_, manifest)| super::PendingChapterProofTransaction {
            transaction_id: manifest.transaction_id,
            key: manifest.key,
            commit_sequence: manifest.commit_sequence,
        })
        .collect();
    transactions.sort_by_key(|transaction| (transaction.key.chapter_id.clone(), transaction.commit_sequence));
    Ok(transactions)
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

/// 读取本章事务目录的全部 manifest（同章枚举的唯一入口）。
fn chapter_manifests_for_key(
    project_root: &Path,
    key: &ChapterKey,
) -> Result<Vec<(PathBuf, ChapterCommitManifest)>> {
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
    let mut manifests = Vec::new();
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
        manifests.push((entry.path(), manifest));
    }
    Ok(manifests)
}

/// 读取本章事务目录中所有已发布、待证明的事务，按 commit_sequence 排序。
pub(crate) fn pending_chapter_proof_transactions_for_key(
    project_root: &Path,
    key: &ChapterKey,
) -> Result<Vec<super::PendingChapterProofTransaction>> {
    let mut transactions: Vec<_> = chapter_manifests_for_key(project_root, key)?
        .into_iter()
        .filter(|(_, manifest)| {
            manifest.phase == ChapterCommitPhase::Applied
                && !manifest.pending_projections.is_empty()
        })
        .map(|(_, manifest)| super::PendingChapterProofTransaction {
            transaction_id: manifest.transaction_id,
            key: manifest.key,
            commit_sequence: manifest.commit_sequence,
        })
        .collect();
    transactions.sort_by_key(|transaction| transaction.commit_sequence);
    Ok(transactions)
}

/// 一次读取 manifest，同时取出本次事务仍待投影的证明事件与同步事实。
pub(crate) fn pending_chapter_proof_payload(
    project_root: &Path,
    key: &ChapterKey,
    transaction_id: &str,
) -> Result<Option<(Vec<super::ChapterProofProjection>, Vec<super::ChapterSyncProofFact>)>> {
    let manifest_path =
        chapter_transaction_dir(project_root, key, transaction_id).join("manifest.json");
    let raw = match fs::read_to_string(&manifest_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to read chapter commit manifest {}: {error}",
                manifest_path.display()
            )))
        }
    };
    let manifest: ChapterCommitManifest = serde_json::from_str(&raw)?;
    if manifest.phase != ChapterCommitPhase::Applied || manifest.pending_projections.is_empty() {
        return Ok(None);
    }
    let pending: std::collections::HashSet<String> =
        manifest.pending_projections.iter().cloned().collect();
    let events = manifest
        .proof_events
        .into_iter()
        .filter(|projection| pending.contains(projection.event_id()))
        .collect();
    let facts = manifest
        .sync_proof_facts
        .into_iter()
        .filter(|fact| pending.contains(fact.event_id.as_str()))
        .collect();
    Ok(Some((events, facts)))
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
    for (tx_dir, mut manifest) in chapter_transaction_manifests(project_root)? {
        if manifest.sync_projection_done || manifest.phase != ChapterCommitPhase::Applied {
            continue;
        }
        match project_manifest_sync_mutations(project_root, &tx_dir, &mut manifest)? {
            true => {
                replayed += 1;
                // 同步账本已 durable；没有其它待确认投影时按现有完成逻辑
                // 结算并删除空事务。
                if manifest.pending_projections.is_empty()
                    && manifest.draft_ack_projections.is_empty()
                {
                    manifest.phase = ChapterCommitPhase::Finished;
                    write_manifest(&tx_dir, &manifest)?;
                    if let Err(error) = remove_transaction_dir(
                        project_root,
                        &manifest.key,
                        &manifest.transaction_id,
                    ) {
                        log::warn!(
                            "failed to clean finished chapter commit {}: {error}",
                            tx_dir.display()
                        );
                    }
                }
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
    cleanup_manifestless_transactions(project_root)?;
    let entries = chapter_transaction_manifests(project_root)?;
    recover_chapter_commit_entries(project_root, entries)
}

/// 启动时清理没有 manifest 的事务残留（Prepared 之前退出）。
fn cleanup_manifestless_transactions(project_root: &Path) -> Result<()> {
    let root = journal_root(project_root).join(CHAPTERS_DIR);
    let volumes = match fs::read_dir(&root) {
        Ok(volumes) => volumes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
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
                if !transaction.path().join("manifest.json").exists() {
                    fs::remove_dir_all(transaction.path())?;
                }
            }
        }
    }
    Ok(())
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
                let manifest: ChapterCommitManifest =
                    serde_json::from_str(&raw).map_err(|error| {
                        Error::Other(format!(
                            "chapter commit manifest {} is corrupt: {error}",
                            manifest_path.display()
                        ))
                    })?;
                manifests.push((transaction.path(), manifest));
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
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::remove_dir_all(&dir)?;
                continue;
            }
            Err(error) => {
                return Err(Error::Other(format!(
                    "failed to read chapter commit manifest {}: {error}",
                    manifest_path.display()
                )))
            }
        };
        let manifest: ChapterCommitManifest = serde_json::from_str(&raw).map_err(|error| {
            Error::Other(format!(
                "legacy chapter commit manifest {} is corrupt: {error}",
                manifest_path.display()
            ))
        })?;
        let target = chapter_transaction_dir(project_root, &manifest.key, &manifest.transaction_id);
        if target.exists() {
            return Err(Error::Other(format!(
                "legacy chapter commit {} conflicts with existing transaction {}",
                dir.display(),
                target.display()
            )));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&dir, &target).map_err(|error| {
            Error::Other(format!(
                "failed to migrate legacy chapter commit {} -> {}: {error}",
                dir.display(),
                target.display()
            ))
        })?;
    }
    let legacy_locator = root.join("chapter-pending");
    if legacy_locator.exists() {
        fs::remove_dir_all(&legacy_locator)?;
    }
    Ok(())
}

/// 读取/提交当前章时只读该章事务目录，找到未发布的正文决议。
fn recover_chapter_transactions(
    project_root: &Path,
    key: &ChapterKey,
) -> Result<Vec<ChapterCommitRecovery>> {
    let mut transactions: Vec<_> = chapter_manifests_for_key(project_root, key)?
        .into_iter()
        .filter(|(_, manifest)| manifest.phase == ChapterCommitPhase::Decided)
        .collect();
    transactions.sort_by_key(|(_, manifest)| (manifest.commit_sequence, manifest.created_at_ms));
    recover_chapter_commit_entries(project_root, transactions)
}

fn recover_chapter_commit_entries(
    project_root: &Path,
    mut entries: Vec<(PathBuf, ChapterCommitManifest)>,
) -> Result<Vec<ChapterCommitRecovery>> {
    entries.sort_by_key(|(_, manifest)| (manifest.commit_sequence, manifest.created_at_ms));

    let mut recovered = Vec::new();
    for (tx_dir, mut manifest) in entries {
        match manifest.phase {
            ChapterCommitPhase::Prepared => {
                // 未决议：live 从未被触碰；清理残留目录，不发布。
                if let Err(error) = remove_transaction_dir(
                    project_root,
                    &manifest.key,
                    &manifest.transaction_id,
                ) {
                    log::warn!("failed to clean prepared chapter commit: {error}");
                }
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
                    if let Err(error) = remove_transaction_dir(project_root, &manifest.key, &manifest.transaction_id) {
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
                if let Err(error) = remove_transaction_dir(project_root, &manifest.key, &manifest.transaction_id) {
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
                        if let Err(error) = remove_transaction_dir(project_root, &manifest.key, &manifest.transaction_id) {
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
            if let Err(error) = remove_transaction_dir(project_root, &manifest.key, &manifest.transaction_id) {
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
