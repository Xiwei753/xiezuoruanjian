//! # 星图桥接函数（Linux UI 层 - Backend Adapter）
//!
//! 将 WriterCore 的星图 API 包装为兼容 DTO，供 AppBackend 转为 QML 对象。
//! TODO(api): migrate when WriterCoreApi exposes this capability
//!
//! ## 架构定位
//!
//! ```text
//! QML StarMapGraphController → starmap_bridge::list_starmaps()
//!   → WriterCore::list_starmaps()
//!     → starmap::list_starmaps()
//! ```
//!
//! ## 职责边界
//!
//! - **做**：类型转换（Rust 结构体 ↔ 兼容 DTO）、错误格式化
//! - **不做**：业务逻辑（全部委托给 WriterCore）
//! - **不做**：文件 I/O（由 WriterCore 负责）
//!
//! ## 兼容协议
//!
//! 旧函数仍返回 JSON 字符串，AppBackend 新接口会转为 `QJsonObject` / `QJsonArray`：
//! - 成功：`{ "success": true, "data": ... }`
//! - 失败：`{ "success": false, "message": "..." }`

use writer_core::facade::WriterCore;
use writer_core::starmap::types::{StarMapEdge, StarMapEdgeKind, StarMapLayout, StarMapNode, StarMapNodeKind, StarMapNodePatch, StarMapEdgePatch};

pub fn list_starmaps(core: &WriterCore) -> String {
    match core.list_starmaps() {
        Ok(list) => serde_json::json!({"success": true, "data": list}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn list_starmaps_for_project(core: &WriterCore, project_id: &str) -> String {
    match core.list_starmaps_for_project(project_id) {
        Ok(list) => serde_json::json!({"success": true, "data": list}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn get_starmap(core: &WriterCore, starmap_id: &str) -> String {
    match core.get_starmap(starmap_id) {
        Ok(meta) => serde_json::json!({"success": true, "data": meta}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn create_starmap(core: &WriterCore, title: &str, description: &str, accent_color: Option<&str>) -> String {
    match core.create_starmap(title, description, accent_color) {
        Ok(meta) => serde_json::json!({"success": true, "data": meta}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn create_child_starmap(core: &WriterCore, parent_id: &str, title: &str, description: &str, accent_color: Option<&str>) -> String {
    match core.create_child_starmap(parent_id, title, description, accent_color) {
        Ok(meta) => serde_json::json!({"success": true, "data": meta}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn rename_starmap(core: &WriterCore, starmap_id: &str, new_title: &str) -> String {
    match core.rename_starmap(starmap_id, new_title) {
        Ok(meta) => serde_json::json!({"success": true, "data": meta}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn delete_starmap(core: &WriterCore, starmap_id: &str) -> String {
    match core.delete_starmap(starmap_id) {
        Ok(()) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn get_starmap_graph_and_layout(core: &WriterCore, starmap_id: &str) -> String {
    match core.get_starmap_graph(starmap_id) {
        Ok(g) => match core.get_starmap_layout(starmap_id) {
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

pub fn create_starmap_node(core: &WriterCore, starmap_id: &str, title: &str, kind: &str, x: f64, y: f64) -> String {
    let node_kind: StarMapNodeKind = serde_json::from_value(serde_json::json!(kind)).unwrap_or(StarMapNodeKind::Note);
    let id = format!("n_{}", uuid::Uuid::new_v4());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;

    let node = StarMapNode {
        id,
        title: title.to_string(),
        kind: node_kind,
        payload: None,
        tags: vec![],
        content: Default::default(),
        anchors: vec![],
        portal: None,
        display_policy: Default::default(),
        open_behavior: Default::default(),
        provenance: Default::default(),
        created_at: now,
        updated_at: now,
    };

    match core.add_starmap_node(starmap_id, node, x as f32, y as f32) {
        Ok(saved_node) => serde_json::json!({"success": true, "data": saved_node}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn update_starmap_node(core: &WriterCore, starmap_id: &str, node_id: &str, patch_json: &str) -> String {
    let patch: StarMapNodePatch = match serde_json::from_str(patch_json) {
        Ok(p) => p,
        Err(_) => return serde_json::json!({"success": false, "message": "Invalid patch JSON"}).to_string(),
    };

    match core.update_starmap_node(starmap_id, node_id, patch) {
        Ok(node) => serde_json::json!({"success": true, "data": node}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn delete_starmap_node(core: &WriterCore, starmap_id: &str, node_id: &str) -> String {
    match core.delete_starmap_node(starmap_id, node_id) {
        Ok(_) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn create_starmap_edge(core: &WriterCore, starmap_id: &str, from_node_id: &str, to_node_id: &str, kind: &str, label: &str) -> String {
    let edge_kind: StarMapEdgeKind = serde_json::from_value(serde_json::json!(kind)).unwrap_or(StarMapEdgeKind::RelatedTo);
    let id = format!("e_{}", uuid::Uuid::new_v4());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;

    let edge = StarMapEdge {
        id,
        from: from_node_id.to_string(),
        to: to_node_id.to_string(),
        kind: edge_kind,
        label: if label.is_empty() { None } else { Some(label.to_string()) },
        payload: None,
        from_target: None,
        to_target: None,
        created_at: now,
        updated_at: now,
    };

    match core.add_starmap_edge(starmap_id, edge) {
        Ok(saved_edge) => serde_json::json!({"success": true, "data": saved_edge}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn update_starmap_edge(core: &WriterCore, starmap_id: &str, edge_id: &str, patch_json: &str) -> String {
    let patch: StarMapEdgePatch = match serde_json::from_str(patch_json) {
        Ok(p) => p,
        Err(_) => return serde_json::json!({"success": false, "message": "Invalid patch JSON"}).to_string(),
    };

    match core.update_starmap_edge(starmap_id, edge_id, patch) {
        Ok(edge) => serde_json::json!({"success": true, "data": edge}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn delete_starmap_edge(core: &WriterCore, starmap_id: &str, edge_id: &str) -> String {
    match core.delete_starmap_edge(starmap_id, edge_id) {
        Ok(_) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn save_starmap_layout(core: &WriterCore, starmap_id: &str, layout_json: &str) -> String {
    let layout: StarMapLayout = match serde_json::from_str(layout_json) {
        Ok(l) => l,
        Err(_) => return serde_json::json!({"success": false, "message": "Invalid layout JSON"}).to_string(),
    };

    match core.save_starmap_layout(starmap_id, &layout) {
        Ok(_) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn bind_starmap_to_project(core: &WriterCore, starmap_id: &str, project_id: &str) -> String {
    match core.bind_starmap_to_project(starmap_id, project_id) {
        Ok(()) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn set_main_starmap(core: &WriterCore, starmap_id: &str, project_id: &str) -> String {
    match core.set_main_starmap_for_project(starmap_id, project_id) {
        Ok(()) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn get_main_starmap(core: &WriterCore, project_id: &str) -> String {
    match core.get_main_starmap_for_project(project_id) {
        Ok(Some(meta)) => serde_json::json!({"success": true, "data": meta}).to_string(),
        Ok(None) => serde_json::json!({"success": true, "data": null}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn unbind_starmap(core: &WriterCore, starmap_id: &str) -> String {
    match core.unbind_starmap_from_project(starmap_id) {
        Ok(()) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn add_starmap_embed(core: &WriterCore, starmap_id: &str, embed_json: &str) -> String {
    let embed: writer_core::starmap::types::StarMapEmbed = match serde_json::from_str(embed_json) {
        Ok(e) => e,
        Err(_) => return serde_json::json!({"success": false, "message": "Invalid embed JSON"}).to_string(),
    };
    match core.add_starmap_embed(starmap_id, embed) {
        Ok(saved_embed) => serde_json::json!({"success": true, "data": saved_embed}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn update_starmap_embed(core: &WriterCore, starmap_id: &str, instance_id: &str, patch_json: &str) -> String {
    let patch: writer_core::starmap::types::StarMapEmbedPatch = match serde_json::from_str(patch_json) {
        Ok(p) => p,
        Err(_) => return serde_json::json!({"success": false, "message": "Invalid patch JSON"}).to_string(),
    };
    match core.update_starmap_embed(starmap_id, instance_id, patch) {
        Ok(embed) => serde_json::json!({"success": true, "data": embed}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn delete_starmap_embed(core: &WriterCore, starmap_id: &str, instance_id: &str) -> String {
    match core.delete_starmap_embed(starmap_id, instance_id) {
        Ok(_) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn add_starmap_link(core: &WriterCore, starmap_id: &str, link_json: &str) -> String {
    let link: writer_core::starmap::types::StarMapLink = match serde_json::from_str(link_json) {
        Ok(l) => l,
        Err(_) => return serde_json::json!({"success": false, "message": "Invalid link JSON"}).to_string(),
    };
    match core.add_starmap_link(starmap_id, link) {
        Ok(saved_link) => serde_json::json!({"success": true, "data": saved_link}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn update_starmap_link(core: &WriterCore, starmap_id: &str, link_id: &str, patch_json: &str) -> String {
    let patch: writer_core::starmap::types::StarMapLinkPatch = match serde_json::from_str(patch_json) {
        Ok(p) => p,
        Err(_) => return serde_json::json!({"success": false, "message": "Invalid patch JSON"}).to_string(),
    };
    match core.update_starmap_link(starmap_id, link_id, patch) {
        Ok(link) => serde_json::json!({"success": true, "data": link}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn delete_starmap_link(core: &WriterCore, starmap_id: &str, link_id: &str) -> String {
    match core.delete_starmap_link(starmap_id, link_id) {
        Ok(_) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn find_starmap_references(core: &WriterCore, target_starmap_id: &str) -> String {
    match core.find_starmap_references(target_starmap_id) {
        Ok(refs) => serde_json::json!({"success": true, "data": refs}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}
