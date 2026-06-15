use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, with_core};

#[no_mangle]
pub unsafe extern "C" fn writer_core_list_starmaps() -> *mut c_char {
    match with_core(|core| {
        let starmaps = core.list_starmaps().map_err(|e| format!("{}", e))?;
        let json_arr: Vec<serde_json::Value> = starmaps.iter().map(|sm| {
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
        }).collect();
        Ok(json_arr)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_list_starmaps_for_project(project_id: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    match with_core(|core| {
        let starmaps = core.list_starmaps_for_project(&pid).map_err(|e| format!("{}", e))?;
        let json_arr: Vec<serde_json::Value> = starmaps.iter().map(|sm| {
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
        }).collect();
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
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid starmap_id: error {}", e)),
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
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid starmap_id: error {}", e)),
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
pub unsafe extern "C" fn writer_core_create_starmap(title: *const c_char, description: *const c_char) -> *mut c_char {
    let t = match c_str_to_rust(title) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid title: error {}", e)),
    };
    let d = match c_str_to_rust(description) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid description: error {}", e)),
    };
    match with_core(|core| {
        let sm = core.create_starmap(&t, &d, None).map_err(|e| format!("{}", e))?;
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
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid starmap_id: error {}", e)),
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
pub unsafe extern "C" fn writer_core_rename_starmap(starmap_id: *const c_char, new_title: *const c_char) -> *mut c_char {
    let sid = match c_str_to_rust(starmap_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid starmap_id: error {}", e)),
    };
    let t = match c_str_to_rust(new_title) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid new_title: error {}", e)),
    };
    match with_core(|core| {
        let sm = core.rename_starmap(&sid, &t).map_err(|e| format!("{}", e))?;
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