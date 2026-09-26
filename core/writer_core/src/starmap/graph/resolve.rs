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
        match load_current_graph_meta(&graph_json_path) {
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
        serde_json::from_str::<crate::starmap::types::StarMapNode>(&content)
            .map(Some)
            .map_err(|_| ResolverReadError::CorruptStarmap)
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
        serde_json::from_str::<crate::starmap::types::StarMapEmbed>(&content)
            .map(Some)
            .map_err(|_| ResolverReadError::CorruptStarmap)
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
        match load_current_graph_meta(&graph_json_path) {
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

/// 检查星图是否存在且可读（先查 context.overlays，找不到再查磁盘 provider）。
///
/// 使用 `starmap_exists_and_readable` 而非仅查目录存在，确保 schema 不兼容
/// 或 graph.json 损坏的星图不会被误判为存在。
///
/// **fail-safe 语义**：遇到 `UnsupportedVersion/CorruptStarmap/ReadFailed` 时返回 `true`，
/// 因为删除保护调用此函数时，不能把"读取失败"当成"星图不存在"而允许删除。
/// `resolve_target` 的起点检查不使用此函数，而是直接调用 `starmap_exists_and_readable`
/// 以获取精确的错误类型。
pub(crate) fn starmap_exists(context: &GraphResolverContext, starmap_id: &str) -> bool {
    if context.overlays.contains_key(starmap_id) {
        return true;
    }
    let provider = ResolverGraphProvider {
        app_data_root: &context.app_data_root,
    };
    // fail-safe：读取错误时认为星图存在（防止删除保护误删）
    provider.starmap_exists_and_readable(starmap_id).unwrap_or(true)
}

/// `resolve_target` 内部使用的星图可读性检查，返回精确的错误类型。
///
/// 与 `starmap_exists` 不同，此函数在遇到 `UnsupportedVersion/CorruptStarmap/ReadFailed`
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
/// 1. **深度限制**：`segments.len() > 32` 返回 `TooDeep`。
/// 2. **起点检查**：`path.starmap_id` 对应的星图必须存在。
/// 3. **逐段穿越**：沿 `segments` 遍历，用 `HashSet` 记录已访问的 `starmap_id`，
///    重复进入同一星图即返回 `CycleDetected`。
/// 4. **终节点校验**：路径末端的 `StarMapTargetDetail` 在最终星图中验证。
pub fn resolve_target(
    context: &GraphResolverContext,
    path: &StarMapTargetPath,
) -> Result<ResolvedTarget, crate::starmap::semantic::StarMapTargetResolveStatus> {
    use crate::starmap::semantic::StarMapTargetResolveStatus::*;

    if path.segments.len() > 32 {
        return Err(TooDeep);
    }

    // 起点检查：直接使用 starmap_exists_and_readable 以正确传递 UnsupportedVersion 等错误
    check_starmap_readable(context, &path.starmap_id)?;

    let mut current_starmap_id = path.starmap_id.clone();
    let mut traversed_starmap_ids = vec![current_starmap_id.clone()];
    let mut visited = std::collections::HashSet::new();
    visited.insert(current_starmap_id.clone());

    for segment in &path.segments {
        match segment {
            StarMapPathSegment::EnterEmbed { instance_id } => {
                let embed = match lookup_embed(context, &current_starmap_id, instance_id) {
                    Ok(Some(e)) => e,
                    Ok(None) => return Err(MissingEmbed),
                    Err(status) => return Err(status),
                };
                current_starmap_id = embed.target_starmap_id.clone();
                if !visited.insert(current_starmap_id.clone()) {
                    return Err(CycleDetected);
                }
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
                if !visited.insert(current_starmap_id.clone()) {
                    return Err(CycleDetected);
                }
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
        StarMapTargetDetail::Entity { .. } | StarMapTargetDetail::External { .. } => {}
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
