//! 画布节点对象的统一版本感知迁移/解码入口（Issue #1062 二.1）。
//!
//! # 背景
//!
//! Issue #1060 把画布节点拆成两层：
//!
//! - 画布节点文件 `starmaps/<starmap_id>/nodes/<bucket>/<instance_id>.json`
//!   只保存 [`StarMapNodeInstance`]（实例身份、位置、画布归属）；
//! - 共享素材本体 [`StarMapNodeDefinition`] 保存在
//!   `starmaps/library/nodes/<bucket>/<asset_id>.json`。
//!
//! 旧数据里的画布节点文件仍是旧版 [`StarMapNode`]（title/kind/content/anchors/
//! portal/style/position…，没有 `assetId`）。旧端也可能已写出只含
//! `instanceId`/`position` 的实例文件（同样缺 `assetId`）：两种输入都先按
//! 对象形状进入实例/旧节点解码，`assetId` 缺失由稳定身份解析补全。本模块是
//! 唯一的解码入口：新格式实例、旧格式完整节点、素材库共同基线、同步 incoming
//! 全部走这里，输出一个完整的对象级计划 [`NodeObjectPlan`]：
//!
//! - `definition`：需要写进素材库的共享本体（已有同版本/更新版本本体时为 `None`）；
//! - `instance`：画布节点文件应保存的实例（含解析出的稳定 `assetId`）；
//! - `rewrite_instance`：节点文件需要从旧格式重写成实例格式；
//! - `legacy_tags`：旧节点携带、需要先迁出到 annotations 仓库的标签。
//!
//! # 稳定身份解析顺序（实例是身份权威）
//!
//! 1. 实例已有的非空 `assetId`：直接采用，派生反向索引从实例重建；
//! 2. 已确认实例反向绑定（`starmaps/library/references.json`）；
//! 3. 素材库里与对象 id 相同的旧节点本体；
//! 4. 旧节点 id 本身。
//!
//! 绝不生成随机 ID，也绝不因为"找不到本体"拒绝一个只有位置、本体尚未到达的
//! 实例：实例允许先落盘，加载时按
//! [`crate::starmap::types::NodeDefinitionState::ProvisionalMissingDefinition`]
//! 呈现未解析节点，等真实本体到达后自动变成已解析。反向索引是派生视图，
//! 不能反向否定实例事实。
//!
//! # 共享内容按对象版本合并
//!
//! 同一 asset id 已有本体时不再把"内容不同"当成身份冲突：按本体 revision
//! 比较，较新的版本胜出（相同 revision 保留已有本体）。旧节点原文在事务
//! 完成前始终保留，失败可安全重试。
//!
//! # 幂等
//!
//! 新格式实例不做任何改写；旧节点迁移后再次解码得到相同计划（第二次
//! `rewrite_instance=false`、`definition=None`）。写入顺序固定为"先标签、
//! 再本体、后实例"，任何一步失败都可以安全重试。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{Error, Result};
use crate::starmap::package_storage;
use crate::starmap::semantic::{
    StarMapNodeContent, StarMapProvenance, StarMapReviewStatus, StarMapSourceKind,
};
use crate::starmap::types::{
    StarMapNode, StarMapNodeDefinition, StarMapNodeInstance, StarMapNodeKind, StarMapNodeReference,
    StarMapNodeStyle, StarMapReferenceIndex,
};

/// 旧节点文件里尚未迁移的标签字段名（Issue #1060 已迁到 annotations 关系仓库）。
const LEGACY_TAGS_FIELD: &str = "tags";

/// 统一解码后的节点对象迁移计划。
#[derive(Debug, Clone)]
pub(crate) struct NodeObjectPlan {
    /// 需要写入素材库的共享本体；`None` 表示素材库已有同内容/更新版本本体可复用。
    pub definition: Option<StarMapNodeDefinition>,
    /// 画布节点文件应保存的实例。
    pub instance: StarMapNodeInstance,
    /// 计划携带了新的共享本体（等价于 `definition.is_some()`）。
    pub wrote_definition: bool,
    /// 输入是旧格式完整节点（或缺少 `assetId` 的实例），节点文件必须重写成实例。
    pub rewrite_instance: bool,
    /// 旧节点文件里尚未迁出的标签（`plan.instance.asset_id` 的标签）。
    ///
    /// 重写实例会丢掉旧 `tags` 字段，因此落盘前必须先把它们写进 annotations
    /// 关系仓库；迁移失败只影响派生标签关系，不阻断实例本身落盘。
    pub legacy_tags: Vec<String>,
}

impl NodeObjectPlan {
    /// 画布节点实例文件的规范 JSON bytes。
    pub(crate) fn instance_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec_pretty(&self.instance)?)
    }
}

/// `references.json` 对某个实例反向绑定的状态。
///
/// 反向索引是派生视图：`assetId` 不一致说明索引落后于实例事实，按实例重建
/// 即可，不能反向否定权威实例，也不能阻断加载或同步。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReferenceBindingState {
    /// 索引里没有该 `(starmap_id, instance_id)` 记录（派生索引待修复）。
    Missing,
    /// 记录存在但 `assetId` 与实例不一致（派生索引落后，按实例重建）。
    Stale { reference_asset_id: String },
    /// 记录存在且资产一致，但 `projectId` 与实例不一致（派生字段待补齐）。
    ProjectStale,
    /// 记录存在且与实例完整一致。
    Consistent,
}

impl ReferenceBindingState {
    /// 派生索引是否需要按实例事实重建/补齐。
    pub(crate) fn needs_repair(&self) -> bool {
        !matches!(self, Self::Consistent)
    }
}

/// 解码时可用的素材库视图。
///
/// 正常加载路径只依赖 `app_data_root`；同步合并需要把 base/staging 里的
/// incoming 本体也算进来，避免"远端刚加入本体、本地还没落盘"被误判成缺失。
#[derive(Debug, Clone, Default)]
pub(crate) struct NodeObjectBindings {
    /// `(starmap_id, instance_id) -> 反向引用记录`（来自 references.json）。
    instance_references: HashMap<(String, String), StarMapNodeReference>,
    /// 额外本体根目录（按顺序在 `app_data_root` 之后查找）。
    definition_roots: Vec<PathBuf>,
}

impl NodeObjectBindings {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 从已读出的反向索引构造视图。
    pub(crate) fn from_reference_index(index: StarMapReferenceIndex) -> Self {
        let mut bindings = Self::new();
        for record in index.nodes {
            bindings.remember_reference(record);
        }
        bindings
    }

    /// 从 `starmaps/library/references.json` 载入实例反向绑定。
    ///
    /// 文件不存在视为空索引；索引损坏/版本不支持返回 `Err`（身份不可靠时
    /// 不静默猜测）。`references.json` 是派生缓存，但它是"已有实例绑定"
    /// 这条身份解析规则的唯一事实来源。
    pub(crate) fn load_reference_bindings(app_data_root: &Path) -> Result<Self> {
        Ok(Self::from_reference_index(
            crate::starmap::library::load_reference_index(app_data_root)?,
        ))
    }

    /// 记入一条已确认的实例反向绑定，保持本次加载内的视图与磁盘一致。
    pub(crate) fn remember_reference(&mut self, reference: StarMapNodeReference) {
        self.instance_references.insert(
            (reference.starmap_id.clone(), reference.instance_id.clone()),
            reference,
        );
    }

    /// 检查实例与 `references.json` 的绑定是否完整一致。
    ///
    /// 缺失/`projectId` 落后/资产身份不同都只是派生索引待修复；调用方按实例
    /// 事实重建索引即可。
    pub(crate) fn reference_binding_state(
        &self,
        instance: &StarMapNodeInstance,
    ) -> ReferenceBindingState {
        let Some(reference) = self
            .instance_references
            .get(&(instance.starmap_id.clone(), instance.instance_id.clone()))
        else {
            return ReferenceBindingState::Missing;
        };
        if reference.asset_id != instance.asset_id {
            return ReferenceBindingState::Stale {
                reference_asset_id: reference.asset_id.clone(),
            };
        }
        let project_stale = match (&reference.project_id, &instance.project_id) {
            (Some(reference_project), Some(instance_project)) => {
                reference_project != instance_project
            }
            // 实例没有项目归属时，索引里的归属是唯一来源，必须保留。
            (None, Some(_)) => true,
            _ => false,
        };
        if project_stale {
            ReferenceBindingState::ProjectStale
        } else {
            ReferenceBindingState::Consistent
        }
    }

    /// 增加一个额外本体根目录（如 full-sync 的 staging/base 根）。
    pub(crate) fn with_definition_root(mut self, root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        if !self.definition_roots.contains(&root) {
            self.definition_roots.push(root);
        }
        self
    }

    /// 登记一条实例反向绑定（同步计划内的实例身份）。
    #[cfg(test)]
    pub(crate) fn insert_instance_reference(
        &mut self,
        starmap_id: &str,
        instance_id: &str,
        asset_id: &str,
        project_id: Option<&str>,
    ) {
        self.remember_reference(StarMapNodeReference {
            asset_id: asset_id.to_string(),
            starmap_id: starmap_id.to_string(),
            instance_id: instance_id.to_string(),
            project_id: project_id.map(str::to_string),
        });
    }

    fn instance_asset(&self, starmap_id: &str, instance_id: &str) -> Option<&str> {
        self.instance_references
            .get(&(starmap_id.to_string(), instance_id.to_string()))
            .map(|record| record.asset_id.as_str())
    }

    fn instance_project_id(&self, starmap_id: &str, instance_id: &str) -> Option<&str> {
        self.instance_references
            .get(&(starmap_id.to_string(), instance_id.to_string()))
            .and_then(|record| record.project_id.as_deref())
    }
}

/// 统一解码入口：新格式实例 / 旧格式完整节点都从这里产出对象级计划。
///
/// `app_data_root` 是素材库所在根；`existing_bindings` 可选，用于同步合并时
/// 带上 base/incoming 的素材库视图。纯解码，**不写任何文件**；需要落盘时
/// 调用 [`persist_node_object_plan`]，顺序固定为"先标签、再本体、后实例"。
///
/// 这是唯一的解码入口：加载、同步落盘、搜索抽取、素材库发布都走这里，
/// 不再有"校验模式/纯内存模式"第二套解码。
pub(crate) fn decode_node_object(
    app_data_root: &Path,
    starmap_id: &str,
    object_id: &str,
    bytes: &[u8],
    existing_bindings: Option<&NodeObjectBindings>,
) -> Result<NodeObjectPlan> {
    decode_node_object_impl(
        Some(app_data_root),
        starmap_id,
        object_id,
        bytes,
        existing_bindings,
    )
}

fn decode_node_object_impl(
    app_data_root: Option<&Path>,
    starmap_id: &str,
    object_id: &str,
    bytes: &[u8],
    existing_bindings: Option<&NodeObjectBindings>,
) -> Result<NodeObjectPlan> {
    let node_path = node_rel_path_string(starmap_id, object_id);
    // 先按新格式实例解析。旧完整节点没有 `instanceId`，不会命中这个分支；
    // `StarMapNodeInstance::asset_id` 带 `#[serde(default)]`，旧端只写
    // `{instanceId, position}` 的实例也能先进入 `decode_instance`，再由
    // 反向绑定 / 旧节点 id 补全稳定身份，而不是在这里退化成
    // `missing field assetId` 的解析错误。
    match serde_json::from_slice::<StarMapNodeInstance>(bytes) {
        Ok(instance) => decode_instance(
            app_data_root,
            starmap_id,
            object_id,
            &node_path,
            instance,
            existing_bindings,
        ),
        Err(instance_error) => match serde_json::from_slice::<StarMapNode>(bytes) {
            Ok(node) => decode_legacy_node(
                app_data_root,
                starmap_id,
                object_id,
                &node_path,
                bytes,
                node,
                existing_bindings,
            ),
            Err(legacy_error) => Err(Error::Other(format!(
                "starmap node object is neither a node instance nor a legacy node: \
                 path={node_path} object_type=node object_id={object_id} \
                 starmap_id={starmap_id} instance_error={instance_error} \
                 legacy_error={legacy_error}"
            ))),
        },
    }
}

/// 解码新格式实例。
fn decode_instance(
    app_data_root: Option<&Path>,
    starmap_id: &str,
    object_id: &str,
    node_path: &str,
    mut instance: StarMapNodeInstance,
    existing_bindings: Option<&NodeObjectBindings>,
) -> Result<NodeObjectPlan> {
    if instance.instance_id != object_id {
        return Err(identity_mismatch_error(
            starmap_id,
            object_id,
            node_path,
            &instance.instance_id,
        ));
    }
    if !instance.starmap_id.is_empty() && instance.starmap_id != starmap_id {
        return Err(Error::Other(format!(
            "starmap node instance belongs to another graph: path={node_path} \
             object_type=node object_id={object_id} instance_starmap_id={} \
             starmap_id={starmap_id}",
            instance.starmap_id
        )));
    }
    let mut rewrite_instance = false;
    if instance.starmap_id.is_empty() {
        instance.starmap_id = starmap_id.to_string();
        rewrite_instance = true;
    }

    if instance.asset_id.trim().is_empty() {
        rewrite_instance = true;
        instance.asset_id = resolve_instance_asset_id(
            app_data_root,
            starmap_id,
            object_id,
            existing_bindings,
        );
    }
    // 非空 `assetId` 是实例的权威身份：派生反向索引落后时由调用方按实例重建，
    // 不在这里把"索引与实例不一致"变成加载/同步阻断。

    // 实例文件只保存画布归属事实；references.json 里的反向绑定是同一
    // (starmap, instance) 的派生归属记录。需要重写实例（旧格式/空 assetId）
    // 时用它补全缺失的 projectId；已经稳定的实例保持原字节不动。
    if rewrite_instance && instance.project_id.is_none() {
        instance.project_id = existing_bindings
            .and_then(|bindings| bindings.instance_project_id(starmap_id, object_id))
            .map(str::to_string);
    }

    Ok(NodeObjectPlan {
        definition: None,
        instance,
        wrote_definition: false,
        rewrite_instance,
        legacy_tags: Vec::new(),
    })
}

/// `assetId` 为空的新格式实例：按反向绑定 / 同 ID 本体 / 旧节点 id 解析。
///
/// 反向绑定是已有实例的派生身份，即使对应本体尚未落盘（旧数据里实例可以
/// 先于本体存在）也直接复用。都没有时退回对象 id 本身——它是稳定身份，
/// 本体稍后到达即可解析；绝不因为"本体还没到"拒绝只有位置的实例。
fn resolve_instance_asset_id(
    app_data_root: Option<&Path>,
    starmap_id: &str,
    object_id: &str,
    existing_bindings: Option<&NodeObjectBindings>,
) -> String {
    if let Some(reference_asset) =
        existing_bindings.and_then(|bindings| bindings.instance_asset(starmap_id, object_id))
    {
        if !reference_asset.is_empty() {
            return reference_asset.to_string();
        }
    }
    // 旧节点 id 本身是稳定身份：素材库里已有同 id 本体时直接复用；即使读取
    // 本体失败也退回对象 id，不让素材库 I/O 阻断实例身份。
    if lookup_definition(app_data_root, existing_bindings, object_id)
        .ok()
        .flatten()
        .is_some()
    {
        return object_id.to_string();
    }
    object_id.to_string()
}

/// 解码旧格式完整节点，生成稳定身份、共享本体和实例。
#[allow(clippy::too_many_arguments)]
fn decode_legacy_node(
    app_data_root: Option<&Path>,
    starmap_id: &str,
    object_id: &str,
    node_path: &str,
    bytes: &[u8],
    node: StarMapNode,
    existing_bindings: Option<&NodeObjectBindings>,
) -> Result<NodeObjectPlan> {
    if node.id != object_id {
        return Err(identity_mismatch_error(
            starmap_id, object_id, node_path, &node.id,
        ));
    }
    // 旧标签由 tags 迁移写进 annotations 关系仓库；计划把它带出去，由
    // `persist_node_object_plan` 在重写实例前迁出，不阻断解码/同步。
    let legacy_tags = legacy_tags(bytes);

    let explicit_asset_id = if node.asset_id.trim().is_empty() {
        None
    } else {
        Some(node.asset_id.as_str())
    };
    let (asset_id, existing_definition) = resolve_legacy_identity(
        app_data_root,
        starmap_id,
        object_id,
        explicit_asset_id,
        existing_bindings,
    )?;
    let candidate = StarMapNodeDefinition::from_node(asset_id.clone(), &node);
    // 共享内容按对象版本合并：已有本体 revision 不低于候选时保留既有本体，不把
    // 历史快照（同步 base / 旧端离线副本）覆盖到较新的共享内容上；更新的候选
    // 照常发布。旧节点原文在事务完成前一直留在磁盘上。
    let definition = match existing_definition {
        Some(existing) if existing.updated_at >= candidate.updated_at => None,
        Some(existing) if shared_content_equal(&existing, &candidate) => None,
        _ => Some(candidate),
    };

    let project_id = existing_bindings
        .and_then(|bindings| bindings.instance_project_id(starmap_id, object_id))
        .map(str::to_string);
    let instance = StarMapNodeInstance {
        instance_id: object_id.to_string(),
        asset_id,
        position: node.position.clone(),
        starmap_id: starmap_id.to_string(),
        project_id,
        created_at: node.created_at,
        updated_at: node.updated_at,
    };

    Ok(NodeObjectPlan {
        wrote_definition: definition.is_some(),
        definition,
        instance,
        rewrite_instance: true,
        legacy_tags,
    })
}

/// 旧节点的稳定身份解析。
///
/// 返回 `(asset_id, 已有本体)`。已有本体若存在，调用方按对象版本合并共享内容。
/// 实例已有的非空 `assetId` 是权威身份，派生反向索引落后不构成冲突。
fn resolve_legacy_identity(
    app_data_root: Option<&Path>,
    starmap_id: &str,
    object_id: &str,
    explicit_asset_id: Option<&str>,
    existing_bindings: Option<&NodeObjectBindings>,
) -> Result<(String, Option<StarMapNodeDefinition>)> {
    if let Some(explicit) = explicit_asset_id {
        let definition = lookup_definition(app_data_root, existing_bindings, explicit)?;
        return Ok((explicit.to_string(), definition));
    }

    let reference_asset = existing_bindings
        .and_then(|bindings| bindings.instance_asset(starmap_id, object_id))
        .filter(|asset_id| !asset_id.is_empty())
        .map(str::to_string);
    let mut candidates: Vec<String> = Vec::new();
    // 优先已确认绑定，其次旧节点 id 自身（稳定身份）。
    if let Some(asset_id) = &reference_asset {
        candidates.push(asset_id.clone());
    }
    if !object_id.is_empty() && !candidates.iter().any(|candidate| candidate == object_id) {
        candidates.push(object_id.to_string());
    }

    let mut found: Vec<(String, StarMapNodeDefinition)> = Vec::new();
    for candidate in &candidates {
        if let Some(definition) = lookup_definition(app_data_root, existing_bindings, candidate)? {
            found.push((candidate.clone(), definition));
        }
    }
    // 同一旧节点 id 对应多个本体时按对象版本合并：取 revision 最高的一个，
    // 相同 revision 取先出现的（已确认绑定优先）。
    if let Some((asset_id, definition)) = found
        .into_iter()
        .max_by_key(|(_, definition)| definition.updated_at)
    {
        return Ok((asset_id, Some(definition)));
    }

    // 没有任何已有本体：优先使用已确认绑定，否则旧节点 id 自身作为稳定身份。
    let fallback = reference_asset.unwrap_or_else(|| object_id.to_string());
    Ok((fallback, None))
}

/// 按 `app_data_root` → 显式覆盖 → 额外根目录的顺序查找共享本体。
fn lookup_definition(
    app_data_root: Option<&Path>,
    existing_bindings: Option<&NodeObjectBindings>,
    asset_id: &str,
) -> Result<Option<StarMapNodeDefinition>> {
    if let Some(app_data_root) = app_data_root {
        match crate::starmap::library::load_node_definition(app_data_root, asset_id) {
            Ok(Some(definition)) => return Ok(Some(definition)),
            Ok(None) => {}
            Err(error) => {
                return Err(Error::Other(format!(
                    "failed to read shared node definition '{}' at {}: {error}",
                    asset_id,
                    app_data_root
                        .join(crate::starmap::library::node_definition_rel_path(asset_id))
                        .display()
                )));
            }
        }
    }
    if let Some(bindings) = existing_bindings {
        for root in &bindings.definition_roots {
            let path = root.join(crate::starmap::library::node_definition_rel_path(asset_id));
            if !path.exists() {
                continue;
            }
            let content = std::fs::read(&path)?;
            let definition: StarMapNodeDefinition =
                serde_json::from_slice(&content).map_err(|e| {
                    Error::Other(format!(
                        "invalid shared node definition '{}' at {}: {e}",
                        asset_id,
                        path.display()
                    ))
                })?;
            if definition.asset_id != asset_id {
                return Err(Error::Other(format!(
                    "shared node definition identity mismatch: expected asset_id={asset_id} \
                     actual_asset_id={} path={}",
                    definition.asset_id,
                    path.display()
                )));
            }
            return Ok(Some(definition));
        }
    }
    Ok(None)
}

/// 缺失共享本体时用于运行时投影的占位本体。
///
/// Issue #1062 评论 #7236446138 第 2 项：旧数据里画布实例可以先于/独立于
/// 共享本体存在，加载不能因此失败。占位本体只存在于内存，对应读模型状态
/// [`crate::starmap::types::NodeDefinitionState::ProvisionalMissingDefinition`]；
/// 它不写回素材库，不会被任何"只改实例"的事务反推成真实素材，也不会覆盖
/// 以后真正发布的本体。标题退回稳定 `asset_id`、内容为空，避免伪造共享内容。
pub(crate) fn placeholder_definition(instance: &StarMapNodeInstance) -> StarMapNodeDefinition {
    StarMapNodeDefinition {
        asset_id: instance.asset_id.clone(),
        title: instance.asset_id.clone(),
        kind: StarMapNodeKind::Note,
        payload: None,
        content: StarMapNodeContent::default(),
        anchors: Vec::new(),
        portal: None,
        style: StarMapNodeStyle::default(),
        provenance: StarMapProvenance {
            source: StarMapSourceKind::default(),
            source_id: None,
            generated_by: None,
            prompt_id: None,
            review_status: StarMapReviewStatus::default(),
            created_from_anchor: None,
        },
        created_at: instance.created_at,
        updated_at: instance.updated_at,
    }
}

/// 把对象级计划落盘：先迁出旧标签，再写共享本体，然后写画布实例，最后
/// 按实例重建反向引用。
///
/// 幂等：已存在同内容本体/实例时重复调用不改变结果。任何一步失败都返回
/// `Err`，调用方不得宣告迁移成功（原始字节仍在磁盘上，可安全重试）。
///
/// 旧标签迁移失败只记录诊断：标签是派生关系，不能阻断实例/本体落盘，否则
/// 同步又会因为一个缺失的派生字段卡住整组。实例已有非空 `assetId` 是身份
/// 权威，反向索引按实例重建。
pub(crate) fn persist_node_object_plan(
    app_data_root: &Path,
    starmap_id: &str,
    object_id: &str,
    plan: &NodeObjectPlan,
) -> Result<()> {
    if plan.instance.instance_id != object_id {
        return Err(Error::Other(format!(
            "node object plan instance id '{}' does not match object id '{}' \
             (starmap_id={starmap_id})",
            plan.instance.instance_id, object_id
        )));
    }
    // 1. 迁出旧标签：重写实例会丢掉旧 `tags` 字段，所以标签没有全部迁出时
    //    只写本体、保留旧节点原文，等下一次（库索引可用后）再完成实例重写。
    let mut tags_pending = false;
    if !plan.legacy_tags.is_empty() && !plan.instance.asset_id.is_empty() {
        let legacy = [(
            plan.instance.asset_id.clone(),
            plan.instance.project_id.clone(),
            plan.legacy_tags.clone(),
        )];
        match crate::annotations::migration::migrate_starmap_node_tags(app_data_root, &legacy) {
            Ok(report) if report.conflicts.is_empty() => {}
            Ok(report) => {
                tags_pending = true;
                log::warn!(
                    "starmap node legacy tags still pending for starmap_id={starmap_id} \
                     object_id={object_id} asset_id={}: {} conflict(s); keeping legacy node bytes",
                    plan.instance.asset_id,
                    report.conflicts.len()
                );
            }
            Err(error) => {
                tags_pending = true;
                log::warn!(
                    "starmap node legacy tags migration failed for starmap_id={starmap_id} \
                     object_id={object_id} asset_id={}: {error}; keeping legacy node bytes",
                    plan.instance.asset_id
                );
            }
        }
    }
    // 2. 写本体：节点文件一旦变成实例引用，本体必须已经可用。
    if let Some(definition) = &plan.definition {
        crate::starmap::library::save_node_definition(app_data_root, definition)?;
    }
    // 3. 写实例：旧标签还留在原文里时不能重写，否则标签会从数据里消失。
    if plan.rewrite_instance && !tags_pending {
        let rel_path = package_storage::node_rel_path(starmap_id, object_id);
        let path = app_data_root.join(&rel_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&plan.instance)?;
        crate::storage::atomic_write_string(&path, &content)?;
    }
    // 4. 反向索引从实例重建（派生视图，落后时直接按实例修正）。
    crate::starmap::library::record_node_reference(
        app_data_root,
        starmap_id,
        plan.instance.project_id.as_deref(),
        &plan.instance.instance_id,
        &plan.instance.asset_id,
    )?;
    Ok(())
}

/// 共享内容（不含 asset id / 时间戳）是否一致。
fn shared_content_equal(left: &StarMapNodeDefinition, right: &StarMapNodeDefinition) -> bool {
    shared_content_fields(left) == shared_content_fields(right)
}

/// 规范化共享内容（不含 asset id / 时间戳）的字段序列。
fn shared_content_fields(definition: &StarMapNodeDefinition) -> Vec<(&'static str, Value)> {
    vec![
        ("title", serde_json::json!(definition.title)),
        ("kind", serde_json::json!(definition.kind)),
        ("payload", serde_json::json!(definition.payload)),
        ("content", serde_json::json!(definition.content)),
        ("anchors", serde_json::json!(definition.anchors)),
        ("portal", serde_json::json!(definition.portal)),
        ("style", serde_json::json!(definition.style)),
        ("provenance", serde_json::json!(definition.provenance)),
    ]
}

/// 画布节点文件的包内相对路径字符串（用于错误信息）。
fn node_rel_path_string(starmap_id: &str, object_id: &str) -> String {
    format!(
        "starmaps/{starmap_id}/nodes/{}/{object_id}.json",
        package_storage::bucket_for_id(object_id)
    )
}

fn identity_mismatch_error(
    starmap_id: &str,
    object_id: &str,
    node_path: &str,
    actual_id: &str,
) -> Error {
    Error::Other(format!(
        "starmap node instance identity mismatch: path={node_path} object_type=node \
         member_id={object_id} instance_id={actual_id} starmap_id={starmap_id}"
    ))
}

/// 旧节点文件里的非空 `tags` 数组；没有则返回空列表。
fn legacy_tags(bytes: &[u8]) -> Vec<String> {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return Vec::new();
    };
    let Some(tags) = value.get(LEGACY_TAGS_FIELD).and_then(Value::as_array) else {
        return Vec::new();
    };
    tags.iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests;
