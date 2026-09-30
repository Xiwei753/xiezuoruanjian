// =============================================================================
// starmap_bridge.rs — 星图 Core DTO / CRUD 适配层
// =============================================================================
//
// 引用了什么：
// - writer_core::api::types::*：星图节点、边、布局及相关 Patch 更新 DTO。
// - writer_core::api::WriterCoreApi：核心库主业务 API。
//
// 干什么的：
// - 负责星图领域核心 DTO 到客户端需要的兼容 JSON 字符串的双向数据编解码与类型转换。
// - 提供星图生命周期（列表获取、绑定/解绑作品、创建/重命名/删除星图）的底层桥接。
// - 提供图数据点、线、嵌入式富文本元素（add_starmap_embed 等）的增删改查动作。
//
// 不干什么：
// - 不再实现边渲染、命中测试、网格布局等纯显示几何算法——它们归
//   apps/Linux_qt/src/starmap_view/bridge.rs（Linux 平台端显示层入口）。
// - graph 读取的布局派生已移到 backend 组合边界，本模块只做 Core DTO/CRUD/
//   ResultEnvelope 适配，不调用 starmap_view::bridge::*。
//
// 被什么引用：
// - 被 apps/Linux_qt/src/backend/starmap_backend.rs 及其分文件引用，作为后端
//   QObject 完成星图数据管理（CRUD）的执行模块。
// =============================================================================

use writer_core::api::types::{
    StarMapEdgeDto, StarMapEdgeKindDto, StarMapEdgePatchDto, StarMapEmbedDto, StarMapEmbedPatchDto,
    StarMapEmbedPatchInputDto, StarMapHyperlinkDto, StarMapHyperlinkPatchDto,
    StarMapNodeContentDto, StarMapNodeDto, StarMapNodeKindDto, StarMapNodePatchDto,
    StarMapPathSegmentDto, StarMapPointDto, StarMapProvenanceDto, StarMapTargetDetailDto,
    StarMapTargetPathDto,
};
use writer_core::api::{WriterCoreApi, WriterError};

fn parse_node_kind(kind: &str) -> StarMapNodeKindDto {
    serde_json::from_value(serde_json::json!(kind)).unwrap_or(StarMapNodeKindDto::Note)
}

fn parse_edge_kind(kind: &str) -> StarMapEdgeKindDto {
    serde_json::from_value(serde_json::json!(kind)).unwrap_or(StarMapEdgeKindDto::RelatedTo)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or(std::time::Duration::ZERO)
        .as_millis() as u64
}

fn envelope<T: serde::Serialize>(result: Result<T, WriterError>) -> String {
    match result {
        Ok(data) => writer_core::api::ResultEnvelope::success(data).to_json_string(),
        Err(error) => writer_core::api::ResultEnvelope::<T>::error(error).to_json_string(),
    }
}

fn envelope_err_str(msg: &str) -> String {
    envelope::<serde_json::Value>(Err(WriterError::Other(msg.to_string())))
}

pub fn list_starmaps(api: &WriterCoreApi) -> String {
    envelope(api.list_starmaps())
}

pub fn list_starmaps_for_project(api: &WriterCoreApi, project_id: &str) -> String {
    envelope(api.list_starmaps_for_project(project_id))
}

pub fn get_starmap(api: &WriterCoreApi, starmap_id: &str) -> String {
    envelope(api.get_starmap(starmap_id))
}

pub fn create_starmap(
    api: &WriterCoreApi,
    title: &str,
    description: &str,
    accent_color: Option<&str>,
) -> String {
    envelope(api.create_starmap(title, description, accent_color))
}

pub fn rename_starmap(api: &WriterCoreApi, starmap_id: &str, new_title: &str) -> String {
    envelope(api.rename_starmap(starmap_id, new_title))
}

pub fn delete_starmap(api: &WriterCoreApi, starmap_id: &str) -> String {
    envelope(api.delete_starmap(starmap_id))
}

pub fn create_starmap_node(
    api: &WriterCoreApi,
    starmap_id: &str,
    title: &str,
    kind: &str,
    x: f64,
    y: f64,
) -> String {
    let now = now_ms();
    let node = StarMapNodeDto {
        id: format!("n_{}", uuid::Uuid::new_v4()),
        title: title.to_string(),
        kind: parse_node_kind(kind),
        payload: None,
        tags: vec![],
        content: StarMapNodeContentDto::default(),
        anchors: vec![],
        portal: None,
        // Core 收口后显示策略由平台端自理，DTO 只保留 position 坐标与 style 外观。
        position: StarMapPointDto {
            x: x as f32,
            y: y as f32,
        },
        style: Default::default(),
        provenance: Default::default(),
        created_at: now,
        updated_at: now,
    };

    envelope(api.add_starmap_node(starmap_id, node, x as f32, y as f32))
}

pub fn update_starmap_node(
    api: &WriterCoreApi,
    starmap_id: &str,
    node_id: &str,
    patch_json: &str,
) -> String {
    let patch: StarMapNodePatchDto = match serde_json::from_str(patch_json) {
        Ok(p) => p,
        Err(e) => return envelope_err_str(&format!("Invalid patch JSON: {}", e)),
    };

    envelope(api.update_starmap_node(starmap_id, node_id, patch))
}

pub fn delete_starmap_node(api: &WriterCoreApi, starmap_id: &str, node_id: &str) -> String {
    envelope(api.delete_starmap_node(starmap_id, node_id))
}

/// 解析 `root starmap + 路径段`，返回最终星图 ID。
///
/// `segments_json` 是 `StarMapPathSegmentDto` 数组的 JSON（QML 的
/// `currentPathSegments`：`enterEmbed{instanceId}` / `enterPortal{nodeId}`）。
/// 当前层身份由 root + segments 逐段穿越决定，解析失败返回错误 envelope，
/// 调用方不得回退到点击事件传来的裸目标 ID。
pub fn resolve_starmap_path(
    api: &WriterCoreApi,
    root_starmap_id: &str,
    segments_json: &str,
) -> String {
    let segments: Vec<StarMapPathSegmentDto> = match serde_json::from_str(segments_json) {
        Ok(s) => s,
        Err(e) => return envelope_err_str(&format!("Invalid path segments JSON: {}", e)),
    };
    envelope(api.resolve_starmap_path(root_starmap_id, segments))
}

pub fn create_starmap_edge(
    api: &WriterCoreApi,
    starmap_id: &str,
    from_node_id: &str,
    to_node_id: &str,
    kind: &str,
    label: &str,
) -> String {
    let now = now_ms();
    let from = StarMapTargetPathDto {
        starmap_id: starmap_id.to_string(),
        segments: vec![],
        target: StarMapTargetDetailDto {
            kind: "node".to_string(),
            node_id: Some(from_node_id.to_string()),
            ..Default::default()
        },
    };
    let to = StarMapTargetPathDto {
        starmap_id: starmap_id.to_string(),
        segments: vec![],
        target: StarMapTargetDetailDto {
            kind: "node".to_string(),
            node_id: Some(to_node_id.to_string()),
            ..Default::default()
        },
    };
    let edge = StarMapEdgeDto {
        id: format!("e_{}", uuid::Uuid::new_v4()),
        from,
        to,
        kind: parse_edge_kind(kind),
        label: if label.is_empty() {
            None
        } else {
            Some(label.to_string())
        },
        payload: None,
        created_at: now,
        updated_at: now,
    };

    envelope(api.add_starmap_edge(starmap_id, edge))
}

/// 用 fromPath/toPath 建边（path 版，支持 Node 和 Embed 作为端点）。
///
/// `from_path_json` / `to_path_json` 是 StarMapTargetPathDto 的 JSON，
/// 反序列化后直接作为 edge 的 from/to，调用 Core 的 add_starmap_edge。
/// 与 `create_starmap_edge`（仅支持 Node 端点）互补，供 QML 在 connect 模式下
/// 把 Node 或 Embed 作为连线端点使用。
pub fn create_starmap_edge_with_paths(
    api: &WriterCoreApi,
    starmap_id: &str,
    from_path_json: &str,
    to_path_json: &str,
    kind: &str,
    label: &str,
) -> String {
    let from: StarMapTargetPathDto = match serde_json::from_str(from_path_json) {
        Ok(p) => p,
        Err(e) => return envelope_err_str(&format!("Invalid fromPath JSON: {}", e)),
    };
    let to: StarMapTargetPathDto = match serde_json::from_str(to_path_json) {
        Ok(p) => p,
        Err(e) => return envelope_err_str(&format!("Invalid toPath JSON: {}", e)),
    };
    let now = now_ms();
    let edge = StarMapEdgeDto {
        id: format!("e_{}", uuid::Uuid::new_v4()),
        from,
        to,
        kind: parse_edge_kind(kind),
        label: if label.is_empty() {
            None
        } else {
            Some(label.to_string())
        },
        payload: None,
        created_at: now,
        updated_at: now,
    };

    envelope(api.add_starmap_edge(starmap_id, edge))
}

pub fn update_starmap_edge(
    api: &WriterCoreApi,
    starmap_id: &str,
    edge_id: &str,
    patch_json: &str,
) -> String {
    let patch: StarMapEdgePatchDto = match serde_json::from_str(patch_json) {
        Ok(p) => p,
        Err(e) => return envelope_err_str(&format!("Invalid patch JSON: {}", e)),
    };

    envelope(api.update_starmap_edge(starmap_id, edge_id, patch))
}

pub fn delete_starmap_edge(api: &WriterCoreApi, starmap_id: &str, edge_id: &str) -> String {
    envelope(api.delete_starmap_edge(starmap_id, edge_id))
}

// -----------------------------------------------------------------------------
// 星图子星图嵌入（embed）envelope 接口
// -----------------------------------------------------------------------------
//
// 子星图改回正式 Embed 语义：Core 的 StarMapEmbedDto 是嵌入的唯一真相源。
// bridge 层只负责生成 instance_id（`em_{uuid}`，与节点 `n_{uuid}` 模式一致）、
// 组装 host_path（指向当前星图）、调用 Core API，不复制业务状态机。

/// 创建子星图嵌入。
///
/// - `starmap_id`：宿主星图 id（当前星图）。
/// - `target_starmap_id`：被嵌入的子星图 id（由调用方先建好子星图再传入）。
/// - `label`：用户输入的子星图名称；空字符串存为 None。
/// - `x` / `y`：右键放置位置（宿主星图坐标系）。
///
/// `host_path` 指向当前星图（segments 空，target 用 Default），provenance 用默认值。
pub fn create_starmap_embed(
    api: &WriterCoreApi,
    starmap_id: &str,
    target_starmap_id: &str,
    label: &str,
    x: f64,
    y: f64,
) -> String {
    let now = now_ms();
    let embed = StarMapEmbedDto {
        instance_id: format!("em_{}", uuid::Uuid::new_v4()),
        target_starmap_id: target_starmap_id.to_string(),
        label: if label.is_empty() {
            None
        } else {
            Some(label.to_string())
        },
        position: StarMapPointDto {
            x: x as f32,
            y: y as f32,
        },
        host_path: StarMapTargetPathDto {
            starmap_id: starmap_id.to_string(),
            segments: vec![],
            target: StarMapTargetDetailDto::default(),
        },
        provenance: StarMapProvenanceDto::default(),
        created_at: now,
        updated_at: now,
    };

    envelope(api.add_starmap_embed(starmap_id, embed))
}

/// 原子创建子星图并嵌入父图（Issue #805 评论 5907045450 第 5 部分）。
///
/// 调用 Core 的 `WriterCoreApi::create_starmap_child_embed`，一次性完成：
/// 1. 创建子星图 meta + index；
/// 2. 在父图添加 Embed；
/// 3. 记录 workspace history。
///
/// 替代 QML 侧 `create_starmap -> create_starmap_embed -> 失败时 delete_starmap`
/// 的非原子拼接。Core 侧通过 `starmap_child_embed` journal 保证 crash-safe。
///
/// - `host_starmap_id`：宿主星图 id（当前星图）。
/// - `title`：子星图标题。
/// - `x` / `y`：Embed 在宿主图中的位置（宿主星图坐标系）。
///
/// 返回 `CreateStarMapChildEmbedResultDto` 的 JSON envelope。
pub fn create_starmap_child_embed(
    api: &WriterCoreApi,
    host_starmap_id: &str,
    title: &str,
    x: f64,
    y: f64,
) -> String {
    let position = StarMapPointDto {
        x: x as f32,
        y: y as f32,
    };
    envelope(api.create_starmap_child_embed(host_starmap_id, title, position))
}

/// 更新子星图嵌入。
///
/// `patch_json` 按 `StarMapEmbedPatchInputDto` 格式（label/clearLabel/position/hostPath），
/// 反序列化后 `Into<StarMapEmbedPatchDto>` 再调 Core。
pub fn update_starmap_embed(
    api: &WriterCoreApi,
    starmap_id: &str,
    instance_id: &str,
    patch_json: &str,
) -> String {
    let input: StarMapEmbedPatchInputDto = match serde_json::from_str(patch_json) {
        Ok(p) => p,
        Err(e) => return envelope_err_str(&format!("Invalid patch JSON: {}", e)),
    };
    let patch: StarMapEmbedPatchDto = input.into();

    envelope(api.update_starmap_embed(starmap_id, instance_id, patch))
}

/// 删除子星图嵌入。
pub fn delete_starmap_embed(api: &WriterCoreApi, starmap_id: &str, instance_id: &str) -> String {
    envelope(api.delete_starmap_embed(starmap_id, instance_id))
}

pub fn bind_starmap_to_project(api: &WriterCoreApi, starmap_id: &str, project_id: &str) -> String {
    envelope(api.bind_starmap_to_project(starmap_id, project_id))
}

pub fn set_main_starmap(api: &WriterCoreApi, starmap_id: &str, project_id: &str) -> String {
    envelope(api.set_main_starmap_for_project(starmap_id, project_id))
}

pub fn get_main_starmap(api: &WriterCoreApi, project_id: &str) -> String {
    envelope(api.get_main_starmap_for_project(project_id))
}

pub fn unbind_starmap(api: &WriterCoreApi, starmap_id: &str) -> String {
    envelope(api.unbind_starmap_from_project(starmap_id))
}

// -----------------------------------------------------------------------------
// 星图超链接（hyperlink）envelope 接口
// -----------------------------------------------------------------------------

pub fn add_starmap_hyperlink(
    api: &WriterCoreApi,
    starmap_id: &str,
    hyperlink_json: &str,
) -> String {
    let mut hl: StarMapHyperlinkDto = match serde_json::from_str(hyperlink_json) {
        Ok(h) => h,
        Err(e) => return envelope_err_str(&format!("Invalid hyperlink JSON: {}", e)),
    };
    // hyperlink_id 由 bridge 层生成。Core 的 add_starmap_hyperlink 直接使用传入的
    // hyperlink_id（重复则报 Duplicate），不会内部生成新 id，因此这里统一分配新 id，
    // 与 create_starmap_node 在 bridge 层生成 `n_{uuid}` 的模式一致。
    let now = now_ms();
    hl.hyperlink_id = format!("hl_{}", uuid::Uuid::new_v4());
    hl.created_at = now;
    hl.updated_at = now;
    envelope(api.add_starmap_hyperlink(starmap_id, hl))
}

pub fn update_starmap_hyperlink(
    api: &WriterCoreApi,
    starmap_id: &str,
    hyperlink_id: &str,
    patch_json: &str,
) -> String {
    let patch: StarMapHyperlinkPatchDto = match serde_json::from_str(patch_json) {
        Ok(p) => p,
        Err(e) => return envelope_err_str(&format!("Invalid patch JSON: {}", e)),
    };
    envelope(api.update_starmap_hyperlink(starmap_id, hyperlink_id, patch))
}

pub fn delete_starmap_hyperlink(
    api: &WriterCoreApi,
    starmap_id: &str,
    hyperlink_id: &str,
) -> String {
    envelope(api.delete_starmap_hyperlink(starmap_id, hyperlink_id))
}

pub fn list_starmap_hyperlinks(api: &WriterCoreApi, starmap_id: &str) -> String {
    envelope(api.list_starmap_hyperlinks(starmap_id))
}

/// 列出根星图，envelope 格式。
pub fn list_root_starmaps_json(api: &WriterCoreApi) -> String {
    envelope(api.list_root_starmaps())
}
