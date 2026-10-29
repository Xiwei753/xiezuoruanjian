use crate::starmap::package_storage::bucket_for_id;
use crate::starmap::semantic::StarMapTargetDetail;
use crate::starmap::store::load::phased::load_current_graph_meta;
use crate::starmap::store::meta::GraphMeta;
use crate::starmap::types::reference::{StarMapPathSegment, StarMapTargetPath};
use crate::starmap::types::StarMapGraph;

/// Resolver 上下文：统一管理 overlay graphs 和磁盘路径。
///
/// `overlays` 按 `starmap_id` 索引，包含所有需要优先于磁盘读取的内存图：
/// - 当前正在校验的 candidate graph
/// - 所有已加载/dirty 的 Store 的 `to_starmap_graph()` 快照
///
/// 引用扫描等已 flush 的场景传空 overlays（从磁盘读取即可）。
#[derive(Debug, Clone)]
pub struct GraphResolverContext {
    pub app_data_root: std::path::PathBuf,
    pub overlays: std::collections::HashMap<String, StarMapGraph>,
}

impl GraphResolverContext {
    /// 创建一个只有 app_data_root、没有 overlays 的 context。
    /// 用于引用扫描等已 flush 的场景。
    pub fn new_disk_only(app_data_root: &std::path::Path) -> Self {
        Self {
            app_data_root: app_data_root.to_path_buf(),
            overlays: std::collections::HashMap::new(),
        }
    }
}

/// 解析后的目标：resolver 的唯一真实解析结果。
///
/// 由 [`resolve_target`] 返回，包含解析过程中收集的全部事实：
/// - `final_starmap_id`：路径最终落在的星图 ID
/// - `target`：路径终点的具体引用（Node/Anchor/ChapterRange 等）
/// - `traversed_starmap_ids`：实际经过的 starmap_id 列表，**含起点和终点图**，
///   顺序为穿越顺序。删除保护据此判断一张星图是否被某条路径经过。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTarget {
    pub final_starmap_id: String,
    pub target: StarMapTargetDetail,
    pub traversed_starmap_ids: Vec<String>,
}

/// resolver 磁盘读取的不可恢复错误。
///
/// 与 [`crate::starmap::semantic::StarMapTargetResolveStatus`] 的后三个变体一一对应，
/// 由 [`map_read_error`] 映射。区分"对象真不存在"（返回 `Ok(None)`）与"读取失败"
/// （返回 `Err`），避免把磁盘错误/版本不兼容吞成 MissingNode/MissingEmbed。
enum ResolverReadError {
    UnsupportedVersion,
    CorruptStarmap,
    ReadFailed,
}

/// 无副作用的只读星图对象 provider，供 resolver 使用。
///
/// 只做：
/// 1. 用 [`load_current_graph_meta`] 检查 graph.json schema 版本；
/// 2. 按 bucket 路径直接读取 node/embed 文件。
///
/// 不跑 `detect_dangling_references`、不跑 orphan scan、不创建完整 `StarMapStore`、
/// 不递归加载整张图。这切断了 `lookup_node`/`lookup_embed` → `load_full` →
/// `detect_dangling_references` → `resolve_target` → `lookup_node`/`lookup_embed`
/// 的无限递归路径。
struct ResolverGraphProvider<'a> {
    app_data_root: &'a std::path::Path,
}

impl<'a> ResolverGraphProvider<'a> {
    fn starmap_dir(&self, starmap_id: &str) -> std::path::PathBuf {
        self.app_data_root.join("starmaps").join(starmap_id)
    }

    /// 加载 GraphMeta 并做 schema 版本检查。
    ///
    /// graph.json 不存在时返回 `Ok(None)`（空星图）。
    /// `load_current_graph_meta` 已内置 schema 版本检查，此处只需正确映射错误类型。
    fn load_graph_meta_checked(
        &self,
        starmap_id: &str,
    ) -> Result<Option<GraphMeta>, ResolverReadError> {
        let graph_json_path = self.starmap_dir(starmap_id).join("graph.json");
        if !graph_json_path.exists() {
            return Ok(None);
        }
        match load_current_graph_meta(&graph_json_path, starmap_id) {
            Ok(meta) => Ok(meta),
            Err(crate::error::Error::UnsupportedVersion { .. }) => {
                Err(ResolverReadError::UnsupportedVersion)
            }
            Err(crate::error::Error::Io(_)) => Err(ResolverReadError::ReadFailed),
            Err(_) => Err(ResolverReadError::CorruptStarmap),
        }
    }

    /// 读取一个 node 对象文件。文件不存在或不在 GraphMeta 成员列表中返回 `Ok(None)`。
    fn get_node(
        &self,
        starmap_id: &str,
        node_id: &str,
    ) -> Result<Option<crate::starmap::types::StarMapNode>, ResolverReadError> {
        let meta = self.load_graph_meta_checked(starmap_id)?;
        // GraphMeta 为 None 表示空星图，对象不存在
        // GraphMeta 存在时，node_id 必须在 node_ids 列表中才视为合法
        if let Some(ref m) = meta {
            if !m.node_ids.iter().any(|id| id == node_id) {
                return Ok(None);
            }
        } else {
            return Ok(None);
        }
        let bucket_dir = self
            .starmap_dir(starmap_id)
            .join("nodes")
            .join(bucket_for_id(node_id));
        let path = bucket_dir.join(format!("{}.json", node_id));
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path).map_err(|_| ResolverReadError::ReadFailed)?;
        let node = serde_json::from_str::<crate::starmap::types::StarMapNode>(&content)
            .map_err(|_| ResolverReadError::CorruptStarmap)?;
        if node.id != node_id {
            return Err(ResolverReadError::CorruptStarmap);
        }
        Ok(Some(node))
    }

    /// 读取一个 embed 对象文件。文件不存在或不在 GraphMeta 成员列表中返回 `Ok(None)`。
    fn get_embed(
        &self,
        starmap_id: &str,
        instance_id: &str,
    ) -> Result<Option<crate::starmap::types::StarMapEmbed>, ResolverReadError> {
        let meta = self.load_graph_meta_checked(starmap_id)?;
        // GraphMeta 为 None 表示空星图，对象不存在
        // GraphMeta 存在时，instance_id 必须在 embed_instance_ids 列表中才视为合法
        if let Some(ref m) = meta {
            if !m.embed_instance_ids.iter().any(|id| id == instance_id) {
                return Ok(None);
            }
        } else {
            return Ok(None);
        }
        let bucket_dir = self
            .starmap_dir(starmap_id)
            .join("embeds")
            .join(bucket_for_id(instance_id));
        let path = bucket_dir.join(format!("{}.json", instance_id));
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path).map_err(|_| ResolverReadError::ReadFailed)?;
        let embed = serde_json::from_str::<crate::starmap::types::StarMapEmbed>(&content)
            .map_err(|_| ResolverReadError::CorruptStarmap)?;
        if embed.instance_id != instance_id {
            return Err(ResolverReadError::CorruptStarmap);
        }
        Ok(Some(embed))
    }

    /// 检查星图是否可读：星图 meta 文件存在且 graph.json（若存在）能正确解析。
    ///
    /// 1. `starmaps/{id}.meta.json` 存在（星图已通过 create_starmap 创建）
    /// 2. graph.json 存在时，能成功通过 `load_current_graph_meta` 解析
    /// 3. graph.json 不存在时也算可读（空星图，尚未写入任何对象）
    fn starmap_exists_and_readable(&self, starmap_id: &str) -> Result<bool, ResolverReadError> {
        // 先检查 .meta.json 是否存在（星图已创建）
        // .meta.json 路径是 starmaps/{id}.meta.json，不是 starmaps/{id}/.meta.json
        let meta_path = self
            .app_data_root
            .join("starmaps")
            .join(format!("{}.meta.json", starmap_id));
        if !meta_path.exists() {
            return Ok(false);
        }
        // 再检查 graph.json 是否可读（schema 版本、JSON 完整性）
        let graph_json_path = self.starmap_dir(starmap_id).join("graph.json");
        if !graph_json_path.exists() {
            // 空星图（尚未写入任何对象），可读
            return Ok(true);
        }
        match load_current_graph_meta(&graph_json_path, starmap_id) {
            Ok(_) => Ok(true),
            Err(crate::error::Error::UnsupportedVersion { .. }) => {
                Err(ResolverReadError::UnsupportedVersion)
            }
            Err(crate::error::Error::Io(_)) => Err(ResolverReadError::ReadFailed),
            Err(_) => Err(ResolverReadError::CorruptStarmap),
        }
    }
}

/// 把 [`ResolverReadError`] 映射成 resolver 对外的解析状态。
fn map_read_error(e: ResolverReadError) -> crate::starmap::semantic::StarMapTargetResolveStatus {
    use crate::starmap::semantic::StarMapTargetResolveStatus::*;
    match e {
        ResolverReadError::UnsupportedVersion => UnsupportedVersion,
        ResolverReadError::CorruptStarmap => CorruptStarmap,
        ResolverReadError::ReadFailed => ReadFailed,
    }
}

/// 从 context.overlays 或磁盘只读 provider 读取一个 embed 实例。
///
/// 先查 context.overlays（按 starmap_id 匹配），找不到再用
/// [`ResolverGraphProvider`] 从磁盘按 bucket 路径直接读取对象文件。
/// 不调用 `StarMapStore::load_full`，避免 detect_dangling_references 递归。
fn lookup_embed(
    context: &GraphResolverContext,
    current_starmap_id: &str,
    instance_id: &str,
) -> Result<
    Option<crate::starmap::types::StarMapEmbed>,
    crate::starmap::semantic::StarMapTargetResolveStatus,
> {
    if let Some(g) = context.overlays.get(current_starmap_id) {
        return Ok(g
            .embeds
            .iter()
            .find(|e| e.instance_id == instance_id)
            .cloned());
    }
    let provider = ResolverGraphProvider {
        app_data_root: &context.app_data_root,
    };
    provider
        .get_embed(current_starmap_id, instance_id)
        .map_err(map_read_error)
}

/// 从 context.overlays 或磁盘只读 provider 读取一个节点。
///
/// 不调用 `StarMapStore::load_full`，避免 detect_dangling_references 递归。
fn lookup_node(
    context: &GraphResolverContext,
    current_starmap_id: &str,
    node_id: &str,
) -> Result<
    Option<crate::starmap::types::StarMapNode>,
    crate::starmap::semantic::StarMapTargetResolveStatus,
> {
    if let Some(g) = context.overlays.get(current_starmap_id) {
        return Ok(g.nodes.iter().find(|n| n.id == node_id).cloned());
    }
    let provider = ResolverGraphProvider {
        app_data_root: &context.app_data_root,
    };
    provider
        .get_node(current_starmap_id, node_id)
        .map_err(map_read_error)
}

/// `resolve_target` 内部使用的星图可读性检查，返回精确的错误类型。
///
/// 此函数在遇到 `UnsupportedVersion/CorruptStarmap/ReadFailed`
/// 时返回对应的 `StarMapTargetResolveStatus`，而不是 fail-safe 返回 `true`。
/// 这确保 resolver 能向上传递"目标星图 schema 不兼容"等错误，而不是吞成 `MissingStarmap`。
fn check_starmap_readable(
    context: &GraphResolverContext,
    starmap_id: &str,
) -> Result<(), crate::starmap::semantic::StarMapTargetResolveStatus> {
    use crate::starmap::semantic::StarMapTargetResolveStatus::*;
    if context.overlays.contains_key(starmap_id) {
        return Ok(());
    }
    let provider = ResolverGraphProvider {
        app_data_root: &context.app_data_root,
    };
    match provider.starmap_exists_and_readable(starmap_id) {
        Ok(true) => Ok(()),
        Ok(false) => Err(MissingStarmap),
        Err(ResolverReadError::UnsupportedVersion) => Err(UnsupportedVersion),
        Err(ResolverReadError::CorruptStarmap) => Err(CorruptStarmap),
        Err(ResolverReadError::ReadFailed) => Err(ReadFailed),
    }
}

/// 解析目标路径的可达性，返回详细解析结果。
///
/// 这是星图路径解析的**唯一真实入口**。所有需要"从某张宿主图开始，经过
/// Embed/Portal，到终点"的引用解析都应调用此函数。
///
/// ## context 参数
///
/// `context` 包含 `app_data_root` 和 `overlays`。`overlays` 按 `starmap_id`
/// 索引，当路径穿越到某个 overlay 对应的星图时，优先从 overlay 读取对象，
/// 而不是从磁盘 Store 读取。这解决了"内存刚改完，resolver 去读旧磁盘"
/// 的一致性问题。多层路径中如果再次走回某个 overlay 对应的星图，也拿 overlay。
/// 引用扫描等已 flush 的场景传空 overlays。
///
/// ## 算法
///
/// 1. **起点检查**：`path.starmap_id` 对应的星图必须存在。
/// 2. **逐段穿越**：沿给定的 `segments` 顺序逐段解析，每一步都由当次的
///    Embed/Portal 实例定位。同一个 `starmap_id` 可以在路径的不同位置重复出现
///    （A→B→A、A→A 都合法），不按 `starmap_id` 去重，也不存在深度上限。
/// 3. **终节点校验**：路径末端的 `StarMapTargetDetail` 在最终星图中验证。
///
/// 路径数组本身是有限的；循环关系是合法内容，终止由数组长度保证。
pub fn resolve_target(
    context: &GraphResolverContext,
    path: &StarMapTargetPath,
) -> Result<ResolvedTarget, crate::starmap::semantic::StarMapTargetResolveStatus> {
    use crate::starmap::semantic::StarMapTargetResolveStatus::*;

    // 起点检查：直接使用 starmap_exists_and_readable 以正确传递 UnsupportedVersion 等错误
    check_starmap_readable(context, &path.starmap_id)?;

    let mut current_starmap_id = path.starmap_id.clone();
    let mut traversed_starmap_ids = vec![current_starmap_id.clone()];

    for segment in &path.segments {
        match segment {
            StarMapPathSegment::EnterEmbed { instance_id } => {
                let embed = match lookup_embed(context, &current_starmap_id, instance_id) {
                    Ok(Some(e)) => e,
                    Ok(None) => return Err(MissingEmbed),
                    Err(status) => return Err(status),
                };
                current_starmap_id = embed.target_starmap_id.clone();
                check_starmap_readable(context, &current_starmap_id)?;
                traversed_starmap_ids.push(current_starmap_id.clone());
            }
            StarMapPathSegment::EnterPortal { node_id } => {
                let node = match lookup_node(context, &current_starmap_id, node_id) {
                    Ok(Some(n)) => n,
                    Ok(None) => return Err(MissingPortal),
                    Err(status) => return Err(status),
                };
                let portal = match &node.portal {
                    Some(p) => p,
                    None => return Err(MissingPortal),
                };
                current_starmap_id = portal.destination_starmap_id.clone();
                check_starmap_readable(context, &current_starmap_id)?;
                traversed_starmap_ids.push(current_starmap_id.clone());
            }
        }
    }

    match &path.target {
        StarMapTargetDetail::Node { node_id } => {
            match lookup_node(context, &current_starmap_id, node_id) {
                Ok(Some(_)) => {}
                Ok(None) => return Err(MissingNode),
                Err(status) => return Err(status),
            }
        }
        StarMapTargetDetail::Anchor { node_id, anchor_id } => {
            match lookup_node(context, &current_starmap_id, node_id) {
                Ok(Some(n)) => {
                    if !n.anchors.iter().any(|a| &a.anchor_id == anchor_id) {
                        return Err(MissingAnchor);
                    }
                }
                Ok(None) => return Err(MissingNode),
                Err(status) => return Err(status),
            }
        }
        StarMapTargetDetail::ChapterRange {
            range_start,
            range_end,
            ..
        } => {
            if let (Some(s), Some(e)) = (range_start, range_end) {
                if s > e {
                    return Err(InvalidRange);
                }
            }
        }
        StarMapTargetDetail::Starmap => {
            // 直接 Starmap 目标也要走 provider 检查，不能只查目录存在
            check_starmap_readable(context, &current_starmap_id)?;
        }
        StarMapTargetDetail::Entity { .. } => {}
    }

    Ok(ResolvedTarget {
        final_starmap_id: current_starmap_id,
        target: path.target.clone(),
        traversed_starmap_ids,
    })
}

/// 解析目标路径的可达性（兼容包装）。
///
/// 内部调用 [`resolve_target`]，仅返回状态而不返回详细解析结果。
pub fn resolve_target_path(
    context: &GraphResolverContext,
    path: &StarMapTargetPath,
) -> crate::starmap::semantic::StarMapTargetResolveStatus {
    match resolve_target(context, path) {
        Ok(_) => crate::starmap::semantic::StarMapTargetResolveStatus::Resolved,
        Err(status) => status,
    }
}

/// 解析目标路径并返回解析后的 `(starmap_id, object_id)` 对。
///
/// 用于跨图引用检查时确定 Edge 端点的真实位置。解析失败时返回 `None`。
///
/// 与 [`resolve_target`] 的区别：此函数将 `ResolvedTarget` 拆解为
/// `(final_starmap_id, target)` 元组，方便直接比较。`resolve_target`
/// 返回完整的 `ResolvedTarget`（含 `traversed_starmap_ids`），适用于
/// 需要完整穿越链的场景。
pub fn resolve_target_with_context(
    context: &GraphResolverContext,
    path: &StarMapTargetPath,
) -> Option<(String, StarMapTargetDetail)> {
    resolve_target(context, path)
        .ok()
        .map(|r| (r.final_starmap_id, r.target))
}

// ---------------------------------------------------------------------------
// 逐段路径依赖提取
// ---------------------------------------------------------------------------

/// 把 `StarMapTargetResolveStatus` 映射成 `crate::error::Error`。
///
/// 导航路由/目标解析失败不是"没有连接"，而是路径本身不可达或读取错误。
/// `UnsupportedVersion`/`CorruptStarmap`/`ReadFailed` 映射成对应的 `Error`，
/// 其余映射成 `Error::Other`。
///
/// 这是全 crate 唯一的解析状态→错误映射，`connections.rs`、
/// `relation_index.rs` 和本模块都复用此函数，不重复实现。
pub(crate) fn resolve_status_to_error(
    status: crate::starmap::semantic::StarMapTargetResolveStatus,
) -> crate::error::Error {
    match status {
        crate::starmap::semantic::StarMapTargetResolveStatus::UnsupportedVersion => {
            crate::error::Error::UnsupportedVersion {
                version: String::new(),
            }
        }
        other => crate::error::Error::Other(format!("navigation route resolve failed: {other:?}")),
    }
}

/// 路径依赖种类：路径终点或途中穿越产生的依赖类型。
///
/// 用于反向依赖索引 [`crate::starmap::store::relation_index::PathDependencyIndex`]
/// 按 `(host_starmap_id, kind, object_id)` 建立倒排表。
///
/// - `NodeTarget`：路径终点落在某图的 Node（`object_id` = node_id）
/// - `AnchorTarget`：路径终点落在某图的 Anchor（`object_id` = node_id，
///   `anchor_id` 计入依赖事实；删除节点连带删除其所有锚点，故以 node_id 为主键）
/// - `PortalTraversal`：路径途中穿越某图的 Portal 节点（`object_id` = node_id）
/// - `EmbedTraversal`：路径途中穿越某图的 Embed 实例（`object_id` = instance_id）
/// - `CanvasEndpoint`：路径已成功到达终点画布，记录最终 canvas ID。
///   用于 [`crate::starmap::store::relation_index::PathDependencyIndex`]
///   的 `collect_candidate_edge_owners` 按最终画布找 Edge 来源，
///   覆盖 Starmap / ChapterRange / Entity 等非 Node/Anchor 终点。
///   `object_id` = 最终 canvas ID（即 `host_starmap_id`）。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathDependencyKind {
    NodeTarget,
    AnchorTarget,
    PortalTraversal,
    EmbedTraversal,
    CanvasEndpoint,
}

/// 单条路径解析产生的单个依赖事实。
///
/// 一条 `StarMapTargetPath` 可产生多个事实：每段成功穿越产生一个
/// `EmbedTraversal`/`PortalTraversal` 事实，终点成功解析产生一个
/// `NodeTarget`/`AnchorTarget` 事实。
#[derive(Debug, Clone)]
pub struct PathDependencyFact {
    /// 依赖所寄宿的星图 ID（穿越段寄宿在穿越前的图，终点寄宿在最终图）。
    pub host_starmap_id: String,
    pub kind: PathDependencyKind,
    /// 被依赖对象的 ID（node_id 或 instance_id）。
    pub object_id: String,
    /// 仅 `AnchorTarget` 有值，标识具体 anchor_id。
    pub anchor_id: Option<String>,
}

/// 逐段提取一条路径产生的全部依赖事实。
///
/// 与 [`resolve_target`] 的关键区别：**不要求终点成功解析才能知道此前的入口依赖**。
/// 沿 `segments` 逐段穿越，每段成功即记录对应的 `EmbedTraversal`/`PortalTraversal`
/// 事实；终点成功解析则记录 `NodeTarget`/`AnchorTarget` 事实。
///
/// ## 错误传播（修改3核心）
///
/// - 读取错误（`UnsupportedVersion`/`CorruptStarmap`/`ReadFailed`）立即返回 `Err`，
///   **不**吞成"无依赖"。
/// - 对象不存在（`MissingNode`/`MissingEmbed`/`MissingPortal`/`MissingAnchor` 等）
///   视为路径已悬空：停止收集后续事实，**保留已成功收集的事实**并返回 `Ok`。
///   已穿越的 Embed/Portal 依赖仍然有效，即使最终目标读取出错。
///
/// 这样删除保护既能传播磁盘错误，又不会因为路径终点恰好被删而漏报途中穿越依赖。
pub fn extract_path_dependency_facts(
    context: &GraphResolverContext,
    path: &StarMapTargetPath,
) -> Result<Vec<PathDependencyFact>, crate::error::Error> {
    use crate::starmap::semantic::StarMapTargetResolveStatus as Status;

    let mut facts = Vec::new();

    // 起点检查
    check_starmap_readable(context, &path.starmap_id).map_err(resolve_status_to_error)?;

    let mut current_starmap_id = path.starmap_id.clone();

    for segment in &path.segments {
        match segment {
            StarMapPathSegment::EnterEmbed { instance_id } => {
                let embed = match lookup_embed(context, &current_starmap_id, instance_id) {
                    Ok(Some(e)) => e,
                    Ok(None) => return Ok(facts),
                    Err(status) => return Err(resolve_status_to_error(status)),
                };
                facts.push(PathDependencyFact {
                    host_starmap_id: current_starmap_id.clone(),
                    kind: PathDependencyKind::EmbedTraversal,
                    object_id: instance_id.clone(),
                    anchor_id: None,
                });
                current_starmap_id = embed.target_starmap_id.clone();
                check_starmap_readable(context, &current_starmap_id)
                    .map_err(resolve_status_to_error)?;
            }
            StarMapPathSegment::EnterPortal { node_id } => {
                let node = match lookup_node(context, &current_starmap_id, node_id) {
                    Ok(Some(n)) => n,
                    Ok(None) => return Ok(facts),
                    Err(status) => return Err(resolve_status_to_error(status)),
                };
                let portal = match &node.portal {
                    Some(p) => p,
                    None => return Ok(facts),
                };
                facts.push(PathDependencyFact {
                    host_starmap_id: current_starmap_id.clone(),
                    kind: PathDependencyKind::PortalTraversal,
                    object_id: node_id.clone(),
                    anchor_id: None,
                });
                current_starmap_id = portal.destination_starmap_id.clone();
                check_starmap_readable(context, &current_starmap_id)
                    .map_err(resolve_status_to_error)?;
            }
        }
    }

    // 终点依赖
    push_terminal_dependency_facts(context, &current_starmap_id, &path.target, &mut facts)?;

    Ok(facts)
}

/// 提取路径终点的依赖事实，追加到 `facts`。
///
/// 读取错误返回 `Err`；终点对象不存在（`MissingNode`/`MissingAnchor`）则不追加、
/// 不报错（路径悬空，但途中穿越依赖已由调用方收集）。
///
/// 路径**已成功到达终点画布**时，额外产出一条 `CanvasEndpoint` 事实
/// （`host_starmap_id` 和 `object_id` 均为最终画布 ID），供
/// [`crate::starmap::store::relation_index::PathDependencyIndex`] 的
/// `collect_candidate_edge_owners` 按最终画布收集 Edge 来源，覆盖
/// Starmap / ChapterRange / Entity 等非 Node/Anchor 终点（问题1）。
fn push_terminal_dependency_facts(
    context: &GraphResolverContext,
    current_starmap_id: &str,
    target: &StarMapTargetDetail,
    facts: &mut Vec<PathDependencyFact>,
) -> Result<(), crate::error::Error> {
    use crate::starmap::semantic::StarMapTargetResolveStatus as Status;

    // 路径是否已成功到达终点画布。只有成功到达才产出 CanvasEndpoint 事实，
    // 悬空路径（MissingNode/MissingAnchor 等）不记录成有效终点。
    let mut reached_canvas = false;

    match target {
        StarMapTargetDetail::Node { node_id } => {
            match lookup_node(context, current_starmap_id, node_id) {
                Ok(Some(_)) => {
                    facts.push(PathDependencyFact {
                        host_starmap_id: current_starmap_id.to_string(),
                        kind: PathDependencyKind::NodeTarget,
                        object_id: node_id.clone(),
                        anchor_id: None,
                    });
                    reached_canvas = true;
                }
                Ok(None) => {}
                Err(status) => return Err(resolve_status_to_error(status)),
            }
        }
        StarMapTargetDetail::Anchor { node_id, anchor_id } => {
            match lookup_node(context, current_starmap_id, node_id) {
                Ok(Some(n)) => {
                    if n.anchors.iter().any(|a| &a.anchor_id == anchor_id) {
                        facts.push(PathDependencyFact {
                            host_starmap_id: current_starmap_id.to_string(),
                            kind: PathDependencyKind::AnchorTarget,
                            object_id: node_id.clone(),
                            anchor_id: Some(anchor_id.clone()),
                        });
                        reached_canvas = true;
                    }
                }
                Ok(None) => {}
                Err(status) => return Err(resolve_status_to_error(status)),
            }
        }
        StarMapTargetDetail::ChapterRange {
            range_start,
            range_end,
            ..
        } => {
            if let (Some(s), Some(e)) = (range_start, range_end) {
                if s > e {
                    return Err(resolve_status_to_error(Status::InvalidRange));
                }
            }
            reached_canvas = true;
        }
        StarMapTargetDetail::Starmap => {
            check_starmap_readable(context, current_starmap_id).map_err(resolve_status_to_error)?;
            reached_canvas = true;
        }
        StarMapTargetDetail::Entity { .. } => {
            reached_canvas = true;
        }
    }

    if reached_canvas {
        facts.push(PathDependencyFact {
            host_starmap_id: current_starmap_id.to_string(),
            kind: PathDependencyKind::CanvasEndpoint,
            object_id: current_starmap_id.to_string(),
            anchor_id: None,
        });
    }

    Ok(())
}
