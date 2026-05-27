use writer_core::facade::WriterCore;
use writer_core::starmap::types::{StarMapEdge, StarMapEdgeKind, StarMapLayout, StarMapNode, StarMapNodeKind, StarMapNodePatch, StarMapEdgePatch};

pub fn list_starmaps(core: &WriterCore) -> String {
    match core.list_starmaps() {
        Ok(list) => serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string()),
        Err(_) => "[]".to_string(),
    }
}

pub fn list_starmaps_for_project(core: &WriterCore, project_id: &str) -> String {
    match core.list_starmaps_for_project(project_id) {
        Ok(list) => serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string()),
        Err(_) => "[]".to_string(),
    }
}

pub fn get_starmap(core: &WriterCore, starmap_id: &str) -> String {
    match core.get_starmap(starmap_id) {
        Ok(meta) => serde_json::to_string(&meta).unwrap_or_else(|_| "{}".to_string()),
        Err(_) => "{}".to_string(),
    }
}

pub fn create_starmap(core: &WriterCore, title: &str, description: &str, accent_color: Option<&str>) -> String {
    match core.create_starmap(title, description, accent_color) {
        Ok(meta) => serde_json::to_string(&meta).unwrap_or_else(|_| "{}".to_string()),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn create_child_starmap(core: &WriterCore, parent_id: &str, title: &str, description: &str, accent_color: Option<&str>) -> String {
    match core.create_child_starmap(parent_id, title, description, accent_color) {
        Ok(meta) => serde_json::to_string(&meta).unwrap_or_else(|_| "{}".to_string()),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}

pub fn rename_starmap(core: &WriterCore, starmap_id: &str, new_title: &str) -> String {
    match core.rename_starmap(starmap_id, new_title) {
        Ok(meta) => serde_json::to_string(&meta).unwrap_or_else(|_| "{}".to_string()),
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
        created_at: now,
        updated_at: now,
    };

    match core.add_starmap_node(starmap_id, node) {
        Ok(saved_node) => {
            if let Ok(mut layout) = core.get_starmap_layout(starmap_id) {
                layout.nodes.push(writer_core::starmap::types::StarMapLayoutNode {
                    node_id: saved_node.id.clone(),
                    x: x as f32,
                    y: y as f32,
                    width: 150.0,
                    height: 60.0,
                    radius: 30.0,
                    collapsed: false,
                    z_index: 0,
                });
                let _ = core.save_starmap_layout(starmap_id, &layout);
            }
            serde_json::json!({"success": true, "data": saved_node}).to_string()
        },
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
        Ok(Some(meta)) => serde_json::to_string(&meta).unwrap_or_else(|_| "{}".to_string()),
        Ok(None) => "{}".to_string(),
        Err(_) => "{}".to_string(),
    }
}

pub fn unbind_starmap(core: &WriterCore, starmap_id: &str) -> String {
    match core.unbind_starmap_from_project(starmap_id) {
        Ok(()) => serde_json::json!({"success": true}).to_string(),
        Err(e) => serde_json::json!({"success": false, "message": format!("{}", e)}).to_string(),
    }
}
