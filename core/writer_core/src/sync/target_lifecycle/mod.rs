//! Target 生命周期 catalog — 远端持久、provider-neutral。
//!
//! catalog 放在不会随 `projects/<id>/` 一起被删除的位置：
//! `app/app-meta/sync/targets.sync.json`（app target 的 remote_prefix 下）。
//!
//! ## 职责
//!
//! - 远端 `targets.sync.json`（本模块）：负责"跨设备都必须知道这个 target 的生命周期"，
//!   离线旧设备上线时先读 catalog，看到 delete tombstone 就不会把旧 project 重新上传。
//! - 本地 `pending_deleted_targets.json`（`pending_deleted` 模块）：负责
//!   "本机删除事务还没同步完成"，本机状态。两个职责不混。
//!
//! ## provider-neutral
//!
//! catalog 只通过 `SyncProvider::read/write` 操作；GitHub SHA、WebDAV ETag 等留在 Provider 里。

use crate::sync::provider::model::{RemoteVersion, WritePrecondition};
use crate::sync::provider::SyncProvider;
use crate::sync::types::{
    RemoteTargetCatalogSnapshot, SyncManifest, TargetLifecycleCatalog, TargetLifecycleRecord,
    TargetOp,
};

/// catalog 在远端的固定路径：app target 的 remote_prefix（`"app"`）下。
///
/// 这个位置不会随 `projects/<id>/` 一起被删除，保证 delete tombstone 持久存在。
pub const TARGET_CATALOG_REMOTE_PATH: &str = "app/app-meta/sync/targets.sync.json";

/// workspace 总 manifest 在远端的固定路径。
///
/// 真实远端仓库（Xiwei753/xiaoshuo）里旧 workspace 概念的全量 records 保存在
/// `app-meta/sync/manifest.sync.json`（不带 `app/` 前缀，与 project 级 manifest
/// 同处 `app-meta/sync/` 命名空间下）。旧的 workspace 概念删除前，全局 manifest
/// 记录所有文件（含 `projects/<id>/...`）。当某个 project 没有自己的
/// `projects/<id>/app-meta/sync/manifest.sync.json` 时，
/// `discover_legacy_remote_catalog` 会从 workspace 总 manifest 中筛
/// `projects/<id>/...` 的 records 推断该 project 的 LWW，避免旧远端作品首次同步直接失败。
pub const WORKSPACE_MANIFEST_REMOTE_PATH: &str = "app-meta/sync/manifest.sync.json";

///   解析当前可见远端 project prefix。
///
/// LiveProject 两步发布的第一步 — 从 catalog record 解析当前可见远端 source：
/// - `Upsert` + `active_generation=Some(G)` → `Ok(Some("projects/P/__generations__/G"))`
///   （`G` 经 [`validate_generation_id`] 校验，防路径穿越）；
/// - `Upsert` + `active_generation=None` → `Ok(Some("projects/P"))`（legacy，无 generation）；
/// - `Delete` → `Ok(None)`（target 已删除，无可见远端）。
///
/// 调用方（LiveProject）用返回的 source prefix 做 只读 merge（下载远端到 staging），
/// 再发布到新 generation。`__generations__` 子目录名与 `full_sync::GENERATION_SUBDIR` 一致。
pub fn resolve_visible_project_prefix(
    record: &TargetLifecycleRecord,
    project_remote_prefix: &str,
) -> crate::error::Result<Option<String>> {
    match record.op {
        TargetOp::Delete => Ok(None),
        TargetOp::Upsert => match &record.active_generation {
            Some(gen) => {
                validate_generation_id(gen)?;
                Ok(Some(format!(
                    "{}/__generations__/{}",
                    project_remote_prefix, gen
                )))
            }
            None => Ok(Some(project_remote_prefix.to_string())),
        },
    }
}

///   统一解析 remote target_id，验证前缀 单段合法 id。
///
/// remote catalog 里的 `target_id` 是远端持久数据，可能损坏或被恶意构造。
/// 直接 `strip_prefix("projects/").unwrap_or_default()` 会把非法记录当成
/// `projects/` 路径拼接，存在路径穿越风险（如 `projects/../app`）。
///
/// 本函数严格校验：
/// - 固定前缀 `projects/`；
/// - 剩余部分只有一个合法 project id segment（不含 `/`、不含 `\`、非 `.`/`..`、非空）；
/// - 通过 [`crate::delete_guard::validate_id_segment`] 复用同一套 ID 验证规则。
///
/// 合法 → 返回 `Ok(project_id)`；非法 → 返回 `Err`。
pub(crate) fn parse_project_target_id(target_id: &str) -> crate::error::Result<String> {
    let rest = target_id.strip_prefix("projects/").ok_or_else(|| {
        crate::Error::Other(format!(
            "parse_project_target_id: missing 'projects/' prefix in {target_id:?}"
        ))
    })?;
    // 必须只有一个 segment — 不含路径分隔符。
    if rest.contains('/') || rest.contains('\\') || rest.is_empty() {
        return Err(crate::Error::Other(format!(
            "parse_project_target_id: invalid project segment in {target_id:?}"
        )));
    }
    // 复用 delete_guard 的 ID 验证（拒绝 `.`、`..`、空、含分隔符）。
    let validated = crate::delete_guard::validate_id_segment(rest)?;
    Ok(validated.to_string())
}

///   校验 generation ID 是合法的单 path segment。
///
/// generation ID 用作 `projects/P/__generations__/G/` 中的 `G` 段，必须不能
/// 越过 generation 目录（空、`.`、`..`、含 `/`、含 `\` 都拒绝）。
/// 复用 [`crate::delete_guard::validate_id_segment`] 同一套 ID 验证规则，
/// 与 [`parse_project_target_id`] 保持一致的路径穿越防护。
///
/// 合法 → `Ok(id)`；非法 → `Err`。
pub(crate) fn validate_generation_id(id: &str) -> crate::error::Result<&str> {
    crate::delete_guard::validate_id_segment(id)
}

///   校验单条 record 的 `active_generation` 合法性。
///
/// - `Delete` 记录不应有 `active_generation`（必须 `None`）；
/// - `Upsert` + `None` → 允许（legacy，无 generation 记录）；
/// - `Upsert` + `Some(G)` → `G` 必须通过 [`validate_generation_id`]（合法单 path segment）。
///
/// 提取为独立 helper 以保持 `validate_catalog` 的嵌套深度在 clippy 阈值内。
fn validate_record_active_generation(record: &TargetLifecycleRecord) -> crate::error::Result<()> {
    match record.op {
        TargetOp::Delete => {
            if record.active_generation.is_some() {
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "validate_catalog: Delete record for {:?} must not have active_generation",
                    record.target_id
                ))));
            }
        }
        TargetOp::Upsert => {
            if let Some(gen) = &record.active_generation {
                if let Err(e) = validate_generation_id(gen) {
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "validate_catalog: invalid active_generation {:?} for target {:?}: {e}",
                        gen, record.target_id
                    ))));
                }
            }
        }
    }
    Ok(())
}

///   校验整个 catalog — 损坏/非法 record 返回错误。
///
/// 校验规则：
/// - `target_id` 合法（`parse_project_target_id` 通过）
/// - `remote_prefix == target_id`
/// - `schema_version` 支持（当前只支持 1）
/// - 同 `target_id` 不重复（合并后唯一）
/// - `Delete` 必须有合法 `deleted_at_ms`
/// -   `active_generation` 合法性
///   - `Delete` 记录不应有 `active_generation`（必须 `None`）；
///   - `Upsert` + `None` → 允许（legacy，无 generation 记录）；
///   - `Upsert` + `Some(G)` → `G` 必须通过 [`validate_generation_id`]（合法单 path segment）。
///
/// 任一不合法 → `Err`，调用方不应在此假 catalog 上继续规划。
fn validate_catalog(catalog: &TargetLifecycleCatalog) -> crate::error::Result<()> {
    use std::collections::HashSet;
    let mut seen_target_ids = HashSet::new();
    for record in &catalog.records {
        // 1. target_id 合法
        if let Err(e) = parse_project_target_id(&record.target_id) {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "validate_catalog: invalid target_id {:?}: {e}",
                record.target_id
            ))));
        }
        // 2. remote_prefix == target_id
        if record.remote_prefix != record.target_id {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "validate_catalog: remote_prefix {:?} != target_id {:?}",
                record.remote_prefix, record.target_id
            ))));
        }
        // 3. schema_version 支持
        if record.schema_version != 1 {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "validate_catalog: unsupported schema_version {} for target {:?}",
                record.schema_version, record.target_id
            ))));
        }
        // 4. 同 target_id 不重复
        if !seen_target_ids.insert(&record.target_id) {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "validate_catalog: duplicate target_id {:?}",
                record.target_id
            ))));
        }
        // 5. Delete 必须有合法 deleted_at_ms
        if record.op == TargetOp::Delete && record.deleted_at_ms.is_none() {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "validate_catalog: Delete record for {:?} missing deleted_at_ms",
                record.target_id
            ))));
        }
        // 6.   active_generation 合法性
        validate_record_active_generation(record)?;
    }
    Ok(())
}

///   加载远端 catalog，返回带版本标识的快照。
///
/// - 远端不存在 catalog → 返回空 catalog + 写入方应用 `CreateNew`；
/// - 解析失败 → 返回 `Err`（不吞错误，调用方决定 Retry）；
/// - catalog 校验失败 → 返回 `Err`（损坏 record 不应被静默隐藏）。
///
/// 返回的 `version` 用于后续 CAS 写入（`IfMatch`），防止多设备并发覆盖。
///
///   本函数是纯只读的（只 `provider.read`，不 `provider.write`）。
/// dry-run 安全调用。`__nonexistent__` version 只是标记"远端不存在"，不会自动写远端 —
/// 只有显式调 [`persist_bootstrap_catalog`] 或 [`write_catalog_once`] 才落盘。
pub fn load_remote_catalog(
    provider: &dyn SyncProvider,
) -> crate::error::Result<RemoteTargetCatalogSnapshot> {
    let obj = provider
        .read(TARGET_CATALOG_REMOTE_PATH)
        .map_err(crate::Error::from)?;
    let Some(obj) = obj else {
        // 文件不存在：首次写应用 CreateNew，版本用 sentinel 表示不存在。
        //   此处不写远端，只返回 sentinel version。
        // dry-run 可以安全调用本函数 — 不会在远端创建 targets.sync.json。
        return Ok(RemoteTargetCatalogSnapshot {
            catalog: TargetLifecycleCatalog::default(),
            version: RemoteVersion::new("__nonexistent__"),
        });
    };
    let version = obj.version.clone();
    let catalog: TargetLifecycleCatalog = serde_json::from_slice(&obj.content).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "load_remote_catalog: parse {}: {e}",
            TARGET_CATALOG_REMOTE_PATH
        )))
    })?;
    //   校验整个 catalog，损坏 record 不应被静默隐藏。
    validate_catalog(&catalog)?;
    Ok(RemoteTargetCatalogSnapshot { catalog, version })
}

///   发现远端 catalog（只读，不写远端）。
///
/// 真做只读 legacy 枚举：
/// 1. 先读 `targets.sync.json`（[`load_remote_catalog`]）。存在 → 直接返回。
/// 2. 不存在 → `provider.list("projects")` 枚举所有 project 前缀。
/// 3. 对每个 project 先读 `projects/<id>/app-meta/sync/manifest.sync.json`
///    （[`read_project_manifest_lww`]），取 manifest 中所有 record 的最大
///    `updated_at_ms` 和对应 `device_id`，合成一条 Upsert `TargetLifecycleRecord`。
///    若 project 级 manifest 不存在（`Ok(None)`），则**按需**加载 workspace 总 manifest
///    （[`load_legacy_workspace_manifest`]，整轮最多一次），并 fallback 到 workspace
///    总 manifest 中 `projects/<id>/...` 的 records
///    （[`read_project_lww_from_workspace_manifest`]）。
/// 4. 返回合成 catalog + `__nonexistent__` version（catalog 文件仍不存在于远端）。
///
/// **绝不**写远端。dry-run 安全调用。正式 sync 在确认 `version == __nonexistent__`
/// 后调 [`persist_bootstrap_catalog`] 把合成 catalog 落盘。
///
/// ## 错误边界（#659）
///
/// - project manifest 存在但损坏 → `Err`（不 fallback 到 workspace manifest）。
/// - project manifest 不存在 + workspace manifest 损坏 → `Err`（仅在真正需要 fallback
///   时才报错；所有 project 都有自己的 project manifest 时不会加载 workspace manifest，
///   workspace manifest 损坏不影响结果）。
/// - project manifest 不存在 + workspace manifest 不存在或无匹配 records → `Err`
///   （两边都失败才报错）。
/// - 非法 project id → `Err`（不 skip，不 fallback）。
///
/// 让真实远端 target 不会静默消失，也不会被伪造的 `(0, "legacy")` LWW 错误地建成合法 record。
pub fn discover_legacy_remote_catalog(
    provider: &dyn SyncProvider,
) -> crate::error::Result<RemoteTargetCatalogSnapshot> {
    // 1. 先读 catalog 文件。
    let snapshot = load_remote_catalog(provider)?;
    if snapshot.version.as_str() != "__nonexistent__" {
        // catalog 存在 → 直接返回，不做 legacy 枚举。
        return Ok(snapshot);
    }

    // 2. catalog 不存在 → legacy 枚举。
    log::info!(
        "[sync] discover_legacy_remote_catalog: targets.sync.json absent, \
         enumerating legacy remote projects"
    );
    let entries = provider.list("projects").map_err(crate::Error::from)?;
    // list("projects") 剥掉 "projects/" 前缀，返回相对路径如 "p1/app-meta/..."。
    // 提取唯一顶层 project id segment。
    let mut project_ids: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in &entries {
        let first_segment = entry.path.split('/').next().unwrap_or("");
        if !first_segment.is_empty() && seen.insert(first_segment.to_string()) {
            project_ids.push(first_segment.to_string());
        }
    }
    project_ids.sort();

    // 3. 对每个 project 读 manifest，合成 Upsert record。
    // 非法 project id 不 skip，直接返回 Err。
    // project manifest 损坏（read_project_manifest_lww 返回 Err）直接传播。
    // workspace 总 manifest **按需**加载：只有某个 project 缺 project manifest 时才加载，
    // 整轮最多一次（workspace_manifest_loaded 标志 + workspace_manifest 缓存）。
    // workspace manifest 损坏只在真正需要 fallback 时才导致 Err；所有 project 都有自己的
    // project manifest 时永远不会加载 workspace manifest，workspace manifest 损坏不影响结果。
    let mut records = Vec::with_capacity(project_ids.len());
    let mut workspace_manifest_loaded = false;
    let mut workspace_manifest: Option<SyncManifest> = None;
    for project_id in &project_ids {
        let target_id = format!("projects/{project_id}");
        // 校验 project id segment（防路径穿越）。非法 → Err（不 skip）。
        parse_project_target_id(&target_id)?;
        let (updated_at_ms, device_id) = match read_project_manifest_lww(provider, project_id)? {
            Some(lww) => lww,
            None => {
                // project manifest 不存在 → 按需加载 workspace 总 manifest（整轮最多一次）。
                if !workspace_manifest_loaded {
                    workspace_manifest = load_legacy_workspace_manifest(provider)?;
                    workspace_manifest_loaded = true;
                }
                let lww_opt = match workspace_manifest.as_ref() {
                    Some(manifest) => {
                        read_project_lww_from_workspace_manifest(manifest, project_id)?
                    }
                    None => None,
                };
                lww_opt.ok_or_else(|| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "read_legacy_project_lww: manifest not found for project {project_id} \
                         — cannot fabricate LWW"
                    )))
                })?
            }
        };
        records.push(TargetLifecycleRecord::upsert(
            &target_id,
            &target_id,
            updated_at_ms,
            &device_id,
        ));
    }

    let catalog = TargetLifecycleCatalog { records };
    // version 仍是 __nonexistent__ — catalog 文件不在远端。
    Ok(RemoteTargetCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("__nonexistent__"),
    })
}

///   只读加载 workspace 总 manifest（`app-meta/sync/manifest.sync.json`）。
///
/// 用于 [`discover_legacy_remote_catalog`] 在 project 级 manifest 缺失时 fallback。
///
/// - workspace manifest 不存在 → `Ok(None)`（workspace manifest 不存在是正常的，
///   旧远端可能从未写过 workspace 概念的 manifest）；
/// - 存在但解析失败 → `Err`（损坏的 manifest 不应被静默隐藏）；
/// - 存在且合法 → `Ok(Some(manifest))`。
///
/// 本函数是纯只读的（只 `provider.read`，不 `provider.write`），dry-run 安全调用。
fn load_legacy_workspace_manifest(
    provider: &dyn SyncProvider,
) -> crate::error::Result<Option<SyncManifest>> {
    let obj = provider
        .read(WORKSPACE_MANIFEST_REMOTE_PATH)
        .map_err(crate::Error::from)?;
    let Some(obj) = obj else {
        // workspace manifest 不存在是正常的 — 旧远端可能从未写过 workspace manifest。
        return Ok(None);
    };
    let manifest: SyncManifest = serde_json::from_slice(&obj.content).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "load_legacy_workspace_manifest: parse {}: {e}",
            WORKSPACE_MANIFEST_REMOTE_PATH
        )))
    })?;
    Ok(Some(manifest))
}

///   从 workspace 总 manifest 中筛 `projects/<id>/...` 的 records，取 LWW。
///
/// 用于 [`discover_legacy_remote_catalog`] 在 project 级 manifest 缺失时 fallback。
///
/// - 无匹配 records（`path` 不以 `projects/<id>/` 开头）→ `Ok(None)`；
/// - 有匹配 records → 按现有 LWW 规则（`deleted_at_ms` 优先 for delete op，
///   否则 `updated_at_ms`；时间相同 `device_id` 字典序大者胜出）取最大
///   `(timestamp, device_id)`，返回 `Ok(Some((timestamp, device_id)))`。
///
/// LWW 规则与 [`read_project_manifest_lww`] 中 project 级 manifest 的 max_by 逻辑一致，
/// 保证 fallback 与 project 级 manifest 行为一致。
fn read_project_lww_from_workspace_manifest(
    manifest: &SyncManifest,
    project_id: &str,
) -> crate::error::Result<Option<(i64, String)>> {
    let prefix = format!("projects/{project_id}/");
    // 筛 path 以 projects/<id>/ 开头的 records，按 LWW 规则取最大值。
    let winner = manifest
        .files
        .iter()
        .filter(|f| f.path.starts_with(&prefix))
        .max_by(|a, b| {
            let a_time = match a.deleted_at_ms {
                Some(t) if a.op == "delete" => t,
                _ => a.updated_at_ms,
            };
            let b_time = match b.deleted_at_ms {
                Some(t) if b.op == "delete" => t,
                _ => b.updated_at_ms,
            };
            a_time
                .cmp(&b_time)
                .then_with(|| a.device_id.cmp(&b.device_id))
        });
    let Some(winner) = winner else {
        // workspace manifest 中无该 project 的 records → None（调用方决定是否 Err）。
        return Ok(None);
    };
    let winner_time = match winner.deleted_at_ms {
        Some(t) if winner.op == "delete" => t,
        _ => winner.updated_at_ms,
    };
    Ok(Some((winner_time, winner.device_id.clone())))
}

/// 读 project 级 manifest，提取 LWW 时间和 device_id。
///
/// 远端 manifest 路径：`projects/<id>/app-meta/sync/manifest.sync.json`。
///
/// 与 [`read_project_lww_from_workspace_manifest`] 的区别：本函数只读 project 级 manifest，
/// 不 fallback 到 workspace 总 manifest。workspace manifest 的按需加载由调用方
/// （[`discover_legacy_remote_catalog`]）负责。
///
/// # 返回
///
/// - project manifest 存在且合法 → `Ok(Some((lww_time, device_id)))`（取所有 file record
///   的最大 `(lww_time, device_id)`，LWW 规则：`deleted_at_ms` 优先 for delete op，
///   否则 `updated_at_ms`；时间相同 `device_id` 字典序大者胜出）；
/// - project manifest 不存在 → `Ok(None)`（调用方决定是否 fallback 到 workspace manifest）；
/// - project manifest 存在但解析失败 → `Err`（损坏的 project manifest 不应被静默隐藏，
///   不 fallback 到 workspace manifest）；
/// - project manifest 存在但 files 为空 → `Err`（空 manifest 无法可靠判断该 project 的真实 LWW）。
fn read_project_manifest_lww(
    provider: &dyn SyncProvider,
    project_id: &str,
) -> crate::error::Result<Option<(i64, String)>> {
    let manifest_path = format!("projects/{project_id}/app-meta/sync/manifest.sync.json");
    let obj = provider.read(&manifest_path).map_err(crate::Error::from)?;
    let Some(obj) = obj else {
        // project 级 manifest 不存在 → Ok(None)，调用方按需 fallback 到 workspace 总 manifest。
        return Ok(None);
    };
    let manifest: SyncManifest = serde_json::from_slice(&obj.content).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "read_project_manifest_lww: parse manifest for {project_id} failed: {e} \
             — cannot fabricate LWW"
        )))
    })?;
    // 取所有 record 的最大 (lww_time, device_id)。
    // manifest 存在但 files 为空 → 返回 Err（不伪造 (0, "") LWW）。
    // 空 manifest 无法可靠判断该 project 的真实 LWW — 调用方（discover_legacy_remote_catalog）
    // 应让整个 bootstrap 返回 RecoverableError，不写 targets.sync.json，
    // 不把这个 Project 写成合法 Upsert。
    let winner = manifest
        .files
        .iter()
        .max_by(|a, b| {
            let a_time = match a.deleted_at_ms {
                Some(t) if a.op == "delete" => t,
                _ => a.updated_at_ms,
            };
            let b_time = match b.deleted_at_ms {
                Some(t) if b.op == "delete" => t,
                _ => b.updated_at_ms,
            };
            a_time
                .cmp(&b_time)
                .then_with(|| a.device_id.cmp(&b.device_id))
        })
        .ok_or_else(|| {
            crate::Error::Io(std::io::Error::other(format!(
            "read_project_manifest_lww: legacy manifest for project {project_id} has no LWW records \
             — cannot fabricate LWW"
        )))
        })?;
    let winner_time = match winner.deleted_at_ms {
        Some(t) if winner.op == "delete" => t,
        _ => winner.updated_at_ms,
    };
    Ok(Some((winner_time, winner.device_id.clone())))
}

///   持久化 bootstrap catalog（正式 sync 才调用）。
///
/// 当 `snapshot.version` 为 `__nonexistent__` 时用 `CreateNew` 首次写入远端。
/// dry-run **不**调本函数 — 只用 [`discover_legacy_remote_catalog`] 发现 catalog，
/// 把内存 catalog 传 planner，绝不写远端。
pub fn persist_bootstrap_catalog(
    provider: &dyn SyncProvider,
    catalog: &TargetLifecycleCatalog,
    version: &RemoteVersion,
) -> crate::error::Result<RemoteTargetCatalogSnapshot> {
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog: catalog.clone(),
        version: version.clone(),
    };
    write_catalog_once(provider, &snapshot)
}

///   CAS 写远端 catalog，返回持久化后的完整 snapshot。
///
/// 使用 `WritePrecondition::IfMatch(version)` 防止多设备并发覆盖。
/// `PreconditionFailed` 时自动重读远端 catalog、LWW 合并本地变更、再 IfMatch 写入。
/// 重试次数限制为 3（超过视为并发冲突过于激烈，返回 `Err`）。
///
/// `snapshot.version` 为 `__nonexistent__` 时使用 `CreateNew`（首次写入）。
/// 序列化失败或 provider.write 失败 → `Err`。
///
///   返回 `RemoteTargetCatalogSnapshot`（实际持久化后的完整
/// catalog + version），调用方必须用返回值更新本地 catalog 和 version，避免
/// "version 是新的、内容还是旧的"非法组合。
///
/// 保留给非 lifecycle 决策的批量 catalog 写入（如初始化）。lifecycle 原子决策
/// 用 [`write_catalog_once`] + [`apply_lifecycle_record`]，不在内部吞 CAS 冲突。
pub fn write_remote_catalog(
    provider: &dyn SyncProvider,
    snapshot: &RemoteTargetCatalogSnapshot,
) -> crate::error::Result<RemoteTargetCatalogSnapshot> {
    let mut current_version = snapshot.version.clone();
    let mut current_catalog = snapshot.catalog.clone();
    let max_retries = 3;

    for attempt in 0..max_retries {
        let content = serde_json::to_vec(&current_catalog).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "write_remote_catalog: serialize: {e}"
            )))
        })?;

        let precondition = if current_version.as_str() == "__nonexistent__" {
            WritePrecondition::CreateNew
        } else {
            WritePrecondition::IfMatch(current_version.clone())
        };

        match provider.write(TARGET_CATALOG_REMOTE_PATH, &content, precondition) {
            Ok(new_version) => {
                log::debug!(
                    "[sync] write_remote_catalog: succeeded (attempt={})",
                    attempt + 1
                );
                //   返回实际持久化后的完整 snapshot。
                // provider.write 成功后远端内容就是 current_catalog，version 是 new_version。
                return Ok(RemoteTargetCatalogSnapshot {
                    catalog: current_catalog.clone(),
                    version: new_version,
                });
            }
            Err(crate::sync::provider::error::ProviderError::PreconditionFailed { .. }) => {
                //   CAS 冲突 → 重读远端最新 catalog，
                // LWW 合并本地变更后重试。
                log::info!(
                    "[sync] write_remote_catalog: PreconditionFailed (attempt={}), \
                     re-reading and merging",
                    attempt + 1
                );
                let reloaded = load_remote_catalog(provider)?;
                current_catalog = merge_catalogs(&[reloaded.catalog, current_catalog]);
                current_version = reloaded.version;
            }
            Err(e) => {
                return Err(crate::Error::from(e));
            }
        }
    }

    Err(crate::Error::Io(std::io::Error::other(format!(
        "write_remote_catalog: CAS retry exhausted after {max_retries} attempts"
    ))))
}

///   单次 CAS 原语 — 只做一次 CreateNew/IfMatch 写入。
///
/// 与 [`write_remote_catalog`] 的关键区别：`PreconditionFailed` **原样返回**，
/// 不在内部重读/merge/重试。调用方（[`apply_lifecycle_record`]）负责在 CAS 冲突后
/// 重新加载最新 snapshot、重新做 target-level LWW 决策、决定是否再次 CAS。
///
/// 这样外层能看到真实的 CAS 冲突，不会把"远端已有更新记录"误判为"我方 Applied"。
///
/// `snapshot.version` 为 `__nonexistent__` 时使用 `CreateNew`（首次写入）。
/// 成功返回持久化后的完整 snapshot；序列化失败或非 CAS 的 provider.write 失败 → `Err`。
pub fn write_catalog_once(
    provider: &dyn SyncProvider,
    snapshot: &RemoteTargetCatalogSnapshot,
) -> crate::error::Result<RemoteTargetCatalogSnapshot> {
    let content = serde_json::to_vec(&snapshot.catalog).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "write_catalog_once: serialize: {e}"
        )))
    })?;

    let precondition = if snapshot.version.as_str() == "__nonexistent__" {
        WritePrecondition::CreateNew
    } else {
        WritePrecondition::IfMatch(snapshot.version.clone())
    };

    let new_version = provider
        .write(TARGET_CATALOG_REMOTE_PATH, &content, precondition)
        .map_err(crate::Error::from)?;
    log::debug!("[sync] write_catalog_once: succeeded");
    Ok(RemoteTargetCatalogSnapshot {
        catalog: snapshot.catalog.clone(),
        version: new_version,
    })
}

///   provider-neutral 原子决策接口。
///
/// 把一条 candidate lifecycle record 通过 CAS 写入远端 catalog。每次 CAS 冲突后：
/// 重读最新 snapshot → candidate 与最新 remote record 重新做 target-level LWW：
/// - candidate 严格赢 → merge → IfMatch 再写 → `Applied`；
/// - candidate 与 remote 完全相等 → `AlreadyCurrent`（不写 catalog，调用方按 candidate.op 继续）；
/// - remote 严格赢 → `RemoteWinner { record }`（携带真实赢的 record，调用方按 record.op 决策）；
/// - Retry → 不删任何远端文件 → pending 保留。
///
/// 不再用含糊的 `LostToRemote(snapshot)` 让调用方猜 op 反转。
/// 完全相等的 record 返回 `AlreadyCurrent`，远端严格赢返回 `RemoteWinner { record }`，
/// 调用方按真实 `record.op` 走对应路径，避免 LWW 相等时误判为"远端 delete 赢"或"远端 upsert 赢"。
///
///   用 [`write_catalog_once`] 单次 CAS 原语，
/// 不再用 [`write_remote_catalog`] 的内部 retry（会把 PreconditionFailed 吞成 Ok，
/// 外层看不到冲突误判 Applied）。CAS 冲突由本函数重读 snapshot + 重新判定 winner 处理。
#[allow(clippy::excessive_nesting, clippy::too_many_lines)]
pub fn apply_lifecycle_record(
    provider: &dyn SyncProvider,
    snapshot: &RemoteTargetCatalogSnapshot,
    candidate: TargetLifecycleRecord,
) -> crate::sync::types::TargetLifecycleApplyResult {
    use crate::sync::types::TargetLifecycleApplyResult;

    let max_retries = 3;
    let mut current_snapshot = snapshot.clone();
    let current_candidate = candidate.clone();

    for attempt in 0..max_retries {
        // 1. 检查 candidate 与当前 remote record 的 LWW 关系。
        let remote_record = find_record(&current_snapshot.catalog, &current_candidate.target_id);
        let candidate_wins = match &remote_record {
            None => true, // 远端无记录，candidate 胜出
            Some(existing) => lww_record_wins(&current_candidate, existing),
        };

        if !candidate_wins {
            // remote 不输给 candidate — 可能完全相等或严格赢。
            let Some(existing) = remote_record else {
                // candidate_wins == false 蕴含 remote_record.is_some()，防御性 Retry。
                return TargetLifecycleApplyResult::Retry(crate::Error::Io(std::io::Error::other(
                    "apply_lifecycle_record: invariant violation — candidate_wins=false but remote_record=None",
                )));
            };
            // 完全相等 → AlreadyCurrent；
            // 远端严格赢 → RemoteWinner { record: existing }（携带真实 op）。
            if records_equal(&current_candidate, existing) {
                log::info!(
                    "[sync] apply_lifecycle_record: AlreadyCurrent target={} — \
                     candidate identical to remote record",
                    current_candidate.target_id
                );
                return TargetLifecycleApplyResult::AlreadyCurrent(current_snapshot.clone());
            }
            log::info!(
                "[sync] apply_lifecycle_record: RemoteWinner target={} \
                 candidate_time={} remote_time={} remote_op={:?} — aborting write",
                current_candidate.target_id,
                record_lww_time(&current_candidate),
                record_lww_time(existing),
                existing.op,
            );
            return TargetLifecycleApplyResult::RemoteWinner {
                snapshot: current_snapshot.clone(),
                record: existing.clone(),
            };
        }

        // 2. candidate 仍赢 → merge → write_catalog_once 单次 CAS。
        let mut merged_catalog = current_snapshot.catalog.clone();
        upsert_record(&mut merged_catalog, current_candidate.clone());
        let write_snapshot = RemoteTargetCatalogSnapshot {
            catalog: merged_catalog,
            version: current_snapshot.version.clone(),
        };

        match write_catalog_once(provider, &write_snapshot) {
            Ok(persisted) => {
                //   验证持久化后的该 target_id record
                // 确实就是 candidate winner，不能只是"写请求成功"。
                let persisted_rec = find_record(&persisted.catalog, &current_candidate.target_id);
                let candidate_persisted = persisted_rec
                    .map(|rec| records_equal(rec, &current_candidate))
                    .unwrap_or(false);
                if candidate_persisted {
                    log::debug!(
                        "[sync] apply_lifecycle_record: Applied (attempt={}) target={}",
                        attempt + 1,
                        current_candidate.target_id
                    );
                    return TargetLifecycleApplyResult::Applied(persisted);
                }
                // 持久化后该 target_id record 不是 candidate winner → 远端已有更新。
                let remote_rec = persisted_rec.cloned().unwrap_or_else(|| {
                    // 防御性：远端无 record 不应发生（CAS 写成功），构造 Retry。
                    TargetLifecycleRecord::upsert(
                        &current_candidate.target_id,
                        &current_candidate.target_id,
                        0,
                        "",
                    )
                });
                log::info!(
                    "[sync] apply_lifecycle_record: RemoteWinner after CAS target={} \
                     — persisted record differs from candidate",
                    current_candidate.target_id
                );
                return TargetLifecycleApplyResult::RemoteWinner {
                    snapshot: persisted,
                    record: remote_rec,
                };
            }
            Err(crate::Error::SyncRemoteError { category, .. })
                if category == "precondition_failed" =>
            {
                // CAS 冲突 → 重读最新 snapshot，重新判定 winner。
                log::info!(
                    "[sync] apply_lifecycle_record: CAS conflict (attempt={}), \
                     re-reading snapshot",
                    attempt + 1
                );
                match load_remote_catalog(provider) {
                    Ok(reloaded) => {
                        current_snapshot = reloaded;
                        // candidate 不变，下一轮重新与最新 remote record 比较。
                    }
                    Err(e) => {
                        return TargetLifecycleApplyResult::Retry(e);
                    }
                }
            }
            Err(e) => {
                return TargetLifecycleApplyResult::Retry(e);
            }
        }
    }

    TargetLifecycleApplyResult::Retry(crate::Error::Io(std::io::Error::other(format!(
        "apply_lifecycle_record: CAS retry exhausted after {max_retries} attempts for target={}",
        candidate.target_id
    ))))
}

/// 判断两条 record 是否完全相等
/// （同 op / 同 lww_time / 同 device_id / 同 target_id / 同 remote_prefix /
/// 同 active_generation）。
///
/// 用于 `apply_lifecycle_record` 区分 `AlreadyCurrent`（完全相等）和 `RemoteWinner`（远端严格赢）。
///   active_generation 也参与相等判断 — 两条 Upsert 只有
/// 指向同一 generation 才算完全相等，否则 candidate 仍需 CAS 写入新 active_generation。
fn records_equal(a: &TargetLifecycleRecord, b: &TargetLifecycleRecord) -> bool {
    a.target_id == b.target_id
        && a.remote_prefix == b.remote_prefix
        && a.op == b.op
        && a.device_id == b.device_id
        && a.active_generation == b.active_generation
        && record_lww_time(a) == record_lww_time(b)
}

/// 按 `target_id` upsert 一条记录（同 `target_id` 替换，否则追加）。
///
/// 注意：这是纯结构操作，不做 LWW 合并。LWW 合并在 [`merge_catalogs`] 里做。
/// 调用方应确保传入的 `record` 是该 `target_id` 的最新版本（已与现有记录做过 LWW 比较）。
pub fn upsert_record(catalog: &mut TargetLifecycleCatalog, record: TargetLifecycleRecord) {
    if let Some(existing) = catalog
        .records
        .iter_mut()
        .find(|r| r.target_id == record.target_id)
    {
        *existing = record;
    } else {
        catalog.records.push(record);
    }
}

/// 计算记录的 LWW 时间（delete 用 `deleted_at_ms`，upsert 用 `updated_at_ms`）。
///
/// 与 `sync::lww::manifest::lww_record_time` 同语义。
pub fn record_lww_time(record: &TargetLifecycleRecord) -> i64 {
    match record.op {
        TargetOp::Delete => record.deleted_at_ms.unwrap_or(record.updated_at_ms),
        TargetOp::Upsert => record.updated_at_ms,
    }
}

/// LWW 合并多个 catalog：按 `target_id` 分组，取 `(lww_time, device_id)` 最大的记录。
///
/// 与 `resolve_lww_path` 同规则：时间大的胜出，时间相同 `device_id` 字典序大的胜出。
/// 合并后 `records` 按 `target_id` 字典序排序，保证序列化稳定。
pub fn merge_catalogs(catalogs: &[TargetLifecycleCatalog]) -> TargetLifecycleCatalog {
    use std::collections::HashMap;
    let mut by_target: HashMap<String, TargetLifecycleRecord> = HashMap::new();
    for catalog in catalogs {
        for record in &catalog.records {
            // 保留 LWW 胜者：若现有记录更新则保留现有，否则用新记录覆盖。
            let winner = match by_target.get(&record.target_id) {
                Some(existing) if !lww_record_wins(record, existing) => existing.clone(),
                _ => record.clone(),
            };
            by_target.insert(record.target_id.clone(), winner);
        }
    }
    let mut records: Vec<TargetLifecycleRecord> = by_target.into_values().collect();
    records.sort_by(|a, b| a.target_id.cmp(&b.target_id));
    TargetLifecycleCatalog { records }
}

/// `candidate` 是否 LWW 胜过 `existing`（与 `resolve_lww_path` 同规则）。
///
/// 时间大的胜出；时间相同 `device_id` 字典序大的胜出。
fn lww_record_wins(candidate: &TargetLifecycleRecord, existing: &TargetLifecycleRecord) -> bool {
    let existing_time = record_lww_time(existing);
    let candidate_time = record_lww_time(candidate);
    if candidate_time > existing_time {
        true
    } else if candidate_time == existing_time {
        candidate.device_id > existing.device_id
    } else {
        false
    }
}

/// 查找指定 `target_id` 的记录。
pub fn find_record<'a>(
    catalog: &'a TargetLifecycleCatalog,
    target_id: &str,
) -> Option<&'a TargetLifecycleRecord> {
    catalog.records.iter().find(|r| r.target_id == target_id)
}

/// 判断 catalog 中该 target 是否有 upsert 记录（target 存在过/仍存在）。
pub fn catalog_has_upsert(catalog: &TargetLifecycleCatalog, target_id: &str) -> bool {
    find_record(catalog, target_id)
        .map(|r| r.op == TargetOp::Upsert)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests;
