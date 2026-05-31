use super::*;

// Perform Sync Diagnostics
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_performSyncDiagnostics(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    config_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let config_json = match jstring_to_string(&mut env, &config_json_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let config = match serde_json::from_str::<writer_core::api::types::SyncConfigDto>(&config_json) {
        Ok(c) => c,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Json(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    let result = api.perform_sync_diagnostics(config);
    result_to_jstring(&mut env, result)
}

// Perform Sync Dry Run
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_performSyncDryRun(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    config_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let config_json = match jstring_to_string(&mut env, &config_json_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let config = match serde_json::from_str::<writer_core::api::types::SyncConfigDto>(&config_json) {
        Ok(c) => c,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Json(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.perform_sync_dry_run(config))
}

// Perform Sync
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_performSync(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    config_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let config_json = match jstring_to_string(&mut env, &config_json_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let config = match serde_json::from_str::<writer_core::api::types::SyncConfigDto>(&config_json) {
        Ok(c) => c,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Json(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.perform_sync(config))
}
