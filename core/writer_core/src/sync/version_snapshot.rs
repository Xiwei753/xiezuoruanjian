//! 对象级本地版本快照 — 空闲同步"不变化"判定的唯一事实来源。
//!
//! 快照只由两类持久化事实构成：
//!
//! 1. target 的 `manifest.sync.json`：上一次成功发布后被共同基线确认的对象版本；
//! 2. `local-mutations.jsonl`：已持久提交、但尚未被共同基线确认的本地对象变更。
//!
//! 运行状态（`state.local.json` 的 `last_sync_time` / `last_error`、
//! `full_state.local.json` 的尝试时间、账本自身的行序）不属于业务版本，绝不进入
//! 指纹。否则一次 `NoChanges` 落盘的 `last_sync_time` 就会让下一轮认为"本地变了"，
//! 从而在空闲时反复进入全树扫描、staging 和发布。
//!
//! 快照不扫描磁盘：业务版本由已提交事务驱动（见 Issue #1065 架构约束
//! 「事务先于落盘，落盘先于同步」）。绕过 Core 事务的外部文件修改必须走显式
//! 校验/导入（`SyncRunMode::RebuildIndex`）建立真实基线，不能被增量路径静默吞掉。

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::sync::local_mutations;
use crate::sync::types::{AppDataGroup, SyncManifest, SyncScope, SyncTarget, TargetSyncResult};

/// 单个 target 的对象版本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalTargetVersion {
    /// 业务对象版本指纹：所有已知对象的 (path, op, content_hash)。
    pub object_fingerprint: String,
    /// 未确认本地变更的稳定代次。
    pub pending_fingerprint: String,
    /// 未确认本地变更的对象数。
    pub pending_objects: usize,
    /// 该 target 是否能走增量/无变化快速路径。
    pub eligible: bool,
    /// 不能走快速路径的原因（诊断字段，不参与比较）。
    pub ineligible_reason: Option<String>,
}

impl LocalTargetVersion {
    fn ineligible(reason: &str) -> Self {
        Self {
            object_fingerprint: String::new(),
            pending_fingerprint: String::new(),
            pending_objects: 0,
            eligible: false,
            ineligible_reason: Some(reason.to_string()),
        }
    }

    /// 两个快照是否代表同一批业务对象版本。
    ///
    /// 只有两端都 eligible 时才可能相等；任何不确定性都必须回到完整 LWW。
    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.eligible
            && other.eligible
            && self.object_fingerprint == other.object_fingerprint
            && self.pending_fingerprint == other.pending_fingerprint
    }
}

/// 一轮同步开始前的本地版本视图。
#[derive(Debug, Clone)]
pub(crate) struct LocalVersionSnapshot {
    pub targets: BTreeMap<String, LocalTargetVersion>,
    /// checkpoint 命中时可以原样回填的 no-change target 结果。
    pub no_change_targets: Vec<TargetSyncResult>,
    /// 存在待完成的删除/远端清理事务。此时不能走"无变化快速路径"：
    /// 这些收口动作不在任何 target 的对象版本里，跳过它们会丢掉远端删除。
    pub lifecycle_pending: bool,
}

pub(crate) fn app_target_key(group: &AppDataGroup) -> String {
    format!("app:{}", group.id())
}

pub(crate) fn project_target_key(project_id: &str) -> String {
    format!("project:{project_id}")
}

fn fingerprint_objects(
    manifest: &SyncManifest,
    pending: &BTreeMap<String, local_mutations::LocalMutationRecord>,
    group: Option<&AppDataGroup>,
) -> String {
    let mut merged: BTreeMap<&str, (String, String)> = BTreeMap::new();
    for record in &manifest.files {
        if group.is_some_and(|group| !group.owns_path(&record.path)) {
            continue;
        }
        merged.insert(
            record.path.as_str(),
            (record.op.clone(), record.content_hash.clone()),
        );
    }
    for (path, record) in pending {
        merged.insert(
            path.as_str(),
            (record.op.clone(), record.content_hash.clone()),
        );
    }
    let mut hasher = Sha256::new();
    for (path, (op, hash)) in merged {
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(op.as_bytes());
        hasher.update(b"\0");
        hasher.update(hash.as_bytes());
        hasher.update(b"\0");
    }
    format!("{:x}", hasher.finalize())
}

fn read_manifest(
    sync_root: &std::path::Path,
    relative: &str,
) -> crate::Result<Option<SyncManifest>> {
    // 权威清单是 v2 存储（root + 两级索引）；root 不存在才回退旧单文件（只读）。
    let store = crate::sync::lww::ManifestStore::new(sync_root, relative);
    if let Some(manifest) = store.load_all()? {
        return Ok(Some(manifest));
    }
    let path = sync_root.join(relative);
    let content = match std::fs::read(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(crate::Error::Io(error)),
    };
    let manifest = serde_json::from_slice::<SyncManifest>(&content).map_err(|error| {
        crate::Error::Other(format!(
            "version_snapshot: manifest corrupt at {}: {error}",
            path.display()
        ))
    })?;
    Ok(Some(manifest))
}

/// target 的稳定对象版本：只由业务决议产生的事实组成（复审 #5）。
///
/// manifest 里的对象版本/hash + 该 target 未确认的 mutation；不读 mtime、
/// 不看文件长度、不混入共享账本的全局序号——否则任何统计事件都会让所有
/// App 组变 dirty，空闲轮询也就退化成全量扫描。

/// 从一个已加载的 manifest 与账本事实计算 target 版本。
fn build_version(
    manifest: Option<&SyncManifest>,
    pending: BTreeMap<String, local_mutations::LocalMutationRecord>,
    known_files: usize,
    group: Option<&AppDataGroup>,
) -> LocalTargetVersion {
    // 没有 manifest 却有基线（known_files 非空）：说明本地索引不可信（旧版布局、
    // 手工删除、迁移中断），必须完整扫描重建，不能靠增量比对宣布无变化。
    if manifest.is_none() && known_files > 0 {
        return LocalTargetVersion::ineligible("missing_baseline");
    }
    // 还没有 manifest：本 target 从未建立过基线，必须走完整同步把 manifest
    // 落下来，不能直接进入"无变化"快速路径（复审 #4）。
    if manifest.is_none() {
        return LocalTargetVersion::ineligible("missing_manifest");
    }
    let empty = SyncManifest::default();
    let manifest = manifest.unwrap_or(&empty);
    // 对象指纹 = manifest 里的对象版本/hash + 本 target 未确认的 mutation。
    // 写入路径未接入账本时不在指纹里猜：对应 target 由显式恢复重建基线。
    let mut hasher = Sha256::new();
    hasher.update(fingerprint_objects(manifest, &pending, group).as_bytes());
    LocalTargetVersion {
        object_fingerprint: format!("{:x}", hasher.finalize()),
        pending_fingerprint: local_mutations::pending_digest(&pending),
        pending_objects: pending.len(),
        eligible: true,
        ineligible_reason: None,
    }
}

/// App 数据组 target 版本。
///
/// `legacy_layout` 表示整份 App 目标仍处于旧版聚合清单/状态，这些 target
/// 统一回到完整 LWW，不做增量推断。`ledger` 是整轮只读一次的 mutation 账本
/// （App 组共享同一份 JSONL），由 `collect` 统一加载并判定损坏。
pub(crate) fn app_group_version(
    app_data_root: &std::path::Path,
    group: &AppDataGroup,
    legacy_layout: bool,
    ledger: &std::collections::HashMap<String, local_mutations::LocalMutationRecord>,
) -> crate::Result<LocalTargetVersion> {
    if legacy_layout {
        return Ok(LocalTargetVersion::ineligible("legacy_layout"));
    }
    let state =
        crate::sync::SyncService::load_sync_state_read_only_for_group(app_data_root, group, None)?;
    if state.last_error.is_some() {
        return Ok(LocalTargetVersion::ineligible("last_error"));
    }
    if !state.conflicted_files.is_empty() {
        return Ok(LocalTargetVersion::ineligible("conflict"));
    }
    if !state.pending_take_remote.is_empty() {
        return Ok(LocalTargetVersion::ineligible("pending_take_remote"));
    }
    let manifest = read_manifest(app_data_root, &group.local_manifest_path())?;
    let pending =
        local_mutations::pending_records_in(ledger, app_data_root, SyncScope::App, Some(group))?;
    Ok(build_version(
        manifest.as_ref(),
        pending,
        state.known_files.len(),
        Some(group),
    ))
}

/// 作品 target 版本。
pub(crate) fn project_version(project_root: &std::path::Path) -> crate::Result<LocalTargetVersion> {
    let state = crate::sync::SyncService::load_sync_state_read_only(project_root, None)?;
    if state.last_error.is_some() {
        return Ok(LocalTargetVersion::ineligible("last_error"));
    }
    if !state.conflicted_files.is_empty() {
        return Ok(LocalTargetVersion::ineligible("conflict"));
    }
    if !state.pending_take_remote.is_empty() {
        return Ok(LocalTargetVersion::ineligible("pending_take_remote"));
    }
    // 读取路径不修复账本：恢复由同步入口的显式 recover_ledger 完成，
    // 这里只把 target 判为不可增量（复审 #3）。
    if local_mutations::recovery_pending(project_root) {
        return Ok(LocalTargetVersion::ineligible("recovery_pending"));
    }
    let ledger = match local_mutations::latest_ledger(project_root) {
        Ok(ledger) => ledger,
        Err(_) => return Ok(LocalTargetVersion::ineligible("ledger_corrupt")),
    };
    let manifest = read_manifest(project_root, crate::sync::lww::SYNC_MANIFEST_PATH)?;
    let pending =
        local_mutations::pending_records_in(&ledger, project_root, SyncScope::Project, None)?;
    Ok(build_version(
        manifest.as_ref(),
        pending,
        state.known_files.len(),
        None,
    ))
}

/// 该 App 目标是否仍在使用旧版聚合清单/状态（只作为迁移输入）。
pub(crate) fn uses_legacy_app_layout(app_data_root: &std::path::Path) -> bool {
    app_data_root
        .join("app-meta/sync/manifest.sync.json")
        .exists()
        || app_data_root.join("app-meta/sync/sync_state.json").exists()
        || crate::writing_stats::migration::legacy_events_present(app_data_root)
}

/// 为 checkpoint 命中时回填的 target 结果。
pub(crate) fn no_change_target(
    target_kind: &str,
    project_id: Option<String>,
    remote_prefix: String,
    group_id: Option<String>,
) -> TargetSyncResult {
    TargetSyncResult {
        target_kind: target_kind.to_string(),
        project_id,
        remote_prefix,
        group_id,
        failure_path: None,
        failure_phase: None,
        result: crate::sync::types::SyncResult::no_changes(),
        deleted_resolution: None,
        local_lifecycle_action: crate::sync::types::LocalLifecycleCommitAction::None,
    }
}

/// 计算整份 App 根 + 所有作品的对象级本地版本。
pub(crate) fn collect(
    app_data_root: &std::path::Path,
    projects_root: &std::path::Path,
) -> crate::Result<LocalVersionSnapshot> {
    let legacy_layout = uses_legacy_app_layout(app_data_root);
    // 读不到待删/待清理清单时按"有收口动作"处理：宁可完整核对，也不能
    // 因为读失败而跳过远端删除。
    let lifecycle_pending =
        match crate::sync::pending_deleted::load_pending_deleted_targets(app_data_root) {
            Ok(pending) => !pending.is_empty(),
            Err(_) => true,
        } || match crate::sync::pending_remote_cleanup::load_pending_remote_cleanups(app_data_root)
        {
            Ok(pending) => !pending.is_empty(),
            Err(_) => true,
        };
    let mut targets = BTreeMap::new();
    let mut no_change_targets = Vec::new();
    // App 数据组共享同一份 mutation 账本：整轮读一次，再按 group 的 owns_path
    // 过滤，不在每个 group 上重复加载同一个 JSONL（复审 #5）。
    // App 根还有未完成的持久恢复（投影/确认重放失败）：所有 App 组都不能走增量。
    let app_recovery_pending = local_mutations::recovery_pending(app_data_root);
    // 只读不修复：恢复统一由同步入口的显式 recover_ledger 负责；这里损坏时
    // 所有 App 组都判为不可增量，回到完整核对。
    let app_ledger = local_mutations::latest_ledger(app_data_root).ok();

    for group in crate::sync::app_data_group_catalog::discover_local_app_data_groups(app_data_root)?
    {
        let version = if app_recovery_pending {
            LocalTargetVersion::ineligible("recovery_pending")
        } else if let Some(ledger) = app_ledger.as_ref() {
            app_group_version(app_data_root, &group, legacy_layout, ledger)?
        } else {
            // 账本损坏/读失败：本组不能走增量，明确判为不可增量并重建基线。
            LocalTargetVersion::ineligible("ledger_corrupt")
        };
        let key = app_target_key(&group);
        let target = SyncTarget::app_data_group(group);
        no_change_targets.push(no_change_target(
            "app",
            None,
            target.remote_prefix.clone(),
            target.app_group.as_ref().map(AppDataGroup::id),
        ));
        targets.insert(key, version);
    }

    for project in crate::project::list_projects(projects_root)? {
        let project_root = projects_root.join(&project.id);
        // 作品仍处于旧版 state.local.json 路径时同样回到完整 LWW。
        if project_root.join("app-meta/sync/sync_state.json").exists() {
            targets.insert(
                project_target_key(&project.id),
                LocalTargetVersion::ineligible("legacy_layout"),
            );
        } else {
            targets.insert(
                project_target_key(&project.id),
                project_version(&project_root)?,
            );
        }
        no_change_targets.push(no_change_target(
            "project",
            Some(project.id.clone()),
            format!("projects/{}", project.id),
            None,
        ));
    }

    // 不能走增量的 target 与原因要留在诊断里：运行时表现为"这一轮又全量核对"，
    // 没有这行日志就无法区分是基线缺失、旧版布局、冲突还是读盘错误。
    for (key, version) in &targets {
        if let Some(reason) = &version.ineligible_reason {
            log::debug!("[sync] checkpoint target {key} ineligible: {reason}");
        }
    }

    Ok(LocalVersionSnapshot {
        targets,
        no_change_targets,
        lifecycle_pending,
    })
}

/// 判定一个 target 的远端路径是否属于某个 target key 的前缀域。
///
/// 远端 HEAD 变化时用它把 Git tree 差异映射到受影响的 app group / project；
/// 无法映射的路径（例如 catalog 顶层文件、未知目录）返回 `None`，调用方必须
/// 按"影响全部 target"处理，不能静默漏掉。
pub(crate) fn target_key_for_remote_path(
    remote_path: &str,
    app_groups: &[AppDataGroup],
    project_ids: &[String],
) -> Option<String> {
    let path = remote_path.replace('\\', "/");
    if let Some(rest) = path.strip_prefix("projects/") {
        let project_id = rest.split('/').next()?;
        if project_ids.iter().any(|id| id == project_id) {
            return Some(project_target_key(project_id));
        }
        return None;
    }
    // App 数据组的远端前缀是 app/__groups__/<storage_key>，不是本地相对路径：
    // 直接拿完整远端路径喂 owns_path 永远映射不到，会让每次远端 App 更新都回落
    // 到"影响全部 target"（Issue #1065 复审 #5）。
    if let Some(rest) = path.strip_prefix("app/__groups__/") {
        let storage_key = rest.split('/').next()?;
        for group in app_groups {
            if group.storage_key() == storage_key {
                return Some(app_target_key(group));
            }
        }
        return None;
    }
    // 兼容旧版聚合布局：catalog 顶层文件（app-meta/sync/app_groups.sync.json、
    // 旧 app 根路径）无法归因到具体组，必须按"影响全部"处理。
    None
}
