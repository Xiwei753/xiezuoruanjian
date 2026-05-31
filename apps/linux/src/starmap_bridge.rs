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
// - 被 apps/linux/src/backend/starmap_backend.rs 引用，作为后端 QObject 完成星图数据管理的执行模块。
// =============================================================================

//! # 星图桥接函数（Linux UI 层 - Backend Adapter）
//!
//! 将 WriterCoreApi 的星图 API 包装为兼容 DTO，供 AppBackend 转为 QML 对象。

use writer_core::api::types::{
    StarMapEdgeDto, StarMapEdgeKindDto, StarMapEdgePatchDto, StarMapEmbedDto,
    StarMapEmbedPatchDto, StarMapLayoutDto, StarMapLinkDto, StarMapLinkPatchDto,
    StarMapNodeContentDto, StarMapNodeDto, StarMapNodeKindDto, StarMapNodePatchDto,
};
use writer_core::api::WriterCoreApi;

fn parse_node_kind(kind: &str) -> StarMapNodeKindDto {
    serde_json::from_value(serde_json::json!(kind)).unwrap_or(StarMapNodeKindDto::Note)
}

fn parse_edge_kind(kind: &str) -> StarMapEdgeKindDto {
    serde_json::from_value(serde_json::json!(kind)).unwrap_or(StarMapEdgeKindDto::RelatedTo)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

pub fn list_starmaps(api: &WriterCoreApi) -> String {
    match api.list_starmaps() {
        Ok(list) => serde_json::json!({"success": true, "data": list}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn list_starmaps_for_project(api: &WriterCoreApi, project_id: &str) -> String {
    match api.list_starmaps_for_project(project_id) {
        Ok(list) => serde_json::json!({"success": true, "data": list}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn get_starmap(api: &WriterCoreApi, starmap_id: &str) -> String {
    match api.get_starmap(starmap_id) {
        Ok(meta) => serde_json::json!({"success": true, "data": meta}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn create_starmap(api: &WriterCoreApi, title: &str, description: &str, accent_color: Option<&str>) -> String {
    match api.create_starmap(title, description, accent_color) {
        Ok(meta) => serde_json::json!({"success": true, "data": meta}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn create_child_starmap_legacy(api: &WriterCoreApi, parent_id: &str, title: &str, description: &str, accent_color: Option<&str>) -> String {
    match api.create_child_starmap_legacy(parent_id, title, description, accent_color) {
        Ok(meta) => serde_json::json!({"success": true, "data": meta}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn rename_starmap(api: &WriterCoreApi, starmap_id: &str, new_title: &str) -> String {
    match api.rename_starmap(starmap_id, new_title) {
        Ok(meta) => serde_json::json!({"success": true, "data": meta}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn delete_starmap(api: &WriterCoreApi, starmap_id: &str) -> String {
    match api.delete_starmap(starmap_id) {
        Ok(_) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn get_starmap_graph_and_layout(api: &WriterCoreApi, starmap_id: &str) -> String {
    match api.get_starmap_graph(starmap_id) {
        Ok(g) => match api.get_starmap_layout(starmap_id) {
            Ok(l) => serde_json::json!({
                "success": true,
                "data": { "graph": g, "layout": l }
            }).to_string(),
            Err(_) => serde_json::json!({
                "success": true,
                "data": { "graph": g, "layout": null }
            }).to_string(),
        },
        Err(e) => serde_json::json!({
            "success": false,
            "userMessage": if e.to_string().contains("not bound to a project") { "请先绑定作品" } else { "加载失败" },
            "message": format!("{}", e)
        }).to_string(),
    }
}

pub fn create_starmap_node(api: &WriterCoreApi, starmap_id: &str, title: &str, kind: &str, x: f64, y: f64) -> String {
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

    match api.add_starmap_node(starmap_id, node, x as f32, y as f32) {
        Ok(saved_node) => serde_json::json!({"success": true, "data": saved_node}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn update_starmap_node(api: &WriterCoreApi, starmap_id: &str, node_id: &str, patch_json: &str) -> String {
    let patch: StarMapNodePatchDto = match serde_json::from_str(patch_json) {
        Ok(p) => p,
        Err(_) => return serde_json::json!({"success": false, "message": "Invalid patch JSON"}).to_string(),
    };

    match api.update_starmap_node(starmap_id, node_id, patch) {
        Ok(node) => serde_json::json!({"success": true, "data": node}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn delete_starmap_node(api: &WriterCoreApi, starmap_id: &str, node_id: &str) -> String {
    match api.delete_starmap_node(starmap_id, node_id) {
        Ok(_) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn create_starmap_edge(api: &WriterCoreApi, starmap_id: &str, from_node_id: &str, to_node_id: &str, kind: &str, label: &str) -> String {
    let now = now_ms();
    let edge = StarMapEdgeDto {
        id: format!("e_{}", uuid::Uuid::new_v4()),
        from: Some(from_node_id.to_string()),
        to: Some(to_node_id.to_string()),
        kind: parse_edge_kind(kind),
        label: if label.is_empty() { None } else { Some(label.to_string()) },
        payload: None,
        from_target: None,
        to_target: None,
        from_endpoint: None,
        to_endpoint: None,
        created_at: now,
        updated_at: now,
    };

    match api.add_starmap_edge(starmap_id, edge) {
        Ok(saved_edge) => serde_json::json!({"success": true, "data": saved_edge}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn update_starmap_edge(api: &WriterCoreApi, starmap_id: &str, edge_id: &str, patch_json: &str) -> String {
    let patch: StarMapEdgePatchDto = match serde_json::from_str(patch_json) {
        Ok(p) => p,
        Err(_) => return serde_json::json!({"success": false, "message": "Invalid patch JSON"}).to_string(),
    };

    match api.update_starmap_edge(starmap_id, edge_id, patch) {
        Ok(edge) => serde_json::json!({"success": true, "data": edge}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn delete_starmap_edge(api: &WriterCoreApi, starmap_id: &str, edge_id: &str) -> String {
    match api.delete_starmap_edge(starmap_id, edge_id) {
        Ok(_) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn save_starmap_layout(api: &WriterCoreApi, starmap_id: &str, layout_json: &str) -> String {
    let layout: StarMapLayoutDto = match serde_json::from_str(layout_json) {
        Ok(l) => l,
        Err(_) => return serde_json::json!({"success": false, "message": "Invalid layout JSON"}).to_string(),
    };

    match api.save_starmap_layout(starmap_id, &layout) {
        Ok(_) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn bind_starmap_to_project(api: &WriterCoreApi, starmap_id: &str, project_id: &str) -> String {
    match api.bind_starmap_to_project(starmap_id, project_id) {
        Ok(_) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn set_main_starmap(api: &WriterCoreApi, starmap_id: &str, project_id: &str) -> String {
    match api.set_main_starmap_for_project(starmap_id, project_id) {
        Ok(_) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn get_main_starmap(api: &WriterCoreApi, project_id: &str) -> String {
    match api.get_main_starmap_for_project(project_id) {
        Ok(Some(meta)) => serde_json::json!({"success": true, "data": meta}).to_string(),
        Ok(None) => serde_json::json!({"success": true, "data": null}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn unbind_starmap(api: &WriterCoreApi, starmap_id: &str) -> String {
    match api.unbind_starmap_from_project(starmap_id) {
        Ok(_) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn add_starmap_embed(api: &WriterCoreApi, starmap_id: &str, embed_json: &str) -> String {
    let embed: StarMapEmbedDto = match serde_json::from_str(embed_json) {
        Ok(e) => e,
        Err(_) => return serde_json::json!({"success": false, "message": "Invalid embed JSON"}).to_string(),
    };
    match api.add_starmap_embed(starmap_id, embed) {
        Ok(saved_embed) => serde_json::json!({"success": true, "data": saved_embed}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn update_starmap_embed(api: &WriterCoreApi, starmap_id: &str, instance_id: &str, patch_json: &str) -> String {
    let patch: StarMapEmbedPatchDto = match serde_json::from_str(patch_json) {
        Ok(p) => p,
        Err(_) => return serde_json::json!({"success": false, "message": "Invalid patch JSON"}).to_string(),
    };
    match api.update_starmap_embed(starmap_id, instance_id, patch) {
        Ok(embed) => serde_json::json!({"success": true, "data": embed}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn delete_starmap_embed(api: &WriterCoreApi, starmap_id: &str, instance_id: &str) -> String {
    match api.delete_starmap_embed(starmap_id, instance_id) {
        Ok(_) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn add_starmap_link(api: &WriterCoreApi, starmap_id: &str, link_json: &str) -> String {
    let link: StarMapLinkDto = match serde_json::from_str(link_json) {
        Ok(l) => l,
        Err(_) => return serde_json::json!({"success": false, "message": "Invalid link JSON"}).to_string(),
    };
    match api.add_starmap_link(starmap_id, link) {
        Ok(saved_link) => serde_json::json!({"success": true, "data": saved_link}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn update_starmap_link(api: &WriterCoreApi, starmap_id: &str, link_id: &str, patch_json: &str) -> String {
    let patch: StarMapLinkPatchDto = match serde_json::from_str(patch_json) {
        Ok(p) => p,
        Err(_) => return serde_json::json!({"success": false, "message": "Invalid patch JSON"}).to_string(),
    };
    match api.update_starmap_link(starmap_id, link_id, patch) {
        Ok(link) => serde_json::json!({"success": true, "data": link}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn delete_starmap_link(api: &WriterCoreApi, starmap_id: &str, link_id: &str) -> String {
    match api.delete_starmap_link(starmap_id, link_id) {
        Ok(_) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn find_starmap_references(api: &WriterCoreApi, target_starmap_id: &str) -> String {
    match api.find_starmap_references(target_starmap_id) {
        Ok(refs) => serde_json::json!({"success": true, "data": refs}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}
