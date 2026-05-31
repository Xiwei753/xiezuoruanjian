use super::*;

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_getMindMapSnapshotJsonNative(
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
    let result = api.get_mind_map_snapshot(&project_id);
    result_to_jstring(&mut env, result)
}


#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_createMindMapGraphJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    title_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let title = match jstring_to_string(&mut env, &title_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    let result = api.create_mind_map_graph(&project_id, &title);
    result_to_jstring(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_listMindMapGraphsJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    let result = api.list_mind_map_graphs(&project_id);
    result_to_jstring(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_setDefaultMindMapGraphJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    graph_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    let result = api.set_default_mind_map_graph(&project_id, &graph_id);
    result_to_jstring(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_createMindMapNodeJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    graph_id_j: JString,
    node_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let node_json = match jstring_to_string(&mut env, &node_json_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };

    let node = match serde_json::from_str(&node_json) {
        Ok(n) => n,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Json(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    let result = api.create_mind_map_node(&project_id, &graph_id, node);
    result_to_jstring(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_updateMindMapNodeJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    graph_id_j: JString,
    node_id_j: JString,
    patch_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let node_id = match jstring_to_string(&mut env, &node_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let patch_json = match jstring_to_string(&mut env, &patch_json_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };

    let patch = match serde_json::from_str(&patch_json) {
        Ok(p) => p,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Json(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    let result = api.update_mind_map_node(&project_id, &graph_id, &node_id, patch);
    result_to_jstring(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_deleteMindMapNodeJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    graph_id_j: JString,
    node_id_j: JString,
    cascade_j: jboolean,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let node_id = match jstring_to_string(&mut env, &node_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };

    let cascade = cascade_j != 0;
    let api = api_from_workspace(&workspace_path);
    let result = api.delete_mind_map_node(&project_id, &graph_id, &node_id, cascade);
    result_to_jstring(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_createMindMapEdgeJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    graph_id_j: JString,
    edge_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let edge_json = match jstring_to_string(&mut env, &edge_json_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };

    let edge = match serde_json::from_str(&edge_json) {
        Ok(ed) => ed,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Json(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    let result = api.create_mind_map_edge(&project_id, &graph_id, edge);
    result_to_jstring(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_updateMindMapEdgeJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    graph_id_j: JString,
    edge_id_j: JString,
    patch_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let edge_id = match jstring_to_string(&mut env, &edge_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let patch_json = match jstring_to_string(&mut env, &patch_json_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };

    let patch = match serde_json::from_str(&patch_json) {
        Ok(p) => p,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Json(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    let result = api.update_mind_map_edge(&project_id, &graph_id, &edge_id, patch);
    result_to_jstring(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_deleteMindMapEdgeJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    graph_id_j: JString,
    edge_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let edge_id = match jstring_to_string(&mut env, &edge_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    let result = api.delete_mind_map_edge(&project_id, &graph_id, &edge_id);
    result_to_jstring(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_createMindMapAnchorJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    graph_id_j: JString,
    anchor_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let anchor_json = match jstring_to_string(&mut env, &anchor_json_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };

    let anchor = match serde_json::from_str(&anchor_json) {
        Ok(an) => an,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Json(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    let result = api.create_mind_map_anchor(&project_id, &graph_id, anchor);
    result_to_jstring(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_bindMindMapAnchorJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    graph_id_j: JString,
    node_id_j: JString,
    anchor_id_j: JString,
    link_kind_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let node_id = match jstring_to_string(&mut env, &node_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let anchor_id = match jstring_to_string(&mut env, &anchor_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let link_kind = match jstring_to_string(&mut env, &link_kind_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    let result = api.bind_mind_map_node_to_anchor(&project_id, &graph_id, &node_id, &anchor_id, &link_kind);
    result_to_jstring(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_saveMindMapLayoutJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    graph_id_j: JString,
    layout_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };
    let layout_json = match jstring_to_string(&mut env, &layout_json_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Io(e.to_string()))),
    };

    let layout = match serde_json::from_str(&layout_json) {
        Ok(la) => la,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Json(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    let result = api.save_mind_map_layout(&project_id, &graph_id, layout);
    result_to_jstring(&mut env, result)
}
