// =============================================================================
// starmap_bridge.rs — 星图模块底层桥接器
// =============================================================================
//
// 引用了什么：
// - writer_core::api::types::*：星图节点、边、布局及相关 Patch 更新 DTO。
// - writer_core::api::WriterCoreApi：核心库主业务 API。
//
// 干什么的：
// - 负责星图领域核心 DTO 到客户端需要的兼容 JSON 字符串的双向数据编解码与类型转换。
// - 提供星图生命周期（列表获取、绑定/解绑作品、创建/重命名/删除星图）的底层桥接。
// - 提供图数据点、线、多维布局及嵌入式富文本元素（add_starmap_embed 等）的增删改查动作。
//
// 被什么引用：
// - 被 apps/Linux_qt/src/backend/starmap_backend.rs 引用，作为后端 QObject 完成星图数据管理的执行模块。
// =============================================================================


use writer_core::api::types::{
    StarMapEdgeDto, StarMapEdgeKindDto, StarMapEdgePatchDto,
    StarMapLayoutDto, StarMapNodeContentDto, StarMapNodeDto,
    StarMapNodeKindDto, StarMapNodePatchDto,
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

fn envelope_ok<T: serde::Serialize>(data: T) -> String {
    envelope(Ok(data))
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

pub fn create_child_starmap(
    api: &WriterCoreApi,
    parent_id: &str,
    title: &str,
    description: &str,
    accent_color: Option<&str>,
) -> String {
    envelope(api.create_child_starmap(parent_id, title, description, accent_color))
}

pub fn rename_starmap(api: &WriterCoreApi, starmap_id: &str, new_title: &str) -> String {
    envelope(api.rename_starmap(starmap_id, new_title))
}

pub fn delete_starmap(api: &WriterCoreApi, starmap_id: &str) -> String {
    envelope(api.delete_starmap(starmap_id))
}

pub fn get_starmap_graph_and_layout(api: &WriterCoreApi, starmap_id: &str) -> String {
    match api.get_starmap_graph(starmap_id) {
        Ok(g) => match api.get_starmap_layout(starmap_id) {
            Ok(l) => envelope_ok(serde_json::json!({ "graph": g, "layout": l })),
            Err(_) => envelope_ok(serde_json::json!({ "graph": g, "layout": null })),
        },
        Err(e) => envelope_err_str(&e.to_string()),
    }
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
        display_policy: Default::default(),
        open_behavior: Default::default(),
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

pub fn create_starmap_edge(
    api: &WriterCoreApi,
    starmap_id: &str,
    from_node_id: &str,
    to_node_id: &str,
    kind: &str,
    label: &str,
) -> String {
    let now = now_ms();
    let edge = StarMapEdgeDto {
        id: format!("e_{}", uuid::Uuid::new_v4()),
        from: Some(from_node_id.to_string()),
        to: Some(to_node_id.to_string()),
        kind: parse_edge_kind(kind),
        label: if label.is_empty() {
            None
        } else {
            Some(label.to_string())
        },
        payload: None,
        from_target: None,
        to_target: None,
        from_endpoint: None,
        to_endpoint: None,
        from_endpoint_path: None,
        to_endpoint_path: None,
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

pub fn save_starmap_layout(api: &WriterCoreApi, starmap_id: &str, layout_json: &str) -> String {
    let layout: StarMapLayoutDto = match serde_json::from_str(layout_json) {
        Ok(l) => l,
        Err(e) => return envelope_err_str(&format!("Invalid layout JSON: {}", e)),
    };

    envelope(api.save_starmap_layout(starmap_id, &layout))
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

pub fn compute_edge_renders_json(edges_json: &str, nodes_json: &str) -> String {
    let edges: Vec<writer_core::starmap::render::EdgeInput> =
        match serde_json::from_str(edges_json) {
            Ok(v) => v,
            Err(e) => return envelope_err_str(&format!("Invalid edges JSON: {}", e)),
        };

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct NodePos {
        id: String,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    }

    let nodes: Vec<NodePos> = match serde_json::from_str(nodes_json) {
        Ok(v) => v,
        Err(e) => return envelope_err_str(&format!("Invalid nodes JSON: {}", e)),
    };

    let centers: std::collections::HashMap<String, (f32, f32)> = nodes
        .into_iter()
        .map(|n| (n.id, (n.x + n.width / 2.0, n.y + n.height / 2.0)))
        .collect();

    let renders = writer_core::starmap::render::compute_edge_renders(
        &edges,
        &centers,
        &writer_core::starmap::render::EdgeRenderParams::default(),
    );
    envelope_ok(renders)
}

pub fn hit_test_edge_renders_json(renders_json: &str, x: f32, y: f32) -> String {
    let renders: Vec<writer_core::starmap::render::EdgeRender> =
        match serde_json::from_str(renders_json) {
            Ok(v) => v,
            Err(e) => return envelope_err_str(&format!("Invalid renders JSON: {}", e)),
        };

    let result = writer_core::starmap::render::hit_test_edge_renders(x, y, &renders);
    envelope_ok(result)
}

pub fn hit_test_nodes_json(nodes_json: &str, x: f32, y: f32) -> String {
    let nodes: Vec<writer_core::starmap::types::StarMapLayoutNode> =
        match serde_json::from_str(nodes_json) {
            Ok(v) => v,
            Err(e) => return envelope_err_str(&format!("Invalid nodes JSON: {}", e)),
        };

    let result = writer_core::starmap::hittest::hit_test_nodes(x, y, &nodes);
    envelope_ok(result.map(|r| r.id))
}

pub fn calculate_grid_layout_json(node_ids_json: &str, existing_layout_json: &str) -> String {
    let node_ids: Vec<String> = match serde_json::from_str(node_ids_json) {
        Ok(v) => v,
        Err(e) => return envelope_err_str(&format!("Invalid node IDs JSON: {}", e)),
    };

    let existing: writer_core::starmap::types::StarMapLayout =
        serde_json::from_str(existing_layout_json).unwrap_or_default();

    let layout = writer_core::starmap::layout::calculate_grid_layout(&node_ids, &existing);
    envelope_ok(layout)
}
