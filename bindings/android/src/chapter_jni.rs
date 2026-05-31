use super::*;

// List Volumes
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_listVolumes(
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
    result_to_jstring(&mut env, api.list_volumes(&project_id))
}

// Create Volume
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_createVolume(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    title_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let title = match jstring_to_string(&mut env, &title_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.create_volume(&project_id, &title))
}

// List Chapters
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_listChapters(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    volume_id_j: JString,
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

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.list_chapters(&project_id, &volume_id))
}

// Create Chapter
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_createChapter(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    volume_id_j: JString,
    title_j: JString,
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
    let title = match jstring_to_string(&mut env, &title_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(
        &mut env,
        api.create_chapter(&project_id, &volume_id, &title),
    )
}

// Read Chapter Content
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_readChapter(
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
        api.open_chapter(&project_id, &volume_id, &chapter_id),
    )
}

// Write Chapter Content
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_writeChapter(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    volume_id_j: JString,
    chapter_id_j: JString,
    content_j: JString,
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
    let content = match jstring_to_string(&mut env, &content_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(
        &mut env,
        api.save_chapter_content(&project_id, &volume_id, &chapter_id, &content),
    )
}

// Explicitly Clear Chapter Content
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_clearChapterContentNative(
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
        api.clear_chapter_content(&project_id, &volume_id, &chapter_id),
    )
}

// Update Chapter Note
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_updateChapterNote(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    volume_id_j: JString,
    chapter_id_j: JString,
    note_j: JString,
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
    let note = match jstring_to_string(&mut env, &note_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(
        &mut env,
        api.update_chapter_note(&project_id, &volume_id, &chapter_id, &note),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_renameVolumeNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    volume_id_j: JString,
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
    let volume_id = match jstring_to_string(&mut env, &volume_id_j) {
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
        api.rename_volume(&project_id, &volume_id, &new_title),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_deleteVolumeNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    volume_id_j: JString,
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

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.delete_volume(&project_id, &volume_id))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_reorderVolumesNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    ordered_ids_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let ordered_ids_json = match jstring_to_string(&mut env, &ordered_ids_json_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let ordered_ids: Vec<String> = match serde_json::from_str(&ordered_ids_json) {
        Ok(ids) => ids,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Json(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.reorder_volumes(&project_id, &ordered_ids))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_renameChapterNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    volume_id_j: JString,
    chapter_id_j: JString,
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
    let volume_id = match jstring_to_string(&mut env, &volume_id_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let chapter_id = match jstring_to_string(&mut env, &chapter_id_j) {
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
        api.rename_chapter(&project_id, &volume_id, &chapter_id, &new_title),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_deleteChapterNative(
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
        api.delete_chapter(&project_id, &volume_id, &chapter_id),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_reorderChaptersNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
    volume_id_j: JString,
    ordered_ids_json_j: JString,
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
    let ordered_ids_json = match jstring_to_string(&mut env, &ordered_ids_json_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let ordered_ids: Vec<String> = match serde_json::from_str(&ordered_ids_json) {
        Ok(ids) => ids,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Json(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(
        &mut env,
        api.reorder_chapters(&project_id, &volume_id, &ordered_ids),
    )
}
