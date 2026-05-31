use super::*;

// Load Local Settings
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_loadLocalSettings(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.load_local_settings())
}

// Save Local Settings
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_saveLocalSettings(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    settings_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let settings_json = match jstring_to_string(&mut env, &settings_json_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let settings = match serde_json::from_str::<writer_core::api::types::LocalSettingsDto>(&settings_json) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Json(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    string_to_jstring(&mut env, api.save_local_settings_envelope_json(settings))
}

// Load Syncable Settings
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_loadSyncableSettings(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.load_syncable_settings())
}

// Save Syncable Settings
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_saveSyncableSettings(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    settings_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let settings_json = match jstring_to_string(&mut env, &settings_json_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let settings = match serde_json::from_str::<writer_core::api::types::SyncableSettingsDto>(&settings_json) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Json(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    string_to_jstring(&mut env, api.save_syncable_settings_envelope_json(settings))
}

// --- Sync Config / Secrets / State ---

// Load Sync Config
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_loadSyncConfig(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.load_sync_config())
}

// Save Sync Config
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_saveSyncConfig(
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
    result_to_jstring(&mut env, api.save_sync_config(config))
}

// Load Sync Secrets
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_loadSyncSecrets(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.load_sync_secrets())
}

// Save Sync Secrets
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_saveSyncSecrets(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    secrets_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let secrets_json = match jstring_to_string(&mut env, &secrets_json_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let secrets = match serde_json::from_str::<writer_core::api::types::SyncSecretsDto>(&secrets_json) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::api::error::WriterError::Json(e.to_string()))),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.save_sync_secrets(secrets))
}

// Load Sync State
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_loadSyncState(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let api = api_from_workspace(&workspace_path);
    result_to_jstring(&mut env, api.load_sync_state())
}
