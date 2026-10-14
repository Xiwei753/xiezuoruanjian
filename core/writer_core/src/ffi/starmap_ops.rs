use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, with_app_service};
use crate::api::{
    StarMapEdgeDto, StarMapEmbedDto, StarMapEmbedPatchInputDto, StarMapEndpointSelectionDto,
    StarMapMetaDto, StarMapNavigationRouteDto, StarMapNodeDto, StarMapNodePatchInputDto,
    StarMapPathSegmentDto, StarMapPointDto, StarMapTargetDetailDto,
};

#[no_mangle]
/// # Safety
///
/// This function does not take any pointer arguments, so there are no additional
/// safety requirements beyond those inherent to FFI boundary calls.
pub unsafe extern "C" fn writer_core_list_starmaps() -> *mut c_char {
    match with_app_service(|svc| {
        let starmaps = svc.list_starmaps().map_err(|e| format!("{}", e))?;
        Ok(starmaps)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// This function does not take any pointer arguments, so there are no additional
/// safety requirements beyond those inherent to FFI boundary calls.
pub unsafe extern "C" fn writer_core_list_root_starmaps() -> *mut c_char {
    match with_app_service(|svc| {
        let starmaps = svc.list_root_starmaps().map_err(|e| format!("{}", e))?;
        Ok(starmaps)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `project_id` points to a valid, null-terminated C string.
/// Passing a null pointer or an invalid pointer is undefined behavior.
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
    match with_app_service(|svc| {
        let starmaps = svc
            .list_starmaps_for_project(&pid)
            .map_err(|e| format!("{}", e))?;
        let dtos: Vec<StarMapMetaDto> = starmaps.into_iter().map(StarMapMetaDto::from).collect();
        Ok(dtos)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `starmap_id` points to a valid, null-terminated C string.
/// Passing a null pointer or an invalid pointer is undefined behavior.
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
    match with_app_service(|svc| {
        let sm = svc.get_starmap(&sid).map_err(|e| format!("{}", e))?;
        Ok(StarMapMetaDto::from(sm))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `starmap_id` points to a valid, null-terminated C string.
/// Passing a null pointer or an invalid pointer is undefined behavior.
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
    match with_app_service(|svc| {
        let graph = svc.get_starmap_graph(sid).map_err(|e| format!("{}", e))?;
        Ok(graph)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `title` and `description` point to valid, null-terminated C strings.
/// Passing null pointers or invalid pointers is undefined behavior.
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
    match with_app_service(|svc| {
        let sm = svc.create_starmap(t, d).map_err(|e| format!("{}", e))?;
        Ok(sm)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ALREADY_EXISTS", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `starmap_id` points to a valid, null-terminated C string.
/// Passing a null pointer or an invalid pointer is undefined behavior.
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
    match with_app_service(|svc| {
        svc.delete_starmap_raw(&sid).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure both `starmap_id` and `new_title` point to valid,
/// null-terminated C strings. Passing null pointers or invalid pointers is undefined behavior.
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
    match with_app_service(|svc| {
        let sm = svc
            .rename_starmap_raw(&sid, &t)
            .map_err(|e| format!("{}", e))?;
        Ok(StarMapMetaDto::from(sm))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `starmap_id_ptr` points to a valid, null-terminated C string.
/// Passing a null pointer or an invalid pointer is undefined behavior.
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
    match with_app_service(|svc| {
        svc.flush_starmap_store(starmap_id)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ERROR", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `starmap_id_ptr` points to a valid, null-terminated C string.
/// Passing a null pointer or an invalid pointer is undefined behavior.
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
    match with_app_service(|svc| {
        svc.close_starmap_store(starmap_id)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ERROR", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// This function does not take any pointer arguments, so there are no additional
/// safety requirements beyond those inherent to FFI boundary calls.
pub unsafe extern "C" fn writer_core_flush_all_starmap_stores() -> *mut c_char {
    match with_app_service(|svc| {
        svc.flush_all_starmap_stores()
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ERROR", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `starmap_id`, `node_id`, and `patch_json` all point to
/// valid, null-terminated C strings. Passing null pointers or invalid pointers is
/// undefined behavior.
pub unsafe extern "C" fn writer_core_update_starmap_node(
    starmap_id: *const c_char,
    node_id: *const c_char,
    patch_json: *const c_char,
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
    let nid = match c_str_to_rust(node_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid node_id: error {}", e)),
    };
    let patch_str = match c_str_to_rust(patch_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid patch_json: error {}", e),
            )
        }
    };
    let patch = match serde_json::from_str::<StarMapNodePatchInputDto>(&patch_str) {
        Ok(p) => p,
        Err(e) => return err_json("PARSE_ERROR", &format!("Failed to parse patch_json: {}", e)),
    };
    match with_app_service(|svc| {
        let node = svc
            .update_starmap_node(sid, nid, patch)
            .map_err(|e| format!("{}", e))?;
        Ok(node)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `starmap_id` and `edge_json` both point to valid,
/// null-terminated C strings. Passing null pointers or invalid pointers is
/// undefined behavior.
pub unsafe extern "C" fn writer_core_add_starmap_edge(
    starmap_id: *const c_char,
    edge_json: *const c_char,
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
    let edge_str = match c_str_to_rust(edge_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid edge_json: error {}", e),
            )
        }
    };
    let edge = match serde_json::from_str::<StarMapEdgeDto>(&edge_str) {
        Ok(e) => e,
        Err(e) => return err_json("PARSE_ERROR", &format!("Failed to parse edge_json: {}", e)),
    };
    match with_app_service(|svc| {
        let result = svc
            .add_starmap_edge(sid, edge)
            .map_err(|e| format!("{}", e))?;
        Ok(result)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ERROR", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `starmap_id` and `node_json` both point to valid,
/// null-terminated C strings. Passing null pointers or invalid pointers is
/// undefined behavior.
pub unsafe extern "C" fn writer_core_add_starmap_node(
    starmap_id: *const c_char,
    node_json: *const c_char,
    x: f64,
    y: f64,
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
    let node_str = match c_str_to_rust(node_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid node_json: error {}", e),
            )
        }
    };
    let node = match serde_json::from_str::<StarMapNodeDto>(&node_str) {
        Ok(n) => n,
        Err(e) => return err_json("PARSE_ERROR", &format!("Failed to parse node_json: {}", e)),
    };
    match with_app_service(|svc| {
        let result = svc
            .add_starmap_node(sid, node, x, y)
            .map_err(|e| format!("{}", e))?;
        Ok(result)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ERROR", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `starmap_id` and `embed_json` both point to valid,
/// null-terminated C strings. Passing null pointers or invalid pointers is
/// undefined behavior.
pub unsafe extern "C" fn writer_core_add_starmap_embed(
    starmap_id: *const c_char,
    embed_json: *const c_char,
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
    let embed_str = match c_str_to_rust(embed_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid embed_json: error {}", e),
            )
        }
    };
    let embed = match serde_json::from_str::<StarMapEmbedDto>(&embed_str) {
        Ok(e) => e,
        Err(e) => return err_json("PARSE_ERROR", &format!("Failed to parse embed_json: {}", e)),
    };
    match with_app_service(|svc| {
        let result = svc
            .add_starmap_embed(sid, embed)
            .map_err(|e| format!("{}", e))?;
        Ok(result)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ERROR", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `host_starmap_id`, `title`, and `position_json` all point to
/// valid, null-terminated C strings. Passing null pointers or invalid pointers is
/// undefined behavior.
pub unsafe extern "C" fn writer_core_create_starmap_child_embed(
    host_starmap_id: *const c_char,
    title: *const c_char,
    position_json: *const c_char,
) -> *mut c_char {
    let host_id = match c_str_to_rust(host_starmap_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid host_starmap_id: error {}", e),
            )
        }
    };
    let title_str = match c_str_to_rust(title) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid title: error {}", e)),
    };
    let position_str = match c_str_to_rust(position_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid position_json: error {}", e),
            )
        }
    };
    let position = match serde_json::from_str::<StarMapPointDto>(&position_str) {
        Ok(p) => p,
        Err(e) => {
            return err_json(
                "PARSE_ERROR",
                &format!("Failed to parse position_json: {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let result = svc
            .create_starmap_child_embed(host_id, title_str, position)
            .map_err(|e| format!("{}", e))?;
        Ok(result)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ERROR", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `starmap_id`, `instance_id`, and `patch_json` all point to
/// valid, null-terminated C strings. Passing null pointers or invalid pointers is
/// undefined behavior.
pub unsafe extern "C" fn writer_core_update_starmap_embed(
    starmap_id: *const c_char,
    instance_id: *const c_char,
    patch_json: *const c_char,
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
    let iid = match c_str_to_rust(instance_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid instance_id: error {}", e),
            )
        }
    };
    let patch_str = match c_str_to_rust(patch_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid patch_json: error {}", e),
            )
        }
    };
    let patch = match serde_json::from_str::<StarMapEmbedPatchInputDto>(&patch_str) {
        Ok(p) => p,
        Err(e) => return err_json("PARSE_ERROR", &format!("Failed to parse patch_json: {}", e)),
    };
    match with_app_service(|svc| {
        let embed = svc
            .update_starmap_embed(sid, iid, patch)
            .map_err(|e| format!("{}", e))?;
        Ok(embed)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `root_starmap_id` and `segments_json` both point to valid,
/// null-terminated C strings. Passing null pointers or invalid pointers is
/// undefined behavior.
pub unsafe extern "C" fn writer_core_resolve_starmap_path(
    root_starmap_id: *const c_char,
    segments_json: *const c_char,
) -> *mut c_char {
    let root = match c_str_to_rust(root_starmap_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid root_starmap_id: error {}", e),
            )
        }
    };
    let segments_str = match c_str_to_rust(segments_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid segments_json: error {}", e),
            )
        }
    };
    let segments = match serde_json::from_str::<Vec<StarMapPathSegmentDto>>(&segments_str) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "PARSE_ERROR",
                &format!("Failed to parse segments_json: {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let resolved = svc
            .resolve_starmap_path(&root, segments)
            .map_err(|e| format!("{}", e))?;
        Ok(resolved)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_RESOLVE_ERROR", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `json` points to a valid, null-terminated C string.
/// Passing a null pointer or an invalid pointer is undefined behavior.
pub unsafe extern "C" fn writer_core_resolve_starmap_navigation(
    json: *const c_char,
) -> *mut c_char {
    let input_str = match c_str_to_rust(json) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid json: error {}", e)),
    };
    let route = match serde_json::from_str::<StarMapNavigationRouteDto>(&input_str) {
        Ok(r) => r,
        Err(e) => return err_json("PARSE_ERROR", &format!("Failed to parse json: {}", e)),
    };
    match with_app_service(|svc| {
        let location = svc
            .resolve_starmap_navigation(route.root_starmap_id, route.segments)
            .map_err(|e| format!("{}", e))?;
        Ok(location)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NAVIGATION_ERROR", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `json` points to a valid, null-terminated C string.
/// Passing a null pointer or an invalid pointer is undefined behavior.
pub unsafe extern "C" fn writer_core_enter_starmap_embed(json: *const c_char) -> *mut c_char {
    let input_str = match c_str_to_rust(json) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid json: error {}", e)),
    };
    let input = match serde_json::from_str::<EnterEmbedInput>(&input_str) {
        Ok(i) => i,
        Err(e) => return err_json("PARSE_ERROR", &format!("Failed to parse json: {}", e)),
    };
    match with_app_service(|svc| {
        let result = svc
            .enter_starmap_embed(input.root_starmap_id, input.segments, input.instance_id)
            .map_err(|e| format!("{}", e))?;
        Ok(result)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NAVIGATION_ERROR", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `json` points to a valid, null-terminated C string.
/// Passing a null pointer or an invalid pointer is undefined behavior.
pub unsafe extern "C" fn writer_core_enter_starmap_portal(json: *const c_char) -> *mut c_char {
    let input_str = match c_str_to_rust(json) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid json: error {}", e)),
    };
    let input = match serde_json::from_str::<EnterPortalInput>(&input_str) {
        Ok(i) => i,
        Err(e) => return err_json("PARSE_ERROR", &format!("Failed to parse json: {}", e)),
    };
    match with_app_service(|svc| {
        let result = svc
            .enter_starmap_portal(input.root_starmap_id, input.segments, input.node_id)
            .map_err(|e| format!("{}", e))?;
        Ok(result)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NAVIGATION_ERROR", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `json` points to a valid, null-terminated C string.
/// Passing a null pointer or an invalid pointer is undefined behavior.
pub unsafe extern "C" fn writer_core_create_cross_canvas_edge(json: *const c_char) -> *mut c_char {
    let input_str = match c_str_to_rust(json) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid json: error {}", e)),
    };
    let input = match serde_json::from_str::<CrossCanvasEdgeInput>(&input_str) {
        Ok(i) => i,
        Err(e) => return err_json("PARSE_ERROR", &format!("Failed to parse json: {}", e)),
    };
    match with_app_service(|svc| {
        let result = svc
            .create_cross_canvas_edge(input.from, input.to, input.kind, input.label, input.payload)
            .map_err(|e| format!("{}", e))?;
        Ok(result)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_CONNECTION_ERROR", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `json` points to a valid, null-terminated C string.
/// Passing a null pointer or an invalid pointer is undefined behavior.
pub unsafe extern "C" fn writer_core_list_starmap_connections(json: *const c_char) -> *mut c_char {
    let input_str = match c_str_to_rust(json) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid json: error {}", e)),
    };
    let input = match serde_json::from_str::<ListConnectionsInput>(&input_str) {
        Ok(i) => i,
        Err(e) => return err_json("PARSE_ERROR", &format!("Failed to parse json: {}", e)),
    };
    match with_app_service(|svc| {
        let connections = svc
            .list_starmap_connections(input.route, input.target)
            .map_err(|e| format!("{}", e))?;
        Ok(connections)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_CONNECTION_ERROR", &e),
    }
}

// --- FFI 内部输入 DTO ---
// 统一采用 camelCase 字段命名，与 route DTO（StarMapNavigationRouteDto 等）一致。

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnterEmbedInput {
    root_starmap_id: String,
    #[serde(default)]
    segments: Vec<StarMapPathSegmentDto>,
    instance_id: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnterPortalInput {
    root_starmap_id: String,
    #[serde(default)]
    segments: Vec<StarMapPathSegmentDto>,
    node_id: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CrossCanvasEdgeInput {
    from: StarMapEndpointSelectionDto,
    to: StarMapEndpointSelectionDto,
    kind: crate::api::types::StarMapEdgeKindDto,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    payload: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListConnectionsInput {
    route: StarMapNavigationRouteDto,
    #[serde(default)]
    target: Option<StarMapTargetDetailDto>,
}
