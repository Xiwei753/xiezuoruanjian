use super::*;

// List Projects
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_listProjects(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.list_projects())
}

// Get Recent Edits
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_getRecentEdits(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.get_recent_edits())
}

// Record Recent Edit
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_recordRecentEdit(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    volume_id_j: JString,
    chapter_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let volume_id = match jstring_to_string(&mut env, &volume_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let chapter_id = match jstring_to_string(&mut env, &chapter_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(
        &mut env,
        api.record_recent_edit(&project_id, &volume_id, &chapter_id),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_getProjectStats(
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
    result_to_jstring(&mut env, api.get_project_stats(&project_id))
}

// Create Project
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_createProject(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    title_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let title = match jstring_to_string(&mut env, &title_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.create_project(&title))
}

// --- Renaming, Deleting, Reordering ---

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_renameProjectNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    new_title_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let new_title = match jstring_to_string(&mut env, &new_title_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.rename_project(&project_id, &new_title))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_deleteProjectNative(
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
    result_to_jstring(&mut env, api.delete_project(&project_id))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_reorderProjectsNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    ordered_ids_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let ordered_ids_json = match jstring_to_string(&mut env, &ordered_ids_json_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let ordered_ids: Vec<String> = match serde_json::from_str(&ordered_ids_json) {
        Ok(ids) => ids,
        Err(e) => {
            return result_to_jstring::<()>(
                &mut env,
                Err(writer_core::api::error::WriterError::Json(e.to_string())),
            )
        }
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.reorder_projects(&ordered_ids))
}
