use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, with_core};

#[no_mangle]
pub unsafe extern "C" fn writer_core_list_starmaps() -> *mut c_char {
    match with_core(|core| {
        let starmaps = core.list_starmaps().map_err(|e| format!("{}", e))?;
        let json_arr: Vec<serde_json::Value> = starmaps
            .iter()
            .map(|sm| {
                serde_json::json!({
                    "id": sm.starmap_id,
                    "title": sm.title,
                    "description": sm.description,
                    "nodeCount": sm.node_count,
                    "edgeCount": sm.edge_count,
                    "projectId": sm.project_id,
                    "createdAt": sm.created_at,
                    "updatedAt": sm.updated_at,
                    "accentColor": sm.accent_color
                })
            })
            .collect();
        Ok(json_arr)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_list_starmaps_for_project(
    project_id: *const c_char,
) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    match with_core(|core| {
        let starmaps = core
            .list_starmaps_for_project(&pid)
            .map_err(|e| format!("{}", e))?;
        let json_arr: Vec<serde_json::Value> = starmaps
            .iter()
            .map(|sm| {
                serde_json::json!({
                    "id": sm.starmap_id,
                    "title": sm.title,
                    "description": sm.description,
                    "nodeCount": sm.node_count,
                    "edgeCount": sm.edge_count,
                    "projectId": sm.project_id,
                    "createdAt": sm.created_at,
                    "updatedAt": sm.updated_at,
                    "accentColor": sm.accent_color
                })
            })
            .collect();
        Ok(json_arr)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_get_starmap(starmap_id: *const c_char) -> *mut c_char {
    let sid = match c_str_to_rust(starmap_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid starmap_id: error {}", e),
            )
        }
    };
    match with_core(|core| {
        let sm = core.get_starmap(&sid).map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "id": sm.starmap_id,
            "title": sm.title,
            "description": sm.description,
            "nodeCount": sm.node_count,
            "edgeCount": sm.edge_count,
            "projectId": sm.project_id,
            "createdAt": sm.created_at,
            "updatedAt": sm.updated_at,
            "accentColor": sm.accent_color
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_get_starmap_graph(starmap_id: *const c_char) -> *mut c_char {
    let sid = match c_str_to_rust(starmap_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid starmap_id: error {}", e),
            )
        }
    };
    match with_core(|core| {
        let graph = core.get_starmap_graph(&sid).map_err(|e| format!("{}", e))?;
        Ok(serde_json::to_value(&graph).unwrap_or_default())
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_create_starmap(
    title: *const c_char,
    description: *const c_char,
) -> *mut c_char {
    let t = match c_str_to_rust(title) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid title: error {}", e)),
    };
    let d = match c_str_to_rust(description) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid description: error {}", e),
            )
        }
    };
    match with_core(|core| {
        let sm = core
            .create_starmap(&t, &d, None)
            .map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "id": sm.starmap_id,
            "title": sm.title,
            "description": sm.description,
            "nodeCount": sm.node_count,
            "edgeCount": sm.edge_count,
            "projectId": sm.project_id,
            "createdAt": sm.created_at,
            "updatedAt": sm.updated_at,
            "accentColor": sm.accent_color
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ALREADY_EXISTS", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_delete_starmap(starmap_id: *const c_char) -> *mut c_char {
    let sid = match c_str_to_rust(starmap_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid starmap_id: error {}", e),
            )
        }
    };
    match with_core(|core| {
        core.delete_starmap(&sid).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_rename_starmap(
    starmap_id: *const c_char,
    new_title: *const c_char,
) -> *mut c_char {
    let sid = match c_str_to_rust(starmap_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid starmap_id: error {}", e),
            )
        }
    };
    let t = match c_str_to_rust(new_title) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid new_title: error {}", e),
            )
        }
    };
    match with_core(|core| {
        let sm = core
            .rename_starmap(&sid, &t)
            .map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "id": sm.starmap_id,
            "title": sm.title,
            "description": sm.description,
            "nodeCount": sm.node_count,
            "edgeCount": sm.edge_count,
            "projectId": sm.project_id,
            "createdAt": sm.created_at,
            "updatedAt": sm.updated_at,
            "accentColor": sm.accent_color
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

/// # Safety
/// Returns a caller-owned C string containing JSON StarMapMotionPolicyDto. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_starmap_motion_policy() -> *mut c_char {
    match with_core(|core| {
        let policy = core.get_motion_policy().map_err(|e| format!("{}", e))?;
        Ok(serde_json::to_value(&policy).unwrap_or_default())
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ERROR", &e),
    }
}

/// # Safety
/// `starmap_id` must be a valid null-terminated UTF-8 C string.
/// Returns a caller-owned C string containing JSON StarMapLayoutDto. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_starmap_layout(starmap_id: *const c_char) -> *mut c_char {
    let sid = match c_str_to_rust(starmap_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid starmap_id: error {}", e),
            )
        }
    };
    match with_core(|core| {
        let layout = core
            .get_starmap_layout(&sid)
            .map_err(|e| format!("{}", e))?;
        Ok(serde_json::to_value(&layout).unwrap_or_default())
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

/// # Safety
/// `starmap_id` must be a valid null-terminated UTF-8 C string.
/// `layout_json` must be a valid null-terminated UTF-8 C string containing StarMapLayoutDto JSON.
/// Returns a caller-owned C string containing JSON boolean. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_save_starmap_layout(
    starmap_id: *const c_char,
    layout_json: *const c_char,
) -> *mut c_char {
    let sid = match c_str_to_rust(starmap_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid starmap_id: error {}", e),
            )
        }
    };
    let layout_str = match c_str_to_rust(layout_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid layout_json: error {}", e),
            )
        }
    };
    let layout: crate::starmap::types::StarMapLayout = match serde_json::from_str(&layout_str) {
        Ok(l) => l,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Failed to parse layout_json: {}", e),
            )
        }
    };
    match with_core(|core| {
        core.save_starmap_layout(&sid, &layout)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ERROR", &e),
    }
}

/// # Safety
/// `starmap_id` must be a valid null-terminated UTF-8 C string.
/// `viewport_json` must be a valid null-terminated UTF-8 C string containing StarMapViewportDto JSON.
/// Returns a caller-owned C string containing JSON boolean. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_save_starmap_viewport(
    starmap_id: *const c_char,
    viewport_json: *const c_char,
) -> *mut c_char {
    let sid = match c_str_to_rust(starmap_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid starmap_id: error {}", e),
            )
        }
    };
    let viewport_str = match c_str_to_rust(viewport_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid viewport_json: error {}", e),
            )
        }
    };
    let viewport: crate::starmap::types::StarMapViewport = match serde_json::from_str(&viewport_str)
    {
        Ok(v) => v,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Failed to parse viewport_json: {}", e),
            )
        }
    };
    match with_core(|core| {
        core.save_starmap_viewport(&sid, &viewport)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ERROR", &e),
    }
}

/// # Safety
/// `graph_json` must be a valid null-terminated UTF-8 C string containing StarMapGraphDto JSON.
/// Returns a caller-owned C string containing JSON array of StarMapEdgeRenderDto. Free with `writer_core_free_string`.
#[no_mangle]
// TODO(#597): 既有代码可读性技术债，待后续重构拆分
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless,
    deprecated
)]
pub unsafe extern "C" fn writer_core_compute_starmap_edge_renders(
    graph_json: *const c_char,
) -> *mut c_char {
    let graph_str = match c_str_to_rust(graph_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid graph_json: error {}", e),
            )
        }
    };
    let graph: crate::starmap::types::StarMapGraph = match serde_json::from_str(&graph_str) {
        Ok(g) => g,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Failed to parse graph_json: {}", e),
            )
        }
    };
    match with_core(|core| {
        // Get layout for the starmap to compute node centers
        let layout = core
            .get_starmap_layout(&graph.starmap_id)
            .map_err(|e| format!("{}", e))?;
        let node_centers: std::collections::HashMap<String, (f32, f32)> = layout
            .nodes
            .iter()
            .map(|node| {
                (
                    node.node_id.clone(),
                    (node.x + node.width / 2.0, node.y + node.height / 2.0),
                )
            })
            .collect();
        let edges: Vec<crate::starmap::render::EdgeInput> = graph
            .edges
            .into_iter()
            .filter_map(|edge| {
                let from = edge.from.filter(|id| !id.is_empty())?;
                let to = edge.to.filter(|id| !id.is_empty())?;
                Some(crate::starmap::render::EdgeInput {
                    id: edge.id,
                    from,
                    to,
                    label: edge.label,
                })
            })
            .collect();
        let renders = crate::starmap::render::compute_edge_renders(
            &edges,
            &node_centers,
            &crate::starmap::render::EdgeRenderParams::default(),
        );
        let json_arr: Vec<serde_json::Value> = renders
            .into_iter()
            .map(|r| {
                serde_json::to_value(crate::api::types::StarMapEdgeRenderDto::from(r))
                    .unwrap_or_default()
            })
            .collect();
        Ok(json_arr)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ERROR", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_flush_starmap_store(
    starmap_id_ptr: *const c_char,
) -> *mut c_char {
    let starmap_id = match c_str_to_rust(starmap_id_ptr) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid starmap_id: error {}", e),
            )
        }
    };
    match with_core(|core| {
        core.flush_starmap_store(&starmap_id)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ERROR", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_close_starmap_store(
    starmap_id_ptr: *const c_char,
) -> *mut c_char {
    let starmap_id = match c_str_to_rust(starmap_id_ptr) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid starmap_id: error {}", e),
            )
        }
    };
    match with_core(|core| {
        core.close_starmap_store(&starmap_id)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ERROR", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_flush_all_starmap_stores() -> *mut c_char {
    match with_core(|core| {
        core.flush_all_starmap_stores()
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ERROR", &e),
    }
}
