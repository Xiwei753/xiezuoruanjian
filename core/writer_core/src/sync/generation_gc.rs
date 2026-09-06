//! #645 评论 5504296097 问题2：generation GC — provider-neutral 清理未引用 generation。
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

use crate::sync::provider::model::DeletePrecondition;
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

/// #645 评论 5504296097 问题2：运行 generation GC。
///
/// 清理 `projects/P/__generations__/` 下未引用的 generation：
///
/// 1. list `projects/P/__generations__/*`，解析每个 generation 的 meta。
/// 2. 当前 `active_generation` 永远不删。
/// 3. incomplete 且 lease 未过期 → 不删（上传中）。
/// 4. unreferenced generation 超过安全保留期（`created_at_ms + retention_ms < now_ms`）
///    后才可删。
/// 5. delete 失败 → `Err`（`RecoverableError` 语义），下轮继续。
/// 6. meta 缺失/损坏的 generation：保守保留（不删），log warn。
///
/// `project_remote_prefix` 如 `"projects/P"`；`active_generation` 为当前 active
/// generation ID（`None` 表示 legacy/无 generation 记录）。
pub fn run_generation_gc(
    provider: &dyn SyncProvider,
    project_remote_prefix: &str,
    active_generation: Option<&str>,
    now_ms: i64,
    retention_ms: i64,
) -> crate::error::Result<()> {
    let generations_prefix = format!("{project_remote_prefix}/__generations__");
    let entries = provider
        .list(&generations_prefix)
        .map_err(crate::Error::from)?;
    // 提取唯一 generation ID segment（__generations__/G/... → G）。
    let mut generation_ids: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in &entries {
        let first_segment = entry.path.split('/').next().unwrap_or("");
        if !first_segment.is_empty() && seen.insert(first_segment.to_string()) {
            generation_ids.push(first_segment.to_string());
        }
    }

    for gen_id in &generation_ids {
        // active_generation 永远不删。
        if Some(gen_id.as_str()) == active_generation {
            continue;
        }

        // 读 meta。
        let meta_path = format!(
            "{}/{}/{}",
            generations_prefix, gen_id, GENERATION_META_FILENAME
        );
        let meta_obj = provider.read(&meta_path).map_err(crate::Error::from)?;
        let meta = match meta_obj {
            Some(obj) => match serde_json::from_slice::<GenerationMeta>(&obj.content) {
                Ok(m) => m,
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
                // meta 缺失：保守保留（不删），log warn。
                log::warn!(
                    "[sync] run_generation_gc: missing meta for {} generation {} — keeping",
                    project_remote_prefix,
                    gen_id
                );
                continue;
            }
        };

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

        // 删 generation prefix 下所有对象。
        log::info!(
            "[sync] run_generation_gc: {} generation {} unreferenced + past retention — deleting",
            project_remote_prefix,
            gen_id
        );
        let gen_prefix = format!("{generations_prefix}/{gen_id}");
        let gen_entries = provider.list(&gen_prefix).map_err(crate::Error::from)?;
        for ge in &gen_entries {
            let full_path = format!("{gen_prefix}/{}", ge.path);
            provider
                .delete(&full_path, DeletePrecondition::Unconditional)
                .map_err(crate::Error::from)?;
        }
    }

    Ok(())
}
