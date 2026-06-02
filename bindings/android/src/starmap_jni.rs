use super::*;

// StarMap Endpoints
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_listStarmapsJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.list_starmaps())
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_createStarmapJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    title_j: JString,
    desc_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let title = match jstring_to_string(&mut env, &title_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let desc = match jstring_to_string(&mut env, &desc_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.create_starmap(&title, &desc, None))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_getStarmapGraphJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let api = api_from_workspace(&workspace_path);
    let graph_res = api.get_starmap_graph(&starmap_id);
    let layout_res = api.get_starmap_layout(&starmap_id);
    match (graph_res, layout_res) {
        (Ok(graph), Ok(layout)) => {
            let data = serde_json::json!({ "graph": graph, "layout": layout });
            result_to_jstring(&mut env, Ok(data))
        }
        (Err(e), _) => result_to_jstring::<()>(&mut env, Err(e)),
        (_, Err(e)) => result_to_jstring::<()>(&mut env, Err(e)),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_addStarmapNodeJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
    node_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let node_json = match jstring_to_string(&mut env, &node_json_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let api = api_from_workspace(&workspace_path);
    // Currently Android Canvas drops node anywhere, default it to 0,0 since canvas isn't tracking click coordinates here yet.
    // The core will default it and save to the layout.
    match serde_json::from_str(&node_json) {
        Ok(node) => result_to_jstring(&mut env, api.add_starmap_node(&starmap_id, node, 0.0, 0.0)),
        Err(e) => result_to_jstring::<()>(
            &mut env,
            Err(writer_core::api::error::WriterError::Io(e.to_string())),
        ),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_saveStarmapLayoutJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
    layout_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let layout_json = match jstring_to_string(&mut env, &layout_json_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let api = api_from_workspace(&workspace_path);
    match serde_json::from_str(&layout_json) {
        Ok(layout) => result_to_jstring(&mut env, api.save_starmap_layout(&starmap_id, &layout)),
        Err(e) => result_to_jstring::<()>(
            &mut env,
            Err(writer_core::api::error::WriterError::Io(e.to_string())),
        ),
    }
}

// --- StarMap: Rename / Delete / Bind ---

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_renameStarmapNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
    new_title_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let new_title = match jstring_to_string(&mut env, &new_title_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let api = api_from_workspace(&workspace_path);
    result_to_jstring(
        &mut env,
        api.rename_starmap(&starmap_id, &new_title).map(|_| ()),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_deleteStarmapNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.delete_starmap(&starmap_id))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_bindStarmapToProjectJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
    project_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let api = api_from_workspace(&workspace_path);
    result_to_jstring(
        &mut env,
        api.bind_starmap_to_project(&starmap_id, &project_id),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_unbindStarmapFromProjectJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.unbind_starmap_from_project(&starmap_id))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_setMainStarmapForProjectJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
    project_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let api = api_from_workspace(&workspace_path);
    result_to_jstring(
        &mut env,
        api.set_main_starmap_for_project(&starmap_id, &project_id),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_getMainStarmapForProjectJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.get_main_starmap_for_project(&project_id))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_createChildStarmapJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    parent_id_j: JString,
    title_j: JString,
    desc_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let parent_id = match jstring_to_string(&mut env, &parent_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let title = match jstring_to_string(&mut env, &title_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let desc = match jstring_to_string(&mut env, &desc_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let api = api_from_workspace(&workspace_path);
    result_to_jstring(
        &mut env,
        api.create_child_starmap_legacy(&parent_id, &title, &desc, None),
    )
}

// --- StarMap: Node CRUD ---

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_updateStarmapNodeJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
    node_id_j: JString,
    patch_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let node_id = match jstring_to_string(&mut env, &node_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let patch_json = match jstring_to_string(&mut env, &patch_json_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let patch: writer_core::api::types::StarMapNodePatchDto =
        match serde_json::from_str(&patch_json) {
            Ok(p) => p,
            Err(e) => {
                return result_to_jstring::<()>(
                    &mut env,
                    Err(writer_core::api::error::WriterError::Json(e.to_string())),
                )
            }
        };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(
        &mut env,
        api.update_starmap_node(&starmap_id, &node_id, patch),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_deleteStarmapNodeJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
    node_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let node_id = match jstring_to_string(&mut env, &node_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.delete_starmap_node(&starmap_id, &node_id))
}

// --- StarMap: Edge CRUD ---

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_addStarmapEdgeJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
    edge_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let edge_json = match jstring_to_string(&mut env, &edge_json_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let edge = match serde_json::from_str(&edge_json) {
        Ok(e) => e,
        Err(e) => {
            return result_to_jstring::<()>(
                &mut env,
                Err(writer_core::api::error::WriterError::Json(e.to_string())),
            )
        }
    };
    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.add_starmap_edge(&starmap_id, edge))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_updateStarmapEdgeJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
    edge_id_j: JString,
    patch_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let edge_id = match jstring_to_string(&mut env, &edge_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let patch_json = match jstring_to_string(&mut env, &patch_json_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let patch: writer_core::api::types::StarMapEdgePatchDto =
        match serde_json::from_str(&patch_json) {
            Ok(p) => p,
            Err(e) => {
                return result_to_jstring::<()>(
                    &mut env,
                    Err(writer_core::api::error::WriterError::Json(e.to_string())),
                )
            }
        };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(
        &mut env,
        api.update_starmap_edge(&starmap_id, &edge_id, patch),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_deleteStarmapEdgeJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
    edge_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let edge_id = match jstring_to_string(&mut env, &edge_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.delete_starmap_edge(&starmap_id, &edge_id))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_saveStarmapGraphJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
    graph_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let graph_json = match jstring_to_string(&mut env, &graph_json_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let graph = match serde_json::from_str(&graph_json) {
        Ok(g) => g,
        Err(e) => {
            return result_to_jstring::<()>(
                &mut env,
                Err(writer_core::api::error::WriterError::Json(e.to_string())),
            )
        }
    };
    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.save_starmap_graph(&starmap_id, &graph))
}
