//!   generation GC — provider-neutral 清理未引用 generation。
//!
//! LiveProject 每次创建新 generation（`projects/P/__generations__/G/`），CAS 成功后
//! 旧 generation 成为未引用。本模块按保留期清理未引用 generation，不碰 active
//! generation 和正在上传（incomplete + lease 未过期）的 generation。
//!
//! ## GC 调用时机
//!
//! 在 `run_transfer` 末尾对每个 project 调一次 [`run_generation_gc`]。**不**在 CAS
//! 成功后立刻删旧 generation — 另一台设备可能正拿着旧 catalog 下载。
//!
//! ## meta 文件
//!
//! 每个 generation 有 `generation.meta.json`，记录上传状态（complete）和租约
//! （upload_lease_until_ms）。GC 用 meta 判断 generation 是否可删。

use crate::sync::cancellation_token::SyncCancellationToken;
use crate::sync::provider::model::{
    BatchMutation, DeletePrecondition, RemoteEntry, RemoteVersion, WritePrecondition,
};
use crate::sync::provider::SyncProvider;

/// generation meta — 记录单个 generation 的上传状态和保留信息。
///
/// 写到 `projects/P/__generations__/G/generation.meta.json`。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenerationMeta {
    pub generation_id: String,
    pub project_id: String,
    pub created_at_ms: i64,
    pub uploader_device_id: String,
    pub upload_lease_until_ms: i64,
    pub complete: bool,
}

/// generation meta 在远端的固定文件名。
pub const GENERATION_META_FILENAME: &str = "generation.meta.json";

/// 上传租约时长（5 分钟）。incomplete 且 lease 未过期的 generation 不删（上传中）。
pub const GENERATION_UPLOAD_LEASE_MS: i64 = 5 * 60 * 1000;

/// 未引用 generation 安全保留期（7 天）。超过保留期且未引用的 generation 才可删。
pub const GENERATION_RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;

// ── Issue #993：缺 meta 代际的 orphan observation 生命周期 ──
//
// 缺 meta 的 generation 无法用 GenerationMeta 判定创建时间/上传状态。UUIDv4 和
// blob SHA 都不含可核实的创建时间，不能据以猜年龄。改为在远端独立 GC 元数据中
// 记录首次发现时间 + 目录内容指纹，观察满保留期且内容未变化后才回收。

/// 远端 orphan observation 存储目录（项目级，独立于 `__generations__`）。
const ORPHAN_OBSERVATIONS_DIR: &str = "__gc__";
/// 远端 orphan observation 文件名。
const ORPHAN_OBSERVATIONS_FILENAME: &str = "orphan-observations.json";

/// 缺 meta 代际的首次发现观察记录。
///
/// 持久化到 `{project_remote_prefix}/__gc__/orphan-observations.json`，用 Provider
/// CAS（IfMatch）写入，多设备并发安全。`content_fingerprint` 是观察时 generation
/// prefix 下所有对象 (path, version) 的指纹，用于检测观察期内目录内容是否变化：
/// 变化则重新计时（可能仍有设备在写入）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrphanObservation {
    pub generation_id: String,
    pub first_observed_at_ms: i64,
    pub content_fingerprint: String,
}

/// 计算 generation prefix 下所有对象的指纹。
///
/// 对 (path, version) 排序后拼接，取 content_md5。内容未变化 ⇒ 指纹不变；
/// 任何对象增删改版本 ⇒ 指纹变化。
fn compute_generation_fingerprint(entries: &[RemoteEntry]) -> String {
    let mut lines: Vec<String> = entries
        .iter()
        .map(|e| format!("{}:{}", e.path, e.version.0))
        .collect();
    lines.sort_unstable();
    let joined = lines.join("\n");
    crate::sync::hash::content_md5(joined.as_bytes())
}

/// 加载远端 orphan observations（只读）。
///
/// 返回 (observations, prev_version)。文件不存在 → 空列表 + None（首次写用 CreateNew）。
fn load_orphan_observations(
    provider: &dyn SyncProvider,
    gc_meta_path: &str,
) -> crate::error::Result<(Vec<OrphanObservation>, Option<RemoteVersion>)> {
    let obj = provider.read(gc_meta_path).map_err(crate::Error::from)?;
    match obj {
        Some(obj) => {
            let observations: Vec<OrphanObservation> =
                serde_json::from_slice(&obj.content).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "load_orphan_observations: parse {}: {e}",
                        gc_meta_path
                    )))
                })?;
            Ok((observations, Some(obj.version)))
        }
        None => Ok((Vec::new(), None)),
    }
}

/// CAS 写回 orphan observations。
///
/// `prev_version` 为 None → CreateNew；Some → IfMatch。PreconditionFailed（其他设备
/// 并发修改）时 log warn 返回 Ok(())：observations 写回失败最坏是下轮重新观察（重新
/// 计时七天），不会误删 generation。其他错误向上返回。
fn save_orphan_observations(
    provider: &dyn SyncProvider,
    gc_meta_path: &str,
    observations: &[OrphanObservation],
    prev_version: Option<RemoteVersion>,
) -> crate::error::Result<()> {
    let content = serde_json::to_vec(observations).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "save_orphan_observations: serialize: {e}"
        )))
    })?;
    let precondition = match prev_version {
        Some(v) => WritePrecondition::IfMatch(v),
        None => WritePrecondition::CreateNew,
    };
    match provider.write(gc_meta_path, &content, precondition) {
        Ok(_) => Ok(()),
        Err(crate::sync::provider::error::ProviderError::PreconditionFailed { path, reason }) => {
            log::warn!(
                "[sync] save_orphan_observations: CAS precondition failed for {}: path={}, reason={} — observations not persisted, will re-observe next round",
                gc_meta_path, path, reason
            );
            Ok(())
        }
        Err(e) => Err(crate::Error::from(e)),
    }
}

/// 运行 generation GC。
///
/// 清理 `projects/P/__generations__/` 下未引用的 generation：
///
/// 1. GC 开始重新 `load_remote_catalog` 掌握远端当前事实（不用调用方传入的过期 snapshot）。
/// 2. list `projects/P/__generations__/*`，对每个 generation ID 调
///    `validate_generation_id` 校验后再拼路径（防路径穿越）。
/// 3. 当前 `active_generation` 永远不删。
/// 4. incomplete 且 lease 未过期 → 不删（上传中）。
/// 5. unreferenced generation 超过安全保留期（`created_at_ms + retention_ms < now_ms`）
///    后才可删。
/// 6. **真正删除每个 generation 前再次 `load_remote_catalog`**，确认该 G 仍不是当前
///    active_generation（Transfer 期间另一台设备可能 CAS 切了 active generation）。
/// 7. delete 失败 → `Err`（`RecoverableError` 语义），下轮继续。
/// 8. meta 缺失/损坏的 generation：保守保留（不删），log warn。
///
/// `project_remote_prefix` 如 `"projects/P"`；`active_generation_hint` 为调用方已知的
/// active generation ID（用作首次确认），但 GC 不依赖它 — 删除前会重新读 catalog。
//
// Issue #729 评论 5765306162 问题5：加取消检查后复杂度/nesting/行数超标，
// 与 transfer_helpers.rs 同类函数一致用 allow 标注。取消检查是必要的健壮性，
// 非宽范围 allow 掩盖 warning。
#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub fn run_generation_gc(
    provider: &dyn SyncProvider,
    project_remote_prefix: &str,
    active_generation_hint: Option<&str>,
    now_ms: i64,
    retention_ms: i64,
    cancellation_token: Option<&SyncCancellationToken>,
) -> crate::error::Result<()> {
    let generations_prefix = format!("{project_remote_prefix}/__generations__");
    let entries = provider
        .list(&generations_prefix)
        .map_err(crate::Error::from)?;
    // Issue #729 评论 5765306162 问题5：provider.list 返回后检查取消令牌。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] run_generation_gc: cancellation requested after list {} — returning Ok",
                project_remote_prefix
            );
            return Ok(());
        }
    }
    // 提取唯一 generation ID segment（__generations__/G/... → G），
    // 并通过 validate_generation_id 校验，防路径穿越。
    let mut generation_ids: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in &entries {
        let first_segment = entry.path.split('/').next().unwrap_or("");
        if first_segment.is_empty() {
            continue;
        }
        if !seen.insert(first_segment.to_string()) {
            continue;
        }
        // validate_generation_id 后再拼路径。
        crate::sync::target_lifecycle::validate_generation_id(first_segment)?;
        generation_ids.push(first_segment.to_string());
    }

    // Issue #993：加载远端 orphan observations（缺 meta 代际的首次发现记录）。
    let gc_meta_path = format!(
        "{}/{ORPHAN_OBSERVATIONS_DIR}/{ORPHAN_OBSERVATIONS_FILENAME}",
        project_remote_prefix
    );
    let (mut orphan_observations, orphan_obs_prev_version) =
        load_orphan_observations(provider, &gc_meta_path)?;
    let mut orphan_obs_changed = false;
    // 清理 stale 观察：generation 已不存在（被其他设备回收或变 active 后不再 list 出）。
    let initial_obs_count = orphan_observations.len();
    orphan_observations.retain(|o| generation_ids.contains(&o.generation_id));
    if orphan_observations.len() != initial_obs_count {
        orphan_obs_changed = true;
    }

    for gen_id in &generation_ids {
        // Issue #729 评论 5765306162 问题5：每次 generation 迭代前检查取消令牌。
        if let Some(token) = cancellation_token {
            if token.is_cancelled() {
                log::info!(
                    "[sync] run_generation_gc: cancellation requested in generation loop — returning Ok"
                );
                return Ok(());
            }
        }
        // active_generation_hint 永远不删（调用方传入的当前 active）。
        if Some(gen_id.as_str()) == active_generation_hint {
            continue;
        }

        // 读 meta。
        let meta_path = format!(
            "{}/{}/{}",
            generations_prefix, gen_id, GENERATION_META_FILENAME
        );
        let meta_obj = provider.read(&meta_path).map_err(crate::Error::from)?;
        // Issue #729 评论 5765306162 问题5：provider.read 返回后检查取消令牌。
        if let Some(token) = cancellation_token {
            if token.is_cancelled() {
                log::info!(
                    "[sync] run_generation_gc: cancellation requested after read meta — returning Ok"
                );
                return Ok(());
            }
        }
        // Issue #993：缺 meta 回收成功后待移除的 observation index。
        let mut orphan_reclaim_idx: Option<usize> = None;

        let meta: Option<GenerationMeta> = match meta_obj {
            Some(obj) => match serde_json::from_slice::<GenerationMeta>(&obj.content) {
                Ok(m) => Some(m),
                Err(e) => {
                    // meta 损坏：保守保留（不删），log warn。
                    log::warn!(
                        "[sync] run_generation_gc: corrupted meta for {} generation {}: {} — keeping",
                        project_remote_prefix,
                        gen_id,
                        e
                    );
                    continue;
                }
            },
            None => {
                // Issue #993：meta 缺失。不能据 UUIDv4/blob SHA 猜年龄（generation_id 是
                // UUIDv4 无生成时间，RemoteEntry 只有 path+version），也不立即删除（可能
                // 是其他版本客户端/迁移程序产生的临时对象）。走 orphan observation 生命
                // 周期：首次发现记录观察起点 + 目录内容指纹，观察满保留期且内容未变化、
                // 仍缺 meta、仍未被引用后才回收。观察记录持久化到远端 __gc__/，多设备
                // 共享。
                let orphan_gen_prefix = format!("{generations_prefix}/{gen_id}");
                let orphan_entries =
                    provider.list(&orphan_gen_prefix).map_err(crate::Error::from)?;
                if let Some(token) = cancellation_token {
                    if token.is_cancelled() {
                        log::info!(
                            "[sync] run_generation_gc: cancellation requested after list orphan {} — returning Ok",
                            orphan_gen_prefix
                        );
                        return Ok(());
                    }
                }
                let fingerprint = compute_generation_fingerprint(&orphan_entries);
                let obs_idx = orphan_observations
                    .iter()
                    .position(|o| o.generation_id == *gen_id);
                match obs_idx {
                    None => {
                        // 首次发现：建立观察记录，保留。
                        orphan_observations.push(OrphanObservation {
                            generation_id: gen_id.clone(),
                            first_observed_at_ms: now_ms,
                            content_fingerprint: fingerprint,
                        });
                        orphan_obs_changed = true;
                        log::info!(
                            "[sync] run_generation_gc: {} generation {} missing meta — first observed, starting {}ms observation period",
                            project_remote_prefix, gen_id, retention_ms
                        );
                        continue;
                    }
                    Some(idx) => {
                        let prev_fp = orphan_observations[idx].content_fingerprint.clone();
                        let prev_observed = orphan_observations[idx].first_observed_at_ms;
                        if prev_fp != fingerprint {
                            // 观察期内内容变化：重新计时，保留。
                            orphan_observations[idx].first_observed_at_ms = now_ms;
                            orphan_observations[idx].content_fingerprint = fingerprint;
                            orphan_obs_changed = true;
                            log::info!(
                                "[sync] run_generation_gc: {} generation {} missing meta — content changed during observation, resetting period",
                                project_remote_prefix, gen_id
                            );
                            continue;
                        }
                        if now_ms - prev_observed < retention_ms {
                            // 未满保留期：保留。
                            log::debug!(
                                "[sync] run_generation_gc: {} generation {} missing meta — within observation period ({}ms / {}ms), keeping",
                                project_remote_prefix, gen_id, now_ms - prev_observed, retention_ms
                            );
                            continue;
                        }
                        // 满保留期 + 指纹未变化：落到下方 catalog 确认 + 删除。
                        log::info!(
                            "[sync] run_generation_gc: {} generation {} missing meta — observation period elapsed + content stable, reclaiming",
                            project_remote_prefix, gen_id
                        );
                        orphan_reclaim_idx = Some(idx);
                        None
                    }
                }
            }
        };

        // 有 meta 时检查上传中和保留期。
        if let Some(meta) = &meta {
            // incomplete 且 lease 未过期 → 不删（上传中）。
            if !meta.complete && meta.upload_lease_until_ms > now_ms {
                log::debug!(
                    "[sync] run_generation_gc: {} generation {} incomplete + lease active — keeping",
                    project_remote_prefix,
                    gen_id
                );
                continue;
            }

            // unreferenced generation 超过安全保留期后才可删。
            if meta.created_at_ms + retention_ms >= now_ms {
                log::debug!(
                    "[sync] run_generation_gc: {} generation {} within retention — keeping",
                    project_remote_prefix,
                    gen_id
                );
                continue;
            }
        }

        // 真正删除前再次 load_remote_catalog，
        // 确认该 G 仍不是当前 active_generation（Transfer 期间另一台设备可能 CAS
        // 切了 active generation）。catalog 读取失败 → Err（不删，下轮重试）。
        let fresh_catalog = crate::sync::target_lifecycle::load_remote_catalog(provider)?;
        // Issue #729 评论 5765306162 问题5：load_remote_catalog 返回后检查取消令牌。
        if let Some(token) = cancellation_token {
            if token.is_cancelled() {
                log::info!(
                    "[sync] run_generation_gc: cancellation requested after load_remote_catalog — returning Ok"
                );
                return Ok(());
            }
        }
        let fresh_active = crate::sync::target_lifecycle::find_record(
            &fresh_catalog.catalog,
            project_remote_prefix,
        )
        .and_then(|r| r.active_generation.as_deref());
        if fresh_active == Some(gen_id.as_str()) {
            log::warn!(
                "[sync] run_generation_gc: {} generation {} became active during GC — skipping",
                project_remote_prefix,
                gen_id
            );
            continue;
        }

        // 删 generation prefix 下所有对象。
        log::info!(
            "[sync] run_generation_gc: {} generation {} unreferenced + past retention — deleting",
            project_remote_prefix,
            gen_id
        );
        let gen_prefix = format!("{generations_prefix}/{gen_id}");
        let gen_entries = provider.list(&gen_prefix).map_err(crate::Error::from)?;
        // Issue #729 评论 5765306162 问题5：provider.list 返回后检查取消令牌。
        if let Some(token) = cancellation_token {
            if token.is_cancelled() {
                log::info!(
                    "[sync] run_generation_gc: cancellation requested after list {} — returning Ok",
                    gen_prefix
                );
                return Ok(());
            }
        }
        // 空 generation 直接跳过（nothing to delete）。
        if gen_entries.is_empty() {
            log::debug!(
                "[sync] run_generation_gc: {} generation {} has no entries — skipping delete",
                project_remote_prefix,
                gen_id
            );
            continue;
        }
        // Issue #779 评论 5853718466：batch provider 用一次 commit_batch 删除整个
        // generation prefix 下所有对象，避免逐文件 delete 产生几十个远端 commit
        // 长时间占用同步。非 batch provider 降级为逐文件 delete（保留原有取消检查）。
        if provider.capabilities().batch {
            let mutations: Vec<BatchMutation> = gen_entries
                .iter()
                .map(|ge| BatchMutation::Delete {
                    path: format!("{gen_prefix}/{}", ge.path),
                })
                .collect();
            let batch_message = format!("generation GC delete {gen_prefix}");
            // Issue #779 评论 5854082763：GC 遇到 PreconditionFailed（ref 被用户同步推进）
            // 时放弃本轮 GC，返回 Ok — 下次再清，不和用户同步争抢。
            // Issue #779 评论 5854511049：只吞 PreconditionFailed，其他错误（网络/认证/403/
            // Git Database API 失败）照常向上返回，交给 maintenance 层记录，不伪装成成功。
            match provider.commit_batch(&mutations, &batch_message) {
                Ok(_) => {
                    // Issue #993：批量删除成功，移除 orphan observation。
                    if let Some(idx) = orphan_reclaim_idx {
                        orphan_observations.remove(idx);
                        orphan_obs_changed = true;
                        log::info!(
                            "[sync] run_generation_gc: {} generation {} orphan reclaimed — removing observation",
                            project_remote_prefix, gen_id
                        );
                    }
                }
                Err(crate::sync::provider::error::ProviderError::PreconditionFailed {
                    path,
                    reason,
                }) => {
                    log::info!(
                        "[sync] run_generation_gc: {} generation {} commit_batch PreconditionFailed (ref moved by user sync): path={}, reason={} — abandoning GC this round",
                        project_remote_prefix, gen_id, path, reason
                    );
                    return Ok(());
                }
                Err(e) => return Err(crate::Error::from(e)),
            }
            // commit_batch 后检查取消令牌（取消则返回 Ok，已完成的删除保留）。
            if let Some(token) = cancellation_token {
                if token.is_cancelled() {
                    log::info!(
                        "[sync] run_generation_gc: cancellation requested after commit_batch {} — returning Ok",
                        gen_prefix
                    );
                    return Ok(());
                }
            }
        } else {
            for ge in &gen_entries {
                let full_path = format!("{gen_prefix}/{}", ge.path);
                provider
                    .delete(&full_path, DeletePrecondition::Unconditional)
                    .map_err(crate::Error::from)?;
                // Issue #729 评论 5765306162 问题5：每次 provider.delete 返回后检查取消令牌。
                // 取消则返回 Ok(())（已完成的删除保留，未完成的不继续）。
                if let Some(token) = cancellation_token {
                    if token.is_cancelled() {
                        log::info!(
                            "[sync] run_generation_gc: cancellation requested after delete {} — returning Ok",
                            full_path
                        );
                        return Ok(());
                    }
                }
            }
            // Issue #993：非批量逐文件删除全部成功，移除 orphan observation。
            if let Some(idx) = orphan_reclaim_idx {
                orphan_observations.remove(idx);
                orphan_obs_changed = true;
                log::info!(
                    "[sync] run_generation_gc: {} generation {} orphan reclaimed — removing observation",
                    project_remote_prefix, gen_id
                );
            }
        }
    }

    // Issue #993：持久化 orphan observations（首次发现/重新计时/回收移除/stale 清理）。
    if orphan_obs_changed {
        save_orphan_observations(
            provider,
            &gc_meta_path,
            &orphan_observations,
            orphan_obs_prev_version,
        )?;
    }

    Ok(())
}
