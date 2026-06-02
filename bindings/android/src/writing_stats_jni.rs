use super::*;

// Record Writing Event
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_recordWritingEventNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    device_id_j: JString,
    project_id_j: JString,
    volume_id_j: JString,
    chapter_id_j: JString,
    source_j: JString,
    inserted_chars: jni::sys::jint,
    deleted_chars: jni::sys::jint,
    pasted_chars: jni::sys::jint,
    ai_inserted_chars: jni::sys::jint,
    session_id_j: JString,
) -> jboolean {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let device_id = match jstring_to_string(&mut env, &device_id_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let volume_id = match jstring_to_string(&mut env, &volume_id_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let chapter_id = match jstring_to_string(&mut env, &chapter_id_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let source = match jstring_to_string(&mut env, &source_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let session_id = match jstring_to_string(&mut env, &session_id_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let api = api_from_workspace(&workspace_path);
    match api.record_writing_event(
        &device_id,
        &project_id,
        &volume_id,
        &chapter_id,
        &source,
        inserted_chars as i32,
        deleted_chars as i32,
        pasted_chars as i32,
        ai_inserted_chars as i32,
        &session_id,
    ) {
        Ok(_) => 1,
        Err(_) => 0,
    }
}

// Flush Writing Stats
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_flushWritingStatsNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
) {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return,
    };

    let api = api_from_workspace(&workspace_path);
    let _ = api.flush_writing_stats();
}

// Get Writing Stats Summary
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_getWritingStatsSummaryNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    start_date_j: JString,
    end_date_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let start_date = match jstring_to_string(&mut env, &start_date_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let end_date = match jstring_to_string(&mut env, &end_date_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(
        &mut env,
        api.get_writing_stats_summary(&start_date, &end_date),
    )
}

// --- Writing Stats: By Project / Chapter / Device / Speed Curve ---

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_getWritingStatsByProjectNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    start_date_j: JString,
    end_date_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let start_date = match jstring_to_string(&mut env, &start_date_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let end_date = match jstring_to_string(&mut env, &end_date_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let api = api_from_workspace(&workspace_path);
    result_to_jstring(
        &mut env,
        api.get_writing_stats_by_project(&start_date, &end_date),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_getWritingStatsByChapterNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    start_date_j: JString,
    end_date_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let start_date = match jstring_to_string(&mut env, &start_date_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let end_date = match jstring_to_string(&mut env, &end_date_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let api = api_from_workspace(&workspace_path);
    result_to_jstring(
        &mut env,
        api.get_writing_stats_by_chapter(&start_date, &end_date),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_getWritingStatsByDeviceNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    start_date_j: JString,
    end_date_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let start_date = match jstring_to_string(&mut env, &start_date_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let end_date = match jstring_to_string(&mut env, &end_date_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let api = api_from_workspace(&workspace_path);
    result_to_jstring(
        &mut env,
        api.get_writing_stats_by_device(&start_date, &end_date),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_getWritingSpeedCurveNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    start_date_j: JString,
    end_date_j: JString,
    bucket_minutes: jni::sys::jint,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let start_date = match jstring_to_string(&mut env, &start_date_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let end_date = match jstring_to_string(&mut env, &end_date_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let api = api_from_workspace(&workspace_path);
    result_to_jstring(
        &mut env,
        api.get_writing_speed_curve(&start_date, &end_date, bucket_minutes as u32),
    )
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_calculateWordCountNative(
    mut env: JNIEnv,
    _this: JObject,
    text_j: JString,
) -> jni::sys::jint {
    let text = match jstring_to_string(&mut env, &text_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    writer_core::chapter::calculate_word_count(&text) as jni::sys::jint
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_processWritingEventNative(
    mut env: JNIEnv,
    _this: JObject,
    workspace_path_j: JString,
    device_id_j: JString,
    platform_j: JString,
    project_id_j: JString,
    volume_id_j: JString,
    chapter_id_j: JString,
    old_text_j: JString,
    new_text_j: JString,
    session_id_j: JString,
) -> jni::sys::jboolean {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let device_id = match jstring_to_string(&mut env, &device_id_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let platform = match jstring_to_string(&mut env, &platform_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let volume_id = match jstring_to_string(&mut env, &volume_id_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let chapter_id = match jstring_to_string(&mut env, &chapter_id_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let old_text = match jstring_to_string(&mut env, &old_text_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let new_text = match jstring_to_string(&mut env, &new_text_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let session_id = match jstring_to_string(&mut env, &session_id_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let api = api_from_workspace(&workspace_path);
    match api.process_writing_event(
        &device_id,
        &platform,
        &project_id,
        &volume_id,
        &chapter_id,
        &old_text,
        &new_text,
        &session_id,
    ) {
        Ok(_) => 1,
        Err(_) => 0,
    }
}
