use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::model::*;

/// 扫描事务目录，恢复未完成的事务，并保留所有无法判定的事务错误。
///
/// 与 [`recover_pending_transactions`] 相比，本函数不会吞掉 manifest 读/解析
/// 失败：调用方必须知道“事务状态未知”，不能在未知时清理事务材料。
/// 每笔错误都绑定到具体事务（[`TransactionRecoveryError`]），调用方不能靠
/// 字符串猜测错误属于哪笔事务。
pub(crate) fn scan_pending_transactions(target_root: &Path) -> TransactionRecoveryScan {
    let tx_base = target_root.join(TRANSACTIONS_DIR);
    let mut scan = TransactionRecoveryScan::default();
    match fs::metadata(&tx_base) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return scan,
        Err(error) => {
            scan.errors.push(TransactionRecoveryError {
                transaction_id: None,
                message: format!(
                    "inspect transaction dir {} failed: {error}",
                    tx_base.display()
                ),
            });
            return scan;
        }
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

/// 预检后的单条目状态：`After` 表示文件已经落盘，只需 ack；
/// `Before` 表示仍是事务准备时的内容，需要安全重放 staging/delete。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryState {
    Before,
    After,
}

/// 一个文件条目的预检结果。只有全部条目都通过预检（当前内容等于
/// before/after、staging 可读且摘要匹配、整章 body/meta 相互一致）后才允许
/// 写任何一个文件；不能一边冲突一边把已通过的条目先回写。
struct PreparedEntry<'a> {
    entry: &'a TransactionEntry,
    state: EntryState,
    current: Option<TransactionFileSnapshot>,
    staged: Option<TransactionFileSnapshot>,
}

#[derive(Debug)]
struct RecoveryConflict {
    message: String,
    affected_paths: Vec<String>,
}

impl RecoveryConflict {
    fn new(message: impl Into<String>, affected_paths: Vec<String>) -> Self {
        Self {
            message: message.into(),
            affected_paths,
        }
    }
}

#[derive(Default)]
struct PreparedFilesOutcome {
    recovered_files: Vec<String>,
    missing_files: Vec<String>,
}

#[allow(clippy::too_many_lines)]
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

    // 清单是否存在必须由真实读取判定：NotFound 才是“从未决议”，其它
    // 读取错误代表事务状态未知，保留目录并记录错误。
    let manifest = match fs::read_to_string(&manifest_path) {
        Ok(raw) => match serde_json::from_str::<TransactionManifest>(&raw) {
            Ok(manifest) => Some(manifest),
            Err(error) => {
                let message = format!(
                    "[transaction] recover: manifest parse failed for tx_dir={}: {error} \
                     — preserving tx_dir, will retry next startup",
                    tx_dir.display()
                );
                log::warn!("{message}");
                record_scan_error(scan, dir_transaction_id, message);
                return;
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            let message = format!(
                "[transaction] recover: manifest read failed for tx_dir={}: {error} \
                 — preserving tx_dir, will retry next startup",
                tx_dir.display()
            );
            log::warn!("{message}");
            record_scan_error(scan, dir_transaction_id, message);
            return;
        }
    };
    let Some(manifest) = manifest else {
        // 清单从未落盘：事务未决议，live 从未被修改，目录没有恢复材料。
        if let Err(error) = fs::remove_dir_all(&tx_dir) {
            log::warn!(
                "failed to clean undecided transaction dir {}: {error}",
                tx_dir.display()
            );
        }
        return;
    };

    match manifest.phase {
        TransactionPhase::RolledBack => {
            if let Err(error) = fs::remove_dir_all(&tx_dir) {
                log::warn!(
                    "failed to clean rolled-back transaction dir {}: {error}",
                    tx_dir.display()
                );
            }
            return;
        }
        TransactionPhase::FilesCommitted | TransactionPhase::Finished => {
            // 正文已提交，事务目录没有其它需要保留的恢复材料。
            if let Err(error) = fs::remove_dir_all(&tx_dir) {
                log::warn!(
                    "failed to clean committed transaction dir {}: {error}",
                    tx_dir.display()
                );
            }
            return;
        }
        TransactionPhase::Prepared => {
            // Prepared 阶段：先完整预检，再重放 rename/delete。
        }
    }

    let outcome = match recover_prepared_files(target_root, &tx_dir, &manifest) {
        Ok(outcome) => outcome,
        Err(conflict) => {
            // 预检失败：不改任何 live 文件，保留 manifest/staging/backup，
            // 把具体冲突绑定到事务并报告给调用方。
            log::warn!(
                "[transaction] recover: tx={} refused partial replay, preserving materials: {}",
                manifest.transaction_id,
                conflict.message
            );
            record_scan_error(
                scan,
                Some(manifest.transaction_id.clone()),
                format!(
                    "[transaction] recover: tx={} conflict, no files replayed: {}",
                    manifest.transaction_id, conflict.message
                ),
            );
            scan.transactions.push(TransactionRecovery {
                transaction_id: manifest.transaction_id,
                recovered_files: Vec::new(),
                missing_files: conflict.affected_paths,
                phase: TransactionPhase::Prepared,
                tx_dir,
                target_root: target_root.to_path_buf(),
            });
            return;
        }
    };

    let recovered_files = outcome.recovered_files;
    let missing_files = outcome.missing_files;
    if recovered_files.is_empty() && !missing_files.is_empty() {
        log::warn!(
            "[transaction] all staging files lost for tx={}, preserving materials",
            manifest.transaction_id
        );
    }

    // 正文事务不再链接证明意图：全部条目重放成功即完成，事务目录可清理；
    // 只要有条目重放失败，staging/manifest 是唯一恢复依据，必须保留重试。
    if missing_files.is_empty() {
        // 全部条目重放成功，事务可判定完成，manifest/staging 不再需要。
        if let Err(error) = fs::remove_dir_all(&tx_dir) {
            log::warn!(
                "failed to clean replayed transaction dir {}: {error}",
                tx_dir.display()
            );
        }
    } else {
        log::warn!(
            "[transaction] recover: tx={} has {} unreplayed file(s), \
             preserving Prepared manifest and staging for retry",
            manifest.transaction_id,
            missing_files.len()
        );
    }

    scan.transactions.push(TransactionRecovery {
        transaction_id: manifest.transaction_id,
        recovered_files,
        missing_files,
        phase: TransactionPhase::Prepared,
        tx_dir,
        target_root: target_root.to_path_buf(),
    });
}

fn conflict(reason: impl Into<String>, path: &str) -> RecoveryConflict {
    RecoveryConflict::new(reason, vec![path.to_string()])
}

fn entry_state(
    entry: &TransactionEntry,
    current: Option<&TransactionFileSnapshot>,
) -> Result<EntryState, RecoveryConflict> {
    let current_md5 = current.map(|snapshot| snapshot.md5.as_str());
    let before = entry.expected_before_hash.as_deref();
    let after = entry.expected_after_hash.as_deref();
    match (current_md5, before, after) {
        (Some(current), _, Some(after)) if current == after => Ok(EntryState::After),
        (Some(current), Some(before), _) if current == before => Ok(EntryState::Before),
        (None, None, Some(_)) if !entry.is_delete => Ok(EntryState::Before),
        (None, None, None) if entry.is_delete => Ok(EntryState::After),
        (None, Some(_), None) if entry.is_delete => Ok(EntryState::After),
        (Some(current), None, None) => Err(conflict(
            format!(
                "legacy manifest without before/after hash, target={} current_md5={current}",
                entry.target_relative
            ),
            &entry.target_relative,
        )),
        (None, None, _) => Err(conflict(
            format!(
                "legacy manifest without before/after hash, target={} missing",
                entry.target_relative
            ),
            &entry.target_relative,
        )),
        (Some(current), before, after) => Err(conflict(
            format!(
                "target changed since transaction prepared, target={} current_md5={current} \
                 before={before:?} after={after:?}",
                entry.target_relative
            ),
            &entry.target_relative,
        )),
        (None, before, after) => Err(conflict(
            format!(
                "target missing, target={} before={before:?} after={after:?}",
                entry.target_relative
            ),
            &entry.target_relative,
        )),
    }
}

fn plan_prepared_entry<'a>(
    target_root: &Path,
    tx_dir: &Path,
    entry: &'a TransactionEntry,
    tombstones: &TransactionTombstoneFacts,
) -> Result<PreparedEntry<'a>, RecoveryConflict> {
    let target_path = target_root.join(&entry.target_relative);
    let parse_metadata_json =
        normalized_transaction_relative(&entry.target_relative).ends_with("chapter.meta.json");
    let current =
        read_transaction_file_snapshot(&target_path, parse_metadata_json).map_err(|error| {
            conflict(
                format!("read target failed for {}: {error}", entry.target_relative),
                &entry.target_relative,
            )
        })?;
    let state = entry_state(entry, current.as_ref())?;

    let staged = if state == EntryState::Before && !entry.is_delete {
        if entry.staging_filename.is_empty() {
            return Err(conflict(
                format!(
                    "staging filename empty for target={}",
                    entry.target_relative
                ),
                &entry.target_relative,
            ));
        }
        let staging_path = tx_dir.join(&entry.staging_filename);
        let staged = read_transaction_file_snapshot(&staging_path, parse_metadata_json).map_err(
            |error| {
                conflict(
                    format!(
                        "read staging failed for target={} staging={}: {error}",
                        entry.target_relative,
                        staging_path.display()
                    ),
                    &entry.target_relative,
                )
            },
        )?;
        let Some(staged) = staged else {
            return Err(conflict(
                format!(
                    "staging missing for target={} staging={}",
                    entry.target_relative,
                    staging_path.display()
                ),
                &entry.target_relative,
            ));
        };
        if let Some(expected_after) = entry.expected_after_hash.as_deref() {
            if staged.md5 != expected_after {
                return Err(conflict(
                    format!(
                        "staging content changed, target={} staging_md5={} expected_after={expected_after}",
                        entry.target_relative, staged.md5
                    ),
                    &entry.target_relative,
                ));
            }
        }
        Some(staged)
    } else {
        None
    };

    // tombstone/删除事实只用于“准备重新创建”：目标已不存在时，staging 可能
    // 把已删除对象复活。删除仍存在的对象由 before/after 哈希与整章
    // generation 校验证明它仍是事务准备时的同一对象，不需要 tombstone。
    if state == EntryState::Before && !entry.is_delete && current.is_none() {
        if let Some(reason) = tombstones.blocks(&entry.target_relative) {
            return Err(conflict(
                format!(
                    "refusing to recreate target={} from stale staging: {reason}",
                    entry.target_relative
                ),
                &entry.target_relative,
            ));
        }
    }

    Ok(PreparedEntry {
        entry,
        state,
        current,
        staged,
    })
}

fn recover_prepared_files(
    target_root: &Path,
    tx_dir: &Path,
    manifest: &TransactionManifest,
) -> Result<PreparedFilesOutcome, RecoveryConflict> {
    let tombstones = TransactionTombstoneFacts::load(target_root, manifest.created_at_ms);
    let mut plans = Vec::with_capacity(manifest.entries.len());
    for entry in &manifest.entries {
        plans.push(plan_prepared_entry(
            target_root,
            tx_dir,
            entry,
            &tombstones,
        )?);
    }

    // 整章一致性校验必须先于任何写盘：正文和 meta 只能一起判定，不能先
    // 把正文 rename 成功后再发现 meta 冲突。
    validate_chapter_entries(target_root, tx_dir, manifest, &plans)?;

    let mut outcome = PreparedFilesOutcome::default();
    for plan in &plans {
        let target_path = target_root.join(&plan.entry.target_relative);
        match plan.state {
            EntryState::After => acknowledge_published(&target_path, plan.entry, &mut outcome),
            EntryState::Before if plan.entry.is_delete => {
                apply_prepared_delete(&target_path, plan.entry, &mut outcome);
            }
            EntryState::Before => {
                apply_prepared_rename(tx_dir, &target_path, plan.entry, &mut outcome);
            }
        }
    }
    Ok(outcome)
}

fn mark_recovered(outcome: &mut PreparedFilesOutcome, relative: &str) {
    outcome.recovered_files.push(relative.to_string());
}

fn mark_missing(outcome: &mut PreparedFilesOutcome, relative: &str) {
    outcome.missing_files.push(relative.to_string());
}

fn finish_durable_write(
    target_path: &Path,
    relative: &str,
    action: &str,
    outcome: &mut PreparedFilesOutcome,
) {
    match crate::storage::sync_parent(target_path) {
        Ok(()) => mark_recovered(outcome, relative),
        Err(error) => {
            log::warn!(
                "[transaction] recovery {action} sync_parent failed for {relative}: {error}"
            );
            mark_missing(outcome, relative);
        }
    }
}

fn acknowledge_published(
    target_path: &Path,
    entry: &TransactionEntry,
    outcome: &mut PreparedFilesOutcome,
) {
    finish_durable_write(target_path, &entry.target_relative, "ack", outcome);
}

fn apply_prepared_delete(
    target_path: &Path,
    entry: &TransactionEntry,
    outcome: &mut PreparedFilesOutcome,
) {
    match fs::remove_file(target_path) {
        Ok(()) => finish_durable_write(target_path, &entry.target_relative, "delete", outcome),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            mark_recovered(outcome, &entry.target_relative);
        }
        Err(error) => {
            log::warn!(
                "[transaction] recovery delete failed: {}: {error}",
                entry.target_relative
            );
            mark_missing(outcome, &entry.target_relative);
        }
    }
}

fn apply_prepared_rename(
    tx_dir: &Path,
    target_path: &Path,
    entry: &TransactionEntry,
    outcome: &mut PreparedFilesOutcome,
) {
    if let Some(parent) = target_path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            log::warn!(
                "[transaction] recovery create dir failed: {}: {error}",
                parent.display()
            );
            mark_missing(outcome, &entry.target_relative);
            return;
        }
    }
    let staging_path = tx_dir.join(&entry.staging_filename);
    match fs::rename(&staging_path, target_path) {
        Ok(()) => finish_durable_write(target_path, &entry.target_relative, "rename", outcome),
        Err(error) => {
            log::warn!(
                "[transaction] recovery rename failed: {} -> {}: {error}",
                entry.staging_filename,
                entry.target_relative
            );
            mark_missing(outcome, &entry.target_relative);
        }
    }
}

#[derive(Default)]
struct ChapterPlanGroup {
    meta: Option<usize>,
    body: Option<usize>,
}

fn chapter_member(directory: &str, name: &str) -> String {
    if directory.is_empty() {
        name.to_string()
    } else {
        format!("{directory}/{name}")
    }
}

fn validate_chapter_entries(
    target_root: &Path,
    tx_dir: &Path,
    manifest: &TransactionManifest,
    plans: &[PreparedEntry<'_>],
) -> Result<(), RecoveryConflict> {
    let mut groups: HashMap<String, ChapterPlanGroup> = HashMap::new();
    for (index, plan) in plans.iter().enumerate() {
        let target = normalized_transaction_relative(&plan.entry.target_relative);
        if let Some(directory) = target.strip_suffix("chapter.meta.json") {
            groups
                .entry(directory.trim_end_matches('/').to_string())
                .or_default()
                .meta = Some(index);
        } else if let Some(directory) = target.strip_suffix("chapter.md") {
            groups
                .entry(directory.trim_end_matches('/').to_string())
                .or_default()
                .body = Some(index);
        }
    }
    for (directory, group) in groups {
        validate_chapter_group(target_root, tx_dir, manifest, plans, &directory, &group)?;
    }
    Ok(())
}

fn read_group_snapshot(
    path: &Path,
    relative: &str,
) -> Result<Option<TransactionFileSnapshot>, RecoveryConflict> {
    let parse_metadata_json =
        normalized_transaction_relative(relative).ends_with("chapter.meta.json");
    read_transaction_file_snapshot(path, parse_metadata_json).map_err(|error| {
        conflict(
            format!("read chapter member {relative} failed: {error}"),
            relative,
        )
    })
}

fn backup_snapshot(
    tx_dir: &Path,
    manifest: &TransactionManifest,
    relative: &str,
) -> Result<Option<TransactionFileSnapshot>, RecoveryConflict> {
    for backup in &manifest.backup_entries {
        let target = match backup {
            BackupEntry::RestoreFile {
                target_relative, ..
            }
            | BackupEntry::RemoveCreated { target_relative } => {
                normalized_transaction_relative(target_relative)
            }
        };
        if target != normalized_transaction_relative(relative) {
            continue;
        }
        return match backup {
            BackupEntry::RestoreFile {
                backup_filename, ..
            } => {
                let path = tx_dir.join("backup").join(backup_filename);
                let parse_metadata_json =
                    normalized_transaction_relative(relative).ends_with("chapter.meta.json");
                match read_transaction_file_snapshot(&path, parse_metadata_json) {
                    Ok(Some(snapshot)) => Ok(Some(snapshot)),
                    Ok(None) => Err(conflict(
                        format!("backup material missing for {relative}: {}", path.display()),
                        relative,
                    )),
                    Err(error) => Err(conflict(
                        format!(
                            "read backup material failed for {relative} at {}: {error}",
                            path.display()
                        ),
                        relative,
                    )),
                }
            }
            BackupEntry::RemoveCreated { .. } => Ok(None),
        };
    }
    Ok(None)
}

fn after_snapshot(plan: Option<&PreparedEntry<'_>>) -> Option<TransactionFileSnapshot> {
    let plan = plan?;
    if plan.entry.is_delete {
        return None;
    }
    match plan.state {
        EntryState::After => plan.current.clone(),
        EntryState::Before => plan.staged.clone(),
    }
}

fn validate_chapter_group(
    target_root: &Path,
    tx_dir: &Path,
    manifest: &TransactionManifest,
    plans: &[PreparedEntry<'_>],
    directory: &str,
    group: &ChapterPlanGroup,
) -> Result<(), RecoveryConflict> {
    let meta_relative = chapter_member(directory, "chapter.meta.json");
    let body_relative = chapter_member(directory, "chapter.md");
    let meta_plan = group.meta.map(|index| &plans[index]);
    let body_plan = group.body.map(|index| &plans[index]);

    // 当前 meta：事务 meta 条目优先，否则读取同目录 live meta（正文单独
    // 提交的旧事务也要按章节身份校验）。
    let live_meta = match meta_plan {
        Some(plan) => plan.current.clone(),
        None => read_group_snapshot(&target_root.join(&meta_relative), &meta_relative)?,
    };
    let live_body = match body_plan {
        Some(plan) => plan.current.clone(),
        None => read_group_snapshot(&target_root.join(&body_relative), &body_relative)?,
    };
    let before_meta = backup_snapshot(tx_dir, manifest, &meta_relative)?;
    // 同时确认正文备份材料存在/可读；正文的 before 内容由 entry 哈希覆盖，
    // 这里不再单独比较。
    let _before_body = backup_snapshot(tx_dir, manifest, &body_relative)?;

    let after_meta = match meta_plan {
        Some(plan) => after_snapshot(Some(plan)),
        None => live_meta.clone(),
    };
    let after_body = match body_plan {
        Some(plan) => after_snapshot(Some(plan)),
        None => live_body.clone(),
    };

    let live_meta_facts = transaction_metadata_facts(
        live_meta
            .as_ref()
            .and_then(|snapshot| snapshot.json.as_ref()),
    );
    let before_meta_facts = transaction_metadata_facts(
        before_meta
            .as_ref()
            .and_then(|snapshot| snapshot.json.as_ref()),
    );
    let after_meta_facts = transaction_metadata_facts(
        after_meta
            .as_ref()
            .and_then(|snapshot| snapshot.json.as_ref()),
    );
    validate_chapter_identity(
        directory,
        &meta_relative,
        &body_relative,
        &live_meta_facts,
        &before_meta_facts,
        &after_meta_facts,
        meta_plan.is_some_and(|plan| plan.state == EntryState::After),
    )?;
    validate_chapter_result(
        directory,
        &meta_relative,
        &body_relative,
        after_meta.as_ref(),
        after_body.as_ref(),
        &after_meta_facts,
        meta_plan,
        body_plan,
    )
}

fn validate_chapter_identity(
    directory: &str,
    meta_relative: &str,
    body_relative: &str,
    live: &TransactionMetadataFacts,
    before: &TransactionMetadataFacts,
    after: &TransactionMetadataFacts,
    meta_already_after: bool,
) -> Result<(), RecoveryConflict> {
    // generation：live 已不是备份基线的对象身份时，只有 meta 已经应用且
    // stage 目标就是 live generation，才允许继续补写关联正文；否则说明对象
    // 被删除重建，旧事务材料不能回写。
    if let (Some(live_generation), Some(before_generation)) =
        (live.generation.as_deref(), before.generation.as_deref())
    {
        let same_object = live_generation == before_generation;
        let staged_object = after.generation.as_deref() == Some(live_generation);
        if !same_object && !(meta_already_after && staged_object) {
            return Err(RecoveryConflict::new(
                format!(
                    "chapter {directory} generation changed after staging                      (live={live_generation}, before={before_generation}, after={:?});                      refusing to publish old body/meta",
                    after.generation
                ),
                vec![meta_relative.to_string(), body_relative.to_string()],
            ));
        }
    }

    // revision：live 已经前进到 stage 之后，不能再用旧 staging 覆盖。
    if let (Some(live_revision), Some(before_revision)) = (live.revision, before.revision) {
        let staged_revision = after.revision == Some(live_revision);
        if live_revision > before_revision && !(meta_already_after && staged_revision) {
            return Err(RecoveryConflict::new(
                format!(
                    "chapter {directory} live revision {live_revision} advanced past                      transaction base {before_revision} (after={:?})",
                    after.revision
                ),
                vec![meta_relative.to_string(), body_relative.to_string()],
            ));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_chapter_result(
    directory: &str,
    meta_relative: &str,
    body_relative: &str,
    after_meta: Option<&TransactionFileSnapshot>,
    after_body: Option<&TransactionFileSnapshot>,
    after_meta_facts: &TransactionMetadataFacts,
    meta_plan: Option<&PreparedEntry<'_>>,
    body_plan: Option<&PreparedEntry<'_>>,
) -> Result<(), RecoveryConflict> {
    // 整章结果一致性：stage 的 meta 声明的正文摘要必须等于将要落盘的正文。
    if let Some(content_sha256) = after_meta_facts.content_sha256.as_deref() {
        match after_body {
            Some(body) if body.sha256 == content_sha256 => {}
            Some(body) => {
                return Err(RecoveryConflict::new(
                    format!(
                        "chapter {directory} staged meta content_sha256={content_sha256}                          does not match body sha256={} (body={body_relative}, meta={meta_relative})",
                        body.sha256
                    ),
                    vec![meta_relative.to_string(), body_relative.to_string()],
                ));
            }
            None if after_meta.is_some() => {
                return Err(RecoveryConflict::new(
                    format!(
                        "chapter {directory} staged meta claims content_sha256={content_sha256}                          but body {body_relative} is missing"
                    ),
                    vec![meta_relative.to_string(), body_relative.to_string()],
                ));
            }
            None => {}
        }
    }

    // 删除必须整章一致：只有 meta 删除或只有正文删除都会留下半个对象。
    let meta_delete = meta_plan.is_some_and(|plan| plan.entry.is_delete);
    let body_delete = body_plan.is_some_and(|plan| plan.entry.is_delete);
    if meta_plan.is_some() && body_plan.is_some() && meta_delete != body_delete {
        return Err(RecoveryConflict::new(
            format!(
                "chapter {directory} meta_delete={meta_delete} body_delete={body_delete};                  refusing inconsistent delete/upsert"
            ),
            vec![meta_relative.to_string(), body_relative.to_string()],
        ));
    }
    Ok(())
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
