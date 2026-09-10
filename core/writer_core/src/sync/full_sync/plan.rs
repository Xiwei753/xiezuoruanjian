//! Full sync plan builder — target enumeration and LWW decision helpers.
//!
//! Contains `build_full_sync_target_plan` and all planner helpers:
//! lifecycle candidates, decision functions, LWW helpers.

use std::path::Path;

use super::{DeletedTargetLww, LifecycleCandidate, LiveTargetLww, PlannedTarget};

///   无副作用共享 target planner。
///
/// 正式 `prepare_full_sync` 和 `perform_full_sync_dry_run` 都调用本函数枚举 targets，
/// 不复制一套 target 枚举逻辑。正式同步再在结果上创建 staging/provider transfer。
///
/// 产出的 `PlannedTarget` 列表顺序：App target → live Project targets → pending
/// deleted targets。`staging_root` 全部为 `None`（由 `prepare_staging_runs` 填充）。
///
///   `remote_catalog` 真正参与 target 决策。按 `target_id`
/// 合并 local live project / local pending delete / remote lifecycle record，
/// 生成 `PlannedTargetKind` 明确类型：
/// - 远端无 delete tombstone 或本地更新 → `LiveProject`；
/// - 远端 delete tombstone 更新 → `DeleteLocalProject`（不上传）；
/// - 本地 pending delete + 本地 tombstone 胜出 → `DeleteRemoteProject`；
/// - 本地 pending delete + 远端 upsert 更新 → `RestoreProject`；
/// - 无法决策 → `Retry`。
///
/// `device_id` 来自真实 `DeviceInfo.device_id`，用于 lifecycle record 的 tie-break。
#[allow(clippy::too_many_arguments)] // 9 个参数均为独立决策输入，打包会掩盖各自语义
#[allow(clippy::too_many_lines)] // target 枚举 + LWW 决策逻辑 inherently long
pub fn build_full_sync_target_plan(
    app_data_root: &Path,
    projects_root: &Path,
    live_projects: &[crate::project::Project],
    pending_deleted: &[crate::sync::types::PendingDeletedTarget],
    remote_catalog: &crate::sync::types::TargetLifecycleCatalog,
    _sync_policy: &crate::sync::types::SyncPolicy,
    _force_sync: bool,
    device_id: &str,
    pending_remote_cleanups: &[crate::sync::pending_remote_cleanup::PendingRemoteTargetCleanup],
) -> Vec<PlannedTarget> {
    use crate::sync::types::PlannedTargetKind;

    let mut targets = Vec::new();

    // App target
    let app_target = crate::sync::types::SyncTarget::app();
    targets.push(PlannedTarget {
        target: app_target,
        local_root: app_data_root.to_path_buf(),
        staging_root: None,
        target_kind: PlannedTargetKind::App,
        project_id: None,
        target_live_root: app_data_root.to_path_buf(),
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: None,
        expected_delete_lww: None,
    });

    // Project targets —   按 remote catalog 决策。
    for project in live_projects {
        let target = crate::sync::types::SyncTarget::project(&project.id);
        let project_local_root = projects_root.join(&project.id);
        let target_id = &target.remote_prefix;

        // lifecycle candidate 走 snapshot_local_records_read_only
        // 单一来源，不再读旧 manifest / 手写 initial scanner。失败 → Retry。
        let candidate = compute_local_project_lifecycle_candidate(&project_local_root, device_id);
        let (live_lww, kind) = match &candidate {
            LifecycleCandidate::Live { lww } => {
                let lww_clone = lww.clone();
                let kind = decide_live_project_kind(remote_catalog, target_id, Some(lww));
                (Some(lww_clone), kind)
            }
            LifecycleCandidate::Retry => {
                //   无法可靠求本地 LWW → 不 DeleteLocalProject
                // （无证据证明远端 delete 更新），让 target 走 Retry 保留 pending。
                (None, PlannedTargetKind::Retry)
            }
        };

        targets.push(PlannedTarget {
            target,
            local_root: project_local_root.clone(),
            staging_root: None,
            target_kind: kind,
            project_id: Some(project.id.clone()),
            target_live_root: project_local_root,
            deleted_journal_token: None,
            deleted_lww: None,
            live_lww,
            expected_delete_lww: None,
        });
    }

    //   Pending deleted targets —
    // 已删除作品的远端前缀需要清理。按 remote catalog 决策：
    // - 本地 tombstone 胜出 → DeleteRemoteProject（删远端 + 写 tombstone）；
    // - 远端 upsert 胜出 → RestoreProject（下载恢复）；
    // - 无法决策 → Retry。
    for pending in pending_deleted {
        //   用 parse_project_target_id 严格验证 target_id，
        // 非法记录跳过（不恢复、不删除、不 panic），不再 unwrap_or_default()。
        let project_id = match crate::sync::target_lifecycle::parse_project_target_id(
            &pending.target.remote_prefix,
        ) {
            Ok(id) => id,
            Err(e) => {
                log::warn!(
                    "[sync] build_full_sync_target_plan: skip pending deleted target \
                     with invalid target_id {:?}: {e}",
                    pending.target.remote_prefix
                );
                continue;
            }
        };
        let project_root = projects_root.join(&project_id);
        let target_id = &pending.target.remote_prefix;

        let kind = decide_pending_deleted_kind(remote_catalog, target_id, pending);

        targets.push(PlannedTarget {
            target: pending.target.clone(),
            local_root: project_root.clone(),
            staging_root: None,
            target_kind: kind,
            project_id: Some(project_id),
            target_live_root: project_root,
            deleted_journal_token: Some(pending.journal_token.clone()),
            deleted_lww: Some(DeletedTargetLww {
                deleted_at_ms: pending.deleted_at_ms,
                device_id: pending.device_id.clone(),
            }),
            live_lww: None,
            expected_delete_lww: None,
        });
    }

    //   遍历 remote_catalog.records 补远端独有 target。
    // 对本地既没有 live project 也没有 pending delete 的远端记录：
    // - 远端 Upsert → RestoreProject（下载远端恢复，让新设备发现远端独有作品）；
    // - 远端 Delete → RemoteCleanupProject（  ：
    //   不再跳过，让远端 Delete tombstone 本身成为 durable cleanup queue）。
    {
        use std::collections::HashSet;
        let local_target_ids: HashSet<String> = targets
            .iter()
            .map(|t| t.target.remote_prefix.clone())
            .collect();
        for remote_rec in &remote_catalog.records {
            if local_target_ids.contains(&remote_rec.target_id) {
                continue;
            }
            //   用 parse_project_target_id 严格验证 target_id，
            // 非法记录跳过并 log warn（不恢复、不删除、不 panic），不再 unwrap_or_default()。
            let project_id =
                match crate::sync::target_lifecycle::parse_project_target_id(&remote_rec.target_id)
                {
                    Ok(id) => id,
                    Err(e) => {
                        log::warn!(
                            "[sync] build_full_sync_target_plan: skip remote-only target \
                         with invalid target_id {:?}: {e}",
                            remote_rec.target_id
                        );
                        continue;
                    }
                };
            let project_root = projects_root.join(&project_id);
            // remote-only Delete 直接生成
            // RemoteCleanupProject target（不再跳过）。这让远端 Delete tombstone
            // 本身成为 durable cleanup queue — 即使本地 pending_remote_cleanups.json
            // 没成功持久化，下一轮看到 remote Delete + local absent 仍会生成
            // cleanup target。本地 pending_remote_cleanups.json 保留诊断/退避信息，
            // 但不再决定是否存在 cleanup target。
            // - remote-only Upsert → RestoreProject（下载远端恢复）；
            // - remote-only Delete → RemoteCleanupProject，expected_delete_lww 从
            //   remote_rec 构造（deleted_at_ms 或 updated_at_ms 作为 lww_time，
            //   device_id 从 remote_rec）。
            match remote_rec.op {
                crate::sync::types::TargetOp::Upsert => {
                    targets.push(PlannedTarget {
                        target: crate::sync::types::SyncTarget::project(&project_id),
                        local_root: project_root.clone(),
                        staging_root: None,
                        target_kind: PlannedTargetKind::RestoreProject,
                        project_id: Some(project_id),
                        target_live_root: project_root,
                        deleted_journal_token: None,
                        deleted_lww: None,
                        live_lww: None,
                        expected_delete_lww: None,
                    });
                }
                crate::sync::types::TargetOp::Delete => {
                    let expected_lww_time =
                        crate::sync::target_lifecycle::record_lww_time(remote_rec);
                    targets.push(PlannedTarget {
                        target: crate::sync::types::SyncTarget::project(&project_id),
                        local_root: project_root.clone(),
                        staging_root: None,
                        target_kind: PlannedTargetKind::RemoteCleanupProject,
                        project_id: Some(project_id),
                        target_live_root: project_root,
                        deleted_journal_token: None,
                        deleted_lww: None,
                        live_lww: None,
                        expected_delete_lww: Some(DeletedTargetLww {
                            deleted_at_ms: expected_lww_time,
                            device_id: remote_rec.device_id.clone(),
                        }),
                    });
                }
            }
        }
    }

    // 加载 pending_remote_cleanups，为每个未已在
    // targets 中的 remote_prefix 生成 RemoteCleanupProject target。这让上一轮
    // authoritative Delete 清 prefix 失败的远端残留能在下一轮被重试清理，
    // 即使本地没有该 Project（remote-only cleanup 场景）。
    {
        use std::collections::HashSet;
        let existing_prefixes: HashSet<String> = targets
            .iter()
            .map(|t| t.target.remote_prefix.clone())
            .collect();
        for cleanup in pending_remote_cleanups {
            if existing_prefixes.contains(&cleanup.remote_prefix) {
                // 已有 target 会处理这个 prefix（如 DeleteLocalProject /
                // DeleteRemoteProject / RestoreProject），不重复加入。
                continue;
            }
            // 校验 project_id（防路径穿越）。非法 → skip（不 panic）。
            let target_id = format!("projects/{}", cleanup.project_id);
            if let Err(e) = crate::sync::target_lifecycle::parse_project_target_id(&target_id) {
                log::warn!(
                    "[sync] build_full_sync_target_plan: skip pending remote cleanup \
                     with invalid project_id {:?}: {e}",
                    cleanup.project_id
                );
                continue;
            }
            let project_root = projects_root.join(&cleanup.project_id);
            targets.push(PlannedTarget {
                target: crate::sync::types::SyncTarget::project(&cleanup.project_id),
                local_root: project_root.clone(),
                staging_root: None,
                target_kind: PlannedTargetKind::RemoteCleanupProject,
                project_id: Some(cleanup.project_id.clone()),
                target_live_root: project_root,
                deleted_journal_token: None,
                deleted_lww: None,
                live_lww: None,
                // 从 PendingRemoteTargetCleanup
                // 填入 Delete lifecycle identity，run_transfer 时 CAS 校验。
                expected_delete_lww: Some(DeletedTargetLww {
                    deleted_at_ms: cleanup.expected_delete_lww_time_ms,
                    device_id: cleanup.expected_delete_device_id.clone(),
                }),
            });
        }
    }

    targets
}

///   本地 project lifecycle candidate。
///
/// `compute_local_project_lifecycle_candidate` 只做：
/// 1. `snapshot_local_records_read_only(project_root, SyncScope::Project, device_id)`
///    → 从 records 取 `max(lww_time, device_id)` → `Live(lww)`；
/// 2. 失败（known file 消失且无 tombstone 等）→ `Retry`。
///
/// 首次同步、已有 manifest、离线改动全部走这一套。target lifecycle 和文件 LWW
/// 共用同一份"当前本地 records"，不再保留旧 manifest 直读 + 手写 initial scanner
/// 的第二套状态机（`build_initial_lww_from_project_scan` / `scan_sync_file` /
/// `append_chapter_meta_records` / `initial_manifest` 已删除）。
pub(super) fn compute_local_project_lifecycle_candidate(
    project_root: &Path,
    device_id: &str,
) -> super::LifecycleCandidate {
    use crate::sync::types::SyncScope;

    let records = match crate::sync::lww::snapshot_local_records_read_only(
        project_root,
        SyncScope::Project,
        device_id,
    ) {
        Ok(records) => records,
        Err(e) => {
            log::warn!(
                "[sync] compute_local_project_lifecycle_candidate: \
                 snapshot_local_records_read_only failed at {}: {e} — returning Retry",
                project_root.display()
            );
            return super::LifecycleCandidate::Retry;
        }
    };

    // 从 records 取 max(lww_time, device_id)。
    let winner = records.values().max_by(|a, b| {
        let a_time = lww_record_time_for_manifest_record(a);
        let b_time = lww_record_time_for_manifest_record(b);
        a_time
            .cmp(&b_time)
            .then_with(|| a.device_id.cmp(&b.device_id))
    });

    match winner {
        Some(w) => {
            let lww = LiveTargetLww {
                lww_time_ms: lww_record_time_for_manifest_record(w),
                device_id: w.device_id.clone(),
            };
            log::debug!(
                "[sync] compute_local_project_lifecycle_candidate: \
                 snapshot at {} — lww_time={} device_id={} records={}",
                project_root.display(),
                lww.lww_time_ms,
                lww.device_id,
                records.len()
            );
            super::LifecycleCandidate::Live { lww }
        }
        None => {
            // records 为空（全新 project，无任何可同步文件）→ 用 device_id + 0 时间。
            // 不伪造 now()，让远端 catalog 决策按真实事实进行（远端无记录 → LiveProject）。
            log::debug!(
                "[sync] compute_local_project_lifecycle_candidate: \
                 empty snapshot at {} — using zero lww",
                project_root.display()
            );
            super::LifecycleCandidate::Live {
                lww: LiveTargetLww {
                    lww_time_ms: 0,
                    device_id: device_id.to_string(),
                },
            }
        }
    }
}

///   live project 的 target-level LWW 决策。
///
/// - 远端无记录或远端是 upsert → `LiveProject`（正常同步）；
/// - 远端是 delete tombstone 且本地 live 更新（`live_lww` 胜出）→ `LiveProject`（重新 upsert）；
/// - 远端是 delete tombstone 且远端胜出 → `DeleteLocalProject`（不上传，本地应删除）；
/// - 远端是 delete tombstone 且无 `live_lww`（manifest 读取失败）→ `Retry`
///   （无证据证明远端 delete 更新，绝不破坏性删除，
///   也不伪造 now 复活远端 delete。让 target 保留 pending，下次同步重试）。
///
/// 远端无记录/upsert 且 manifest 缺失时仍返回 `LiveProject`，
/// 但 `run_transfer` 的 LiveProject 分支在 `live_lww=None` 时不伪造 now()，
/// 返回 `RecoverableError`（防御性，不写 catalog，不改 remote lifecycle）。
fn decide_live_project_kind(
    remote_catalog: &crate::sync::types::TargetLifecycleCatalog,
    target_id: &str,
    live_lww: Option<&LiveTargetLww>,
) -> crate::sync::types::PlannedTargetKind {
    use crate::sync::types::{PlannedTargetKind, TargetOp};

    let Some(remote_rec) = crate::sync::target_lifecycle::find_record(remote_catalog, target_id)
    else {
        // 远端无记录 → 正常同步（run_transfer 在 live_lww=None 时不伪造 now）。
        return PlannedTargetKind::LiveProject;
    };

    // 远端是 upsert → 正常同步。
    if remote_rec.op == TargetOp::Upsert {
        return PlannedTargetKind::LiveProject;
    }

    // 远端是 delete tombstone → 需要与本地 live LWW 比较。
    let Some(live) = live_lww else {
        //   无本地 LWW（manifest 读取失败）→
        // 不 DeleteLocalProject（无证据证明远端 delete 更新），
        // 也不伪造 now 复活远端 delete。返回 Retry，pending 保留。
        return PlannedTargetKind::Retry;
    };

    let remote_time = crate::sync::target_lifecycle::record_lww_time(remote_rec);
    let local_wins = live.lww_time_ms > remote_time
        || (live.lww_time_ms == remote_time && live.device_id > remote_rec.device_id);

    if local_wins {
        // 本地离线编辑更晚 → 重新 upsert 建立远端 target。
        PlannedTargetKind::LiveProject
    } else {
        // 远端 delete 更晚 → 不上传，本地应删除。
        PlannedTargetKind::DeleteLocalProject
    }
}

///   pending deleted target 的 target-level LWW 决策。
///
/// - 远端无记录或远端是 upsert 且本地 tombstone 胜出 → `DeleteRemoteProject`；
/// - 远端是 upsert 且远端胜出 → `RestoreProject`；
/// - 远端是 delete tombstone → `DeleteRemoteProject`（catalog 已有 tombstone，仍需清理远端对象）。
fn decide_pending_deleted_kind(
    remote_catalog: &crate::sync::types::TargetLifecycleCatalog,
    target_id: &str,
    pending: &crate::sync::types::PendingDeletedTarget,
) -> crate::sync::types::PlannedTargetKind {
    use crate::sync::types::{PlannedTargetKind, TargetOp};

    let Some(remote_rec) = crate::sync::target_lifecycle::find_record(remote_catalog, target_id)
    else {
        // 远端无 catalog 记录 → 本地 tombstone 胜出，删远端。
        return PlannedTargetKind::DeleteRemoteProject;
    };

    match remote_rec.op {
        TargetOp::Delete => {
            // 远端已有 delete tombstone → 仍需清理远端对象（可能有残留文件）。
            PlannedTargetKind::DeleteRemoteProject
        }
        TargetOp::Upsert => {
            // 远端是 upsert → 需要与本地 tombstone 做 LWW 比较。
            let remote_time = crate::sync::target_lifecycle::record_lww_time(remote_rec);
            let local_wins = pending.deleted_at_ms > remote_time
                || (pending.deleted_at_ms == remote_time
                    && pending.device_id > remote_rec.device_id);

            if local_wins {
                PlannedTargetKind::DeleteRemoteProject
            } else {
                PlannedTargetKind::RestoreProject
            }
        }
    }
}

/// 计算单条 manifest 记录的 LWW 时间（delete 用 `deleted_at_ms`，upsert 用 `updated_at_ms`）。
fn lww_record_time_for_manifest_record(r: &crate::sync::types::ManifestFileRecord) -> i64 {
    if r.op == "delete" {
        r.deleted_at_ms.unwrap_or(r.updated_at_ms)
    } else {
        r.updated_at_ms
    }
}

/// 从 manifest 取 target-level LWW，携带 winner 的 device_id。
///
/// 按 `(lww_record_time(record), record.device_id)` 取最大 record，
/// 返回完整 `LiveTargetLww { lww_time_ms, device_id: winner_record.device_id }`。
/// Prepare 前判断和 post-transfer publish 都用同一个 helper，
/// 保证 catalog 写入的 device_id 与真实 winner 一致（不再硬塞本机设备）。
fn manifest_target_lww(manifest: &crate::sync::types::SyncManifest) -> Option<LiveTargetLww> {
    let winner = manifest.files.iter().max_by(|a, b| {
        let a_time = lww_record_time_for_manifest_record(a);
        let b_time = lww_record_time_for_manifest_record(b);
        // 先按时间比较，时间相同按 device_id 字典序比较（与 resolve_lww_path 同规则）。
        a_time
            .cmp(&b_time)
            .then_with(|| a.device_id.cmp(&b.device_id))
    })?;
    Some(LiveTargetLww {
        lww_time_ms: lww_record_time_for_manifest_record(winner),
        device_id: winner.device_id.clone(),
    })
}

/// 读 post-transfer staging manifest，算最终 LWW。
///
/// 正文 transfer 成功后，publish candidate 必须用 staging manifest 的
/// `max(lww_record_time)` 作为 lifecycle 时间，不能用 Transfer 前的旧 live_lww
/// （那只是"本机有没有资格尝试同步"的判断，不是正文 LWW 合并后的最终状态）。
///
/// 返回完整 `LiveTargetLww`（含 winner 的 device_id），
/// 不再只返回 `i64` 时间。调用方用 winner 的 device_id 构造 candidate，
/// 不再硬塞本机设备。
///
/// `root` 是 staging root（`planned.staging_root`）或 `planned.local_root`。
/// manifest 不存在或解析失败 → None（调用方返回 RecoverableError，不伪造旧时间）。
pub(super) fn read_post_transfer_lww(root: &std::path::Path) -> Option<LiveTargetLww> {
    let manifest_path = root.join("app-meta/sync/manifest.sync.json");
    let content = std::fs::read(&manifest_path).ok()?;
    let manifest: crate::sync::types::SyncManifest = serde_json::from_slice(&content).ok()?;
    manifest_target_lww(&manifest)
}
